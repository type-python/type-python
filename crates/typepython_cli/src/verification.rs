use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode, Output},
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use rayon::prelude::*;
use regex::Regex;
use ruff_python_ast::{Expr, Stmt};
use ruff_python_parser::parse_module;
use tar::Archive as TarArchive;
use typepython_config::ConfigHandle;
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Severity};
use typepython_emit::{EmitArtifact, TypePythonStubContext, generate_typepython_stub_source};
use typepython_incremental::decode_snapshot;
use typepython_lowering::{BackportRequirement, LoweredModule};
use typepython_syntax::{SourceFile, SourceKind};
use typepython_target::{PythonTarget, RuntimeFeature};
use zip::ZipArchive;

use crate::api_diff::{ApiSurfaceDiffReport, api_surface_diff_diagnostics, diff_api_surfaces};
use crate::archive::{ArchiveMemberPaths, ArchivePathKind, validate_wheel_record_path};
use crate::cli::{OutputFormat, VerifyArgs};
use crate::discovery::normalize_glob_path;
use crate::pipeline::{
    build_diagnostics, ensure_output_dirs, materialize_build_outputs,
    persist_pipeline_analysis_state, py_typed_package_roots, run_pipeline,
    runtime_write_diagnostic, should_emit_build_outputs,
};
use crate::type_health::{TypeHealthReport, build_type_health_report_for_target};
use crate::{
    CLI_JSON_SCHEMA_VERSION, CommandSummary, RUNTIME_IMPORTABILITY_SCRIPT, bytecode_path_for,
    exit_code, load_project, load_project_without_python_executable_validation, print_summary,
    resolve_python_executable,
};

#[derive(Debug, serde::Deserialize)]
struct RuntimeImportabilityResult {
    importable: bool,
    error: Option<String>,
    public_names: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize)]
struct AnnotationRuntimeAuditResult {
    consumers: Vec<String>,
    findings: Vec<AnnotationRuntimeAuditFinding>,
    #[serde(default)]
    parse_error: Option<AnnotationRuntimeAuditParseError>,
}

#[derive(Debug, serde::Deserialize)]
struct AnnotationRuntimeAuditFinding {
    code: String,
    message: String,
    line: usize,
    column: usize,
}

#[derive(Debug, serde::Deserialize)]
struct AnnotationRuntimeAuditParseError {
    host_python: String,
    target_python: String,
    message: String,
    line: Option<usize>,
    column: Option<usize>,
    host_older_than_target: bool,
}

const ANNOTATION_RUNTIME_AUDIT_SCRIPT: &str = r#"
import ast
import json
import pathlib
import sys

source_path = pathlib.Path(sys.argv[1])
target_text = sys.argv[2]
target_version = tuple(int(part) for part in target_text.split(".", 1))
host_version = sys.version_info[:2]
source = source_path.read_text(encoding="utf-8")
from typepython.annotation_compat import audit_source

try:
    ast.parse(
        source,
        filename=str(source_path),
        feature_version=target_version if target_version <= host_version else None,
    )
except SyntaxError as error:
    payload = {
        "consumers": [],
        "findings": [],
        "parse_error": {
            "host_python": f"{host_version[0]}.{host_version[1]}",
            "target_python": target_text,
            "message": error.msg,
            "line": error.lineno,
            "column": error.offset,
            "host_older_than_target": host_version < target_version,
        },
    }
    print(json.dumps(payload))
    raise SystemExit(0)

audit = audit_source(source, filename=str(source_path))
payload = {
    "consumers": [consumer.value for consumer in audit.consumers],
    "findings": [
        {
            "code": finding.code,
            "message": finding.message,
            "line": finding.line,
            "column": finding.column,
        }
        for finding in audit.findings
    ],
    "parse_error": None,
}
print(json.dumps(payload))
"#;

#[derive(Debug, Default, Clone, Eq, PartialEq)]
struct PublicationRequirements {
    min_python: Option<PythonTarget>,
    needs_typing_extensions: bool,
}

#[derive(Debug, Default, Clone, Eq, PartialEq)]
struct PackageMetadata {
    requires_python: Option<String>,
    requires_dist: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct PyProjectMetadata {
    project: Option<PyProjectProjectMetadata>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct CheckerAllowlist {
    #[serde(default)]
    disagreements: Vec<CheckerAllowlistEntry>,
}

#[derive(Debug, Clone, serde::Deserialize, Eq, PartialEq)]
pub(crate) struct CheckerAllowlistEntry {
    pub(crate) checker: String,
    pub(crate) contains: String,
    pub(crate) reason: String,
    pub(crate) issue: Option<String>,
    pub(crate) expires: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize)]
pub(crate) struct TypePortabilityReport {
    pub(crate) score: usize,
    pub(crate) passing_checkers: usize,
    pub(crate) total_checkers: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize)]
pub(crate) struct Pep561ReadinessReport {
    pub(crate) ready: bool,
    pub(crate) blocking_issues: Vec<String>,
    pub(crate) advisories: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct PyProjectProjectMetadata {
    name: Option<String>,
    version: Option<String>,
    #[serde(rename = "requires-python")]
    requires_python: Option<String>,
    dependencies: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub(crate) struct SuppliedVerifyArtifact {
    pub(crate) kind: SuppliedArtifactKind,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SuppliedArtifactKind {
    Wheel,
    Sdist,
}

impl SuppliedArtifactKind {
    fn label(self) -> &'static str {
        match self {
            Self::Wheel => "wheel",
            Self::Sdist => "sdist",
        }
    }
}

pub(crate) fn supplied_verify_artifacts(args: &VerifyArgs) -> Vec<SuppliedVerifyArtifact> {
    let mut artifacts = args
        .wheels
        .iter()
        .cloned()
        .map(|path| SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path })
        .collect::<Vec<_>>();
    artifacts.extend(
        args.sdists
            .iter()
            .cloned()
            .map(|path| SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path }),
    );
    artifacts
}

pub(crate) fn run_verify(args: VerifyArgs) -> Result<ExitCode> {
    run_verify_with_command("verify", args)
}

pub(crate) fn run_verify_with_command(command_name: &str, args: VerifyArgs) -> Result<ExitCode> {
    let mut config = if args.unsafe_runtime_imports {
        load_project(args.run.project.as_ref())?
    } else {
        load_project_without_python_executable_validation(args.run.project.as_ref())?
    };
    let safe_verify_ignored_python_executable =
        !args.unsafe_runtime_imports && config.config.resolution.python_executable.is_some();
    if !args.unsafe_runtime_imports {
        config.config.resolution.python_executable = None;
    }
    ensure_output_dirs(&config)?;
    let snapshot = run_pipeline(&config)?;
    let _ = persist_pipeline_analysis_state(&config, &snapshot)?;
    let mut notes = vec![String::from(
        "verifies current runtime artifacts, emitted stubs, and `py.typed` in the build tree",
    )];
    let mut diagnostics = build_diagnostics(&config, &snapshot);
    if should_emit_build_outputs(&config, &snapshot) {
        match materialize_build_outputs(&config, &snapshot) {
            Ok(materialize_notes) => notes.extend(materialize_notes),
            Err(error) => {
                if let Some(diagnostic) = runtime_write_diagnostic(&error) {
                    diagnostics.push(diagnostic);
                } else {
                    return Err(error).with_context(|| {
                        format!(
                            "unable to write runtime artifacts under {}",
                            config.resolve_relative_path(&config.config.project.out_dir).display()
                        )
                    });
                }
            }
        }
    }
    if !snapshot.diagnostics.has_errors() && !diagnostics.has_errors() {
        diagnostics = verify_build_artifacts(&config, &snapshot.emit_plan);
    }
    if !snapshot.diagnostics.has_errors() && !diagnostics.has_errors() {
        if args.unsafe_runtime_imports {
            diagnostics.diagnostics.extend(
                verify_runtime_public_name_parity(&config, &snapshot.emit_plan).diagnostics,
            );
            notes.push(String::from(
                "imported emitted runtime modules to compare runtime-visible public names",
            ));
        } else {
            notes.push(String::from(
                "skipped runtime import probes; pass --unsafe-runtime-imports to execute emitted modules during verification",
            ));
            if safe_verify_ignored_python_executable {
                notes.push(String::from(
                    "ignored configured resolution.python_executable in safe verify mode; rerun with --unsafe-runtime-imports to verify against that interpreter environment",
                ));
            }
        }
    }
    if !snapshot.diagnostics.has_errors() && !diagnostics.has_errors() {
        diagnostics.diagnostics.extend(
            verify_packaged_artifacts(
                &config,
                &snapshot.emit_plan,
                &supplied_verify_artifacts(&args),
            )
            .diagnostics,
        );
    }
    if !snapshot.diagnostics.has_errors() && !diagnostics.has_errors() {
        diagnostics.diagnostics.extend(
            verify_publication_metadata(
                &config,
                &snapshot.emit_plan,
                None,
                &supplied_verify_artifacts(&args),
            )
            .diagnostics,
        );
    }
    let mut api_diff_report = None;
    if !snapshot.diagnostics.has_errors()
        && !diagnostics.has_errors()
        && let Some(old_surface) = args.api_diff_old.as_deref()
    {
        let current_surface = config.resolve_relative_path(&config.config.project.out_dir);
        let report = diff_api_surfaces(old_surface, &current_surface)?;
        diagnostics.diagnostics.extend(api_surface_diff_diagnostics(&report).diagnostics);
        notes.push(format!(
            "compared previous public API surface `{}` against current build output",
            old_surface.display()
        ));
        api_diff_report = Some(report);
    }
    let mut type_health_report = None;
    if !snapshot.diagnostics.has_errors()
        && !diagnostics.has_errors()
        && args.publication_type_health
    {
        let report = build_type_health_report_for_target(
            &config.config_dir,
            &config.config.resolution.type_roots,
            config.config.project.target_python,
        )?;
        diagnostics.diagnostics.extend(publication_type_health_diagnostics(&report).diagnostics);
        notes.push(format!("publication type-health score: {}/100", report.score));
        type_health_report = Some(report);
    }
    let checkers = verify_checker_invocations(&args)?;
    let checker_allowlist = load_checker_allowlist(&config, args.checker_allowlist.as_deref())?;
    if !snapshot.diagnostics.has_errors() && !diagnostics.has_errors() {
        diagnostics
            .diagnostics
            .extend(verify_external_checkers(&config, &checkers, &checker_allowlist).diagnostics);
    }

    let supplied_artifact_count = args.wheels.len() + args.sdists.len();
    if supplied_artifact_count > 0 {
        notes.push(format!(
            "verified {} supplied wheel/sdist artifact(s) against the authoritative build tree",
            supplied_artifact_count
        ));
    }
    let portability_report = if checkers.is_empty() {
        None
    } else {
        Some(type_portability_report(&diagnostics, checkers.len()))
    };
    let pep561_report = pep561_readiness_report(&config, &snapshot.emit_plan, &diagnostics);
    if portability_report.is_some() {
        notes.push(format!(
            "ran {} external checker invocation(s) against the emitted build output",
            checkers.len()
        ));
        notes.push(format!(
            "type portability score: {}",
            type_portability_score(&diagnostics, checkers.len())
        ));
    }
    notes.push(format!(
        "PEP 561 readiness: {}",
        if pep561_report.ready { "ready" } else { "not ready" }
    ));

    let summary = CommandSummary {
        command: String::from(command_name),
        config_path: config.config_path.display().to_string(),
        config_source: config.source,
        discovered_sources: snapshot.discovered_sources,
        lowered_modules: snapshot.lowered_modules.len(),
        planned_artifacts: snapshot.emit_plan.len(),
        tracked_modules: snapshot.tracked_modules,
        notes,
    };

    print_verify_summary(
        args.run.format,
        &summary,
        &diagnostics,
        portability_report.as_ref(),
        Some(&pep561_report),
        api_diff_report.as_ref(),
        type_health_report.as_ref(),
    )?;
    Ok(exit_code(&diagnostics))
}

fn print_verify_summary(
    format: OutputFormat,
    summary: &CommandSummary,
    diagnostics: &DiagnosticReport,
    portability: Option<&TypePortabilityReport>,
    pep561: Option<&Pep561ReadinessReport>,
    api_diff: Option<&ApiSurfaceDiffReport>,
    type_health: Option<&TypeHealthReport>,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            print_summary(OutputFormat::Text, summary, diagnostics)?;
            if let Some(pep561) = pep561 {
                println!(
                    "  pep561 readiness: {}",
                    if pep561.ready { "ready" } else { "not ready" }
                );
                for issue in &pep561.blocking_issues {
                    println!("    blocking: {issue}");
                }
                for advisory in &pep561.advisories {
                    println!("    advisory: {advisory}");
                }
            }
            if let Some(type_health) = type_health {
                println!("  publication type-health score: {}/100", type_health.score);
            }
            if let Some(api_diff) = api_diff {
                println!("  api diff semver recommendation: {}", api_diff.semver_recommendation);
            }
            Ok(())
        }
        OutputFormat::Json => {
            let payload = serde_json::json!({
                "schema_version": CLI_JSON_SCHEMA_VERSION,
                "summary": summary,
                "diagnostics": diagnostics,
                "portability": portability,
                "pep561": pep561,
                "api_diff": api_diff,
                "type_health": type_health,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload)
                    .context("unable to serialize verify summary as JSON")?
            );
            Ok(())
        }
    }
}

pub(crate) fn publication_type_health_diagnostics(report: &TypeHealthReport) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    if report.score < 100 {
        diagnostics.push(Diagnostic::error(
            "TPY7002",
            format!(
                "publication type-health score {} is below package maintainer threshold 100",
                report.score
            ),
        ));
    }
    for package in &report.packages {
        if !package.has_py_typed && !package.is_stub_only {
            diagnostics.push(Diagnostic::error(
                "TPY7002",
                format!(
                    "package `{}` does not expose PEP 561 typing metadata (`py.typed` or stubs)",
                    package.name
                ),
            ));
        }
        if package.stub_version_matches_runtime == Some(false) {
            diagnostics.push(Diagnostic::warning(
                "TPY7002",
                format!(
                    "stub package `{}` version {} does not match runtime version {}",
                    package.name,
                    package.stub_version.as_deref().unwrap_or("unknown"),
                    package.runtime_version.as_deref().unwrap_or("unknown")
                ),
            ));
        }
        if package.precision_debt > 0 {
            diagnostics.push(Diagnostic::warning(
                "TPY7002",
                format!(
                    "package `{}` has {} type precision debt finding(s)",
                    package.name, package.precision_debt
                ),
            ));
        }
    }
    diagnostics
}

pub(crate) fn pep561_readiness_report(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
    diagnostics: &DiagnosticReport,
) -> Pep561ReadinessReport {
    let out_root = config.resolve_relative_path(&config.config.project.out_dir);
    let package_roots = py_typed_package_roots(&out_root, artifacts);
    let has_stub_surface = artifacts.iter().any(|artifact| artifact.stub_path.is_some());
    let mut blocking_issues = Vec::new();
    let mut advisories = Vec::new();

    if !has_stub_surface {
        blocking_issues.push(String::from(
            "authoritative build output does not include `.pyi` files for the package surface",
        ));
    }
    if !config.config.emit.write_py_typed {
        blocking_issues.push(String::from(
            "`emit.write_py_typed` is disabled, so published packages will not advertise inline typing support",
        ));
    } else if package_roots.is_empty() {
        advisories.push(String::from(
            "no package roots were detected for `py.typed`; module-only layouts should verify publication metadata manually",
        ));
    }

    for diagnostic in
        diagnostics.diagnostics.iter().filter(|diagnostic| diagnostic.code == "TPY5003")
    {
        match diagnostic.severity {
            Severity::Error => blocking_issues.push(diagnostic.message.clone()),
            Severity::Warning | Severity::Note => advisories.push(diagnostic.message.clone()),
        }
    }

    blocking_issues.sort();
    blocking_issues.dedup();
    advisories.sort();
    advisories.dedup();

    Pep561ReadinessReport { ready: blocking_issues.is_empty(), blocking_issues, advisories }
}

fn load_checker_allowlist(
    config: &ConfigHandle,
    path: Option<&Path>,
) -> Result<Vec<CheckerAllowlistEntry>> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };
    let resolved_path =
        if path.is_absolute() { path.to_path_buf() } else { config.config_dir.join(path) };
    let contents = fs::read_to_string(&resolved_path)
        .with_context(|| format!("unable to read checker allowlist {}", resolved_path.display()))?;
    let allowlist: CheckerAllowlist = toml::from_str(&contents).with_context(|| {
        format!("unable to parse checker allowlist {}", resolved_path.display())
    })?;
    let today = current_utc_day()?;
    for (index, entry) in allowlist.disagreements.iter().enumerate() {
        validate_checker_allowlist_entry(entry, today).with_context(|| {
            format!("invalid checker allowlist entry {} in {}", index + 1, resolved_path.display())
        })?;
    }
    Ok(allowlist.disagreements)
}

fn current_utc_day() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?;
    Ok(i64::try_from(elapsed.as_secs() / 86_400).unwrap_or(i64::MAX))
}

pub(crate) fn validate_checker_allowlist_entry(
    entry: &CheckerAllowlistEntry,
    today: i64,
) -> Result<()> {
    if entry.checker.trim().is_empty() {
        anyhow::bail!("`checker` must not be empty");
    }
    if entry.contains.trim().is_empty() {
        anyhow::bail!("`contains` must not be empty");
    }
    if entry.reason.trim().is_empty() {
        anyhow::bail!("`reason` must not be empty");
    }
    let expires = entry
        .expires
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("`expires` is required for temporary allowlist entries"))?;
    let expiration_day = parse_iso_date_as_utc_day(expires)?;
    if expiration_day < today {
        anyhow::bail!("allowlist entry expired on `{expires}`");
    }
    Ok(())
}

fn parse_iso_date_as_utc_day(value: &str) -> Result<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        anyhow::bail!("`expires` must use the ISO date format YYYY-MM-DD");
    }
    let year = value[0..4].parse::<i64>().context("invalid expiration year")?;
    let month = value[5..7].parse::<u32>().context("invalid expiration month")?;
    let day = value[8..10].parse::<u32>().context("invalid expiration day")?;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => anyhow::bail!("invalid expiration month `{month}`"),
    };
    if day == 0 || day > days_in_month {
        anyhow::bail!("invalid expiration day `{day}` for month `{month}`");
    }
    Ok(days_from_civil(year, month, day))
}

fn days_from_civil(mut year: i64, month: u32, day: u32) -> i64 {
    year -= i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub(crate) fn expand_checker_list(raw: &str) -> Result<Vec<String>> {
    let values = raw.split(',').map(str::trim).filter(|value| !value.is_empty());
    let mut checkers = Vec::new();
    for value in values {
        if value == "all" {
            checkers.extend([
                String::from("basedpyright"),
                String::from("mypy"),
                String::from("pyright"),
                String::from("ty"),
            ]);
        } else {
            checkers.push(value.to_owned());
        }
    }
    if checkers.is_empty() {
        anyhow::bail!("checker list must name at least one checker");
    }
    checkers.sort();
    checkers.dedup();
    Ok(checkers)
}

pub(crate) fn verify_checker_invocations(args: &VerifyArgs) -> Result<Vec<String>> {
    let mut checkers = args.checkers.clone();
    if let Some(preset) = args.checker_preset.as_deref() {
        checkers.extend(
            expand_checker_list(preset)
                .with_context(|| format!("unable to expand verify checker preset `{preset}`"))?,
        );
    }
    checkers.sort();
    checkers.dedup();
    Ok(checkers)
}

pub(crate) fn type_portability_score(
    diagnostics: &DiagnosticReport,
    checker_count: usize,
) -> String {
    if checker_count == 0 {
        return String::from("n/a");
    }
    type_portability_report(diagnostics, checker_count).render()
}

pub(crate) fn type_portability_report(
    diagnostics: &DiagnosticReport,
    checker_count: usize,
) -> TypePortabilityReport {
    if checker_count == 0 {
        return TypePortabilityReport { score: 0, passing_checkers: 0, total_checkers: 0 };
    }
    let blocking_checker_failures = diagnostics
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.severity == typepython_diagnostics::Severity::Error
                && diagnostic.message.contains("external checker `")
        })
        .count()
        .min(checker_count);
    let passing_checkers = checker_count - blocking_checker_failures;
    let score = (passing_checkers * 100) / checker_count;
    TypePortabilityReport { score, passing_checkers, total_checkers: checker_count }
}

impl TypePortabilityReport {
    fn render(&self) -> String {
        format!(
            "{}/100 ({}/{} checker(s) passing)",
            self.score, self.passing_checkers, self.total_checkers
        )
    }
}

pub(crate) fn verify_build_artifacts(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    let out_root = config.resolve_relative_path(&config.config.project.out_dir);

    let artifact_diagnostics = artifacts
        .par_iter()
        .map(|artifact| verify_build_artifact(config, artifact))
        .collect::<Vec<_>>();
    for artifact_group in artifact_diagnostics {
        diagnostics.diagnostics.extend(artifact_group);
    }

    if config.config.emit.write_py_typed {
        for package_root in py_typed_package_roots(&out_root, artifacts) {
            let marker_path = package_root.join("py.typed");
            if !marker_path.exists() {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!("missing package marker `{}`", marker_path.display()),
                ));
            }
        }
    }

    let snapshot_path =
        config.resolve_relative_path(&config.config.project.cache_dir).join("snapshot.json");
    if !snapshot_path.exists() {
        diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!("missing incremental snapshot `{}`", snapshot_path.display()),
        ));
    } else if let Err(error) = verify_incremental_snapshot(&snapshot_path) {
        diagnostics.push(Diagnostic::error(
            "TPY6001",
            format!(
                "incremental snapshot `{}` is incompatible or corrupt: {}",
                snapshot_path.display(),
                error
            ),
        ));
    }

    diagnostics
}

pub(crate) fn verify_packaged_artifacts(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
    supplied_artifacts: &[SuppliedVerifyArtifact],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    if supplied_artifacts.is_empty() {
        return diagnostics;
    }

    let expected_files = match expected_published_files(config, artifacts) {
        Ok(files) => files,
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!("unable to collect authoritative build artifacts for publication verification: {error}"),
            ));
            return diagnostics;
        }
    };
    let published_package_roots = published_package_roots(&expected_files);
    let published_top_level_surface_files = published_top_level_surface_files(&expected_files);

    let supplied_diagnostics = supplied_artifacts
        .par_iter()
        .map(|artifact| {
            verify_supplied_artifact(
                artifact,
                &expected_files,
                &published_package_roots,
                &published_top_level_surface_files,
            )
        })
        .collect::<Vec<_>>();
    for diagnostic_group in supplied_diagnostics {
        diagnostics.diagnostics.extend(diagnostic_group);
    }

    let supplied_identities = supplied_artifacts
        .iter()
        .filter_map(|artifact| {
            let archive = read_supplied_artifact_entries(artifact).ok()?;
            let identity = supplied_archive_distribution_identity(artifact, &archive.entries)?;
            Some((artifact, identity))
        })
        .collect::<Vec<_>>();
    if let Some(project_identity) = local_project_distribution_identity(config) {
        for (artifact, identity) in &supplied_identities {
            if *identity != project_identity {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "{} artifact `{}` identity `{}-{}` does not match project metadata `{}-{}`",
                        artifact.kind.label(),
                        artifact.path.display(),
                        identity.normalized_name,
                        identity.normalized_version,
                        project_identity.normalized_name,
                        project_identity.normalized_version,
                    ),
                ));
            }
        }
    }
    if let Some((first_artifact, first_identity)) = supplied_identities.first() {
        for (artifact, identity) in supplied_identities.iter().skip(1) {
            if identity != first_identity {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "{} artifact `{}` identity `{}-{}` does not match {} artifact `{}` identity `{}-{}`",
                        artifact.kind.label(),
                        artifact.path.display(),
                        identity.normalized_name,
                        identity.normalized_version,
                        first_artifact.kind.label(),
                        first_artifact.path.display(),
                        first_identity.normalized_name,
                        first_identity.normalized_version,
                    ),
                ));
            }
        }
    }

    diagnostics
}

pub(crate) fn verify_publication_metadata(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
    modules: Option<&[LoweredModule]>,
    supplied_artifacts: &[SuppliedVerifyArtifact],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    let requirements = modules
        .map(|modules| publication_requirements_from_modules(artifacts, modules))
        .unwrap_or_else(|| publication_requirements_from_artifacts(artifacts));

    if let Some(metadata) = local_project_package_metadata(config) {
        diagnostics.diagnostics.extend(publication_metadata_diagnostics(
            "project metadata",
            &requirements,
            &metadata,
        ));
    }

    for artifact in supplied_artifacts {
        match supplied_artifact_package_metadata(artifact) {
            Ok(Some(metadata)) => diagnostics.diagnostics.extend(publication_metadata_diagnostics(
                &format!("{} metadata `{}`", artifact.kind.label(), artifact.path.display()),
                &requirements,
                &metadata,
            )),
            Ok(None) => {}
            Err(error) => diagnostics.push(Diagnostic::warning(
                "TPY5003",
                format!(
                    "unable to inspect packaging metadata in {} artifact `{}`: {error}",
                    artifact.kind.label(),
                    artifact.path.display(),
                ),
            )),
        }
    }

    diagnostics
}

pub(crate) fn verify_external_checkers(
    config: &ConfigHandle,
    checkers: &[String],
    allowlist: &[CheckerAllowlistEntry],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    if checkers.is_empty() {
        return diagnostics;
    }

    let out_root = config.resolve_relative_path(&config.config.project.out_dir);
    let checker_diagnostics = checkers
        .par_iter()
        .filter_map(|checker| verify_external_checker(config, &out_root, checker, allowlist))
        .collect::<Vec<_>>();
    diagnostics.diagnostics.extend(checker_diagnostics);

    diagnostics
}

fn publication_requirements_from_artifacts(artifacts: &[EmitArtifact]) -> PublicationRequirements {
    let mut requirements = PublicationRequirements::default();

    for artifact in artifacts {
        for path in
            [artifact.runtime_path.as_ref(), artifact.stub_path.as_ref()].into_iter().flatten()
        {
            let Ok(source) = fs::read_to_string(path) else {
                continue;
            };
            let file_requirements = publication_requirements_from_source(&source);
            requirements.min_python =
                max_python_target(requirements.min_python, file_requirements.min_python);
            requirements.needs_typing_extensions |= file_requirements.needs_typing_extensions;
        }
    }

    requirements
}

fn publication_requirements_from_modules(
    artifacts: &[EmitArtifact],
    modules: &[LoweredModule],
) -> PublicationRequirements {
    let modules_by_source = modules
        .iter()
        .map(|module| (module.source_path.as_path(), module))
        .collect::<BTreeMap<_, _>>();
    let mut requirements = PublicationRequirements::default();

    for artifact in artifacts {
        let Some(module) = modules_by_source.get(artifact.source_path.as_path()) else {
            continue;
        };
        for feature in &module.metadata.required_runtime_features {
            requirements.min_python = max_python_target(
                requirements.min_python,
                Some(PythonTarget::min_runtime_for(*feature)),
            );
        }
        if module
            .metadata
            .required_backports
            .contains(&BackportRequirement::TypingExtensionsAtLeast412)
        {
            requirements.needs_typing_extensions = true;
        }
    }

    requirements
}

fn publication_requirements_from_source(source: &str) -> PublicationRequirements {
    let mut requirements = PublicationRequirements::default();

    if source.contains("typing_extensions.") || source.contains("from typing_extensions import ") {
        requirements.needs_typing_extensions = true;
    }
    if source.contains("typing.ReadOnly")
        || source.contains("from typing import ReadOnly")
        || source.contains("typing.TypeIs")
        || source.contains("from typing import TypeIs")
        || source.contains("typing.NoDefault")
        || source.contains("from typing import NoDefault")
        || source.contains("warnings.deprecated")
        || source.contains("from warnings import deprecated")
    {
        requirements.min_python =
            max_python_target(requirements.min_python, Some(PythonTarget::PYTHON_3_13));
    }

    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("type ") {
            requirements.min_python =
                max_python_target(requirements.min_python, Some(PythonTarget::PYTHON_3_12));
            if native_type_params_include_default(trimmed) {
                requirements.min_python =
                    max_python_target(requirements.min_python, Some(PythonTarget::PYTHON_3_13));
            }
        }
        if (trimmed.starts_with("def ")
            || trimmed.starts_with("async def ")
            || trimmed.starts_with("class "))
            && native_header_uses_type_params(trimmed)
        {
            requirements.min_python =
                max_python_target(requirements.min_python, Some(PythonTarget::PYTHON_3_12));
            if native_type_params_include_default(trimmed) {
                requirements.min_python =
                    max_python_target(requirements.min_python, Some(PythonTarget::PYTHON_3_13));
            }
        }
    }

    requirements
}

fn native_header_uses_type_params(line: &str) -> bool {
    let prefix_len = if line.starts_with("async def ") {
        "async def ".len()
    } else if line.starts_with("def ") {
        "def ".len()
    } else if line.starts_with("class ") {
        "class ".len()
    } else {
        return false;
    };
    let name_len = line[prefix_len..]
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
        .map(char::len_utf8)
        .sum::<usize>();
    line.as_bytes().get(prefix_len + name_len) == Some(&b'[')
}

fn native_type_params_include_default(line: &str) -> bool {
    let Some(start) = line.find('[') else {
        return false;
    };
    let Some(end) = line[start..].find(']') else {
        return false;
    };
    line[start + 1..start + end].contains('=')
}

fn max_python_target(
    current: Option<PythonTarget>,
    candidate: Option<PythonTarget>,
) -> Option<PythonTarget> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.max(candidate)),
        (Some(current), None) => Some(current),
        (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

fn local_project_package_metadata(config: &ConfigHandle) -> Option<PackageMetadata> {
    let pyproject_path = config.config_dir.join("pyproject.toml");
    let rendered = fs::read_to_string(pyproject_path).ok()?;
    let parsed = toml::from_str::<PyProjectMetadata>(&rendered).ok()?;
    let project = parsed.project?;
    Some(PackageMetadata {
        requires_python: project.requires_python,
        requires_dist: project.dependencies.unwrap_or_default(),
    })
}

fn local_project_distribution_identity(config: &ConfigHandle) -> Option<DistributionIdentity> {
    let pyproject_path = config.config_dir.join("pyproject.toml");
    let rendered = fs::read_to_string(pyproject_path).ok()?;
    let parsed = toml::from_str::<PyProjectMetadata>(&rendered).ok()?;
    let project = parsed.project?;
    let name = project.name?;
    let version = project.version?;
    if !valid_distribution_name_regex().is_match(&name) {
        return None;
    }
    Some(DistributionIdentity {
        normalized_name: normalize_distribution_name(&name),
        normalized_version: normalize_version(&version)?,
    })
}

fn supplied_artifact_package_metadata(
    artifact: &SuppliedVerifyArtifact,
) -> std::result::Result<Option<PackageMetadata>, String> {
    let archive = read_supplied_artifact_entries(artifact)?;
    let entries = &archive.entries;
    let metadata = match artifact.kind {
        SuppliedArtifactKind::Wheel => entries
            .iter()
            .find(|(path, _)| path.ends_with(".dist-info/METADATA"))
            .map(|(_, bytes)| bytes),
        SuppliedArtifactKind::Sdist => entries.get("PKG-INFO"),
    };
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    parse_package_metadata_text(metadata)
}

fn supplied_archive_distribution_identity(
    artifact: &SuppliedVerifyArtifact,
    entries: &BTreeMap<String, Vec<u8>>,
) -> Option<DistributionIdentity> {
    let (metadata_path, metadata) = match artifact.kind {
        SuppliedArtifactKind::Wheel => {
            let mut metadata =
                entries.iter().filter(|(path, _)| path.ends_with(".dist-info/METADATA"));
            let first = metadata.next()?;
            if metadata.next().is_some() {
                return None;
            }
            (first.0.as_str(), first.1.as_slice())
        }
        SuppliedArtifactKind::Sdist => ("PKG-INFO", entries.get("PKG-INFO")?.as_slice()),
    };
    core_metadata_diagnostics(artifact, metadata_path, metadata).1
}

fn parse_package_metadata_text(
    bytes: &[u8],
) -> std::result::Result<Option<PackageMetadata>, String> {
    let rendered = String::from_utf8(bytes.to_vec())
        .map_err(|error| format!("invalid UTF-8 metadata: {error}"))?;
    let mut requires_python = None;
    let mut requires_dist = Vec::new();
    for line in rendered.lines() {
        if let Some(value) = line.strip_prefix("Requires-Python:") {
            requires_python = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("Requires-Dist:") {
            requires_dist.push(value.trim().to_owned());
        }
    }
    Ok(Some(PackageMetadata { requires_python, requires_dist }))
}

fn publication_metadata_diagnostics(
    label: &str,
    requirements: &PublicationRequirements,
    metadata: &PackageMetadata,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if let Some(required_python) = requirements.min_python {
        match metadata
            .requires_python
            .as_deref()
            .and_then(minimum_python_from_specifier)
        {
            Some(actual_min) if actual_min < required_python => diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "{label} declares Requires-Python `{}` but emitted artifacts require at least `{required_python}`",
                    metadata.requires_python.as_deref().unwrap_or_default()
                ),
            )),
            None => diagnostics.push(Diagnostic::warning(
                "TPY5003",
                format!(
                    "{label} does not declare a parseable Requires-Python lower bound while emitted artifacts require at least `{required_python}`"
                ),
            )),
            Some(_) => {}
        }
    }

    if requirements.needs_typing_extensions {
        match typing_extensions_lower_bound(&metadata.requires_dist) {
            Some(version) if version < (4, 12) => diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "{label} declares `typing_extensions` with lower bound `{}` but emitted artifacts require `typing_extensions>=4.12`",
                    format_version_pair(version)
                ),
            )),
            None => diagnostics.push(Diagnostic::warning(
                "TPY5003",
                format!(
                    "{label} does not declare `typing_extensions>=4.12` even though emitted artifacts import `typing_extensions`"
                ),
            )),
            Some(_) => {}
        }
    }

    diagnostics
}

fn minimum_python_from_specifier(specifier: &str) -> Option<PythonTarget> {
    specifier.split(',').find_map(|clause| {
        let clause = clause.trim();
        let version = clause.strip_prefix(">=")?;
        PythonTarget::parse(version.trim())
    })
}

fn typing_extensions_lower_bound(requirements: &[String]) -> Option<(u16, u16)> {
    requirements.iter().find_map(|requirement| {
        let normalized = requirement.replace(' ', "");
        if !normalized.starts_with("typing_extensions") {
            return None;
        }
        let lower = normalized.split(';').next()?.split(',').find_map(|clause| {
            let version = clause.strip_prefix("typing_extensions>=")?;
            parse_major_minor_version(version)
        });
        lower.or_else(|| normalized.starts_with("typing_extensions").then_some((0, 0)))
    })
}

fn parse_major_minor_version(text: &str) -> Option<(u16, u16)> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

fn format_version_pair(version: (u16, u16)) -> String {
    format!("{}.{}", version.0, version.1)
}

pub(crate) fn verify_runtime_public_name_parity(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    let out_root = config.resolve_relative_path(&config.config.project.out_dir);

    let artifact_diagnostics = artifacts
        .par_iter()
        .map(|artifact| verify_runtime_public_name_parity_for_artifact(config, &out_root, artifact))
        .collect::<Vec<_>>();
    for diagnostic_group in artifact_diagnostics {
        diagnostics.diagnostics.extend(diagnostic_group);
    }

    diagnostics
}

fn verify_build_artifact(config: &ConfigHandle, artifact: &EmitArtifact) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if let Some(runtime_path) = &artifact.runtime_path {
        if !runtime_path.exists() {
            diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!("missing runtime artifact `{}`", runtime_path.display()),
            ));
        } else if let Some(diagnostic) = verify_emitted_text_artifact(runtime_path) {
            diagnostics.push(diagnostic);
        } else {
            diagnostics.extend(runtime_annotation_compatibility_diagnostics(
                config,
                runtime_path,
                config.config.project.target_python,
            ));
        }
        if config.config.emit.emit_pyc {
            let bytecode_path = match bytecode_path_for(runtime_path) {
                Ok(path) => path,
                Err(error) => {
                    diagnostics.push(Diagnostic::error(
                        "TPY5003",
                        format!(
                            "unable to determine bytecode path for `{}`: {error}",
                            runtime_path.display()
                        ),
                    ));
                    return diagnostics;
                }
            };
            if !bytecode_path.exists() {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!("missing bytecode artifact `{}`", bytecode_path.display()),
                ));
            }
        }
    }

    if let Some(stub_path) = &artifact.stub_path {
        if !stub_path.exists() {
            diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!("missing stub artifact `{}`", stub_path.display()),
            ));
        } else if let Some(diagnostic) = verify_emitted_text_artifact(stub_path) {
            diagnostics.push(diagnostic);
        } else {
            diagnostics.extend(stub_portability_diagnostics(
                stub_path,
                config.config.project.target_python,
            ));
            diagnostics.extend(stub_metadata_expectation_warnings(stub_path));
        }
    }

    if let (Some(runtime_path), Some(stub_path)) = (&artifact.runtime_path, &artifact.stub_path)
        && runtime_path.exists()
        && stub_path.exists()
        && let Some(diagnostic) = verify_emitted_declaration_surface(runtime_path, stub_path)
    {
        diagnostics.push(diagnostic);
    }

    diagnostics
}

pub(crate) fn runtime_annotation_compatibility_diagnostics(
    config: &ConfigHandle,
    runtime_path: &Path,
    target_python: PythonTarget,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let interpreter = resolve_python_executable(config);
    let mut command = ProcessCommand::new(&interpreter);
    command
        .args(["-B", "-c", ANNOTATION_RUNTIME_AUDIT_SCRIPT])
        .arg(runtime_path)
        .arg(target_python.to_string());
    if let Some(py_path) = annotation_runtime_pythonpath() {
        command.env("PYTHONPATH", py_path);
    }
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            diagnostics.push(Diagnostic::warning(
                "TPY5004",
                format!(
                    "unable to audit runtime annotation compatibility for `{}` with `{}`: {error}",
                    runtime_path.display(),
                    interpreter.display()
                ),
            ));
            return diagnostics;
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        diagnostics.push(Diagnostic::warning(
            "TPY5004",
            format!(
                "runtime annotation compatibility audit failed for `{}`: {}{}",
                runtime_path.display(),
                output.status,
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", stderr.trim())
                }
            ),
        ));
        return diagnostics;
    }
    let audit = match serde_json::from_slice::<AnnotationRuntimeAuditResult>(&output.stdout) {
        Ok(audit) => audit,
        Err(error) => {
            diagnostics.push(Diagnostic::warning(
                "TPY5004",
                format!(
                    "unable to parse runtime annotation audit output for `{}`: {error}",
                    runtime_path.display()
                ),
            ));
            return diagnostics;
        }
    };
    if let Some(parse_error) = audit.parse_error {
        let location = match (parse_error.line, parse_error.column) {
            (Some(line), Some(column)) => format!(" at {line}:{column}"),
            (Some(line), None) => format!(" at line {line}"),
            _ => String::new(),
        };
        let message = if parse_error.host_older_than_target {
            format!(
                "runtime annotation audit could not parse `{}` with Python {}{}: {}; the artifact targets Python {}, whose newer syntax requires a matching audit interpreter",
                runtime_path.display(),
                parse_error.host_python,
                location,
                parse_error.message,
                parse_error.target_python,
            )
        } else {
            format!(
                "runtime artifact `{}` is not valid Python {} syntax{}: {}",
                runtime_path.display(),
                parse_error.target_python,
                location,
                parse_error.message,
            )
        };
        diagnostics.push(if parse_error.host_older_than_target {
            Diagnostic::warning("TPY5004", message)
        } else {
            Diagnostic::error("TPY5004", message)
        });
        return diagnostics;
    }
    if target_python >= PythonTarget::PYTHON_3_14 && !audit.consumers.is_empty() {
        let mut diagnostic = Diagnostic::warning(
            "TPY5004",
            format!(
                "runtime annotation consumer(s) in `{}` may observe deferred annotations under Python {}",
                runtime_path.display(),
                target_python
            ),
        );
        for consumer in &audit.consumers {
            diagnostic =
                diagnostic.with_note(format!("detected runtime annotation consumer `{consumer}`"));
        }
        diagnostics.push(diagnostic);
    }
    for finding in audit.findings {
        diagnostics.push(
            Diagnostic::warning(
                "TPY5004",
                format!(
                    "runtime annotation compatibility risk in `{}`: {}",
                    runtime_path.display(),
                    finding.message
                ),
            )
            .with_note(format!(
                "annotation audit {} at {}:{}",
                finding.code, finding.line, finding.column
            )),
        );
    }
    diagnostics
}

fn annotation_runtime_pythonpath() -> Option<std::ffi::OsString> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let annotation_module = repo_root.join("typepython/annotation_compat.py");
    if !annotation_module.exists() {
        return env::var_os("PYTHONPATH").filter(|value| !value.is_empty());
    }
    prepend_pythonpath(&repo_root, env::var_os("PYTHONPATH").as_deref()).ok()
}

pub(crate) fn prepend_pythonpath(
    path: &Path,
    existing: Option<&std::ffi::OsStr>,
) -> Result<std::ffi::OsString, env::JoinPathsError> {
    let mut paths = vec![path.to_path_buf()];
    if let Some(existing) = existing.filter(|value| !value.is_empty()) {
        paths.extend(env::split_paths(existing));
    }
    env::join_paths(paths)
}

fn verify_supplied_artifact(
    artifact: &SuppliedVerifyArtifact,
    expected_files: &BTreeMap<String, Vec<u8>>,
    published_package_roots: &BTreeSet<String>,
    published_top_level_surface_files: &BTreeSet<String>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    match read_supplied_artifact_entries(artifact) {
        Ok(archive) => {
            let entries = &archive.entries;
            diagnostics.extend(archive_metadata_diagnostics(
                artifact,
                entries,
                archive.common_root.as_deref(),
            ));
            for (relative_path, expected_bytes) in expected_files {
                match entries.get(relative_path) {
                    None => diagnostics.push(Diagnostic::error(
                        "TPY5003",
                        format!(
                            "{} artifact `{}` is missing published file `{relative_path}`",
                            artifact.kind.label(),
                            artifact.path.display(),
                        ),
                    )),
                    Some(actual_bytes) if actual_bytes != expected_bytes => {
                        diagnostics.push(Diagnostic::error(
                            "TPY5003",
                            format!(
                                "{} artifact `{}` contains `{relative_path}` that diverges from the authoritative build output",
                                artifact.kind.label(),
                                artifact.path.display(),
                            ),
                        ));
                    }
                    Some(_) => {}
                }
            }
            for relative_path in entries.keys().filter(|path| {
                is_authoritative_publication_file(
                    path,
                    artifact.kind,
                    published_package_roots,
                    published_top_level_surface_files,
                )
            }) {
                if !expected_files.contains_key(relative_path) {
                    diagnostics.push(Diagnostic::error(
                        "TPY5003",
                        format!(
                            "{} artifact `{}` contains unexpected published file `{relative_path}`",
                            artifact.kind.label(),
                            artifact.path.display(),
                        ),
                    ));
                }
            }
        }
        Err(error) => diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!(
                "unable to inspect {} artifact `{}`: {error}",
                artifact.kind.label(),
                artifact.path.display(),
            ),
        )),
    }

    diagnostics
}

fn archive_metadata_diagnostics(
    artifact: &SuppliedVerifyArtifact,
    entries: &BTreeMap<String, Vec<u8>>,
    common_root: Option<&str>,
) -> Vec<Diagnostic> {
    match artifact.kind {
        SuppliedArtifactKind::Wheel => wheel_metadata_diagnostics(artifact, entries),
        SuppliedArtifactKind::Sdist => sdist_metadata_diagnostics(artifact, entries, common_root),
    }
}

fn wheel_metadata_diagnostics(
    artifact: &SuppliedVerifyArtifact,
    entries: &BTreeMap<String, Vec<u8>>,
) -> Vec<Diagnostic> {
    let dist_info_roots = entries
        .keys()
        .filter_map(|path| {
            let (root, _) = path.split_once('/')?;
            root.ends_with(".dist-info").then_some(root)
        })
        .collect::<BTreeSet<_>>();
    if dist_info_roots.len() != 1 {
        return vec![Diagnostic::error(
            "TPY5003",
            format!(
                "wheel artifact `{}` must contain exactly one top-level `.dist-info` directory with METADATA, WHEEL, and RECORD; found {}",
                artifact.path.display(),
                dist_info_roots.len(),
            ),
        )];
    }

    let Some(dist_info) = dist_info_roots.iter().next().copied() else {
        return Vec::new();
    };
    let metadata_path = format!("{dist_info}/METADATA");
    let wheel_path = format!("{dist_info}/WHEEL");
    let record_path = format!("{dist_info}/RECORD");
    let mut diagnostics = Vec::new();
    diagnostics.extend(required_archive_file_diagnostics(
        artifact,
        entries,
        [&metadata_path, &wheel_path, &record_path],
    ));

    let metadata_identity = entries.get(&metadata_path).and_then(|metadata| {
        let (metadata_diagnostics, identity) =
            core_metadata_diagnostics(artifact, &metadata_path, metadata);
        diagnostics.extend(metadata_diagnostics);
        identity
    });
    let wheel_identity = entries.get(&wheel_path).and_then(|wheel| {
        let (wheel_diagnostics, identity) = wheel_header_diagnostics(artifact, &wheel_path, wheel);
        diagnostics.extend(wheel_diagnostics);
        identity
    });
    if let Some(record) = entries.get(&record_path) {
        match record_paths(record) {
            Ok(recorded_paths) => {
                let missing = entries
                    .keys()
                    .filter(|path| !recorded_paths.contains(path.as_str()))
                    .take(6)
                    .cloned()
                    .collect::<Vec<_>>();
                if !missing.is_empty() {
                    diagnostics.push(Diagnostic::error(
                        "TPY5003",
                        format!(
                            "wheel artifact `{}` has an incomplete `{record_path}`; missing entr{} {}",
                            artifact.path.display(),
                            if missing.len() == 1 { "y" } else { "ies" },
                            missing.join(", "),
                        ),
                    ));
                }
            }
            Err(error) => diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "wheel artifact `{}` contains invalid `{record_path}`: {error}",
                    artifact.path.display(),
                ),
            )),
        }
    }
    if let Some(metadata_identity) = metadata_identity {
        diagnostics.extend(wheel_identity_diagnostics(
            artifact,
            dist_info,
            &metadata_identity,
            wheel_identity.as_ref(),
        ));
    }
    diagnostics
}

fn sdist_metadata_diagnostics(
    artifact: &SuppliedVerifyArtifact,
    entries: &BTreeMap<String, Vec<u8>>,
    common_root: Option<&str>,
) -> Vec<Diagnostic> {
    let Some(metadata) = entries.get("PKG-INFO") else {
        return vec![Diagnostic::error(
            "TPY5003",
            format!(
                "sdist artifact `{}` is missing required root `PKG-INFO` metadata",
                artifact.path.display(),
            ),
        )];
    };
    let (mut diagnostics, identity) = core_metadata_diagnostics(artifact, "PKG-INFO", metadata);
    if let Some(identity) = identity {
        diagnostics.extend(sdist_identity_diagnostics(artifact, common_root, &identity));
    }
    diagnostics
}

fn required_archive_file_diagnostics<'a>(
    artifact: &SuppliedVerifyArtifact,
    entries: &BTreeMap<String, Vec<u8>>,
    required: impl IntoIterator<Item = &'a String>,
) -> Vec<Diagnostic> {
    required
        .into_iter()
        .filter(|path| !entries.contains_key(path.as_str()))
        .map(|path| {
            Diagnostic::error(
                "TPY5003",
                format!(
                    "{} artifact `{}` is missing required metadata file `{path}`",
                    artifact.kind.label(),
                    artifact.path.display(),
                ),
            )
        })
        .collect()
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct DistributionIdentity {
    normalized_name: String,
    normalized_version: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct WheelArchiveIdentity {
    build: Option<String>,
    tags: BTreeSet<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct WheelFilenameIdentity {
    distribution: DistributionIdentity,
    build: Option<String>,
    tags: BTreeSet<String>,
}

fn core_metadata_diagnostics(
    artifact: &SuppliedVerifyArtifact,
    metadata_path: &str,
    bytes: &[u8],
) -> (Vec<Diagnostic>, Option<DistributionIdentity>) {
    let headers = match metadata_headers(bytes) {
        Ok(headers) => headers,
        Err(error) => {
            return (vec![archive_metadata_error(artifact, metadata_path, &error)], None);
        }
    };
    let mut diagnostics = Vec::new();
    let metadata_version = required_single_header(
        artifact,
        metadata_path,
        &headers,
        "Metadata-Version",
        &mut diagnostics,
    );
    let name = required_single_header(artifact, metadata_path, &headers, "Name", &mut diagnostics);
    let version =
        required_single_header(artifact, metadata_path, &headers, "Version", &mut diagnostics);

    if let Some(metadata_version) = metadata_version {
        let known = matches!(
            metadata_version,
            "1.0" | "1.1" | "1.2" | "2.1" | "2.2" | "2.3" | "2.4" | "2.5"
        );
        if !known {
            match numeric_format_version(metadata_version) {
                Some((2, minor)) if minor > 5 => diagnostics.push(archive_metadata_warning(
                    artifact,
                    metadata_path,
                    &format!("uses newer unsupported Metadata-Version `{metadata_version}`"),
                )),
                _ => diagnostics.push(archive_metadata_error(
                    artifact,
                    metadata_path,
                    &format!("has invalid Metadata-Version `{metadata_version}`"),
                )),
            }
        }
        if matches!(artifact.kind, SuppliedArtifactKind::Wheel) && metadata_version == "1.0" {
            diagnostics.push(archive_metadata_error(
                artifact,
                metadata_path,
                "uses Metadata-Version `1.0`, but wheels require version 1.1 or newer",
            ));
        }
    }
    let normalized_name = name.and_then(|name| {
        if valid_distribution_name_regex().is_match(name) {
            Some(normalize_distribution_name(name))
        } else {
            diagnostics.push(archive_metadata_error(
                artifact,
                metadata_path,
                &format!("has invalid distribution Name `{name}`"),
            ));
            None
        }
    });
    let normalized_version = version.and_then(|version| match normalize_version(version) {
        Some(version) => Some(version),
        None => {
            diagnostics.push(archive_metadata_error(
                artifact,
                metadata_path,
                &format!("has invalid PEP 440 Version `{version}`"),
            ));
            None
        }
    });
    let identity =
        normalized_name.zip(normalized_version).map(|(normalized_name, normalized_version)| {
            DistributionIdentity { normalized_name, normalized_version }
        });
    (diagnostics, identity)
}

fn wheel_header_diagnostics(
    artifact: &SuppliedVerifyArtifact,
    metadata_path: &str,
    bytes: &[u8],
) -> (Vec<Diagnostic>, Option<WheelArchiveIdentity>) {
    let headers = match metadata_headers(bytes) {
        Ok(headers) => headers,
        Err(error) => {
            return (vec![archive_metadata_error(artifact, metadata_path, &error)], None);
        }
    };
    let mut diagnostics = Vec::new();
    let wheel_version = required_single_header(
        artifact,
        metadata_path,
        &headers,
        "Wheel-Version",
        &mut diagnostics,
    );
    let root_is_purelib = required_single_header(
        artifact,
        metadata_path,
        &headers,
        "Root-Is-Purelib",
        &mut diagnostics,
    );
    let tags = headers.get("tag").map(Vec::as_slice).unwrap_or_default();
    if tags.is_empty() {
        diagnostics.push(archive_metadata_error(
            artifact,
            metadata_path,
            "is missing required `Tag`",
        ));
    }
    if let Some(wheel_version) = wheel_version {
        match numeric_format_version(wheel_version) {
            Some((1, 0)) => {}
            Some((1, _)) => diagnostics.push(archive_metadata_warning(
                artifact,
                metadata_path,
                &format!("uses newer unsupported Wheel-Version `{wheel_version}`"),
            )),
            _ => diagnostics.push(archive_metadata_error(
                artifact,
                metadata_path,
                &format!("has unsupported Wheel-Version `{wheel_version}`"),
            )),
        }
    }
    if let Some(root_is_purelib) = root_is_purelib
        && !matches!(root_is_purelib, "true" | "false")
    {
        diagnostics.push(archive_metadata_error(
            artifact,
            metadata_path,
            &format!("has invalid Root-Is-Purelib `{root_is_purelib}`"),
        ));
    }
    let mut valid_tags = BTreeSet::new();
    let mut tags_valid = !tags.is_empty();
    for tag in tags {
        if expanded_wheel_tag_regex().is_match(tag) {
            valid_tags.insert(tag.to_ascii_lowercase());
        } else {
            tags_valid = false;
            diagnostics.push(archive_metadata_error(
                artifact,
                metadata_path,
                &format!("has invalid expanded compatibility Tag `{tag}`"),
            ));
        }
    }
    let builds = headers.get("build").map(Vec::as_slice).unwrap_or_default();
    let valid_build = builds.is_empty()
        || (builds.len() == 1
            && !builds[0].is_empty()
            && builds[0].chars().next().is_some_and(|character| character.is_ascii_digit()));
    if !valid_build {
        diagnostics.push(archive_metadata_error(
            artifact,
            metadata_path,
            "has invalid Build tag; it must be unique and start with a digit",
        ));
    }
    let identity = (tags_valid && valid_build)
        .then(|| WheelArchiveIdentity { build: builds.first().cloned(), tags: valid_tags });
    (diagnostics, identity)
}

fn wheel_identity_diagnostics(
    artifact: &SuppliedVerifyArtifact,
    dist_info: &str,
    metadata_identity: &DistributionIdentity,
    wheel_identity: Option<&WheelArchiveIdentity>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let dist_info_identity = dist_info_identity(dist_info);
    match dist_info_identity {
        Some(identity) if identity != *metadata_identity => diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!(
                "wheel artifact `{}` identity mismatch: `{dist_info}` does not match METADATA name/version `{}-{}`",
                artifact.path.display(),
                metadata_identity.normalized_name,
                metadata_identity.normalized_version,
            ),
        )),
        None => diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!(
                "wheel artifact `{}` has invalid `.dist-info` identity `{dist_info}`",
                artifact.path.display(),
            ),
        )),
        Some(_) => {}
    }

    if artifact.path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("whl")) {
        match wheel_filename_identity(&artifact.path) {
            Ok(identity) => {
                if identity.distribution != *metadata_identity {
                    diagnostics.push(Diagnostic::error(
                        "TPY5003",
                        format!(
                            "wheel artifact filename `{}` does not match METADATA name/version `{}-{}`",
                            artifact.path.display(),
                            metadata_identity.normalized_name,
                            metadata_identity.normalized_version,
                        ),
                    ));
                }
                if let Some(wheel_identity) = wheel_identity {
                    if identity.build != wheel_identity.build {
                        diagnostics.push(Diagnostic::error(
                            "TPY5003",
                            format!(
                                "wheel artifact `{}` WHEEL Build does not match its filename build tag",
                                artifact.path.display(),
                            ),
                        ));
                    }
                    if identity.tags != wheel_identity.tags {
                        diagnostics.push(Diagnostic::error(
                            "TPY5003",
                            format!(
                                "wheel artifact `{}` WHEEL Tag fields do not match its filename compatibility tags",
                                artifact.path.display(),
                            ),
                        ));
                    }
                }
            }
            Err(error) => diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "wheel artifact `{}` has invalid filename: {error}",
                    artifact.path.display()
                ),
            )),
        }
    }
    diagnostics
}

fn sdist_identity_diagnostics(
    artifact: &SuppliedVerifyArtifact,
    common_root: Option<&str>,
    metadata_identity: &DistributionIdentity,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    match sdist_filename_identity(&artifact.path) {
        Ok(identity) if identity != *metadata_identity => diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!(
                "sdist artifact filename `{}` does not match PKG-INFO name/version `{}-{}`",
                artifact.path.display(),
                metadata_identity.normalized_name,
                metadata_identity.normalized_version,
            ),
        )),
        Err(error) => diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!("sdist artifact `{}` has invalid filename: {error}", artifact.path.display()),
        )),
        Ok(_) => {}
    }
    match common_root.and_then(distribution_stem_identity) {
        Some(identity) if identity != *metadata_identity => diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!(
                "sdist artifact `{}` root directory `{}` does not match PKG-INFO name/version `{}-{}`",
                artifact.path.display(),
                common_root.unwrap_or_default(),
                metadata_identity.normalized_name,
                metadata_identity.normalized_version,
            ),
        )),
        None => diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!(
                "sdist artifact `{}` must contain one valid top-level `name-version` directory",
                artifact.path.display(),
            ),
        )),
        Some(_) => {}
    }
    diagnostics
}

fn metadata_headers(bytes: &[u8]) -> std::result::Result<BTreeMap<String, Vec<String>>, String> {
    let rendered = std::str::from_utf8(bytes)
        .map_err(|error| format!("metadata is not valid UTF-8: {error}"))?;
    let mut headers = BTreeMap::<String, Vec<String>>::new();
    let mut current_header = None::<String>;
    for line in rendered.lines() {
        if line.is_empty() {
            break;
        }
        if line.chars().next().is_some_and(char::is_whitespace) {
            let Some(header) = current_header.as_ref() else {
                return Err(String::from("metadata starts with a continuation line"));
            };
            let Some(value) = headers.get_mut(header).and_then(|values| values.last_mut()) else {
                return Err(format!("metadata continuation for missing header `{header}`"));
            };
            let continuation = line.trim();
            if !value.is_empty() && !continuation.is_empty() {
                value.push(' ');
            }
            value.push_str(continuation);
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(format!("metadata header line `{line}` is missing `:`"));
        };
        if name.is_empty()
            || !name.chars().all(|character| character.is_ascii_graphic() && character != ':')
        {
            return Err(format!("metadata header line `{line}` has an invalid field name"));
        }
        let normalized_name = name.to_ascii_lowercase();
        headers.entry(normalized_name.clone()).or_default().push(value.trim().to_owned());
        current_header = Some(normalized_name);
    }
    Ok(headers)
}

fn required_single_header<'a>(
    artifact: &SuppliedVerifyArtifact,
    metadata_path: &str,
    headers: &'a BTreeMap<String, Vec<String>>,
    header: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'a str> {
    let values = headers.get(&header.to_ascii_lowercase()).map(Vec::as_slice).unwrap_or_default();
    if values.len() != 1 || values[0].is_empty() {
        diagnostics.push(archive_metadata_error(
            artifact,
            metadata_path,
            &format!("must contain exactly one non-empty `{header}`"),
        ));
        return None;
    }
    Some(values[0].as_str())
}

fn archive_metadata_error(
    artifact: &SuppliedVerifyArtifact,
    metadata_path: &str,
    detail: &str,
) -> Diagnostic {
    Diagnostic::error(
        "TPY5003",
        format!(
            "{} artifact `{}` metadata `{metadata_path}` {detail}",
            artifact.kind.label(),
            artifact.path.display(),
        ),
    )
}

fn archive_metadata_warning(
    artifact: &SuppliedVerifyArtifact,
    metadata_path: &str,
    detail: &str,
) -> Diagnostic {
    Diagnostic::warning(
        "TPY5003",
        format!(
            "{} artifact `{}` metadata `{metadata_path}` {detail}",
            artifact.kind.label(),
            artifact.path.display(),
        ),
    )
}

fn numeric_format_version(value: &str) -> Option<(u64, u64)> {
    let (major, minor) = value.split_once('.')?;
    if major.is_empty()
        || minor.is_empty()
        || !major.chars().all(|character| character.is_ascii_digit())
        || !minor.chars().all(|character| character.is_ascii_digit())
    {
        return None;
    }
    Some((major.parse().ok()?, minor.parse().ok()?))
}

fn valid_distribution_name_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        match Regex::new(r"^(?:[A-Za-z0-9]|[A-Za-z0-9][A-Za-z0-9._-]*[A-Za-z0-9])\z") {
            Ok(regex) => regex,
            Err(error) => panic!("invalid built-in distribution name regex: {error}"),
        }
    })
}

fn compressed_wheel_tag_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        match Regex::new(
            r"^[A-Za-z0-9_]+(?:\.[A-Za-z0-9_]+)*-[A-Za-z0-9_]+(?:\.[A-Za-z0-9_]+)*-[A-Za-z0-9_]+(?:\.[A-Za-z0-9_]+)*\z",
        ) {
            Ok(regex) => regex,
            Err(error) => panic!("invalid built-in wheel tag regex: {error}"),
        }
    })
}

fn expanded_wheel_tag_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| match Regex::new(r"^[A-Za-z0-9_]+-[A-Za-z0-9_]+-[A-Za-z0-9_]+\z") {
        Ok(regex) => regex,
        Err(error) => panic!("invalid built-in expanded wheel tag regex: {error}"),
    })
}

fn version_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        match Regex::new(
            r"(?x)^\s*v?(?:(?:(?P<epoch>[0-9]+)!)?(?P<release>[0-9]+(?:\.[0-9]+)*)(?P<pre>[-_.]?(?P<pre_l>a|b|c|rc|alpha|beta|pre|preview)[-_.]?(?P<pre_n>[0-9]+)?)?(?P<post>(?:-(?P<post_n1>[0-9]+))|(?:[-_.]?(?P<post_l>post|rev|r)[-_.]?(?P<post_n2>[0-9]+)?))?(?P<dev>[-_.]?(?P<dev_l>dev)[-_.]?(?P<dev_n>[0-9]+)?)?)(?:\+(?P<local>[a-z0-9]+(?:[-_.][a-z0-9]+)*))?\s*$",
        ) {
            Ok(regex) => regex,
            Err(error) => panic!("invalid built-in version regex: {error}"),
        }
    })
}

fn normalize_distribution_name(name: &str) -> String {
    let mut normalized = String::new();
    let mut separator = false;
    for character in name.chars() {
        if matches!(character, '-' | '_' | '.') {
            separator = true;
        } else {
            if separator && !normalized.is_empty() {
                normalized.push('-');
            }
            separator = false;
            normalized.push(character.to_ascii_lowercase());
        }
    }
    normalized
}

fn normalize_version(version: &str) -> Option<String> {
    if !version.is_ascii() {
        return None;
    }
    let lowercase_version = version.to_ascii_lowercase();
    let captures = version_regex().captures(&lowercase_version)?;
    let mut normalized = String::new();
    if let Some(epoch) = captures.name("epoch") {
        let epoch = normalize_integer(epoch.as_str());
        if epoch != "0" {
            normalized.push_str(&epoch);
            normalized.push('!');
        }
    }
    let release = captures.name("release")?.as_str();
    let mut release = release.split('.').map(normalize_integer).collect::<Vec<_>>();
    while release.len() > 1 && release.last().is_some_and(|segment| segment == "0") {
        release.pop();
    }
    normalized.push_str(&release.join("."));
    if let Some(pre_label) = captures.name("pre_l") {
        normalized.push_str(match pre_label.as_str().to_ascii_lowercase().as_str() {
            "a" | "alpha" => "a",
            "b" | "beta" => "b",
            "c" | "rc" | "pre" | "preview" => "rc",
            _ => return None,
        });
        normalized.push_str(&normalize_integer(
            captures.name("pre_n").map_or("0", |value| value.as_str()),
        ));
    }
    if captures.name("post").is_some() {
        normalized.push_str(".post");
        let number = captures
            .name("post_n1")
            .or_else(|| captures.name("post_n2"))
            .map_or("0", |value| value.as_str());
        normalized.push_str(&normalize_integer(number));
    }
    if captures.name("dev").is_some() {
        normalized.push_str(".dev");
        normalized.push_str(&normalize_integer(
            captures.name("dev_n").map_or("0", |value| value.as_str()),
        ));
    }
    if let Some(local) = captures.name("local") {
        normalized.push('+');
        normalized.push_str(
            &local
                .as_str()
                .split(['-', '_', '.'])
                .map(|part| {
                    if part.chars().all(|character| character.is_ascii_digit()) {
                        normalize_integer(part)
                    } else {
                        part.to_ascii_lowercase()
                    }
                })
                .collect::<Vec<_>>()
                .join("."),
        );
    }
    Some(normalized)
}

fn normalize_integer(value: &str) -> String {
    let normalized = value.trim_start_matches('0');
    if normalized.is_empty() { String::from("0") } else { normalized.to_owned() }
}

fn dist_info_identity(dist_info: &str) -> Option<DistributionIdentity> {
    let stem = dist_info.strip_suffix(".dist-info")?;
    let (name, version) = stem.rsplit_once('-')?;
    Some(DistributionIdentity {
        normalized_name: normalize_distribution_name(name),
        normalized_version: normalize_version(version)?,
    })
}

fn wheel_filename_identity(path: &Path) -> std::result::Result<WheelFilenameIdentity, String> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| String::from("filename is not valid UTF-8"))?;
    let stem =
        filename.strip_suffix(".whl").ok_or_else(|| String::from("filename must end in `.whl`"))?;
    let parts = stem.split('-').collect::<Vec<_>>();
    if !matches!(parts.len(), 5 | 6) {
        return Err(String::from("expected distribution-version(-build)-python-abi-platform.whl"));
    }
    if parts.len() == 6
        && !parts[2].chars().next().is_some_and(|character| character.is_ascii_digit())
    {
        return Err(String::from("build tag must start with a digit"));
    }
    let tag_start = parts.len() - 3;
    let tag = parts[tag_start..].join("-");
    if !compressed_wheel_tag_regex().is_match(&tag) {
        return Err(format!("invalid compatibility tag `{tag}`"));
    }
    if !valid_distribution_name_regex().is_match(parts[0]) {
        return Err(format!("invalid distribution component `{}`", parts[0]));
    }
    let normalized_version = normalize_version(parts[1])
        .ok_or_else(|| format!("invalid version component `{}`", parts[1]))?;
    Ok(WheelFilenameIdentity {
        distribution: DistributionIdentity {
            normalized_name: normalize_distribution_name(parts[0]),
            normalized_version,
        },
        build: (parts.len() == 6).then(|| parts[2].to_owned()),
        tags: expand_compressed_wheel_tags(
            parts[tag_start],
            parts[tag_start + 1],
            parts[tag_start + 2],
        ),
    })
}

fn expand_compressed_wheel_tags(
    python_tags: &str,
    abi_tags: &str,
    platform_tags: &str,
) -> BTreeSet<String> {
    let mut tags = BTreeSet::new();
    for python_tag in python_tags.split('.') {
        for abi_tag in abi_tags.split('.') {
            for platform_tag in platform_tags.split('.') {
                tags.insert(format!("{python_tag}-{abi_tag}-{platform_tag}").to_ascii_lowercase());
            }
        }
    }
    tags
}

fn sdist_filename_identity(path: &Path) -> std::result::Result<DistributionIdentity, String> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| String::from("filename is not valid UTF-8"))?;
    let stem = filename
        .strip_suffix(".tar.gz")
        .or_else(|| filename.strip_suffix(".tgz"))
        .or_else(|| filename.strip_suffix(".zip"))
        .ok_or_else(|| String::from("filename must end in `.tar.gz`, `.tgz`, or `.zip`"))?;
    distribution_stem_identity(stem)
        .ok_or_else(|| String::from("expected a valid name-version distribution identity"))
}

fn distribution_stem_identity(stem: &str) -> Option<DistributionIdentity> {
    let (name, version) = stem.rsplit_once('-')?;
    if !valid_distribution_name_regex().is_match(name) {
        return None;
    }
    Some(DistributionIdentity {
        normalized_name: normalize_distribution_name(name),
        normalized_version: normalize_version(version)?,
    })
}

fn record_paths(bytes: &[u8]) -> std::result::Result<BTreeSet<String>, String> {
    let rendered = std::str::from_utf8(bytes)
        .map_err(|error| format!("RECORD is not valid UTF-8: {error}"))?;
    let mut paths = BTreeSet::new();
    for (index, line) in rendered.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let path = record_first_field(line)
            .ok_or_else(|| format!("line {} is not valid CSV", index + 1))?;
        if path.is_empty() {
            return Err(format!("line {} has an empty path", index + 1));
        }
        paths.insert(
            validate_wheel_record_path(&path)
                .map_err(|error| format!("line {} has an invalid path: {error}", index + 1))?,
        );
    }
    Ok(paths)
}

fn record_first_field(line: &str) -> Option<String> {
    if let Some(remainder) = line.strip_prefix('"') {
        let mut value = String::new();
        let mut characters = remainder.chars().peekable();
        while let Some(character) = characters.next() {
            if character != '"' {
                value.push(character);
                continue;
            }
            if characters.peek() == Some(&'"') {
                characters.next();
                value.push('"');
                continue;
            }
            return (characters.next() == Some(',')).then_some(value);
        }
        None
    } else {
        line.split_once(',').map(|(path, _)| path.to_owned())
    }
}

fn verify_external_checker(
    config: &ConfigHandle,
    out_root: &Path,
    checker: &str,
    allowlist: &[CheckerAllowlistEntry],
) -> Option<Diagnostic> {
    let invocation = external_checker_invocation(
        checker,
        &config.config.project.target_python.to_string(),
        out_root,
    );
    let mut command = ProcessCommand::new(&invocation.program);
    command.args(&invocation.args).current_dir(&config.config_dir);
    let diagnostic = checker_diagnostic_from_output(&invocation.label, out_root, command.output())?;
    Some(allowlisted_checker_diagnostic(&invocation.label, diagnostic, allowlist))
}

pub(crate) fn allowlisted_checker_diagnostic(
    checker: &str,
    diagnostic: Diagnostic,
    allowlist: &[CheckerAllowlistEntry],
) -> Diagnostic {
    let Some(entry) = allowlist
        .iter()
        .find(|entry| entry.checker == checker && diagnostic.message.contains(&entry.contains))
    else {
        return diagnostic;
    };

    let mut warning = Diagnostic::warning(
        diagnostic.code,
        format!("known checker disagreement allowed for `{checker}`: {}", entry.reason),
    )
    .with_note(diagnostic.message)
    .with_note(format!("matched allowlist substring `{}`", entry.contains));
    if let Some(issue) = &entry.issue {
        warning = warning.with_note(format!("tracking issue: {issue}"));
    }
    if let Some(expires) = &entry.expires {
        warning = warning.with_note(format!("allowlist expires: {expires}"));
    }
    warning
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ExternalCheckerInvocation {
    pub(crate) label: String,
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
}

pub(crate) fn external_checker_invocation(
    checker: &str,
    target_python: &str,
    out_root: &Path,
) -> ExternalCheckerInvocation {
    let out_root = out_root.display().to_string();
    match checker {
        "mypy" => ExternalCheckerInvocation {
            label: String::from("mypy"),
            program: String::from("mypy"),
            args: vec![String::from("--python-version"), target_python.to_owned(), out_root],
        },
        "pyright" => ExternalCheckerInvocation {
            label: String::from("pyright"),
            program: String::from("pyright"),
            args: vec![String::from("--pythonversion"), target_python.to_owned(), out_root],
        },
        "ty" => ExternalCheckerInvocation {
            label: String::from("ty"),
            program: String::from("ty"),
            args: vec![
                String::from("check"),
                String::from("--no-progress"),
                String::from("--python-version"),
                target_python.to_owned(),
                out_root,
            ],
        },
        "pyrefly" => ExternalCheckerInvocation {
            label: String::from("pyrefly"),
            program: String::from("pyrefly"),
            args: vec![String::from("check"), out_root],
        },
        "basedpyright" => ExternalCheckerInvocation {
            label: String::from("basedpyright"),
            program: String::from("basedpyright"),
            args: vec![String::from("--pythonversion"), target_python.to_owned(), out_root],
        },
        "zuban" => ExternalCheckerInvocation {
            label: String::from("zuban"),
            program: String::from("zuban"),
            args: vec![String::from("check"), out_root],
        },
        custom => ExternalCheckerInvocation {
            label: custom.to_owned(),
            program: custom.to_owned(),
            args: vec![out_root],
        },
    }
}

pub(crate) fn verify_runtime_public_name_parity_for_artifact(
    config: &ConfigHandle,
    out_root: &Path,
    artifact: &EmitArtifact,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let (Some(runtime_path), Some(stub_path)) = (&artifact.runtime_path, &artifact.stub_path)
    else {
        return diagnostics;
    };
    if !(runtime_path.exists() && stub_path.exists()) {
        return diagnostics;
    }
    let Some(module_name) = logical_module_name_from_runtime_path(out_root, runtime_path) else {
        return diagnostics;
    };
    if let Err(error) = verify_runtime_module_importability(config, out_root, &module_name) {
        diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!(
                "runtime module `{}` from `{}` is not importable: {}",
                module_name,
                runtime_path.display(),
                error,
            ),
        ));
        return diagnostics;
    }
    let runtime_names = match runtime_public_names_from_import(config, out_root, &module_name) {
        Ok(names) => names,
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "unable to inspect runtime public names for `{}` from `{}`: {error}",
                    module_name,
                    runtime_path.display(),
                ),
            ));
            return diagnostics;
        }
    };
    let authoritative_names = match authoritative_public_names(stub_path) {
        Ok(names) => names,
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "unable to determine authoritative public names for `{}` from `{}`: {error}",
                    module_name,
                    stub_path.display(),
                ),
            ));
            return diagnostics;
        }
    };

    diagnostics.extend(surface_parity_diagnostics(
        &module_name,
        &runtime_names,
        &authoritative_names,
    ));
    diagnostics
}

fn logical_module_name_from_runtime_path(out_root: &Path, runtime_path: &Path) -> Option<String> {
    let relative = runtime_path.strip_prefix(out_root).ok()?;
    let stem = runtime_path.file_stem()?.to_str()?;
    let mut components = relative
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .components()
        .map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;

    if stem == "__init__" {
        return (!components.is_empty()).then(|| components.join("."));
    }

    components.push(stem.to_owned());
    Some(components.join("."))
}

fn runtime_public_names(runtime_path: &Path) -> std::result::Result<BTreeSet<String>, String> {
    public_names_from_module_file(runtime_path)
}

fn verify_runtime_module_importability(
    config: &ConfigHandle,
    out_root: &Path,
    module_name: &str,
) -> std::result::Result<(), String> {
    let probe_dir = runtime_import_probe_dir(module_name)?;
    let import_root = runtime_import_root(config, out_root)?;
    let interpreter = resolve_python_executable(config);
    let output = ProcessCommand::new(&interpreter)
        .current_dir(&probe_dir)
        .args(["-B", "-c", RUNTIME_IMPORTABILITY_SCRIPT])
        .arg(&import_root)
        .arg(module_name)
        .output()
        .map_err(|error| {
            format!(
                "unable to run runtime importability probe with `{}`: {error}",
                interpreter.display()
            )
        });
    let _ = fs::remove_dir_all(&probe_dir);
    let output = output?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr_suffix =
            if stderr.trim().is_empty() { String::new() } else { format!(": {}", stderr.trim()) };
        return Err(format!(
            "runtime importability probe exited with status {}{}",
            output.status, stderr_suffix
        ));
    }
    let result = serde_json::from_slice::<RuntimeImportabilityResult>(&output.stdout)
        .map_err(|error| format!("unable to parse runtime importability output: {error}"))?;
    if result.importable {
        Ok(())
    } else {
        Err(result.error.unwrap_or_else(|| format!("module `{module_name}` could not be imported")))
    }
}

fn runtime_public_names_from_import(
    config: &ConfigHandle,
    out_root: &Path,
    module_name: &str,
) -> std::result::Result<BTreeSet<String>, String> {
    let probe_dir = runtime_import_probe_dir(module_name)?;
    let import_root = runtime_import_root(config, out_root)?;
    let interpreter = resolve_python_executable(config);
    let output = ProcessCommand::new(&interpreter)
        .current_dir(&probe_dir)
        .args(["-B", "-c", RUNTIME_IMPORTABILITY_SCRIPT])
        .arg(&import_root)
        .arg(module_name)
        .output()
        .map_err(|error| {
            format!(
                "unable to inspect runtime public names with `{}`: {error}",
                interpreter.display()
            )
        });
    let _ = fs::remove_dir_all(&probe_dir);
    let output = output?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr_suffix =
            if stderr.trim().is_empty() { String::new() } else { format!(": {}", stderr.trim()) };
        return Err(format!(
            "runtime public-name probe exited with status {}{}",
            output.status, stderr_suffix
        ));
    }
    let result = serde_json::from_slice::<RuntimeImportabilityResult>(&output.stdout)
        .map_err(|error| format!("unable to parse runtime public-name output: {error}"))?;
    if !result.importable {
        return Err(result
            .error
            .unwrap_or_else(|| format!("module `{module_name}` could not be imported")));
    }
    result
        .public_names
        .map(|names| names.into_iter().collect())
        .ok_or_else(|| format!("runtime module `{module_name}` did not report public names"))
}

fn runtime_import_root(
    config: &ConfigHandle,
    out_root: &Path,
) -> std::result::Result<PathBuf, String> {
    let resolved = if out_root.is_absolute() {
        out_root.to_path_buf()
    } else {
        config.config_dir.join(out_root)
    };
    resolved.canonicalize().map_err(|error| {
        format!("unable to resolve runtime import root `{}`: {error}", resolved.display())
    })
}

fn runtime_import_probe_dir(module_name: &str) -> std::result::Result<PathBuf, String> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system time should be after epoch: {error}"))?
        .as_nanos();
    let sanitized = module_name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    let directory = env::temp_dir().join(format!("typepython-runtime-import-{sanitized}-{unique}"));
    fs::create_dir_all(&directory).map_err(|error| {
        format!("unable to create runtime probe dir {}: {error}", directory.display())
    })?;
    Ok(directory)
}

fn authoritative_public_names(path: &Path) -> std::result::Result<BTreeSet<String>, String> {
    public_names_from_module_file(path)
}

fn public_names_from_module_file(path: &Path) -> std::result::Result<BTreeSet<String>, String> {
    let source = SourceFile::from_path(path).map_err(|error| {
        format!("unable to read emitted artifact `{}`: {error}", path.display())
    })?;

    if let Some(names) = static_all_names_from_source(path, &source.text)? {
        return Ok(names);
    }

    let syntax = {
        let syntax = typepython_syntax::parse(source);
        if syntax.diagnostics.has_errors() { None } else { Some(syntax) }
    }
    .ok_or_else(|| format!("`{}` could not be parsed as a Python module", path.display()))?;
    Ok(module_level_surface_names(&syntax)
        .into_iter()
        .filter(|name| !name.starts_with('_'))
        .collect())
}

fn static_all_names_from_source(
    path: &Path,
    source: &str,
) -> std::result::Result<Option<BTreeSet<String>>, String> {
    let parsed = parse_module(source).map_err(|error| {
        format!("unable to parse `{}` while collecting `__all__`: {error}", path.display())
    })?;
    let suite = parsed.suite();
    let functions = suite
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::FunctionDef(function) => Some((function.name.as_str(), function)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    for stmt in suite {
        let value = match stmt {
            Stmt::Assign(assign)
                if assign.targets.iter().any(|target| is_name_target(target, "__all__")) =>
            {
                Some(assign.value.as_ref())
            }
            Stmt::AnnAssign(assign) if is_name_target(assign.target.as_ref(), "__all__") => {
                assign.value.as_deref()
            }
            _ => None,
        };

        if let Some(names) = resolve_static_all_names(value, &functions) {
            return Ok(Some(names.into_iter().collect()));
        }
        if value.is_some() {
            return Ok(None);
        }
    }

    Ok(None)
}

fn expected_published_files(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
) -> Result<BTreeMap<String, Vec<u8>>> {
    let out_root = config.resolve_relative_path(&config.config.project.out_dir);
    let mut expected_files = BTreeMap::new();
    let package_roots = py_typed_package_roots(&out_root, artifacts);

    for artifact in artifacts {
        if let Some(runtime_path) = &artifact.runtime_path {
            expected_files
                .insert(relative_publish_path(&out_root, runtime_path)?, fs::read(runtime_path)?);
        }
        if let Some(stub_path) = &artifact.stub_path {
            expected_files
                .insert(relative_publish_path(&out_root, stub_path)?, fs::read(stub_path)?);
        }
    }

    if config.config.emit.write_py_typed {
        for package_root in package_roots {
            let marker_path = package_root.join("py.typed");
            expected_files
                .insert(relative_publish_path(&out_root, &marker_path)?, fs::read(marker_path)?);
        }
    }

    Ok(expected_files)
}

fn relative_publish_path(out_root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(out_root)
        .with_context(|| format!("{} is not inside {}", path.display(), out_root.display()))?;
    Ok(normalize_glob_path(relative))
}

fn published_package_roots(expected_files: &BTreeMap<String, Vec<u8>>) -> BTreeSet<String> {
    expected_files
        .keys()
        .filter_map(|path| path.split_once('/').map(|(root, _)| root.to_owned()))
        .collect()
}

fn published_top_level_surface_files(
    expected_files: &BTreeMap<String, Vec<u8>>,
) -> BTreeSet<String> {
    expected_files
        .keys()
        .filter(|path| !path.contains('/') && is_importable_publication_file_name(path))
        .cloned()
        .collect()
}

fn is_importable_publication_file_name(path: &str) -> bool {
    let leaf = path.rsplit('/').next().unwrap_or(path);
    leaf.ends_with(".py")
        || leaf.ends_with(".pyi")
        || leaf.ends_with(".pyc")
        || leaf.ends_with(".pyo")
        || leaf.ends_with(".pth")
        || leaf.ends_with(".pyd")
        || leaf.ends_with(".so")
}

fn is_authoritative_publication_file(
    path: &str,
    artifact_kind: SuppliedArtifactKind,
    published_package_roots: &BTreeSet<String>,
    published_top_level_surface_files: &BTreeSet<String>,
) -> bool {
    if !is_importable_publication_file_name(path) {
        return false;
    }
    if matches!(artifact_kind, SuppliedArtifactKind::Wheel) {
        if let Some((root, remainder)) = path.split_once('/') {
            if published_package_roots.contains(root) {
                return true;
            }
            if root.ends_with(".dist-info") {
                return false;
            }
            if root.ends_with(".data") {
                return remainder.starts_with("purelib/") || remainder.starts_with("platlib/");
            }
            return true;
        }
        return true;
    }
    if let Some((root, _)) = path.split_once('/') {
        if published_package_roots.contains(root) {
            return true;
        }
        return !is_allowed_non_surface_path(path)
            && (!published_top_level_surface_files.is_empty()
                || !published_package_roots.is_empty());
    }
    if published_top_level_surface_files.contains(path) {
        return true;
    }
    !is_allowed_non_surface_file(path)
        && (!published_top_level_surface_files.is_empty() || !published_package_roots.is_empty())
}

fn is_allowed_non_surface_path(path: &str) -> bool {
    if let Some((root, remainder)) = path.split_once('/') {
        return root.ends_with(".data")
            && remainder.starts_with("scripts/")
            && remainder["scripts/".len()..].ends_with(".py")
            && !remainder["scripts/".len()..].contains('/')
            && !matches!(&remainder["scripts/".len()..], "__init__.py" | "__init__.pyi")
            && !remainder["scripts/".len()..].ends_with(".pyi");
    }
    false
}

fn is_allowed_non_surface_file(path: &str) -> bool {
    matches!(path, "setup.py" | "conftest.py" | "noxfile.py" | "toxfile.py")
}

struct SuppliedArchiveEntries {
    entries: BTreeMap<String, Vec<u8>>,
    common_root: Option<String>,
}

fn read_supplied_artifact_entries(
    artifact: &SuppliedVerifyArtifact,
) -> std::result::Result<SuppliedArchiveEntries, String> {
    let path_text = artifact.path.to_string_lossy().to_ascii_lowercase();
    match artifact.kind {
        SuppliedArtifactKind::Wheel => {
            if !(path_text.ends_with(".whl") || path_text.ends_with(".zip")) {
                return Err(String::from("expected a .whl or .zip file"));
            }
            Ok(SuppliedArchiveEntries {
                entries: read_zip_entries(&artifact.path, ArchivePathKind::Wheel)?
                    .into_iter()
                    .collect(),
                common_root: None,
            })
        }
        SuppliedArtifactKind::Sdist => {
            let entries = if path_text.ends_with(".tar.gz") || path_text.ends_with(".tgz") {
                read_tar_gz_entries(&artifact.path)?
            } else if path_text.ends_with(".zip") {
                read_zip_entries(&artifact.path, ArchivePathKind::Sdist)?
            } else {
                return Err(String::from("expected a .tar.gz, .tgz, or .zip file"));
            };
            let common_root = common_archive_root(&entries);
            let entries = strip_archive_root(entries, common_root.as_deref());
            Ok(SuppliedArchiveEntries { entries, common_root })
        }
    }
}

#[cfg(test)]
pub(crate) fn inspect_supplied_archive_paths(
    artifact: &SuppliedVerifyArtifact,
) -> std::result::Result<Vec<String>, String> {
    read_supplied_artifact_entries(artifact).map(|archive| archive.entries.into_keys().collect())
}

fn read_zip_entries(
    path: &Path,
    kind: ArchivePathKind,
) -> std::result::Result<Vec<(String, Vec<u8>)>, String> {
    let file = fs::File::open(path).map_err(|error| format!("unable to open archive: {error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("unable to read zip archive: {error}"))?;
    let mut entries = Vec::new();
    let mut member_paths = ArchiveMemberPaths::new(kind);

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| format!("unable to read zip entry {index}: {error}"))?;
        let entry_path = member_paths.register(file.name_raw(), file.is_dir())?;
        if file.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| format!("unable to read zip entry `{entry_path}`: {error}"))?;
        entries.push((entry_path, bytes));
    }

    Ok(entries)
}

fn read_tar_gz_entries(path: &Path) -> std::result::Result<Vec<(String, Vec<u8>)>, String> {
    let file = fs::File::open(path).map_err(|error| format!("unable to open archive: {error}"))?;
    let decoder = GzDecoder::new(file);
    let mut archive = TarArchive::new(decoder);
    let mut entries = Vec::new();
    let mut member_paths = ArchiveMemberPaths::new(ArchivePathKind::Sdist);

    for entry in
        archive.entries().map_err(|error| format!("unable to read tar archive: {error}"))?
    {
        let mut entry = entry.map_err(|error| format!("unable to read tar entry: {error}"))?;
        let entry_type = entry.header().entry_type();
        let raw_path = entry.path_bytes();
        let entry_path = member_paths.register(raw_path.as_ref(), entry_type.is_dir())?;
        if !entry_type.is_file() {
            continue;
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| format!("unable to read tar entry `{entry_path}`: {error}"))?;
        entries.push((entry_path, bytes));
    }

    Ok(entries)
}

fn strip_archive_root(
    entries: Vec<(String, Vec<u8>)>,
    common_root: Option<&str>,
) -> BTreeMap<String, Vec<u8>> {
    let Some(common_root) = common_root else {
        return entries.into_iter().collect();
    };

    entries
        .into_iter()
        .map(|(path, bytes)| {
            let normalized =
                path.strip_prefix(&format!("{common_root}/")).map(str::to_owned).unwrap_or(path);
            (normalized, bytes)
        })
        .collect()
}

fn common_archive_root(entries: &[(String, Vec<u8>)]) -> Option<String> {
    let mut root: Option<&str> = None;

    for (path, _) in entries {
        let mut components = path.split('/').filter(|component| !component.is_empty());
        let first = components.next()?;
        components.next()?;
        match root {
            Some(existing) if existing != first => return None,
            Some(_) => {}
            None => root = Some(first),
        }
    }

    root.map(str::to_owned)
}

fn verify_incremental_snapshot(path: &Path) -> Result<(), String> {
    let rendered = fs::read_to_string(path)
        .map_err(|error| format!("unable to read incremental snapshot: {error}"))?;
    decode_snapshot(&rendered).map(|_| ()).map_err(|error| error.to_string())
}

fn verify_emitted_text_artifact(path: &Path) -> Option<Diagnostic> {
    let source = match SourceFile::from_path(path) {
        Ok(source) => source,
        Err(error) => {
            return Some(Diagnostic::error(
                "TPY5003",
                format!("unable to read emitted artifact `{}`: {error}", path.display()),
            ));
        }
    };
    let syntax = typepython_syntax::parse(source);
    if syntax.diagnostics.has_errors() {
        Some(Diagnostic::error(
            "TPY5003",
            format!("emitted artifact `{}` is not valid Python syntax", path.display()),
        ))
    } else if path.extension().is_some_and(|extension| extension == "pyi") {
        verify_stub_syntax_rules(path, &syntax)
    } else {
        None
    }
}

pub(crate) fn verify_emitted_declaration_surface(
    runtime_path: &Path,
    stub_path: &Path,
) -> Option<Diagnostic> {
    let runtime_syntax = emitted_syntax(runtime_path)?;
    let stub_syntax = emitted_syntax(stub_path)?;
    let runtime_names = runtime_public_names(runtime_path).ok()?;
    let authoritative_names = authoritative_public_names(stub_path).ok()?;

    let runtime_surface = declaration_surface(&runtime_syntax)
        .into_iter()
        .filter(|entry| surface_entry_is_exported(entry, &runtime_names))
        .collect::<BTreeSet<_>>();
    let stub_surface = declaration_surface(&stub_syntax)
        .into_iter()
        .filter(|entry| surface_entry_is_exported(entry, &authoritative_names))
        .collect::<BTreeSet<_>>();

    declaration_surface_drift_diagnostic(runtime_path, stub_path, &runtime_surface, &stub_surface)
}

fn declaration_surface_drift_diagnostic(
    runtime_path: &Path,
    stub_path: &Path,
    runtime_surface: &BTreeSet<SurfaceEntry>,
    stub_surface: &BTreeSet<SurfaceEntry>,
) -> Option<Diagnostic> {
    if runtime_surface == stub_surface {
        return None;
    }

    let runtime_by_identity = runtime_surface
        .iter()
        .map(|entry| ((entry.owner.clone(), entry.name.clone(), entry.kind), entry))
        .collect::<BTreeMap<_, _>>();
    let stub_by_identity = stub_surface
        .iter()
        .map(|entry| ((entry.owner.clone(), entry.name.clone(), entry.kind), entry))
        .collect::<BTreeMap<_, _>>();

    let runtime_by_name = runtime_surface
        .iter()
        .map(|entry| ((entry.owner.clone(), entry.name.clone()), entry))
        .collect::<BTreeMap<_, _>>();
    let stub_by_name = stub_surface
        .iter()
        .map(|entry| ((entry.owner.clone(), entry.name.clone()), entry))
        .collect::<BTreeMap<_, _>>();

    let added = stub_by_identity
        .iter()
        .filter(|(key, stub_entry)| {
            !runtime_by_identity.contains_key(*key)
                && !runtime_by_name
                    .get(&(stub_entry.owner.clone(), stub_entry.name.clone()))
                    .is_some_and(|runtime_entry| {
                        transformed_stub_surface_is_compatible(runtime_entry, stub_entry)
                    })
        })
        .map(|(_, entry)| display_surface_entry(entry))
        .collect::<Vec<_>>();
    let removed = runtime_by_identity
        .iter()
        .filter(|(key, runtime_entry)| {
            !stub_by_identity.contains_key(*key)
                && !stub_by_name
                    .get(&(runtime_entry.owner.clone(), runtime_entry.name.clone()))
                    .is_some_and(|stub_entry| {
                        transformed_stub_surface_is_compatible(runtime_entry, stub_entry)
                    })
        })
        .map(|(_, entry)| display_surface_entry(entry))
        .collect::<Vec<_>>();
    let changed = runtime_by_identity
        .iter()
        .filter_map(|(key, runtime_entry)| {
            let stub_entry = stub_by_identity.get(key)?;
            (runtime_entry.legacy_detail != stub_entry.legacy_detail).then(|| {
                format!(
                    "{}: runtime=`{}` stub=`{}`",
                    display_surface_entry(runtime_entry),
                    runtime_entry.legacy_detail,
                    stub_entry.legacy_detail
                )
            })
        })
        .collect::<Vec<_>>();

    if added.is_empty() && removed.is_empty() && changed.is_empty() {
        return None;
    }

    let mut diagnostic = Diagnostic::error(
        "TPY5003",
        format!(
            "emitted runtime/stub declaration surface differs between `{}` and `{}`",
            runtime_path.display(),
            stub_path.display()
        ),
    );
    if !added.is_empty() {
        diagnostic = diagnostic.with_note(format!("type surface only: {}", added.join(", ")));
    }
    if !removed.is_empty() {
        diagnostic = diagnostic.with_note(format!("runtime only: {}", removed.join(", ")));
    }
    if !changed.is_empty() {
        diagnostic = diagnostic.with_note(format!("changed members: {}", changed.join("; ")));
    }
    Some(diagnostic)
}

fn transformed_stub_surface_is_compatible(
    runtime_entry: &SurfaceEntry,
    stub_entry: &SurfaceEntry,
) -> bool {
    runtime_entry.owner.is_none()
        && stub_entry.owner.is_none()
        && runtime_entry.name == stub_entry.name
        && runtime_entry.kind == "function"
        && stub_entry.kind == "value"
        && !stub_entry.legacy_detail.is_empty()
}

fn verify_stub_syntax_rules(
    path: &Path,
    syntax: &typepython_syntax::SyntaxTree,
) -> Option<Diagnostic> {
    if syntax.statements.iter().any(stub_statement_is_runtime) {
        return Some(Diagnostic::error(
            "TPY5003",
            format!("emitted stub artifact `{}` contains runtime statements", path.display()),
        ));
    }

    for statement in &syntax.statements {
        match statement {
            typepython_syntax::SyntaxStatement::Value(statement)
                if statement.owner_name.is_some() =>
            {
                return Some(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` contains executable assignments",
                        path.display()
                    ),
                ));
            }
            typepython_syntax::SyntaxStatement::Value(statement)
                if stub_unannotated_value_statement_is_allowed(statement) => {}
            typepython_syntax::SyntaxStatement::Value(statement)
                if statement.owner_name.is_none() && statement.annotation.is_none() =>
            {
                return Some(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` contains value declarations without annotations",
                        path.display()
                    ),
                ));
            }
            typepython_syntax::SyntaxStatement::Interface(_)
            | typepython_syntax::SyntaxStatement::DataClass(_)
            | typepython_syntax::SyntaxStatement::SealedClass(_)
            | typepython_syntax::SyntaxStatement::Unsafe(_) => {
                return Some(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` contains TypePython-only syntax",
                        path.display()
                    ),
                ));
            }
            typepython_syntax::SyntaxStatement::ClassDef(statement) => {
                if statement.members.iter().any(|member| {
                    member.kind == typepython_syntax::ClassMemberKind::Field
                        && member.annotation.is_none()
                }) {
                    return Some(Diagnostic::error(
                        "TPY5003",
                        format!(
                            "emitted stub artifact `{}` contains class fields without annotations",
                            path.display()
                        ),
                    ));
                }
            }
            _ => {}
        }
    }

    None
}

pub(crate) fn stub_portability_diagnostics(
    path: &Path,
    target_python: PythonTarget,
) -> Vec<Diagnostic> {
    let Ok(rendered) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();

    for (index, line) in rendered.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        if (trimmed.starts_with("type ") || native_header_uses_type_params(trimmed))
            && !target_python.supports(RuntimeFeature::InlineTypeParams)
        {
            diagnostics.push(
                Diagnostic::warning(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` uses Python 3.12 generic syntax that may not be portable for target Python {}",
                        path.display(),
                        target_python
                    ),
                )
                .with_note(format!(
                    "line {line_number}: use compat emit style or lower to TypeVar/TypeAlias forms before invoking downstream checkers"
                )),
            );
        }
        if native_type_params_include_default(trimmed)
            && !target_python.supports(RuntimeFeature::GenericDefaults)
        {
            diagnostics.push(
                Diagnostic::warning(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` uses Python 3.13 generic default syntax that may not be portable for target Python {}",
                        path.display(),
                        target_python
                    ),
                )
                .with_note(format!(
                    "line {line_number}: remove defaulted type parameters or emit typing_extensions-compatible aliases"
                )),
            );
        }
    }

    for (needle, feature, rewrite) in [
        (
            "typing.ReadOnly",
            RuntimeFeature::TypingReadOnly,
            "import ReadOnly from typing_extensions for Python targets before 3.13",
        ),
        (
            "from typing import ReadOnly",
            RuntimeFeature::TypingReadOnly,
            "import ReadOnly from typing_extensions for Python targets before 3.13",
        ),
        (
            "typing.TypeIs",
            RuntimeFeature::TypingTypeIs,
            "import TypeIs from typing_extensions for Python targets before 3.13",
        ),
        (
            "from typing import TypeIs",
            RuntimeFeature::TypingTypeIs,
            "import TypeIs from typing_extensions for Python targets before 3.13",
        ),
        (
            "typing.NoDefault",
            RuntimeFeature::TypingNoDefault,
            "avoid NoDefault or gate it behind Python 3.13+ output",
        ),
        (
            "from typing import NoDefault",
            RuntimeFeature::TypingNoDefault,
            "avoid NoDefault or gate it behind Python 3.13+ output",
        ),
    ] {
        if rendered.contains(needle) && !target_python.supports(feature) {
            diagnostics.push(
                Diagnostic::warning(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` references `{needle}`, which may not be portable for target Python {}",
                        path.display(),
                        target_python
                    ),
                )
                .with_note(rewrite),
            );
        }
    }

    diagnostics
}

fn stub_metadata_expectation_warnings(path: &Path) -> Vec<Diagnostic> {
    let Ok(rendered) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut markers = Vec::new();
    if rendered.contains("# tpy:sealed") {
        markers.push("tpy:sealed");
    }
    if rendered.contains("# tpy:derived") {
        markers.push("tpy:derived");
    }
    if rendered.contains("# tpy:unknown") {
        markers.push("tpy:unknown");
    }
    if markers.is_empty() {
        return Vec::new();
    }

    vec![Diagnostic::warning(
        "TPY5003",
        format!(
            "emitted stub artifact `{}` uses TypePython metadata comments ({}) that external type checkers ignore; use `typepython verify --checker ...` to validate downstream behavior",
            path.display(),
            markers.join(", ")
        ),
    )]
}

fn stub_statement_is_runtime(statement: &typepython_syntax::SyntaxStatement) -> bool {
    match statement {
        typepython_syntax::SyntaxStatement::Call(statement)
            if stub_type_parameter_factory_call_is_allowed(statement) =>
        {
            false
        }
        typepython_syntax::SyntaxStatement::Call(_)
        | typepython_syntax::SyntaxStatement::MemberAccess(_)
        | typepython_syntax::SyntaxStatement::MethodCall(_)
        | typepython_syntax::SyntaxStatement::Return(_)
        | typepython_syntax::SyntaxStatement::Yield(_)
        | typepython_syntax::SyntaxStatement::If(_)
        | typepython_syntax::SyntaxStatement::Assert(_)
        | typepython_syntax::SyntaxStatement::Invalidate(_)
        | typepython_syntax::SyntaxStatement::Match(_)
        | typepython_syntax::SyntaxStatement::For(_)
        | typepython_syntax::SyntaxStatement::With(_)
        | typepython_syntax::SyntaxStatement::ExceptHandler(_)
        | typepython_syntax::SyntaxStatement::Unsafe(_) => true,
        _ => false,
    }
}

fn stub_unannotated_value_statement_is_allowed(
    statement: &typepython_syntax::ValueStatement,
) -> bool {
    statement.owner_name.is_none()
        && statement.annotation.is_none()
        && (statement.names == [String::from("__all__")]
            || statement.value_callee.as_deref().is_some_and(is_stub_type_parameter_factory))
}

fn stub_type_parameter_factory_call_is_allowed(
    statement: &typepython_syntax::CallStatement,
) -> bool {
    is_stub_type_parameter_factory(&statement.callee)
}

fn is_stub_type_parameter_factory(callee: &str) -> bool {
    matches!(callee, "TypeVar" | "ParamSpec" | "TypeVarTuple" | "NewType")
}

fn emitted_syntax(path: &Path) -> Option<typepython_syntax::SyntaxTree> {
    let source = SourceFile::from_path(path).ok()?;
    let syntax = typepython_syntax::parse(source);
    if syntax.diagnostics.has_errors() { None } else { Some(syntax) }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct SurfaceEntry {
    owner: Option<String>,
    kind: &'static str,
    name: String,
    legacy_detail: String,
}

fn declaration_surface(
    syntax: &typepython_syntax::SyntaxTree,
) -> std::collections::BTreeSet<SurfaceEntry> {
    let mut surface = std::collections::BTreeSet::new();

    for statement in &syntax.statements {
        match statement {
            typepython_syntax::SyntaxStatement::TypeAlias(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "typealias",
                    name: statement.name.clone(),
                    legacy_detail: statement.value.clone(),
                });
            }
            typepython_syntax::SyntaxStatement::Interface(statement)
            | typepython_syntax::SyntaxStatement::DataClass(statement)
            | typepython_syntax::SyntaxStatement::SealedClass(statement)
            | typepython_syntax::SyntaxStatement::ClassDef(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "class",
                    name: statement.name.clone(),
                    legacy_detail: format!(
                        "bases=[{}];final={}",
                        statement.bases.join(","),
                        statement.is_final_decorator
                    ),
                });
                for member in &statement.members {
                    surface.insert(SurfaceEntry {
                        owner: Some(statement.name.clone()),
                        kind: match member.kind {
                            typepython_syntax::ClassMemberKind::Field => "field",
                            typepython_syntax::ClassMemberKind::Method => "method",
                            typepython_syntax::ClassMemberKind::Overload => "overload",
                        },
                        name: member.name.clone(),
                        legacy_detail: match member.kind {
                            typepython_syntax::ClassMemberKind::Field => format!(
                                "annotation={};final={};classvar={}",
                                member.annotation.clone().unwrap_or_default(),
                                member.is_final_decorator,
                                member.is_class_var
                            ),
                            typepython_syntax::ClassMemberKind::Method
                            | typepython_syntax::ClassMemberKind::Overload => format!(
                                "kind={:?};final={};sig={}",
                                member
                                    .method_kind
                                    .unwrap_or(typepython_syntax::MethodKind::Instance),
                                member.is_final_decorator,
                                format_signature(&member.params, member.returns.as_deref())
                            ),
                        },
                    });
                }
            }
            typepython_syntax::SyntaxStatement::OverloadDef(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "overload",
                    name: statement.name.clone(),
                    legacy_detail: format_signature(
                        &statement.params,
                        statement.returns.as_deref(),
                    ),
                });
            }
            typepython_syntax::SyntaxStatement::FunctionDef(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "function",
                    name: statement.name.clone(),
                    legacy_detail: format_signature(
                        &statement.params,
                        statement.returns.as_deref(),
                    ),
                });
            }
            typepython_syntax::SyntaxStatement::Import(statement) => {
                for binding in &statement.bindings {
                    surface.insert(SurfaceEntry {
                        owner: None,
                        kind: "import",
                        name: binding.local_name.clone(),
                        legacy_detail: binding.source_path.clone(),
                    });
                }
            }
            typepython_syntax::SyntaxStatement::Value(statement) => {
                if statement.owner_name.is_some() || statement.owner_type_name.is_some() {
                    continue;
                }
                for name in &statement.names {
                    surface.insert(SurfaceEntry {
                        owner: None,
                        kind: "value",
                        name: name.clone(),
                        legacy_detail: statement.annotation.clone().unwrap_or_default(),
                    });
                }
            }
            typepython_syntax::SyntaxStatement::Call(_) => {}
            typepython_syntax::SyntaxStatement::MethodCall(_) => {}
            typepython_syntax::SyntaxStatement::MemberAccess(_) => {}
            typepython_syntax::SyntaxStatement::Return(_) => {}
            typepython_syntax::SyntaxStatement::Yield(_) => {}
            typepython_syntax::SyntaxStatement::If(_) => {}
            typepython_syntax::SyntaxStatement::Assert(_) => {}
            typepython_syntax::SyntaxStatement::Invalidate(_) => {}
            typepython_syntax::SyntaxStatement::Match(_) => {}
            typepython_syntax::SyntaxStatement::For(_) => {}
            typepython_syntax::SyntaxStatement::With(_) => {}
            typepython_syntax::SyntaxStatement::ExceptHandler(_) => {}
            typepython_syntax::SyntaxStatement::Unsafe(_) => {}
        }
    }

    surface
}

fn module_level_surface_names(syntax: &typepython_syntax::SyntaxTree) -> BTreeSet<String> {
    declaration_surface(syntax)
        .into_iter()
        .filter(|entry| entry.owner.is_none())
        .map(|entry| entry.name)
        .collect()
}

fn checker_diagnostic_from_output(
    checker: &str,
    out_root: &Path,
    output: std::io::Result<Output>,
) -> Option<Diagnostic> {
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return Some(Diagnostic::error(
                "TPY5003",
                format!("unable to run external checker `{checker}`: {error}"),
            ));
        }
    };
    if output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let details = [stdout, stderr]
        .into_iter()
        .filter(|stream| !stream.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let suffix = if details.is_empty() { String::new() } else { format!(":\n{details}") };

    Some(Diagnostic::error(
        "TPY5003",
        format!(
            "external checker `{checker}` rejected emitted build output under `{}`{}",
            out_root.display(),
            suffix
        ),
    ))
}

fn surface_parity_diagnostics(
    module_name: &str,
    runtime_names: &BTreeSet<String>,
    authoritative_names: &BTreeSet<String>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let missing_from_runtime =
        authoritative_names.difference(runtime_names).cloned().collect::<Vec<_>>();
    let missing_from_type_surface =
        runtime_names.difference(authoritative_names).cloned().collect::<Vec<_>>();

    if !missing_from_runtime.is_empty() {
        diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!(
                "runtime module `{}` is missing public names declared by the authoritative type surface: {}",
                module_name,
                missing_from_runtime.join(", "),
            ),
        ));
    }
    if !missing_from_type_surface.is_empty() {
        diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!(
                "authoritative type surface for `{}` is missing runtime public names: {}",
                module_name,
                missing_from_type_surface.join(", "),
            ),
        ));
    }

    diagnostics
}

fn resolve_static_all_names(
    value: Option<&Expr>,
    functions: &BTreeMap<&str, &ruff_python_ast::StmtFunctionDef>,
) -> Option<Vec<String>> {
    let value = value?;
    let names = literal_string_sequence(value);
    if names.is_some() {
        return names;
    }

    let Expr::Call(call) = value else {
        return None;
    };
    let Expr::Name(name) = call.func.as_ref() else {
        return None;
    };
    if !call.arguments.args.is_empty() || !call.arguments.keywords.is_empty() {
        return None;
    }

    resolve_function_return(functions.get(name.id.as_str())?)
}

fn resolve_function_return(function: &ruff_python_ast::StmtFunctionDef) -> Option<Vec<String>> {
    if !function.decorator_list.is_empty() {
        return None;
    }
    let parameters = &function.parameters;
    if !parameters.posonlyargs.is_empty()
        || !parameters.args.is_empty()
        || !parameters.kwonlyargs.is_empty()
        || parameters.vararg.is_some()
        || parameters.kwarg.is_some()
    {
        return None;
    }
    let [Stmt::Return(return_stmt)] = function.body.as_slice() else {
        return None;
    };
    literal_string_sequence(return_stmt.value.as_deref()?)
}

fn literal_string_sequence(expr: &Expr) -> Option<Vec<String>> {
    let elements = match expr {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return None,
    };
    elements
        .iter()
        .map(|element| match element {
            Expr::StringLiteral(string) => Some(string.value.to_str().to_owned()),
            _ => None,
        })
        .collect()
}

fn is_name_target(expr: &Expr, expected: &str) -> bool {
    matches!(expr, Expr::Name(name) if name.id.as_str() == expected)
}

pub(crate) fn public_surface_completeness_diagnostics(
    config: &ConfigHandle,
    syntax_trees: &[typepython_syntax::SyntaxTree],
    lowered_modules: &[LoweredModule],
    stub_contexts: &BTreeMap<PathBuf, TypePythonStubContext>,
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();

    if !config.config.typing.require_known_public_types {
        return diagnostics;
    }

    let lowered_by_source = lowered_modules
        .iter()
        .map(|module| (module.source_path.clone(), module))
        .collect::<BTreeMap<_, _>>();

    for syntax in syntax_trees {
        let source_incomplete_entries = declaration_surface(syntax)
            .into_iter()
            .filter(is_public_surface_entry)
            .filter(|entry| entry.kind != "import")
            .filter(|entry| surface_detail_is_incomplete(&entry.legacy_detail))
            .map(|entry| display_surface_entry(&entry))
            .collect::<BTreeSet<_>>();
        let surface_syntax = if syntax.source.kind == SourceKind::TypePython {
            let Some(module) = lowered_by_source.get(&syntax.source.path) else {
                continue;
            };
            let context = stub_contexts.get(&syntax.source.path).cloned().unwrap_or_default();
            let Ok(stub_source) = generate_typepython_stub_source(module, &context) else {
                continue;
            };
            let stub_file = SourceFile {
                path: syntax.source.path.with_extension("pyi"),
                kind: SourceKind::Stub,
                logical_module: syntax.source.logical_module.clone(),
                text: stub_source,
            };
            typepython_syntax::parse(stub_file)
        } else {
            syntax.clone()
        };

        for entry in declaration_surface(&surface_syntax)
            .into_iter()
            .filter(is_public_surface_entry)
            .filter(|entry| entry.kind != "import")
        {
            let display = display_surface_entry(&entry);
            if !surface_detail_is_incomplete(&entry.legacy_detail)
                && !source_incomplete_entries.contains(&display)
            {
                continue;
            }

            diagnostics.push(Diagnostic::error(
                "TPY4015",
                format!(
                    "module `{}` exports incomplete type surface for `{}`",
                    syntax.source.path.display(),
                    display
                ),
            ));
        }
    }

    diagnostics
}

fn is_public_surface_entry(entry: &SurfaceEntry) -> bool {
    if entry.name.starts_with('_') {
        return false;
    }

    match &entry.owner {
        Some(owner) => !owner.starts_with('_'),
        None => true,
    }
}

fn surface_entry_is_exported(entry: &SurfaceEntry, exported_names: &BTreeSet<String>) -> bool {
    match &entry.owner {
        Some(owner) => exported_names.contains(owner),
        None => exported_names.contains(&entry.name),
    }
}

fn display_surface_entry(entry: &SurfaceEntry) -> String {
    match &entry.owner {
        Some(owner) => format!("{owner}.{}", entry.name),
        None => entry.name.clone(),
    }
}

fn surface_detail_is_incomplete(detail: &str) -> bool {
    let mut token = String::new();

    for ch in detail.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
            continue;
        }

        if matches!(token.as_str(), "dynamic" | "unknown") {
            return true;
        }
        token.clear();
    }

    matches!(token.as_str(), "dynamic" | "unknown")
}

fn format_signature(params: &[typepython_syntax::FunctionParam], returns: Option<&str>) -> String {
    format!(
        "({})->{}",
        params
            .iter()
            .map(|param| match &param.annotation {
                Some(annotation) => format!("{}:{}", param.name, annotation),
                None => param.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(","),
        returns.unwrap_or("")
    )
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use std::process::ExitStatus;

    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;

    fn exit_status(code: i32) -> ExitStatus {
        #[cfg(unix)]
        {
            ExitStatus::from_raw(code << 8)
        }
        #[cfg(windows)]
        {
            ExitStatus::from_raw(code as u32)
        }
    }

    #[test]
    fn static_all_names_collects_literal_assignment() {
        let names = static_all_names_from_source(
            Path::new("module.py"),
            "__all__ = [\"build\", \"parse\"]\n\ndef build() -> int:\n    return 1\n",
        )
        .expect("literal __all__ should parse");

        assert_eq!(names, Some(BTreeSet::from([String::from("build"), String::from("parse"),])));
    }

    #[test]
    fn static_all_names_collects_helper_function_tuple_return() {
        let names = static_all_names_from_source(
            Path::new("module.py"),
            "def exports():\n    return (\"build\",)\n\n__all__ = exports()\n",
        )
        .expect("helper __all__ should parse");

        assert_eq!(names, Some(BTreeSet::from([String::from("build")])));
    }

    #[test]
    fn static_all_names_rejects_decorated_helper_function() {
        let names = static_all_names_from_source(
            Path::new("module.py"),
            "@cache\ndef exports():\n    return [\"build\"]\n\n__all__ = exports()\n",
        )
        .expect("decorated helper should still parse");

        assert_eq!(names, None);
    }

    #[test]
    fn checker_diagnostic_from_output_includes_streams() {
        let diagnostic = checker_diagnostic_from_output(
            "pyright",
            Path::new("/tmp/build"),
            Ok(Output {
                status: exit_status(1),
                stdout: b"stdout details\n".to_vec(),
                stderr: b"stderr details\n".to_vec(),
            }),
        )
        .expect("failing checker should produce a diagnostic");

        assert!(diagnostic.message.contains("external checker `pyright` rejected"));
        assert!(diagnostic.message.contains("stdout details"));
        assert!(diagnostic.message.contains("stderr details"));
    }

    #[test]
    fn surface_parity_diagnostics_report_both_directions() {
        let diagnostics = surface_parity_diagnostics(
            "app",
            &BTreeSet::from([String::from("runtime_only"), String::from("shared")]),
            &BTreeSet::from([String::from("shared"), String::from("stub_only")]),
        );

        let rendered = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("stub_only"));
        assert!(rendered.contains("runtime_only"));
    }
}

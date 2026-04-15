use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use rayon::prelude::*;
use ruff_python_ast::{Expr, Stmt};
use ruff_python_parser::parse_module;
use tar::Archive as TarArchive;
use typepython_config::ConfigHandle;
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_emit::{EmitArtifact, TypePythonStubContext, generate_typepython_stub_source};
use typepython_incremental::decode_snapshot;
use typepython_lowering::{BackportRequirement, LoweredModule};
use typepython_syntax::{SourceFile, SourceKind};
use typepython_target::PythonTarget;
use zip::ZipArchive;

use crate::cli::VerifyArgs;
use crate::discovery::normalize_glob_path;
use crate::pipeline::{
    build_diagnostics, ensure_output_dirs, materialize_build_outputs,
    persist_pipeline_analysis_state, py_typed_package_roots, run_pipeline,
    runtime_write_diagnostic, should_emit_build_outputs,
};
use crate::{
    CommandSummary, RUNTIME_IMPORTABILITY_SCRIPT, bytecode_path_for, exit_code, load_project,
    load_project_without_python_executable_validation, print_summary, resolve_python_executable,
};

#[derive(Debug, serde::Deserialize)]
struct RuntimeImportabilityResult {
    importable: bool,
    error: Option<String>,
    public_names: Option<Vec<String>>,
}

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

#[derive(Debug, serde::Deserialize)]
struct PyProjectProjectMetadata {
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
    if !snapshot.diagnostics.has_errors() && !diagnostics.has_errors() {
        diagnostics
            .diagnostics
            .extend(verify_external_checkers(&config, &args.checkers).diagnostics);
    }

    let supplied_artifact_count = args.wheels.len() + args.sdists.len();
    if supplied_artifact_count > 0 {
        notes.push(format!(
            "verified {} supplied wheel/sdist artifact(s) against the authoritative build tree",
            supplied_artifact_count
        ));
    }
    if !args.checkers.is_empty() {
        notes.push(format!(
            "ran {} external checker invocation(s) against the emitted build output",
            args.checkers.len()
        ));
    }

    let summary = CommandSummary {
        command: String::from("verify"),
        config_path: config.config_path.display().to_string(),
        config_source: config.source,
        discovered_sources: snapshot.discovered_sources,
        lowered_modules: snapshot.lowered_modules.len(),
        planned_artifacts: snapshot.emit_plan.len(),
        tracked_modules: snapshot.tracked_modules,
        notes,
    };

    print_summary(args.run.format, &summary, &diagnostics)?;
    Ok(exit_code(&diagnostics))
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
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    if checkers.is_empty() {
        return diagnostics;
    }

    let out_root = config.resolve_relative_path(&config.config.project.out_dir);
    let checker_diagnostics = checkers
        .par_iter()
        .filter_map(|checker| verify_external_checker(config, &out_root, checker))
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

fn supplied_artifact_package_metadata(
    artifact: &SuppliedVerifyArtifact,
) -> std::result::Result<Option<PackageMetadata>, String> {
    let entries = read_supplied_artifact_entries(artifact)?;
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

fn verify_supplied_artifact(
    artifact: &SuppliedVerifyArtifact,
    expected_files: &BTreeMap<String, Vec<u8>>,
    published_package_roots: &BTreeSet<String>,
    published_top_level_surface_files: &BTreeSet<String>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    match read_supplied_artifact_entries(artifact) {
        Ok(entries) => {
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

fn verify_external_checker(
    config: &ConfigHandle,
    out_root: &Path,
    checker: &str,
) -> Option<Diagnostic> {
    checker_diagnostic_from_output(
        checker,
        out_root,
        ProcessCommand::new(checker).arg(out_root).current_dir(&config.config_dir).output(),
    )
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

fn read_supplied_artifact_entries(
    artifact: &SuppliedVerifyArtifact,
) -> std::result::Result<BTreeMap<String, Vec<u8>>, String> {
    let path_text = artifact.path.to_string_lossy().to_ascii_lowercase();
    match artifact.kind {
        SuppliedArtifactKind::Wheel => {
            if !(path_text.ends_with(".whl") || path_text.ends_with(".zip")) {
                return Err(String::from("expected a .whl or .zip file"));
            }
            Ok(read_zip_entries(&artifact.path)?.into_iter().collect())
        }
        SuppliedArtifactKind::Sdist => {
            let entries = if path_text.ends_with(".tar.gz") || path_text.ends_with(".tgz") {
                read_tar_gz_entries(&artifact.path)?
            } else if path_text.ends_with(".zip") {
                read_zip_entries(&artifact.path)?
            } else {
                return Err(String::from("expected a .tar.gz, .tgz, or .zip file"));
            };
            Ok(strip_common_archive_root(entries))
        }
    }
}

fn read_zip_entries(path: &Path) -> std::result::Result<Vec<(String, Vec<u8>)>, String> {
    let file = fs::File::open(path).map_err(|error| format!("unable to open archive: {error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("unable to read zip archive: {error}"))?;
    let mut entries = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| format!("unable to read zip entry {index}: {error}"))?;
        if file.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| format!("unable to read zip entry `{}`: {error}", file.name()))?;
        entries.push((normalize_archive_path(file.name()), bytes));
    }

    Ok(entries)
}

fn read_tar_gz_entries(path: &Path) -> std::result::Result<Vec<(String, Vec<u8>)>, String> {
    let file = fs::File::open(path).map_err(|error| format!("unable to open archive: {error}"))?;
    let decoder = GzDecoder::new(file);
    let mut archive = TarArchive::new(decoder);
    let mut entries = Vec::new();

    for entry in
        archive.entries().map_err(|error| format!("unable to read tar archive: {error}"))?
    {
        let mut entry = entry.map_err(|error| format!("unable to read tar entry: {error}"))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let entry_path = entry
            .path()
            .map_err(|error| format!("unable to read tar entry path: {error}"))?
            .display()
            .to_string();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| format!("unable to read tar entry `{entry_path}`: {error}"))?;
        entries.push((normalize_archive_path(&entry_path), bytes));
    }

    Ok(entries)
}

fn strip_common_archive_root(entries: Vec<(String, Vec<u8>)>) -> BTreeMap<String, Vec<u8>> {
    let Some(common_root) = common_archive_root(&entries) else {
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

fn normalize_archive_path(path: &str) -> String {
    path.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
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

fn verify_emitted_declaration_surface(runtime_path: &Path, stub_path: &Path) -> Option<Diagnostic> {
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

    if runtime_surface == stub_surface {
        None
    } else {
        Some(Diagnostic::error(
            "TPY5003",
            format!(
                "emitted runtime/stub declaration surface differs between `{}` and `{}`",
                runtime_path.display(),
                stub_path.display()
            ),
        ))
    }
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
                if statement.owner_name.is_none()
                    && statement.annotation.is_none()
                    && statement.names == [String::from("__all__")] => {}
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
    matches!(
        statement,
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
            | typepython_syntax::SyntaxStatement::Unsafe(_)
    )
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

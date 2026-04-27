use std::{fs, path::Path, process::Command as ProcessCommand, process::ExitCode};

use anyhow::{Context, Result};
use serde::Serialize;
use typepython_diagnostics::{Diagnostic, DiagnosticReport};

use crate::{
    CommandSummary,
    cli::{OutputFormat, TypeHealthArgs},
    exit_code, load_project, print_summary,
};

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct TypeHealthReport {
    pub(crate) score: u8,
    pub(crate) packages: Vec<TypePackageHealth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lock_inputs: Option<TypeLockInputs>,
    pub(crate) lock_path: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct TypeLockInputs {
    pub(crate) target_python: String,
    pub(crate) analysis_python: String,
    pub(crate) typing_extensions_version: Option<String>,
    pub(crate) typeshed_commit: Option<String>,
    pub(crate) checker_versions: Vec<CheckerVersion>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct CheckerVersion {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct TypePackageHealth {
    pub(crate) name: String,
    pub(crate) root: String,
    pub(crate) has_py_typed: bool,
    pub(crate) is_stub_only: bool,
    pub(crate) is_partial_stub: bool,
    pub(crate) public_any_returns: usize,
    pub(crate) runtime_version: Option<String>,
    pub(crate) stub_version: Option<String>,
    pub(crate) stub_version_matches_runtime: Option<bool>,
}

pub(crate) fn run_type_health(args: TypeHealthArgs) -> Result<ExitCode> {
    let config = load_project(args.run.project.as_ref())?;
    let mut report =
        build_type_health_report(&config.config_dir, &config.config.resolution.type_roots)?;
    let lock_inputs = collect_type_lock_inputs(&config.config_dir, &config.config)?;
    if args.write_lock {
        let lock_path = config.config_dir.join(".typepython/type-lock.toml");
        write_type_lock(&lock_path, &report, &lock_inputs)?;
        report.lock_path = Some(lock_path.display().to_string());
    }
    report.lock_inputs = Some(lock_inputs);

    let mut diagnostics = DiagnosticReport::default();
    if let Some(threshold) = args.fail_under
        && report.score < threshold
    {
        diagnostics.push(Diagnostic::error(
            "TPY7002",
            format!("type health score {} is below required threshold {threshold}", report.score),
        ));
    }

    if args.run.format == OutputFormat::Json {
        let payload = serde_json::json!({ "summary": report, "diagnostics": diagnostics });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .context("unable to serialize type-health report")?
        );
    } else {
        print_type_health_text(&report);
        let summary = CommandSummary {
            command: String::from("type-health"),
            config_path: config.config_path.display().to_string(),
            config_source: config.source,
            discovered_sources: report.packages.len(),
            lowered_modules: 0,
            planned_artifacts: usize::from(args.write_lock),
            tracked_modules: report
                .packages
                .iter()
                .filter(|package| package.has_py_typed || package.is_stub_only)
                .count(),
            notes: vec![String::from(
                "inspected configured resolution.type_roots for PEP 561 typing metadata",
            )],
        };
        print_summary(args.run.format, &summary, &diagnostics)?;
    }
    Ok(exit_code(&diagnostics))
}

pub(crate) fn build_type_health_report(
    config_dir: &Path,
    type_roots: &[String],
) -> Result<TypeHealthReport> {
    let mut packages = Vec::new();
    for root in type_roots {
        let root_path = config_dir.join(root);
        if !root_path.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&root_path)
            .with_context(|| format!("unable to read {}", root_path.display()))?
        {
            let path = entry?.path();
            let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            if path.is_dir() && !file_name.ends_with(".dist-info") {
                packages.push(package_health(&path)?);
            }
        }
    }
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    let typed =
        packages.iter().filter(|package| package.has_py_typed || package.is_stub_only).count();
    let score = if packages.is_empty() { 100 } else { ((typed * 100) / packages.len()) as u8 };
    Ok(TypeHealthReport { score, packages, lock_inputs: None, lock_path: None })
}

pub(crate) fn collect_type_lock_inputs(
    config_dir: &Path,
    config: &typepython_config::Config,
) -> Result<TypeLockInputs> {
    Ok(TypeLockInputs {
        target_python: config.project.target_python_text.clone(),
        analysis_python: config
            .resolution
            .analysis_python_text
            .clone()
            .unwrap_or_else(|| config.project.target_python_text.clone()),
        typing_extensions_version: typing_extensions_version(
            config_dir,
            &config.resolution.type_roots,
        )?,
        typeshed_commit: bundled_typeshed_commit()?,
        checker_versions: ["mypy", "pyright", "ty"]
            .into_iter()
            .map(|checker| CheckerVersion {
                name: checker.to_owned(),
                version: checker_version(checker),
            })
            .collect(),
    })
}

fn typing_extensions_version(config_dir: &Path, type_roots: &[String]) -> Result<Option<String>> {
    for root in type_roots {
        let root_path = config_dir.join(root);
        let version = match distribution_version(&root_path, "typing-extensions")? {
            Some(version) => Some(version),
            None => distribution_version(&root_path, "typing_extensions")?,
        };
        if version.is_some() {
            return Ok(version);
        }
    }
    Ok(None)
}

fn bundled_typeshed_commit() -> Result<Option<String>> {
    let baseline_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/BASELINE.toml");
    let baseline = fs::read_to_string(&baseline_path)
        .with_context(|| format!("unable to read {}", baseline_path.display()))?;
    Ok(baseline.lines().find_map(|line| {
        let stripped = line.trim();
        stripped
            .strip_prefix("typeshed_commit = ")
            .and_then(|value| value.trim_matches('"').split_once('"').map(|(commit, _)| commit))
            .or_else(|| {
                stripped.strip_prefix("typeshed_commit = ").map(|value| value.trim_matches('"'))
            })
            .map(str::to_owned)
    }))
}

fn checker_version(checker: &str) -> Option<String> {
    let output = ProcessCommand::new(checker).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stdout.is_empty() { (!stderr.is_empty()).then_some(stderr) } else { Some(stdout) }
}

fn package_health(path: &Path) -> Result<TypePackageHealth> {
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let py_typed = path.join("py.typed");
    let marker = fs::read_to_string(&py_typed).unwrap_or_default();
    let package_name = file_name.trim_end_matches("-stubs").to_owned();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let runtime_version = distribution_version(parent, &package_name)?;
    let stub_version = distribution_version(parent, &format!("{package_name}-stubs"))?;
    let stub_version_matches_runtime =
        runtime_version.as_ref().zip(stub_version.as_ref()).map(|(runtime, stub)| runtime == stub);
    let public_any_returns = public_any_return_count(path)?;
    Ok(TypePackageHealth {
        name: package_name,
        root: path.display().to_string(),
        has_py_typed: py_typed.exists() && !file_name.ends_with("-stubs"),
        is_stub_only: file_name.ends_with("-stubs"),
        is_partial_stub: marker.lines().any(|line| line.trim() == "partial"),
        public_any_returns,
        runtime_version,
        stub_version,
        stub_version_matches_runtime,
    })
}

fn public_any_return_count(path: &Path) -> Result<usize> {
    let mut count = 0;
    collect_public_any_returns(path, &mut count)?;
    Ok(count)
}

fn collect_public_any_returns(path: &Path, count: &mut usize) -> Result<()> {
    if path.is_dir() {
        for entry in
            fs::read_dir(path).with_context(|| format!("unable to read {}", path.display()))?
        {
            collect_public_any_returns(&entry?.path(), count)?;
        }
        return Ok(());
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("pyi") {
        return Ok(());
    }
    let source =
        fs::read_to_string(path).with_context(|| format!("unable to read {}", path.display()))?;
    *count += source.lines().filter(|line| public_function_returns_any(line)).count();
    Ok(())
}

fn public_function_returns_any(line: &str) -> bool {
    let stripped = line.trim_start();
    let Some(signature) =
        stripped.strip_prefix("def ").or_else(|| stripped.strip_prefix("async def "))
    else {
        return false;
    };
    let Some((name, _)) = signature.split_once('(') else {
        return false;
    };
    if name.starts_with('_') {
        return false;
    }
    let Some((_, return_annotation)) = stripped.split_once("->") else {
        return false;
    };
    starts_with_any_annotation(return_annotation.trim_start())
}

fn starts_with_any_annotation(annotation: &str) -> bool {
    annotation.strip_prefix("Any").or_else(|| annotation.strip_prefix("typing.Any")).is_some_and(
        |rest| {
            rest.chars()
                .next()
                .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
        },
    )
}

fn distribution_version(site_root: &Path, distribution_name: &str) -> Result<Option<String>> {
    if !site_root.is_dir() {
        return Ok(None);
    }
    let normalized_distribution = distribution_name.replace('_', "-").to_ascii_lowercase();
    for entry in fs::read_dir(site_root)
        .with_context(|| format!("unable to read {}", site_root.display()))?
    {
        let path = entry?.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let normalized_name = file_name.replace('_', "-").to_ascii_lowercase();
        if !normalized_name.starts_with(&format!("{normalized_distribution}-"))
            || !normalized_name.ends_with(".dist-info")
        {
            continue;
        }
        let metadata_path = path.join("METADATA");
        let metadata = fs::read_to_string(&metadata_path)
            .with_context(|| format!("unable to read {}", metadata_path.display()))?;
        if let Some(version) = metadata.lines().find_map(|line| line.strip_prefix("Version: ")) {
            return Ok(Some(version.trim().to_owned()));
        }
    }
    Ok(None)
}

fn write_type_lock(path: &Path, report: &TypeHealthReport, inputs: &TypeLockInputs) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("unable to create {}", parent.display()))?;
    }
    let mut rendered = String::from("# Generated by typepython type-health --write-lock\n");
    rendered.push_str(&format!("score = {}\n\n", report.score));
    rendered.push_str("[inputs]\n");
    rendered.push_str(&format!("target_python = \"{}\"\n", toml_string(&inputs.target_python)));
    rendered.push_str(&format!("analysis_python = \"{}\"\n", toml_string(&inputs.analysis_python)));
    if let Some(version) = &inputs.typing_extensions_version {
        rendered.push_str(&format!("typing_extensions_version = \"{}\"\n", toml_string(version)));
    }
    if let Some(commit) = &inputs.typeshed_commit {
        rendered.push_str(&format!("typeshed_commit = \"{}\"\n", toml_string(commit)));
    }
    rendered.push('\n');
    for checker in &inputs.checker_versions {
        rendered.push_str("[[checker]]\n");
        rendered.push_str(&format!("name = \"{}\"\n", toml_string(&checker.name)));
        if let Some(version) = &checker.version {
            rendered.push_str(&format!("version = \"{}\"\n", toml_string(version)));
        }
        rendered.push('\n');
    }
    for package in &report.packages {
        rendered.push_str("[[package]]\n");
        rendered.push_str(&format!("name = \"{}\"\n", toml_string(&package.name)));
        rendered.push_str(&format!("root = \"{}\"\n", toml_string(&package.root)));
        rendered.push_str(&format!("has_py_typed = {}\n", package.has_py_typed));
        rendered.push_str(&format!("is_stub_only = {}\n", package.is_stub_only));
        rendered.push_str(&format!("is_partial_stub = {}\n", package.is_partial_stub));
        rendered.push_str(&format!("public_any_returns = {}\n", package.public_any_returns));
        if let Some(version) = &package.runtime_version {
            rendered.push_str(&format!("runtime_version = \"{}\"\n", toml_string(version)));
        }
        if let Some(version) = &package.stub_version {
            rendered.push_str(&format!("stub_version = \"{}\"\n", toml_string(version)));
        }
        if let Some(matches) = package.stub_version_matches_runtime {
            rendered.push_str(&format!("stub_version_matches_runtime = {matches}\n"));
        }
        rendered.push('\n');
    }
    fs::write(path, rendered).with_context(|| format!("unable to write {}", path.display()))
}

fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn print_type_health_text(report: &TypeHealthReport) {
    println!("type-health:");
    println!("  score: {}", report.score);
    for package in &report.packages {
        println!(
            "  package: {} py.typed={} stub_only={} partial={} public_any_returns={} runtime_version={} stub_version={} version_match={}",
            package.name,
            package.has_py_typed,
            package.is_stub_only,
            package.is_partial_stub,
            package.public_any_returns,
            package.runtime_version.as_deref().unwrap_or("?"),
            package.stub_version.as_deref().unwrap_or("?"),
            package
                .stub_version_matches_runtime
                .map(|matches| matches.to_string())
                .unwrap_or_else(|| String::from("?"))
        );
    }
    if let Some(lock_path) = &report.lock_path {
        println!("  lock: {lock_path}");
    }
}

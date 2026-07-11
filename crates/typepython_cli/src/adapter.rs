use std::{collections::BTreeSet, fs, path::Path, process::ExitCode};

use anyhow::{Context, Result};
use pep440_rs::Version;
use serde::{Deserialize, Serialize};
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_target::PythonTarget;

use crate::{
    CLI_JSON_SCHEMA_VERSION,
    cli::{AdapterArgs, AdapterCommand, AdapterValidateArgs, OutputFormat},
};

#[derive(Debug, Deserialize)]
struct AdapterManifest {
    adapter: AdapterMetadata,
    #[serde(default)]
    transforms: Vec<AdapterTransform>,
    #[serde(default)]
    golden_tests: Vec<AdapterGoldenTest>,
}

#[derive(Debug, Deserialize)]
struct AdapterMetadata {
    name: String,
    version: String,
    framework: String,
    typepython_min: String,
    #[serde(default)]
    python_targets: Vec<String>,
    stability: String,
}

#[derive(Debug, Deserialize)]
struct AdapterTransform {
    provider: String,
    kind: String,
    target: String,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    field_collector: Option<String>,
    #[serde(default)]
    constructor: Option<String>,
    #[serde(default)]
    fallback: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AdapterGoldenTest {
    name: String,
    input: String,
    expected_py: String,
    expected_pyi: String,
    #[serde(default)]
    checkers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AdapterValidationSummary {
    command: String,
    manifest: String,
    adapter: String,
    transforms: usize,
    golden_tests: usize,
}

pub(crate) fn run_adapter(args: AdapterArgs) -> Result<ExitCode> {
    match args.command {
        AdapterCommand::Validate(args) => run_adapter_validate(args),
    }
}

pub(crate) fn run_adapter_validate(args: AdapterValidateArgs) -> Result<ExitCode> {
    let contents = fs::read_to_string(&args.manifest)
        .with_context(|| format!("unable to read adapter manifest {}", args.manifest.display()))?;
    let manifest: AdapterManifest = toml::from_str(&contents)
        .with_context(|| format!("unable to parse adapter manifest {}", args.manifest.display()))?;
    let diagnostics = validate_adapter_manifest(&manifest, args.manifest.parent());
    let summary = AdapterValidationSummary {
        command: String::from("adapter validate"),
        manifest: args.manifest.display().to_string(),
        adapter: manifest.adapter.name.clone(),
        transforms: manifest.transforms.len(),
        golden_tests: manifest.golden_tests.len(),
    };
    print_adapter_validation_report(args.format, &summary, &diagnostics)?;
    Ok(if diagnostics.has_errors() { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn validate_adapter_manifest(
    manifest: &AdapterManifest,
    base_dir: Option<&Path>,
) -> DiagnosticReport {
    let mut report = DiagnosticReport::default();
    if manifest.adapter.name.trim().is_empty() {
        report.push(Diagnostic::error("TPY7003", "adapter manifest is missing adapter.name"));
    }
    for (field, value) in [
        ("adapter.version", &manifest.adapter.version),
        ("adapter.framework", &manifest.adapter.framework),
        ("adapter.typepython_min", &manifest.adapter.typepython_min),
    ] {
        if value.trim().is_empty() {
            report
                .push(Diagnostic::error("TPY7003", format!("adapter manifest is missing {field}")));
        }
    }
    validate_adapter_version("adapter.version", &manifest.adapter.version, &mut report);
    validate_adapter_version(
        "adapter.typepython_min",
        &manifest.adapter.typepython_min,
        &mut report,
    );
    if manifest.adapter.stability.trim().is_empty() {
        report.push(Diagnostic::error("TPY7003", "adapter manifest is missing adapter.stability"));
    } else if manifest.adapter.stability != "prototype" {
        report.push(Diagnostic::error(
            "TPY7003",
            format!(
                "adapter.stability `{}` is unsupported; the adapter SDK currently accepts only `prototype`",
                manifest.adapter.stability
            ),
        ));
    }
    if manifest.adapter.python_targets.is_empty() {
        report.push(Diagnostic::error(
            "TPY7003",
            "adapter manifest must list at least one adapter.python_targets entry",
        ));
    } else {
        let mut seen_targets = BTreeSet::new();
        for target in &manifest.adapter.python_targets {
            let parsed = target.parse::<PythonTarget>().ok();
            let supported = parsed.is_some_and(|target| {
                matches!(
                    target,
                    PythonTarget::PYTHON_3_10
                        | PythonTarget::PYTHON_3_11
                        | PythonTarget::PYTHON_3_12
                        | PythonTarget::PYTHON_3_13
                        | PythonTarget::PYTHON_3_14
                )
            });
            if !supported {
                report.push(Diagnostic::error(
                    "TPY7003",
                    format!(
                        "adapter.python_targets entry `{target}` is unsupported; expected one of `3.10`, `3.11`, `3.12`, `3.13`, or `3.14`"
                    ),
                ));
            } else if !seen_targets.insert(parsed.expect("supported target must parse")) {
                report.push(Diagnostic::error(
                    "TPY7003",
                    format!("adapter.python_targets contains duplicate target `{target}`"),
                ));
            }
        }
    }
    if manifest.transforms.is_empty() {
        report.push(Diagnostic::error(
            "TPY7003",
            "adapter manifest must declare at least one transform",
        ));
    }
    if manifest.golden_tests.is_empty() {
        report.push(Diagnostic::error(
            "TPY7003",
            "adapter manifest must declare at least one golden test",
        ));
    }
    for transform in &manifest.transforms {
        validate_transform(transform, &mut report);
    }
    for golden in &manifest.golden_tests {
        validate_golden_test(golden, base_dir, &mut report);
    }
    report
}

fn validate_adapter_version(field: &str, value: &str, report: &mut DiagnosticReport) {
    if !value.trim().is_empty() && value.parse::<Version>().is_err() {
        report.push(Diagnostic::error(
            "TPY7003",
            format!("adapter manifest {field} `{value}` is not a valid PEP 440 version"),
        ));
    }
}

#[cfg(test)]
pub(crate) fn adapter_validation_diagnostics(
    contents: &str,
    base_dir: Option<&Path>,
) -> DiagnosticReport {
    let manifest: AdapterManifest =
        toml::from_str(contents).expect("adapter test manifest should parse");
    validate_adapter_manifest(&manifest, base_dir)
}

fn validate_transform(transform: &AdapterTransform, report: &mut DiagnosticReport) {
    let allowed_kinds = BTreeSet::from([
        "class_decorator",
        "base_class",
        "metaclass",
        "function_to_object_decorator",
        "function_decorator",
    ]);
    let allowed_targets = BTreeSet::from(["class", "function", "method"]);
    let allowed_capabilities = BTreeSet::from([
        "field_collection",
        "constructor_generation",
        "alias_handling",
        "required_optional_fields",
        "readonly_fields",
        "descriptor_backed_attributes",
        "method_synthesis",
        "function_to_object_replacement",
        "generic_preservation",
        "taint_source",
        "taint_sink",
        "taint_sanitizer",
        "validator_witness",
        "effect_unsafe",
        "effect_io_fs",
        "effect_io_net",
        "effect_io_proc",
        "effect_time",
        "effect_random",
        "effect_runtime_validation",
        "effect_taint_sanitize",
    ]);
    if transform.provider.trim().is_empty() {
        report.push(Diagnostic::error("TPY7003", "adapter transform is missing provider"));
    }
    if !allowed_kinds.contains(transform.kind.as_str()) {
        report.push(Diagnostic::error(
            "TPY7003",
            format!(
                "adapter transform `{}` uses unsupported kind `{}`",
                transform.provider, transform.kind
            ),
        ));
    }
    if !allowed_targets.contains(transform.target.as_str()) {
        report.push(Diagnostic::error(
            "TPY7003",
            format!(
                "adapter transform `{}` uses unsupported target `{}`",
                transform.provider, transform.target
            ),
        ));
    }
    for capability in &transform.capabilities {
        if !allowed_capabilities.contains(capability.as_str()) {
            report.push(Diagnostic::error(
                "TPY7003",
                format!(
                    "adapter transform `{}` uses unsupported capability `{capability}`",
                    transform.provider
                ),
            ));
        }
    }
    if transform.kind == "function_to_object_decorator"
        && !transform
            .capabilities
            .iter()
            .any(|capability| capability == "function_to_object_replacement")
    {
        report.push(Diagnostic::error(
            "TPY7003",
            format!(
                "adapter transform `{}` must declare function_to_object_replacement",
                transform.provider
            ),
        ));
    }
    if matches!(transform.kind.as_str(), "class_decorator" | "base_class" | "metaclass")
        && !(transform.capabilities.iter().any(|capability| capability == "field_collection")
            && transform
                .capabilities
                .iter()
                .any(|capability| capability == "constructor_generation"))
    {
        report.push(Diagnostic::error(
            "TPY7003",
            format!(
                "adapter transform `{}` must declare field_collection and constructor_generation",
                transform.provider
            ),
        ));
    }
    if let Some(field_collector) = &transform.field_collector
        && field_collector != "annotated_class_fields"
    {
        report.push(Diagnostic::error(
            "TPY7003",
            format!(
                "adapter transform `{}` uses unsupported field_collector `{field_collector}`",
                transform.provider
            ),
        ));
    }
    if let Some(constructor) = &transform.constructor
        && constructor != "fields"
    {
        report.push(Diagnostic::error(
            "TPY7003",
            format!(
                "adapter transform `{}` uses unsupported constructor `{constructor}`",
                transform.provider
            ),
        ));
    }
    if let Some(fallback) = &transform.fallback
        && !matches!(fallback.as_str(), "strict_diagnostic" | "non_strict_degrade")
    {
        report.push(Diagnostic::error(
            "TPY7003",
            format!(
                "adapter transform `{}` uses unsupported fallback `{fallback}`",
                transform.provider
            ),
        ));
    }
}

fn validate_golden_test(
    golden: &AdapterGoldenTest,
    base_dir: Option<&Path>,
    report: &mut DiagnosticReport,
) {
    if golden.name.trim().is_empty() {
        report.push(Diagnostic::error("TPY7003", "adapter golden test is missing name"));
    }
    for (label, path) in [
        ("input", &golden.input),
        ("expected_py", &golden.expected_py),
        ("expected_pyi", &golden.expected_pyi),
    ] {
        if path.trim().is_empty() {
            report.push(Diagnostic::error(
                "TPY7003",
                format!("adapter golden test `{}` is missing {label}", golden.name),
            ));
            continue;
        }
        let candidate =
            base_dir.map_or_else(|| Path::new(path).to_path_buf(), |base| base.join(path));
        if !candidate.is_file() {
            report.push(Diagnostic::error(
                "TPY7003",
                format!(
                    "adapter golden test `{}` {label} does not exist: {}",
                    golden.name,
                    candidate.display()
                ),
            ));
        }
    }
    if golden.checkers.is_empty() {
        report.push(Diagnostic::error(
            "TPY7003",
            format!(
                "adapter golden test `{}` must name at least one downstream checker",
                golden.name
            ),
        ));
    }
    for checker in &golden.checkers {
        if !matches!(checker.as_str(), "basedpyright" | "mypy" | "pyright" | "ty") {
            report.push(Diagnostic::error(
                "TPY7003",
                format!(
                    "adapter golden test `{}` names unsupported checker `{checker}`",
                    golden.name
                ),
            ));
        }
    }
}

fn print_adapter_validation_report(
    format: OutputFormat,
    summary: &AdapterValidationSummary,
    diagnostics: &DiagnosticReport,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("adapter validate:");
            println!("  manifest: {}", summary.manifest);
            println!("  adapter: {}", summary.adapter);
            println!("  transforms: {}", summary.transforms);
            println!("  golden tests: {}", summary.golden_tests);
            if diagnostics.is_empty() {
                println!("  status: ok");
            } else {
                print!("{}", diagnostics.as_text());
            }
        }
        OutputFormat::Json => {
            let payload = serde_json::json!({
                "schema_version": CLI_JSON_SCHEMA_VERSION,
                "summary": summary,
                "diagnostics": diagnostics,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload)
                    .context("unable to serialize adapter validation report as JSON")?
            );
        }
    }
    Ok(())
}

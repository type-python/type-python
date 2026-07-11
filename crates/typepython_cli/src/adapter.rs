use std::{collections::BTreeSet, fs, path::Path, process::ExitCode};

use anyhow::{Context, Result};
use pep440_rs::Version;
use ruff_python_parser::parse_expression;
use serde::{Deserialize, Serialize};
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_target::PythonTarget;
use unicode_ident::{is_xid_continue, is_xid_start};

use crate::{
    CLI_JSON_SCHEMA_VERSION,
    cli::{AdapterArgs, AdapterCommand, AdapterValidateArgs, OutputFormat},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterManifest {
    adapter: AdapterMetadata,
    #[serde(default)]
    transforms: Vec<AdapterTransform>,
    #[serde(default)]
    golden_tests: Vec<AdapterGoldenTest>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
    alias: Option<AdapterAliasRule>,
    #[serde(default)]
    default: Option<AdapterKeywordRule>,
    #[serde(default)]
    default_factory: Option<AdapterKeywordRule>,
    #[serde(default)]
    frozen: Option<AdapterFrozenRule>,
    #[serde(default)]
    replacement_type: Option<String>,
    #[serde(default)]
    preserve_paramspec: Option<bool>,
    #[serde(default)]
    preserve_return_type: Option<bool>,
    #[serde(default)]
    fallback: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterAliasRule {
    source: String,
    keyword: String,
    literal_only: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterKeywordRule {
    keyword: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterFrozenRule {
    model_keyword: String,
    field_keyword: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
    let contents = match fs::read_to_string(&args.manifest) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            let summary = invalid_adapter_summary(&args.manifest);
            let diagnostics = adapter_schema_error_diagnostics(format!(
                "manifest text is not valid UTF-8: {error}"
            ));
            print_adapter_validation_report(args.format, &summary, &diagnostics)?;
            return Ok(ExitCode::FAILURE);
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!("unable to read adapter manifest {}", args.manifest.display())
            });
        }
    };
    let (summary, diagnostics) = match toml::from_str::<AdapterManifest>(&contents) {
        Ok(manifest) => {
            let diagnostics = validate_adapter_manifest(&manifest, args.manifest.parent());
            let summary = AdapterValidationSummary {
                command: String::from("adapter validate"),
                manifest: args.manifest.display().to_string(),
                adapter: manifest.adapter.name.clone(),
                transforms: manifest.transforms.len(),
                golden_tests: manifest.golden_tests.len(),
            };
            (summary, diagnostics)
        }
        Err(error) => {
            let summary = invalid_adapter_summary(&args.manifest);
            (summary, adapter_schema_diagnostics(&error))
        }
    };
    print_adapter_validation_report(args.format, &summary, &diagnostics)?;
    Ok(if diagnostics.has_errors() { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn invalid_adapter_summary(path: &Path) -> AdapterValidationSummary {
    AdapterValidationSummary {
        command: String::from("adapter validate"),
        manifest: path.display().to_string(),
        adapter: String::from("<invalid>"),
        transforms: 0,
        golden_tests: 0,
    }
}

fn adapter_schema_diagnostics(error: &toml::de::Error) -> DiagnosticReport {
    adapter_schema_error_diagnostics(error.to_string())
}

fn adapter_schema_error_diagnostics(detail: impl AsRef<str>) -> DiagnosticReport {
    let mut report = DiagnosticReport::default();
    report.push(Diagnostic::error(
        "TPY7003",
        format!("adapter manifest has invalid TOML or schema: {}", detail.as_ref()),
    ));
    report
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
    match toml::from_str::<AdapterManifest>(contents) {
        Ok(manifest) => validate_adapter_manifest(&manifest, base_dir),
        Err(error) => adapter_schema_diagnostics(&error),
    }
}

#[cfg(test)]
pub(crate) fn adapter_manifest_schema_diagnostics(contents: &str) -> DiagnosticReport {
    match toml::from_str::<AdapterManifest>(contents) {
        Ok(_) => DiagnosticReport::default(),
        Err(error) => adapter_schema_diagnostics(&error),
    }
}

fn validate_transform(transform: &AdapterTransform, report: &mut DiagnosticReport) {
    const ALLOWED_KINDS: [&str; 5] = [
        "class_decorator",
        "base_class",
        "metaclass",
        "function_to_object_decorator",
        "function_decorator",
    ];
    const ALLOWED_TARGETS: [&str; 3] = ["class", "function", "method"];
    const ALLOWED_CAPABILITIES: [&str; 21] = [
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
    ];
    if transform.provider.trim().is_empty() {
        report.push(Diagnostic::error("TPY7003", "adapter transform is missing provider"));
    }
    if !ALLOWED_KINDS.contains(&transform.kind.as_str()) {
        report.push(Diagnostic::error(
            "TPY7003",
            format!(
                "adapter transform `{}` uses unsupported kind `{}`",
                transform.provider, transform.kind
            ),
        ));
    }
    if !ALLOWED_TARGETS.contains(&transform.target.as_str()) {
        report.push(Diagnostic::error(
            "TPY7003",
            format!(
                "adapter transform `{}` uses unsupported target `{}`",
                transform.provider, transform.target
            ),
        ));
    }
    let mut seen_capabilities = BTreeSet::new();
    for capability in &transform.capabilities {
        if !ALLOWED_CAPABILITIES.contains(&capability.as_str()) {
            report.push(Diagnostic::error(
                "TPY7003",
                format!(
                    "adapter transform `{}` uses unsupported capability `{capability}`",
                    transform.provider
                ),
            ));
        } else if !seen_capabilities.insert(capability) {
            report.push(Diagnostic::error(
                "TPY7003",
                format!(
                    "adapter transform `{}` contains duplicate capability `{capability}`",
                    transform.provider
                ),
            ));
        }
    }

    let class_transform =
        matches!(transform.kind.as_str(), "class_decorator" | "base_class" | "metaclass");
    let function_transform =
        matches!(transform.kind.as_str(), "function_decorator" | "function_to_object_decorator");
    if class_transform && transform.target != "class" {
        report.push(adapter_transform_error(
            transform,
            format!(
                "kind `{}` requires target `class`, not `{}`",
                transform.kind, transform.target
            ),
        ));
    }
    if function_transform && !matches!(transform.target.as_str(), "function" | "method") {
        report.push(adapter_transform_error(
            transform,
            format!(
                "kind `{}` requires target `function` or `method`, not `{}`",
                transform.kind, transform.target
            ),
        ));
    }
    if !class_transform {
        for capability in [
            "field_collection",
            "constructor_generation",
            "alias_handling",
            "required_optional_fields",
            "readonly_fields",
            "descriptor_backed_attributes",
            "method_synthesis",
        ] {
            if transform_has_capability(transform, capability) {
                report.push(adapter_transform_error(
                    transform,
                    format!("capability `{capability}` requires a class transform kind"),
                ));
            }
        }
    }

    validate_class_transform_fields(transform, class_transform, report);
    validate_function_to_object_fields(transform, report);

    match transform.fallback.as_deref() {
        None => report.push(adapter_transform_error(
            transform,
            "must declare fallback as `strict_diagnostic` or `non_strict_degrade`",
        )),
        Some("strict_diagnostic" | "non_strict_degrade") => {}
        Some(fallback) => report.push(adapter_transform_error(
            transform,
            format!("uses unsupported fallback `{fallback}`"),
        )),
    }
}

fn validate_class_transform_fields(
    transform: &AdapterTransform,
    class_transform: bool,
    report: &mut DiagnosticReport,
) {
    if class_transform {
        if !(transform_has_capability(transform, "field_collection")
            && transform_has_capability(transform, "constructor_generation"))
        {
            report.push(adapter_transform_error(
                transform,
                "must declare field_collection and constructor_generation",
            ));
        }
        match transform.field_collector.as_deref() {
            None => report.push(adapter_transform_error(
                transform,
                "must declare field_collector `annotated_class_fields`",
            )),
            Some("annotated_class_fields") => {}
            Some(field_collector) => report.push(adapter_transform_error(
                transform,
                format!("uses unsupported field_collector `{field_collector}`"),
            )),
        }
        match transform.constructor.as_deref() {
            None => {
                report.push(adapter_transform_error(transform, "must declare constructor `fields`"))
            }
            Some("fields") => {}
            Some(constructor) => report.push(adapter_transform_error(
                transform,
                format!("uses unsupported constructor `{constructor}`"),
            )),
        }
    } else {
        for (field, present) in [
            ("field_collector", transform.field_collector.is_some()),
            ("constructor", transform.constructor.is_some()),
            ("alias", transform.alias.is_some()),
            ("default", transform.default.is_some()),
            ("default_factory", transform.default_factory.is_some()),
            ("frozen", transform.frozen.is_some()),
        ] {
            if present {
                report.push(adapter_transform_error(
                    transform,
                    format!("field `{field}` is valid only for class transforms"),
                ));
            }
        }
    }

    if let Some(alias) = &transform.alias {
        if !transform_has_capability(transform, "alias_handling") {
            report.push(adapter_transform_error(
                transform,
                "field `alias` requires capability `alias_handling`",
            ));
        }
        if alias.source != "field_specifier" {
            report.push(adapter_transform_error(
                transform,
                format!(
                    "alias.source `{}` is unsupported; expected `field_specifier`",
                    alias.source
                ),
            ));
        }
        validate_keyword(transform, "alias.keyword", &alias.keyword, report);
        if !alias.literal_only {
            report.push(adapter_transform_error(
                transform,
                "alias.literal_only must be true; dynamic aliases are outside the prototype safety boundary",
            ));
        }
    }
    if let Some(default) = &transform.default {
        if !transform_has_capability(transform, "required_optional_fields") {
            report.push(adapter_transform_error(
                transform,
                "field `default` requires capability `required_optional_fields`",
            ));
        }
        validate_keyword(transform, "default.keyword", &default.keyword, report);
    }
    if let Some(default_factory) = &transform.default_factory {
        if !transform_has_capability(transform, "required_optional_fields") {
            report.push(adapter_transform_error(
                transform,
                "field `default_factory` requires capability `required_optional_fields`",
            ));
        }
        validate_keyword(transform, "default_factory.keyword", &default_factory.keyword, report);
    }
    if let Some(frozen) = &transform.frozen {
        if !transform_has_capability(transform, "readonly_fields") {
            report.push(adapter_transform_error(
                transform,
                "field `frozen` requires capability `readonly_fields`",
            ));
        }
        validate_keyword(transform, "frozen.model_keyword", &frozen.model_keyword, report);
        validate_keyword(transform, "frozen.field_keyword", &frozen.field_keyword, report);
    }
}

fn validate_function_to_object_fields(transform: &AdapterTransform, report: &mut DiagnosticReport) {
    let function_to_object = transform.kind == "function_to_object_decorator";
    let replacement_capability =
        transform_has_capability(transform, "function_to_object_replacement");
    let generic_capability = transform_has_capability(transform, "generic_preservation");
    if function_to_object {
        if !replacement_capability {
            report.push(adapter_transform_error(
                transform,
                "must declare capability `function_to_object_replacement`",
            ));
        }
        match transform.replacement_type.as_deref() {
            None => {
                report.push(adapter_transform_error(transform, "must declare a replacement_type"))
            }
            Some(replacement_type) if !valid_replacement_type(replacement_type) => {
                report.push(adapter_transform_error(
                    transform,
                    format!(
                        "replacement_type `{replacement_type}` is not a valid TypePython type expression"
                    ),
                ));
            }
            Some(_) => {}
        }
    } else {
        if replacement_capability {
            report.push(adapter_transform_error(
                transform,
                "capability `function_to_object_replacement` requires kind `function_to_object_decorator`",
            ));
        }
        if generic_capability {
            report.push(adapter_transform_error(
                transform,
                "capability `generic_preservation` requires kind `function_to_object_decorator`",
            ));
        }
        for (field, present) in [
            ("replacement_type", transform.replacement_type.is_some()),
            ("preserve_paramspec", transform.preserve_paramspec.is_some()),
            ("preserve_return_type", transform.preserve_return_type.is_some()),
        ] {
            if present {
                report.push(adapter_transform_error(
                    transform,
                    format!("field `{field}` requires kind `function_to_object_decorator`"),
                ));
            }
        }
    }

    for (field, value) in [
        ("preserve_paramspec", transform.preserve_paramspec),
        ("preserve_return_type", transform.preserve_return_type),
    ] {
        if generic_capability && value != Some(true) {
            report.push(adapter_transform_error(
                transform,
                format!("capability `generic_preservation` requires {field} = true"),
            ));
        } else if !generic_capability && value.is_some() {
            report.push(adapter_transform_error(
                transform,
                format!("field `{field}` requires capability `generic_preservation`"),
            ));
        }
    }
}

fn transform_has_capability(transform: &AdapterTransform, capability: &str) -> bool {
    transform.capabilities.iter().any(|candidate| candidate == capability)
}

fn adapter_transform_error(transform: &AdapterTransform, detail: impl AsRef<str>) -> Diagnostic {
    Diagnostic::error(
        "TPY7003",
        format!("adapter transform `{}` {}", transform.provider, detail.as_ref()),
    )
}

fn validate_keyword(
    transform: &AdapterTransform,
    field: &str,
    keyword: &str,
    report: &mut DiagnosticReport,
) {
    if !valid_python_identifier(keyword) {
        report.push(adapter_transform_error(
            transform,
            format!("{field} `{keyword}` is not a valid Python identifier"),
        ));
    }
}

fn valid_python_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || is_xid_start(first))
        && characters.all(|character| character == '_' || is_xid_continue(character))
        && !matches!(
            value,
            "False"
                | "None"
                | "True"
                | "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
        )
}

fn valid_replacement_type(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && parse_expression(value).is_ok()
        && typepython_syntax::TypeExpr::parse(value).is_some()
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

//! Output planning boundary for TypePython.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use ruff_python_ast::{Expr, Stmt};
use ruff_python_parser::parse_module;
use ruff_text_size::Ranged;
use typepython_config::ConfigHandle;
use typepython_lowering::LoweredModule;
use typepython_syntax::{FunctionParam, MethodKind, SourceKind};

/// Planned runtime and stub artifacts for one source module.
#[derive(Debug, Clone)]
pub struct EmitArtifact {
    /// Original source file.
    pub source_path: PathBuf,
    /// Planned `.py` output, if any.
    pub runtime_path: Option<PathBuf>,
    /// Planned `.pyi` output, if any.
    pub stub_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PlannedModuleSource {
    pub source_path: PathBuf,
    pub source_kind: SourceKind,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct RuntimeWriteSummary {
    pub runtime_files_written: usize,
    pub stub_files_written: usize,
    pub py_typed_written: usize,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct TypePythonStubContext {
    pub value_overrides: Vec<StubValueOverride>,
    pub callable_overrides: Vec<StubCallableOverride>,
    pub synthetic_methods: Vec<StubSyntheticMethod>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StubValueOverride {
    pub line: usize,
    pub annotation: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StubCallableOverride {
    pub line: usize,
    pub params: Vec<FunctionParam>,
    pub returns: Option<String>,
    pub use_async_syntax: bool,
    pub drop_non_builtin_decorators: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StubSyntheticMethod {
    pub class_line: usize,
    pub name: String,
    pub method_kind: MethodKind,
    pub params: Vec<FunctionParam>,
    pub returns: Option<String>,
}

/// Generated stub flavor for inferred pass-through Python surfaces.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InferredStubMode {
    /// Internal cache-only stubs used as a typing surface for local `.py` files.
    Shadow,
    /// User-facing migration stubs with TODO markers for manual refinement.
    Migration,
}

/// Plans output paths for the provided modules.
#[must_use]
pub fn plan_emits(config: &ConfigHandle, modules: &[LoweredModule]) -> Vec<EmitArtifact> {
    let sources: Vec<_> = modules
        .iter()
        .map(|module| PlannedModuleSource {
            source_path: module.source_path.clone(),
            source_kind: module.source_kind,
        })
        .collect();
    plan_emits_for_sources(config, &sources)
}

/// Plans output paths for the provided source descriptors.
#[must_use]
pub fn plan_emits_for_sources(
    config: &ConfigHandle,
    sources: &[PlannedModuleSource],
) -> Vec<EmitArtifact> {
    sources
        .iter()
        .map(|source| {
            let relative = relative_module_path(config, &source.source_path);
            let out_root = config.resolve_relative_path(&config.config.project.out_dir);

            match source.source_kind {
                SourceKind::TypePython => EmitArtifact {
                    source_path: source.source_path.clone(),
                    runtime_path: Some(out_root.join(&relative).with_extension("py")),
                    stub_path: config
                        .config
                        .emit
                        .emit_pyi
                        .then(|| out_root.join(relative).with_extension("pyi")),
                },
                SourceKind::Python => EmitArtifact {
                    source_path: source.source_path.clone(),
                    runtime_path: Some(out_root.join(relative)),
                    stub_path: None,
                },
                SourceKind::Stub => EmitArtifact {
                    source_path: source.source_path.clone(),
                    runtime_path: None,
                    stub_path: Some(out_root.join(relative)),
                },
            }
        })
        .collect()
}

pub fn write_runtime_outputs(
    artifacts: &[EmitArtifact],
    modules: &[LoweredModule],
    runtime_validators: bool,
    stub_contexts: Option<&BTreeMap<PathBuf, TypePythonStubContext>>,
) -> Result<RuntimeWriteSummary, io::Error> {
    let modules_by_source: BTreeMap<_, _> =
        modules.iter().map(|module| (module.source_path.as_path(), module)).collect();
    let mut runtime_files_written = 0usize;
    let mut stub_files_written = 0usize;
    let mut package_roots = std::collections::BTreeSet::new();

    for artifact in artifacts {
        let Some(module) = modules_by_source.get(artifact.source_path.as_path()) else {
            continue;
        };

        if let Some(runtime_path) = &artifact.runtime_path {
            if let Some(parent) = runtime_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let runtime_source =
                if runtime_validators && module.source_kind == SourceKind::TypePython {
                    inject_runtime_validators(&module.python_source)?
                } else {
                    module.python_source.clone()
                };
            fs::write(runtime_path, runtime_source)?;
            if runtime_path.file_name().is_some_and(|name| name == "__init__.py") {
                if let Some(parent) = runtime_path.parent() {
                    package_roots.insert(parent.to_path_buf());
                }
            }
            runtime_files_written += 1;
        }

        if let Some(stub_path) = &artifact.stub_path {
            if let Some(parent) = stub_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let stub_source = if module.source_kind == SourceKind::TypePython {
                let context = stub_contexts
                    .and_then(|contexts| contexts.get(&module.source_path))
                    .cloned()
                    .unwrap_or_default();
                generate_typepython_stub_source(module, &context).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "TPY5001: unable to generate `.pyi` for `{}`: {}",
                            module.source_path.display(),
                            error
                        ),
                    )
                })?
            } else {
                module.python_source.clone()
            };
            fs::write(stub_path, stub_source)?;
            if is_package_init_path(stub_path) {
                if let Some(parent) = stub_path.parent() {
                    package_roots.insert(parent.to_path_buf());
                }
            }
            stub_files_written += 1;
        }
    }

    let mut py_typed_written = 0usize;
    for package_root in package_roots {
        fs::write(package_root.join("py.typed"), "")?;
        py_typed_written += 1;
    }

    Ok(RuntimeWriteSummary { runtime_files_written, stub_files_written, py_typed_written })
}

fn is_package_init_path(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "__init__.py" || name == "__init__.pyi")
}

fn inject_runtime_validators(python: &str) -> Result<String, io::Error> {
    let typed_dicts = collect_typed_dict_fields(python);
    let edits = collect_runtime_validator_edits(python, &typed_dicts);
    if edits.is_empty() {
        return Ok(python.to_owned());
    }

    let lines: Vec<&str> = python.lines().collect();
    let mut output = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let line_number = index + 1;
        output.push((*line).to_owned());
        for edit in edits.iter().filter(|edit| edit.after_line == line_number) {
            output.extend(edit.insertion.iter().cloned());
        }
    }

    let mut rewritten = output.join("\n");
    if python.ends_with('\n') {
        rewritten.push('\n');
    }
    Ok(rewritten)
}

#[derive(Debug)]
struct RuntimeValidatorEdit {
    after_line: usize,
    insertion: Vec<String>,
}

#[derive(Debug, Clone)]
struct ValidatorField {
    name: String,
    annotation: String,
    has_default: bool,
}

fn collect_runtime_validator_edits(
    python: &str,
    typed_dicts: &BTreeMap<String, Vec<ValidatorField>>,
) -> Vec<RuntimeValidatorEdit> {
    let lines: Vec<&str> = python.lines().collect();
    let mut edits = Vec::new();
    let mut index = 0usize;

    while index + 1 < lines.len() {
        if lines[index].trim() != "@dataclass" {
            index += 1;
            continue;
        }
        let header_line = lines[index + 1];
        let trimmed_header = header_line.trim_start();
        if !trimmed_header.starts_with("class ") {
            index += 1;
            continue;
        }

        let class_indent = header_line.len() - trimmed_header.len();
        let class_name = trimmed_header
            .strip_prefix("class ")
            .and_then(|rest| rest.split(['(', ':']).next())
            .map(str::trim)
            .filter(|name| !name.is_empty());
        let Some(class_name) = class_name else {
            index += 1;
            continue;
        };

        let body_indent = class_indent + 4;
        let mut fields = Vec::new();
        let mut body_end = index + 1;
        let mut cursor = index + 2;
        while cursor < lines.len() {
            let line = lines[cursor];
            let trimmed = line.trim_start();
            if !trimmed.is_empty() {
                let indent = line.len() - trimmed.len();
                if indent <= class_indent {
                    break;
                }
                body_end = cursor;
                if indent == body_indent
                    && !trimmed.starts_with('@')
                    && !trimmed.starts_with("def ")
                    && !trimmed.starts_with("class ")
                    && trimmed.contains(':')
                {
                    if let Some(field) = parse_validator_field(trimmed) {
                        fields.push(field);
                    }
                }
            }
            cursor += 1;
        }

        if !fields.is_empty() {
            edits.push(RuntimeValidatorEdit {
                after_line: body_end + 1,
                insertion: build_validator_lines(class_indent, class_name, &fields, typed_dicts),
            });
        }
        index = cursor;
    }

    edits
}

fn parse_validator_field(trimmed: &str) -> Option<ValidatorField> {
    let (name, rest) = trimmed.split_once(':')?;
    let name = name.trim();
    if name.is_empty() || name == "pass" {
        return None;
    }
    let (annotation, has_default) = match rest.split_once('=') {
        Some((annotation, _)) => (annotation.trim(), true),
        None => (rest.trim(), false),
    };
    Some(ValidatorField { name: name.to_owned(), annotation: annotation.to_owned(), has_default })
}

fn build_validator_lines(
    class_indent: usize,
    class_name: &str,
    fields: &[ValidatorField],
    typed_dicts: &BTreeMap<String, Vec<ValidatorField>>,
) -> Vec<String> {
    let method_indent = " ".repeat(class_indent + 4);
    let body_indent = " ".repeat(class_indent + 8);
    let nested_indent = " ".repeat(class_indent + 12);
    let mut lines = vec![
        String::new(),
        format!("{method_indent}@classmethod"),
        format!("{method_indent}def __tpy_validate__(cls, __data: dict) -> \"{class_name}\":"),
        format!("{body_indent}if not isinstance(__data, dict):"),
        format!(
            "{nested_indent}raise TypeError(\"field `<root>` expected dict but got \" + type(__data).__name__)"
        ),
    ];

    for field in fields {
        let variable = format!("__tpy_{}", field.name);
        if field.has_default {
            lines.push(format!("{body_indent}if \"{}\" in __data:", field.name));
            lines.push(format!("{nested_indent}{variable} = __data[\"{}\"]", field.name));
            lines.extend(emit_validation_lines(
                &variable,
                &field.annotation,
                &field.name,
                typed_dicts,
                class_indent + 12,
            ));
            lines.push(format!("{body_indent}else:"));
            lines.push(format!("{nested_indent}{variable} = getattr(cls, \"{}\")", field.name));
        } else {
            lines.push(format!("{body_indent}if \"{}\" not in __data:", field.name));
            lines.push(format!(
                "{nested_indent}raise TypeError(\"field `{}' expected {} but got missing\")",
                field.name, field.annotation
            ));
            lines.push(format!("{body_indent}{variable} = __data[\"{}\"]", field.name));
            lines.extend(emit_validation_lines(
                &variable,
                &field.annotation,
                &field.name,
                typed_dicts,
                class_indent + 8,
            ));
        }
    }

    lines.push(format!(
        "{body_indent}return cls({})",
        fields
            .iter()
            .map(|field| format!("{}=__tpy_{}", field.name, field.name))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    lines
}

fn emit_validation_lines(
    variable: &str,
    annotation: &str,
    field_path: &str,
    typed_dicts: &BTreeMap<String, Vec<ValidatorField>>,
    indent: usize,
) -> Vec<String> {
    let indent = " ".repeat(indent);
    let annotation = annotation.trim();
    if annotation.is_empty() {
        return Vec::new();
    }

    if let Some(inner) = strip_wrapper(annotation, "NotRequired")
        .or_else(|| strip_wrapper(annotation, "Required_"))
        .or_else(|| strip_wrapper(annotation, "ReadOnly"))
        .or_else(|| strip_wrapper(annotation, "Mutable"))
    {
        return emit_validation_lines(variable, inner, field_path, typed_dicts, indent.len());
    }

    let union_parts = split_top_level(annotation, '|');
    if union_parts.len() > 1 {
        let checks = union_parts
            .iter()
            .filter_map(|part| runtime_check_expression(variable, part, typed_dicts))
            .collect::<Vec<_>>();
        if checks.is_empty() {
            return Vec::new();
        }
        return vec![format!(
            "{indent}if not ({}):\n{}    raise TypeError(\"field `{}' expected {} but got \" + type({}).__name__)",
            checks.join(" or "),
            indent,
            field_path,
            annotation,
            variable
        )];
    }

    if let Some(fields) = typed_dicts.get(annotation) {
        let nested = " ".repeat(indent.len() + 4);
        let mut lines = vec![format!(
            "{indent}if not isinstance({variable}, dict):\n{nested}raise TypeError(\"field `{}' expected {} but got \" + type({}).__name__)",
            field_path, annotation, variable
        )];
        for field in fields {
            let nested_var = format!("{variable}[\"{}\"]", field.name);
            let nested_path = format!("{}.{}", field_path, field.name);
            if field.has_default || field.annotation.starts_with("NotRequired[") {
                lines.push(format!("{indent}if \"{}\" in {variable}:", field.name));
                lines.extend(emit_validation_lines(
                    &nested_var,
                    &field.annotation,
                    &nested_path,
                    typed_dicts,
                    indent.len() + 4,
                ));
            } else {
                lines.push(format!("{indent}if \"{}\" not in {variable}:", field.name));
                lines.push(format!(
                    "{nested}raise TypeError(\"field `{}' expected {} but got missing\")",
                    nested_path, field.annotation
                ));
                lines.extend(emit_validation_lines(
                    &nested_var,
                    &field.annotation,
                    &nested_path,
                    typed_dicts,
                    indent.len(),
                ));
            }
        }
        return lines;
    }

    let Some(check) = runtime_check_expression(variable, annotation, typed_dicts) else {
        return Vec::new();
    };
    vec![format!(
        "{indent}if not ({}):\n{}    raise TypeError(\"field `{}' expected {} but got \" + type({}).__name__)",
        check, indent, field_path, annotation, variable
    )]
}

fn runtime_check_expression(
    variable: &str,
    annotation: &str,
    typed_dicts: &BTreeMap<String, Vec<ValidatorField>>,
) -> Option<String> {
    let annotation = annotation.trim();
    if annotation.is_empty() {
        return None;
    }
    if let Some(inner) = strip_wrapper(annotation, "NotRequired")
        .or_else(|| strip_wrapper(annotation, "Required_"))
        .or_else(|| strip_wrapper(annotation, "ReadOnly"))
        .or_else(|| strip_wrapper(annotation, "Mutable"))
    {
        return runtime_check_expression(variable, inner, typed_dicts);
    }
    if annotation == "None" {
        return Some(format!("{variable} is None"));
    }
    if matches!(
        annotation,
        "int" | "float" | "str" | "bytes" | "bool" | "list" | "dict" | "set" | "tuple"
    ) {
        return Some(format!("isinstance({variable}, {annotation})"));
    }
    if let Some((head, _)) = annotation.split_once('[') {
        let head = head.trim();
        if matches!(head, "list" | "dict" | "set" | "tuple") {
            return Some(format!("isinstance({variable}, {head})"));
        }
    }
    if typed_dicts.contains_key(annotation) {
        return Some(format!("isinstance({variable}, dict)"));
    }
    if annotation.chars().all(|character| character.is_ascii_alphanumeric() || character == '_') {
        return Some(format!("isinstance({variable}, {annotation})"));
    }
    None
}

fn strip_wrapper<'a>(annotation: &'a str, wrapper: &str) -> Option<&'a str> {
    let inner = annotation.strip_prefix(wrapper)?.strip_prefix('[')?.strip_suffix(']')?;
    Some(inner.trim())
}

fn split_top_level(annotation: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in annotation.char_indices() {
        match character {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = depth.saturating_sub(1),
            _ if character == delimiter && depth == 0 => {
                parts.push(annotation[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(annotation[start..].trim());
    parts
}

fn collect_typed_dict_fields(python: &str) -> BTreeMap<String, Vec<ValidatorField>> {
    let lines: Vec<&str> = python.lines().collect();
    let mut typed_dicts = BTreeMap::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        if !trimmed.starts_with("class ") || !trimmed.contains("TypedDict") {
            index += 1;
            continue;
        }
        let class_indent = line.len() - trimmed.len();
        let Some(class_name) = trimmed
            .strip_prefix("class ")
            .and_then(|rest| rest.split(['(', ':']).next())
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            index += 1;
            continue;
        };
        let body_indent = class_indent + 4;
        let mut fields = Vec::new();
        let mut cursor = index + 1;
        while cursor < lines.len() {
            let body_line = lines[cursor];
            let body_trimmed = body_line.trim_start();
            if !body_trimmed.is_empty() {
                let indent = body_line.len() - body_trimmed.len();
                if indent <= class_indent {
                    break;
                }
                if indent == body_indent && body_trimmed.contains(':') {
                    if let Some(field) = parse_validator_field(body_trimmed) {
                        fields.push(field);
                    }
                }
            }
            cursor += 1;
        }
        typed_dicts.insert(class_name.to_owned(), fields);
        index = cursor;
    }
    typed_dicts
}

#[allow(dead_code)]
fn rewrite_to_stub_source(python: &str) -> Result<String, io::Error> {
    let module = LoweredModule {
        source_path: PathBuf::new(),
        source_kind: SourceKind::TypePython,
        python_source: python.to_owned(),
        source_map: Vec::new(),
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };
    generate_typepython_stub_source(&module, &TypePythonStubContext::default())
}

pub fn generate_typepython_stub_source(
    module: &LoweredModule,
    context: &TypePythonStubContext,
) -> Result<String, io::Error> {
    let python = &module.python_source;
    let parsed = parse_module(python).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unable to parse lowered Python for stub emission: {}", error.error),
        )
    })?;

    let lowered_context = LoweredStubContext::from_module(module, context);
    let mut edits = Vec::new();
    collect_authoritative_stub_edits(python, parsed.suite(), &lowered_context, &mut edits);
    edits.sort_by_key(|edit| edit.start_line);

    let lines: Vec<&str> = python.lines().collect();
    let mut output = Vec::new();
    let mut line = 1usize;
    let mut edits = edits.into_iter().peekable();

    while line <= lines.len() {
        if let Some(edit) = edits.peek() {
            if edit.start_line == line {
                if let Some(replacement) = &edit.replacement {
                    output.push(replacement.clone());
                }
                line = edit.end_line + 1;
                edits.next();
                continue;
            }
        }

        output.push(lines[line - 1].to_owned());
        line += 1;
    }

    let mut rewritten = output.join("\n");
    if python.ends_with('\n') {
        rewritten.push('\n');
    }
    Ok(rewritten)
}

/// Generates a best-effort `.pyi` surface for a pass-through Python module.
pub fn generate_inferred_stub_source(
    python: &str,
    mode: InferredStubMode,
) -> Result<String, io::Error> {
    let parsed = parse_module(python).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unable to parse Python source for inferred stub emission: {}", error.error),
        )
    })?;

    let context = StubInferenceContext::from_suite(python, parsed.suite());
    let mut edits = Vec::new();
    collect_inferred_stub_edits(python, parsed.suite(), &context, mode, &mut edits);
    edits.sort_by_key(|edit| edit.start_line);

    let lines: Vec<&str> = python.lines().collect();
    let mut output = Vec::new();
    let mut line = 1usize;
    let mut edits = edits.into_iter().peekable();

    while line <= lines.len() {
        if let Some(edit) = edits.peek() {
            if edit.start_line == line {
                if let Some(replacement) = &edit.replacement {
                    output.push(replacement.clone());
                }
                line = edit.end_line + 1;
                edits.next();
                continue;
            }
        }

        output.push(lines[line - 1].to_owned());
        line += 1;
    }

    let mut rewritten = output.join("\n");
    if mode == InferredStubMode::Migration {
        rewritten = if rewritten.is_empty() {
            String::from("# auto-generated by typepython migrate\n")
        } else {
            format!("# auto-generated by typepython migrate\n\n{rewritten}")
        };
    }
    if python.ends_with('\n') {
        rewritten.push('\n');
    }
    Ok(rewritten)
}

#[derive(Debug, Clone, Default)]
struct StubInferenceContext {
    class_names: std::collections::BTreeSet<String>,
    value_bindings: BTreeMap<String, String>,
    instance_attributes: BTreeMap<String, String>,
    receiver_name: Option<String>,
}

impl StubInferenceContext {
    fn from_suite(source: &str, suite: &[Stmt]) -> Self {
        let class_names = suite
            .iter()
            .filter_map(|statement| match statement {
                Stmt::ClassDef(class_def) => Some(class_def.name.as_str().to_owned()),
                _ => None,
            })
            .collect();
        let mut context = Self {
            class_names,
            value_bindings: BTreeMap::new(),
            instance_attributes: BTreeMap::new(),
            receiver_name: None,
        };
        collect_name_bindings_from_suite(source, suite, &mut context);
        context
    }

    fn with_instance_attributes(&self, instance_attributes: BTreeMap<String, String>) -> Self {
        let mut derived = self.clone();
        derived.instance_attributes = instance_attributes;
        derived
    }

    fn with_receiver_name(&self, receiver_name: Option<&str>) -> Self {
        let mut derived = self.clone();
        derived.receiver_name = receiver_name.map(str::to_owned);
        derived
    }
}

fn collect_name_bindings_from_suite(
    source: &str,
    suite: &[Stmt],
    context: &mut StubInferenceContext,
) {
    for statement in suite {
        collect_name_bindings_from_statement(source, statement, context);
    }
}

fn collect_name_bindings_from_statement(
    source: &str,
    statement: &Stmt,
    context: &mut StubInferenceContext,
) {
    match statement {
        Stmt::Assign(assign) => {
            let Some(inferred) = infer_expr_type(&assign.value, context) else {
                return;
            };
            for target in &assign.targets {
                if let Expr::Name(name) = target {
                    context.value_bindings.insert(name.id.as_str().to_owned(), inferred.clone());
                } else if let Some(receiver_name) = context.receiver_name.as_deref()
                    && let Some(attribute) = self_attribute_name(target, receiver_name)
                {
                    context.instance_attributes.insert(attribute.to_owned(), inferred.clone());
                }
            }
        }
        Stmt::AnnAssign(assign) => {
            let Some(annotation) =
                slice_range(source, assign.annotation.range()).map(str::to_owned)
            else {
                return;
            };
            if let Expr::Name(name) = assign.target.as_ref() {
                context.value_bindings.insert(name.id.as_str().to_owned(), annotation);
            } else if let Some(receiver_name) = context.receiver_name.as_deref()
                && let Some(attribute) = self_attribute_name(assign.target.as_ref(), receiver_name)
            {
                context.instance_attributes.insert(attribute.to_owned(), annotation);
            }
        }
        _ => {}
    }
}

fn collect_inferred_stub_edits(
    source: &str,
    suite: &[Stmt],
    context: &StubInferenceContext,
    mode: InferredStubMode,
    edits: &mut Vec<StubEdit>,
) {
    for statement in suite {
        match statement {
            Stmt::FunctionDef(function) => {
                let start_line = offset_to_line(source, function.name.range.start().to_usize());
                let end_offset = function.range.end().to_usize().saturating_sub(1);
                let end_line =
                    offset_to_line(source, end_offset.max(function.range.start().to_usize()));
                edits.push(StubEdit {
                    start_line,
                    end_line,
                    replacement: Some(render_function_stub(source, function, false, context, mode)),
                });
            }
            Stmt::Assign(assign) => {
                let start_line = offset_to_line(source, assign.range.start().to_usize());
                let end_offset = assign.range.end().to_usize().saturating_sub(1);
                let end_line =
                    offset_to_line(source, end_offset.max(assign.range.start().to_usize()));
                if let Some(replacement) =
                    render_assignment_stub(source, &assign.targets, &assign.value, context, mode)
                {
                    edits.push(StubEdit { start_line, end_line, replacement: Some(replacement) });
                }
            }
            Stmt::AnnAssign(assign) => {
                let start_line = offset_to_line(source, assign.range.start().to_usize());
                let end_offset = assign.range.end().to_usize().saturating_sub(1);
                let end_line =
                    offset_to_line(source, end_offset.max(assign.range.start().to_usize()));
                if let Some(replacement) = render_annotated_assignment_stub(source, assign) {
                    edits.push(StubEdit { start_line, end_line, replacement: Some(replacement) });
                }
            }
            Stmt::ClassDef(class_def) => {
                let start_line = offset_to_line(source, class_def.name.range.start().to_usize());
                let end_offset = class_def.range.end().to_usize().saturating_sub(1);
                let end_line =
                    offset_to_line(source, end_offset.max(class_def.range.start().to_usize()));
                edits.push(StubEdit {
                    start_line,
                    end_line,
                    replacement: Some(render_class_stub(source, class_def, context, mode)),
                });
            }
            _ => {}
        }
    }
}

fn render_function_stub(
    source: &str,
    function: &ruff_python_ast::StmtFunctionDef,
    in_class: bool,
    context: &StubInferenceContext,
    mode: InferredStubMode,
) -> String {
    render_function_stub_parts(
        source,
        function.name.as_str(),
        &function.parameters,
        function.returns.as_deref(),
        &function.body,
        in_class,
        function.is_async,
        is_static_method(&function.decorator_list),
        context,
        mode,
        function.name.range.start().to_usize(),
    )
}

fn render_function_stub_parts(
    source: &str,
    name: &str,
    parameters: &ruff_python_ast::Parameters,
    returns: Option<&Expr>,
    body: &[Stmt],
    in_class: bool,
    is_async: bool,
    is_static_method: bool,
    context: &StubInferenceContext,
    mode: InferredStubMode,
    start_offset: usize,
) -> String {
    let def_line = offset_to_line(source, start_offset);
    let indent = leading_indent(source.lines().nth(def_line.saturating_sub(1)).unwrap_or(""));
    let (params, used_placeholder) =
        render_parameter_list(source, parameters, in_class, is_static_method, context, mode);
    let (return_annotation, missing_return) = match returns
        .and_then(|annotation| slice_range(source, annotation.range()))
        .map(str::to_owned)
    {
        Some(annotation) => (annotation, false),
        None => {
            let inferred = if is_async {
                None
            } else {
                infer_function_return_type(
                    source,
                    parameters,
                    body,
                    in_class,
                    is_static_method,
                    context,
                )
            };
            match inferred {
                Some(annotation) if annotation == missing_type_annotation(mode) => {
                    (annotation, true)
                }
                Some(annotation) => (annotation, false),
                None => (missing_type_annotation(mode).to_owned(), true),
            }
        }
    };
    let mut lines = Vec::new();
    if mode == InferredStubMode::Migration && (used_placeholder || missing_return) {
        lines.push(format!("{indent}# TODO: add type annotation"));
    }
    lines.push(format!(
        "{indent}{}def {}({}) -> {}: ...",
        if is_async { "async " } else { "" },
        name,
        params,
        return_annotation
    ));
    lines.join("\n")
}

fn render_parameter_list(
    source: &str,
    parameters: &ruff_python_ast::Parameters,
    in_class: bool,
    is_static_method: bool,
    context: &StubInferenceContext,
    mode: InferredStubMode,
) -> (String, bool) {
    let mut parts = Vec::new();
    let mut missing = false;
    let mut parameter_index = 0usize;

    for parameter in &parameters.posonlyargs {
        let (text, parameter_missing) = render_parameter(
            source,
            parameter.name().as_str(),
            parameter.annotation(),
            parameter.default(),
            if parameter_index == 0 && in_class && !is_static_method {
                ReceiverKind::Implicit
            } else {
                ReceiverKind::None
            },
            ParameterPrefix::None,
            context,
            mode,
        );
        parts.push(text);
        missing |= parameter_missing;
        parameter_index += 1;
    }
    if !parameters.posonlyargs.is_empty() {
        parts.push(String::from("/"));
    }

    for parameter in &parameters.args {
        let (text, parameter_missing) = render_parameter(
            source,
            parameter.name().as_str(),
            parameter.annotation(),
            parameter.default(),
            if parameter_index == 0 && in_class && !is_static_method {
                ReceiverKind::Implicit
            } else {
                ReceiverKind::None
            },
            ParameterPrefix::None,
            context,
            mode,
        );
        parts.push(text);
        missing |= parameter_missing;
        parameter_index += 1;
    }

    if let Some(parameter) = parameters.vararg.as_ref() {
        let (text, parameter_missing) = render_parameter(
            source,
            parameter.name().as_str(),
            parameter.annotation(),
            None,
            ReceiverKind::None,
            ParameterPrefix::Variadic,
            context,
            mode,
        );
        parts.push(text);
        missing |= parameter_missing;
    } else if !parameters.kwonlyargs.is_empty() {
        parts.push(String::from("*"));
    }

    for parameter in &parameters.kwonlyargs {
        let (text, parameter_missing) = render_parameter(
            source,
            parameter.name().as_str(),
            parameter.annotation(),
            parameter.default(),
            ReceiverKind::None,
            ParameterPrefix::None,
            context,
            mode,
        );
        parts.push(text);
        missing |= parameter_missing;
    }

    if let Some(parameter) = parameters.kwarg.as_ref() {
        let (text, parameter_missing) = render_parameter(
            source,
            parameter.name().as_str(),
            parameter.annotation(),
            None,
            ReceiverKind::None,
            ParameterPrefix::KeywordVariadic,
            context,
            mode,
        );
        parts.push(text);
        missing |= parameter_missing;
    }

    (parts.join(", "), missing)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ReceiverKind {
    None,
    Implicit,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ParameterPrefix {
    None,
    Variadic,
    KeywordVariadic,
}

fn render_parameter(
    source: &str,
    name: &str,
    annotation: Option<&Expr>,
    default: Option<&Expr>,
    receiver: ReceiverKind,
    prefix: ParameterPrefix,
    context: &StubInferenceContext,
    mode: InferredStubMode,
) -> (String, bool) {
    let prefix_text = match prefix {
        ParameterPrefix::None => "",
        ParameterPrefix::Variadic => "*",
        ParameterPrefix::KeywordVariadic => "**",
    };

    if receiver == ReceiverKind::Implicit && annotation.is_none() {
        return (
            format!("{prefix_text}{name}{}", if default.is_some() { " = ..." } else { "" }),
            false,
        );
    }

    let (annotation, missing) = match annotation
        .and_then(|annotation| slice_range(source, annotation.range()))
        .map(str::to_owned)
    {
        Some(annotation) => (annotation, false),
        None => match default.and_then(|value| infer_expr_type(value, context)) {
            Some(annotation) => (annotation, false),
            None => (missing_type_annotation(mode).to_owned(), true),
        },
    };

    (
        format!(
            "{prefix_text}{name}: {annotation}{}",
            if default.is_some() { " = ..." } else { "" }
        ),
        missing,
    )
}

fn render_assignment_stub(
    _source: &str,
    targets: &[Expr],
    value: &Expr,
    context: &StubInferenceContext,
    mode: InferredStubMode,
) -> Option<String> {
    let mut lines = Vec::new();
    let inferred = infer_expr_type(value, context);
    for target in targets {
        if let Expr::Name(name) = target {
            let missing =
                inferred.is_none() || inferred.as_deref() == Some(missing_type_annotation(mode));
            if mode == InferredStubMode::Migration && missing {
                lines.push(String::from("# TODO: add type annotation"));
            }
            lines.push(format!(
                "{}: {}",
                name.id.as_str(),
                inferred.clone().unwrap_or_else(|| missing_type_annotation(mode).to_owned())
            ));
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn render_annotated_assignment_stub(
    source: &str,
    assign: &ruff_python_ast::StmtAnnAssign,
) -> Option<String> {
    let Expr::Name(name) = assign.target.as_ref() else {
        return None;
    };
    let annotation = slice_range(source, assign.annotation.range())?;
    Some(format!("{}: {}", name.id.as_str(), annotation))
}

fn render_class_stub(
    source: &str,
    class_def: &ruff_python_ast::StmtClassDef,
    context: &StubInferenceContext,
    mode: InferredStubMode,
) -> String {
    let header =
        source_header_text(source, class_def.name.range.start().to_usize(), &class_def.body)
            .trim_end()
            .to_owned();
    let inferred_instance_attributes = infer_instance_attributes(source, class_def, context, mode);
    let mut body_lines = Vec::new();
    let mut known_instance_attributes =
        inferred_instance_attributes.iter().cloned().collect::<BTreeMap<_, _>>();
    let mut declared_names = std::collections::BTreeSet::new();

    for statement in &class_def.body {
        match statement {
            Stmt::AnnAssign(assign) => {
                let Expr::Name(name) = assign.target.as_ref() else {
                    continue;
                };
                let Some(annotation) = slice_range(source, assign.annotation.range()) else {
                    continue;
                };
                declared_names.insert(name.id.as_str().to_owned());
                known_instance_attributes
                    .insert(name.id.as_str().to_owned(), annotation.to_owned());
                body_lines.push(format!("    {}: {}", name.id.as_str(), annotation));
            }
            Stmt::Assign(assign) => {
                let inferred = infer_expr_type(&assign.value, context);
                for target in &assign.targets {
                    let Expr::Name(name) = target else {
                        continue;
                    };
                    declared_names.insert(name.id.as_str().to_owned());
                    if let Some(inferred) = &inferred {
                        known_instance_attributes
                            .insert(name.id.as_str().to_owned(), inferred.clone());
                    }
                    if mode == InferredStubMode::Migration
                        && (inferred.is_none()
                            || inferred.as_deref() == Some(missing_type_annotation(mode)))
                    {
                        body_lines.push(String::from("    # TODO: add type annotation"));
                    }
                    body_lines.push(format!(
                        "    {}: {}",
                        name.id.as_str(),
                        inferred
                            .clone()
                            .unwrap_or_else(|| missing_type_annotation(mode).to_owned())
                    ));
                }
            }
            Stmt::FunctionDef(function) => {
                let method_context =
                    context.with_instance_attributes(known_instance_attributes.clone());
                body_lines.push(render_method_with_decorators(
                    source,
                    function,
                    &method_context,
                    mode,
                ))
            }
            Stmt::ClassDef(nested) => {
                body_lines.push(render_class_stub(source, nested, context, mode))
            }
            _ => {}
        }
    }

    for (name, annotation) in inferred_instance_attributes {
        if declared_names.contains(&name) {
            continue;
        }
        if mode == InferredStubMode::Migration && annotation == missing_type_annotation(mode) {
            body_lines.push(String::from("    # TODO: add type annotation"));
        }
        body_lines.push(format!("    {name}: {annotation}"));
    }

    if body_lines.is_empty() {
        body_lines.push(String::from("    ..."));
    }

    format!("{header}\n{}", body_lines.join("\n"))
}

fn render_method_with_decorators(
    source: &str,
    function: &ruff_python_ast::StmtFunctionDef,
    context: &StubInferenceContext,
    mode: InferredStubMode,
) -> String {
    let mut parts =
        decorator_lines(source, &function.decorator_list, function.name.range.start().to_usize());
    parts.push(render_function_stub(source, function, true, context, mode));
    parts.join("\n")
}

fn decorator_lines(
    source: &str,
    decorators: &[ruff_python_ast::Decorator],
    start_offset: usize,
) -> Vec<String> {
    let Some(first) = decorators.first() else {
        return Vec::new();
    };
    let start_line = offset_to_line(source, first.expression.range().start().to_usize());
    let end_line = offset_to_line(source, start_offset).saturating_sub(1);
    if start_line > end_line {
        return Vec::new();
    }
    source
        .lines()
        .skip(start_line.saturating_sub(1))
        .take(end_line.saturating_sub(start_line) + 1)
        .map(str::to_owned)
        .collect()
}

fn is_static_method(decorators: &[ruff_python_ast::Decorator]) -> bool {
    decorators.iter().any(|decorator| match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "staticmethod",
        Expr::Attribute(attribute) => attribute.attr.as_str() == "staticmethod",
        _ => false,
    })
}

fn first_receiver_name(parameters: &ruff_python_ast::Parameters) -> &str {
    if let Some(parameter) = parameters.posonlyargs.first() {
        parameter.name().as_str()
    } else if let Some(parameter) = parameters.args.first() {
        parameter.name().as_str()
    } else {
        "self"
    }
}

fn leading_indent(line: &str) -> String {
    line.chars().take_while(|character| character.is_whitespace()).collect()
}

fn missing_type_annotation(mode: InferredStubMode) -> &'static str {
    match mode {
        InferredStubMode::Shadow => "unknown",
        InferredStubMode::Migration => "...",
    }
}

fn infer_function_return_type(
    source: &str,
    parameters: &ruff_python_ast::Parameters,
    body: &[Stmt],
    in_class: bool,
    is_static_method: bool,
    context: &StubInferenceContext,
) -> Option<String> {
    let mut inferred = Vec::new();
    let mut unresolved = false;
    let function_context =
        build_function_inference_context(source, parameters, in_class, is_static_method, context);
    collect_return_annotations(source, body, &function_context, &mut inferred, &mut unresolved);
    if unresolved {
        return None;
    }
    if inferred.is_empty() { Some(String::from("None")) } else { normalize_union_types(inferred) }
}

fn build_function_inference_context(
    source: &str,
    parameters: &ruff_python_ast::Parameters,
    in_class: bool,
    is_static_method: bool,
    context: &StubInferenceContext,
) -> StubInferenceContext {
    let receiver_name = (in_class && !is_static_method).then(|| first_receiver_name(parameters));
    let mut function_context = context.with_receiver_name(receiver_name);
    let mut parameter_index = 0usize;
    for parameter in &parameters.posonlyargs {
        bind_parameter_inference(
            source,
            parameter.name().as_str(),
            parameter.annotation(),
            parameter.default(),
            if parameter_index == 0 && in_class && !is_static_method {
                ReceiverKind::Implicit
            } else {
                ReceiverKind::None
            },
            ParameterPrefix::None,
            &mut function_context,
        );
        parameter_index += 1;
    }
    for parameter in &parameters.args {
        bind_parameter_inference(
            source,
            parameter.name().as_str(),
            parameter.annotation(),
            parameter.default(),
            if parameter_index == 0 && in_class && !is_static_method {
                ReceiverKind::Implicit
            } else {
                ReceiverKind::None
            },
            ParameterPrefix::None,
            &mut function_context,
        );
        parameter_index += 1;
    }
    if let Some(parameter) = parameters.vararg.as_ref() {
        bind_parameter_inference(
            source,
            parameter.name().as_str(),
            parameter.annotation(),
            None,
            ReceiverKind::None,
            ParameterPrefix::Variadic,
            &mut function_context,
        );
    }
    for parameter in &parameters.kwonlyargs {
        bind_parameter_inference(
            source,
            parameter.name().as_str(),
            parameter.annotation(),
            parameter.default(),
            ReceiverKind::None,
            ParameterPrefix::None,
            &mut function_context,
        );
    }
    if let Some(parameter) = parameters.kwarg.as_ref() {
        bind_parameter_inference(
            source,
            parameter.name().as_str(),
            parameter.annotation(),
            None,
            ReceiverKind::None,
            ParameterPrefix::KeywordVariadic,
            &mut function_context,
        );
    }
    function_context
}

fn bind_parameter_inference(
    source: &str,
    name: &str,
    annotation: Option<&Expr>,
    default: Option<&Expr>,
    receiver: ReceiverKind,
    prefix: ParameterPrefix,
    context: &mut StubInferenceContext,
) {
    let inferred = match annotation
        .and_then(|annotation| slice_range(source, annotation.range()))
        .map(str::to_owned)
    {
        Some(annotation) => Some(annotation),
        None if receiver == ReceiverKind::Implicit => None,
        None => default.and_then(|value| infer_expr_type(value, context)),
    };
    let Some(inferred) = inferred else {
        return;
    };
    let binding = match prefix {
        ParameterPrefix::None => inferred,
        ParameterPrefix::Variadic => format!("tuple[{inferred}, ...]"),
        ParameterPrefix::KeywordVariadic => format!("dict[str, {inferred}]"),
    };
    context.value_bindings.insert(name.to_owned(), binding);
}

fn collect_return_annotations(
    source: &str,
    suite: &[Stmt],
    context: &StubInferenceContext,
    inferred: &mut Vec<String>,
    unresolved: &mut bool,
) {
    let mut local_context = context.clone();
    for statement in suite {
        match statement {
            Stmt::Return(return_stmt) => match return_stmt.value.as_deref() {
                Some(value) => match infer_expr_type(value, &local_context) {
                    Some(annotation) => inferred.push(annotation),
                    None => *unresolved = true,
                },
                None => inferred.push(String::from("None")),
            },
            Stmt::Assign(_) | Stmt::AnnAssign(_) => {
                collect_name_bindings_from_statement(source, statement, &mut local_context)
            }
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            Stmt::If(if_stmt) => {
                collect_return_annotations(
                    source,
                    &if_stmt.body,
                    &local_context,
                    inferred,
                    unresolved,
                );
                for clause in &if_stmt.elif_else_clauses {
                    collect_return_annotations(
                        source,
                        &clause.body,
                        &local_context,
                        inferred,
                        unresolved,
                    );
                }
            }
            Stmt::Try(try_stmt) => {
                collect_return_annotations(
                    source,
                    &try_stmt.body,
                    &local_context,
                    inferred,
                    unresolved,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_return_annotations(
                        source,
                        &handler.body,
                        &local_context,
                        inferred,
                        unresolved,
                    );
                }
                collect_return_annotations(
                    source,
                    &try_stmt.orelse,
                    &local_context,
                    inferred,
                    unresolved,
                );
                collect_return_annotations(
                    source,
                    &try_stmt.finalbody,
                    &local_context,
                    inferred,
                    unresolved,
                );
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_return_annotations(
                        source,
                        &case.body,
                        &local_context,
                        inferred,
                        unresolved,
                    );
                }
            }
            Stmt::For(for_stmt) => {
                collect_return_annotations(
                    source,
                    &for_stmt.body,
                    &local_context,
                    inferred,
                    unresolved,
                );
                collect_return_annotations(
                    source,
                    &for_stmt.orelse,
                    &local_context,
                    inferred,
                    unresolved,
                );
            }
            Stmt::While(while_stmt) => {
                collect_return_annotations(
                    source,
                    &while_stmt.body,
                    &local_context,
                    inferred,
                    unresolved,
                );
                collect_return_annotations(
                    source,
                    &while_stmt.orelse,
                    &local_context,
                    inferred,
                    unresolved,
                );
            }
            Stmt::With(with_stmt) => collect_return_annotations(
                source,
                &with_stmt.body,
                &local_context,
                inferred,
                unresolved,
            ),
            _ => {}
        }
    }
}

fn infer_instance_attributes(
    source: &str,
    class_def: &ruff_python_ast::StmtClassDef,
    context: &StubInferenceContext,
    mode: InferredStubMode,
) -> Vec<(String, String)> {
    let mut attributes = BTreeMap::new();
    for statement in &class_def.body {
        let Stmt::FunctionDef(function) = statement else {
            continue;
        };
        if function.name.as_str() != "__init__" {
            continue;
        }
        let init_context = build_function_inference_context(
            source,
            &function.parameters,
            true,
            is_static_method(&function.decorator_list),
            context,
        );
        collect_instance_attribute_annotations(
            source,
            function.body.as_slice(),
            first_receiver_name(&function.parameters),
            &init_context,
            mode,
            &mut attributes,
        );
    }
    attributes.into_iter().collect()
}

fn collect_instance_attribute_annotations(
    source: &str,
    suite: &[Stmt],
    receiver_name: &str,
    context: &StubInferenceContext,
    mode: InferredStubMode,
    attributes: &mut BTreeMap<String, String>,
) {
    for statement in suite {
        match statement {
            Stmt::Assign(assign) => {
                let inferred = infer_expr_type(&assign.value, context)
                    .unwrap_or_else(|| missing_type_annotation(mode).to_owned());
                for target in &assign.targets {
                    if let Some(attribute) = self_attribute_name(target, receiver_name) {
                        merge_inferred_annotation(attributes, attribute, inferred.clone());
                    }
                }
            }
            Stmt::AnnAssign(assign) => {
                if let Some(attribute) = self_attribute_name(assign.target.as_ref(), receiver_name)
                {
                    let annotation = slice_range(source, assign.annotation.range())
                        .map(str::to_owned)
                        .unwrap_or_else(|| missing_type_annotation(mode).to_owned());
                    merge_inferred_annotation(attributes, attribute, annotation);
                }
            }
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            Stmt::If(if_stmt) => {
                collect_instance_attribute_annotations(
                    source,
                    &if_stmt.body,
                    receiver_name,
                    context,
                    mode,
                    attributes,
                );
                for clause in &if_stmt.elif_else_clauses {
                    collect_instance_attribute_annotations(
                        source,
                        &clause.body,
                        receiver_name,
                        context,
                        mode,
                        attributes,
                    );
                }
            }
            Stmt::Try(try_stmt) => {
                collect_instance_attribute_annotations(
                    source,
                    &try_stmt.body,
                    receiver_name,
                    context,
                    mode,
                    attributes,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_instance_attribute_annotations(
                        source,
                        &handler.body,
                        receiver_name,
                        context,
                        mode,
                        attributes,
                    );
                }
                collect_instance_attribute_annotations(
                    source,
                    &try_stmt.orelse,
                    receiver_name,
                    context,
                    mode,
                    attributes,
                );
                collect_instance_attribute_annotations(
                    source,
                    &try_stmt.finalbody,
                    receiver_name,
                    context,
                    mode,
                    attributes,
                );
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_instance_attribute_annotations(
                        source,
                        &case.body,
                        receiver_name,
                        context,
                        mode,
                        attributes,
                    );
                }
            }
            Stmt::For(for_stmt) => {
                collect_instance_attribute_annotations(
                    source,
                    &for_stmt.body,
                    receiver_name,
                    context,
                    mode,
                    attributes,
                );
                collect_instance_attribute_annotations(
                    source,
                    &for_stmt.orelse,
                    receiver_name,
                    context,
                    mode,
                    attributes,
                );
            }
            Stmt::While(while_stmt) => {
                collect_instance_attribute_annotations(
                    source,
                    &while_stmt.body,
                    receiver_name,
                    context,
                    mode,
                    attributes,
                );
                collect_instance_attribute_annotations(
                    source,
                    &while_stmt.orelse,
                    receiver_name,
                    context,
                    mode,
                    attributes,
                );
            }
            Stmt::With(with_stmt) => collect_instance_attribute_annotations(
                source,
                &with_stmt.body,
                receiver_name,
                context,
                mode,
                attributes,
            ),
            _ => {}
        }
    }
}

fn self_attribute_name<'a>(expr: &'a Expr, receiver_name: &str) -> Option<&'a str> {
    let Expr::Attribute(attribute) = expr else {
        return None;
    };
    let Expr::Name(name) = attribute.value.as_ref() else {
        return None;
    };
    (name.id.as_str() == receiver_name).then_some(attribute.attr.as_str())
}

fn merge_inferred_annotation(
    attributes: &mut BTreeMap<String, String>,
    attribute: &str,
    annotation: String,
) {
    match attributes.get(attribute) {
        Some(existing) if existing == &annotation => {}
        Some(existing) => {
            attributes.insert(
                attribute.to_owned(),
                normalize_union_types(vec![existing.clone(), annotation])
                    .unwrap_or_else(|| String::from("unknown")),
            );
        }
        None => {
            attributes.insert(attribute.to_owned(), annotation);
        }
    }
}

fn infer_expr_type(expr: &Expr, context: &StubInferenceContext) -> Option<String> {
    match expr {
        Expr::Name(name) => context.value_bindings.get(name.id.as_str()).cloned(),
        Expr::Attribute(attribute) => {
            let Expr::Name(name) = attribute.value.as_ref() else {
                return None;
            };
            if context.receiver_name.as_deref() == Some(name.id.as_str()) {
                return context.instance_attributes.get(attribute.attr.as_str()).cloned();
            }
            None
        }
        Expr::NumberLiteral(_) => Some(String::from("int")),
        Expr::StringLiteral(_) => Some(String::from("str")),
        Expr::BooleanLiteral(_) => Some(String::from("bool")),
        Expr::NoneLiteral(_) => Some(String::from("None")),
        Expr::Compare(_) => Some(String::from("bool")),
        Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::Not => {
            Some(String::from("bool"))
        }
        Expr::BoolOp(bool_op) => normalize_union_types(
            bool_op.values.iter().filter_map(|value| infer_expr_type(value, context)).collect(),
        ),
        Expr::BinOp(bin_op) => infer_binop_type(bin_op, context),
        Expr::List(list) => Some(format!(
            "list[{}]",
            normalize_union_types(
                list.elts.iter().filter_map(|value| infer_expr_type(value, context)).collect(),
            )
            .unwrap_or_else(|| String::from("unknown"))
        )),
        Expr::Tuple(tuple) => Some(if tuple.elts.is_empty() {
            String::from("tuple[()]")
        } else {
            format!(
                "tuple[{}]",
                tuple
                    .elts
                    .iter()
                    .map(|value| infer_expr_type(value, context)
                        .unwrap_or_else(|| String::from("unknown")))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }),
        Expr::Set(set) => Some(format!(
            "set[{}]",
            normalize_union_types(
                set.elts.iter().filter_map(|value| infer_expr_type(value, context)).collect(),
            )
            .unwrap_or_else(|| String::from("unknown"))
        )),
        Expr::Dict(dict) => {
            let mut keys = Vec::new();
            let mut values = Vec::new();
            for item in &dict.items {
                let Some(key) = item.key.as_ref() else {
                    return None;
                };
                keys.push(infer_expr_type(key, context).unwrap_or_else(|| String::from("unknown")));
                values.push(
                    infer_expr_type(&item.value, context)
                        .unwrap_or_else(|| String::from("unknown")),
                );
            }
            Some(format!(
                "dict[{}, {}]",
                normalize_union_types(keys).unwrap_or_else(|| String::from("unknown")),
                normalize_union_types(values).unwrap_or_else(|| String::from("unknown"))
            ))
        }
        Expr::Call(call) => infer_call_type(call, context),
        _ => None,
    }
}

fn infer_call_type(
    call: &ruff_python_ast::ExprCall,
    context: &StubInferenceContext,
) -> Option<String> {
    match call.func.as_ref() {
        Expr::Name(name) => match name.id.as_str() {
            "list" | "dict" | "set" | "tuple" | "int" | "float" | "str" | "bytes" | "bool" => {
                Some(name.id.as_str().to_owned())
            }
            other if context.class_names.contains(other) => Some(other.to_owned()),
            _ => None,
        },
        _ => None,
    }
}

fn infer_binop_type(
    bin_op: &ruff_python_ast::ExprBinOp,
    context: &StubInferenceContext,
) -> Option<String> {
    let left = infer_expr_type(&bin_op.left, context)?;
    let right = infer_expr_type(&bin_op.right, context)?;
    match bin_op.op {
        ruff_python_ast::Operator::Add => {
            if left == "str" && right == "str" {
                return Some(String::from("str"));
            }
            if is_numeric_type(&left) && is_numeric_type(&right) {
                return Some(join_numeric_type(&left, &right));
            }
            if let (Some((left_head, left_args)), Some((right_head, right_args))) =
                (split_generic_type(&left), split_generic_type(&right))
            {
                return match (left_head.as_str(), right_head.as_str()) {
                    ("list", "list") if left_args.len() == 1 && right_args.len() == 1 => {
                        Some(format!(
                            "list[{}]",
                            normalize_union_types(vec![
                                left_args[0].clone(),
                                right_args[0].clone()
                            ])
                            .unwrap_or_else(|| String::from("unknown"))
                        ))
                    }
                    ("tuple", "tuple") => {
                        let mut args = left_args;
                        args.extend(right_args);
                        Some(format!("tuple[{}]", args.join(", ")))
                    }
                    _ => None,
                };
            }
            None
        }
        ruff_python_ast::Operator::BitOr => {
            normalize_union_types(vec![left, right]).map(|annotation| format!("type[{annotation}]"))
        }
        _ => None,
    }
}

fn is_numeric_type(text: &str) -> bool {
    matches!(text, "int" | "float" | "complex")
}

fn join_numeric_type(left: &str, right: &str) -> String {
    if left == "complex" || right == "complex" {
        String::from("complex")
    } else if left == "float" || right == "float" {
        String::from("float")
    } else {
        String::from("int")
    }
}

fn split_generic_type(text: &str) -> Option<(String, Vec<String>)> {
    let (head, inner) = text.split_once('[')?;
    let inner = inner.strip_suffix(']')?;
    Some((head.to_owned(), inner.split(',').map(|part| part.trim().to_owned()).collect()))
}

fn normalize_union_types(types: Vec<String>) -> Option<String> {
    let mut unique = Vec::new();
    for value in types.into_iter().filter(|value| !value.is_empty()) {
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    if unique.is_empty() { None } else { Some(unique.join(" | ")) }
}

fn slice_range(source: &str, range: ruff_text_size::TextRange) -> Option<&str> {
    source.get(range.start().to_usize()..range.end().to_usize())
}

#[derive(Debug, Clone, Default)]
struct LoweredStubContext {
    value_overrides: BTreeMap<usize, String>,
    callable_overrides: BTreeMap<usize, LoweredCallableOverride>,
    synthetic_methods: BTreeMap<usize, Vec<StubSyntheticMethod>>,
}

#[derive(Debug, Clone)]
struct LoweredCallableOverride {
    params: Vec<FunctionParam>,
    returns: Option<String>,
    use_async_syntax: bool,
    drop_non_builtin_decorators: bool,
}

impl LoweredStubContext {
    fn from_module(module: &LoweredModule, context: &TypePythonStubContext) -> Self {
        let mut lowered = Self::default();
        for override_line in &context.value_overrides {
            lowered.value_overrides.insert(
                original_to_lowered_line(module, override_line.line),
                override_line.annotation.clone(),
            );
        }
        for override_line in &context.callable_overrides {
            lowered.callable_overrides.insert(
                original_to_lowered_line(module, override_line.line),
                LoweredCallableOverride {
                    params: override_line.params.clone(),
                    returns: override_line.returns.clone(),
                    use_async_syntax: override_line.use_async_syntax,
                    drop_non_builtin_decorators: override_line.drop_non_builtin_decorators,
                },
            );
        }
        for method in &context.synthetic_methods {
            lowered
                .synthetic_methods
                .entry(original_to_lowered_line(module, method.class_line))
                .or_default()
                .push(method.clone());
        }
        lowered
    }
}

fn original_to_lowered_line(module: &LoweredModule, original_line: usize) -> usize {
    module
        .source_map
        .iter()
        .find(|entry| entry.original_line == original_line)
        .map(|entry| entry.lowered_line)
        .unwrap_or(original_line)
}

#[derive(Debug)]
struct StubEdit {
    start_line: usize,
    end_line: usize,
    replacement: Option<String>,
}

fn collect_authoritative_stub_edits(
    source: &str,
    suite: &[Stmt],
    context: &LoweredStubContext,
    edits: &mut Vec<StubEdit>,
) {
    let overloaded_names: std::collections::BTreeSet<_> = suite
        .iter()
        .filter_map(|statement| match statement {
            Stmt::FunctionDef(function)
                if function.decorator_list.iter().any(is_overload_decorator) =>
            {
                Some(function.name.as_str().to_owned())
            }
            _ => None,
        })
        .collect();

    for statement in suite {
        match statement {
            Stmt::Import(_) | Stmt::ImportFrom(_) => {}
            Stmt::FunctionDef(function) => {
                let function_line = offset_to_line(source, function.name.range.start().to_usize());
                let start_line = function
                    .decorator_list
                    .first()
                    .map(|decorator| offset_to_line(source, decorator.range().start().to_usize()))
                    .unwrap_or(function_line);
                let end_offset = function.range.end().to_usize().saturating_sub(1);
                let end_line =
                    offset_to_line(source, end_offset.max(function.range.start().to_usize()));
                let replacement = if function.decorator_list.iter().any(is_overload_decorator) {
                    Some(render_authoritative_function_stub(source, function, context))
                } else if overloaded_names.contains(function.name.as_str()) {
                    None
                } else {
                    Some(render_authoritative_function_stub(source, function, context))
                };
                edits.push(StubEdit { start_line, end_line, replacement });
            }
            Stmt::AnnAssign(assign) => {
                let start_line = offset_to_line(source, assign.range.start().to_usize());
                let replacement = render_authoritative_annotated_assignment_stub(source, assign);
                edits.push(StubEdit { start_line, end_line: start_line, replacement });
            }
            Stmt::Assign(assign) => {
                let start_line = offset_to_line(source, assign.range.start().to_usize());
                let end_offset = assign.range.end().to_usize().saturating_sub(1);
                let end_line =
                    offset_to_line(source, end_offset.max(assign.range.start().to_usize()));
                let replacement = render_authoritative_assignment_stub(source, assign, context);
                edits.push(StubEdit { start_line, end_line, replacement });
            }
            Stmt::ClassDef(class_def) => {
                let class_line = offset_to_line(source, class_def.name.range.start().to_usize());
                let start_line = class_def
                    .decorator_list
                    .first()
                    .map(|decorator| offset_to_line(source, decorator.range().start().to_usize()))
                    .unwrap_or(class_line);
                let end_offset = class_def.range.end().to_usize().saturating_sub(1);
                let end_line =
                    offset_to_line(source, end_offset.max(class_def.range.start().to_usize()));
                edits.push(StubEdit {
                    start_line,
                    end_line,
                    replacement: Some(render_authoritative_class_stub(source, class_def, context)),
                });
            }
            Stmt::Expr(_) | Stmt::Pass(_) => {
                let start_line = offset_to_line(source, statement.range().start().to_usize());
                let end_offset = statement.range().end().to_usize().saturating_sub(1);
                let end_line =
                    offset_to_line(source, end_offset.max(statement.range().start().to_usize()));
                edits.push(StubEdit { start_line, end_line, replacement: None });
            }
            _ => {
                let start_line = offset_to_line(source, statement.range().start().to_usize());
                let end_offset = statement.range().end().to_usize().saturating_sub(1);
                let end_line =
                    offset_to_line(source, end_offset.max(statement.range().start().to_usize()));
                edits.push(StubEdit { start_line, end_line, replacement: None });
            }
        }
    }
}

fn render_authoritative_function_stub(
    source: &str,
    function: &ruff_python_ast::StmtFunctionDef,
    context: &LoweredStubContext,
) -> String {
    let function_line = offset_to_line(source, function.name.range.start().to_usize());
    if let Some(override_signature) = context.callable_overrides.get(&function_line) {
        let indentation =
            leading_indent(source.lines().nth(function_line.saturating_sub(1)).unwrap_or_default());
        let mut parts = if override_signature.drop_non_builtin_decorators {
            builtin_decorator_lines(source, &function.decorator_list, &indentation)
        } else {
            decorator_lines(
                source,
                &function.decorator_list,
                function.name.range.start().to_usize(),
            )
        };
        parts.push(format_function_stub_signature(
            &indentation,
            function.name.as_str(),
            &override_signature.params,
            override_signature.returns.as_deref(),
            override_signature.use_async_syntax,
        ));
        return parts.join("\n");
    }
    let mut parts =
        decorator_lines(source, &function.decorator_list, function.name.range.start().to_usize());
    parts.push(rewrite_stub_function_signature(source, function));
    parts.join("\n")
}

fn render_authoritative_class_stub(
    source: &str,
    class_def: &ruff_python_ast::StmtClassDef,
    context: &LoweredStubContext,
) -> String {
    let indentation = leading_indent(
        source
            .lines()
            .nth(offset_to_line(source, class_def.name.range.start().to_usize()).saturating_sub(1))
            .unwrap_or_default(),
    );
    let decorators = builtin_decorator_lines(source, &class_def.decorator_list, &indentation);
    let header =
        source_header_text(source, class_def.name.range.start().to_usize(), &class_def.body)
            .trim_end()
            .to_owned();
    let indent = format!("{}    ", indentation);
    let class_line = offset_to_line(source, class_def.name.range.start().to_usize());
    let mut body_lines = render_authoritative_class_body(source, class_def, context, &indent);

    if let Some(extra_methods) = context.synthetic_methods.get(&class_line) {
        for method in extra_methods {
            body_lines.push(render_synthetic_method_stub(method, &indent));
        }
    }

    let mut lines = decorators;
    if body_lines.is_empty() {
        lines.push(rewrite_stub_header_text(&header));
        lines.join("\n")
    } else {
        lines.push(header);
        lines.push(body_lines.join("\n"));
        lines.join("\n")
    }
}

fn render_authoritative_class_body(
    source: &str,
    class_def: &ruff_python_ast::StmtClassDef,
    context: &LoweredStubContext,
    indent: &str,
) -> Vec<String> {
    let overloaded_names: std::collections::BTreeSet<_> = class_def
        .body
        .iter()
        .filter_map(|statement| match statement {
            Stmt::FunctionDef(function)
                if function.decorator_list.iter().any(is_overload_decorator) =>
            {
                Some(function.name.as_str().to_owned())
            }
            _ => None,
        })
        .collect();
    let mut body_lines = Vec::new();

    for statement in &class_def.body {
        match statement {
            Stmt::AnnAssign(assign) => {
                if let Some(replacement) =
                    render_authoritative_annotated_assignment_stub(source, assign)
                {
                    body_lines.push(indent_block_lines(&replacement, indent));
                }
            }
            Stmt::Assign(assign) => {
                if let Some(replacement) =
                    render_authoritative_assignment_stub(source, assign, context)
                {
                    body_lines.push(indent_block_lines(&replacement, indent));
                }
            }
            Stmt::FunctionDef(function) => {
                if !function.decorator_list.iter().any(is_overload_decorator)
                    && overloaded_names.contains(function.name.as_str())
                {
                    continue;
                }
                body_lines.push(render_authoritative_function_stub(source, function, context));
            }
            Stmt::ClassDef(nested) => {
                body_lines.push(render_authoritative_class_stub(source, nested, context));
            }
            _ => {}
        }
    }

    body_lines
}

fn indent_block_lines(block: &str, indentation: &str) -> String {
    block.lines().map(|line| format!("{indentation}{line}")).collect::<Vec<_>>().join("\n")
}

fn render_authoritative_annotated_assignment_stub(
    source: &str,
    assign: &ruff_python_ast::StmtAnnAssign,
) -> Option<String> {
    if source
        .lines()
        .nth(offset_to_line(source, assign.range.start().to_usize()).saturating_sub(1))
        .is_some_and(|line| line.contains("TypeAlias ="))
    {
        return source_stmt_text(source, assign.range());
    }
    render_annotated_assignment_stub(source, assign)
}

fn render_authoritative_assignment_stub(
    source: &str,
    assign: &ruff_python_ast::StmtAssign,
    context: &LoweredStubContext,
) -> Option<String> {
    let start_line = offset_to_line(source, assign.range.start().to_usize());
    if let Some(statement_text) = source_stmt_text(source, assign.range())
        && statement_text.contains("TypeAlias =")
    {
        return Some(statement_text);
    }
    if let Some(annotation) = context.value_overrides.get(&start_line) {
        let mut lines = Vec::new();
        for target in &assign.targets {
            let Expr::Name(name) = target else {
                continue;
            };
            lines.push(format!("{}: {}", name.id.as_str(), annotation));
        }
        if !lines.is_empty() {
            return Some(lines.join("\n"));
        }
    }
    render_assignment_stub(
        source,
        &assign.targets,
        &assign.value,
        &StubInferenceContext::default(),
        InferredStubMode::Shadow,
    )
}

fn render_synthetic_method_stub(method: &StubSyntheticMethod, indentation: &str) -> String {
    let mut lines = Vec::new();
    lines.extend(method_decorator_lines(method.method_kind, indentation));
    lines.push(format_function_stub_signature(
        indentation,
        &method.name,
        &method.params,
        method.returns.as_deref(),
        false,
    ));
    lines.join("\n")
}

fn method_decorator_lines(method_kind: MethodKind, indentation: &str) -> Vec<String> {
    match method_kind {
        MethodKind::Class => vec![format!("{indentation}@classmethod")],
        MethodKind::Static => vec![format!("{indentation}@staticmethod")],
        MethodKind::Property => vec![format!("{indentation}@property")],
        MethodKind::PropertySetter => vec![format!("{indentation}@property.setter")],
        MethodKind::Instance => Vec::new(),
    }
}

fn format_function_stub_signature(
    indentation: &str,
    name: &str,
    params: &[FunctionParam],
    returns: Option<&str>,
    use_async_syntax: bool,
) -> String {
    let prefix = if use_async_syntax { "async def" } else { "def" };
    format!(
        "{indentation}{prefix} {name}({}) -> {}: ...",
        format_stub_params(params),
        returns.unwrap_or("None")
    )
}

fn format_stub_params(params: &[FunctionParam]) -> String {
    let mut rendered = Vec::new();
    let positional_only_count = params.iter().filter(|param| param.positional_only).count();
    let has_variadic = params.iter().any(|param| param.variadic);
    let keyword_only_index = params.iter().position(|param| param.keyword_only);

    for (index, param) in params.iter().enumerate() {
        if keyword_only_index == Some(index) && !has_variadic {
            rendered.push(String::from("*"));
        }
        let mut part = match &param.annotation {
            Some(annotation) => format!("{}: {}", param.name, annotation),
            None => param.name.clone(),
        };
        if param.has_default {
            part.push_str(" = ...");
        }
        if param.keyword_variadic {
            part = format!("**{part}");
        } else if param.variadic {
            part = format!("*{part}");
        }
        rendered.push(part);
        if positional_only_count > 0 && index + 1 == positional_only_count {
            rendered.push(String::from("/"));
        }
    }

    rendered.join(", ")
}

fn builtin_decorator_lines(
    source: &str,
    decorators: &[ruff_python_ast::Decorator],
    indentation: &str,
) -> Vec<String> {
    decorators
        .iter()
        .filter(|decorator| is_stub_surface_decorator(&decorator.expression))
        .filter_map(|decorator| {
            slice_range(source, decorator.expression.range())
                .map(|expression| format!("{indentation}@{expression}"))
        })
        .collect()
}

fn is_stub_surface_decorator(expression: &Expr) -> bool {
    match expression {
        Expr::Name(name) => matches!(
            name.id.as_str(),
            "overload"
                | "staticmethod"
                | "classmethod"
                | "property"
                | "final"
                | "override"
                | "deprecated"
        ),
        Expr::Attribute(attribute) => {
            matches!(
                attribute.attr.as_str(),
                "overload" | "staticmethod" | "classmethod" | "setter" | "deprecated"
            ) || matches!(attribute.attr.as_str(), "property")
        }
        Expr::Call(call) => is_stub_surface_decorator(call.func.as_ref()),
        _ => false,
    }
}

fn source_stmt_text(source: &str, range: ruff_text_size::TextRange) -> Option<String> {
    let start_line = offset_to_line(source, range.start().to_usize());
    let end_offset = range.end().to_usize().saturating_sub(1);
    let end_line = offset_to_line(source, end_offset.max(range.start().to_usize()));
    let text = source
        .lines()
        .skip(start_line.saturating_sub(1))
        .take(end_line.saturating_sub(start_line) + 1)
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

#[allow(dead_code)]
fn collect_stub_edits(source: &str, suite: &[Stmt], edits: &mut Vec<StubEdit>) {
    let overloaded_names: std::collections::BTreeSet<_> = suite
        .iter()
        .filter_map(|statement| match statement {
            Stmt::FunctionDef(function)
                if function.decorator_list.iter().any(is_overload_decorator) =>
            {
                Some(function.name.as_str().to_owned())
            }
            _ => None,
        })
        .collect();

    for statement in suite {
        match statement {
            Stmt::FunctionDef(function) => {
                let start_line = offset_to_line(source, function.name.range.start().to_usize());
                let end_offset = function.range.end().to_usize().saturating_sub(1);
                let end_line =
                    offset_to_line(source, end_offset.max(function.range.start().to_usize()));
                edits.push(StubEdit {
                    start_line,
                    end_line,
                    replacement: if function.decorator_list.iter().any(is_overload_decorator) {
                        Some(rewrite_stub_function_signature(source, function))
                    } else if overloaded_names.contains(function.name.as_str()) {
                        None
                    } else {
                        Some(rewrite_stub_function_signature(source, function))
                    },
                });
            }
            Stmt::AnnAssign(assign) => {
                if let Some(replacement) = rewrite_stub_annotated_assignment_line(
                    source
                        .lines()
                        .nth(offset_to_line(source, assign.range.start().to_usize()) - 1)
                        .unwrap_or(""),
                ) {
                    let start_line = offset_to_line(source, assign.range.start().to_usize());
                    edits.push(StubEdit {
                        start_line,
                        end_line: start_line,
                        replacement: Some(replacement),
                    });
                }
            }
            Stmt::ClassDef(class_def) => {
                if is_empty_stub_class_body(&class_def.body) {
                    let start_line =
                        offset_to_line(source, class_def.name.range.start().to_usize());
                    let end_offset = class_def.range.end().to_usize().saturating_sub(1);
                    let end_line =
                        offset_to_line(source, end_offset.max(class_def.range.start().to_usize()));
                    edits.push(StubEdit {
                        start_line,
                        end_line,
                        replacement: Some(rewrite_stub_class_line(source, class_def)),
                    });
                } else {
                    collect_stub_edits(source, &class_def.body, edits)
                }
            }
            _ => {}
        }
    }
}

fn rewrite_stub_function_signature(
    source: &str,
    function: &ruff_python_ast::StmtFunctionDef,
) -> String {
    let header = source_header_text(source, function.name.range.start().to_usize(), &function.body);
    rewrite_stub_header_text(&header)
}

fn rewrite_stub_header_text(header: &str) -> String {
    let trimmed = header.trim_end();
    if trimmed.contains(": ...") {
        trimmed.to_owned()
    } else if trimmed.ends_with(':') {
        format!("{trimmed} ...")
    } else {
        trimmed.to_owned()
    }
}

#[allow(dead_code)]
fn rewrite_stub_annotated_assignment_line(line: &str) -> Option<String> {
    if line.contains("TypeAlias =") {
        return None;
    }
    let (head, _) = line.split_once('=')?;
    Some(head.trim_end().to_owned())
}

#[allow(dead_code)]
fn rewrite_stub_class_line(source: &str, class_def: &ruff_python_ast::StmtClassDef) -> String {
    let header =
        source_header_text(source, class_def.name.range.start().to_usize(), &class_def.body);
    rewrite_stub_header_text(&header)
}

fn source_header_text(source: &str, start_offset: usize, body: &[Stmt]) -> String {
    let start_line = offset_to_line(source, start_offset);
    let header_end_line = body
        .first()
        .map(|statement| {
            offset_to_line(source, statement.range().start().to_usize()).saturating_sub(1)
        })
        .unwrap_or(start_line);
    source
        .lines()
        .skip(start_line.saturating_sub(1))
        .take(header_end_line.saturating_sub(start_line) + 1)
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(dead_code)]
fn is_empty_stub_class_body(body: &[Stmt]) -> bool {
    body.iter().all(|statement| match statement {
        Stmt::Pass(_) => true,
        Stmt::Expr(expr) => {
            matches!(expr.value.as_ref(), Expr::StringLiteral(_) | Expr::EllipsisLiteral(_))
        }
        _ => false,
    })
}

fn is_overload_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "overload",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "overload"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if name.id.as_str() == "typing")
        }
        _ => false,
    }
}

fn offset_to_line(source: &str, offset: usize) -> usize {
    let mut line = 1usize;

    for (index, character) in source.char_indices() {
        if index >= offset {
            break;
        }
        if character == '\n' {
            line += 1;
        }
    }

    line
}

fn relative_module_path(config: &ConfigHandle, source_path: &Path) -> PathBuf {
    let logical_root = config.resolve_relative_path(&config.config.project.root_dir);

    if let Ok(relative) = source_path.strip_prefix(logical_root) {
        return relative.to_path_buf();
    }

    source_path.file_name().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("unknown"))
}

#[cfg(test)]
mod tests {
    use super::{
        EmitArtifact, InferredStubMode, PlannedModuleSource, RuntimeWriteSummary,
        StubCallableOverride, StubSyntheticMethod, TypePythonStubContext,
        generate_inferred_stub_source, generate_typepython_stub_source, plan_emits_for_sources,
        write_runtime_outputs,
    };
    use std::{
        env, fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };
    use typepython_config::load;
    use typepython_lowering::{LoweredModule, SourceMapEntry};
    use typepython_syntax::{FunctionParam, MethodKind, SourceKind};

    #[test]
    fn plan_emits_for_sources_matches_source_kinds_without_lowered_modules() {
        let temp_dir =
            temp_dir("plan_emits_for_sources_matches_source_kinds_without_lowered_modules");
        fs::create_dir_all(temp_dir.join("src/pkg")).expect("test setup should succeed");
        fs::write(
            temp_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\nout_dir = \"build\"\n[emit]\nemit_pyi = true\n",
        )
        .expect("test setup should succeed");
        let config = load(&temp_dir).expect("config should load");
        let artifacts = plan_emits_for_sources(
            &config,
            &[
                PlannedModuleSource {
                    source_path: temp_dir.join("src/pkg/__init__.tpy"),
                    source_kind: SourceKind::TypePython,
                },
                PlannedModuleSource {
                    source_path: temp_dir.join("src/pkg/helpers.py"),
                    source_kind: SourceKind::Python,
                },
                PlannedModuleSource {
                    source_path: temp_dir.join("src/pkg/helpers.pyi"),
                    source_kind: SourceKind::Stub,
                },
            ],
        );

        assert_eq!(artifacts.len(), 3);
        assert_eq!(artifacts[0].runtime_path, Some(temp_dir.join("build/pkg/__init__.py")));
        assert_eq!(artifacts[0].stub_path, Some(temp_dir.join("build/pkg/__init__.pyi")));
        assert_eq!(artifacts[1].runtime_path, Some(temp_dir.join("build/pkg/helpers.py")));
        assert_eq!(artifacts[1].stub_path, None);
        assert_eq!(artifacts[2].runtime_path, None);
        assert_eq!(artifacts[2].stub_path, Some(temp_dir.join("build/pkg/helpers.pyi")));
        fs::remove_dir_all(&temp_dir).expect("temp dir cleanup should succeed");
    }

    #[test]
    fn write_runtime_outputs_emits_lowered_typepython_and_python_modules() {
        let temp_dir =
            temp_dir("write_runtime_outputs_emits_lowered_typepython_and_python_modules");
        let modules = vec![
            LoweredModule {
                source_path: PathBuf::from("src/app/__init__.tpy"),
                source_kind: SourceKind::TypePython,
                python_source: String::from(
                    "from typing import TypeAlias\nUserId: TypeAlias = int\ncount: int = 1\n\ndef build_user() -> int:\n    return 1\n",
                ),
                source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                span_map: Vec::new(),
                required_imports: Vec::new(),
                metadata: typepython_lowering::LoweringMetadata::default(),
            },
            LoweredModule {
                source_path: PathBuf::from("src/app/helpers.py"),
                source_kind: SourceKind::Python,
                python_source: String::from("def helper():\n    return 1\n"),
                source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                span_map: Vec::new(),
                required_imports: Vec::new(),
                metadata: typepython_lowering::LoweringMetadata::default(),
            },
            LoweredModule {
                source_path: PathBuf::from("src/app/parse.tpy"),
                source_kind: SourceKind::TypePython,
                python_source: String::from(
                    "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n\ndef parse(x):\n    return 0\n",
                ),
                source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                span_map: Vec::new(),
                required_imports: Vec::new(),
                metadata: typepython_lowering::LoweringMetadata::default(),
            },
            LoweredModule {
                source_path: PathBuf::from("src/app/empty.tpy"),
                source_kind: SourceKind::TypePython,
                python_source: String::from("class Empty:\n    pass\n"),
                source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                span_map: Vec::new(),
                required_imports: Vec::new(),
                metadata: typepython_lowering::LoweringMetadata::default(),
            },
            LoweredModule {
                source_path: PathBuf::from("src/app/helpers.pyi"),
                source_kind: SourceKind::Stub,
                python_source: String::from("def helper() -> int: ...\n"),
                source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                span_map: Vec::new(),
                required_imports: Vec::new(),
                metadata: typepython_lowering::LoweringMetadata::default(),
            },
        ];
        let artifacts = vec![
            EmitArtifact {
                source_path: PathBuf::from("src/app/__init__.tpy"),
                runtime_path: Some(temp_dir.join("build/app/__init__.py")),
                stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
            },
            EmitArtifact {
                source_path: PathBuf::from("src/app/helpers.py"),
                runtime_path: Some(temp_dir.join("build/app/helpers.py")),
                stub_path: None,
            },
            EmitArtifact {
                source_path: PathBuf::from("src/app/parse.tpy"),
                runtime_path: Some(temp_dir.join("build/app/parse.py")),
                stub_path: Some(temp_dir.join("build/app/parse.pyi")),
            },
            EmitArtifact {
                source_path: PathBuf::from("src/app/empty.tpy"),
                runtime_path: Some(temp_dir.join("build/app/empty.py")),
                stub_path: Some(temp_dir.join("build/app/empty.pyi")),
            },
            EmitArtifact {
                source_path: PathBuf::from("src/app/helpers.pyi"),
                runtime_path: None,
                stub_path: Some(temp_dir.join("build/app/helpers.pyi")),
            },
        ];

        let summary = write_runtime_outputs(&artifacts, &modules, false, None)
            .expect("runtime outputs should be written");
        let runtime_init = fs::read_to_string(temp_dir.join("build/app/__init__.py"))
            .expect("runtime __init__.py should be readable");
        let stub_init = fs::read_to_string(temp_dir.join("build/app/__init__.pyi"))
            .expect("stub __init__.pyi should be readable");
        let runtime_helpers = fs::read_to_string(temp_dir.join("build/app/helpers.py"))
            .expect("helpers.py should be readable");
        let runtime_parse = fs::read_to_string(temp_dir.join("build/app/parse.py"))
            .expect("parse.py should be readable");
        let stub_parse = fs::read_to_string(temp_dir.join("build/app/parse.pyi"))
            .expect("parse.pyi should be readable");
        let runtime_empty = fs::read_to_string(temp_dir.join("build/app/empty.py"))
            .expect("empty.py should be readable");
        let stub_empty = fs::read_to_string(temp_dir.join("build/app/empty.pyi"))
            .expect("empty.pyi should be readable");
        let stub_helpers = fs::read_to_string(temp_dir.join("build/app/helpers.pyi"))
            .expect("helpers.pyi should be readable");
        let py_typed = fs::read_to_string(temp_dir.join("build/app/py.typed"))
            .expect("py.typed should be readable");

        let result = (
            summary,
            runtime_init,
            stub_init,
            runtime_helpers,
            runtime_parse,
            stub_parse,
            runtime_empty,
            stub_empty,
            stub_helpers,
            py_typed,
        );
        remove_temp_dir(&temp_dir);

        let (
            summary,
            runtime_init,
            stub_init,
            runtime_helpers,
            runtime_parse,
            stub_parse,
            runtime_empty,
            stub_empty,
            stub_helpers,
            py_typed,
        ) = result;
        assert_eq!(
            summary,
            RuntimeWriteSummary {
                runtime_files_written: 4,
                stub_files_written: 4,
                py_typed_written: 1,
            }
        );
        assert_eq!(
            runtime_init,
            "from typing import TypeAlias\nUserId: TypeAlias = int\ncount: int = 1\n\ndef build_user() -> int:\n    return 1\n"
        );
        assert_eq!(
            stub_init,
            "from typing import TypeAlias\nUserId: TypeAlias = int\ncount: int\n\ndef build_user() -> int: ...\n"
        );
        assert_eq!(runtime_helpers, "def helper():\n    return 1\n");
        assert_eq!(
            runtime_parse,
            "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n\ndef parse(x):\n    return 0\n"
        );
        assert_eq!(
            stub_parse,
            "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n\n"
        );
        assert_eq!(runtime_empty, "class Empty:\n    pass\n");
        assert_eq!(stub_empty, "class Empty: ...\n");
        assert_eq!(stub_helpers, "def helper() -> int: ...\n");
        assert_eq!(py_typed, "");
    }

    #[test]
    fn write_runtime_outputs_adds_runtime_validators_only_when_enabled() {
        let temp_dir = temp_dir("write_runtime_outputs_adds_runtime_validators_only_when_enabled");
        fs::create_dir_all(temp_dir.join("src/app")).expect("src/app should be created");
        fs::write(
            temp_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nruntime_validators = true\n",
        )
        .expect("typepython.toml should be written");
        let _config = load(&temp_dir).expect("config should load");
        let modules = vec![LoweredModule {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from(
                "from dataclasses import dataclass\n\n@dataclass\nclass UserInput:\n    name: str\n    age: int\n    email: str | None = None\n",
            ),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        }];
        let artifacts = vec![EmitArtifact {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            runtime_path: Some(temp_dir.join("build/app/__init__.py")),
            stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
        }];

        write_runtime_outputs(&artifacts, &modules, true, None)
            .expect("runtime validator outputs should be written");
        let runtime = fs::read_to_string(temp_dir.join("build/app/__init__.py"))
            .expect("runtime validator file should be readable");
        let stub = fs::read_to_string(temp_dir.join("build/app/__init__.pyi"))
            .expect("stub validator file should be readable");
        let result = (runtime, stub);
        remove_temp_dir(&temp_dir);

        let (runtime, stub) = result;
        assert!(runtime.contains("def __tpy_validate__(cls, __data: dict) -> \"UserInput\":"));
        assert!(runtime.contains("field `name' expected str but got"));
        assert!(!stub.contains("__tpy_validate__"));
    }

    #[test]
    fn write_runtime_outputs_skips_runtime_validators_when_disabled() {
        let temp_dir = temp_dir("write_runtime_outputs_skips_runtime_validators_when_disabled");
        let modules = vec![LoweredModule {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from(
                "from dataclasses import dataclass\n\n@dataclass\nclass UserInput:\n    name: str\n",
            ),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        }];
        let artifacts = vec![EmitArtifact {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            runtime_path: Some(temp_dir.join("build/app/__init__.py")),
            stub_path: None,
        }];
        write_runtime_outputs(&artifacts, &modules, false, None)
            .expect("runtime outputs should be written without validators");
        let runtime = fs::read_to_string(temp_dir.join("build/app/__init__.py"))
            .expect("runtime file should be readable");
        remove_temp_dir(&temp_dir);

        assert!(!runtime.contains("__tpy_validate__"));
    }

    #[test]
    fn write_runtime_outputs_reports_pyi_generation_failure() {
        let temp_dir = temp_dir("write_runtime_outputs_reports_pyi_generation_failure");
        let modules = vec![LoweredModule {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from("def broken(:\n"),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        }];
        let artifacts = vec![EmitArtifact {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            runtime_path: Some(temp_dir.join("build/app/__init__.py")),
            stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
        }];

        let result = write_runtime_outputs(&artifacts, &modules, false, None);
        remove_temp_dir(&temp_dir);

        let error = result.expect_err("invalid lowered python should fail stub generation");
        assert!(error.to_string().contains("TPY5001"));
        assert!(error.to_string().contains("unable to generate `.pyi`"));
    }

    #[test]
    fn write_runtime_outputs_writes_py_typed_for_stub_only_package() {
        let temp_dir = temp_dir("write_runtime_outputs_writes_py_typed_for_stub_only_package");
        let modules = vec![LoweredModule {
            source_path: PathBuf::from("src/app/__init__.pyi"),
            source_kind: SourceKind::Stub,
            python_source: String::from("def helper() -> int: ...\n"),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        }];
        let artifacts = vec![EmitArtifact {
            source_path: PathBuf::from("src/app/__init__.pyi"),
            runtime_path: None,
            stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
        }];

        let summary = write_runtime_outputs(&artifacts, &modules, false, None)
            .expect("stub-only package outputs should be written");
        let stub = fs::read_to_string(temp_dir.join("build/app/__init__.pyi"))
            .expect("stub-only __init__.pyi should be readable");
        let py_typed =
            fs::read_to_string(temp_dir.join("build/app/py.typed")).expect("py.typed should exist");
        remove_temp_dir(&temp_dir);

        assert_eq!(summary.stub_files_written, 1);
        assert_eq!(summary.py_typed_written, 1);
        assert_eq!(stub, "def helper() -> int: ...\n");
        assert_eq!(py_typed, "");
    }

    #[test]
    fn write_runtime_outputs_preserves_multiline_stub_headers() {
        let temp_dir = temp_dir("write_runtime_outputs_preserves_multiline_stub_headers");
        let modules = vec![LoweredModule {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from(
                "class Box(\n    Generic[T],\n):\n    pass\n\ndef build(\n    value: int,\n) -> int:\n    return value\n",
            ),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        }];
        let artifacts = vec![EmitArtifact {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            runtime_path: Some(temp_dir.join("build/app/__init__.py")),
            stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
        }];

        write_runtime_outputs(&artifacts, &modules, false, None)
            .expect("multiline runtime outputs should be written");
        let stub = fs::read_to_string(temp_dir.join("build/app/__init__.pyi"))
            .expect("multiline stub should be readable");
        remove_temp_dir(&temp_dir);

        assert!(stub.contains("class Box(\n    Generic[T],\n): ..."));
        assert!(stub.contains("def build(\n    value: int,\n) -> int: ..."));
    }

    #[test]
    fn generate_inferred_shadow_stub_uses_unknown_fallback_and_infers_simple_returns() {
        let stub = generate_inferred_stub_source(
            "VALUE = 1\n\ndef parse(text, retries=3):\n    return 1\n",
            InferredStubMode::Shadow,
        )
        .expect("shadow stub generation should succeed");

        assert!(stub.contains("VALUE: int"));
        assert!(stub.contains("def parse(text: unknown, retries: int = ...) -> int: ..."));
    }

    #[test]
    fn generate_inferred_migration_stub_marks_missing_types_and_init_attrs() {
        let stub = generate_inferred_stub_source(
            "class User:\n    def __init__(self, name):\n        self.name = name\n        self.age = 3\n\n    @property\n    def title(self):\n        return self.name\n",
            InferredStubMode::Migration,
        )
        .expect("migration stub generation should succeed");

        assert!(stub.starts_with("# auto-generated by typepython migrate"));
        assert!(stub.contains("    # TODO: add type annotation\n    name: ..."));
        assert!(stub.contains("    age: int"));
        assert!(stub.contains("    @property"));
        assert!(stub.contains("    # TODO: add type annotation\n    def title(self) -> ...: ..."));
    }

    #[test]
    fn generate_inferred_migration_stub_infers_local_and_attribute_returns() {
        let stub = generate_inferred_stub_source(
            "DEFAULT_RETRIES = 3\n\nclass User:\n    def __init__(self, age: int):\n        self.age = age\n\n    @property\n    def years(self):\n        return self.age\n\ndef parse(text: str):\n    retries = DEFAULT_RETRIES\n    return retries\n",
            InferredStubMode::Migration,
        )
        .expect("migration stub generation should succeed");

        assert!(stub.contains("DEFAULT_RETRIES: int"));
        assert!(stub.contains("def parse(text: str) -> int: ..."));
        assert!(stub.contains("    def years(self) -> int: ..."));
        assert!(!stub.contains("# TODO: add type annotation\ndef parse"));
        assert!(!stub.contains("# TODO: add type annotation\n    def years"));
    }

    #[test]
    fn generate_typepython_stub_source_materializes_semantic_callable_and_synthetic_init() {
        let module = LoweredModule {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from(
                "@decorate\ndef build(name: str) -> int:\n    return 1\n\n@model\nclass User:\n    name: str\n",
            ),
            source_map: vec![
                SourceMapEntry { original_line: 1, lowered_line: 1 },
                SourceMapEntry { original_line: 2, lowered_line: 2 },
                SourceMapEntry { original_line: 3, lowered_line: 3 },
                SourceMapEntry { original_line: 4, lowered_line: 4 },
                SourceMapEntry { original_line: 5, lowered_line: 5 },
                SourceMapEntry { original_line: 6, lowered_line: 6 },
            ],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        };
        let context = TypePythonStubContext {
            value_overrides: Vec::new(),
            callable_overrides: vec![StubCallableOverride {
                line: 2,
                params: vec![FunctionParam {
                    name: String::from("name"),
                    annotation: Some(String::from("str")),
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                }],
                returns: Some(String::from("str")),
                use_async_syntax: false,
                drop_non_builtin_decorators: true,
            }],
            synthetic_methods: vec![StubSyntheticMethod {
                class_line: 6,
                name: String::from("__init__"),
                method_kind: MethodKind::Instance,
                params: vec![
                    FunctionParam {
                        name: String::from("self"),
                        annotation: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    },
                    FunctionParam {
                        name: String::from("name"),
                        annotation: Some(String::from("str")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    },
                ],
                returns: Some(String::from("None")),
            }],
        };

        let stub = generate_typepython_stub_source(&module, &context)
            .expect("semantic stub should generate");

        assert!(!stub.contains("@decorate"));
        assert!(stub.contains("def build(name: str) -> str: ..."));
        assert!(!stub.contains("@model"));
        assert!(stub.contains("def __init__(self, name: str) -> None: ..."));
    }

    #[test]
    fn generate_typepython_stub_source_drops_runtime_control_flow_and_rewrites_assignments() {
        let module = LoweredModule {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from(
                "VALUE = 1\nif True:\n    VALUE = 2\n\ndef build() -> int:\n    return VALUE\n",
            ),
            source_map: vec![
                SourceMapEntry { original_line: 1, lowered_line: 1 },
                SourceMapEntry { original_line: 2, lowered_line: 2 },
                SourceMapEntry { original_line: 3, lowered_line: 3 },
                SourceMapEntry { original_line: 4, lowered_line: 4 },
                SourceMapEntry { original_line: 5, lowered_line: 5 },
            ],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        };

        let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
            .expect("stub should generate");

        assert!(stub.contains("VALUE: int"));
        assert!(!stub.contains("if True"));
        assert!(!stub.contains("return VALUE"));
        assert!(stub.contains("def build() -> int: ..."));
    }

    fn temp_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let directory = env::temp_dir().join(format!("typepython-emit-{test_name}-{unique}"));
        fs::create_dir_all(&directory).expect("temp directory should be created");
        directory
    }

    fn remove_temp_dir(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).expect("temp directory should be removed");
        }
    }
}

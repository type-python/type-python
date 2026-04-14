use super::*;

#[derive(Debug)]
pub enum RuntimeWriteError {
    Io(io::Error),
    StubGeneration { source_path: PathBuf, legacy_detail: String },
}

impl std::fmt::Display for RuntimeWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::StubGeneration { source_path, legacy_detail } => write!(
                f,
                "TPY5001: unable to generate `.pyi` for `{}`: {}",
                source_path.display(),
                legacy_detail
            ),
        }
    }
}

impl std::error::Error for RuntimeWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::StubGeneration { .. } => None,
        }
    }
}

impl From<io::Error> for RuntimeWriteError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Materializes planned runtime and stub artifacts to disk and optionally writes `py.typed`.
pub fn write_runtime_outputs(
    artifacts: &[EmitArtifact],
    modules: &[LoweredModule],
    write_py_typed: bool,
    runtime_validators: bool,
    stub_contexts: Option<&BTreeMap<PathBuf, TypePythonStubContext>>,
) -> Result<RuntimeWriteSummary, RuntimeWriteError> {
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
                    RuntimeWriteError::StubGeneration {
                        source_path: module.source_path.clone(),
                        legacy_detail: error.to_string(),
                    }
                })?
            } else if module.source_kind == SourceKind::Python {
                let companion_stub = artifact.source_path.with_extension("pyi");
                modules_by_source
                    .get(companion_stub.as_path())
                    .filter(|stub_module| stub_module.source_kind == SourceKind::Stub)
                    .map(|stub_module| stub_module.python_source.clone())
                    .unwrap_or_else(|| module.python_source.clone())
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
    if write_py_typed {
        for package_root in package_roots {
            fs::write(package_root.join("py.typed"), "")?;
            py_typed_written += 1;
        }
    }

    Ok(RuntimeWriteSummary { runtime_files_written, stub_files_written, py_typed_written })
}

fn is_package_init_path(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "__init__.py" || name == "__init__.pyi")
}

fn inject_runtime_validators(python: &str) -> Result<String, RuntimeWriteError> {
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

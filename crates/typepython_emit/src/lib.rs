//! Output planning boundary for TypePython.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use ruff_python_ast::{Expr, Stmt};
use ruff_python_parser::parse_module;
use typepython_config::ConfigHandle;
use typepython_lowering::LoweredModule;
use typepython_syntax::SourceKind;

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

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct RuntimeWriteSummary {
    pub runtime_files_written: usize,
    pub stub_files_written: usize,
    pub py_typed_written: usize,
}

/// Plans output paths for the provided modules.
#[must_use]
pub fn plan_emits(config: &ConfigHandle, modules: &[LoweredModule]) -> Vec<EmitArtifact> {
    modules
        .iter()
        .map(|module| {
            let relative = relative_module_path(config, &module.source_path);
            let out_root = config.resolve_relative_path(&config.config.project.out_dir);

            match module.source_kind {
                SourceKind::TypePython => EmitArtifact {
                    source_path: module.source_path.clone(),
                    runtime_path: Some(out_root.join(&relative).with_extension("py")),
                    stub_path: config
                        .config
                        .emit
                        .emit_pyi
                        .then(|| out_root.join(relative).with_extension("pyi")),
                },
                SourceKind::Python => EmitArtifact {
                    source_path: module.source_path.clone(),
                    runtime_path: Some(out_root.join(relative)),
                    stub_path: None,
                },
                SourceKind::Stub => EmitArtifact {
                    source_path: module.source_path.clone(),
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
                rewrite_to_stub_source(&module.python_source).map_err(|error| {
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

fn rewrite_to_stub_source(python: &str) -> Result<String, io::Error> {
    let parsed = parse_module(python).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unable to parse lowered Python for stub emission: {}", error.error),
        )
    })?;

    let mut edits = Vec::new();
    collect_stub_edits(python, parsed.suite(), &mut edits);
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

#[derive(Debug)]
struct StubEdit {
    start_line: usize,
    end_line: usize,
    replacement: Option<String>,
}

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
                let line = source.lines().nth(start_line - 1).unwrap_or("");
                edits.push(StubEdit {
                    start_line,
                    end_line,
                    replacement: if function.decorator_list.iter().any(is_overload_decorator) {
                        Some(rewrite_stub_signature_line(line))
                    } else if overloaded_names.contains(function.name.as_str()) {
                        None
                    } else {
                        Some(rewrite_stub_signature_line(line))
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
                    let line = source.lines().nth(start_line - 1).unwrap_or("");
                    edits.push(StubEdit {
                        start_line,
                        end_line,
                        replacement: Some(rewrite_stub_class_line(line)),
                    });
                } else {
                    collect_stub_edits(source, &class_def.body, edits)
                }
            }
            _ => {}
        }
    }
}

fn rewrite_stub_signature_line(line: &str) -> String {
    let trimmed = line.trim_end();
    if trimmed.contains(": ...") {
        trimmed.to_owned()
    } else if trimmed.ends_with(':') {
        format!("{trimmed} ...")
    } else {
        trimmed.to_owned()
    }
}

fn rewrite_stub_annotated_assignment_line(line: &str) -> Option<String> {
    if line.contains("TypeAlias =") {
        return None;
    }
    let (head, _) = line.split_once('=')?;
    Some(head.trim_end().to_owned())
}

fn rewrite_stub_class_line(line: &str) -> String {
    let trimmed = line.trim_end();
    if trimmed.contains(": ...") {
        trimmed.to_owned()
    } else if trimmed.ends_with(':') {
        format!("{trimmed} ...")
    } else {
        trimmed.to_owned()
    }
}

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
    use super::{EmitArtifact, RuntimeWriteSummary, write_runtime_outputs};
    use std::{
        env, fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };
    use typepython_config::load;
    use typepython_lowering::{LoweredModule, SourceMapEntry};
    use typepython_syntax::SourceKind;

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
                required_imports: Vec::new(),
                metadata: typepython_lowering::LoweringMetadata::default(),
            },
            LoweredModule {
                source_path: PathBuf::from("src/app/helpers.py"),
                source_kind: SourceKind::Python,
                python_source: String::from("def helper():\n    return 1\n"),
                source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
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
                required_imports: Vec::new(),
                metadata: typepython_lowering::LoweringMetadata::default(),
            },
            LoweredModule {
                source_path: PathBuf::from("src/app/empty.tpy"),
                source_kind: SourceKind::TypePython,
                python_source: String::from("class Empty:\n    pass\n"),
                source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                required_imports: Vec::new(),
                metadata: typepython_lowering::LoweringMetadata::default(),
            },
            LoweredModule {
                source_path: PathBuf::from("src/app/helpers.pyi"),
                source_kind: SourceKind::Stub,
                python_source: String::from("def helper() -> int: ...\n"),
                source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
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

        let summary = write_runtime_outputs(&artifacts, &modules, false)
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
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        }];
        let artifacts = vec![EmitArtifact {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            runtime_path: Some(temp_dir.join("build/app/__init__.py")),
            stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
        }];

        write_runtime_outputs(&artifacts, &modules, true)
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
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        }];
        let artifacts = vec![EmitArtifact {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            runtime_path: Some(temp_dir.join("build/app/__init__.py")),
            stub_path: None,
        }];
        write_runtime_outputs(&artifacts, &modules, false)
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
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        }];
        let artifacts = vec![EmitArtifact {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            runtime_path: Some(temp_dir.join("build/app/__init__.py")),
            stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
        }];

        let result = write_runtime_outputs(&artifacts, &modules, false);
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
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        }];
        let artifacts = vec![EmitArtifact {
            source_path: PathBuf::from("src/app/__init__.pyi"),
            runtime_path: None,
            stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
        }];

        let summary = write_runtime_outputs(&artifacts, &modules, false)
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

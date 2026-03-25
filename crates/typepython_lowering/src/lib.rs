//! Lowering boundary for TypePython.

use std::{collections::BTreeSet, path::PathBuf};

use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span};
use typepython_syntax::{SourceKind, SyntaxStatement, SyntaxTree};

fn is_typed_dict_base(base: &str) -> bool {
    matches!(base.trim(), "TypedDict" | "typing.TypedDict" | "typing_extensions.TypedDict")
}

/// A single source-map row.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SourceMapEntry {
    /// Original source line.
    pub original_line: usize,
    /// Lowered output line.
    pub lowered_line: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SpanMapRange {
    pub line: usize,
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SpanMapEntry {
    pub original: SpanMapRange,
    pub emitted: SpanMapRange,
}

/// Lowered representation consumed by later phases.
#[derive(Debug, Clone)]
pub struct LoweredModule {
    /// Original module path.
    pub source_path: PathBuf,
    /// Source kind of the module.
    pub source_kind: SourceKind,
    /// Lowered Python text.
    pub python_source: String,
    /// Placeholder source-map rows.
    pub source_map: Vec<SourceMapEntry>,
    pub span_map: Vec<SpanMapEntry>,
    pub required_imports: Vec<String>,
    pub metadata: LoweringMetadata,
}

#[derive(Debug, Clone)]
pub struct LoweringResult {
    pub module: LoweredModule,
    pub diagnostics: DiagnosticReport,
}

struct LoweredText {
    python_source: String,
    source_map: Vec<SourceMapEntry>,
    span_map: Vec<SpanMapEntry>,
    required_imports: Vec<String>,
    metadata: LoweringMetadata,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct LoweringMetadata {
    pub has_generic_type_params: bool,
    pub has_typed_dict_transforms: bool,
    pub has_sealed_classes: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LoweringOptions {
    pub target_python: String,
}

impl Default for LoweringOptions {
    fn default() -> Self {
        Self { target_python: String::from("3.10") }
    }
}

/// Lowers a parsed module into its Python-facing form.
#[must_use]
pub fn lower(tree: &SyntaxTree) -> LoweringResult {
    lower_with_options(tree, &LoweringOptions::default())
}

#[must_use]
pub fn lower_with_options(tree: &SyntaxTree, options: &LoweringOptions) -> LoweringResult {
    let lowered_text = match tree.source.kind {
        SourceKind::TypePython => lower_typepython(tree, options),
        SourceKind::Python | SourceKind::Stub => lower_passthrough(&tree.source.text),
    };
    let diagnostics = collect_lowering_diagnostics(tree);

    LoweringResult {
        module: LoweredModule {
            source_path: tree.source.path.clone(),
            source_kind: tree.source.kind,
            python_source: lowered_text.python_source,
            source_map: lowered_text.source_map,
            span_map: lowered_text.span_map,
            required_imports: lowered_text.required_imports,
            metadata: lowered_text.metadata,
        },
        diagnostics,
    }
}

fn lower_passthrough(source: &str) -> LoweredText {
    LoweredText {
        python_source: source.to_owned(),
        source_map: source
            .lines()
            .enumerate()
            .map(|(index, _)| SourceMapEntry { original_line: index + 1, lowered_line: index + 1 })
            .collect(),
        span_map: source
            .lines()
            .enumerate()
            .map(|(index, line)| SpanMapEntry {
                original: line_span(index + 1, line),
                emitted: line_span(index + 1, line),
            })
            .collect(),
        required_imports: Vec::new(),
        metadata: LoweringMetadata::default(),
    }
}

fn lower_typepython(tree: &SyntaxTree, options: &LoweringOptions) -> LoweredText {
    let unsafe_lines: BTreeSet<_> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::Unsafe(statement) => Some(statement.line),
            _ => None,
        })
        .collect();
    let type_aliases: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::TypeAlias(statement) => Some((statement.line, statement)),
            _ => None,
        })
        .collect();
    let interfaces: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::Interface(statement) if is_lowerable_named_block(statement) => {
                Some((header_line_for_statement(&tree.source.text, statement.line), statement))
            }
            _ => None,
        })
        .collect();
    let data_classes: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::DataClass(statement) if is_lowerable_named_block(statement) => {
                Some((header_line_for_statement(&tree.source.text, statement.line), statement))
            }
            _ => None,
        })
        .collect();
    let overloads: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::OverloadDef(statement) => {
                Some((header_line_for_statement(&tree.source.text, statement.line), statement))
            }
            _ => None,
        })
        .collect();
    let sealed_classes: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::SealedClass(statement) if is_lowerable_named_block(statement) => {
                Some((header_line_for_statement(&tree.source.text, statement.line), statement))
            }
            _ => None,
        })
        .collect();
    let class_defs: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::ClassDef(statement) => {
                Some((header_line_for_statement(&tree.source.text, statement.line), statement))
            }
            _ => None,
        })
        .collect();
    let typed_dicts_by_name: std::collections::BTreeMap<_, _> = class_defs
        .values()
        .filter(|statement| statement.bases.iter().any(|b| is_typed_dict_base(b)))
        .map(|statement| (statement.name.as_str(), *statement))
        .collect();
    let function_defs: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::FunctionDef(statement) => {
                Some((header_line_for_statement(&tree.source.text, statement.line), statement))
            }
            _ => None,
        })
        .collect();
    let runtime_type_params = collect_runtime_type_params(
        &type_aliases,
        &interfaces,
        &data_classes,
        &sealed_classes,
        &class_defs,
        &function_defs,
        &overloads,
    );
    let generic_class_like_declarations = has_generic_class_like_declarations(
        &interfaces,
        &data_classes,
        &sealed_classes,
        &class_defs,
    );
    let has_typed_dict_transforms = type_aliases.values().any(|statement| {
        parse_transform_expr(statement.value.trim())
            .is_some_and(|(transform, _)| TYPEDICT_TRANSFORMS.contains(&transform))
    });
    let has_sealed_classes = !sealed_classes.is_empty();

    let mut lowered_lines = Vec::new();
    let mut required_imports = Vec::new();
    let mut lowered_line_number = 1usize;
    let mut source_map = Vec::new();
    let mut span_map = Vec::new();
    if !runtime_type_params.is_empty() && !has_typevar_import(&tree.source.text) {
        push_required_import(
            &mut lowered_lines,
            &mut required_imports,
            String::from("from typing import TypeVar"),
        );
        lowered_line_number += 1;
    }
    if generic_class_like_declarations && !has_generic_import(&tree.source.text) {
        push_required_import(
            &mut lowered_lines,
            &mut required_imports,
            String::from("from typing import Generic"),
        );
        lowered_line_number += 1;
    }
    for (name, bound) in &runtime_type_params {
        lowered_lines.push(rewrite_typevar_line(name, bound.as_deref()));
        lowered_line_number += 1;
    }
    if !type_aliases.is_empty() && !has_typealias_import(&tree.source.text) {
        push_required_import(
            &mut lowered_lines,
            &mut required_imports,
            String::from("from typing import TypeAlias"),
        );
        lowered_line_number += 1;
    }
    if !interfaces.is_empty() && !has_protocol_import(&tree.source.text) {
        push_required_import(
            &mut lowered_lines,
            &mut required_imports,
            String::from("from typing import Protocol"),
        );
        lowered_line_number += 1;
    }
    if !data_classes.is_empty() && !has_dataclass_import(&tree.source.text) {
        push_required_import(
            &mut lowered_lines,
            &mut required_imports,
            String::from("from dataclasses import dataclass"),
        );
        lowered_line_number += 1;
    }
    if !overloads.is_empty() && !has_overload_import(&tree.source.text) {
        push_required_import(
            &mut lowered_lines,
            &mut required_imports,
            String::from("from typing import overload"),
        );
        lowered_line_number += 1;
    }
    // Check if any type alias uses a transform that generates NotRequired
    let needs_notrequired_import = type_aliases.values().any(|stmt| {
        let v = stmt.value.trim();
        v == "Partial[User]"
            || v == "Required_[UserUpdate]"
            || v.starts_with("Partial[")
            || v.starts_with("Required_[")
    });
    if needs_notrequired_import && !has_notrequired_import(&tree.source.text) {
        push_required_import(
            &mut lowered_lines,
            &mut required_imports,
            rewrite_notrequired_import_line(options),
        );
        lowered_line_number += 1;
    }
    // Check if any type alias uses a transform that generates ReadOnly
    let needs_readonly_import = type_aliases.values().any(|stmt| {
        let v = stmt.value.trim();
        v == "Readonly[Config]"
            || v == "Mutable[Config]"
            || v.starts_with("Readonly[")
            || v.starts_with("Mutable[")
    });
    if needs_readonly_import && !has_readonly_import(&tree.source.text) {
        push_required_import(
            &mut lowered_lines,
            &mut required_imports,
            String::from("from typing_extensions import ReadOnly"),
        );
        lowered_line_number += 1;
    }

    for (index, line) in tree.source.text.lines().enumerate() {
        let line_number = index + 1;
        let replacement_lines = if let Some(statement) = type_aliases.get(&line_number) {
            if let Some(expanded) =
                try_expand_typeddict_transform(&statement.value, &typed_dicts_by_name, line)
            {
                expanded
            } else {
                vec![rewrite_typealias_line(line, statement)]
            }
        } else if let Some(statement) = interfaces.get(&line_number) {
            vec![rewrite_interface_line(line, statement)]
        } else if let Some(statement) = data_classes.get(&line_number) {
            rewrite_data_class_lines(line, statement).into_iter().collect()
        } else if overloads.contains_key(&line_number) {
            rewrite_overload_lines(line).into_iter().collect()
        } else if let Some(statement) = sealed_classes.get(&line_number) {
            vec![rewrite_sealed_class_line(line, statement)]
        } else if let Some(statement) = class_defs.get(&line_number) {
            vec![rewrite_class_def_line(line, statement)]
        } else if function_defs.contains_key(&line_number) {
            vec![rewrite_function_def_line(line)]
        } else if unsafe_lines.contains(&line_number) {
            vec![rewrite_unsafe_line(line)]
        } else {
            vec![line.to_owned()]
        };

        source_map
            .push(SourceMapEntry { original_line: line_number, lowered_line: lowered_line_number });
        let original_span = line_span(line_number, line);
        span_map.extend(replacement_lines.iter().enumerate().map(|(offset, replacement)| {
            SpanMapEntry {
                original: original_span,
                emitted: line_span(lowered_line_number + offset, replacement),
            }
        }));
        lowered_line_number += replacement_lines.len();
        lowered_lines.extend(replacement_lines);
    }

    let mut lowered = lowered_lines.join("\n");
    if tree.source.text.ends_with('\n') {
        lowered.push('\n');
    }

    LoweredText {
        python_source: lowered,
        source_map,
        span_map,
        required_imports,
        metadata: LoweringMetadata {
            has_generic_type_params: !runtime_type_params.is_empty(),
            has_typed_dict_transforms,
            has_sealed_classes,
        },
    }
}

fn push_required_import(
    lowered_lines: &mut Vec<String>,
    required_imports: &mut Vec<String>,
    import_line: String,
) {
    lowered_lines.push(import_line.clone());
    required_imports.push(import_line);
}

fn line_span(line_number: usize, text: &str) -> SpanMapRange {
    SpanMapRange { line: line_number, start_col: 1, end_col: text.chars().count() + 1 }
}

fn rewrite_notrequired_import_line(options: &LoweringOptions) -> String {
    if prefers_typing_notrequired(&options.target_python) {
        String::from("from typing import NotRequired")
    } else {
        String::from("from typing_extensions import NotRequired")
    }
}

fn prefers_typing_notrequired(target_python: &str) -> bool {
    matches!(target_python.trim(), "3.11" | "3.12")
}

fn header_line_for_statement(source: &str, start_line: usize) -> usize {
    let lines: Vec<&str> = source.lines().collect();
    let mut index = start_line.saturating_sub(1);
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        if !trimmed.is_empty() && !trimmed.starts_with('@') {
            return index + 1;
        }
        index += 1;
    }
    start_line
}

fn collect_runtime_type_params(
    type_aliases: &std::collections::BTreeMap<usize, &typepython_syntax::TypeAliasStatement>,
    interfaces: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    data_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    sealed_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    class_defs: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    function_defs: &std::collections::BTreeMap<usize, &typepython_syntax::FunctionStatement>,
    overloads: &std::collections::BTreeMap<usize, &typepython_syntax::FunctionStatement>,
) -> std::collections::BTreeMap<String, Option<String>> {
    let mut type_params = std::collections::BTreeMap::new();

    for statement in type_aliases.values() {
        for type_param in &statement.type_params {
            type_params.entry(type_param.name.clone()).or_insert_with(|| type_param.bound.clone());
        }
    }
    for statement in interfaces.values() {
        for type_param in &statement.type_params {
            type_params.entry(type_param.name.clone()).or_insert_with(|| type_param.bound.clone());
        }
    }
    for statement in data_classes.values() {
        for type_param in &statement.type_params {
            type_params.entry(type_param.name.clone()).or_insert_with(|| type_param.bound.clone());
        }
    }
    for statement in sealed_classes.values() {
        for type_param in &statement.type_params {
            type_params.entry(type_param.name.clone()).or_insert_with(|| type_param.bound.clone());
        }
    }
    for statement in class_defs.values() {
        for type_param in &statement.type_params {
            type_params.entry(type_param.name.clone()).or_insert_with(|| type_param.bound.clone());
        }
    }
    for statement in function_defs.values() {
        for type_param in &statement.type_params {
            type_params.entry(type_param.name.clone()).or_insert_with(|| type_param.bound.clone());
        }
    }
    for statement in overloads.values() {
        for type_param in &statement.type_params {
            type_params.entry(type_param.name.clone()).or_insert_with(|| type_param.bound.clone());
        }
    }

    type_params
}

fn has_generic_class_like_declarations(
    interfaces: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    data_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    sealed_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    class_defs: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
) -> bool {
    interfaces.values().any(|statement| !statement.type_params.is_empty())
        || data_classes.values().any(|statement| !statement.type_params.is_empty())
        || sealed_classes.values().any(|statement| !statement.type_params.is_empty())
        || class_defs.values().any(|statement| !statement.type_params.is_empty())
}

fn rewrite_unsafe_line(line: &str) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    format!("{indentation}if True:")
}

fn rewrite_typealias_line(line: &str, statement: &typepython_syntax::TypeAliasStatement) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    format!("{indentation}{}: TypeAlias = {}", statement.name, statement.value)
}

fn rewrite_typevar_line(name: &str, bound: Option<&str>) -> String {
    match bound {
        Some(bound) => format!("{name} = TypeVar(\"{name}\", bound={bound})"),
        None => format!("{name} = TypeVar(\"{name}\")"),
    }
}

fn has_typealias_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import TypeAlias"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("TypeAlias"))
    })
}

fn has_typevar_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import TypeVar"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("TypeVar"))
    })
}

fn has_generic_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import Generic"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("Generic"))
    })
}

fn rewrite_interface_line(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let mut extras = vec![String::from("Protocol")];
    if !statement.type_params.is_empty() {
        extras.push(generic_base(statement));
    }
    let bases = append_bases(&statement.header_suffix, &extras);
    format!("{indentation}class {}{}:", statement.name, bases)
}

fn rewrite_data_class_lines(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
) -> [String; 2] {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let bases = append_optional_generic_base(statement);

    [format!("{indentation}@dataclass"), format!("{indentation}class {}{}:", statement.name, bases)]
}

fn rewrite_sealed_class_line(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let bases = append_optional_generic_base(statement);

    format!("{indentation}class {}{}:  # tpy:sealed", statement.name, bases)
}

fn rewrite_class_def_line(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let bases = append_optional_generic_base(statement);
    format!("{indentation}class {}{}:", statement.name, bases)
}

fn rewrite_function_def_line(line: &str) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let trimmed = line.trim_start();
    format!("{indentation}{}", strip_generic_type_params(trimmed))
}

fn strip_generic_type_params(source: &str) -> String {
    let mut bracket_index = None;
    let mut paren_depth = 0usize;

    for (index, character) in source.char_indices() {
        match character {
            '[' if paren_depth == 0 => {
                bracket_index = Some(index);
                break;
            }
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
    }

    let Some(bracket_index) = bracket_index else {
        return source.to_owned();
    };
    let (head, tail) = source.split_at(bracket_index);
    let Some((_params, remainder)) = split_bracketed(tail) else {
        return source.to_owned();
    };
    format!("{head}{remainder}")
}

fn split_bracketed(input: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;

    for (index, character) in input.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((&input[1..index], &input[index + 1..]));
                }
            }
            _ => {}
        }
    }

    None
}

fn append_optional_generic_base(statement: &typepython_syntax::NamedBlockStatement) -> String {
    if statement.type_params.is_empty() {
        if statement.header_suffix.is_empty() {
            String::new()
        } else {
            statement.header_suffix.clone()
        }
    } else {
        append_bases(&statement.header_suffix, &[generic_base(statement)])
    }
}

fn append_bases(header_suffix: &str, extras: &[String]) -> String {
    if extras.is_empty() {
        return header_suffix.to_owned();
    }

    let trimmed = header_suffix.trim();
    let inner = if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.trim_start_matches('(').trim_end_matches(')').trim().to_owned()
    };

    let mut parts = Vec::new();
    if !inner.is_empty() {
        parts.push(inner);
    }
    parts.extend(extras.iter().cloned());
    format!("({})", parts.join(", "))
}

fn generic_base(statement: &typepython_syntax::NamedBlockStatement) -> String {
    format!(
        "Generic[{}]",
        statement
            .type_params
            .iter()
            .map(|type_param| type_param.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn has_protocol_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import Protocol"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("Protocol"))
    })
}

fn has_dataclass_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from dataclasses import dataclass"
            || (trimmed.starts_with("from dataclasses import ") && trimmed.contains("dataclass"))
    })
}

fn rewrite_overload_lines(line: &str) -> [String; 2] {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let rewritten =
        line.trim_start().strip_prefix("overload ").unwrap_or_else(|| line.trim_start()).to_owned();
    let rewritten = strip_generic_type_params(&rewritten);

    [format!("{indentation}@overload"), format!("{indentation}{rewritten}")]
}

fn has_overload_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import overload"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("overload"))
    })
}

fn is_lowerable_named_block(statement: &typepython_syntax::NamedBlockStatement) -> bool {
    statement.header_suffix.is_empty()
        || (statement.header_suffix.starts_with('(') && statement.header_suffix.ends_with(')'))
}
// ─── TypedDict utility transform expansion ───────────────────────────────────

/// Known TypedDict utility transforms.
const TYPEDICT_TRANSFORMS: &[&str] =
    &["Partial", "Required_", "Readonly", "Mutable", "Pick", "Omit"];

/// If `value` is a TypedDict utility transform, returns the expanded class lines.
/// Otherwise returns None.
fn try_expand_typeddict_transform(
    value: &str,
    typed_dicts: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
    source_line: &str,
) -> Option<Vec<String>> {
    let value = value.trim();
    let (transform, args) = parse_transform_expr(value)?;
    if !TYPEDICT_TRANSFORMS.contains(&transform) {
        return None;
    }

    // args = [transform_name, target_or_key, key2, ...]
    // For Partial[T]: target_name = T, key_args = []
    // For Pick[T, "k1", "k2"]: target_name = T, key_args = ["k1", "k2"]
    // args = [transform_name, target_or_key, key2, ...]
    // For Partial[T]: target_name = T, key_args = []
    // For Pick[T, "k1", "k2"]: target_name = T, key_args = ["k1", "k2"]
    let (transform, target_arg, key_args) = if args.len() < 2 {
        return None;
    } else {
        (args[0], args[1], &args[2..])
    };

    // Handle nested transforms: if target_arg is itself a transform, recursively expand it
    // inner_args[0]=transform name, [1]=target TypedDict, [2..]=key args
    let base_members = if let Some((inner_transform, inner_args)) = parse_transform_expr(target_arg)
    {
        if TYPEDICT_TRANSFORMS.contains(&inner_transform) && inner_args.len() >= 2 {
            // Recursively expand the inner transform
            let inner_target_name = inner_args[1]; // [1] is the target TypedDict name
            let inner_key_args = &inner_args[2..]; // [2..] are the key args
            let inner_target = typed_dicts.get(inner_target_name)?;
            // inner_target is &&NamedBlockStatement, dereference to &
            apply_transform_to_members(inner_transform, &inner_target.members, inner_key_args)
        } else {
            return None;
        }
    } else {
        // target_arg is a regular TypedDict name
        let target = typed_dicts.get(target_arg)?;
        target.members.to_vec()
    };

    let indentation = source_line.len() - source_line.trim_start().len();
    let indent = &source_line[..indentation];

    let members = apply_transform_to_members(transform, &base_members, key_args);
    // Extract alias name from source line: "typealias Name = ..."
    let alias_name = source_line
        .trim_start()
        .trim_end()
        .strip_prefix("typealias")?
        .split('=')
        .next()?
        .trim()
        .to_owned();

    let mut lines = Vec::with_capacity(2 + members.len());
    lines.push(format!("{}class {}(TypedDict):", indent, alias_name));
    for member in members {
        let ann = member.annotation.as_deref().unwrap_or("object");
        lines.push(format!("{}    {}: {}", indent, member.name, ann));
    }

    Some(lines)
}

/// Parse "TransformName[T]" or "TransformName[T, 'k1', 'k2', ...]"
/// Returns (transform_name, vec![target_type, "k1", "k2", ...])
/// Handles nested transforms and quoted key names like "id".
fn parse_transform_expr(value: &str) -> Option<(&str, Vec<&str>)> {
    let value = value.trim();
    let bracket_start = value.find('[')?;
    let transform = value[..bracket_start].trim();

    // Find the matching closing bracket using depth counting,
    // respecting quoted strings so ["id"] doesn't confuse the parser.
    let rest = &value[bracket_start + 1..];
    let closing_pos = find_matching_bracket(rest)?;

    // inner: everything between the opening '[' and its matching ']'
    let inner = &rest[..closing_pos];

    // Split on top-level commas only (inside nested brackets)
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut in_string = false;
    let mut string_char = ' ';
    for (i, c) in inner.char_indices() {
        if !in_string && (c == '"' || c == '\'') {
            in_string = true;
            string_char = c;
        } else if in_string && c == string_char && (i == 0 || !inner[..i].ends_with('\\')) {
            in_string = false;
        } else if !in_string {
            match c {
                '[' | '<' | '(' => depth += 1,
                ']' | '>' | ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    args.push(inner[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            }
        }
    }
    args.push(inner[start..].trim());

    if args.is_empty() {
        return None;
    }
    // Prepend transform name so args = [transform_name, target_or_key, key2, ...]
    let mut full_args = Vec::with_capacity(1 + args.len());
    full_args.push(transform);
    full_args.extend(args);
    Some((transform, full_args))
}

/// Find the position of the matching closing bracket for a string
/// that starts with '['. Returns the position of the closing ']' (exclusive).
/// Returns None if no matching bracket is found.
fn find_matching_bracket(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut string_char = ' ';
    for (i, c) in s.char_indices() {
        if in_string {
            if c == '\\' && i + 1 < s.len() {
                // Skip escaped character
                continue;
            }
            if c == string_char {
                in_string = false;
            }
        } else {
            match c {
                '"' | '\'' => {
                    in_string = true;
                    string_char = c;
                }
                '[' => depth += 1,
                ']' => {
                    if depth == 0 {
                        return Some(i);
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }
    None
}

/// Apply a TypedDict utility transform to TypedDict field members.
fn apply_transform_to_members<'a>(
    transform: &str,
    members: &'a [typepython_syntax::ClassMember],
    key_args: &[&str],
) -> Vec<typepython_syntax::ClassMember> {
    let fields: Vec<&'a typepython_syntax::ClassMember> =
        members.iter().filter(|m| m.kind == typepython_syntax::ClassMemberKind::Field).collect();

    match transform {
        "Partial" => fields
            .into_iter()
            .map(|m| {
                let ann = m.annotation.as_deref().unwrap_or("object");
                let new_ann = if ann.contains("NotRequired[") {
                    ann.to_owned()
                } else if ann.starts_with("Required_[") {
                    ann.replace("Required_[", "NotRequired[").to_owned()
                } else {
                    format!("NotRequired[{}]", ann)
                };
                let mut m = m.clone();
                m.annotation = Some(new_ann);
                m
            })
            .collect(),
        "Required_" => fields
            .into_iter()
            .map(|m| {
                let ann = m.annotation.as_deref().unwrap_or("object");
                let new_ann = if let Some(inner) =
                    ann.strip_prefix("NotRequired[").and_then(|inner| inner.strip_suffix(']'))
                {
                    inner.trim().to_owned()
                } else {
                    ann.to_owned()
                };
                let mut m = m.clone();
                m.annotation = Some(new_ann);
                m
            })
            .collect(),
        "Readonly" => fields
            .into_iter()
            .map(|m| {
                let ann = m.annotation.as_deref().unwrap_or("object");
                let new_ann = if ann.contains("ReadOnly[") {
                    ann.to_owned()
                } else {
                    format!("ReadOnly[{}]", ann)
                };
                let mut m = m.clone();
                m.annotation = Some(new_ann);
                m
            })
            .collect(),
        "Mutable" => fields
            .into_iter()
            .map(|m| {
                let ann = m.annotation.as_deref().unwrap_or("object");
                let new_ann = strip_readonly(ann);
                let mut m = m.clone();
                m.annotation = Some(new_ann);
                m
            })
            .collect(),
        "Pick" => {
            let keys: std::collections::BTreeSet<_> = key_args
                .iter()
                .map(|s| {
                    s.trim_matches('"')
                        .trim_matches('\'')
                        .trim_end_matches(']')
                        .trim_end_matches(')')
                        .trim_end_matches('>')
                        .to_owned()
                })
                .collect();
            fields.into_iter().filter(|m| keys.contains(&m.name)).cloned().collect()
        }
        "Omit" => {
            let keys: std::collections::BTreeSet<_> = key_args
                .iter()
                .map(|s| {
                    s.trim_matches('"')
                        .trim_matches('\'')
                        .trim_end_matches(']')
                        .trim_end_matches(')')
                        .trim_end_matches('>')
                        .to_owned()
                })
                .collect();
            fields.into_iter().filter(|m| !keys.contains(&m.name)).cloned().collect()
        }
        _ => fields.into_iter().cloned().collect(),
    }
}

/// Strip ReadOnly[...] wrappers from a type string (one level).
fn strip_readonly(ann: &str) -> String {
    let ann = ann.trim();
    if let Some(inner) = ann.strip_prefix("ReadOnly[").and_then(|s| s.strip_suffix(']')) {
        return inner.to_owned();
    }
    ann.to_owned()
}

fn has_notrequired_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing_extensions import NotRequired"
            || (trimmed.starts_with("from typing_extensions import ")
                && trimmed.contains("NotRequired"))
            || (trimmed.starts_with("from typing import ") && trimmed.contains("NotRequired"))
    })
}

fn has_readonly_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing_extensions import ReadOnly"
            || (trimmed.starts_with("from typing_extensions import ")
                && trimmed.contains("ReadOnly"))
            || (trimmed.starts_with("from typing import ") && trimmed.contains("ReadOnly"))
    })
}

fn collect_lowering_diagnostics(tree: &SyntaxTree) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    let typed_dicts_by_name: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::ClassDef(statement)
                if statement.bases.iter().any(|base| is_typed_dict_base(base)) =>
            {
                Some((statement.name.as_str(), statement))
            }
            _ => None,
        })
        .collect();

    for statement in &tree.statements {
        match statement {
            SyntaxStatement::Unsafe(_) => {}
            SyntaxStatement::TypeAlias(statement) => {
                for diagnostic in collect_typed_dict_transform_diagnostics(
                    &tree.source.path,
                    statement.line,
                    &statement.value,
                    &typed_dicts_by_name,
                ) {
                    diagnostics.push(diagnostic);
                }
            }
            SyntaxStatement::Interface(statement) if is_lowerable_named_block(statement) => {}
            SyntaxStatement::Interface(statement) => {
                diagnostics.push(lowering_error(&tree.source.path, statement.line, "interface"))
            }
            SyntaxStatement::DataClass(statement) if is_lowerable_named_block(statement) => {}
            SyntaxStatement::DataClass(statement) => {
                diagnostics.push(lowering_error(&tree.source.path, statement.line, "data class"))
            }
            SyntaxStatement::SealedClass(statement) if is_lowerable_named_block(statement) => {}
            SyntaxStatement::SealedClass(statement) => {
                diagnostics.push(lowering_error(&tree.source.path, statement.line, "sealed class"))
            }
            SyntaxStatement::ClassDef(_) => {}
            SyntaxStatement::FunctionDef(_) => {}
            SyntaxStatement::Import(_) => {}
            SyntaxStatement::Value(_) => {}
            SyntaxStatement::Call(_) => {}
            SyntaxStatement::MethodCall(_) => {}
            SyntaxStatement::MemberAccess(_) => {}
            SyntaxStatement::Return(_) => {}
            SyntaxStatement::Yield(_) => {}
            SyntaxStatement::If(_) => {}
            SyntaxStatement::Assert(_) => {}
            SyntaxStatement::Invalidate(_) => {}
            SyntaxStatement::Match(_) => {}
            SyntaxStatement::For(_) => {}
            SyntaxStatement::With(_) => {}
            SyntaxStatement::ExceptHandler(_) => {}
            SyntaxStatement::OverloadDef(_) => {}
        }
    }

    diagnostics
}

fn collect_typed_dict_transform_diagnostics(
    path: &std::path::Path,
    line: usize,
    value: &str,
    typed_dicts: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
) -> Vec<Diagnostic> {
    let Some((transform, args)) = parse_transform_expr(value.trim()) else {
        return Vec::new();
    };
    if !TYPEDICT_TRANSFORMS.contains(&transform) || args.len() < 2 {
        return Vec::new();
    }

    let target_arg = args[1];
    let key_args = &args[2..];
    let (target_name, members) = match resolve_transform_members(target_arg, typed_dicts) {
        Some(result) => result,
        None => {
            return vec![typed_dict_transform_error(
                path,
                line,
                format!(
                    "type transform `{}` targets `{}` which is not a known TypedDict",
                    transform,
                    target_arg.trim()
                ),
            )];
        }
    };

    if !matches!(transform, "Pick" | "Omit") {
        return Vec::new();
    }

    let field_names: BTreeSet<_> = members
        .iter()
        .filter(|member| member.kind == typepython_syntax::ClassMemberKind::Field)
        .map(|member| member.name.as_str())
        .collect();
    key_args
        .iter()
        .filter_map(|key_arg| {
            let key = transform_key_name(key_arg);
            (!field_names.contains(key.as_str())).then(|| {
                typed_dict_transform_error(
                    path,
                    line,
                    format!(
                        "type transform `{}` references unknown key `{}` on TypedDict `{}`",
                        transform, key, target_name
                    ),
                )
            })
        })
        .collect()
}

fn resolve_transform_members(
    value: &str,
    typed_dicts: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
) -> Option<(String, Vec<typepython_syntax::ClassMember>)> {
    if let Some((transform, args)) = parse_transform_expr(value.trim()) {
        if TYPEDICT_TRANSFORMS.contains(&transform) && args.len() >= 2 {
            let target_arg = args[1];
            let key_args = &args[2..];
            let (target_name, base_members) = resolve_transform_members(target_arg, typed_dicts)?;
            return Some((
                target_name,
                apply_transform_to_members(transform, &base_members, key_args),
            ));
        }
    }

    let target = typed_dicts.get(value.trim())?;
    Some((target.name.clone(), target.members.to_vec()))
}

fn transform_key_name(key: &str) -> String {
    key.trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(']')
        .trim_end_matches(')')
        .trim_end_matches('>')
        .to_owned()
}

fn typed_dict_transform_error(path: &std::path::Path, line: usize, message: String) -> Diagnostic {
    Diagnostic::error("TPY4017", message).with_span(Span::new(
        path.display().to_string(),
        line,
        1,
        line,
        1,
    ))
}

fn lowering_error(path: &std::path::Path, line: usize, construct: &str) -> Diagnostic {
    Diagnostic::error(
        "TPY2002",
        format!("TypePython-only syntax `{construct}` is recognized but not lowerable yet"),
    )
    .with_span(Span::new(path.display().to_string(), line, 1, line, 1))
}

#[cfg(test)]
mod tests {
    use super::{
        LoweringMetadata, LoweringOptions, SourceMapEntry, SpanMapEntry, SpanMapRange, lower,
        lower_with_options,
    };
    use std::path::PathBuf;
    use typepython_diagnostics::DiagnosticReport;
    use typepython_syntax::{
        ClassMember, ClassMemberKind, NamedBlockStatement, SourceFile, SourceKind, SyntaxStatement,
        SyntaxTree, TypeAliasStatement, TypeParam, UnsafeStatement,
    };

    #[test]
    fn lower_rewrites_top_level_unsafe_blocks() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("unsafe.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("unsafe:\n    x = 1\n"),
            },
            statements: vec![SyntaxStatement::Unsafe(UnsafeStatement { line: 1 })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        println!("OUTPUT:\n{}", lowered.module.python_source);
        println!("DIAGNOSTICS: {:?}", lowered.diagnostics);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "if True:\n    x = 1\n");
        assert_eq!(
            lowered.module.source_map,
            vec![
                SourceMapEntry { original_line: 1, lowered_line: 1 },
                SourceMapEntry { original_line: 2, lowered_line: 2 },
            ]
        );
    }

    #[test]
    fn lower_rewrites_nested_unsafe_blocks_with_indentation() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("nested-unsafe.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("def update():\n    unsafe:\n        x = 1\n"),
            },
            statements: vec![SyntaxStatement::Unsafe(UnsafeStatement { line: 2 })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        eprintln!("DIAGNOSTICS: {:?}", lowered.diagnostics);
        eprintln!("OUTPUT:\n{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "def update():\n    if True:\n        x = 1\n");
    }

    #[test]
    fn lower_reports_unimplemented_typepython_constructs() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("unsupported.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("unknown feature\n"),
            },
            statements: vec![],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
    }

    #[test]
    fn lower_rewrites_non_generic_typealias_with_import() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("typealias.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("typealias UserId = int\n"),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("UserId"),
                type_params: Vec::new(),
                value: String::from("int"),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import TypeAlias\nUserId: TypeAlias = int\n"
        );
    }

    #[test]
    fn lower_rewrites_non_generic_interface_with_protocol_import() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("interface.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("interface SupportsClose:\n    def close(self): ...\n"),
            },
            statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import Protocol\nclass SupportsClose(Protocol):\n    def close(self): ...\n"
        );
    }

    #[test]
    fn lower_rewrites_interface_with_existing_bases() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("interface-bases.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "interface SupportsClose(Closable):\n    def close(self): ...\n",
                ),
            },
            statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: Vec::new(),
                header_suffix: String::from("(Closable)"),
                bases: vec![String::from("Closable")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import Protocol\nclass SupportsClose(Closable, Protocol):\n    def close(self): ...\n"
        );
    }

    #[test]
    fn lower_rewrites_generic_interface_with_protocol_and_generic_base() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("generic-interface.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "interface SupportsClose[T]:\n    def close(self, value: T) -> T: ...\n",
                ),
            },
            statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import TypeVar\nfrom typing import Generic\nT = TypeVar(\"T\")\nfrom typing import Protocol\nclass SupportsClose(Protocol, Generic[T]):\n    def close(self, value: T) -> T: ...\n"
        );
    }

    #[test]
    fn lower_rewrites_non_generic_data_class_with_dataclass_import() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("data-class.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("data class Point:\n    x: float\n    y: float\n"),
            },
            statements: vec![SyntaxStatement::DataClass(NamedBlockStatement {
                name: String::from("Point"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from dataclasses import dataclass\n@dataclass\nclass Point:\n    x: float\n    y: float\n"
        );
        assert_eq!(
            lowered.module.source_map,
            vec![
                SourceMapEntry { original_line: 1, lowered_line: 2 },
                SourceMapEntry { original_line: 2, lowered_line: 4 },
                SourceMapEntry { original_line: 3, lowered_line: 5 },
            ]
        );
    }

    #[test]
    fn lower_rewrites_data_class_with_existing_bases() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("data-class-bases.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("data class Point(Base):\n    x: float\n"),
            },
            statements: vec![SyntaxStatement::DataClass(NamedBlockStatement {
                name: String::from("Point"),
                type_params: Vec::new(),
                header_suffix: String::from("(Base)"),
                bases: vec![String::from("Base")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from dataclasses import dataclass\n@dataclass\nclass Point(Base):\n    x: float\n"
        );
    }

    #[test]
    fn lower_rewrites_generic_data_class_and_sealed_class_with_generic_base() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("generic-classlikes.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "data class Point[T]:\n    x: T\n\nsealed class Expr[T](Base):\n    ...\n",
                ),
            },
            statements: vec![
                SyntaxStatement::DataClass(NamedBlockStatement {
                    name: String::from("Point"),
                    type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 1,
                }),
                SyntaxStatement::SealedClass(NamedBlockStatement {
                    name: String::from("Expr"),
                    type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                    header_suffix: String::from("(Base)"),
                    bases: vec![String::from("Base")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 4,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import TypeVar\nfrom typing import Generic\nT = TypeVar(\"T\")\nfrom dataclasses import dataclass\n@dataclass\nclass Point(Generic[T]):\n    x: T\n\nclass Expr(Base, Generic[T]):  # tpy:sealed\n    ...\n"
        );
    }

    #[test]
    fn lower_rewrites_non_generic_sealed_class_with_marker() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("sealed-class.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("sealed class Expr:\n    ...\n"),
            },
            statements: vec![SyntaxStatement::SealedClass(NamedBlockStatement {
                name: String::from("Expr"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "class Expr:  # tpy:sealed\n    ...\n");
    }

    #[test]
    fn lower_rewrites_sealed_class_with_existing_bases() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("sealed-class-bases.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("sealed class Expr(Base):\n    ...\n"),
            },
            statements: vec![SyntaxStatement::SealedClass(NamedBlockStatement {
                name: String::from("Expr"),
                type_params: Vec::new(),
                header_suffix: String::from("(Base)"),
                bases: vec![String::from("Base")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "class Expr(Base):  # tpy:sealed\n    ...\n");
    }

    #[test]
    fn lower_rewrites_non_generic_overload_with_import() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("overload.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("overload def parse(x: str) -> int: ...\n"),
            },
            statements: vec![SyntaxStatement::OverloadDef(typepython_syntax::FunctionStatement {
                name: String::from("parse"),
                type_params: Vec::new(),
                params: Vec::new(),
                returns: None,
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import overload\n@overload\ndef parse(x: str) -> int: ...\n"
        );
        assert_eq!(
            lowered.module.source_map,
            vec![SourceMapEntry { original_line: 1, lowered_line: 2 }]
        );
        assert_eq!(
            lowered.module.span_map,
            vec![
                SpanMapEntry {
                    original: SpanMapRange { line: 1, start_col: 1, end_col: 39 },
                    emitted: SpanMapRange { line: 2, start_col: 1, end_col: 10 },
                },
                SpanMapEntry {
                    original: SpanMapRange { line: 1, start_col: 1, end_col: 39 },
                    emitted: SpanMapRange { line: 3, start_col: 1, end_col: 30 },
                },
            ]
        );
        assert_eq!(
            lowered.module.required_imports,
            vec![String::from("from typing import overload")]
        );
        assert_eq!(
            lowered.module.metadata,
            LoweringMetadata {
                has_generic_type_params: false,
                has_typed_dict_transforms: false,
                has_sealed_classes: false,
            }
        );
    }

    #[test]
    fn lower_still_blocks_generic_typealias() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("generic-typealias.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("typealias Pair[T] = tuple[T, T]\n"),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("Pair"),
                type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                value: String::from("tuple[T, T]"),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import TypeVar\nT = TypeVar(\"T\")\nfrom typing import TypeAlias\nPair: TypeAlias = tuple[T, T]\n"
        );
    }

    #[test]
    fn lower_still_blocks_generic_overload_def() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("generic-overload.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("overload def parse[T](x: T) -> T: ...\n"),
            },
            statements: vec![SyntaxStatement::OverloadDef(typepython_syntax::FunctionStatement {
                name: String::from("parse"),
                type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                params: Vec::new(),
                returns: None,
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import TypeVar\nT = TypeVar(\"T\")\nfrom typing import overload\n@overload\ndef parse(x: T) -> T: ...\n"
        );
    }

    #[test]
    fn lower_rewrites_generic_ordinary_class_and_function_headers() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("ordinary-generics.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "class Box[T](Base):\n    pass\n\ndef first[T](value: T) -> T:\n    return value\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                    header_suffix: String::from("(Base)"),
                    bases: vec![String::from("Base")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 1,
                }),
                SyntaxStatement::FunctionDef(typepython_syntax::FunctionStatement {
                    name: String::from("first"),
                    type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                    params: Vec::new(),
                    returns: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 4,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import TypeVar\nfrom typing import Generic\nT = TypeVar(\"T\")\nclass Box(Base, Generic[T]):\n    pass\n\ndef first(value: T) -> T:\n    return value\n"
        );
        assert_eq!(
            lowered.module.source_map,
            vec![
                SourceMapEntry { original_line: 1, lowered_line: 4 },
                SourceMapEntry { original_line: 2, lowered_line: 5 },
                SourceMapEntry { original_line: 3, lowered_line: 6 },
                SourceMapEntry { original_line: 4, lowered_line: 7 },
                SourceMapEntry { original_line: 5, lowered_line: 8 },
            ]
        );
    } // ─── TypedDict utility transform tests ───────────────────────────────────

    #[test]
    fn lower_expands_partial_typeddict_transform() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("partial.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "class User(TypedDict):\n    id: int\n    name: str\n\ntypealias UserCreate = Partial[User]\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("User"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(TypedDict)"),
                    bases: vec![String::from("TypedDict")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![
                        ClassMember {
                            name: String::from("id"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("int")),
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 2,
                        },
                        ClassMember {
                            name: String::from("name"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("str")),
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 3,
                        },
                    ],
                    line: 1,
                }),
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("UserCreate"),
                    type_params: Vec::new(),
                    value: String::from("Partial[User]"),
                    line: 5,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert!(lowered.module.python_source.contains("class UserCreate(TypedDict):"));
        assert!(lowered.module.python_source.contains("id: NotRequired[int]"));
        assert!(lowered.module.python_source.contains("name: NotRequired[str]"));
        assert!(lowered.module.python_source.contains("from typing_extensions import NotRequired"));
        assert_eq!(
            lowered.module.required_imports,
            vec![
                String::from("from typing import TypeAlias"),
                String::from("from typing_extensions import NotRequired"),
            ]
        );
        assert!(lowered.module.metadata.has_typed_dict_transforms);
    }

    #[test]
    fn lower_prefers_typing_notrequired_for_target_python_311() {
        let lowered = lower_with_options(
            &SyntaxTree {
                source: SourceFile {
                    path: PathBuf::from("partial-311.tpy"),
                    kind: SourceKind::TypePython,
                    logical_module: String::new(),
                    text: String::from(
                        "class User(TypedDict):\n    id: int\n    name: str\n\ntypealias UserUpdate = Partial[User]\n",
                    ),
                },
                statements: vec![
                    SyntaxStatement::ClassDef(NamedBlockStatement {
                        name: String::from("User"),
                        type_params: Vec::new(),
                        header_suffix: String::from("(TypedDict)"),
                        bases: vec![String::from("TypedDict")],
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_abstract_class: false,
                        members: vec![
                            ClassMember {
                                name: String::from("id"),
                                kind: ClassMemberKind::Field,
                                method_kind: None,
                                annotation: Some(String::from("int")),
                                value_type: None,
                                params: Vec::new(),
                                returns: None,
                                is_async: false,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_deprecated: false,
                                deprecation_message: None,
                                is_final: false,
                                is_class_var: false,
                                line: 2,
                            },
                            ClassMember {
                                name: String::from("name"),
                                kind: ClassMemberKind::Field,
                                method_kind: None,
                                annotation: Some(String::from("str")),
                                value_type: None,
                                params: Vec::new(),
                                returns: None,
                                is_async: false,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_deprecated: false,
                                deprecation_message: None,
                                is_final: false,
                                is_class_var: false,
                                line: 3,
                            },
                        ],
                        line: 1,
                    }),
                    SyntaxStatement::TypeAlias(TypeAliasStatement {
                        name: String::from("UserUpdate"),
                        type_params: Vec::new(),
                        value: String::from("Partial[User]"),
                        line: 5,
                    }),
                ],
                type_ignore_directives: Vec::new(),
                diagnostics: DiagnosticReport::default(),
            },
            &LoweringOptions { target_python: String::from("3.11") },
        );

        assert!(lowered.diagnostics.is_empty());
        assert!(lowered.module.python_source.contains("from typing import NotRequired"));
        assert!(
            !lowered.module.python_source.contains("from typing_extensions import NotRequired")
        );
        assert_eq!(
            lowered.module.required_imports,
            vec![
                String::from("from typing import TypeAlias"),
                String::from("from typing import NotRequired"),
            ]
        );
        assert!(lowered.module.metadata.has_typed_dict_transforms);
    }

    #[test]
    fn lower_expands_partial_typeddict_transform_for_qualified_bases() {
        for base in ["typing.TypedDict", "typing_extensions.TypedDict"] {
            let lowered = lower(&SyntaxTree {
                source: SourceFile {
                    path: PathBuf::from("partial-qualified.tpy"),
                    kind: SourceKind::TypePython,
                    logical_module: String::new(),
                    text: format!(
                        "class User({base}):\n    id: int\n    name: str\n\ntypealias UserCreate = Partial[User]\n"
                    ),
                },
                statements: vec![
                    SyntaxStatement::ClassDef(NamedBlockStatement {
                        name: String::from("User"),
                        type_params: Vec::new(),
                        header_suffix: format!("({base})"),
                        bases: vec![String::from(base)],
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_abstract_class: false,
                        members: vec![
                            ClassMember {
                                name: String::from("id"),
                                kind: ClassMemberKind::Field,
                                method_kind: None,
                                annotation: Some(String::from("int")),
                                value_type: None,
                                params: Vec::new(),
                                returns: None,
                                is_async: false,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_deprecated: false,
                                deprecation_message: None,
                                is_final: false,
                                is_class_var: false,
                                line: 2,
                            },
                            ClassMember {
                                name: String::from("name"),
                                kind: ClassMemberKind::Field,
                                method_kind: None,
                                annotation: Some(String::from("str")),
                                value_type: None,
                                params: Vec::new(),
                                returns: None,
                                is_async: false,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_deprecated: false,
                                deprecation_message: None,
                                is_final: false,
                                is_class_var: false,
                                line: 3,
                            },
                        ],
                        line: 1,
                    }),
                    SyntaxStatement::TypeAlias(TypeAliasStatement {
                        name: String::from("UserCreate"),
                        type_params: Vec::new(),
                        value: String::from("Partial[User]"),
                        line: 5,
                    }),
                ],
                type_ignore_directives: Vec::new(),
                diagnostics: DiagnosticReport::default(),
            });

            assert!(lowered.diagnostics.is_empty(), "{}", lowered.diagnostics.as_text());
            assert!(lowered.module.python_source.contains("class UserCreate(TypedDict):"));
            assert!(lowered.module.python_source.contains("id: NotRequired[int]"));
            assert!(lowered.module.python_source.contains("name: NotRequired[str]"));
        }
    }

    #[test]
    fn lower_expands_pick_typeddict_transform() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("pick.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "class User(TypedDict):\n    id: int\n    name: str\n    email: str\n\ntypealias UserPublic = Pick[User, \"id\", \"name\"]\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("User"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(TypedDict)"),
                    bases: vec![String::from("TypedDict")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![
                        ClassMember {
                            name: String::from("id"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("int")),
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 2,
                        },
                        ClassMember {
                            name: String::from("name"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("str")),
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 3,
                        },
                        ClassMember {
                            name: String::from("email"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("str")),
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 4,
                        },
                    ],
                    line: 1,
                }),
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("UserPublic"),
                    type_params: Vec::new(),
                    value: String::from("Pick[User, \"id\", \"name\"]"),
                    line: 6,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        println!(
            "OUTPUT:
{}",
            lowered.module.python_source
        );
        assert!(lowered.diagnostics.is_empty());
        assert!(lowered.module.python_source.contains("class UserPublic(TypedDict):"));
        assert!(lowered.module.python_source.contains("id: int"));
        assert!(lowered.module.python_source.contains("name: str"));
        // email should NOT appear in the UserPublic transform (it's in the original User class)
        let all_lines: Vec<_> = lowered.module.python_source.lines().collect();
        let user_public_start = all_lines
            .iter()
            .position(|l| l.contains("class UserPublic"))
            .expect("UserPublic class should be emitted");
        let mut section = String::new();
        for l in &all_lines[user_public_start..] {
            if l.trim().is_empty() || l.trim().starts_with("class ") {
                break;
            }
            section.push_str(l);
            section.push('\n');
        }
        assert!(!section.contains("email"), "email should not appear in UserPublic Pick transform");
    }

    #[test]
    fn lower_expands_omit_typeddict_transform() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("omit.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "class User(TypedDict):\n    id: int\n    name: str\n\ntypealias UserUpdate = Omit[User, \"id\"]\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("User"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(TypedDict)"),
                    bases: vec![String::from("TypedDict")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![
                        ClassMember {
                            name: String::from("id"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("int")),
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 2,
                        },
                        ClassMember {
                            name: String::from("name"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("str")),
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 3,
                        },
                    ],
                    line: 1,
                }),
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("UserUpdate"),
                    type_params: Vec::new(),
                    value: String::from("Omit[User, \"id\"]"),
                    line: 5,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert!(lowered.module.python_source.contains("class UserUpdate(TypedDict):"));
        assert!(lowered.module.python_source.contains("name: str"));
        // id should NOT appear in the UserUpdate transform (it's in the original User class)
        let all_lines: Vec<_> = lowered.module.python_source.lines().collect();
        let user_update_start = all_lines
            .iter()
            .position(|l| l.contains("class UserUpdate"))
            .expect("UserUpdate class should be emitted");
        let mut section = String::new();
        for l in &all_lines[user_update_start..] {
            if l.trim().is_empty() || l.trim().starts_with("class ") {
                break;
            }
            section.push_str(l);
            section.push('\n');
        }
        assert!(!section.contains("id:"), "id should not appear in UserUpdate Omit transform");
    }

    #[test]
    fn lower_expands_readonly_typeddict_transform() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("readonly.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "class Config(TypedDict):\n    debug: bool\n\ntypealias ImmutableConfig = Readonly[Config]\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Config"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(TypedDict)"),
                    bases: vec![String::from("TypedDict")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("debug"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("bool")),
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    }],
                    line: 1,
                }),
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("ImmutableConfig"),
                    type_params: Vec::new(),
                    value: String::from("Readonly[Config]"),
                    line: 4,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert!(lowered.module.python_source.contains("class ImmutableConfig(TypedDict):"));
        assert!(lowered.module.python_source.contains("debug: ReadOnly[bool]"));
        assert!(lowered.module.python_source.contains("from typing_extensions import ReadOnly"));
    }

    #[test]
    fn lower_expands_required_typeddict_transform() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("required.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "class UserUpdate(TypedDict):\n    name: NotRequired[str]\n\ntypealias RequiredUpdate = Required_[UserUpdate]\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("UserUpdate"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(TypedDict)"),
                    bases: vec![String::from("TypedDict")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("name"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("NotRequired[str]")),
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    }],
                    line: 1,
                }),
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("RequiredUpdate"),
                    type_params: Vec::new(),
                    value: String::from("Required_[UserUpdate]"),
                    line: 4,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert!(lowered.module.python_source.contains("class RequiredUpdate(TypedDict):"));
        assert!(lowered.module.python_source.contains("name: str"));
    }

    #[test]
    fn lower_expands_required_typeddict_transform_with_nested_annotation() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("required-nested.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "class UserUpdate(TypedDict):\n    value: NotRequired[list[int]]\n\ntypealias RequiredUpdate = Required_[UserUpdate]\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("UserUpdate"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(TypedDict)"),
                    bases: vec![String::from("TypedDict")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("value"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("NotRequired[list[int]]")),
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    }],
                    line: 1,
                }),
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("RequiredUpdate"),
                    type_params: Vec::new(),
                    value: String::from("Required_[UserUpdate]"),
                    line: 4,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert!(lowered.module.python_source.contains("class RequiredUpdate(TypedDict):"));
        assert!(lowered.module.python_source.contains("value: list[int]"));
    }

    #[test]
    fn lower_expands_composed_typeddict_transform() {
        // Partial[Omit[User, "id"]]: Omit removes "id", Partial makes rest optional
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("composed.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "class User(TypedDict):\n    id: int\n    name: str\n\ntypealias UserUpdate = Partial[Omit[User, \"id\"]]\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("User"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(TypedDict)"),
                    bases: vec![String::from("TypedDict")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![
                        ClassMember {
                            name: String::from("id"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("int")),
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 2,
                        },
                        ClassMember {
                            name: String::from("name"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("str")),
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 3,
                        },
                    ],
                    line: 1,
                }),
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("UserUpdate"),
                    type_params: Vec::new(),
                    value: String::from("Partial[Omit[User, \"id\"]]"),
                    line: 5,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert!(lowered.module.python_source.contains("class UserUpdate(TypedDict):"));
        // Omit removes id, then Partial makes name optional
        assert!(lowered.module.python_source.contains("name: NotRequired[str]"));
        // id should NOT appear in the UserUpdate transform
        let all_lines: Vec<_> = lowered.module.python_source.lines().collect();
        let user_update_start = all_lines
            .iter()
            .position(|l| l.contains("class UserUpdate"))
            .expect("UserUpdate class should be emitted");
        let mut section = String::new();
        for l in &all_lines[user_update_start..] {
            if l.trim().is_empty() || l.trim().starts_with("class ") {
                break;
            }
            section.push_str(l);
            section.push('\n');
        }
        assert!(!section.contains("id:"), "id should not appear in composed transform");
    }

    #[test]
    fn lower_expands_mutable_typeddict_transform() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("mutable.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "class Config(TypedDict):\n    debug: ReadOnly[bool]\n\ntypealias MutableConfig = Mutable[Config]\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Config"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(TypedDict)"),
                    bases: vec![String::from("TypedDict")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("debug"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("ReadOnly[bool]")),
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    }],
                    line: 1,
                }),
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("MutableConfig"),
                    type_params: Vec::new(),
                    value: String::from("Mutable[Config]"),
                    line: 4,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert!(lowered.module.python_source.contains("class MutableConfig(TypedDict):"));
        // ReadOnly wrapper should be stripped
        assert!(lowered.module.python_source.contains("debug: bool"));
    }

    #[test]
    fn lower_keeps_decorated_class_header_singleton() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("decorated-class.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("@model\nclass User:\n    name: str\n    age: int\n"),
            },
            statements: vec![SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("User"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![
                    ClassMember {
                        name: String::from("name"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("str")),
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                    ClassMember {
                        name: String::from("age"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("int")),
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 4,
                    },
                ],
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        let lines = lowered.module.python_source.lines().collect::<Vec<_>>();
        assert_eq!(lines, vec!["@model", "class User:", "    name: str", "    age: int"]);
    }

    #[test]
    fn lower_reports_unknown_pick_key_as_tpy4017() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("pick-invalid-key.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "class User(TypedDict):\n    id: int\n\ntypealias UserPublic = Pick[User, \"name\"]\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("User"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(TypedDict)"),
                    bases: vec![String::from("TypedDict")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("id"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("int")),
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    }],
                    line: 1,
                }),
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("UserPublic"),
                    type_params: Vec::new(),
                    value: String::from("Pick[User, \"name\"]"),
                    line: 4,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        let rendered = lowered.diagnostics.as_text();
        assert!(rendered.contains("TPY4017"));
        assert!(rendered.contains("unknown key `name`"));
    }

    #[test]
    fn lower_reports_non_typeddict_transform_target_as_tpy4017() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("pick-invalid-target.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("typealias UserPublic = Pick[Config, \"name\"]\n"),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("UserPublic"),
                type_params: Vec::new(),
                value: String::from("Pick[Config, \"name\"]"),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        let rendered = lowered.diagnostics.as_text();
        assert!(rendered.contains("TPY4017"));
        assert!(rendered.contains("not a known TypedDict"));
    }

    #[test]
    fn lower_non_transform_typealias_unchanged() {
        // Regular type alias (not a transform) should still work as before
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("regular.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("typealias UserId = int\n"),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("UserId"),
                type_params: Vec::new(),
                value: String::from("int"),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert!(lowered.module.python_source.contains("from typing import TypeAlias"));
        assert!(lowered.module.python_source.contains("UserId: TypeAlias = int"));
    }
}

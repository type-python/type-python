use super::*;

pub(super) fn is_typed_dict_base(base: &str) -> bool {
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SpanMapEntry {
    pub source_path: PathBuf,
    pub emitted_path: PathBuf,
    pub original: SpanMapRange,
    pub emitted: SpanMapRange,
    pub kind: LoweringSegmentKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LoweringSegmentKind {
    Copied,
    Inserted,
    Rewritten,
    Synthetic,
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
    pub required_runtime_features: BTreeSet<RuntimeFeature>,
    pub required_backports: BTreeSet<BackportRequirement>,
    pub export_runtime_semantics: std::collections::BTreeMap<String, RuntimeTypingSemantics>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum BackportRequirement {
    TypingExtensionsAtLeast412,
}

struct InsertedLineTracker<'a> {
    source_path: &'a Path,
    emitted_path: &'a Path,
    lowered_lines: &'a mut Vec<String>,
    required_imports: &'a mut Vec<String>,
    span_map: &'a mut Vec<SpanMapEntry>,
    lowered_line_number: &'a mut usize,
}

impl<'a> InsertedLineTracker<'a> {
    fn emit_required_import(&mut self, import_line: String) {
        push_required_import(self.lowered_lines, self.required_imports, import_line);
        self.record_last_line(LoweringSegmentKind::Inserted);
    }

    fn emit_synthetic_line(&mut self, line: String) {
        self.lowered_lines.push(line);
        self.record_last_line(LoweringSegmentKind::Synthetic);
    }

    fn record_last_line(&mut self, kind: LoweringSegmentKind) {
        let Some(text) = self.lowered_lines.last() else {
            return;
        };
        self.span_map.push(inserted_span_map_entry(
            self.source_path,
            self.emitted_path,
            *self.lowered_line_number,
            text,
            kind,
        ));
        *self.lowered_line_number += 1;
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LoweringOptions {
    pub target_python: PythonTarget,
    pub emit_style: EmitStyle,
}

impl Default for LoweringOptions {
    fn default() -> Self {
        Self { target_python: PythonTarget::default(), emit_style: EmitStyle::Compat }
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
    let passthrough_path = PathBuf::from("<passthrough>");
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
                source_path: passthrough_path.clone(),
                emitted_path: passthrough_path.clone(),
                original: line_span(index + 1, line),
                emitted: line_span(index + 1, line),
                kind: LoweringSegmentKind::Copied,
            })
            .collect(),
        required_imports: Vec::new(),
        metadata: LoweringMetadata::default(),
    }
}

fn lower_typepython(tree: &SyntaxTree, options: &LoweringOptions) -> LoweredText {
    let source_path = tree.source.path.clone();
    let emitted_path = tree.source.path.with_extension("py");
    let normalized_source =
        typepython_syntax::normalize_annotated_lambda_source_for_emission(&tree.source.text);
    let compatibility_normalized_lines = normalized_source
        .lines()
        .flat_map(|line| normalize_target_compatibility_line(line, options))
        .collect::<Vec<_>>();
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
    let compat_type_param_bindings = collect_runtime_type_param_bindings(
        tree,
        options,
        SplitCompatFunctionTypeParams::SplitDistinctFunctions,
    );
    let declaration_type_param_rewrites =
        compat_type_param_rewrites_by_declaration_line(&compat_type_param_bindings);
    let class_member_type_param_rewrites =
        compat_type_param_rewrites_by_class_member_line(&tree.source.text, &compat_type_param_bindings);
    let generic_class_like_declarations = has_compat_generic_class_like_declarations(
        &interfaces,
        &data_classes,
        &sealed_classes,
        &class_defs,
        options,
    );
    let has_generic_type_params = has_any_generic_type_params(
        &type_aliases,
        &interfaces,
        &data_classes,
        &sealed_classes,
        &class_defs,
        &function_defs,
        &overloads,
    );
    let has_typed_dict_transforms = type_aliases.values().any(|statement| {
        parse_transform_expr(statement.value.trim())
            .is_some_and(|(transform, _)| TYPEDICT_TRANSFORMS.contains(&transform))
    });
    let has_sealed_classes = !sealed_classes.is_empty();
    let needs_typing_extensions_runtime_type_params =
        compat_type_param_bindings.iter().any(|binding| binding.type_param.default.is_some())
            && !options.target_python.supports_generic_defaults();
    let has_runtime_typevars = compat_type_param_bindings
        .iter()
        .any(|binding| binding.type_param.kind == typepython_syntax::TypeParamKind::TypeVar);
    let has_runtime_paramspecs = compat_type_param_bindings
        .iter()
        .any(|binding| binding.type_param.kind == typepython_syntax::TypeParamKind::ParamSpec);
    let has_runtime_typevartuples = compat_type_param_bindings
        .iter()
        .any(|binding| binding.type_param.kind == typepython_syntax::TypeParamKind::TypeVarTuple);
    let needs_unpack_import =
        has_unqualified_symbol_usage(&tree.source.text, "Unpack") || has_runtime_typevartuples;
    let typevartuple_owner =
        options.target_python.stdlib_owner("TypeVarTuple").unwrap_or("typing_extensions");
    let unpack_owner = options.target_python.stdlib_owner("Unpack").unwrap_or("typing_extensions");
    let needs_typing_module_import =
        compatibility_normalized_lines.iter().any(|line| line.contains("typing."))
            && !has_module_import(&tree.source.text, "typing");
    let needs_typing_extensions_module_import =
        compatibility_normalized_lines.iter().any(|line| line.contains("typing_extensions."))
            && !has_module_import(&tree.source.text, "typing_extensions");

    let mut lowered_lines = Vec::new();
    let mut required_imports = Vec::new();
    let mut lowered_line_number = 1usize;
    let mut source_map = Vec::new();
    let mut span_map = Vec::new();
    {
        let mut inserted_lines = InsertedLineTracker {
            source_path: &source_path,
            emitted_path: &emitted_path,
            lowered_lines: &mut lowered_lines,
            required_imports: &mut required_imports,
            span_map: &mut span_map,
            lowered_line_number: &mut lowered_line_number,
        };
        if has_runtime_typevars
            && !has_typevar_import(
                &tree.source.text,
                if needs_typing_extensions_runtime_type_params {
                    "typing_extensions"
                } else {
                    "typing"
                },
            )
        {
            inserted_lines.emit_required_import(rewrite_typevar_import_line(
                if needs_typing_extensions_runtime_type_params {
                    "typing_extensions"
                } else {
                    "typing"
                },
            ));
        }
        if has_runtime_paramspecs
            && !has_paramspec_import(
                &tree.source.text,
                if needs_typing_extensions_runtime_type_params {
                    "typing_extensions"
                } else {
                    "typing"
                },
            )
        {
            inserted_lines.emit_required_import(rewrite_paramspec_import_line(
                if needs_typing_extensions_runtime_type_params {
                    "typing_extensions"
                } else {
                    "typing"
                },
            ));
        }
        if has_runtime_typevartuples
            && !has_typevartuple_import(&tree.source.text, typevartuple_owner)
        {
            inserted_lines
                .emit_required_import(rewrite_typevartuple_import_line(typevartuple_owner));
        }
        if needs_unpack_import && !has_unpack_import(&tree.source.text, unpack_owner) {
            inserted_lines.emit_required_import(rewrite_unpack_import_line(unpack_owner));
        }
        if generic_class_like_declarations && !has_generic_import(&tree.source.text) {
            inserted_lines.emit_required_import(String::from("from typing import Generic"));
        }
        if needs_typing_module_import {
            inserted_lines.emit_required_import(String::from("import typing"));
        }
        if needs_typing_extensions_module_import {
            inserted_lines.emit_required_import(String::from("import typing_extensions"));
        }
        if tree_uses_dynamic_intrinsic(tree) && !has_any_import(&tree.source.text) {
            inserted_lines.emit_required_import(String::from("from typing import Any"));
        }
        for binding in &compat_type_param_bindings {
            inserted_lines.emit_synthetic_line(rewrite_typevar_line(
                &binding.emitted_name,
                &binding.source_name,
                &binding.type_param,
            ));
        }
        if type_aliases.values().any(|statement| !can_use_native_typealias(statement, options))
            && !has_typealias_import(&tree.source.text)
        {
            inserted_lines.emit_required_import(String::from("from typing import TypeAlias"));
        }
        if !interfaces.is_empty() && !has_protocol_import(&tree.source.text) {
            inserted_lines.emit_required_import(String::from("from typing import Protocol"));
        }
        if !data_classes.is_empty() && !has_dataclass_import(&tree.source.text) {
            inserted_lines.emit_required_import(String::from("from dataclasses import dataclass"));
        }
        if !overloads.is_empty() && !has_overload_import(&tree.source.text) {
            inserted_lines.emit_required_import(String::from("from typing import overload"));
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
            inserted_lines.emit_required_import(rewrite_notrequired_import_line(options));
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
            inserted_lines.emit_required_import(format!(
                "from {} import ReadOnly",
                options.target_python.stdlib_owner("ReadOnly").unwrap_or("typing_extensions")
            ));
        }
    }

    for (index, line) in normalized_source.lines().enumerate() {
        let line_number = index + 1;
        let (replacement_lines, preserve_variadic_syntax) =
            if let Some(statement) = type_aliases.get(&line_number) {
                if let Some(expanded) =
                    try_expand_typeddict_transform(&statement.value, &typed_dicts_by_name, line)
                {
                    (expanded, false)
                } else {
                    (
                        vec![rewrite_typealias_line(
                            line,
                            statement,
                            options,
                            declaration_type_param_rewrites.get(&line_number),
                        )],
                        can_use_native_typealias(statement, options),
                    )
                }
            } else if let Some(statement) = interfaces.get(&line_number) {
                (
                    vec![rewrite_interface_line(
                        line,
                        statement,
                        options,
                        declaration_type_param_rewrites.get(&line_number),
                    )],
                    can_use_native_type_params(&statement.type_params, options),
                )
            } else if let Some(statement) = data_classes.get(&line_number) {
                (
                    rewrite_data_class_lines(
                        line,
                        statement,
                        options,
                        declaration_type_param_rewrites.get(&line_number),
                    )
                    .into_iter()
                    .collect(),
                    can_use_native_type_params(&statement.type_params, options),
                )
            } else if let Some(statement) = overloads.get(&line_number) {
                (
                    rewrite_overload_lines(
                        line,
                        statement,
                        options,
                        declaration_type_param_rewrites.get(&line_number),
                    )
                    .into_iter()
                    .collect(),
                    can_use_native_type_params(&statement.type_params, options),
                )
            } else if let Some(statement) = sealed_classes.get(&line_number) {
                (
                    vec![rewrite_sealed_class_line(
                        line,
                        statement,
                        options,
                        declaration_type_param_rewrites.get(&line_number),
                    )],
                    can_use_native_type_params(&statement.type_params, options),
                )
            } else if let Some(statement) = class_defs.get(&line_number) {
                (
                    vec![rewrite_class_def_line(
                        line,
                        statement,
                        options,
                        declaration_type_param_rewrites.get(&line_number),
                    )],
                    can_use_native_type_params(&statement.type_params, options),
                )
            } else if let Some(statement) = function_defs.get(&line_number) {
                (
                    vec![rewrite_function_def_line(
                        line,
                        statement,
                        options,
                        declaration_type_param_rewrites.get(&line_number),
                    )],
                    can_use_native_type_params(&statement.type_params, options),
                )
            } else if let Some(type_param_rewrites) = class_member_type_param_rewrites.get(&line_number)
            {
                (vec![rewrite_type_param_tokens(line, type_param_rewrites)], false)
            } else if unsafe_lines.contains(&line_number) {
                (vec![rewrite_unsafe_line(line)], false)
            } else {
                (vec![line.to_owned()], false)
            };
        let replacement_lines = replacement_lines
            .into_iter()
            .map(|replacement| {
                if preserve_variadic_syntax {
                    replacement
                } else {
                    typepython_syntax::normalize_source_variadic_type_syntax(&replacement)
                }
            })
            .flat_map(|replacement| normalize_target_compatibility_line(&replacement, options))
            .collect::<Vec<_>>();

        source_map
            .push(SourceMapEntry { original_line: line_number, lowered_line: lowered_line_number });
        let original_span = line_span(line_number, line);
        let segment_kind = if replacement_lines.len() == 1 && replacement_lines[0] == line {
            LoweringSegmentKind::Copied
        } else {
            LoweringSegmentKind::Rewritten
        };
        span_map.extend(replacement_lines.iter().enumerate().map(|(offset, replacement)| {
            SpanMapEntry {
                source_path: source_path.clone(),
                emitted_path: emitted_path.clone(),
                original: original_span,
                emitted: line_span(lowered_line_number + offset, replacement),
                kind: segment_kind,
            }
        }));
        lowered_line_number += replacement_lines.len();
        lowered_lines.extend(replacement_lines);
    }

    let mut lowered = normalize_runtime_intrinsic_types(&lowered_lines.join("\n"));
    if normalized_source.ends_with('\n') {
        lowered.push('\n');
    }

    let required_runtime_features = collect_required_runtime_features(
        &type_aliases,
        &interfaces,
        &data_classes,
        &sealed_classes,
        &class_defs,
        &function_defs,
        &overloads,
        &lowered,
        options,
    );
    let required_backports = collect_required_backports(&lowered);
    let export_runtime_semantics = collect_export_runtime_semantics(
        &type_aliases,
        &interfaces,
        &data_classes,
        &sealed_classes,
        &class_defs,
        &function_defs,
        &overloads,
        options,
    );

    LoweredText {
        python_source: lowered,
        source_map,
        span_map,
        required_imports,
        metadata: LoweringMetadata {
            has_generic_type_params,
            has_typed_dict_transforms,
            has_sealed_classes,
            required_runtime_features,
            required_backports,
            export_runtime_semantics,
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

fn slice_range(source: &str, range: ruff_text_size::TextRange) -> Option<&str> {
    source.get(range.start().to_usize()..range.end().to_usize())
}

fn inserted_span_map_entry(
    source_path: &Path,
    emitted_path: &Path,
    emitted_line: usize,
    text: &str,
    kind: LoweringSegmentKind,
) -> SpanMapEntry {
    SpanMapEntry {
        source_path: source_path.to_path_buf(),
        emitted_path: emitted_path.to_path_buf(),
        original: SpanMapRange { line: 0, start_col: 0, end_col: 0 },
        emitted: line_span(emitted_line, text),
        kind,
    }
}

fn rewrite_notrequired_import_line(options: &LoweringOptions) -> String {
    format!(
        "from {} import NotRequired",
        options.target_python.stdlib_owner("NotRequired").unwrap_or("typing_extensions")
    )
}

fn compat_module_for_symbol(symbol: &str, target_python: PythonTarget) -> Option<&'static str> {
    target_python.stdlib_owner(symbol)
}

fn normalize_target_compatibility_line(line: &str, options: &LoweringOptions) -> Vec<String> {
    let trimmed = line.trim();
    if let Some((module, names)) =
        trimmed.strip_prefix("from ").and_then(|rest| rest.split_once(" import "))
        && matches!(module, "typing" | "typing_extensions" | "warnings")
    {
        let indentation = &line[..line.len() - trimmed.len()];
        let normalized =
            normalize_import_from_line(indentation, module, names, options.target_python);
        if normalized.len() != 1 || normalized[0].trim() != trimmed {
            return normalized;
        }
    }

    vec![normalize_target_compatibility_text(line, options)]
}

fn normalize_import_from_line(
    indentation: &str,
    module: &str,
    names: &str,
    target_python: PythonTarget,
) -> Vec<String> {
    let mut grouped = std::collections::BTreeMap::<String, Vec<String>>::new();
    let mut order = Vec::<String>::new();
    for raw_name in names.split(',') {
        let entry = raw_name.trim();
        if entry.is_empty() {
            continue;
        }
        let symbol = entry.split_once(" as ").map(|(name, _)| name.trim()).unwrap_or(entry);
        let target_module = compat_module_for_symbol(symbol, target_python).unwrap_or(module);
        if !order.iter().any(|existing| existing == target_module) {
            order.push(target_module.to_owned());
        }
        grouped.entry(target_module.to_owned()).or_default().push(entry.to_owned());
    }

    order
        .into_iter()
        .filter_map(|target_module| {
            let names = grouped.remove(&target_module)?;
            Some(format!("{indentation}from {target_module} import {}", names.join(", ")))
        })
        .collect()
}

fn normalize_target_compatibility_text(text: &str, options: &LoweringOptions) -> String {
    let mut normalized = text.to_owned();
    for symbol in [
        "Self",
        "Required",
        "NotRequired",
        "dataclass_transform",
        "override",
        "TypeVarTuple",
        "Unpack",
        "ReadOnly",
        "TypeIs",
        "NoDefault",
        "deprecated",
    ] {
        if let Some(owner) = options.target_python.stdlib_owner(symbol) {
            for source_module in ["typing", "typing_extensions", "warnings"] {
                if source_module == owner {
                    continue;
                }
                normalized = normalized
                    .replace(&format!("{source_module}.{symbol}"), &format!("{owner}.{symbol}"));
            }
        }
    }

    normalized
}

fn normalize_runtime_intrinsic_types(source: &str) -> String {
    let Ok(parsed) = parse_module(source) else {
        return source.to_owned();
    };
    let mut replacements = Vec::new();
    collect_intrinsic_type_replacements(source, parsed.suite(), &mut replacements);
    replacements
        .sort_by(|(left_range, _), (right_range, _)| right_range.start().cmp(&left_range.start()));

    let mut normalized = source.to_owned();
    for (range, replacement) in replacements {
        if let Some(existing) = slice_range(&normalized, range)
            && existing == replacement
        {
            continue;
        }
        normalized.replace_range(range.start().to_usize()..range.end().to_usize(), &replacement);
    }
    normalized
}

fn collect_intrinsic_type_replacements(
    source: &str,
    suite: &[Stmt],
    replacements: &mut Vec<(ruff_text_size::TextRange, String)>,
) {
    for statement in suite {
        match statement {
            Stmt::FunctionDef(function) => {
                collect_parameter_intrinsic_type_replacements(
                    source,
                    &function.parameters,
                    replacements,
                );
                if let Some(returns) = function.returns.as_deref()
                    && let Some(replacement) = normalize_intrinsic_type_text(
                        slice_range(source, returns.range()).unwrap_or_default(),
                    )
                {
                    replacements.push((returns.range(), replacement));
                }
            }
            Stmt::AnnAssign(assign) => {
                if let Some(replacement) = normalize_intrinsic_type_text(
                    slice_range(source, assign.annotation.range()).unwrap_or_default(),
                ) {
                    replacements.push((assign.annotation.range(), replacement));
                }
                if let Some(value) = assign.value.as_deref()
                    && slice_range(source, assign.annotation.range())
                        .is_some_and(|annotation| annotation.trim_end() == "TypeAlias")
                    && let Some(replacement) = normalize_intrinsic_type_text(
                        slice_range(source, value.range()).unwrap_or_default(),
                    )
                {
                    replacements.push((value.range(), replacement));
                }
            }
            Stmt::ClassDef(class_def) => {
                if let Some(arguments) = class_def.arguments.as_ref() {
                    for argument in &arguments.args {
                        if let Some(replacement) = normalize_intrinsic_type_text(
                            slice_range(source, argument.range()).unwrap_or_default(),
                        ) {
                            replacements.push((argument.range(), replacement));
                        }
                    }
                }
                collect_intrinsic_type_replacements(source, &class_def.body, replacements);
            }
            _ => {}
        }
    }
}

fn collect_parameter_intrinsic_type_replacements(
    source: &str,
    parameters: &ruff_python_ast::Parameters,
    replacements: &mut Vec<(ruff_text_size::TextRange, String)>,
) {
    let mut collect_annotation = |annotation: Option<&Expr>| {
        if let Some(annotation) = annotation
            && let Some(replacement) = normalize_intrinsic_type_text(
                slice_range(source, annotation.range()).unwrap_or_default(),
            )
        {
            replacements.push((annotation.range(), replacement));
        }
    };
    for parameter in &parameters.posonlyargs {
        collect_annotation(parameter.annotation());
    }
    for parameter in &parameters.args {
        collect_annotation(parameter.annotation());
    }
    if let Some(parameter) = parameters.vararg.as_ref() {
        collect_annotation(parameter.annotation());
    }
    for parameter in &parameters.kwonlyargs {
        collect_annotation(parameter.annotation());
    }
    if let Some(parameter) = parameters.kwarg.as_ref() {
        collect_annotation(parameter.annotation());
    }
}

fn normalize_intrinsic_type_text(text: &str) -> Option<String> {
    let mut normalized = String::new();
    let mut token = String::new();
    let mut changed = false;

    let flush_token = |token: &mut String, normalized: &mut String, changed: &mut bool| {
        if token.is_empty() {
            return;
        }
        match token.as_str() {
            "unknown" => {
                normalized.push_str("object");
                *changed = true;
            }
            "dynamic" => {
                normalized.push_str("Any");
                *changed = true;
            }
            _ => normalized.push_str(token),
        }
        token.clear();
    };

    for character in text.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            token.push(character);
        } else {
            flush_token(&mut token, &mut normalized, &mut changed);
            normalized.push(character);
        }
    }
    flush_token(&mut token, &mut normalized, &mut changed);

    changed.then_some(normalized)
}

fn has_any_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import Any"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("Any"))
    })
}

fn tree_uses_dynamic_intrinsic(tree: &SyntaxTree) -> bool {
    tree.statements.iter().any(statement_uses_dynamic_intrinsic)
}

fn statement_uses_dynamic_intrinsic(statement: &SyntaxStatement) -> bool {
    match statement {
        SyntaxStatement::TypeAlias(statement) => statement.value.contains("dynamic"),
        SyntaxStatement::Interface(statement)
        | SyntaxStatement::DataClass(statement)
        | SyntaxStatement::SealedClass(statement)
        | SyntaxStatement::ClassDef(statement) => {
            statement.header_suffix.contains("dynamic")
                || statement.members.iter().any(|member| {
                    member
                        .annotation
                        .as_deref()
                        .is_some_and(|annotation| annotation.contains("dynamic"))
                })
        }
        SyntaxStatement::FunctionDef(statement) | SyntaxStatement::OverloadDef(statement) => {
            statement.returns.as_deref().is_some_and(|returns| returns.contains("dynamic"))
                || statement.params.iter().any(|param| {
                    param
                        .annotation
                        .as_deref()
                        .is_some_and(|annotation| annotation.contains("dynamic"))
                })
        }
        SyntaxStatement::Value(statement) => {
            statement.annotation.as_deref().is_some_and(|annotation| annotation.contains("dynamic"))
        }
        _ => false,
    }
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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SplitCompatFunctionTypeParams {
    SharedWhenMetadataMatches,
    SplitDistinctFunctions,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
enum CompatTypeParamOwnerKind {
    TypeAlias,
    Interface,
    DataClass,
    SealedClass,
    ClassDef,
    Function,
    Overload,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct CompatTypeParamOwnerKey {
    kind: CompatTypeParamOwnerKind,
    name: String,
    line: usize,
}

impl CompatTypeParamOwnerKey {
    fn shares_function_binding(&self, other: &Self) -> bool {
        matches!(
            (&self.kind, &other.kind),
            (CompatTypeParamOwnerKind::Function | CompatTypeParamOwnerKind::Overload,
             CompatTypeParamOwnerKind::Function | CompatTypeParamOwnerKind::Overload)
        ) && self.name == other.name
    }
}

#[derive(Debug, Clone)]
struct CompatTypeParamOwner {
    key: CompatTypeParamOwnerKey,
    type_params: Vec<typepython_syntax::TypeParam>,
}

#[derive(Debug, Clone)]
struct RuntimeTypeParamBinding {
    source_name: String,
    emitted_name: String,
    type_param: RuntimeTypeParam,
    owners: Vec<CompatTypeParamOwnerKey>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct PendingRuntimeTypeParamBinding {
    source_name: String,
    type_param: RuntimeTypeParam,
    owners: Vec<CompatTypeParamOwnerKey>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct RuntimeTypeParam {
    kind: typepython_syntax::TypeParamKind,
    bound: Option<String>,
    constraints: Vec<String>,
    default: Option<String>,
}

fn collect_runtime_type_param_bindings(
    tree: &SyntaxTree,
    options: &LoweringOptions,
    split_function_type_params: SplitCompatFunctionTypeParams,
) -> Vec<RuntimeTypeParamBinding> {
    let owners = collect_compat_type_param_owners(tree, options);
    let mut bindings = Vec::<PendingRuntimeTypeParamBinding>::new();
    let mut binding_indices_by_source = std::collections::BTreeMap::<String, Vec<usize>>::new();

    for owner in owners {
        for type_param in &owner.type_params {
            let pending = PendingRuntimeTypeParamBinding {
                source_name: type_param.name.clone(),
                type_param: RuntimeTypeParam::from_type_param(type_param),
                owners: vec![owner.key.clone()],
            };
            let binding_index = binding_indices_by_source
                .entry(type_param.name.clone())
                .or_default()
                .iter()
                .copied()
                .find(|index| {
                    let candidate = &bindings[*index];
                    candidate.type_param == pending.type_param
                        && match split_function_type_params {
                            SplitCompatFunctionTypeParams::SharedWhenMetadataMatches => true,
                            SplitCompatFunctionTypeParams::SplitDistinctFunctions => candidate
                                .owners
                                .first()
                                .zip(pending.owners.first())
                                .is_none_or(|(left, right)| {
                                    !matches!(
                                        (&left.kind, &right.kind),
                                        (CompatTypeParamOwnerKind::Function
                                            | CompatTypeParamOwnerKind::Overload,
                                         CompatTypeParamOwnerKind::Function
                                            | CompatTypeParamOwnerKind::Overload)
                                    ) || left.shares_function_binding(right)
                                }),
                        }
                });
            if let Some(binding_index) = binding_index {
                bindings[binding_index].owners.push(owner.key.clone());
            } else {
                binding_indices_by_source
                    .entry(type_param.name.clone())
                    .or_default()
                    .push(bindings.len());
                bindings.push(pending);
            }
        }
    }

    let mut runtime_bindings = Vec::new();
    for (source_name, binding_indices) in binding_indices_by_source {
        let multiple_bindings = binding_indices.len() > 1;
        for (offset, binding_index) in binding_indices.into_iter().enumerate() {
            let pending = bindings[binding_index].clone();
            let emitted_name = if multiple_bindings {
                format!("__typepython_{}_{}", source_name, offset + 1)
            } else {
                source_name.clone()
            };
            runtime_bindings.push(RuntimeTypeParamBinding {
                source_name: source_name.clone(),
                emitted_name,
                type_param: pending.type_param,
                owners: pending.owners,
            });
        }
    }

    runtime_bindings
}

fn collect_compat_type_param_owners(
    tree: &SyntaxTree,
    options: &LoweringOptions,
) -> Vec<CompatTypeParamOwner> {
    tree.statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::TypeAlias(statement)
                if !statement.type_params.is_empty()
                    && !can_use_native_typealias(statement, options) =>
            {
                Some(CompatTypeParamOwner {
                    key: CompatTypeParamOwnerKey {
                        kind: CompatTypeParamOwnerKind::TypeAlias,
                        name: statement.name.clone(),
                        line: statement.line,
                    },
                    type_params: statement.type_params.clone(),
                })
            }
            SyntaxStatement::Interface(statement)
                if !statement.type_params.is_empty()
                    && !can_use_native_type_params(&statement.type_params, options) =>
            {
                Some(CompatTypeParamOwner {
                    key: CompatTypeParamOwnerKey {
                        kind: CompatTypeParamOwnerKind::Interface,
                        name: statement.name.clone(),
                        line: header_line_for_statement(&tree.source.text, statement.line),
                    },
                    type_params: statement.type_params.clone(),
                })
            }
            SyntaxStatement::DataClass(statement)
                if !statement.type_params.is_empty()
                    && !can_use_native_type_params(&statement.type_params, options) =>
            {
                Some(CompatTypeParamOwner {
                    key: CompatTypeParamOwnerKey {
                        kind: CompatTypeParamOwnerKind::DataClass,
                        name: statement.name.clone(),
                        line: header_line_for_statement(&tree.source.text, statement.line),
                    },
                    type_params: statement.type_params.clone(),
                })
            }
            SyntaxStatement::SealedClass(statement)
                if !statement.type_params.is_empty()
                    && !can_use_native_type_params(&statement.type_params, options) =>
            {
                Some(CompatTypeParamOwner {
                    key: CompatTypeParamOwnerKey {
                        kind: CompatTypeParamOwnerKind::SealedClass,
                        name: statement.name.clone(),
                        line: header_line_for_statement(&tree.source.text, statement.line),
                    },
                    type_params: statement.type_params.clone(),
                })
            }
            SyntaxStatement::ClassDef(statement)
                if !statement.type_params.is_empty()
                    && !can_use_native_type_params(&statement.type_params, options) =>
            {
                Some(CompatTypeParamOwner {
                    key: CompatTypeParamOwnerKey {
                        kind: CompatTypeParamOwnerKind::ClassDef,
                        name: statement.name.clone(),
                        line: header_line_for_statement(&tree.source.text, statement.line),
                    },
                    type_params: statement.type_params.clone(),
                })
            }
            SyntaxStatement::FunctionDef(statement)
                if !statement.type_params.is_empty()
                    && !can_use_native_type_params(&statement.type_params, options) =>
            {
                Some(CompatTypeParamOwner {
                    key: CompatTypeParamOwnerKey {
                        kind: CompatTypeParamOwnerKind::Function,
                        name: statement.name.clone(),
                        line: statement.line,
                    },
                    type_params: statement.type_params.clone(),
                })
            }
            SyntaxStatement::OverloadDef(statement)
                if !statement.type_params.is_empty()
                    && !can_use_native_type_params(&statement.type_params, options) =>
            {
                Some(CompatTypeParamOwner {
                    key: CompatTypeParamOwnerKey {
                        kind: CompatTypeParamOwnerKind::Overload,
                        name: statement.name.clone(),
                        line: statement.line,
                    },
                    type_params: statement.type_params.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

fn compat_type_param_rewrites_by_declaration_line(
    bindings: &[RuntimeTypeParamBinding],
) -> std::collections::BTreeMap<usize, std::collections::BTreeMap<String, String>> {
    let mut rewrites = std::collections::BTreeMap::new();
    for binding in bindings {
        if binding.emitted_name == binding.source_name {
            continue;
        }
        for owner in &binding.owners {
            rewrites
                .entry(owner.line)
                .or_insert_with(std::collections::BTreeMap::new)
                .insert(binding.source_name.clone(), binding.emitted_name.clone());
        }
    }
    rewrites
}

fn compat_type_param_rewrites_by_class_member_line(
    source: &str,
    bindings: &[RuntimeTypeParamBinding],
) -> std::collections::BTreeMap<usize, std::collections::BTreeMap<String, String>> {
    let mut rewrites = std::collections::BTreeMap::new();
    for binding in bindings {
        if binding.emitted_name == binding.source_name {
            continue;
        }
        for owner in &binding.owners {
            if !matches!(
                owner.kind,
                CompatTypeParamOwnerKind::Interface
                    | CompatTypeParamOwnerKind::DataClass
                    | CompatTypeParamOwnerKind::SealedClass
                    | CompatTypeParamOwnerKind::ClassDef
            ) {
                continue;
            }
            for line in compat_class_member_header_lines(source, owner.line) {
                rewrites
                    .entry(line)
                    .or_insert_with(std::collections::BTreeMap::new)
                    .insert(binding.source_name.clone(), binding.emitted_name.clone());
            }
        }
    }
    rewrites
}

fn compat_class_member_header_lines(source: &str, class_line: usize) -> Vec<usize> {
    let lines = source.lines().collect::<Vec<_>>();
    let Some(header_line) = lines.get(class_line.saturating_sub(1)) else {
        return Vec::new();
    };
    let class_indent = header_line.len() - header_line.trim_start().len();
    let member_indent = class_indent + 4;
    let mut member_lines = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(class_line) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let indentation = line.len() - trimmed.len();
        if indentation <= class_indent {
            break;
        }
        if indentation == member_indent && !trimmed.starts_with('@') {
            member_lines.push(index + 1);
        }
    }
    member_lines
}

impl RuntimeTypeParam {
    fn from_type_param(type_param: &typepython_syntax::TypeParam) -> Self {
        Self {
            kind: type_param.kind.clone(),
            bound: type_param.rendered_bound(),
            constraints: type_param.rendered_constraints(),
            default: type_param.rendered_default(),
        }
    }
}

fn has_compat_generic_class_like_declarations(
    interfaces: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    data_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    sealed_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    class_defs: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    options: &LoweringOptions,
) -> bool {
    interfaces.values().any(|statement| {
        !statement.type_params.is_empty()
            && !can_use_native_type_params(&statement.type_params, options)
    }) || data_classes.values().any(|statement| {
        !statement.type_params.is_empty()
            && !can_use_native_type_params(&statement.type_params, options)
    }) || sealed_classes.values().any(|statement| {
        !statement.type_params.is_empty()
            && !can_use_native_type_params(&statement.type_params, options)
    }) || class_defs.values().any(|statement| {
        !statement.type_params.is_empty()
            && !can_use_native_type_params(&statement.type_params, options)
    })
}

fn has_any_generic_type_params(
    type_aliases: &std::collections::BTreeMap<usize, &typepython_syntax::TypeAliasStatement>,
    interfaces: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    data_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    sealed_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    class_defs: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    function_defs: &std::collections::BTreeMap<usize, &typepython_syntax::FunctionStatement>,
    overloads: &std::collections::BTreeMap<usize, &typepython_syntax::FunctionStatement>,
) -> bool {
    type_aliases.values().any(|statement| !statement.type_params.is_empty())
        || interfaces.values().any(|statement| !statement.type_params.is_empty())
        || data_classes.values().any(|statement| !statement.type_params.is_empty())
        || sealed_classes.values().any(|statement| !statement.type_params.is_empty())
        || class_defs.values().any(|statement| !statement.type_params.is_empty())
        || function_defs.values().any(|statement| !statement.type_params.is_empty())
        || overloads.values().any(|statement| !statement.type_params.is_empty())
}

#[expect(
    clippy::too_many_arguments,
    reason = "runtime feature collection spans all lowerable declaration categories"
)]
fn collect_required_runtime_features(
    type_aliases: &std::collections::BTreeMap<usize, &typepython_syntax::TypeAliasStatement>,
    interfaces: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    data_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    sealed_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    class_defs: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    function_defs: &std::collections::BTreeMap<usize, &typepython_syntax::FunctionStatement>,
    overloads: &std::collections::BTreeMap<usize, &typepython_syntax::FunctionStatement>,
    lowered: &str,
    options: &LoweringOptions,
) -> BTreeSet<RuntimeFeature> {
    let mut features = BTreeSet::new();

    if type_aliases.values().any(|statement| can_use_native_typealias(statement, options)) {
        features.insert(RuntimeFeature::TypeStmt);
    }
    if interfaces
        .values()
        .any(|statement| can_use_native_type_params(&statement.type_params, options))
        || data_classes
            .values()
            .any(|statement| can_use_native_type_params(&statement.type_params, options))
        || sealed_classes
            .values()
            .any(|statement| can_use_native_type_params(&statement.type_params, options))
        || class_defs
            .values()
            .any(|statement| can_use_native_type_params(&statement.type_params, options))
        || function_defs
            .values()
            .any(|statement| can_use_native_type_params(&statement.type_params, options))
        || overloads
            .values()
            .any(|statement| can_use_native_type_params(&statement.type_params, options))
        || type_aliases.values().any(|statement| {
            can_use_native_typealias(statement, options) && !statement.type_params.is_empty()
        })
    {
        features.insert(RuntimeFeature::InlineTypeParams);
    }
    if type_aliases.values().any(|statement| {
        can_use_native_typealias(statement, options)
            && type_params_have_defaults(&statement.type_params)
    }) || interfaces.values().any(|statement| {
        can_use_native_type_params(&statement.type_params, options)
            && type_params_have_defaults(&statement.type_params)
    }) || data_classes.values().any(|statement| {
        can_use_native_type_params(&statement.type_params, options)
            && type_params_have_defaults(&statement.type_params)
    }) || sealed_classes.values().any(|statement| {
        can_use_native_type_params(&statement.type_params, options)
            && type_params_have_defaults(&statement.type_params)
    }) || class_defs.values().any(|statement| {
        can_use_native_type_params(&statement.type_params, options)
            && type_params_have_defaults(&statement.type_params)
    }) || function_defs.values().any(|statement| {
        can_use_native_type_params(&statement.type_params, options)
            && type_params_have_defaults(&statement.type_params)
    }) || overloads.values().any(|statement| {
        can_use_native_type_params(&statement.type_params, options)
            && type_params_have_defaults(&statement.type_params)
    }) {
        features.insert(RuntimeFeature::GenericDefaults);
    }

    if imports_symbol_from_module(lowered, "typing", "ReadOnly")
        || lowered.contains("typing.ReadOnly")
    {
        features.insert(RuntimeFeature::TypingReadOnly);
    }
    if imports_symbol_from_module(lowered, "typing", "TypeIs") || lowered.contains("typing.TypeIs")
    {
        features.insert(RuntimeFeature::TypingTypeIs);
    }
    if imports_symbol_from_module(lowered, "typing", "NoDefault")
        || lowered.contains("typing.NoDefault")
    {
        features.insert(RuntimeFeature::TypingNoDefault);
    }
    if imports_symbol_from_module(lowered, "warnings", "deprecated")
        || lowered.contains("warnings.deprecated")
    {
        features.insert(RuntimeFeature::WarningsDeprecated);
    }

    features
}

fn collect_required_backports(lowered: &str) -> BTreeSet<BackportRequirement> {
    let mut backports = BTreeSet::new();
    if lowered.contains("typing_extensions.")
        || lowered
            .lines()
            .any(|line| line.trim_start().starts_with("from typing_extensions import "))
        || lowered.lines().any(|line| line.trim_start() == "import typing_extensions")
    {
        backports.insert(BackportRequirement::TypingExtensionsAtLeast412);
    }
    backports
}

#[expect(
    clippy::too_many_arguments,
    reason = "export runtime semantics are derived across all declaration categories"
)]
fn collect_export_runtime_semantics(
    type_aliases: &std::collections::BTreeMap<usize, &typepython_syntax::TypeAliasStatement>,
    interfaces: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    data_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    sealed_classes: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    class_defs: &std::collections::BTreeMap<usize, &typepython_syntax::NamedBlockStatement>,
    function_defs: &std::collections::BTreeMap<usize, &typepython_syntax::FunctionStatement>,
    overloads: &std::collections::BTreeMap<usize, &typepython_syntax::FunctionStatement>,
    options: &LoweringOptions,
) -> std::collections::BTreeMap<String, RuntimeTypingSemantics> {
    let mut semantics = std::collections::BTreeMap::new();

    for statement in type_aliases.values() {
        if let Some(runtime) = native_alias_runtime_semantics(statement, options) {
            semantics.insert(statement.name.clone(), runtime);
        }
    }
    for statement in interfaces.values() {
        if let Some(runtime) = native_class_runtime_semantics(
            &statement.name,
            &statement.type_params,
            RuntimeTypingForm::NativeGenericClass,
            options,
        ) {
            semantics.insert(statement.name.clone(), runtime);
        }
    }
    for statement in data_classes.values() {
        if let Some(runtime) = native_class_runtime_semantics(
            &statement.name,
            &statement.type_params,
            RuntimeTypingForm::NativeGenericClass,
            options,
        ) {
            semantics.insert(statement.name.clone(), runtime);
        }
    }
    for statement in sealed_classes.values() {
        if let Some(runtime) = native_class_runtime_semantics(
            &statement.name,
            &statement.type_params,
            RuntimeTypingForm::NativeGenericClass,
            options,
        ) {
            semantics.insert(statement.name.clone(), runtime);
        }
    }
    for statement in class_defs.values() {
        if let Some(runtime) = native_class_runtime_semantics(
            &statement.name,
            &statement.type_params,
            RuntimeTypingForm::NativeGenericClass,
            options,
        ) {
            semantics.insert(statement.name.clone(), runtime);
        }
    }
    for statement in function_defs.values() {
        if let Some(runtime) = native_function_runtime_semantics(statement, options) {
            semantics.insert(statement.name.clone(), runtime);
        }
    }
    for statement in overloads.values() {
        if let Some(runtime) = native_function_runtime_semantics(statement, options) {
            semantics.entry(statement.name.clone()).or_insert(runtime);
        }
    }

    semantics
}

fn native_alias_runtime_semantics(
    statement: &typepython_syntax::TypeAliasStatement,
    options: &LoweringOptions,
) -> Option<RuntimeTypingSemantics> {
    can_use_native_typealias(statement, options).then(|| RuntimeTypingSemantics {
        form: RuntimeTypingForm::TypeAliasType,
        type_param_names: statement
            .type_params
            .iter()
            .map(|type_param| type_param.name.clone())
            .collect(),
        annotation_scope_owner: Some(statement.name.clone()),
        lazy_alias_value: true,
        local_type_params_hidden_from_globals: true,
        required_features: native_required_features(&statement.type_params, true),
    })
}

fn native_class_runtime_semantics(
    name: &str,
    type_params: &[typepython_syntax::TypeParam],
    form: RuntimeTypingForm,
    options: &LoweringOptions,
) -> Option<RuntimeTypingSemantics> {
    can_use_native_type_params(type_params, options).then(|| RuntimeTypingSemantics {
        form,
        type_param_names: type_params.iter().map(|type_param| type_param.name.clone()).collect(),
        annotation_scope_owner: Some(name.to_owned()),
        lazy_alias_value: false,
        local_type_params_hidden_from_globals: true,
        required_features: native_required_features(type_params, false),
    })
}

fn native_function_runtime_semantics(
    statement: &typepython_syntax::FunctionStatement,
    options: &LoweringOptions,
) -> Option<RuntimeTypingSemantics> {
    can_use_native_type_params(&statement.type_params, options).then(|| RuntimeTypingSemantics {
        form: RuntimeTypingForm::NativeGenericFunction,
        type_param_names: statement
            .type_params
            .iter()
            .map(|type_param| type_param.name.clone())
            .collect(),
        annotation_scope_owner: Some(statement.name.clone()),
        lazy_alias_value: false,
        local_type_params_hidden_from_globals: true,
        required_features: native_required_features(&statement.type_params, false),
    })
}

fn native_required_features(
    type_params: &[typepython_syntax::TypeParam],
    is_alias: bool,
) -> Vec<RuntimeFeature> {
    let mut features = Vec::new();
    if is_alias {
        features.push(RuntimeFeature::TypeStmt);
    }
    if !type_params.is_empty() {
        features.push(RuntimeFeature::InlineTypeParams);
    }
    if type_params_have_defaults(type_params) {
        features.push(RuntimeFeature::GenericDefaults);
    }
    features
}

fn type_params_have_defaults(type_params: &[typepython_syntax::TypeParam]) -> bool {
    type_params.iter().any(|type_param| type_param.default.is_some())
}

fn imports_symbol_from_module(source: &str, module: &str, symbol: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == format!("from {module} import {symbol}")
            || (trimmed.starts_with(&format!("from {module} import "))
                && trimmed.trim_start_matches(&format!("from {module} import ")).split(',').any(
                    |entry| {
                        entry
                            .trim()
                            .split_once(" as ")
                            .map(|(name, _)| name.trim())
                            .unwrap_or(entry.trim())
                            == symbol
                    },
                ))
    })
}

fn rewrite_unsafe_line(line: &str) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    format!("{indentation}if True:  # tpy:unsafe")
}

fn rewrite_typealias_line(
    line: &str,
    statement: &typepython_syntax::TypeAliasStatement,
    options: &LoweringOptions,
    type_param_rewrites: Option<&std::collections::BTreeMap<String, String>>,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let value =
        apply_type_param_rewrites(&statement.value, type_param_rewrites).unwrap_or_else(|| {
            statement.value.clone()
        });
    if can_use_native_typealias(statement, options) {
        format!(
            "{indentation}type {}{} = {}",
            statement.name,
            render_native_type_params(&statement.type_params),
            value
        )
    } else {
        format!("{indentation}{}: TypeAlias = {}", statement.name, value)
    }
}

fn rewrite_typevar_line(
    emitted_name: &str,
    source_name: &str,
    type_param: &RuntimeTypeParam,
) -> String {
    let mut args = vec![format!("\"{source_name}\"")];
    args.extend(type_param.constraints.iter().map(|constraint| format!("{constraint:?}")));
    if type_param.constraints.is_empty()
        && let Some(bound) = &type_param.bound
    {
        args.push(format!("bound={bound:?}"));
    }
    if let Some(default) = &type_param.default {
        args.push(format!("default={default:?}"));
    }
    match type_param.kind {
        typepython_syntax::TypeParamKind::TypeVar => {
            format!("{emitted_name} = TypeVar({})", args.join(", "))
        }
        typepython_syntax::TypeParamKind::ParamSpec => {
            format!("{emitted_name} = ParamSpec({})", args.join(", "))
        }
        typepython_syntax::TypeParamKind::TypeVarTuple => {
            format!("{emitted_name} = TypeVarTuple({source_name:?})")
        }
    }
}

fn has_typealias_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import TypeAlias"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("TypeAlias"))
    })
}

fn has_typevar_import(source: &str, module: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == format!("from {module} import TypeVar")
            || (trimmed.starts_with(&format!("from {module} import "))
                && trimmed.contains("TypeVar"))
    })
}

fn has_paramspec_import(source: &str, module: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == format!("from {module} import ParamSpec")
            || (trimmed.starts_with(&format!("from {module} import "))
                && trimmed.contains("ParamSpec"))
    })
}

fn has_typevartuple_import(source: &str, module: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == format!("from {module} import TypeVarTuple")
            || (trimmed.starts_with(&format!("from {module} import "))
                && trimmed.contains("TypeVarTuple"))
    })
}

fn has_unpack_import(source: &str, module: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == format!("from {module} import Unpack")
            || (trimmed.starts_with(&format!("from {module} import "))
                && trimmed.contains("Unpack"))
    })
}

fn rewrite_typevar_import_line(module: &str) -> String {
    format!("from {module} import TypeVar")
}

fn rewrite_paramspec_import_line(module: &str) -> String {
    format!("from {module} import ParamSpec")
}

fn rewrite_typevartuple_import_line(module: &str) -> String {
    format!("from {module} import TypeVarTuple")
}

fn rewrite_unpack_import_line(module: &str) -> String {
    format!("from {module} import Unpack")
}

fn has_unqualified_symbol_usage(source: &str, symbol: &str) -> bool {
    let needle = format!("{symbol}[");
    source.match_indices(&needle).any(|(index, _)| {
        source[..index]
            .chars()
            .next_back()
            .is_none_or(|prev| !(prev.is_ascii_alphanumeric() || prev == '_' || prev == '.'))
    })
}

fn has_generic_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import Generic"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("Generic"))
    })
}

fn has_module_import(source: &str, module: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == format!("import {module}")
            || trimmed.starts_with(&format!("import {module},"))
            || (trimmed.starts_with("import ")
                && trimmed
                    .trim_start_matches("import ")
                    .split(',')
                    .any(|entry| entry.trim() == module))
    })
}

fn rewrite_interface_line(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
    options: &LoweringOptions,
    type_param_rewrites: Option<&std::collections::BTreeMap<String, String>>,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let mut extras = vec![String::from("Protocol")];
    if !statement.type_params.is_empty()
        && !can_use_native_type_params(&statement.type_params, options)
    {
        extras.push(generic_base(statement, type_param_rewrites));
    }
    let header_suffix =
        apply_type_param_rewrites(&runtime_header_suffix(statement), type_param_rewrites)
            .unwrap_or_else(|| runtime_header_suffix(statement));
    let bases = append_bases(&header_suffix, &extras);
    format!(
        "{indentation}class {}{}{}:",
        statement.name,
        native_type_param_list_if_enabled(&statement.type_params, options),
        bases
    )
}

fn rewrite_data_class_lines(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
    options: &LoweringOptions,
    type_param_rewrites: Option<&std::collections::BTreeMap<String, String>>,
) -> [String; 2] {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let bases = append_optional_generic_base(statement, options, type_param_rewrites);

    [
        format!("{indentation}@dataclass"),
        format!(
            "{indentation}class {}{}{}:",
            statement.name,
            native_type_param_list_if_enabled(&statement.type_params, options),
            bases
        ),
    ]
}

fn rewrite_sealed_class_line(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
    options: &LoweringOptions,
    type_param_rewrites: Option<&std::collections::BTreeMap<String, String>>,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let bases = append_optional_generic_base(statement, options, type_param_rewrites);

    format!(
        "{indentation}class {}{}{}:  # tpy:sealed",
        statement.name,
        native_type_param_list_if_enabled(&statement.type_params, options),
        bases
    )
}

fn rewrite_class_def_line(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
    options: &LoweringOptions,
    type_param_rewrites: Option<&std::collections::BTreeMap<String, String>>,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let bases = append_optional_generic_base(statement, options, type_param_rewrites);
    format!(
        "{indentation}class {}{}{}:",
        statement.name,
        native_type_param_list_if_enabled(&statement.type_params, options),
        bases
    )
}

fn rewrite_function_def_line(
    line: &str,
    statement: &typepython_syntax::FunctionStatement,
    options: &LoweringOptions,
    type_param_rewrites: Option<&std::collections::BTreeMap<String, String>>,
) -> String {
    if can_use_native_type_params(&statement.type_params, options) {
        return line.to_owned();
    }
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let trimmed = line.trim_start();
    let rewritten = strip_generic_type_params(trimmed);
    let rewritten =
        apply_type_param_rewrites(&rewritten, type_param_rewrites).unwrap_or(rewritten);
    format!("{indentation}{rewritten}")
}

fn strip_generic_type_params(source: &str) -> String {
    let prefix_len = if source.starts_with("async def ") {
        "async def ".len()
    } else if source.starts_with("def ") {
        "def ".len()
    } else {
        return source.to_owned();
    };

    let name_len = source[prefix_len..]
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
        .map(char::len_utf8)
        .sum::<usize>();
    let bracket_index = prefix_len + name_len;
    if source.as_bytes().get(bracket_index) != Some(&b'[') {
        return source.to_owned();
    }

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

fn can_use_native_type_params(
    type_params: &[typepython_syntax::TypeParam],
    options: &LoweringOptions,
) -> bool {
    options.emit_style == EmitStyle::Native
        && options.target_python.supports_inline_type_params()
        && (options.target_python.supports_generic_defaults()
            || !type_params.iter().any(|type_param| type_param.default.is_some()))
}

fn can_use_native_typealias(
    statement: &typepython_syntax::TypeAliasStatement,
    options: &LoweringOptions,
) -> bool {
    options.emit_style == EmitStyle::Native
        && options.target_python.supports_type_stmt()
        && can_use_native_type_params(&statement.type_params, options)
}

fn native_type_param_list_if_enabled(
    type_params: &[typepython_syntax::TypeParam],
    options: &LoweringOptions,
) -> String {
    if can_use_native_type_params(type_params, options) {
        render_native_type_params(type_params)
    } else {
        String::new()
    }
}

fn render_native_type_params(type_params: &[typepython_syntax::TypeParam]) -> String {
    if type_params.is_empty() {
        return String::new();
    }

    format!("[{}]", type_params.iter().map(render_native_type_param).collect::<Vec<_>>().join(", "))
}

fn apply_type_param_rewrites(
    text: &str,
    type_param_rewrites: Option<&std::collections::BTreeMap<String, String>>,
) -> Option<String> {
    let type_param_rewrites = type_param_rewrites?;
    if type_param_rewrites.is_empty() {
        return None;
    }
    let rewritten = rewrite_type_param_tokens(text, type_param_rewrites);
    (rewritten != text).then_some(rewritten)
}

fn rewrite_type_param_tokens(
    text: &str,
    type_param_rewrites: &std::collections::BTreeMap<String, String>,
) -> String {
    let mut rewritten = String::with_capacity(text.len());
    let mut token = String::new();
    let mut quote = None::<char>;
    let mut escaped = false;

    let flush_token =
        |token: &mut String,
         rewritten: &mut String,
         type_param_rewrites: &std::collections::BTreeMap<String, String>| {
            if token.is_empty() {
                return;
            }
            if let Some(replacement) = type_param_rewrites.get(token) {
                rewritten.push_str(replacement);
            } else {
                rewritten.push_str(token);
            }
            token.clear();
        };

    for character in text.chars() {
        if let Some(active_quote) = quote {
            rewritten.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }

        if character == '\'' || character == '"' {
            flush_token(&mut token, &mut rewritten, type_param_rewrites);
            rewritten.push(character);
            quote = Some(character);
            continue;
        }

        if character.is_ascii_alphanumeric() || character == '_' {
            token.push(character);
            continue;
        }

        flush_token(&mut token, &mut rewritten, type_param_rewrites);
        rewritten.push(character);
    }

    flush_token(&mut token, &mut rewritten, type_param_rewrites);
    rewritten
}

fn render_native_type_param(type_param: &typepython_syntax::TypeParam) -> String {
    let prefix = match type_param.kind {
        typepython_syntax::TypeParamKind::TypeVar => "",
        typepython_syntax::TypeParamKind::ParamSpec => "**",
        typepython_syntax::TypeParamKind::TypeVarTuple => "*",
    };
    let mut rendered = if !type_param.constraints.is_empty() {
        format!(
            "{}: ({})",
            type_param.name,
            if type_param.constraint_exprs.is_empty() {
                type_param.constraints.join(", ")
            } else {
                type_param
                    .constraint_exprs
                    .iter()
                    .map(typepython_syntax::TypeExpr::render)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )
    } else if let Some(bound) = &type_param.bound_expr {
        format!("{}: {}", type_param.name, bound.render())
    } else if let Some(bound) = &type_param.bound {
        format!("{}: {}", type_param.name, bound)
    } else {
        type_param.name.clone()
    };
    rendered.insert_str(0, prefix);
    if let Some(default) = &type_param.default_expr {
        rendered.push_str(" = ");
        rendered.push_str(&default.render());
    } else if let Some(default) = &type_param.default {
        rendered.push_str(" = ");
        rendered.push_str(default);
    }
    rendered
}

fn append_optional_generic_base(
    statement: &typepython_syntax::NamedBlockStatement,
    options: &LoweringOptions,
    type_param_rewrites: Option<&std::collections::BTreeMap<String, String>>,
) -> String {
    let header_suffix =
        apply_type_param_rewrites(&runtime_header_suffix(statement), type_param_rewrites)
            .unwrap_or_else(|| runtime_header_suffix(statement));
    if statement.type_params.is_empty()
        || can_use_native_type_params(&statement.type_params, options)
    {
        if header_suffix.is_empty() { String::new() } else { header_suffix }
    } else {
        append_bases(&header_suffix, &[generic_base(statement, type_param_rewrites)])
    }
}

fn runtime_header_suffix(statement: &typepython_syntax::NamedBlockStatement) -> String {
    if !statement.bases.iter().any(|base| is_typed_dict_base(base)) {
        return statement.header_suffix.clone();
    }
    strip_typeddict_runtime_keywords(&statement.header_suffix)
}

fn strip_typeddict_runtime_keywords(header_suffix: &str) -> String {
    let trimmed = header_suffix.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let inner = trimmed.trim_start_matches('(').trim_end_matches(')').trim();
    if inner.is_empty() {
        return String::new();
    }

    let parts = split_header_suffix_args(inner)
        .into_iter()
        .filter(|part| {
            let keyword = part.split_once('=').map(|(name, _)| name.trim());
            !matches!(keyword, Some("closed" | "extra_items"))
        })
        .collect::<Vec<_>>();

    if parts.is_empty() { String::new() } else { format!("({})", parts.join(", ")) }
}

fn split_header_suffix_args(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut string_quote = None::<char>;
    let mut escaped = false;

    for character in text.chars() {
        if let Some(quote) = string_quote {
            current.push(character);
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
            } else if character == quote {
                string_quote = None;
            }
            continue;
        }

        match character {
            '\'' | '"' => {
                string_quote = Some(character);
                current.push(character);
            }
            '(' => {
                paren_depth += 1;
                current.push(character);
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                current.push(character);
            }
            '[' => {
                bracket_depth += 1;
                current.push(character);
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                current.push(character);
            }
            '{' => {
                brace_depth += 1;
                current.push(character);
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                current.push(character);
            }
            ',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                let part = current.trim();
                if !part.is_empty() {
                    parts.push(part.to_owned());
                }
                current.clear();
            }
            _ => current.push(character),
        }
    }

    let tail = current.trim();
    if !tail.is_empty() {
        parts.push(tail.to_owned());
    }
    parts
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

fn generic_base(
    statement: &typepython_syntax::NamedBlockStatement,
    type_param_rewrites: Option<&std::collections::BTreeMap<String, String>>,
) -> String {
    format!(
        "Generic[{}]",
        statement
            .type_params
            .iter()
            .map(|type_param| {
                type_param_rewrites
                    .and_then(|rewrites| rewrites.get(&type_param.name))
                    .cloned()
                    .unwrap_or_else(|| type_param.name.clone())
            })
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

fn rewrite_overload_lines(
    line: &str,
    statement: &typepython_syntax::FunctionStatement,
    options: &LoweringOptions,
    type_param_rewrites: Option<&std::collections::BTreeMap<String, String>>,
) -> [String; 2] {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let rewritten =
        line.trim_start().strip_prefix("overload ").unwrap_or_else(|| line.trim_start()).to_owned();
    let rewritten = if can_use_native_type_params(&statement.type_params, options) {
        rewritten
    } else {
        strip_generic_type_params(&rewritten)
    };
    let rewritten =
        apply_type_param_rewrites(&rewritten, type_param_rewrites).unwrap_or(rewritten);

    [format!("{indentation}@overload"), format!("{indentation}{rewritten}")]
}

fn has_overload_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import overload"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("overload"))
    })
}

pub(super) fn is_lowerable_named_block(statement: &typepython_syntax::NamedBlockStatement) -> bool {
    statement.header_suffix.is_empty()
        || (statement.header_suffix.starts_with('(') && statement.header_suffix.ends_with(')'))
}

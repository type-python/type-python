pub(crate) fn collect_missing_annotation_code_actions(
    workspace: &WorkspaceState,
    document: &DocumentState,
    range: LspRange,
) -> Vec<Value> {
    let line = range.start.line as usize + 1;
    document
        .syntax
        .statements
        .iter()
        .filter_map(|statement| {
            let SyntaxStatement::Value(value) = statement else {
                return None;
            };
            if value.line != line || value.annotation.is_some() || value.names.len() != 1 {
                return None;
            }
            let name = value.names.first()?;
            let inferred =
                resolve_value_statement_type_text(workspace, document, value, line + 1, 0)?;
            if inferred.is_empty() || inferred.contains("unknown") || inferred.contains("dynamic") {
                return None;
            }
            let name_range = find_name_range(&document.text, value.line, name)?;
            Some(code_action(
                format!("Add type annotation `{name}: {inferred}`"),
                &document.uri,
                vec![LspTextEdit {
                    range: LspRange { start: name_range.end, end: name_range.end },
                    new_text: format!(": {inferred}"),
                }],
            ))
        })
        .collect()
}

pub(crate) fn collect_common_migration_code_actions(
    document: &DocumentState,
    range: LspRange,
) -> Vec<Value> {
    let line = range.start.line as usize + 1;
    let mut actions = Vec::new();
    actions.extend(collect_class_migration_actions(document, line));
    actions.extend(collect_dict_shape_migration_actions(document, line));
    actions.extend(collect_unknown_annotation_actions(document, line));
    actions.extend(collect_selected_symbol_stub_preview_action(document, range));
    actions
}

fn collect_class_migration_actions(document: &DocumentState, line: usize) -> Vec<Value> {
    let mut actions = Vec::new();
    for statement in &document.syntax.statements {
        let SyntaxStatement::ClassDef(class) = statement else {
            continue;
        };
        if class.line != line {
            continue;
        }
        let Some(class_range) = find_name_range(&document.text, class.line, "class") else {
            continue;
        };
        actions.push(code_action(
            format!("Convert `{}` to TypePython data class", class.name),
            &document.uri,
            vec![LspTextEdit { range: class_range, new_text: String::from("data class") }],
        ));
        actions.push(code_action(
            format!("Extract interface from `{}`", class.name),
            &document.uri,
            vec![LspTextEdit {
                range: LspRange {
                    start: LspPosition { line: class.line.saturating_sub(1) as u32, character: 0 },
                    end: LspPosition { line: class.line.saturating_sub(1) as u32, character: 0 },
                },
                new_text: interface_stub_for_class(class),
            }],
        ));
        actions.push(code_action(
            format!("Convert `{}` DTO to shape-backed model", class.name),
            &document.uri,
            vec![LspTextEdit {
                range: LspRange {
                    start: LspPosition { line: class.line.saturating_sub(1) as u32, character: 0 },
                    end: LspPosition { line: class.line.saturating_sub(1) as u32, character: 0 },
                },
                new_text: format!("# tpy:shape-backed DTO candidate for `{}`\n", class.name),
            }],
        ));
        actions.push(symbol_stub_preview_action(document, &class.name));
    }
    actions
}

fn collect_dict_shape_migration_actions(document: &DocumentState, line: usize) -> Vec<Value> {
    let Some(line_text) = document.text.lines().nth(line.saturating_sub(1)) else {
        return Vec::new();
    };
    if !line_text.contains("= {") {
        return Vec::new();
    }
    let name = line_text.split_once('=').map(|(left, _)| left.trim()).unwrap_or("Shape");
    if name.is_empty() {
        return Vec::new();
    }
    let shape_name = format!("{}Shape", to_pascal_case(name));
    vec![code_action(
        format!("Extract TypedDict `{shape_name}` from dict literal"),
        &document.uri,
        vec![LspTextEdit {
            range: LspRange {
                start: LspPosition { line: line.saturating_sub(1) as u32, character: 0 },
                end: LspPosition { line: line.saturating_sub(1) as u32, character: 0 },
            },
            new_text: format!("class {shape_name}(TypedDict):\n    ...\n\n"),
        }],
    )]
}

fn collect_unknown_annotation_actions(document: &DocumentState, line: usize) -> Vec<Value> {
    document
        .syntax
        .statements
        .iter()
        .filter_map(|statement| {
            let SyntaxStatement::Value(value) = statement else {
                return None;
            };
            if value.line != line || value.annotation.is_some() || value.names.len() != 1 {
                return None;
            }
            let name = value.names.first()?;
            let name_range = find_name_range(&document.text, value.line, name)?;
            Some(code_action(
                format!("Insert minimal annotation for `{name}`"),
                &document.uri,
                vec![LspTextEdit {
                    range: LspRange { start: name_range.end, end: name_range.end },
                    new_text: String::from(": object"),
                }],
            ))
        })
        .collect()
}

fn collect_selected_symbol_stub_preview_action(document: &DocumentState, range: LspRange) -> Vec<Value> {
    let Some(token) = token_at_position(&document.text, range.start) else {
        return Vec::new();
    };
    if !document.local_symbols.contains_key(&token.name) {
        return Vec::new();
    }
    vec![symbol_stub_preview_action(document, &token.name)]
}

fn symbol_stub_preview_action(document: &DocumentState, name: &str) -> Value {
    let title = format!("Generate public `.pyi` preview for `{name}`");
    json!({
        "title": title,
        "kind": "source",
        "command": {
            "title": title,
            "command": "typepython.previewEmit",
            "arguments": [document.uri]
        }
    })
}

fn interface_stub_for_class(class: &NamedBlockStatement) -> String {
    let mut text = format!("interface {}Interface:\n", class.name);
    let mut emitted_member = false;
    for member in &class.members {
        if matches!(member.kind, typepython_syntax::ClassMemberKind::Method) {
            text.push_str(&format!("    def {}(...): ...\n", member.name));
            emitted_member = true;
        }
    }
    if !emitted_member {
        text.push_str("    ...\n");
    }
    text.push('\n');
    text
}

fn to_pascal_case(name: &str) -> String {
    let mut output = String::new();
    let mut capitalize = true;
    for ch in name.chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_') {
        if ch == '_' {
            capitalize = true;
            continue;
        }
        if capitalize {
            output.extend(ch.to_uppercase());
            capitalize = false;
        } else {
            output.push(ch);
        }
    }
    if output.is_empty() { String::from("Generated") } else { output }
}

pub(crate) fn collect_diagnostic_suggestion_code_actions(
    document: &DocumentState,
    range: LspRange,
    params: &Value,
) -> Vec<Value> {
    let diagnostics = params
        .get("context")
        .and_then(|context| context.get("diagnostics"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut actions = Vec::new();
    for diagnostic in diagnostics {
        let Some(data) = diagnostic.get("data") else {
            continue;
        };
        let Some(suggestions) = data.get("suggestions").and_then(Value::as_array) else {
            continue;
        };
        for suggestion in suggestions {
            let applicability =
                suggestion.get("applicability").and_then(Value::as_str).unwrap_or_default();
            if applicability != "machineApplicable" {
                continue;
            }
            let Some(span) = suggestion.get("span") else {
                continue;
            };
            let span_value = |name: &str| {
                span.get(name)
                    .and_then(Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(1)
            };
            let suggestion_range = LspRange {
                start: lsp_position_from_scalar_column(
                    &document.text,
                    span_value("line"),
                    span_value("column"),
                ),
                end: lsp_position_from_scalar_column(
                    &document.text,
                    span_value("end_line"),
                    span_value("end_column"),
                ),
            };
            if !range_intersects(range, suggestion_range) {
                continue;
            }
            let replacement =
                suggestion.get("replacement").and_then(Value::as_str).unwrap_or_default();
            let title = suggestion
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Apply suggested fix")
                .to_owned();
            actions.push(code_action(
                title,
                &document.uri,
                vec![LspTextEdit { range: suggestion_range, new_text: replacement.to_owned() }],
            ));
        }
    }
    actions
}

pub(crate) fn collect_unsafe_code_actions(
    document: &DocumentState,
    range: LspRange,
    params: &Value,
) -> Vec<Value> {
    let diagnostics = params
        .get("context")
        .and_then(|context| context.get("diagnostics"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let line = range.start.line as usize + 1;
    let line_text = document_line_text(&document.text, line);
    if line_text.trim_start().starts_with("unsafe:") {
        return Vec::new();
    }
    let has_unsafe_diagnostic = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.get("code").and_then(Value::as_str) == Some("TPY4019"));
    if !has_unsafe_diagnostic {
        return Vec::new();
    }

    let indent = line_text.chars().take_while(|ch| ch.is_whitespace()).collect::<String>();
    let trimmed = line_text.trim_start();
    let replacement = format!("{indent}unsafe:\n{indent}    {trimmed}");
    vec![code_action(
        String::from("Wrap in `unsafe:` block"),
        &document.uri,
        vec![LspTextEdit {
            range: LspRange {
                start: LspPosition { line: range.start.line, character: 0 },
                end: LspPosition {
                    line: range.start.line,
                    character: utf16_len(line_text),
                },
            },
            new_text: replacement,
        }],
    )]
}

pub(crate) fn collect_effect_declaration_code_actions(
    document: &DocumentState,
    range: LspRange,
    params: &Value,
) -> Vec<Value> {
    let diagnostics = params
        .get("context")
        .and_then(|context| context.get("diagnostics"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let effect_labels = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.get("code").and_then(Value::as_str) == Some("TPY4026"))
        .and_then(|diagnostic| diagnostic.get("message").and_then(Value::as_str))
        .map(extract_effect_row_labels)
        .unwrap_or_default();
    if effect_labels.is_empty() {
        return Vec::new();
    }
    let title_text = effect_decorator_title_text(&effect_labels);
    let Some((line_index, indent)) = nearest_enclosing_function_line(&document.text, range.start.line)
    else {
        return Vec::new();
    };
    let decorator_text = effect_decorator_text(&effect_labels, &indent);
    let previous_line = line_index
        .checked_sub(1)
        .and_then(|index| document.text.lines().nth(index as usize));
    if previous_line.is_some_and(|line| {
        let trimmed = line.trim();
        trimmed == "@effect_pure" || trimmed == "@pure"
    }) {
        return vec![code_action(
            format!("Replace pure marker with {title_text}"),
            &document.uri,
            vec![LspTextEdit {
                range: LspRange {
                    start: LspPosition { line: line_index - 1, character: 0 },
                    end: LspPosition {
                        line: line_index - 1,
                        character: utf16_len(previous_line.unwrap_or_default()),
                    },
                },
                new_text: decorator_text,
            }],
        )];
    }
    vec![code_action(
        format!("Add {title_text} to caller"),
        &document.uri,
        vec![LspTextEdit {
            range: LspRange {
                start: LspPosition { line: line_index, character: 0 },
                end: LspPosition { line: line_index, character: 0 },
            },
            new_text: format!("{decorator_text}\n"),
        }],
    )]
}

fn extract_effect_row_labels(message: &str) -> Vec<String> {
    let Some((_, rest)) = message.split_once("effect row `") else {
        return Vec::new();
    };
    let Some((label, _)) = rest.split_once('`') else {
        return Vec::new();
    };
    label
        .split(',')
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
        .collect()
}

fn effect_decorator_text(labels: &[String], indent: &str) -> String {
    labels
        .iter()
        .map(|label| format!("{indent}@effect(\"{label}\")"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn effect_decorator_title_text(labels: &[String]) -> String {
    labels
        .iter()
        .map(|label| format!("`@effect(\"{label}\")`"))
        .collect::<Vec<_>>()
        .join(" + ")
}

fn nearest_enclosing_function_line(source: &str, diagnostic_line: u32) -> Option<(u32, String)> {
    let mut nearest = None;
    for (index, line) in source.lines().take(diagnostic_line as usize + 1).enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("def ") || trimmed.starts_with("async def ") {
            let indent = line.chars().take_while(|ch| ch.is_whitespace()).collect::<String>();
            nearest = Some((index as u32, indent));
        }
    }
    nearest
}

pub(crate) fn collect_missing_import_code_actions(
    workspace: &WorkspaceState,
    document: &DocumentState,
    range: LspRange,
) -> Vec<Value> {
    let Some(token) = token_at_position(&document.text, range.start) else {
        return Vec::new();
    };
    if token.preceded_by_dot || document.local_symbols.contains_key(&token.name) {
        return Vec::new();
    }

    let current_module = &document.syntax.source.logical_module;
    let mut candidates = workspace
        .graph
        .nodes
        .iter()
        .filter(|node| node.module_key != *current_module)
        .filter_map(|node| {
            node.declarations
                .iter()
                .find(|declaration| declaration.owner.is_none() && declaration.name == token.name)
                .map(|_| node.module_key.clone())
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    if candidates.len() != 1 {
        return Vec::new();
    }

    let import_line = format!("from {} import {}\n", candidates[0], token.name);
    vec![code_action(
        format!("Import `{}` from `{}`", token.name, candidates[0]),
        &document.uri,
        vec![LspTextEdit { range: import_insertion_range(document), new_text: import_line }],
    )]
}

pub(crate) fn collect_portable_typing_rewrite_code_actions(
    document: &DocumentState,
    range: LspRange,
) -> Vec<Value> {
    let mut actions = portable_typing_rewrite_sites(&document.text)
        .into_iter()
        .filter(|site| range_intersects(range, site.range))
        .map(|site| {
            code_action(
                format!(
                    "Rewrite `{}` to checker-portable `{}`",
                    site.legacy, site.replacement
                ),
                &document.uri,
                vec![LspTextEdit { range: site.range, new_text: site.replacement }],
            )
        })
        .collect::<Vec<_>>();
    actions.extend(collect_typing_extensions_import_actions(document, range));
    actions.extend(collect_overload_normalization_actions(document, range));
    actions.extend(collect_typeddict_requiredness_actions(document, range));
    actions
}

fn collect_typing_extensions_import_actions(
    document: &DocumentState,
    range: LspRange,
) -> Vec<Value> {
    let line = range.start.line as usize + 1;
    let Some(line_text) = document.text.lines().nth(line.saturating_sub(1)) else {
        return Vec::new();
    };
    if !line_text.trim_start().starts_with("from typing import ") {
        return Vec::new();
    }
    let portable_names = ["NotRequired", "Required", "ReadOnly", "TypeIs", "TypeVarTuple", "Unpack"];
    if !portable_names.iter().any(|name| line_text.contains(name)) {
        return Vec::new();
    }
    vec![code_action(
        String::from("Select target-compatible `typing_extensions` import"),
        &document.uri,
        vec![LspTextEdit {
            range: LspRange {
                start: LspPosition { line: line.saturating_sub(1) as u32, character: 0 },
                end: LspPosition {
                    line: line.saturating_sub(1) as u32,
                    character: utf16_len(line_text),
                },
            },
            new_text: line_text.replacen("from typing import ", "from typing_extensions import ", 1),
        }],
    )]
}

fn collect_overload_normalization_actions(document: &DocumentState, range: LspRange) -> Vec<Value> {
    let line = range.start.line as usize + 1;
    let Some(line_text) = document.text.lines().nth(line.saturating_sub(1)) else {
        return Vec::new();
    };
    let Some(prefix_index) = line_text.find("overload def ") else {
        return Vec::new();
    };
    let indent = line_text.chars().take_while(|ch| ch.is_whitespace()).collect::<String>();
    let replacement = format!(
        "{indent}@overload\n{}def {}",
        &line_text[..prefix_index],
        &line_text[prefix_index + "overload def ".len()..]
    );
    vec![code_action(
        String::from("Normalize overload to standard `@overload` form"),
        &document.uri,
        vec![LspTextEdit {
            range: LspRange {
                start: LspPosition { line: line.saturating_sub(1) as u32, character: 0 },
                end: LspPosition {
                    line: line.saturating_sub(1) as u32,
                    character: utf16_len(line_text),
                },
            },
            new_text: replacement,
        }],
    )]
}

fn collect_typeddict_requiredness_actions(
    document: &DocumentState,
    range: LspRange,
) -> Vec<Value> {
    let line = range.start.line as usize + 1;
    let Some(line_text) = document.text.lines().nth(line.saturating_sub(1)) else {
        return Vec::new();
    };
    if !line_text.contains(':') || line_text.contains("NotRequired[") || line_text.contains("Required[") {
        return Vec::new();
    }
    let inside_typeddict = document.syntax.statements.iter().any(|statement| {
        let SyntaxStatement::ClassDef(class) = statement else {
            return false;
        };
        class.line < line
            && class.bases.iter().any(|base| base.ends_with("TypedDict"))
            && class.members.iter().any(|member| member.line == line)
    });
    if !inside_typeddict {
        return Vec::new();
    }
    let Some((left, right)) = line_text.split_once(':') else {
        return Vec::new();
    };
    vec![code_action(
        String::from("Mark TypedDict key as `NotRequired`"),
        &document.uri,
        vec![LspTextEdit {
            range: LspRange {
                start: LspPosition { line: line.saturating_sub(1) as u32, character: 0 },
                end: LspPosition {
                    line: line.saturating_sub(1) as u32,
                    character: utf16_len(line_text),
                },
            },
            new_text: format!("{}: NotRequired[{}]", left, right.trim()),
        }],
    )]
}

pub(crate) fn collect_project_workflow_code_actions(document: &DocumentState) -> Vec<Value> {
    vec![
        command_code_action(
            String::from("Run `typepython migrate --report`"),
            String::from("typepython.migrateReport"),
            &document.uri,
        ),
        command_code_action(
            String::from("Run `typepython compat`"),
            String::from("typepython.compat"),
            &document.uri,
        ),
        command_code_action(
            String::from("Run `typepython type-health`"),
            String::from("typepython.typeHealth"),
            &document.uri,
        ),
        command_code_action(
            String::from("Preview emitted `.py` and `.pyi`"),
            String::from("typepython.previewEmit"),
            &document.uri,
        ),
    ]
}

pub(crate) fn collect_type_source_code_actions(
    document: &DocumentState,
    range: LspRange,
) -> Vec<Value> {
    let Some(token) = token_at_position(&document.text, range.start) else {
        return Vec::new();
    };
    if !matches!(token.name.as_str(), "Any" | "unknown") {
        return Vec::new();
    }
    let title = format!("Find source of `{}`", token.name);
    vec![json!({
        "title": title,
        "kind": "quickfix",
        "command": {
            "title": title,
            "command": "typepython.findTypeSource",
            "arguments": [document.uri, token.range.start.line, token.range.start.character]
        }
    })]
}

pub(crate) fn code_action(title: String, uri: &str, edits: Vec<LspTextEdit>) -> Value {
    json!({
        "title": title,
        "kind": "quickfix",
        "edit": {
            "changes": {
                uri: edits
            }
        }
    })
}

pub(crate) fn command_code_action(title: String, command: String, uri: &str) -> Value {
    json!({
        "title": title,
        "kind": "source",
        "command": {
            "title": title,
            "command": command,
            "arguments": [uri]
        }
    })
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PortableTypingRewriteSite {
    legacy: String,
    replacement: String,
    range: LspRange,
}

pub(crate) fn portable_typing_rewrite_sites(text: &str) -> Vec<PortableTypingRewriteSite> {
    let mut sites = Vec::new();
    let imported_legacy_names = imported_legacy_typing_names(text);
    for (line_index, line) in text.lines().enumerate() {
        collect_qualified_portable_typing_sites(line_index, line, &mut sites);
        collect_imported_portable_typing_sites(line_index, line, &imported_legacy_names, &mut sites);
    }
    sites
}

fn imported_legacy_typing_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("from typing import ") {
            continue;
        }
        for item in trimmed.trim_start_matches("from typing import ").split(',') {
            let Some(name) = item.split_whitespace().next() else {
                continue;
            };
            if matches!(name, "List" | "Dict" | "Tuple" | "Set" | "FrozenSet") {
                names.push(name.to_owned());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

fn collect_qualified_portable_typing_sites(
    line_index: usize,
    line: &str,
    sites: &mut Vec<PortableTypingRewriteSite>,
) {
    for (legacy, replacement) in [
        ("typing.List", "list"),
        ("typing.Dict", "dict"),
        ("typing.Tuple", "tuple"),
        ("typing.Set", "set"),
        ("typing.FrozenSet", "frozenset"),
    ] {
        let mut search_start = 0usize;
        while let Some(relative) = line[search_start..].find(legacy) {
            let start = search_start + relative;
            let end = start + legacy.len();
            let start_character = utf16_len(&line[..start]);
            let end_character = utf16_len(&line[..end]);
            sites.push(PortableTypingRewriteSite {
                legacy: legacy.to_owned(),
                replacement: replacement.to_owned(),
                range: LspRange {
                    start: LspPosition {
                        line: line_index as u32,
                        character: start_character,
                    },
                    end: LspPosition {
                        line: line_index as u32,
                        character: end_character,
                    },
                },
            });
            search_start = end;
        }
    }
}

fn collect_imported_portable_typing_sites(
    line_index: usize,
    line: &str,
    imported_legacy_names: &[String],
    sites: &mut Vec<PortableTypingRewriteSite>,
) {
    if line.trim_start().starts_with("from typing import ") {
        return;
    }
    let legacy_names = [
        ("List", "list"),
        ("Dict", "dict"),
        ("Tuple", "tuple"),
        ("Set", "set"),
        ("FrozenSet", "frozenset"),
    ];
    for token in tokenize_identifiers(line) {
        let Some((legacy, replacement)) = legacy_names.iter().find(|(legacy, _)| token.name == *legacy)
        else {
            continue;
        };
        if token.preceded_by_dot || !imported_legacy_names.iter().any(|name| name == legacy) {
            continue;
        }
        sites.push(PortableTypingRewriteSite {
            legacy: (*legacy).to_owned(),
            replacement: (*replacement).to_owned(),
            range: LspRange {
                start: LspPosition {
                    line: line_index as u32,
                    character: token.range.start.character,
                },
                end: LspPosition { line: line_index as u32, character: token.range.end.character },
            },
        });
    }
}

pub(crate) fn import_insertion_range(document: &DocumentState) -> LspRange {
    let import_line = document
        .syntax
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::Import(statement) => Some(statement.line),
            _ => None,
        })
        .max();
    let insert_line = import_line.unwrap_or(0);
    LspRange {
        start: LspPosition { line: insert_line as u32, character: 0 },
        end: LspPosition { line: insert_line as u32, character: 0 },
    }
}

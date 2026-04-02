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
            let suggestion_range = LspRange {
                start: LspPosition {
                    line: span.get("line").and_then(Value::as_u64).unwrap_or(1).saturating_sub(1)
                        as u32,
                    character: span
                        .get("column")
                        .and_then(Value::as_u64)
                        .unwrap_or(1)
                        .saturating_sub(1) as u32,
                },
                end: LspPosition {
                    line: span
                        .get("end_line")
                        .and_then(Value::as_u64)
                        .unwrap_or(1)
                        .saturating_sub(1) as u32,
                    character: span
                        .get("end_column")
                        .and_then(Value::as_u64)
                        .unwrap_or(1)
                        .saturating_sub(1) as u32,
                },
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
                    character: line_text.chars().count() as u32,
                },
            },
            new_text: replacement,
        }],
    )]
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


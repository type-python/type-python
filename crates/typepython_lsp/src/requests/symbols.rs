use super::*;

pub(crate) fn collect_document_symbols(document: &DocumentState) -> Vec<LspDocumentSymbol> {
    document
        .syntax
        .statements
        .iter()
        .flat_map(|statement| match statement {
            SyntaxStatement::TypeAlias(statement) => {
                find_name_range(&document.text, statement.line, &statement.name)
                    .map(|selection_range| {
                        vec![LspDocumentSymbol {
                            name: statement.name.clone(),
                            kind: 13,
                            range: single_line_range(&document.text, statement.line),
                            selection_range,
                            detail: Some(statement.value.clone()),
                            children: Vec::new(),
                        }]
                    })
                    .unwrap_or_default()
            }
            SyntaxStatement::Interface(statement) => {
                collect_type_block_symbols(document, statement, 11)
            }
            SyntaxStatement::DataClass(statement)
            | SyntaxStatement::SealedClass(statement)
            | SyntaxStatement::ClassDef(statement) => {
                collect_type_block_symbols(document, statement, 5)
            }
            SyntaxStatement::OverloadDef(statement) | SyntaxStatement::FunctionDef(statement) => {
                find_name_range(&document.text, statement.line, &statement.name)
                    .map(|selection_range| {
                        vec![LspDocumentSymbol {
                            name: statement.name.clone(),
                            kind: 12,
                            range: block_range(&document.text, statement.line),
                            selection_range,
                            detail: Some(format_signature(
                                &statement.params,
                                statement.returns.as_deref(),
                            )),
                            children: Vec::new(),
                        }]
                    })
                    .unwrap_or_default()
            }
            SyntaxStatement::Value(statement) => statement
                .names
                .iter()
                .filter_map(|name| {
                    find_name_range(&document.text, statement.line, name).map(|selection_range| {
                        LspDocumentSymbol {
                            name: name.clone(),
                            kind: value_symbol_kind(name, statement.is_final),
                            range: single_line_range(&document.text, statement.line),
                            selection_range,
                            detail: statement
                                .annotation
                                .clone()
                                .or_else(|| statement.rendered_value_type()),
                            children: Vec::new(),
                        }
                    })
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

pub(crate) fn collect_type_block_symbols(
    document: &DocumentState,
    statement: &NamedBlockStatement,
    kind: u32,
) -> Vec<LspDocumentSymbol> {
    find_name_range(&document.text, statement.line, &statement.name)
        .map(|selection_range| {
            vec![LspDocumentSymbol {
                name: statement.name.clone(),
                kind,
                range: block_range(&document.text, statement.line),
                selection_range,
                detail: (!statement.bases.is_empty()).then(|| statement.bases.join(", ")),
                children: collect_class_member_symbols(document, statement),
            }]
        })
        .unwrap_or_default()
}

pub(crate) fn collect_class_member_symbols(
    document: &DocumentState,
    class_like: &NamedBlockStatement,
) -> Vec<LspDocumentSymbol> {
    class_like
        .members
        .iter()
        .filter_map(|member| {
            find_name_range(&document.text, member.line, &member.name).map(|selection_range| {
                let kind = match member.kind {
                    typepython_syntax::ClassMemberKind::Field => {
                        value_symbol_kind(&member.name, member.is_final)
                    }
                    typepython_syntax::ClassMemberKind::Method
                    | typepython_syntax::ClassMemberKind::Overload => match member.method_kind {
                        Some(typepython_syntax::MethodKind::Property)
                        | Some(typepython_syntax::MethodKind::PropertySetter) => 7,
                        _ => 6,
                    },
                };
                let detail = match member.kind {
                    typepython_syntax::ClassMemberKind::Field => {
                        member.rendered_annotation().or_else(|| member.rendered_value_type())
                    }
                    typepython_syntax::ClassMemberKind::Method
                    | typepython_syntax::ClassMemberKind::Overload => {
                        Some(format_signature(&member.params, member.returns.as_deref()))
                    }
                };
                LspDocumentSymbol {
                    name: member.name.clone(),
                    kind,
                    range: match member.kind {
                        typepython_syntax::ClassMemberKind::Field => {
                            single_line_range(&document.text, member.line)
                        }
                        typepython_syntax::ClassMemberKind::Method
                        | typepython_syntax::ClassMemberKind::Overload => {
                            block_range(&document.text, member.line)
                        }
                    },
                    selection_range,
                    detail,
                    children: Vec::new(),
                }
            })
        })
        .collect()
}

pub(crate) fn value_symbol_kind(name: &str, is_final: bool) -> u32 {
    if is_final || name.chars().all(|ch| !ch.is_ascii_lowercase()) { 14 } else { 13 }
}

pub(crate) fn single_line_range(text: &str, line: usize) -> LspRange {
    let line_text = text.lines().nth(line.saturating_sub(1)).unwrap_or_default();
    LspRange {
        start: LspPosition { line: line.saturating_sub(1) as u32, character: 0 },
        end: LspPosition {
            line: line.saturating_sub(1) as u32,
            character: line_text.chars().count() as u32,
        },
    }
}

pub(crate) fn block_range(text: &str, line: usize) -> LspRange {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return LspRange {
            start: LspPosition { line: line.saturating_sub(1) as u32, character: 0 },
            end: LspPosition { line: line.saturating_sub(1) as u32, character: 0 },
        };
    }

    let start_index = line.saturating_sub(1).min(lines.len().saturating_sub(1));
    let start_indent = line_indentation(lines[start_index]);
    let mut end_index = start_index;
    for (index, candidate) in lines.iter().enumerate().skip(start_index + 1) {
        if candidate.trim().is_empty() {
            end_index = index;
            continue;
        }
        if line_indentation(candidate) <= start_indent {
            break;
        }
        end_index = index;
    }

    LspRange {
        start: LspPosition { line: start_index as u32, character: 0 },
        end: LspPosition {
            line: end_index as u32,
            character: lines[end_index].chars().count() as u32,
        },
    }
}

pub(crate) fn workspace_symbol_metadata(
    workspace: &WorkspaceState,
    canonical: &str,
) -> Option<(u32, Option<String>)> {
    let (node, declaration) = binding_declaration_for_canonical(workspace, canonical)?;
    let kind = match declaration.kind {
        typepython_binding::DeclarationKind::TypeAlias => 13,
        typepython_binding::DeclarationKind::Class => match declaration.class_kind {
            Some(typepython_binding::DeclarationOwnerKind::Interface) => 11,
            _ => 5,
        },
        typepython_binding::DeclarationKind::Function
        | typepython_binding::DeclarationKind::Overload => {
            if declaration.owner.is_some() {
                match declaration.method_kind {
                    Some(typepython_syntax::MethodKind::Property)
                    | Some(typepython_syntax::MethodKind::PropertySetter) => 7,
                    _ => 6,
                }
            } else {
                12
            }
        }
        typepython_binding::DeclarationKind::Value => {
            if declaration.owner.is_some() {
                if declaration.is_final { 14 } else { 8 }
            } else if declaration.is_final {
                14
            } else {
                13
            }
        }
        typepython_binding::DeclarationKind::Import => 2,
    };
    let container_name = declaration
        .owner
        .as_ref()
        .map(|owner| owner.name.clone())
        .or_else(|| (!node.module_key.is_empty()).then(|| node.module_key.clone()));
    Some((kind, container_name))
}

pub(crate) fn binding_declaration_for_canonical<'a>(
    workspace: &'a WorkspaceState,
    canonical: &str,
) -> Option<(&'a ModuleNode, &'a typepython_binding::Declaration)> {
    let module_key = canonical_module_key(workspace, canonical)?;
    let node = workspace.queries.nodes_by_module_key.get(&module_key)?;
    node.declarations.iter().find_map(|declaration| {
        let declaration_canonical = declaration_canonical(node, declaration);
        (declaration_canonical == canonical).then_some((node, declaration))
    })
}

pub(crate) fn declaration_canonical(
    node: &ModuleNode,
    declaration: &typepython_binding::Declaration,
) -> String {
    match &declaration.owner {
        Some(owner) => format!("{}.{}.{}", node.module_key, owner.name, declaration.name),
        None => format!("{}.{}", node.module_key, declaration.name),
    }
}

pub(crate) fn canonical_module_key(workspace: &WorkspaceState, canonical: &str) -> Option<String> {
    let mut current = canonical.to_owned();
    loop {
        if workspace.queries.nodes_by_module_key.contains_key(&current) {
            return Some(current);
        }
        let (prefix, _) = current.rsplit_once('.')?;
        current = prefix.to_owned();
    }
}

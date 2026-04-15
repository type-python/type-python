pub(crate) fn collect_declarations(
    document: &DocumentState,
) -> (BTreeMap<String, String>, Vec<SymbolOccurrence>) {
    let mut local_symbols = BTreeMap::new();
    let mut declarations = Vec::new();
    let module_key = &document.syntax.source.logical_module;

    for statement in &document.syntax.statements {
        match statement {
            SyntaxStatement::TypeAlias(statement) => {
                let canonical = format!("{module_key}.{}", statement.name);
                local_symbols.insert(statement.name.clone(), canonical.clone());
                if let Some(range) =
                    find_name_range(&document.text, statement.line, &statement.name)
                {
                    declarations.push(SymbolOccurrence {
                        canonical,
                        name: statement.name.clone(),
                        uri: document.uri.clone(),
                        range,
                        legacy_detail: format!(
                            "typealias {} = {}",
                            statement.name, statement.value
                        ),
                        declaration: true,
                    });
                }
            }
            SyntaxStatement::Interface(statement)
            | SyntaxStatement::DataClass(statement)
            | SyntaxStatement::SealedClass(statement)
            | SyntaxStatement::ClassDef(statement) => {
                let canonical = format!("{module_key}.{}", statement.name);
                local_symbols.insert(statement.name.clone(), canonical.clone());
                if let Some(range) =
                    find_name_range(&document.text, statement.line, &statement.name)
                {
                    declarations.push(SymbolOccurrence {
                        canonical: canonical.clone(),
                        name: statement.name.clone(),
                        uri: document.uri.clone(),
                        range,
                        legacy_detail: format!(
                            "class {}({})",
                            statement.name,
                            statement.bases.join(", ")
                        ),
                        declaration: true,
                    });
                }
                for member in &statement.members {
                    let member_canonical = format!("{canonical}.{}", member.name);
                    if let Some(range) = find_name_range(&document.text, member.line, &member.name)
                    {
                        declarations.push(SymbolOccurrence {
                            canonical: member_canonical,
                            name: member.name.clone(),
                            uri: document.uri.clone(),
                            range,
                            legacy_detail: match member.kind {
                                typepython_syntax::ClassMemberKind::Field => format!(
                                    "field {}: {}",
                                    member.name,
                                    member.annotation.clone().unwrap_or_default()
                                ),
                                typepython_syntax::ClassMemberKind::Method
                                | typepython_syntax::ClassMemberKind::Overload => format!(
                                    "method {}{}",
                                    member.name,
                                    format_signature(&member.params, member.returns.as_deref())
                                ),
                            },
                            declaration: true,
                        });
                    }
                }
            }
            SyntaxStatement::OverloadDef(statement) | SyntaxStatement::FunctionDef(statement) => {
                let name = &statement.name;
                let line = statement.line;
                let params = &statement.params;
                let returns = statement.returns.as_deref();

                let canonical = format!("{module_key}.{}", name);
                local_symbols.insert(name.clone(), canonical.clone());
                if let Some(range) = find_name_range(&document.text, line, name) {
                    declarations.push(SymbolOccurrence {
                        canonical,
                        name: name.clone(),
                        uri: document.uri.clone(),
                        range,
                        legacy_detail: format!(
                            "function {}{}",
                            name,
                            format_signature(params, returns)
                        ),
                        declaration: true,
                    });
                }
            }
            SyntaxStatement::Import(statement) => {
                for binding in &statement.bindings {
                    local_symbols.insert(binding.local_name.clone(), binding.source_path.clone());
                }
            }
            SyntaxStatement::Value(statement) => {
                for name in &statement.names {
                    let canonical = format!("{module_key}.{name}");
                    local_symbols.insert(name.clone(), canonical.clone());
                    if let Some(range) = find_name_range(&document.text, statement.line, name) {
                        declarations.push(SymbolOccurrence {
                            canonical,
                            name: name.clone(),
                            uri: document.uri.clone(),
                            range,
                            legacy_detail: format!(
                                "value {}: {}",
                                name,
                                statement.annotation.clone().unwrap_or_default()
                            ),
                            declaration: true,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    (local_symbols, declarations)
}

pub(crate) fn collect_reference_occurrences(
    workspace: &WorkspaceState,
    document: &DocumentState,
    member_symbols: &BTreeMap<String, Vec<String>>,
    declarations_by_canonical: &BTreeMap<String, SymbolOccurrence>,
) -> Vec<SymbolOccurrence> {
    tokenize_identifiers(&document.text)
        .into_iter()
        .filter_map(|token| {
            let local = document.local_symbols.get(&token.name).cloned();
            let member = if token.preceded_by_dot {
                resolve_member_symbol(
                    workspace,
                    document,
                    member_symbols,
                    declarations_by_canonical,
                    &token,
                )
            } else {
                None
            };
            let canonical = local.or(member)?;
            Some(SymbolOccurrence {
                canonical: canonical.clone(),
                name: token.name,
                uri: document.uri.clone(),
                range: token.range,
                legacy_detail: canonical,
                declaration: false,
            })
        })
        .collect()
}

pub(crate) fn resolve_member_symbol(
    workspace: &WorkspaceState,
    document: &DocumentState,
    member_symbols: &BTreeMap<String, Vec<String>>,
    declarations_by_canonical: &BTreeMap<String, SymbolOccurrence>,
    token: &TokenOccurrence,
) -> Option<String> {
    let candidates = member_symbols.get(&token.name)?;
    if candidates.len() == 1 {
        return candidates.first().cloned();
    }

    let owner_canonical =
        resolve_member_owner_canonical(workspace, document, declarations_by_canonical, token)
            .or_else(|| {
                let receiver = member_receiver_name(&document.text, token.range.start)?;
                document.local_value_types.get(&receiver).cloned()
            })?;
    let expected = format!("{}.{}", owner_canonical, token.name);
    candidates.iter().find(|candidate| *candidate == &expected).cloned()
}

pub(crate) fn resolve_member_owner_canonical(
    workspace: &WorkspaceState,
    document: &DocumentState,
    declarations_by_canonical: &BTreeMap<String, SymbolOccurrence>,
    token: &TokenOccurrence,
) -> Option<String> {
    let line = token.range.start.line as usize + 1;
    for statement in &document.syntax.statements {
        match statement {
            SyntaxStatement::MethodCall(method_call)
                if method_call.line == line && method_call.method == token.name =>
            {
                return resolve_owner_canonical(
                    workspace,
                    document,
                    declarations_by_canonical,
                    &method_call.owner_name,
                    method_call.through_instance,
                );
            }
            SyntaxStatement::MemberAccess(member_access)
                if member_access.line == line && member_access.member == token.name =>
            {
                return resolve_owner_canonical(
                    workspace,
                    document,
                    declarations_by_canonical,
                    &member_access.owner_name,
                    member_access.through_instance,
                );
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn resolve_completion_member_owner_types(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
) -> Vec<String> {
    let line = position.line as usize + 1;
    let owner_type = document
        .syntax
        .statements
        .iter()
        .find_map(|statement| match statement {
            SyntaxStatement::MethodCall(method_call) if method_call.line == line => {
                resolve_completion_owner_type_text(
                    workspace,
                    document,
                    position,
                    &method_call.owner_name,
                    method_call.through_instance,
                )
            }
            SyntaxStatement::MemberAccess(member_access) if member_access.line == line => {
                resolve_completion_owner_type_text(
                    workspace,
                    document,
                    position,
                    &member_access.owner_name,
                    member_access.through_instance,
                )
            }
            _ => None,
        })
        .or_else(|| {
            let receiver = member_receiver_name(&document.text, position)?;
            resolve_visible_name_type_text(workspace, document, position, &receiver, 0)
        });

    owner_type
        .map(|owner_type| resolve_type_canonicals(workspace, document, &owner_type))
        .unwrap_or_default()
}

pub(crate) fn resolve_completion_owner_type_text(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
    owner_name: &str,
    through_instance: bool,
) -> Option<String> {
    if through_instance {
        return resolve_callable_return_type_text(workspace, document, position, owner_name);
    }

    resolve_visible_name_type_text(workspace, document, position, owner_name, 0)
        .or_else(|| document.local_symbols.get(owner_name).cloned())
}

pub(crate) fn collect_member_completion_items(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
) -> Vec<LspCompletionItem> {
    let owner_types = resolve_completion_member_owner_types(workspace, document, position);
    if owner_types.is_empty() {
        let mut seen = BTreeSet::new();
        return workspace
            .declarations_by_canonical
            .values()
            .filter(|occurrence| occurrence.canonical.matches('.').count() >= 2)
            .filter(|occurrence| seen.insert(occurrence.name.clone()))
            .map(completion_item_from_occurrence)
            .collect();
    }

    let mut owner_members = owner_types
        .iter()
        .map(|owner| collect_visible_member_details(workspace, owner))
        .filter(|members| !members.is_empty())
        .collect::<Vec<_>>();
    if owner_members.is_empty() {
        return Vec::new();
    }

    let mut visible = owner_members.remove(0);
    for members in owner_members {
        visible.retain(|label, _| members.contains_key(label));
    }

    visible.into_iter().map(|(label, detail)| completion_item_from_detail(label, detail)).collect()
}

pub(crate) fn completion_item_from_occurrence(occurrence: &SymbolOccurrence) -> LspCompletionItem {
    completion_item(
        occurrence.name.clone(),
        Some(occurrence.legacy_detail.clone()),
        completion_item_kind_from_detail(&occurrence.legacy_detail),
    )
}

pub(crate) fn completion_item_from_canonical(
    workspace: &WorkspaceState,
    label: String,
    canonical: &str,
) -> LspCompletionItem {
    let detail = workspace
        .declarations_by_canonical
        .get(canonical)
        .map(|occurrence| occurrence.legacy_detail.clone())
        .unwrap_or_else(|| canonical.to_owned());
    let kind = binding_declaration_for_canonical(workspace, canonical)
        .map(|(_, declaration)| completion_item_kind_for_declaration(declaration))
        .unwrap_or_else(|| completion_item_kind_from_detail(&detail));
    completion_item(label, Some(detail), kind)
}

pub(crate) fn completion_item_from_detail(
    label: String,
    legacy_detail: String,
) -> LspCompletionItem {
    completion_item(
        label,
        Some(legacy_detail.clone()),
        completion_item_kind_from_detail(&legacy_detail),
    )
}

fn completion_item(label: String, legacy_detail: Option<String>, kind: u32) -> LspCompletionItem {
    let normalized = label.to_lowercase();
    LspCompletionItem {
        filter_text: label.clone(),
        sort_text: format!("{normalized}:{kind:02}"),
        label,
        detail: legacy_detail,
        kind,
        insert_text: None,
        insert_text_format: None,
    }
}

pub(crate) fn keyword_snippet_completion_items() -> Vec<LspCompletionItem> {
    [
        ("class", "snippet class Name", "class ${1:Name}:\n    ${0:pass}"),
        ("def", "snippet def name(...)", "def ${1:name}(${2:args}) -> ${3:None}:\n    ${0:pass}"),
        ("typealias", "snippet typealias Name = ...", "typealias ${1:Name} = ${0:object}"),
        (
            "overload def",
            "snippet overload def name(...)",
            "overload def ${1:name}(${2:args}) -> ${0:object}: ...",
        ),
        ("unsafe", "snippet unsafe block", "unsafe:\n    ${0:pass}"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (label, detail, insert_text))| LspCompletionItem {
        label: label.to_owned(),
        detail: Some(detail.to_owned()),
        kind: 15,
        filter_text: label.to_owned(),
        sort_text: format!("0:{index:02}:{}", label.to_lowercase()),
        insert_text: Some(insert_text.to_owned()),
        insert_text_format: Some(2),
    })
    .collect()
}

fn completion_item_kind_for_declaration(declaration: &typepython_binding::Declaration) -> u32 {
    match declaration.kind {
        typepython_binding::DeclarationKind::TypeAlias => 25,
        typepython_binding::DeclarationKind::Class => match declaration.class_kind {
            Some(typepython_binding::DeclarationOwnerKind::Interface) => 8,
            _ => 7,
        },
        typepython_binding::DeclarationKind::Function
        | typepython_binding::DeclarationKind::Overload => {
            if declaration.owner.is_some() {
                match declaration.method_kind {
                    Some(typepython_syntax::MethodKind::Property)
                    | Some(typepython_syntax::MethodKind::PropertySetter) => 10,
                    _ => 2,
                }
            } else {
                3
            }
        }
        typepython_binding::DeclarationKind::Value => {
            if declaration.owner.is_some() {
                5
            } else {
                6
            }
        }
        typepython_binding::DeclarationKind::Import => 9,
    }
}

fn completion_item_kind_from_detail(detail: &str) -> u32 {
    let normalized = detail.trim_start();
    if normalized.starts_with("method ") {
        2
    } else if normalized.starts_with("field ") {
        5
    } else if normalized.starts_with("function ") {
        3
    } else if normalized.starts_with("class ") {
        7
    } else if normalized.starts_with("typealias ") {
        25
    } else if normalized.starts_with("value ") {
        6
    } else {
        9
    }
}

pub(crate) fn collect_visible_member_details(
    workspace: &WorkspaceState,
    owner_canonical: &str,
) -> BTreeMap<String, String> {
    let mut members = BTreeMap::new();
    let mut visited = BTreeSet::new();
    collect_visible_member_details_recursive(
        workspace,
        owner_canonical,
        &mut visited,
        &mut members,
    );
    members
}

pub(crate) fn collect_visible_member_details_recursive(
    workspace: &WorkspaceState,
    owner_canonical: &str,
    visited: &mut BTreeSet<String>,
    members: &mut BTreeMap<String, String>,
) {
    if !visited.insert(owner_canonical.to_owned()) {
        return;
    }

    let Some((node, declaration)) = resolve_top_level_declaration(workspace, owner_canonical)
    else {
        return;
    };

    for member in node.declarations.iter().filter(|candidate| {
        candidate.owner.as_ref().is_some_and(|owner| owner.name == declaration.name)
    }) {
        members.entry(member.name.clone()).or_insert_with(|| {
            let member_canonical = format!("{owner_canonical}.{}", member.name);
            workspace
                .declarations_by_canonical
                .get(&member_canonical)
                .map(|occurrence| occurrence.legacy_detail.clone())
                .unwrap_or_else(|| render_member_detail(member))
        });
    }

    let Some(owner_document) = document_for_module_key(workspace, &node.module_key) else {
        return;
    };
    for base in declaration.rendered_class_bases() {
        for base_canonical in resolve_type_canonicals(workspace, owner_document, &base) {
            collect_visible_member_details_recursive(workspace, &base_canonical, visited, members);
        }
    }
}

pub(crate) fn resolve_top_level_declaration<'a>(
    workspace: &'a WorkspaceState,
    canonical: &str,
) -> Option<(&'a ModuleNode, &'a typepython_binding::Declaration)> {
    let (module_key, name) = canonical.rsplit_once('.')?;
    let node = workspace.queries.nodes_by_module_key.get(module_key)?;
    let declaration = node
        .declarations
        .iter()
        .find(|declaration| declaration.owner.is_none() && declaration.name == name)?;
    Some((node, declaration))
}

pub(crate) fn document_for_module_key<'a>(
    workspace: &'a WorkspaceState,
    module_key: &str,
) -> Option<&'a DocumentState> {
    workspace.queries.documents_by_module_key.get(module_key)
}

pub(crate) fn render_member_detail(member: &typepython_binding::Declaration) -> String {
    match member.kind {
        typepython_binding::DeclarationKind::Value => {
            let rendered = member
                .value_annotation_text()
                .or_else(|| member.rendered_value_type())
                .unwrap_or_else(|| member.rendered_detail());
            let annotation = rendered.as_str();
            format!("field {}: {}", member.name, annotation)
        }
        typepython_binding::DeclarationKind::Function
        | typepython_binding::DeclarationKind::Overload => {
            format!("method {}{}", member.name, member.rendered_detail())
        }
        _ => member.rendered_detail(),
    }
}

pub(crate) fn render_declaration_detail(declaration: &typepython_binding::Declaration) -> String {
    match declaration.kind {
        typepython_binding::DeclarationKind::TypeAlias => {
            let rendered =
                declaration.type_alias_body_text().unwrap_or_else(|| declaration.rendered_detail());
            format!("typealias {} = {}", declaration.name, rendered)
        }
        typepython_binding::DeclarationKind::Class => {
            format!("class {}({})", declaration.name, declaration.rendered_class_bases().join(", "))
        }
        typepython_binding::DeclarationKind::Function
        | typepython_binding::DeclarationKind::Overload => {
            format!("function {}{}", declaration.name, declaration.rendered_detail())
        }
        typepython_binding::DeclarationKind::Value => {
            let rendered = declaration
                .value_annotation_text()
                .or_else(|| declaration.rendered_value_type())
                .unwrap_or_else(|| declaration.rendered_detail());
            format!("value {}: {}", declaration.name, rendered)
        }
        typepython_binding::DeclarationKind::Import => {
            declaration.import_raw_target_text().unwrap_or_else(|| declaration.rendered_detail())
        }
    }
}

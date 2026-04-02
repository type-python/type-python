use super::*;

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
                        detail: format!("typealias {} = {}", statement.name, statement.value),
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
                        detail: format!("class {}({})", statement.name, statement.bases.join(", ")),
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
                            detail: match member.kind {
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
                        detail: format!("function {}{}", name, format_signature(params, returns)),
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
                            detail: format!(
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
    document: &DocumentState,
    member_symbols: &BTreeMap<String, Vec<String>>,
    declarations_by_canonical: &BTreeMap<String, SymbolOccurrence>,
) -> Vec<SymbolOccurrence> {
    tokenize_identifiers(&document.text)
        .into_iter()
        .filter_map(|token| {
            let local = document.local_symbols.get(&token.name).cloned();
            let member = if token.preceded_by_dot {
                resolve_member_symbol(document, member_symbols, declarations_by_canonical, &token)
            } else {
                None
            };
            let canonical = local.or(member)?;
            Some(SymbolOccurrence {
                canonical: canonical.clone(),
                name: token.name,
                uri: document.uri.clone(),
                range: token.range,
                detail: canonical,
                declaration: false,
            })
        })
        .collect()
}

pub(crate) fn resolve_member_symbol(
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
        resolve_member_owner_canonical(document, declarations_by_canonical, token).or_else(
            || {
                let receiver = member_receiver_name(&document.text, token.range.start)?;
                document.local_value_types.get(&receiver).cloned()
            },
        )?;
    let expected = format!("{}.{}", owner_canonical, token.name);
    candidates.iter().find(|candidate| *candidate == &expected).cloned()
}

pub(crate) fn resolve_member_owner_canonical(
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
) -> Vec<Value> {
    let owner_types = resolve_completion_member_owner_types(workspace, document, position);
    if owner_types.is_empty() {
        let mut seen = BTreeSet::new();
        return workspace
            .declarations_by_canonical
            .values()
            .filter(|occurrence| occurrence.canonical.matches('.').count() >= 2)
            .filter(|occurrence| seen.insert(occurrence.name.clone()))
            .map(|occurrence| json!({"label": occurrence.name, "detail": occurrence.detail}))
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

    visible.into_iter().map(|(label, detail)| json!({"label": label, "detail": detail})).collect()
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
                .map(|occurrence| occurrence.detail.clone())
                .unwrap_or_else(|| render_member_detail(member))
        });
    }

    let Some(owner_document) = document_for_module_key(workspace, &node.module_key) else {
        return;
    };
    for base in &declaration.bases {
        for base_canonical in resolve_type_canonicals(workspace, owner_document, base) {
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
            let annotation = member.value_type.as_deref().unwrap_or(member.detail.as_str());
            format!("field {}: {}", member.name, annotation)
        }
        typepython_binding::DeclarationKind::Function
        | typepython_binding::DeclarationKind::Overload => {
            format!("method {}{}", member.name, member.detail)
        }
        _ => member.detail.clone(),
    }
}

pub(crate) fn resolve_visible_name_type_text(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
    name: &str,
    depth: usize,
) -> Option<String> {
    if depth > 8 {
        return None;
    }

    let line = position.line as usize + 1;
    let (owner_name, owner_type_name) = scope_context_at_position(document, position);
    if name == "self" {
        return owner_type_name;
    }
    if name == "cls" {
        return owner_type_name.map(|owner_type_name| format!("type[{owner_type_name}]"));
    }

    let base_type = resolve_parameter_annotation(
        document,
        owner_name.as_deref(),
        owner_type_name.as_deref(),
        name,
    )
    .or_else(|| {
        resolve_latest_assignment_type_text(
            workspace,
            document,
            line,
            owner_name.as_deref(),
            owner_type_name.as_deref(),
            name,
            depth,
        )
    })
    .or_else(|| {
        document
            .local_value_types
            .get(name)
            .and_then(|value_type| value_type.contains('.').then_some(value_type.clone()))
    })?;

    Some(apply_guard_narrowing(
        workspace,
        document,
        owner_name.as_deref(),
        owner_type_name.as_deref(),
        line,
        name,
        &base_type,
    ))
}

pub(crate) fn resolve_parameter_annotation(
    document: &DocumentState,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    name: &str,
) -> Option<String> {
    let params = if let Some(owner_type_name) = owner_type_name {
        document.syntax.statements.iter().find_map(|statement| match statement {
            SyntaxStatement::Interface(class_like)
            | SyntaxStatement::DataClass(class_like)
            | SyntaxStatement::SealedClass(class_like)
            | SyntaxStatement::ClassDef(class_like)
                if class_like.name == owner_type_name =>
            {
                class_like.members.iter().find_map(|member| {
                    (Some(member.name.as_str()) == owner_name).then_some(member.params.as_slice())
                })
            }
            _ => None,
        })
    } else {
        document.syntax.statements.iter().find_map(|statement| match statement {
            SyntaxStatement::FunctionDef(function) | SyntaxStatement::OverloadDef(function)
                if Some(function.name.as_str()) == owner_name =>
            {
                Some(function.params.as_slice())
            }
            _ => None,
        })
    }?;

    params.iter().find(|param| param.name == name).and_then(|param| param.annotation.clone())
}

pub(crate) fn resolve_latest_assignment_type_text(
    workspace: &WorkspaceState,
    document: &DocumentState,
    current_line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    name: &str,
    depth: usize,
) -> Option<String> {
    document.syntax.statements.iter().rev().find_map(|statement| {
        let SyntaxStatement::Value(value) = statement else {
            return None;
        };
        if value.line >= current_line
            || value.owner_name.as_deref() != owner_name
            || value.owner_type_name.as_deref() != owner_type_name
            || !value.names.iter().any(|candidate| candidate == name)
        {
            return None;
        }
        resolve_value_statement_type_text(workspace, document, value, current_line, depth)
    })
}

pub(crate) fn resolve_value_statement_type_text(
    workspace: &WorkspaceState,
    document: &DocumentState,
    value: &typepython_syntax::ValueStatement,
    current_line: usize,
    depth: usize,
) -> Option<String> {
    value
        .annotation
        .clone()
        .filter(|annotation| !annotation.trim().is_empty())
        .or_else(|| value.value_type.clone().filter(|value_type| !value_type.trim().is_empty()))
        .or_else(|| {
            value.value_callee.as_deref().and_then(|callee| {
                resolve_callable_return_type_text(
                    workspace,
                    document,
                    lsp_position(value.line),
                    callee,
                )
            })
        })
        .or_else(|| {
            value.value_name.as_deref().and_then(|value_name| {
                resolve_visible_name_type_text(
                    workspace,
                    document,
                    lsp_position(current_line),
                    value_name,
                    depth + 1,
                )
            })
        })
}

pub(crate) fn resolve_callable_return_type_text(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
    callee: &str,
) -> Option<String> {
    let (owner_name, owner_type_name) = scope_context_at_position(document, position);
    resolve_callable_return_type_in_scope(
        workspace,
        document,
        owner_name.as_deref(),
        owner_type_name.as_deref(),
        callee,
    )
}

pub(crate) fn resolve_callable_return_type_in_scope(
    workspace: &WorkspaceState,
    document: &DocumentState,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    callee: &str,
) -> Option<String> {
    if let Some(canonical) = document.local_symbols.get(callee) {
        if let Some((_, declaration)) = resolve_top_level_declaration(workspace, canonical) {
            if declaration.kind == typepython_binding::DeclarationKind::Class {
                return Some(callee.to_owned());
            }
            return parse_return_annotation(&declaration.detail);
        }
    }

    resolve_parameter_annotation(document, owner_name, owner_type_name, callee)
        .and_then(|annotation| parse_return_annotation(&annotation))
}

pub(crate) fn parse_return_annotation(detail: &str) -> Option<String> {
    detail
        .split_once("->")
        .map(|(_, returns)| returns.trim().to_owned())
        .filter(|returns| !returns.is_empty())
}

pub(crate) fn resolve_type_canonicals(
    workspace: &WorkspaceState,
    document: &DocumentState,
    type_text: &str,
) -> Vec<String> {
    let mut resolved = Vec::new();
    for branch in union_branches(type_text) {
        let normalized = strip_type_wrappers(&branch);
        if normalized.is_empty() || normalized == "None" {
            continue;
        }
        let head = strip_generic_args(&normalized);
        if workspace.declarations_by_canonical.contains_key(head) {
            push_unique(&mut resolved, head.to_owned());
            continue;
        }
        if let Some(canonical) = document.local_symbols.get(head) {
            push_unique(&mut resolved, canonical.clone());
            continue;
        }
        if let Some((module_key, name)) = head.rsplit_once('.') {
            if workspace.queries.nodes_by_module_key.get(module_key).is_some_and(|node| {
                node.declarations
                    .iter()
                    .any(|declaration| declaration.owner.is_none() && declaration.name == name)
            }) {
                push_unique(&mut resolved, head.to_owned());
            }
        }
    }
    resolved
}

pub(crate) fn apply_guard_narrowing(
    workspace: &WorkspaceState,
    document: &DocumentState,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
    base_type: &str,
) -> String {
    let Some(node) =
        workspace.queries.nodes_by_module_key.get(&document.syntax.source.logical_module)
    else {
        return base_type.to_owned();
    };
    let mut narrowed = base_type.to_owned();

    let mut if_guards = node
        .if_guards
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == owner_name
                && guard.owner_type_name.as_deref() == owner_type_name
                && guard.line < current_line
                && !name_reassigned_after_line(
                    node,
                    owner_name,
                    owner_type_name,
                    value_name,
                    guard.line,
                    current_line,
                )
        })
        .filter_map(|guard| {
            let branch_true = if current_line >= guard.true_start_line
                && current_line <= guard.true_end_line
            {
                Some(true)
            } else if let (Some(start), Some(end)) = (guard.false_start_line, guard.false_end_line)
            {
                (current_line >= start && current_line <= end).then_some(false)
            } else {
                None
            }?;
            Some((guard.line, branch_true, guard.guard.as_ref()?))
        })
        .collect::<Vec<_>>();
    if_guards.sort_by_key(|(line, _, _)| *line);
    for (_, branch_true, guard) in if_guards {
        narrowed =
            apply_guard_condition(workspace, document, &narrowed, value_name, guard, branch_true);
    }

    let mut asserts = node
        .asserts
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == owner_name
                && guard.owner_type_name.as_deref() == owner_type_name
                && guard.line < current_line
                && !name_reassigned_after_line(
                    node,
                    owner_name,
                    owner_type_name,
                    value_name,
                    guard.line,
                    current_line,
                )
        })
        .filter_map(|guard| Some((guard.line, guard.guard.as_ref()?)))
        .collect::<Vec<_>>();
    asserts.sort_by_key(|(line, _)| *line);
    for (_, guard) in asserts {
        narrowed = apply_guard_condition(workspace, document, &narrowed, value_name, guard, true);
    }

    narrowed
}

pub(crate) fn name_reassigned_after_line(
    node: &ModuleNode,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    value_name: &str,
    after_line: usize,
    current_line: usize,
) -> bool {
    node.assignments.iter().any(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.as_deref() == owner_name
            && assignment.owner_type_name.as_deref() == owner_type_name
            && after_line < assignment.line
            && assignment.line < current_line
    }) || node.invalidations.iter().any(|site| {
        site.names.iter().any(|name| name == value_name)
            && site.owner_name.as_deref() == owner_name
            && site.owner_type_name.as_deref() == owner_type_name
            && after_line < site.line
            && site.line < current_line
    })
}

pub(crate) fn apply_guard_condition(
    workspace: &WorkspaceState,
    document: &DocumentState,
    base_type: &str,
    value_name: &str,
    guard: &typepython_binding::GuardConditionSite,
    branch_true: bool,
) -> String {
    match guard {
        typepython_binding::GuardConditionSite::IsNone { name, negated } if name == value_name => {
            match (branch_true, negated) {
                (true, false) | (false, true) => String::from("None"),
                (false, false) | (true, true) => {
                    remove_none_branch(base_type).unwrap_or_else(|| base_type.to_owned())
                }
            }
        }
        typepython_binding::GuardConditionSite::IsInstance { name, types }
            if name == value_name =>
        {
            if branch_true {
                narrow_to_instance_types(base_type, types)
            } else {
                remove_instance_types(base_type, types)
            }
        }
        typepython_binding::GuardConditionSite::PredicateCall { name, callee }
            if name == value_name =>
        {
            apply_predicate_guard(workspace, document, base_type, callee, branch_true)
        }
        typepython_binding::GuardConditionSite::TruthyName { name } if name == value_name => {
            apply_truthy_narrowing(base_type, branch_true)
        }
        typepython_binding::GuardConditionSite::Not(inner) => {
            apply_guard_condition(workspace, document, base_type, value_name, inner, !branch_true)
        }
        typepython_binding::GuardConditionSite::And(parts) => {
            if branch_true {
                parts.iter().fold(base_type.to_owned(), |current, part| {
                    apply_guard_condition(workspace, document, &current, value_name, part, true)
                })
            } else {
                join_type_candidates(
                    parts
                        .iter()
                        .scan(base_type.to_owned(), |current_true, part| {
                            let narrowed_false = apply_guard_condition(
                                workspace,
                                document,
                                current_true,
                                value_name,
                                part,
                                false,
                            );
                            *current_true = apply_guard_condition(
                                workspace,
                                document,
                                current_true,
                                value_name,
                                part,
                                true,
                            );
                            Some(narrowed_false)
                        })
                        .collect(),
                )
            }
        }
        typepython_binding::GuardConditionSite::Or(parts) => {
            if branch_true {
                join_type_candidates(
                    parts
                        .iter()
                        .scan(base_type.to_owned(), |current_false, part| {
                            let narrowed_true = apply_guard_condition(
                                workspace,
                                document,
                                current_false,
                                value_name,
                                part,
                                true,
                            );
                            *current_false = apply_guard_condition(
                                workspace,
                                document,
                                current_false,
                                value_name,
                                part,
                                false,
                            );
                            Some(narrowed_true)
                        })
                        .collect(),
                )
            } else {
                parts.iter().fold(base_type.to_owned(), |current, part| {
                    apply_guard_condition(workspace, document, &current, value_name, part, false)
                })
            }
        }
        _ => base_type.to_owned(),
    }
}

pub(crate) fn apply_predicate_guard(
    workspace: &WorkspaceState,
    document: &DocumentState,
    base_type: &str,
    callee: &str,
    branch_true: bool,
) -> String {
    let Some((kind, guarded_type)) = parse_guard_return_kind(workspace, document, callee) else {
        return base_type.to_owned();
    };
    match (kind.as_str(), branch_true) {
        ("TypeGuard", true) | ("TypeIs", true) => {
            narrow_to_instance_types(base_type, &[guarded_type])
        }
        ("TypeIs", false) => remove_instance_types(base_type, &[guarded_type]),
        _ => base_type.to_owned(),
    }
}

pub(crate) fn parse_guard_return_kind(
    workspace: &WorkspaceState,
    document: &DocumentState,
    callee: &str,
) -> Option<(String, String)> {
    let returns = resolve_callable_return_type_in_scope(workspace, document, None, None, callee)?;
    if let Some(inner) =
        returns.strip_prefix("TypeGuard[").and_then(|inner| inner.strip_suffix(']'))
    {
        return Some((String::from("TypeGuard"), inner.trim().to_owned()));
    }
    if let Some(inner) = returns.strip_prefix("TypeIs[").and_then(|inner| inner.strip_suffix(']')) {
        return Some((String::from("TypeIs"), inner.trim().to_owned()));
    }
    None
}

pub(crate) fn narrow_to_instance_types(base_type: &str, types: &[String]) -> String {
    let kept = union_branches(base_type)
        .into_iter()
        .filter(|branch| types.iter().any(|expected| type_branch_matches(branch, expected)))
        .collect::<Vec<_>>();
    if kept.is_empty() { join_type_candidates(types.to_vec()) } else { join_type_candidates(kept) }
}

pub(crate) fn remove_instance_types(base_type: &str, types: &[String]) -> String {
    let kept = union_branches(base_type)
        .into_iter()
        .filter(|branch| !types.iter().any(|expected| type_branch_matches(branch, expected)))
        .collect::<Vec<_>>();
    if kept.is_empty() { base_type.to_owned() } else { join_type_candidates(kept) }
}

pub(crate) fn remove_none_branch(base_type: &str) -> Option<String> {
    let kept =
        union_branches(base_type).into_iter().filter(|branch| branch != "None").collect::<Vec<_>>();
    (!kept.is_empty()).then(|| join_type_candidates(kept))
}

pub(crate) fn apply_truthy_narrowing(base_type: &str, branch_true: bool) -> String {
    let branches = union_branches(base_type);
    let non_none =
        branches.iter().filter(|branch| branch.as_str() != "None").cloned().collect::<Vec<_>>();
    if branches.iter().any(|branch| branch == "None") {
        return if branch_true { join_type_candidates(non_none) } else { String::from("None") };
    }
    base_type.to_owned()
}

pub(crate) fn join_type_candidates(candidates: Vec<String>) -> String {
    let mut unique = Vec::new();
    for candidate in candidates {
        for branch in union_branches(&candidate) {
            push_unique(&mut unique, branch);
        }
    }
    unique.join(" | ")
}

pub(crate) fn union_branches(type_text: &str) -> Vec<String> {
    let trimmed = type_text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Some(inner) = trimmed.strip_prefix("Union[").and_then(|inner| inner.strip_suffix(']')) {
        return split_top_level(inner, ',');
    }
    if trimmed.contains('|') {
        let branches = split_top_level(trimmed, '|');
        if branches.len() > 1 {
            return branches;
        }
    }
    vec![trimmed.to_owned()]
}

pub(crate) fn strip_type_wrappers(type_text: &str) -> String {
    let mut current = type_text.trim().to_owned();
    loop {
        let next = [
            "Annotated[",
            "ClassVar[",
            "Final[",
            "Required[",
            "NotRequired[",
            "ReadOnly[",
            "type[",
        ]
        .into_iter()
        .find_map(|prefix| unwrap_first_type_argument(&current, prefix));
        let Some(next) = next else {
            return current;
        };
        current = next;
    }
}

pub(crate) fn unwrap_first_type_argument(type_text: &str, prefix: &str) -> Option<String> {
    let inner = type_text.strip_prefix(prefix)?.strip_suffix(']')?;
    split_top_level(inner, ',').into_iter().next()
}

pub(crate) fn strip_generic_args(type_text: &str) -> &str {
    type_text.split_once('[').map_or(type_text, |(head, _)| head.trim())
}

pub(crate) fn split_top_level(text: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in text.chars() {
        match ch {
            '[' => {
                depth += 1;
                current.push(ch);
            }
            ']' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            _ if ch == separator && depth == 0 => {
                let part = current.trim();
                if !part.is_empty() {
                    parts.push(part.to_owned());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let part = current.trim();
    if !part.is_empty() {
        parts.push(part.to_owned());
    }
    parts
}

pub(crate) fn type_branch_matches(branch: &str, expected: &str) -> bool {
    strip_generic_args(&strip_type_wrappers(branch))
        == strip_generic_args(&strip_type_wrappers(expected))
}

pub(crate) fn push_unique(items: &mut Vec<String>, item: String) {
    if !items.contains(&item) {
        items.push(item);
    }
}

pub(crate) fn scope_context_at_position(
    document: &DocumentState,
    position: LspPosition,
) -> (Option<String>, Option<String>) {
    let line = position.line as usize + 1;
    let mut best = None;
    for statement in &document.syntax.statements {
        if let Some(candidate) = statement_scope_context(statement, &document.text, line) {
            if best.as_ref().is_none_or(|(best_line, _, _)| candidate.0 >= *best_line) {
                best = Some(candidate);
            }
        }
    }
    best.map(|(_, owner_name, owner_type_name)| (owner_name, owner_type_name)).unwrap_or_default()
}

pub(crate) fn statement_scope_context(
    statement: &SyntaxStatement,
    text: &str,
    current_line: usize,
) -> Option<(usize, Option<String>, Option<String>)> {
    match statement {
        SyntaxStatement::Value(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::Return(value) if value.line <= current_line => {
            Some((value.line, Some(value.owner_name.clone()), value.owner_type_name.clone()))
        }
        SyntaxStatement::Yield(value) if value.line <= current_line => {
            Some((value.line, Some(value.owner_name.clone()), value.owner_type_name.clone()))
        }
        SyntaxStatement::If(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::Assert(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::Invalidate(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::Match(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::For(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::With(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::ExceptHandler(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::FunctionDef(function) | SyntaxStatement::OverloadDef(function)
            if function.line < current_line
                && line_indentation(document_line_text(text, current_line))
                    > line_indentation(document_line_text(text, function.line)) =>
        {
            Some((function.line, Some(function.name.clone()), None))
        }
        SyntaxStatement::Interface(class_like)
        | SyntaxStatement::DataClass(class_like)
        | SyntaxStatement::SealedClass(class_like)
        | SyntaxStatement::ClassDef(class_like) => {
            class_member_scope_context(class_like, text, current_line)
        }
        _ => None,
    }
}

pub(crate) fn class_member_scope_context(
    class_like: &NamedBlockStatement,
    text: &str,
    current_line: usize,
) -> Option<(usize, Option<String>, Option<String>)> {
    class_like.members.iter().rev().find_map(|member| {
        (member.line < current_line
            && line_indentation(document_line_text(text, current_line))
                > line_indentation(document_line_text(text, member.line)))
        .then(|| (member.line, Some(member.name.clone()), Some(class_like.name.clone())))
    })
}

pub(crate) fn document_line_text(text: &str, line: usize) -> &str {
    text.lines().nth(line.saturating_sub(1)).unwrap_or("")
}

pub(crate) fn line_indentation(text: &str) -> usize {
    text.chars().take_while(|ch| ch.is_whitespace()).count()
}

pub(crate) fn lsp_position(line: usize) -> LspPosition {
    LspPosition { line: line.saturating_sub(1) as u32, character: 0 }
}

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

pub(crate) fn full_document_range(text: &str) -> LspRange {
    let mut last_line = 0u32;
    let mut last_character = 0u32;
    for (index, line) in text.lines().enumerate() {
        last_line = index as u32;
        last_character = line.chars().count() as u32;
    }
    if text.ends_with('\n') {
        last_line = text.lines().count() as u32;
        last_character = 0;
    }
    LspRange {
        start: LspPosition { line: 0, character: 0 },
        end: LspPosition { line: last_line, character: last_character },
    }
}

pub(crate) fn token_at_position(text: &str, position: LspPosition) -> Option<TokenOccurrence> {
    tokenize_identifiers(text).into_iter().find(|token| range_contains(token.range, position))
}

pub(crate) fn resolve_owner_canonical(
    document: &DocumentState,
    declarations_by_canonical: &BTreeMap<String, SymbolOccurrence>,
    owner_name: &str,
    through_instance: bool,
) -> Option<String> {
    if !through_instance {
        return document
            .local_value_types
            .get(owner_name)
            .cloned()
            .or_else(|| document.local_symbols.get(owner_name).cloned());
    }

    let callable_canonical = document.local_symbols.get(owner_name)?.clone();
    let callable = declarations_by_canonical.get(&callable_canonical)?;
    let return_type = callable.detail.split_once("->")?.1.trim();
    document.local_symbols.get(return_type).cloned().or_else(|| Some(return_type.to_owned()))
}

pub(crate) fn member_receiver_name(text: &str, position: LspPosition) -> Option<String> {
    let line = text.lines().nth(position.line as usize)?;
    let prefix = line.chars().take(position.character as usize).collect::<String>();
    let mut chars = prefix.chars().collect::<Vec<_>>();
    while chars.last().is_some_and(|ch| ch.is_whitespace()) {
        chars.pop();
    }
    if chars.pop()? != '.' {
        return None;
    }
    while chars.last().is_some_and(|ch| ch.is_whitespace()) {
        chars.pop();
    }
    let end = chars.len();
    let mut start = end;
    while start > 0 {
        let ch = chars[start - 1];
        if ch.is_ascii_alphanumeric() || ch == '_' {
            start -= 1;
        } else {
            break;
        }
    }
    (start < end).then(|| chars[start..end].iter().collect())
}

pub(crate) fn collect_local_value_types(
    document: &DocumentState,
    local_symbols: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut types = BTreeMap::new();
    for statement in &document.syntax.statements {
        let SyntaxStatement::Value(statement) = statement else {
            continue;
        };
        let resolved_type = statement
            .annotation
            .as_ref()
            .and_then(|annotation| local_symbols.get(annotation))
            .cloned()
            .or_else(|| {
                statement
                    .value_callee
                    .as_ref()
                    .and_then(|callee| local_symbols.get(callee))
                    .cloned()
            })
            .or_else(|| {
                statement
                    .value_type
                    .as_ref()
                    .and_then(|value_type| local_symbols.get(value_type))
                    .cloned()
            });
        let Some(resolved_type) = resolved_type else {
            continue;
        };
        for name in &statement.names {
            types.insert(name.clone(), resolved_type.clone());
        }
    }
    types
}

pub(crate) fn dedupe_occurrences(occurrences: &mut Vec<SymbolOccurrence>) {
    let mut seen = BTreeSet::new();
    occurrences.retain(|occurrence| {
        seen.insert((
            occurrence.canonical.clone(),
            occurrence.uri.clone(),
            occurrence.range.start.line,
            occurrence.range.start.character,
            occurrence.range.end.line,
            occurrence.range.end.character,
            occurrence.declaration,
        ))
    });
}

#[derive(Debug)]
pub(crate) struct TokenOccurrence {
    name: String,
    range: LspRange,
    preceded_by_dot: bool,
}

pub(crate) fn tokenize_identifiers(text: &str) -> Vec<TokenOccurrence> {
    let mut tokens = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let chars = line.chars().collect::<Vec<_>>();
        let mut index = 0usize;
        while index < chars.len() {
            if chars[index].is_ascii_alphabetic() || chars[index] == '_' {
                let start = index;
                index += 1;
                while index < chars.len()
                    && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
                {
                    index += 1;
                }
                let name = chars[start..index].iter().collect::<String>();
                let preceded_by_dot = chars[..start]
                    .iter()
                    .rev()
                    .find(|ch| !ch.is_whitespace())
                    .is_some_and(|ch| *ch == '.');
                tokens.push(TokenOccurrence {
                    name,
                    range: LspRange {
                        start: LspPosition { line: line_index as u32, character: start as u32 },
                        end: LspPosition { line: line_index as u32, character: index as u32 },
                    },
                    preceded_by_dot,
                });
            } else {
                index += 1;
            }
        }
    }
    tokens
}

pub(crate) fn find_name_range(text: &str, line: usize, name: &str) -> Option<LspRange> {
    let line_text = text.lines().nth(line.saturating_sub(1))?;
    let column = line_text.find(name)?;
    Some(LspRange {
        start: LspPosition { line: line.saturating_sub(1) as u32, character: column as u32 },
        end: LspPosition {
            line: line.saturating_sub(1) as u32,
            character: (column + name.len()) as u32,
        },
    })
}

pub(crate) fn line_prefix(text: &str, position: LspPosition) -> String {
    text.lines()
        .nth(position.line as usize)
        .map(|line| line.chars().take(position.character as usize).collect())
        .unwrap_or_default()
}

pub(crate) fn format_signature(
    params: &[typepython_syntax::FunctionParam],
    returns: Option<&str>,
) -> String {
    format!(
        "({})->{}",
        params
            .iter()
            .map(|param| match &param.annotation {
                Some(annotation) => format!("{}:{}", param.name, annotation),
                None => param.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(","),
        returns.unwrap_or("")
    )
}

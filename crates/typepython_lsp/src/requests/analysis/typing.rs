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
            return declaration
                .callable_signature()
                .and_then(|signature| {
                    signature.returns.as_ref().map(typepython_binding::BoundTypeExpr::render)
                })
                .or_else(|| parse_return_annotation(&declaration.rendered_detail()));
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
    if kept.is_empty() {
        join_type_candidates(types.to_vec())
    } else {
        join_type_candidates(kept)
    }
}

pub(crate) fn remove_instance_types(base_type: &str, types: &[String]) -> String {
    let kept = union_branches(base_type)
        .into_iter()
        .filter(|branch| !types.iter().any(|expected| type_branch_matches(branch, expected)))
        .collect::<Vec<_>>();
    if kept.is_empty() {
        base_type.to_owned()
    } else {
        join_type_candidates(kept)
    }
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

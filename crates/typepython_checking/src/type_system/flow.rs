pub(super) fn apply_guard_narrowing_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
    base_type: &SemanticType,
) -> SemanticType {
    let mut narrowed = base_type.clone();

    let mut if_guards = node
        .if_guards
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == current_owner_name
                && guard.owner_type_name.as_deref() == current_owner_type_name
                && guard.line < current_line
                && !name_reassigned_after_line(
                    node,
                    current_owner_name,
                    current_owner_type_name,
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
            apply_guard_condition_semantic(node, nodes, &narrowed, value_name, guard, branch_true);
    }

    let mut post_if_guards = node
        .if_guards
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == current_owner_name
                && guard.owner_type_name.as_deref() == current_owner_type_name
                && guard.line < current_line
                && !name_reassigned_after_line(
                    node,
                    current_owner_name,
                    current_owner_type_name,
                    value_name,
                    guard.line,
                    current_line,
                )
                && current_line > guard.false_end_line.unwrap_or(guard.true_end_line)
        })
        .filter_map(|guard| {
            let true_terminal = branch_has_return(
                node,
                current_owner_name,
                current_owner_type_name,
                guard.true_start_line,
                guard.true_end_line,
            );
            let false_terminal =
                guard.false_start_line.zip(guard.false_end_line).is_some_and(|(start, end)| {
                    branch_has_return(node, current_owner_name, current_owner_type_name, start, end)
                });
            let branch_true =
                match (true_terminal, false_terminal, guard.false_start_line.is_some()) {
                    (true, false, _) => Some(false),
                    (false, true, true) => Some(true),
                    _ => None,
                }?;
            Some((guard.line, branch_true, guard.guard.as_ref()?))
        })
        .collect::<Vec<_>>();
    post_if_guards.sort_by_key(|(line, _, _)| *line);
    for (_, branch_true, guard) in post_if_guards {
        narrowed =
            apply_guard_condition_semantic(node, nodes, &narrowed, value_name, guard, branch_true);
    }

    let mut asserts = node
        .asserts
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == current_owner_name
                && guard.owner_type_name.as_deref() == current_owner_type_name
                && guard.line < current_line
                && !name_reassigned_after_line(
                    node,
                    current_owner_name,
                    current_owner_type_name,
                    value_name,
                    guard.line,
                    current_line,
                )
        })
        .filter_map(|guard| Some((guard.line, guard.guard.as_ref()?)))
        .collect::<Vec<_>>();
    asserts.sort_by_key(|(line, _)| *line);
    for (_, guard) in asserts {
        narrowed = apply_guard_condition_semantic(node, nodes, &narrowed, value_name, guard, true);
    }

    narrowed
}

pub(super) fn branch_has_return(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    start_line: usize,
    end_line: usize,
) -> bool {
    node.returns.iter().any(|site| {
        site.owner_name == current_owner_name.unwrap_or_default()
            && site.owner_type_name.as_deref() == current_owner_type_name
            && start_line <= site.line
            && site.line <= end_line
    })
}

pub(super) fn name_reassigned_after_line(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    value_name: &str,
    after_line: usize,
    current_line: usize,
) -> bool {
    node.assignments.iter().any(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.as_deref() == current_owner_name
            && assignment.owner_type_name.as_deref() == current_owner_type_name
            && after_line < assignment.line
            && assignment.line < current_line
    }) || node.invalidations.iter().any(|site| {
        site.names.iter().any(|name| name == value_name)
            && site.owner_name.as_deref() == current_owner_name
            && site.owner_type_name.as_deref() == current_owner_type_name
            && after_line < site.line
            && site.line < current_line
    })
}

pub(super) fn latest_delete_invalidation_line(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<usize> {
    node.invalidations
        .iter()
        .rev()
        .find(|site| {
            site.kind == typepython_binding::InvalidationKind::Delete
                && site.names.iter().any(|name| name == value_name)
                && site.owner_name.as_deref() == current_owner_name
                && site.owner_type_name.as_deref() == current_owner_type_name
                && site.line < current_line
        })
        .map(|site| site.line)
}

pub(super) fn apply_guard_condition_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    base_type: &SemanticType,
    value_name: &str,
    guard: &typepython_binding::GuardConditionSite,
    branch_true: bool,
) -> SemanticType {
    match guard {
        typepython_binding::GuardConditionSite::IsNone { name, negated } if name == value_name => {
            match (branch_true, negated) {
                (true, false) | (false, true) => SemanticType::Name(String::from("None")),
                (false, false) | (true, true) => {
                    remove_none_semantic_branch(base_type).unwrap_or_else(|| base_type.clone())
                }
            }
        }
        typepython_binding::GuardConditionSite::IsInstance { name, types }
            if name == value_name =>
        {
            let narrowed_types = types.iter().map(|ty| lower_type_text_or_name(ty)).collect::<Vec<_>>();
            if branch_true {
                narrow_to_instance_semantic_types(node, nodes, base_type, &narrowed_types)
            } else {
                remove_instance_semantic_types(node, nodes, base_type, &narrowed_types)
            }
        }
        typepython_binding::GuardConditionSite::PredicateCall { name, callee }
            if name == value_name =>
        {
            apply_predicate_guard_semantic(node, nodes, base_type, callee, branch_true)
        }
        typepython_binding::GuardConditionSite::TruthyName { name } if name == value_name => {
            apply_truthy_semantic_narrowing(base_type, branch_true)
        }
        typepython_binding::GuardConditionSite::Not(inner) => {
            apply_guard_condition_semantic(node, nodes, base_type, value_name, inner, !branch_true)
        }
        typepython_binding::GuardConditionSite::And(parts) => {
            if branch_true {
                parts.iter().fold(base_type.clone(), |current, part| {
                    apply_guard_condition_semantic(node, nodes, &current, value_name, part, true)
                })
            } else {
                let mut joined = Vec::new();
                let mut current_true = base_type.clone();
                for part in parts {
                    joined.push(apply_guard_condition_semantic(
                        node,
                        nodes,
                        &current_true,
                        value_name,
                        part,
                        false,
                    ));
                    current_true = apply_guard_condition_semantic(
                        node,
                        nodes,
                        &current_true,
                        value_name,
                        part,
                        true,
                    );
                }
                join_semantic_type_candidates(joined)
            }
        }
        typepython_binding::GuardConditionSite::Or(parts) => {
            if branch_true {
                let mut joined = Vec::new();
                let mut current_false = base_type.clone();
                for part in parts {
                    joined.push(apply_guard_condition_semantic(
                        node,
                        nodes,
                        &current_false,
                        value_name,
                        part,
                        true,
                    ));
                    current_false = apply_guard_condition_semantic(
                        node,
                        nodes,
                        &current_false,
                        value_name,
                        part,
                        false,
                    );
                }
                join_semantic_type_candidates(joined)
            } else {
                parts.iter().fold(base_type.clone(), |current, part| {
                    apply_guard_condition_semantic(node, nodes, &current, value_name, part, false)
                })
            }
        }
        _ => base_type.clone(),
    }
}

pub(super) fn apply_predicate_guard_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    base_type: &SemanticType,
    callee: &str,
    branch_true: bool,
) -> SemanticType {
    let Some((kind, guarded_type)) = parse_guard_return_kind_semantic(node, nodes, callee) else {
        return base_type.clone();
    };
    match (kind.as_str(), branch_true) {
        ("TypeGuard", true) | ("TypeIs", true) => {
            narrow_to_instance_semantic_types(node, nodes, base_type, &[guarded_type])
        }
        ("TypeIs", false) => remove_instance_semantic_types(node, nodes, base_type, &[guarded_type]),
        _ => base_type.clone(),
    }
}

pub(super) fn parse_guard_return_kind_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<(String, SemanticType)> {
    let function = resolve_direct_function(node, nodes, callee)?;
    let returns = normalized_direct_return_annotation(function.detail.split_once("->")?.1.trim())?;
    if let Some(inner) =
        returns.strip_prefix("TypeGuard[").and_then(|inner| inner.strip_suffix(']'))
    {
        return Some((String::from("TypeGuard"), lower_type_text_or_name(inner)));
    }
    if let Some(inner) = returns.strip_prefix("TypeIs[").and_then(|inner| inner.strip_suffix(']')) {
        return Some((String::from("TypeIs"), lower_type_text_or_name(inner)));
    }
    None
}

pub(super) fn narrow_to_instance_semantic_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    base_type: &SemanticType,
    types: &[SemanticType],
) -> SemanticType {
    if let Some(branches) = semantic_union_branches(base_type) {
        let kept = branches
            .into_iter()
            .filter(|branch| {
                types
                    .iter()
                    .any(|ty| semantic_type_matches(node, nodes, ty, branch))
            })
            .collect::<Vec<_>>();
        if !kept.is_empty() {
            return join_semantic_type_candidates(kept);
        }
    }
    join_semantic_type_candidates(types.to_vec())
}

pub(super) fn remove_instance_semantic_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    base_type: &SemanticType,
    types: &[SemanticType],
) -> SemanticType {
    let Some(branches) = semantic_union_branches(base_type) else {
        return base_type.clone();
    };
    let kept = branches
        .into_iter()
        .filter(|branch| {
            !types
                .iter()
                .any(|ty| semantic_type_matches(node, nodes, ty, branch))
        })
        .collect::<Vec<_>>();
    if kept.is_empty() { base_type.clone() } else { join_semantic_type_candidates(kept) }
}

pub(super) fn remove_none_branch(base_type: &str) -> Option<String> {
    let normalized = normalize_type_text(base_type);
    let branches = union_branches(&normalized)?;
    let kept = branches.into_iter().filter(|branch| branch != "None").collect::<Vec<_>>();
    (!kept.is_empty()).then(|| join_union_branches(kept))
}

pub(super) fn remove_none_semantic_branch(base_type: &SemanticType) -> Option<SemanticType> {
    let branches = semantic_union_branches(base_type)?;
    let kept = branches
        .into_iter()
        .filter(|branch| !matches!(branch.strip_annotated(), SemanticType::Name(name) if name == "None"))
        .collect::<Vec<_>>();
    (!kept.is_empty()).then(|| join_semantic_type_candidates(kept))
}

pub(super) fn join_union_branches(branches: Vec<String>) -> String {
    if branches.len() == 1 {
        branches.into_iter().next().unwrap_or_default()
    } else {
        format!("Union[{}]", branches.join(", "))
    }
}

pub(super) fn join_type_candidates(candidates: Vec<String>) -> String {
    let mut branches = Vec::new();
    for candidate in candidates {
        if let Some(candidate_branches) = union_branches(&candidate) {
            for branch in candidate_branches {
                if !branches.contains(&branch) {
                    branches.push(branch);
                }
            }
        } else if !branches.contains(&candidate) {
            branches.push(candidate);
        }
    }
    join_union_branches(branches)
}

pub(super) fn apply_truthy_semantic_narrowing(
    base_type: &SemanticType,
    branch_true: bool,
) -> SemanticType {
    if let Some(value) = semantic_literal_bool_value(base_type) {
        return if branch_true == value {
            base_type.clone()
        } else {
            SemanticType::Generic {
                head: String::from("Literal"),
                args: vec![SemanticType::Name(
                    if !value { "True" } else { "False" }.to_owned(),
                )],
            }
        };
    }
    if matches!(base_type.strip_annotated(), SemanticType::Name(name) if name == "bool") {
        return base_type.clone();
    }

    let Some(branches) = semantic_union_branches(base_type) else {
        return base_type.clone();
    };
    let non_none = branches
        .iter()
        .filter(|branch| !matches!(branch.strip_annotated(), SemanticType::Name(name) if name == "None"))
        .cloned()
        .collect::<Vec<_>>();
    if branches
        .iter()
        .any(|branch| matches!(branch.strip_annotated(), SemanticType::Name(name) if name == "None"))
        && non_none
            .iter()
            .all(semantic_is_definitely_truthy_branch)
    {
        return if branch_true {
            join_semantic_type_candidates(non_none)
        } else {
            SemanticType::Name(String::from("None"))
        };
    }

    base_type.clone()
}

pub(super) fn semantic_is_definitely_truthy_branch(branch: &SemanticType) -> bool {
    if semantic_literal_bool_value(branch) == Some(true) {
        return true;
    }
    if semantic_literal_bool_value(branch) == Some(false)
        || matches!(branch.strip_annotated(), SemanticType::Name(name) if name == "None" || name == "bool")
    {
        return false;
    }
    !matches!(
        branch.strip_annotated(),
        SemanticType::Name(name)
            if matches!(
                name.as_str(),
                "bytes" | "str" | "int" | "float" | "complex" | "list" | "dict" | "set" | "tuple"
            )
    )
}

fn semantic_literal_bool_value(ty: &SemanticType) -> Option<bool> {
    let (head, args) = ty.generic_parts()?;
    if head != "Literal" || args.len() != 1 {
        return None;
    }
    match args.first()?.strip_annotated() {
        SemanticType::Name(name) if name == "True" => Some(true),
        SemanticType::Name(name) if name == "False" => Some(false),
        _ => None,
    }
}

pub(super) fn resolve_exception_binding_semantic_type(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<SemanticType> {
    let except_site = node.except_handlers.iter().rev().find(|except_site| {
        except_site.binding_name.as_deref() == Some(value_name)
            && except_site.owner_name.as_deref() == current_owner_name
            && except_site.owner_type_name.as_deref() == current_owner_type_name
            && except_site.line < current_line
            && current_line <= except_site.end_line
    })?;

    Some(lower_type_text_or_name(&normalize_exception_binding_type(
        &except_site.exception_type,
    )))
}

pub(super) fn normalize_exception_binding_type(text: &str) -> String {
    let text = text.trim();
    if let Some(inner) = text.strip_prefix('(').and_then(|inner| inner.strip_suffix(')')) {
        let members = split_top_level_type_args(inner)
            .into_iter()
            .map(normalize_type_text)
            .collect::<Vec<_>>();
        return format!("Union[{}]", members.join(", "));
    }
    normalize_type_text(text)
}

pub(super) fn resolve_for_loop_target_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<SemanticType> {
    let loop_site = node.for_loops.iter().rev().find(|for_loop| {
        (for_loop.target_name == value_name
            || for_loop.target_names.iter().any(|name| name == value_name))
            && for_loop.owner_name.as_deref() == current_owner_name
            && for_loop.owner_type_name.as_deref() == current_owner_type_name
            && for_loop.line < current_line
    })?;

    let iter_type = resolve_direct_expression_semantic_type(
        node,
        nodes,
        signature,
        None,
        loop_site.owner_name.as_deref(),
        loop_site.owner_type_name.as_deref(),
        loop_site.line,
        loop_site.iter_type.as_deref(),
        loop_site.iter_is_awaited,
        loop_site.iter_callee.as_deref(),
        loop_site.iter_name.as_deref(),
        loop_site.iter_member_owner_name.as_deref(),
        loop_site.iter_member_name.as_deref(),
        loop_site.iter_member_through_instance,
        loop_site.iter_method_owner_name.as_deref(),
        loop_site.iter_method_name.as_deref(),
        loop_site.iter_method_through_instance,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    let element_type = unwrap_for_iterable_semantic_type(&iter_type)?;

    if let Some(index) = loop_site.target_names.iter().position(|name| name == value_name) {
        if let Some(elements) = unpacked_fixed_tuple_semantic_elements(&element_type) {
            if elements.len() == loop_site.target_names.len() {
                return elements.get(index).cloned();
            }
            return None;
        }
        return unwrap_for_iterable_semantic_type(&element_type);
    }

    Some(element_type)
}

pub(super) fn resolve_with_target_name_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<SemanticType> {
    let with_site = node.with_statements.iter().rev().find(|with_site| {
        with_site.target_name.as_deref() == Some(value_name)
            && with_site.owner_name.as_deref() == current_owner_name
            && with_site.owner_type_name.as_deref() == current_owner_type_name
            && with_site.line < current_line
    })?;

    resolve_with_target_semantic_type_for_signature(node, nodes, signature, with_site)
}

pub(super) fn resolve_with_owner_signature<'a>(
    node: &'a typepython_graph::ModuleNode,
    with_site: &typepython_binding::WithSite,
) -> Option<&'a str> {
    let owner_name = with_site.owner_name.as_deref()?;
    node.declarations
        .iter()
        .find(|declaration| {
            declaration.kind == DeclarationKind::Function
                && declaration.name == owner_name
                && match (&with_site.owner_type_name, &declaration.owner) {
                    (Some(owner_type_name), Some(owner)) => owner.name == *owner_type_name,
                    (None, None) => true,
                    _ => false,
                }
        })
        .map(|declaration| declaration.detail.as_str())
}

pub(super) fn resolve_with_target_semantic_type_for_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    with_site: &typepython_binding::WithSite,
) -> Option<SemanticType> {
    let context_type = resolve_direct_expression_semantic_type(
        node,
        nodes,
        signature,
        None,
        with_site.owner_name.as_deref(),
        with_site.owner_type_name.as_deref(),
        with_site.line,
        with_site.context_type.as_deref(),
        with_site.context_is_awaited,
        with_site.context_callee.as_deref(),
        with_site.context_name.as_deref(),
        with_site.context_member_owner_name.as_deref(),
        with_site.context_member_name.as_deref(),
        with_site.context_member_through_instance,
        with_site.context_method_owner_name.as_deref(),
        with_site.context_method_name.as_deref(),
        with_site.context_method_through_instance,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    let context_type_name = render_semantic_type(&context_type);
    let (class_node, class_decl) = resolve_direct_base(nodes, node, &context_type_name)?;
    let enter = class_node.declarations.iter().find(|declaration| {
        declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration.name == "__enter__"
            && declaration.kind == DeclarationKind::Function
    })?;
    let exit = class_node.declarations.iter().find(|declaration| {
        declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration.name == "__exit__"
            && declaration.kind == DeclarationKind::Function
    })?;
    let _ = exit;

    normalized_direct_return_annotation(enter.detail.split_once("->")?.1.trim())
        .map(lower_type_text_or_name)
}

pub(super) fn resolve_local_assignment_reference_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<SemanticType> {
    let owner_name = current_owner_name?;
    let deleted_after_line = latest_delete_invalidation_line(
        node,
        Some(owner_name),
        current_owner_type_name,
        current_line,
        value_name,
    );
    if let Some(joined) = resolve_post_if_joined_assignment_semantic_type(
        node,
        nodes,
        signature,
        Some(owner_name),
        current_owner_type_name,
        current_line,
        value_name,
    )
    .filter(|_| deleted_after_line.is_none())
    {
        return Some(joined);
    }
    let assignment = node.assignments.iter().rev().find(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.as_deref() == Some(owner_name)
            && assignment.owner_type_name.as_deref() == current_owner_type_name
            && assignment.line < current_line
            && deleted_after_line.is_none_or(|deleted_line| assignment.line > deleted_line)
    })?;
    resolve_assignment_site_semantic_type(node, nodes, signature, assignment)
}

pub(super) fn resolve_module_level_assignment_reference_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<SemanticType> {
    let deleted_after_line =
        latest_delete_invalidation_line(node, None, None, current_line, value_name);
    if let Some(joined) = resolve_post_if_joined_assignment_semantic_type(
        node,
        nodes,
        signature,
        None,
        None,
        current_line,
        value_name,
    )
    .filter(|_| deleted_after_line.is_none())
    {
        return Some(joined);
    }
    let assignment = node.assignments.iter().rev().find(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.is_none()
            && assignment.line < current_line
            && deleted_after_line.is_none_or(|deleted_line| assignment.line > deleted_line)
    })?;
    resolve_assignment_site_semantic_type(node, nodes, signature, assignment)
}

pub(super) fn resolve_assignment_site_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    assignment: &typepython_binding::AssignmentSite,
) -> Option<SemanticType> {
    if let Some(index) = assignment.destructuring_index {
        let tuple_elements = unpacked_fixed_tuple_semantic_elements(
            &resolve_direct_expression_semantic_type(
            node,
            nodes,
            signature,
            Some(assignment.name.as_str()),
            assignment.owner_name.as_deref(),
            assignment.owner_type_name.as_deref(),
            assignment.line,
            assignment.value_type.as_deref(),
            assignment.is_awaited,
            assignment.value_callee.as_deref(),
            assignment.value_name.as_deref(),
            assignment.value_member_owner_name.as_deref(),
            assignment.value_member_name.as_deref(),
            assignment.value_member_through_instance,
            assignment.value_method_owner_name.as_deref(),
            assignment.value_method_name.as_deref(),
            assignment.value_method_through_instance,
            assignment.value_subscript_target.as_deref(),
            assignment.value_subscript_string_key.as_deref(),
            assignment.value_subscript_index.as_deref(),
            assignment.value_if_true.as_deref(),
            assignment.value_if_false.as_deref(),
            assignment.value_if_guard.as_ref(),
            assignment.value_bool_left.as_deref(),
            assignment.value_bool_right.as_deref(),
            assignment.value_binop_left.as_deref(),
            assignment.value_binop_right.as_deref(),
            assignment.value_binop_operator.as_deref(),
        )?,
        )?;
        let target_names = assignment.destructuring_target_names.as_ref()?;
        if tuple_elements.len() == target_names.len() {
            return tuple_elements.get(index).cloned();
        }
        return None;
    }
    if let Some(comprehension) = assignment.value_list_comprehension.as_deref() {
        return match comprehension.kind {
            typepython_syntax::ComprehensionKind::List => resolve_list_comprehension_semantic_type(
                node,
                nodes,
                signature,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                comprehension,
            ),
            typepython_syntax::ComprehensionKind::Set => resolve_set_comprehension_semantic_type(
                node,
                nodes,
                signature,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                comprehension,
            ),
            typepython_syntax::ComprehensionKind::Dict => resolve_dict_comprehension_semantic_type(
                node,
                nodes,
                signature,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                comprehension,
            ),
            typepython_syntax::ComprehensionKind::Generator => {
                resolve_generator_comprehension_semantic_type(
                    node,
                    nodes,
                    signature,
                    assignment.owner_name.as_deref(),
                    assignment.owner_type_name.as_deref(),
                    assignment.line,
                    comprehension,
                )
            }
        };
    }
    if let Some(comprehension) = assignment.value_generator_comprehension.as_deref() {
        return resolve_generator_comprehension_semantic_type(
            node,
            nodes,
            signature,
            assignment.owner_name.as_deref(),
            assignment.owner_type_name.as_deref(),
            assignment.line,
            comprehension,
        );
    }

    resolve_direct_expression_semantic_type(
        node,
        nodes,
        signature,
        Some(assignment.name.as_str()),
        assignment.owner_name.as_deref(),
        assignment.owner_type_name.as_deref(),
        assignment.line,
        assignment.value_type.as_deref(),
        assignment.is_awaited,
        assignment.value_callee.as_deref(),
        assignment.value_name.as_deref(),
        assignment.value_member_owner_name.as_deref(),
        assignment.value_member_name.as_deref(),
        assignment.value_member_through_instance,
        assignment.value_method_owner_name.as_deref(),
        assignment.value_method_name.as_deref(),
        assignment.value_method_through_instance,
        assignment.value_subscript_target.as_deref(),
        assignment.value_subscript_string_key.as_deref(),
        assignment.value_subscript_index.as_deref(),
        assignment.value_if_true.as_deref(),
        assignment.value_if_false.as_deref(),
        assignment.value_if_guard.as_ref(),
        assignment.value_bool_left.as_deref(),
        assignment.value_bool_right.as_deref(),
        assignment.value_binop_left.as_deref(),
        assignment.value_binop_right.as_deref(),
        assignment.value_binop_operator.as_deref(),
    )
}

pub(super) fn resolve_comprehension_local_semantic_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    comprehension: &typepython_syntax::ComprehensionMetadata,
) -> Option<BTreeMap<String, SemanticType>> {
    let mut local_bindings = BTreeMap::new();
    for clause in &comprehension.clauses {
        let iter_type = resolve_direct_expression_semantic_type_from_metadata(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            clause.iter.as_ref(),
        )?;
        let element_type = unwrap_for_iterable_semantic_type(&iter_type)?;
        bind_list_comprehension_semantic_targets(
            &mut local_bindings,
            &clause.target_names,
            &element_type,
        );
        for guard in &clause.filters {
            for (name, value_type) in local_bindings.clone() {
                local_bindings.insert(
                    name.clone(),
                    apply_guard_condition_semantic(
                        node,
                        nodes,
                        &value_type,
                        &name,
                        &guard_to_site(guard),
                        true,
                    ),
                );
            }
        }
    }
    Some(local_bindings)
}

pub(super) fn resolve_list_comprehension_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    comprehension: &typepython_syntax::ComprehensionMetadata,
) -> Option<SemanticType> {
    let local_bindings = resolve_comprehension_local_semantic_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension,
    )?;

    let element_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension.element.as_ref(),
        &local_bindings,
    )?;
    Some(SemanticType::Generic { head: String::from("list"), args: vec![element_type] })
}

pub(super) fn resolve_set_comprehension_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    comprehension: &typepython_syntax::ComprehensionMetadata,
) -> Option<SemanticType> {
    let local_bindings = resolve_comprehension_local_semantic_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension,
    )?;

    let element_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension.element.as_ref(),
        &local_bindings,
    )?;
    Some(SemanticType::Generic { head: String::from("set"), args: vec![element_type] })
}

pub(super) fn resolve_dict_comprehension_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    comprehension: &typepython_syntax::ComprehensionMetadata,
) -> Option<SemanticType> {
    let local_bindings = resolve_comprehension_local_semantic_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension,
    )?;
    let key_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension.key.as_deref()?,
        &local_bindings,
    )?;
    let value_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension.element.as_ref(),
        &local_bindings,
    )?;
    Some(SemanticType::Generic {
        head: String::from("dict"),
        args: vec![key_type, value_type],
    })
}

pub(super) fn resolve_generator_comprehension_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    comprehension: &typepython_syntax::ComprehensionMetadata,
) -> Option<SemanticType> {
    let local_bindings = resolve_comprehension_local_semantic_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension,
    )?;

    let element_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension.element.as_ref(),
        &local_bindings,
    )?;
    Some(SemanticType::Generic {
        head: String::from("Generator"),
        args: vec![
            element_type,
            SemanticType::Name(String::from("None")),
            SemanticType::Name(String::from("None")),
        ],
    })
}

pub(super) fn collect_guard_binding_names(
    guard: &typepython_binding::GuardConditionSite,
    names: &mut BTreeSet<String>,
) {
    match guard {
        typepython_binding::GuardConditionSite::IsNone { name, .. }
        | typepython_binding::GuardConditionSite::IsInstance { name, .. }
        | typepython_binding::GuardConditionSite::PredicateCall { name, .. }
        | typepython_binding::GuardConditionSite::TruthyName { name } => {
            names.insert(name.clone());
        }
        typepython_binding::GuardConditionSite::Not(inner) => {
            collect_guard_binding_names(inner, names);
        }
        typepython_binding::GuardConditionSite::And(parts)
        | typepython_binding::GuardConditionSite::Or(parts) => {
            for part in parts {
                collect_guard_binding_names(part, names);
            }
        }
    }
}

pub(super) fn apply_guard_to_local_semantic_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    local_bindings: &BTreeMap<String, SemanticType>,
    guard: &typepython_binding::GuardConditionSite,
    branch_true: bool,
) -> BTreeMap<String, SemanticType> {
    let mut narrowed = local_bindings.clone();
    let mut names = BTreeSet::new();
    collect_guard_binding_names(guard, &mut names);
    for name in names {
        if let Some(base_type) = local_bindings.get(&name) {
            narrowed.insert(
                name.clone(),
                apply_guard_condition_semantic(node, nodes, base_type, &name, guard, branch_true),
            );
        }
    }
    narrowed
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_guard_scope_semantic_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    guard: &typepython_binding::GuardConditionSite,
) -> BTreeMap<String, SemanticType> {
    let mut bindings = BTreeMap::new();
    let mut names = BTreeSet::new();
    collect_guard_binding_names(guard, &mut names);
    for name in names {
        if let Some(base_type) = resolve_direct_name_reference_semantic_type(
            node,
            nodes,
            signature,
            exclude_name,
            current_owner_name,
            current_owner_type_name,
            current_line,
            &name,
        ) {
            bindings.insert(name, base_type);
        }
    }
    bindings
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_direct_expression_semantic_type_from_metadata_with_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    local_bindings: &BTreeMap<String, SemanticType>,
) -> Option<SemanticType> {
    if let Some(lambda) = metadata.value_lambda.as_deref() {
        return resolve_contextual_lambda_callable_semantic_type(
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            current_line,
            lambda,
            signature,
            Some(local_bindings),
        );
    }
    if let Some(value_name) = metadata.value_name.as_deref()
        && let Some(bound_type) = local_bindings.get(value_name)
    {
        return Some(bound_type.clone());
    }
    if let Some(target) = metadata.value_subscript_target.as_deref() {
        let target_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            target,
            local_bindings,
        )?;
        return resolve_subscript_type_from_target_semantic_type(
            node,
            nodes,
            &target_type,
            metadata.value_subscript_string_key.as_deref(),
            metadata.value_subscript_index.as_deref(),
        );
    }
    if let (Some(true_branch), Some(false_branch)) =
        (metadata.value_if_true.as_deref(), metadata.value_if_false.as_deref())
    {
        if let Some(guard) = metadata.value_if_guard.as_ref() {
            let guard = guard_to_site(guard);
            let true_bindings =
                apply_guard_to_local_semantic_bindings(node, nodes, local_bindings, &guard, true);
            let false_bindings =
                apply_guard_to_local_semantic_bindings(node, nodes, local_bindings, &guard, false);
            let true_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                true_branch,
                &true_bindings,
            )?;
            let false_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                false_branch,
                &false_bindings,
            )?;
            return Some(join_semantic_type_candidates(vec![true_type, false_type]));
        }
        let true_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            true_branch,
            local_bindings,
        )?;
        let false_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            false_branch,
            local_bindings,
        )?;
        return Some(join_semantic_type_candidates(vec![true_type, false_type]));
    }
    if let (Some(left), Some(right), Some(operator)) = (
        metadata.value_bool_left.as_deref(),
        metadata.value_bool_right.as_deref(),
        metadata.value_binop_operator.as_deref(),
    ) && (operator == "and" || operator == "or")
    {
        let left_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            left,
            local_bindings,
        )?;
        let right_type = if let Some(guard) = metadata.value_if_guard.as_ref() {
            let narrowed_bindings = apply_guard_to_local_semantic_bindings(
                node,
                nodes,
                local_bindings,
                &guard_to_site(guard),
                operator == "and",
            );
            resolve_direct_expression_semantic_type_from_metadata_with_bindings(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                right,
                &narrowed_bindings,
            )?
        } else {
            resolve_direct_expression_semantic_type_from_metadata_with_bindings(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                right,
                local_bindings,
            )?
        };
        return Some(join_semantic_type_candidates(vec![left_type, right_type]));
    }
    if let (Some(left), Some(right), Some(operator)) = (
        metadata.value_binop_left.as_deref(),
        metadata.value_binop_right.as_deref(),
        metadata.value_binop_operator.as_deref(),
    ) {
        let left_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            left,
            local_bindings,
        )?;
        let right_type = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            right,
            local_bindings,
        )?;
        if let Some(result) = resolve_binop_result_semantic_type(&left_type, &right_type, operator)
        {
            return Some(result);
        }
    }

    resolve_direct_expression_semantic_type(
        node,
        nodes,
        signature,
        None,
        current_owner_name,
        current_owner_type_name,
        current_line,
        metadata.value_type.as_deref(),
        metadata.is_awaited,
        metadata.value_callee.as_deref(),
        metadata.value_name.as_deref(),
        metadata.value_member_owner_name.as_deref(),
        metadata.value_member_name.as_deref(),
        metadata.value_member_through_instance,
        metadata.value_method_owner_name.as_deref(),
        metadata.value_method_name.as_deref(),
        metadata.value_method_through_instance,
        metadata.value_subscript_target.as_deref(),
        metadata.value_subscript_string_key.as_deref(),
        metadata.value_subscript_index.as_deref(),
        metadata.value_if_true.as_deref(),
        metadata.value_if_false.as_deref(),
        metadata.value_if_guard.as_ref().map(guard_to_site).as_ref(),
        metadata.value_bool_left.as_deref(),
        metadata.value_bool_right.as_deref(),
        metadata.value_binop_left.as_deref(),
        metadata.value_binop_right.as_deref(),
        metadata.value_binop_operator.as_deref(),
    )
}

pub(super) fn bind_list_comprehension_semantic_targets(
    local_bindings: &mut BTreeMap<String, SemanticType>,
    target_names: &[String],
    element_type: &SemanticType,
) {
    if target_names.is_empty() {
        return;
    }
    if target_names.len() == 1 {
        local_bindings.insert(target_names[0].clone(), element_type.clone());
        return;
    }
    if let Some(tuple_elements) = unpacked_fixed_tuple_semantic_elements(element_type)
        && tuple_elements.len() == target_names.len()
    {
        for (name, value_type) in target_names.iter().zip(tuple_elements) {
            local_bindings.insert(name.clone(), value_type);
        }
        return;
    }
    for name in target_names {
        local_bindings.insert(name.clone(), element_type.clone());
    }
}

pub(super) fn guard_to_site(
    guard: &typepython_syntax::GuardCondition,
) -> typepython_binding::GuardConditionSite {
    match guard {
        typepython_syntax::GuardCondition::IsNone { name, negated } => {
            typepython_binding::GuardConditionSite::IsNone { name: name.clone(), negated: *negated }
        }
        typepython_syntax::GuardCondition::IsInstance { name, types } => {
            typepython_binding::GuardConditionSite::IsInstance {
                name: name.clone(),
                types: types.clone(),
            }
        }
        typepython_syntax::GuardCondition::PredicateCall { name, callee } => {
            typepython_binding::GuardConditionSite::PredicateCall {
                name: name.clone(),
                callee: callee.clone(),
            }
        }
        typepython_syntax::GuardCondition::TruthyName { name } => {
            typepython_binding::GuardConditionSite::TruthyName { name: name.clone() }
        }
        typepython_syntax::GuardCondition::Not(inner) => {
            typepython_binding::GuardConditionSite::Not(Box::new(guard_to_site(inner)))
        }
        typepython_syntax::GuardCondition::And(parts) => {
            typepython_binding::GuardConditionSite::And(parts.iter().map(guard_to_site).collect())
        }
        typepython_syntax::GuardCondition::Or(parts) => {
            typepython_binding::GuardConditionSite::Or(parts.iter().map(guard_to_site).collect())
        }
    }
}

pub(super) fn resolve_post_if_joined_assignment_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<SemanticType> {
    let mut guards = node
        .if_guards
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == current_owner_name
                && guard.owner_type_name.as_deref() == current_owner_type_name
                && guard.false_start_line.is_some()
                && guard.false_end_line.is_some()
        })
        .filter_map(|guard| {
            let false_end = guard.false_end_line?;
            let after_line = guard.true_end_line.max(false_end);
            (current_line > after_line).then_some((after_line, guard))
        })
        .collect::<Vec<_>>();
    guards.sort_by_key(|(after_line, _)| *after_line);

    for (after_line, guard) in guards.into_iter().rev() {
        if name_reassigned_after_line(
            node,
            current_owner_name,
            current_owner_type_name,
            value_name,
            after_line,
            current_line,
        ) {
            continue;
        }

        let true_assignment = latest_assignment_in_range(
            node,
            current_owner_name,
            current_owner_type_name,
            value_name,
            guard.true_start_line,
            guard.true_end_line,
        )?;
        let false_assignment = latest_assignment_in_range(
            node,
            current_owner_name,
            current_owner_type_name,
            value_name,
            guard.false_start_line?,
            guard.false_end_line?,
        )?;
        let true_type =
            resolve_assignment_site_semantic_type(node, nodes, signature, true_assignment)?;
        let false_type =
            resolve_assignment_site_semantic_type(node, nodes, signature, false_assignment)?;
        return Some(join_semantic_type_candidates(vec![true_type, false_type]));
    }

    None
}

pub(super) fn latest_assignment_in_range<'a>(
    node: &'a typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    value_name: &str,
    start_line: usize,
    end_line: usize,
) -> Option<&'a typepython_binding::AssignmentSite> {
    node.assignments.iter().rev().find(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.as_deref() == current_owner_name
            && assignment.owner_type_name.as_deref() == current_owner_type_name
            && start_line <= assignment.line
            && assignment.line <= end_line
    })
}

pub(super) fn join_branch_types(types: Vec<String>) -> String {
    if types.iter().any(|ty| ty == "Any") {
        return String::from("Any");
    }
    join_type_candidates(types)
}

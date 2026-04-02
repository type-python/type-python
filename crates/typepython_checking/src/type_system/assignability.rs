pub(super) fn direct_type_matches(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    let expected = normalize_type_text(expected);
    let actual = normalize_type_text(actual);
    let mut visiting = BTreeSet::new();

    direct_type_matches_normalized(node, nodes, &expected, &actual, &mut visiting)
}

pub(super) fn direct_type_is_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    let expected = normalize_type_text(expected);
    let actual = normalize_type_text(actual);
    let mut visiting = BTreeSet::new();
    direct_type_is_assignable_normalized(node, nodes, &expected, &actual, &mut visiting)
}

pub(super) fn direct_type_matches_normalized(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
    visiting: &mut BTreeSet<(String, String)>,
) -> bool {
    if let Some(inner) = annotated_inner(expected) {
        return direct_type_matches_normalized(node, nodes, &inner, actual, visiting);
    }
    if let Some(inner) = annotated_inner(actual) {
        return direct_type_matches_normalized(node, nodes, expected, &inner, visiting);
    }

    if expected == actual || expected == "Any" || actual == "Any" {
        return true;
    }

    let key = (expected.to_owned(), actual.to_owned());
    if !visiting.insert(key.clone()) {
        return true;
    }

    let result = if let Some(expanded_expected) = expand_type_alias_once(node, nodes, expected) {
        direct_type_matches_normalized(node, nodes, &expanded_expected, actual, visiting)
    } else if let Some(expanded_actual) = expand_type_alias_once(node, nodes, actual) {
        direct_type_matches_normalized(node, nodes, expected, &expanded_actual, visiting)
    } else if let Some(branches) = union_branches(expected) {
        if let Some(actual_branches) = union_branches(actual) {
            actual_branches.iter().all(|actual_branch| {
                branches.iter().any(|expected_branch| {
                    direct_type_matches_normalized(
                        node,
                        nodes,
                        expected_branch,
                        actual_branch,
                        visiting,
                    )
                })
            }) && branches.iter().all(|expected_branch| {
                actual_branches.iter().any(|actual_branch| {
                    direct_type_matches_normalized(
                        node,
                        nodes,
                        expected_branch,
                        actual_branch,
                        visiting,
                    )
                })
            })
        } else {
            branches.into_iter().any(|branch| {
                direct_type_matches_normalized(node, nodes, &branch, actual, visiting)
            })
        }
    } else if enum_member_owner_name(actual).is_some_and(|owner| owner == expected) {
        true
    } else {
        match (split_generic_type(expected), split_generic_type(actual)) {
            (Some((expected_head, expected_args)), Some((actual_head, actual_args)))
                if expected_head == actual_head =>
            {
                let (expected_args, actual_args) = if expected_head == "tuple" {
                    (
                        expanded_tuple_shape_args(&expected_args),
                        expanded_tuple_shape_args(&actual_args),
                    )
                } else {
                    (expected_args, actual_args)
                };
                if expected_args.len() != actual_args.len() {
                    false
                } else {
                    expected_args.iter().zip(actual_args.iter()).all(
                        |(expected_arg, actual_arg)| {
                            direct_type_matches_normalized(
                                node,
                                nodes,
                                expected_arg,
                                actual_arg,
                                visiting,
                            )
                        },
                    )
                }
            }
            _ => false,
        }
    };

    visiting.remove(&key);
    result
}

pub(super) fn direct_type_matches_normalized_plain(expected: &str, actual: &str) -> bool {
    if let Some(inner) = annotated_inner(expected) {
        return direct_type_matches_normalized_plain(&inner, actual);
    }
    if let Some(inner) = annotated_inner(actual) {
        return direct_type_matches_normalized_plain(expected, &inner);
    }

    if expected == actual || expected == "Any" || actual == "Any" {
        return true;
    }

    if let Some(branches) = union_branches(expected) {
        if let Some(actual_branches) = union_branches(actual) {
            return actual_branches.iter().all(|actual_branch| {
                branches.iter().any(|expected_branch| {
                    direct_type_matches_normalized_plain(expected_branch, actual_branch)
                })
            }) && branches.iter().all(|expected_branch| {
                actual_branches.iter().any(|actual_branch| {
                    direct_type_matches_normalized_plain(expected_branch, actual_branch)
                })
            });
        }
        return branches
            .into_iter()
            .any(|branch| direct_type_matches_normalized_plain(&branch, actual));
    }

    if enum_member_owner_name(actual).is_some_and(|owner| owner == expected) {
        return true;
    }

    match (split_generic_type(expected), split_generic_type(actual)) {
        (Some((expected_head, expected_args)), Some((actual_head, actual_args)))
            if expected_head == actual_head && expected_args.len() == actual_args.len() =>
        {
            expected_args.iter().zip(actual_args.iter()).all(|(expected_arg, actual_arg)| {
                direct_type_matches_normalized_plain(expected_arg, actual_arg)
            })
        }
        _ => false,
    }
}

pub(super) fn direct_type_is_assignable_normalized(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
    visiting: &mut BTreeSet<(String, String)>,
) -> bool {
    if let Some(inner) = annotated_inner(expected) {
        return direct_type_is_assignable_normalized(node, nodes, &inner, actual, visiting);
    }
    if let Some(inner) = annotated_inner(actual) {
        return direct_type_is_assignable_normalized(node, nodes, expected, &inner, visiting);
    }

    if expected == actual
        || expected == "Any"
        || expected == "unknown"
        || expected == "dynamic"
        || actual == "Any"
        || actual == "unknown"
        || actual == "dynamic"
    {
        return true;
    }

    let key = (expected.to_owned(), actual.to_owned());
    if !visiting.insert(key.clone()) {
        return true;
    }

    let result = if let Some(expanded_expected) = expand_type_alias_once(node, nodes, expected) {
        direct_type_is_assignable_normalized(node, nodes, &expanded_expected, actual, visiting)
    } else if let Some(expanded_actual) = expand_type_alias_once(node, nodes, actual) {
        direct_type_is_assignable_normalized(node, nodes, expected, &expanded_actual, visiting)
    } else if let Some(branches) = union_branches(expected) {
        if let Some(actual_branches) = union_branches(actual) {
            actual_branches.iter().all(|actual_branch| {
                branches.iter().any(|expected_branch| {
                    direct_type_is_assignable_normalized(
                        node,
                        nodes,
                        expected_branch,
                        actual_branch,
                        visiting,
                    )
                })
            })
        } else {
            branches.into_iter().any(|branch| {
                direct_type_is_assignable_normalized(node, nodes, &branch, actual, visiting)
            })
        }
    } else if enum_member_owner_name(actual).is_some_and(|owner| owner == expected)
        || protocol_assignable(node, nodes, expected, actual)
        || nominal_subclass_assignable(node, nodes, expected, actual)
    {
        true
    } else if let Some(result) = assignable_generic_bridge(node, nodes, expected, actual) {
        result
    } else {
        direct_type_matches(node, nodes, expected, actual)
    };

    visiting.remove(&key);
    result
}

pub(super) fn nominal_subclass_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    if expected == actual {
        return true;
    }
    let Some((actual_node, actual_decl)) = resolve_direct_base(nodes, node, actual) else {
        return false;
    };
    actual_decl.bases.iter().any(|base| {
        normalize_type_text(base) == expected
            || direct_type_is_assignable(actual_node, nodes, expected, base)
    })
}

pub(super) fn protocol_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    let Some((interface_node, interface_decl)) = resolve_direct_base(nodes, node, expected) else {
        return false;
    };
    if !is_interface_like_declaration(interface_node, interface_decl, nodes) {
        return false;
    }
    let Some((actual_node, actual_decl)) = resolve_direct_base(nodes, node, actual) else {
        return false;
    };
    type_satisfies_interface(nodes, actual_node, actual_decl, interface_node, interface_decl)
}

pub(super) fn type_satisfies_interface(
    nodes: &[typepython_graph::ModuleNode],
    actual_node: &typepython_graph::ModuleNode,
    actual_decl: &Declaration,
    interface_node: &typepython_graph::ModuleNode,
    interface_decl: &Declaration,
) -> bool {
    collect_interface_members(interface_node, interface_decl, nodes).into_iter().all(|required| {
        actual_member_satisfies_requirement(nodes, actual_node, actual_decl, &required)
    })
}

#[derive(Debug, Clone)]
pub(super) struct InterfaceMemberRequirement {
    pub(super) name: String,
    pub(super) declaration: Declaration,
}

pub(super) fn collect_interface_members(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<InterfaceMemberRequirement> {
    let mut visited = BTreeSet::new();
    let mut requirements = BTreeMap::new();
    collect_interface_members_with_visited(
        node,
        declaration,
        nodes,
        &mut visited,
        &mut requirements,
    );
    requirements.into_values().collect()
}

pub(super) fn collect_interface_members_with_visited(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    nodes: &[typepython_graph::ModuleNode],
    visited: &mut BTreeSet<(String, String)>,
    requirements: &mut BTreeMap<String, InterfaceMemberRequirement>,
) {
    let key = (node.module_key.clone(), declaration.name.clone());
    if !visited.insert(key) {
        return;
    }

    for member in node.declarations.iter().filter(|candidate| {
        candidate.owner.as_ref().is_some_and(|owner| owner.name == declaration.name)
            && matches!(candidate.kind, DeclarationKind::Value | DeclarationKind::Function)
    }) {
        requirements.entry(member.name.clone()).or_insert_with(|| InterfaceMemberRequirement {
            name: member.name.clone(),
            declaration: member.clone(),
        });
    }

    for base in &declaration.bases {
        if let Some((base_node, base_decl)) = resolve_direct_base(nodes, node, base)
            && is_interface_like_declaration(base_node, base_decl, nodes)
        {
            collect_interface_members_with_visited(
                base_node,
                base_decl,
                nodes,
                visited,
                requirements,
            );
        }
    }
}

pub(super) fn actual_member_satisfies_requirement(
    nodes: &[typepython_graph::ModuleNode],
    actual_node: &typepython_graph::ModuleNode,
    actual_decl: &Declaration,
    requirement: &InterfaceMemberRequirement,
) -> bool {
    match requirement.declaration.kind {
        DeclarationKind::Function => {
            find_apparent_callable_declaration(nodes, actual_node, actual_decl, &requirement.name)
                .is_some_and(|member| {
                    methods_are_compatible_for_override(
                        actual_node,
                        nodes,
                        member,
                        &requirement.declaration,
                    )
                })
        }
        DeclarationKind::Value => {
            find_apparent_value_declaration(nodes, actual_node, actual_decl, &requirement.name)
                .is_some_and(|member| {
                    let expected = normalize_type_text(requirement.declaration.detail.as_str());
                    let actual = normalize_type_text(member.detail.as_str());
                    expected.is_empty()
                        || actual.is_empty()
                        || direct_type_is_assignable(actual_node, nodes, &expected, &actual)
                })
        }
        _ => false,
    }
}

pub(super) fn find_apparent_value_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<&'a Declaration> {
    find_apparent_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
        declaration.kind == DeclarationKind::Value
    })
}

pub(super) fn find_apparent_callable_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<&'a Declaration> {
    find_apparent_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
        declaration.kind == DeclarationKind::Function
    })
}

pub(super) fn find_apparent_member_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
    predicate: impl Fn(&Declaration) -> bool + Copy,
) -> Option<&'a Declaration> {
    let mut visited = BTreeSet::new();
    find_apparent_member_declaration_with_visited(
        nodes,
        class_node,
        class_decl,
        member_name,
        predicate,
        &mut visited,
    )
}

pub(super) fn find_apparent_member_declaration_with_visited<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
    predicate: impl Fn(&Declaration) -> bool + Copy,
    visited: &mut BTreeSet<(String, String)>,
) -> Option<&'a Declaration> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visited.insert(key) {
        return None;
    }

    if let Some(local) = class_node.declarations.iter().find(|declaration| {
        declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration.name == member_name
            && predicate(declaration)
    }) {
        return Some(local);
    }

    for base in &class_decl.bases {
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) else {
            continue;
        };
        if is_interface_like_declaration(base_node, base_decl, nodes) {
            continue;
        }
        if let Some(inherited) = find_apparent_member_declaration_with_visited(
            nodes,
            base_node,
            base_decl,
            member_name,
            predicate,
            visited,
        ) {
            return Some(inherited);
        }
    }

    None
}

pub(super) fn assignable_generic_bridge(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> Option<bool> {
    let (expected_head, expected_args) = split_generic_type(expected)?;
    let (actual_head, actual_args) = split_generic_type(actual)?;

    if expected_head == "tuple" && actual_head == "tuple" {
        let expected_args = expanded_tuple_shape_args(&expected_args);
        let actual_args = expanded_tuple_shape_args(&actual_args);
        if expected_args.len() != actual_args.len() {
            return Some(false);
        }
        return same_head_generic_assignable(
            node,
            nodes,
            expected_head,
            &expected_args,
            &actual_args,
        );
    }

    if expected_head == actual_head && expected_args.len() == actual_args.len() {
        return same_head_generic_assignable(
            node,
            nodes,
            expected_head,
            &expected_args,
            &actual_args,
        );
    }

    match (expected_head, actual_head) {
        ("Sequence", "list") | ("Sequence", "tuple") if !expected_args.is_empty() => {
            if actual_head == "tuple" && actual_args.len() == 2 && actual_args[1] == "..." {
                return Some(direct_type_is_assignable(
                    node,
                    nodes,
                    &expected_args[0],
                    &actual_args[0],
                ));
            }
            let element = if actual_head == "tuple" {
                join_branch_types(actual_args)
            } else {
                actual_args.first().cloned().unwrap_or_default()
            };
            return Some(direct_type_is_assignable(node, nodes, &expected_args[0], &element));
        }
        ("Mapping", "dict") if expected_args.len() == 2 && actual_args.len() == 2 => {
            return Some(
                invariant_type_matches(node, nodes, &expected_args[0], &actual_args[0])
                    && direct_type_is_assignable(node, nodes, &expected_args[1], &actual_args[1]),
            );
        }
        _ => {}
    }

    None
}

#[derive(Clone, Copy)]
pub(super) enum GenericVariance {
    Invariant,
    Covariant,
    Contravariant,
}

pub(super) fn same_head_generic_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    head: &str,
    expected_args: &[String],
    actual_args: &[String],
) -> Option<bool> {
    if head == "Callable" {
        return callable_annotation_assignable(node, nodes, expected_args, actual_args);
    }

    let variances = variances_for_generic_head(head, expected_args.len());
    Some(expected_args.iter().zip(actual_args.iter()).zip(variances).all(
        |((expected_arg, actual_arg), variance)| match variance {
            GenericVariance::Invariant => {
                invariant_type_matches(node, nodes, expected_arg, actual_arg)
            }
            GenericVariance::Covariant => {
                direct_type_is_assignable(node, nodes, expected_arg, actual_arg)
            }
            GenericVariance::Contravariant => {
                direct_type_is_assignable(node, nodes, actual_arg, expected_arg)
            }
        },
    ))
}

pub(crate) fn expanded_tuple_shape_args(args: &[String]) -> Vec<String> {
    let mut expanded = Vec::new();
    for arg in args {
        if let Some(inner) = unpack_inner(arg)
            && let Some(elements) = unpacked_fixed_tuple_elements(&inner)
        {
            expanded.extend(elements);
            continue;
        }
        expanded.push(normalize_type_text(arg));
    }
    expanded
}

pub(super) fn callable_annotation_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_args: &[String],
    actual_args: &[String],
) -> Option<bool> {
    let expected = format!("Callable[{}]", expected_args.join(", "));
    let actual = format!("Callable[{}]", actual_args.join(", "));
    let (expected_params, expected_return) = parse_callable_annotation(&expected)?;
    let (actual_params, actual_return) = parse_callable_annotation(&actual)?;

    if !direct_type_is_assignable(node, nodes, &expected_return, &actual_return) {
        return Some(false);
    }

    match (expected_params, actual_params) {
        (None, _) | (_, None) => Some(true),
        (Some(expected_params), Some(actual_params)) => {
            if expected_params.len() != actual_params.len() {
                return Some(false);
            }
            Some(expected_params.iter().zip(actual_params.iter()).all(
                |(expected_param, actual_param)| {
                    direct_type_is_assignable(node, nodes, actual_param, expected_param)
                },
            ))
        }
    }
}

pub(super) fn invariant_type_matches(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    (direct_type_matches(node, nodes, expected, actual)
        && direct_type_matches(node, nodes, actual, expected))
        || recursive_type_alias_head(node, nodes, expected)
            .is_some_and(|_| direct_type_is_assignable(node, nodes, expected, actual))
}

pub(super) fn recursive_type_alias_head(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    text: &str,
) -> Option<String> {
    let normalized = normalize_type_text(text);
    let head =
        split_generic_type(&normalized).map(|(head, _)| head.to_owned()).unwrap_or(normalized);
    let (alias_node, alias_decl) = resolve_direct_type_alias(nodes, node, &head)?;
    let mut visiting = BTreeSet::new();
    type_alias_eventually_mentions(
        alias_node,
        nodes,
        alias_decl.name.as_str(),
        &head,
        &mut visiting,
    )
    .then_some(head)
}

pub(super) fn type_alias_eventually_mentions(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current: &str,
    target: &str,
    visiting: &mut BTreeSet<(String, String)>,
) -> bool {
    let Some((alias_node, alias_decl)) = resolve_direct_type_alias(nodes, node, current) else {
        return false;
    };
    let key = (alias_node.module_key.clone(), alias_decl.name.clone());
    if !visiting.insert(key.clone()) {
        return alias_decl.name == target;
    }

    let result =
        type_expr_mentions_alias(alias_node, nodes, alias_decl.detail.as_str(), target, visiting);
    visiting.remove(&key);
    result
}

pub(super) fn type_expr_mentions_alias(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    text: &str,
    target: &str,
    visiting: &mut BTreeSet<(String, String)>,
) -> bool {
    let normalized = normalize_type_text(text);

    if let Some(inner) = annotated_inner(&normalized) {
        return type_expr_mentions_alias(node, nodes, &inner, target, visiting);
    }
    if let Some(branches) = union_branches(&normalized) {
        return branches
            .into_iter()
            .any(|branch| type_expr_mentions_alias(node, nodes, &branch, target, visiting));
    }
    if let Some((head, args)) = split_generic_type(&normalized) {
        return head == target
            || type_alias_eventually_mentions(node, nodes, head, target, visiting)
            || args.iter().any(|arg| type_expr_mentions_alias(node, nodes, arg, target, visiting));
    }

    normalized == target
        || type_alias_eventually_mentions(node, nodes, &normalized, target, visiting)
}

pub(super) fn variances_for_generic_head(head: &str, arity: usize) -> Vec<GenericVariance> {
    match head {
        "Sequence" | "Iterable" | "Iterator" | "Reversible" | "Collection" | "AbstractSet"
        | "frozenset" | "tuple" | "type" => vec![GenericVariance::Covariant; arity],
        "Mapping" if arity == 2 => {
            vec![GenericVariance::Invariant, GenericVariance::Covariant]
        }
        "Generator" if arity == 3 => vec![
            GenericVariance::Covariant,
            GenericVariance::Contravariant,
            GenericVariance::Covariant,
        ],
        _ => vec![GenericVariance::Invariant; arity],
    }
}

pub(super) fn enum_member_owner_name(text: &str) -> Option<String> {
    let inner = text.strip_prefix("Literal[")?.strip_suffix(']')?;
    let (owner, _member) = inner.rsplit_once('.')?;
    Some(normalize_type_text(owner))
}

pub(super) fn split_generic_type(text: &str) -> Option<(&str, Vec<String>)> {
    let text = text.trim();
    let open_index = text.find('[')?;
    let inner = text.strip_suffix(']')?;
    let head = &inner[..open_index];
    let args = split_top_level_type_args(&inner[open_index + 1..])
        .into_iter()
        .map(normalize_type_text)
        .collect::<Vec<_>>();
    Some((head, args))
}

pub(super) fn resolve_builtin_return_type(callee: &str) -> Option<&'static str> {
    BUILTIN_FUNCTION_RETURN_TYPES
        .iter()
        .find_map(|(name, return_type)| (*name == callee).then_some(*return_type))
}

pub(super) fn resolve_typing_callable_signature(callee: &str) -> Option<&'static str> {
    TYPING_SYNTHETIC_CALLABLE_SIGNATURES
        .iter()
        .find_map(|(name, signature)| (*name == callee).then_some(*signature))
}

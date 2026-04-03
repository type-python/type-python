pub(super) fn direct_type_matches(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    let mut types = TypeStore::default();
    let expected = types.intern(lower_type_text_or_name(expected));
    let actual = types.intern(lower_type_text_or_name(actual));
    direct_semantic_type_matches(
        node,
        nodes,
        types.get(expected).expect("interned semantic expected type"),
        types.get(actual).expect("interned semantic actual type"),
        &mut BTreeSet::new(),
    )
}

pub(super) fn direct_type_is_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    let mut types = TypeStore::default();
    let expected = types.intern(lower_type_text_or_name(expected));
    let actual = types.intern(lower_type_text_or_name(actual));
    direct_semantic_type_is_assignable(
        node,
        nodes,
        types.get(expected).expect("interned semantic expected type"),
        types.get(actual).expect("interned semantic actual type"),
        &mut BTreeSet::new(),
    )
}

pub(super) fn direct_type_matches_normalized_plain(expected: &str, actual: &str) -> bool {
    let expected = lower_type_text_or_name(expected);
    let actual = lower_type_text_or_name(actual);
    direct_semantic_type_matches_plain(&expected, &actual)
}

fn direct_semantic_type_matches(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
    visiting: &mut BTreeSet<(SemanticType, SemanticType)>,
) -> bool {
    let expected = expected.strip_annotated().clone();
    let actual = actual.strip_annotated().clone();

    if expected == actual || is_any_semantic_type(&expected) || is_any_semantic_type(&actual) {
        return true;
    }

    let key = (expected.clone(), actual.clone());
    if !visiting.insert(key.clone()) {
        return true;
    }

    let result =
        if let Some(expanded_expected) = expand_semantic_type_alias_once(node, nodes, &expected) {
            direct_semantic_type_matches(node, nodes, &expanded_expected, &actual, visiting)
        } else if let Some(expanded_actual) = expand_semantic_type_alias_once(node, nodes, &actual)
        {
            direct_semantic_type_matches(node, nodes, &expected, &expanded_actual, visiting)
        } else if let Some(branches) = semantic_union_branches(&expected) {
            if let Some(actual_branches) = semantic_union_branches(&actual) {
                actual_branches.iter().all(|actual_branch| {
                    branches.iter().any(|expected_branch| {
                        direct_semantic_type_matches(
                            node,
                            nodes,
                            expected_branch,
                            actual_branch,
                            visiting,
                        )
                    })
                }) && branches.iter().all(|expected_branch| {
                    actual_branches.iter().any(|actual_branch| {
                        direct_semantic_type_matches(
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
                    direct_semantic_type_matches(node, nodes, &branch, &actual, visiting)
                })
            }
        } else if semantic_enum_member_owner_name(&actual)
            .is_some_and(|owner| owner == render_semantic_type(&expected))
        {
            true
        } else {
            match (expected.generic_parts(), actual.generic_parts()) {
                (Some((expected_head, expected_args)), Some((actual_head, actual_args)))
                    if expected_head == actual_head =>
                {
                    let (expected_args, actual_args) = if expected_head == "tuple" {
                        (
                            expanded_tuple_shape_semantic_args(expected_args),
                            expanded_tuple_shape_semantic_args(actual_args),
                        )
                    } else {
                        (expected_args.to_vec(), actual_args.to_vec())
                    };
                    expected_args.len() == actual_args.len()
                        && expected_args.iter().zip(actual_args.iter()).all(
                            |(expected_arg, actual_arg)| {
                                direct_semantic_type_matches(
                                    node,
                                    nodes,
                                    expected_arg,
                                    actual_arg,
                                    visiting,
                                )
                            },
                        )
                }
                _ => false,
            }
        };

    visiting.remove(&key);
    result
}

fn direct_semantic_type_matches_plain(expected: &SemanticType, actual: &SemanticType) -> bool {
    let expected = expected.strip_annotated();
    let actual = actual.strip_annotated();

    if expected == actual || is_any_semantic_type(expected) || is_any_semantic_type(actual) {
        return true;
    }

    if let Some(branches) = semantic_union_branches(expected) {
        if let Some(actual_branches) = semantic_union_branches(actual) {
            return actual_branches.iter().all(|actual_branch| {
                branches.iter().any(|expected_branch| {
                    direct_semantic_type_matches_plain(expected_branch, actual_branch)
                })
            }) && branches.iter().all(|expected_branch| {
                actual_branches.iter().any(|actual_branch| {
                    direct_semantic_type_matches_plain(expected_branch, actual_branch)
                })
            });
        }
        return branches
            .into_iter()
            .any(|branch| direct_semantic_type_matches_plain(&branch, actual));
    }

    if semantic_enum_member_owner_name(actual)
        .is_some_and(|owner| owner == render_semantic_type(expected))
    {
        return true;
    }

    match (expected.generic_parts(), actual.generic_parts()) {
        (Some((expected_head, expected_args)), Some((actual_head, actual_args)))
            if expected_head == actual_head && expected_args.len() == actual_args.len() =>
        {
            expected_args.iter().zip(actual_args.iter()).all(|(expected_arg, actual_arg)| {
                direct_semantic_type_matches_plain(expected_arg, actual_arg)
            })
        }
        _ => false,
    }
}

fn direct_semantic_type_is_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
    visiting: &mut BTreeSet<(SemanticType, SemanticType)>,
) -> bool {
    let expected = expected.strip_annotated().clone();
    let actual = actual.strip_annotated().clone();

    if expected == actual
        || is_top_assignable_semantic_type(&expected)
        || is_top_assignable_semantic_type(&actual)
    {
        return true;
    }

    let key = (expected.clone(), actual.clone());
    if !visiting.insert(key.clone()) {
        return true;
    }

    let expected_rendered = render_semantic_type(&expected);
    let actual_rendered = render_semantic_type(&actual);
    let result =
        if let Some(expanded_expected) = expand_semantic_type_alias_once(node, nodes, &expected) {
            direct_semantic_type_is_assignable(node, nodes, &expanded_expected, &actual, visiting)
        } else if let Some(expanded_actual) = expand_semantic_type_alias_once(node, nodes, &actual)
        {
            direct_semantic_type_is_assignable(node, nodes, &expected, &expanded_actual, visiting)
        } else if let Some(branches) = semantic_union_branches(&expected) {
            if let Some(actual_branches) = semantic_union_branches(&actual) {
                actual_branches.iter().all(|actual_branch| {
                    branches.iter().any(|expected_branch| {
                        direct_semantic_type_is_assignable(
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
                    direct_semantic_type_is_assignable(node, nodes, &branch, &actual, visiting)
                })
            }
        } else {
            let enum_match =
                semantic_enum_member_owner_name(&actual).is_some_and(|owner| owner == expected_rendered);
            let protocol = protocol_assignable(node, nodes, &expected_rendered, &actual_rendered);
            let nominal = nominal_subclass_assignable(node, nodes, &expected_rendered, &actual_rendered);
            if enum_match || protocol || nominal {
                true
            } else if let Some(result) =
                assignable_semantic_generic_bridge(node, nodes, &expected, &actual)
            {
                result
            } else {
                direct_semantic_type_matches(node, nodes, &expected, &actual, &mut BTreeSet::new())
            }
        };

    visiting.remove(&key);
    result
}

fn is_any_semantic_type(ty: &SemanticType) -> bool {
    matches!(ty, SemanticType::Name(name) if name == "Any")
}

fn is_top_assignable_semantic_type(ty: &SemanticType) -> bool {
    matches!(ty, SemanticType::Name(name) if matches!(name.as_str(), "Any" | "unknown" | "dynamic"))
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

fn assignable_semantic_generic_bridge(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
) -> Option<bool> {
    let (expected_head, expected_args) = expected.generic_parts()?;
    let (actual_head, actual_args) = actual.generic_parts()?;

    if expected_head == "tuple" && actual_head == "tuple" {
        let expected_args = expanded_tuple_shape_semantic_args(expected_args);
        let actual_args = expanded_tuple_shape_semantic_args(actual_args);
        if expected_args.len() != actual_args.len() {
            return Some(false);
        }
        return same_head_semantic_generic_assignable(
            node,
            nodes,
            expected_head,
            &expected_args,
            &actual_args,
        );
    }

    if expected_head == actual_head && expected_args.len() == actual_args.len() {
        return same_head_semantic_generic_assignable(
            node,
            nodes,
            expected_head,
            expected_args,
            actual_args,
        );
    }

    match (expected_head, actual_head) {
        ("Sequence", "list") | ("Sequence", "tuple") if !expected_args.is_empty() => {
            if actual_head == "tuple"
                && actual_args.len() == 2
                && matches!(&actual_args[1], SemanticType::Name(name) if name == "...")
            {
                return Some(direct_type_is_assignable(
                    node,
                    nodes,
                    &render_semantic_type(&expected_args[0]),
                    &render_semantic_type(&actual_args[0]),
                ));
            }
            let element = if actual_head == "tuple" {
                lower_type_text_or_name(&join_branch_types(
                    actual_args.iter().map(render_semantic_type).collect(),
                ))
            } else {
                actual_args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| SemanticType::Name(String::from("dynamic")))
            };
            return Some(direct_type_is_assignable(
                node,
                nodes,
                &render_semantic_type(&expected_args[0]),
                &render_semantic_type(&element),
            ));
        }
        ("Mapping", "dict") if expected_args.len() == 2 && actual_args.len() == 2 => {
            return Some(
                invariant_type_matches(
                    node,
                    nodes,
                    &render_semantic_type(&expected_args[0]),
                    &render_semantic_type(&actual_args[0]),
                ) && direct_type_is_assignable(
                    node,
                    nodes,
                    &render_semantic_type(&expected_args[1]),
                    &render_semantic_type(&actual_args[1]),
                ),
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

fn same_head_semantic_generic_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    head: &str,
    expected_args: &[SemanticType],
    actual_args: &[SemanticType],
) -> Option<bool> {
    if head == "Callable" {
        let expected = expected_args.iter().map(render_semantic_type).collect::<Vec<_>>();
        let actual = actual_args.iter().map(render_semantic_type).collect::<Vec<_>>();
        return callable_annotation_assignable(node, nodes, &expected, &actual);
    }

    let variances = variances_for_generic_head(head, expected_args.len());
    Some(expected_args.iter().zip(actual_args.iter()).zip(variances).all(
        |((expected_arg, actual_arg), variance)| match variance {
            GenericVariance::Invariant => invariant_type_matches(
                node,
                nodes,
                &render_semantic_type(expected_arg),
                &render_semantic_type(actual_arg),
            ),
            GenericVariance::Covariant => direct_type_is_assignable(
                node,
                nodes,
                &render_semantic_type(expected_arg),
                &render_semantic_type(actual_arg),
            ),
            GenericVariance::Contravariant => direct_type_is_assignable(
                node,
                nodes,
                &render_semantic_type(actual_arg),
                &render_semantic_type(expected_arg),
            ),
        },
    ))
}

pub(crate) fn expanded_tuple_shape_args(args: &[String]) -> Vec<String> {
    expanded_tuple_shape_semantic_args(
        &args.iter().map(|arg| lower_type_text_or_name(arg)).collect::<Vec<_>>(),
    )
    .into_iter()
    .map(|arg| render_semantic_type(&arg))
    .collect()
}

pub(super) fn callable_annotation_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_args: &[String],
    actual_args: &[String],
) -> Option<bool> {
    let expected = lower_type_text_or_name(&format!("Callable[{}]", expected_args.join(", ")));
    let actual = lower_type_text_or_name(&format!("Callable[{}]", actual_args.join(", ")));
    match (&expected, &actual) {
        (
            SemanticType::Callable { params: expected_params, return_type: expected_return },
            SemanticType::Callable { params: actual_params, return_type: actual_return },
        ) => callable_semantic_annotation_assignable(
            node,
            nodes,
            expected_params,
            expected_return,
            actual_params,
            actual_return,
        ),
        _ => None,
    }
}

fn callable_semantic_annotation_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_params: &SemanticCallableParams,
    expected_return: &SemanticType,
    actual_params: &SemanticCallableParams,
    actual_return: &SemanticType,
) -> Option<bool> {
    if !direct_type_is_assignable(
        node,
        nodes,
        &render_semantic_type(expected_return),
        &render_semantic_type(actual_return),
    ) {
        return Some(false);
    }

    match (expected_params, actual_params) {
        (SemanticCallableParams::Ellipsis, _) | (_, SemanticCallableParams::Ellipsis) => Some(true),
        (SemanticCallableParams::ParamList(expected), SemanticCallableParams::ParamList(actual)) => {
            if expected.len() != actual.len() {
                return Some(false);
            }
            Some(expected.iter().zip(actual.iter()).all(|(expected_param, actual_param)| {
                direct_type_is_assignable(
                    node,
                    nodes,
                    &render_semantic_type(actual_param),
                    &render_semantic_type(expected_param),
                )
            }))
        }
        _ => {
            let expected = render_semantic_callable_params(expected_params);
            let actual = render_semantic_callable_params(actual_params);
            let expected = normalize_callable_param_expr(&expected);
            let actual = normalize_callable_param_expr(&actual);
            if expected == "..." || actual == "..." {
                return Some(true);
            }
            let expected = expected.strip_prefix('[')?.strip_suffix(']')?;
            let actual = actual.strip_prefix('[')?.strip_suffix(']')?;
            let expected = if expected.trim().is_empty() {
                Vec::new()
            } else {
                split_top_level_type_args(expected)
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            };
            let actual = if actual.trim().is_empty() {
                Vec::new()
            } else {
                split_top_level_type_args(actual)
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            };
            callable_annotation_assignable(node, nodes, &expected, &actual)
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
    let normalized = lower_type_text_or_name(text);
    let head = normalized
        .generic_parts()
        .map(|(head, _)| head.to_owned())
        .unwrap_or_else(|| render_semantic_type(&normalized));
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

fn expand_semantic_type_alias_once(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    ty: &SemanticType,
) -> Option<SemanticType> {
    let stripped = ty.strip_annotated();
    let (head, args) = stripped
        .generic_parts()
        .map(|(head, args)| (head.to_owned(), args.iter().map(render_semantic_type).collect()))
        .unwrap_or_else(|| (render_semantic_type(stripped), Vec::new()));
    let (alias_node, alias_decl) = resolve_direct_type_alias(nodes, node, &head)?;
    let substitutions = alias_type_param_substitutions(alias_decl, &args)?;
    let detail = rewrite_imported_typing_aliases(alias_node, &alias_decl.detail);
    let expanded = substitute_semantic_type_params(&lower_type_text_or_name(&detail), &substitutions);
    (expanded != *stripped).then_some(expanded)
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
    let ty = lower_type_text_or_name(text);
    semantic_type_mentions_alias(node, nodes, &ty, target, visiting)
}

fn semantic_type_mentions_alias(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    ty: &SemanticType,
    target: &str,
    visiting: &mut BTreeSet<(String, String)>,
) -> bool {
    let ty = ty.strip_annotated();

    if let Some(branches) = semantic_union_branches(ty) {
        return branches
            .into_iter()
            .any(|branch| semantic_type_mentions_alias(node, nodes, &branch, target, visiting));
    }

    if let Some((head, args)) = ty.generic_parts() {
        return head == target
            || type_alias_eventually_mentions(node, nodes, head, target, visiting)
            || args
                .iter()
                .any(|arg| semantic_type_mentions_alias(node, nodes, arg, target, visiting));
    }

    let normalized = render_semantic_type(ty);
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
    let ty = lower_type_text_or_name(text);
    semantic_enum_member_owner_name(&ty)
}

fn semantic_enum_member_owner_name(ty: &SemanticType) -> Option<String> {
    let SemanticType::Generic { head, args } = ty.strip_annotated() else {
        return None;
    };
    if head != "Literal" || args.len() != 1 {
        return None;
    }
    let SemanticType::Name(name) = &args[0] else {
        return None;
    };
    let (owner, _member) = name.rsplit_once('.')?;
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

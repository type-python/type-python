#[cfg(test)]
pub(super) fn direct_type_is_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    direct_type_is_assignable_with_options(
        node,
        nodes,
        expected,
        actual,
        AssignabilityOptions::default(),
    )
}

pub(super) fn direct_type_is_assignable_with_options(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
    options: AssignabilityOptions,
) -> bool {
    let mut types = TypeStore::default();
    let expected = types.intern(lower_type_text_or_name(expected));
    let actual = types.intern(lower_type_text_or_name(actual));
    TypeRelationContext::with_options(node, nodes, options).is_assignable(
        types.get(expected).expect("interned semantic expected type"),
        types.get(actual).expect("interned semantic actual type"),
    )
}

pub(super) fn semantic_type_matches(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
) -> bool {
    TypeRelationContext::new(node, nodes).matches(expected, actual)
}

pub(super) fn semantic_type_matches_with_options(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
    options: AssignabilityOptions,
) -> bool {
    TypeRelationContext::with_options(node, nodes, options).matches(expected, actual)
}

#[cfg(test)]
pub(super) fn semantic_type_is_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
) -> bool {
    semantic_type_is_assignable_with_options(
        node,
        nodes,
        expected,
        actual,
        AssignabilityOptions::default(),
    )
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct AssignabilityOptions {
    pub(super) strict_nulls: bool,
    pub(super) framework_adapters: bool,
    pub(super) taint: bool,
}

impl Default for AssignabilityOptions {
    fn default() -> Self {
        Self { strict_nulls: true, framework_adapters: false, taint: false }
    }
}

pub(super) fn semantic_type_is_assignable_with_options(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
    options: AssignabilityOptions,
) -> bool {
    TypeRelationContext::with_options(node, nodes, options).is_assignable(expected, actual)
}

pub(super) struct TypeRelationContext<'a> {
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    options: AssignabilityOptions,
}

impl<'a> TypeRelationContext<'a> {
    pub(super) fn new(
        node: &'a typepython_graph::ModuleNode,
        nodes: &'a [typepython_graph::ModuleNode],
    ) -> Self {
        Self::with_options(node, nodes, AssignabilityOptions::default())
    }

    pub(super) fn with_options(
        node: &'a typepython_graph::ModuleNode,
        nodes: &'a [typepython_graph::ModuleNode],
        options: AssignabilityOptions,
    ) -> Self {
        Self { node, nodes, options }
    }

    pub(super) fn matches(&self, expected: &SemanticType, actual: &SemanticType) -> bool {
        direct_semantic_type_matches(
            self.node,
            self.nodes,
            expected,
            actual,
            self.options,
            &mut BTreeSet::new(),
        )
    }

    pub(super) fn is_assignable(&self, expected: &SemanticType, actual: &SemanticType) -> bool {
        direct_semantic_type_is_assignable(
            self.node,
            self.nodes,
            expected,
            actual,
            self.options,
            &mut BTreeSet::new(),
        )
    }

    pub(super) fn invariant_matches(
        &self,
        expected: &SemanticType,
        actual: &SemanticType,
    ) -> bool {
        (self.matches(expected, actual) && self.matches(actual, expected))
            || recursive_type_alias_head_semantic(self.node, self.nodes, expected)
                .is_some_and(|_| self.is_assignable(expected, actual))
    }
}

fn semantic_invariant_type_matches_with_options(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
    options: AssignabilityOptions,
) -> bool {
    TypeRelationContext::with_options(node, nodes, options).invariant_matches(expected, actual)
}

fn direct_semantic_type_matches(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
    options: AssignabilityOptions,
    visiting: &mut BTreeSet<(SemanticType, SemanticType)>,
) -> bool {
    let expected = normalize_type_for_assignability_options(expected, options);
    let actual = normalize_type_for_assignability_options(actual, options);

    if expected == actual || is_any_semantic_type(&expected) || is_any_semantic_type(&actual) {
        return true;
    }

    let key = (expected.clone(), actual.clone());
    if !visiting.insert(key.clone()) {
        return true;
    }

    let result =
        if let Some(expanded_expected) = expand_semantic_type_alias_once(node, nodes, &expected) {
            direct_semantic_type_matches(
                node,
                nodes,
                &expanded_expected,
                &actual,
                options,
                visiting,
            )
        } else if let Some(expanded_actual) = expand_semantic_type_alias_once(node, nodes, &actual)
        {
            direct_semantic_type_matches(
                node,
                nodes,
                &expected,
                &expanded_actual,
                options,
                visiting,
            )
        } else if let Some(branches) = semantic_union_branches(&expected) {
            if let Some(actual_branches) = semantic_union_branches(&actual) {
                actual_branches.iter().all(|actual_branch| {
                    branches.iter().any(|expected_branch| {
                        direct_semantic_type_matches(
                            node,
                            nodes,
                            expected_branch,
                            actual_branch,
                            options,
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
                            options,
                            visiting,
                        )
                    })
                })
            } else {
                branches.into_iter().any(|branch| {
                    direct_semantic_type_matches(node, nodes, &branch, &actual, options, visiting)
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
                                    options,
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

fn direct_semantic_type_is_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
    options: AssignabilityOptions,
    visiting: &mut BTreeSet<(SemanticType, SemanticType)>,
) -> bool {
    let expected = normalize_type_for_assignability_options(expected, options);
    let actual = normalize_type_for_assignability_options(actual, options);

    if expected == actual {
        return true;
    }

    let actual_is_unknown = is_unknown_semantic_type(&actual);
    if actual_is_unknown
        && (is_unknown_semantic_type(&expected)
            || is_dynamic_semantic_type(&expected)
            || is_object_semantic_type(&expected))
    {
        return true;
    }
    if actual_is_unknown && is_any_semantic_type(&expected) {
        return false;
    }

    if !actual_is_unknown
        && (is_object_semantic_type(&expected)
            || is_top_receiving_semantic_type(&expected)
            || is_top_escaping_semantic_type(&actual))
    {
        return true;
    }

    if !options.strict_nulls
        && semantic_type_is_none(&actual)
        && semantic_type_accepts_implicit_none(&expected)
    {
        return true;
    }

    if options.taint
        && let Some(result) = taint_qualified_assignability(node, nodes, &expected, &actual, options)
    {
        return result;
    }

    let key = (expected.clone(), actual.clone());
    if !visiting.insert(key.clone()) {
        return true;
    }

    let expected_rendered = render_semantic_type(&expected);
    let actual_rendered = render_semantic_type(&actual);
    let result =
        if let Some(expanded_expected) = expand_semantic_type_alias_once(node, nodes, &expected) {
            direct_semantic_type_is_assignable(
                node,
                nodes,
                &expanded_expected,
                &actual,
                options,
                visiting,
            )
        } else if let Some(expanded_actual) = expand_semantic_type_alias_once(node, nodes, &actual)
        {
            direct_semantic_type_is_assignable(
                node,
                nodes,
                &expected,
                &expanded_actual,
                options,
                visiting,
            )
        } else if let Some(branches) = semantic_union_branches(&expected) {
            if let Some(actual_branches) = semantic_union_branches(&actual) {
                actual_branches.iter().all(|actual_branch| {
                    branches.iter().any(|expected_branch| {
                        direct_semantic_type_is_assignable(
                            node,
                            nodes,
                            expected_branch,
                            actual_branch,
                            options,
                            visiting,
                        )
                    })
                })
            } else {
                branches.into_iter().any(|branch| {
                    direct_semantic_type_is_assignable(
                        node, nodes, &branch, &actual, options, visiting,
                    )
                })
            }
        } else if let Some(actual_branches) = semantic_union_branches(&actual) {
            actual_branches.iter().all(|actual_branch| {
                direct_semantic_type_is_assignable(
                    node,
                    nodes,
                    &expected,
                    actual_branch,
                    options,
                    visiting,
                )
            })
        } else if let (Some((expected_params, expected_return)), Some((actual_params, actual_return))) =
            (expected.callable_parts(), actual.callable_parts())
        {
            callable_semantic_annotation_assignable(
                node,
                nodes,
                expected_params,
                expected_return,
                actual_params,
                actual_return,
                options,
            )
            .unwrap_or(false)
        } else {
            let enum_match =
                semantic_enum_member_owner_name(&actual).is_some_and(|owner| owner == expected_rendered);
            let protocol =
                protocol_assignable(node, nodes, &expected_rendered, &actual_rendered, options);
            let nominal =
                nominal_subclass_assignable(node, nodes, &expected_rendered, &actual_rendered, options);
            if enum_match || protocol || nominal {
                true
            } else if let Some(result) =
                assignable_semantic_generic_bridge(node, nodes, &expected, &actual, options)
            {
                result
            } else if actual_is_unknown {
                false
            } else {
                direct_semantic_type_matches(
                    node,
                    nodes,
                    &expected,
                    &actual,
                    options,
                    &mut BTreeSet::new(),
                )
            }
        };

    visiting.remove(&key);
    result
}

fn is_any_semantic_type(ty: &SemanticType) -> bool {
    matches!(ty, SemanticType::Name(name) if name == "Any")
}

fn is_top_receiving_semantic_type(ty: &SemanticType) -> bool {
    matches!(ty, SemanticType::Name(name) if matches!(name.as_str(), "Any" | "unknown" | "dynamic"))
}

fn is_top_escaping_semantic_type(ty: &SemanticType) -> bool {
    is_any_semantic_type(ty) || is_dynamic_semantic_type(ty)
}

fn is_dynamic_semantic_type(ty: &SemanticType) -> bool {
    matches!(ty, SemanticType::Name(name) if name == "dynamic")
}

fn is_unknown_semantic_type(ty: &SemanticType) -> bool {
    matches!(ty, SemanticType::Name(name) if name == "unknown")
}

fn is_object_semantic_type(ty: &SemanticType) -> bool {
    matches!(ty, SemanticType::Name(name) if name == "object")
}

fn taint_qualified_assignability(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
    options: AssignabilityOptions,
) -> Option<bool> {
    match (semantic_tainted_parts(expected), semantic_tainted_parts(actual)) {
        (None, None) => None,
        (None, Some(_)) | (Some(_), None) => Some(false),
        (Some((expected_inner, expected_context)), Some((actual_inner, actual_context))) => {
            Some(
                semantic_invariant_type_matches_with_options(
                    node,
                    nodes,
                    expected_context,
                    actual_context,
                    options,
                )
                    && direct_semantic_type_is_assignable(
                        node,
                        nodes,
                        expected_inner,
                        actual_inner,
                        options,
                        &mut BTreeSet::new(),
                    ),
            )
        }
    }
}

fn semantic_type_is_none(ty: &SemanticType) -> bool {
    matches!(ty.strip_annotated(), SemanticType::Name(name) if name == "None")
}

fn semantic_type_accepts_implicit_none(ty: &SemanticType) -> bool {
    let ty = ty.strip_annotated();
    if let Some(branches) = semantic_union_branches(ty) {
        return branches.iter().any(semantic_type_accepts_implicit_none);
    }
    if semantic_type_is_never(ty) || semantic_type_is_literal_domain(ty) {
        return false;
    }
    !semantic_type_is_none(ty)
}

fn semantic_type_is_never(ty: &SemanticType) -> bool {
    matches!(ty.strip_annotated(), SemanticType::Name(name) if name == "Never")
}

fn semantic_type_is_literal_domain(ty: &SemanticType) -> bool {
    matches!(ty.strip_annotated(), SemanticType::Generic { head, .. } if head == "Literal")
}

fn normalize_type_for_assignability_options(
    ty: &SemanticType,
    options: AssignabilityOptions,
) -> SemanticType {
    let ty = ty.strip_annotated();
    if options.taint { ty.clone() } else { erase_taint_qualifiers(ty) }
}

fn erase_taint_qualifiers(ty: &SemanticType) -> SemanticType {
    match ty {
        SemanticType::Name(_) => ty.clone(),
        SemanticType::Generic { head, args } if head == "Tainted" && args.len() == 2 => {
            erase_taint_qualifiers(&args[0])
        }
        SemanticType::Generic { head, args } => SemanticType::Generic {
            head: head.clone(),
            args: args.iter().map(erase_taint_qualifiers).collect(),
        },
        SemanticType::Callable { params, return_type } => SemanticType::Callable {
            params: erase_taint_qualifiers_from_callable_params(params),
            return_type: Box::new(erase_taint_qualifiers(return_type)),
        },
        SemanticType::Union(branches) => {
            SemanticType::Union(branches.iter().map(erase_taint_qualifiers).collect())
        }
        SemanticType::Annotated { value, metadata } => SemanticType::Annotated {
            value: Box::new(erase_taint_qualifiers(value)),
            metadata: metadata.clone(),
        },
        SemanticType::Unpack(inner) => SemanticType::Unpack(Box::new(erase_taint_qualifiers(inner))),
    }
}

fn erase_taint_qualifiers_from_callable_params(
    params: &SemanticCallableParams,
) -> SemanticCallableParams {
    match params {
        SemanticCallableParams::Ellipsis => SemanticCallableParams::Ellipsis,
        SemanticCallableParams::ParamList(params) => {
            SemanticCallableParams::ParamList(params.iter().map(erase_taint_qualifiers).collect())
        }
        SemanticCallableParams::Concatenate(params) => {
            SemanticCallableParams::Concatenate(params.iter().map(erase_taint_qualifiers).collect())
        }
        SemanticCallableParams::Single(param) => {
            SemanticCallableParams::Single(Box::new(erase_taint_qualifiers(param)))
        }
    }
}

fn semantic_tainted_parts(ty: &SemanticType) -> Option<(&SemanticType, &SemanticType)> {
    let (head, args) = ty.generic_parts()?;
    (head == "Tainted" && args.len() == 2).then(|| (&args[0], &args[1]))
}

pub(super) fn nominal_subclass_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
    options: AssignabilityOptions,
) -> bool {
    if expected == actual {
        return true;
    }
    let Some((actual_node, actual_decl)) = resolve_direct_base(nodes, node, actual) else {
        return false;
    };
    actual_decl.rendered_class_bases().iter().any(|base| {
        normalize_type_text(base) == expected
            || semantic_type_is_assignable_with_options(
                actual_node,
                nodes,
                &lower_type_text_or_name(expected),
                &lower_type_text_or_name(base),
                options,
            )
    })
}

pub(super) fn protocol_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
    options: AssignabilityOptions,
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
    type_satisfies_interface(
        nodes,
        actual_node,
        actual_decl,
        interface_node,
        interface_decl,
        options,
    )
}

pub(super) fn type_satisfies_interface(
    nodes: &[typepython_graph::ModuleNode],
    actual_node: &typepython_graph::ModuleNode,
    actual_decl: &Declaration,
    interface_node: &typepython_graph::ModuleNode,
    interface_decl: &Declaration,
    options: AssignabilityOptions,
) -> bool {
    collect_interface_members(interface_node, interface_decl, nodes).into_iter().all(|required| {
        actual_member_satisfies_requirement(nodes, actual_node, actual_decl, &required, options)
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

    for base in declaration.rendered_class_bases() {
        if let Some((base_node, base_decl)) = resolve_direct_base(nodes, node, &base)
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
    options: AssignabilityOptions,
) -> bool {
    match requirement.declaration.kind {
        DeclarationKind::Function => {
            if requirement.declaration.method_kind == Some(typepython_syntax::MethodKind::Property) {
                if let Some(member) =
                    find_apparent_callable_declaration(nodes, actual_node, actual_decl, &requirement.name)
                {
                    return methods_are_compatible_for_override(
                        actual_node,
                        nodes,
                        member,
                        &requirement.declaration,
                        options,
                    );
                }
                return find_apparent_value_declaration(
                    nodes,
                    actual_node,
                    actual_decl,
                    &requirement.name,
                )
                .is_some_and(|member| {
                    let expected = declaration_signature_return_semantic_type(&requirement.declaration);
                    let actual = declaration_effective_value_semantic_type(member);
                    expected.is_none()
                        || actual.is_none()
                        || semantic_type_is_assignable_with_options(
                            actual_node,
                            nodes,
                            expected.as_ref().expect("checked is_some"),
                            actual.as_ref().expect("checked is_some"),
                            options,
                        )
                });
            }
            find_apparent_callable_declaration(nodes, actual_node, actual_decl, &requirement.name)
                .is_some_and(|member| {
                    methods_are_compatible_for_override(
                        actual_node,
                        nodes,
                        member,
                        &requirement.declaration,
                        options,
                    )
                })
        }
        DeclarationKind::Value => {
            find_apparent_value_declaration(nodes, actual_node, actual_decl, &requirement.name)
                .is_some_and(|member| {
                    let expected = declaration_value_annotation_semantic_type(&requirement.declaration);
                    let actual = declaration_effective_value_semantic_type(member);
                    expected.is_none()
                        || actual.is_none()
                        || semantic_type_is_assignable_with_options(
                            actual_node,
                            nodes,
                            expected.as_ref().expect("checked is_some"),
                            actual.as_ref().expect("checked is_some"),
                            options,
                        )
                })
        }
        _ => false,
    }
}

fn declaration_effective_value_semantic_type(declaration: &Declaration) -> Option<SemanticType> {
    declaration_value_annotation_semantic_type(declaration)
        .or_else(|| declaration.inferred_value_type().map(|expr| lower_type_expr(expr.expr.clone())))
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

    for base in class_decl.rendered_class_bases() {
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, &base) else {
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
    options: AssignabilityOptions,
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
            options,
        );
    }

    if expected_head == actual_head && expected_args.len() == actual_args.len() {
        return same_head_semantic_generic_assignable(
            node,
            nodes,
            expected_head,
            expected_args,
            actual_args,
            options,
        );
    }

    match (expected_head, actual_head) {
        ("Sequence", "list") | ("Sequence", "tuple") if !expected_args.is_empty() => {
            if actual_head == "tuple"
                && actual_args.len() == 2
                && matches!(&actual_args[1], SemanticType::Name(name) if name == "...")
            {
                return Some(semantic_type_is_assignable_with_options(
                    node,
                    nodes,
                    &expected_args[0],
                    &actual_args[0],
                    options,
                ));
            }
            let element = if actual_head == "tuple" {
                join_semantic_type_candidates(actual_args.to_vec())
            } else {
                actual_args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| SemanticType::Name(String::from("dynamic")))
            };
            return Some(semantic_type_is_assignable_with_options(
                node,
                nodes,
                &expected_args[0],
                &element,
                options,
            ));
        }
        ("Mapping", "dict") if expected_args.len() == 2 && actual_args.len() == 2 => {
            return Some(
                semantic_invariant_type_matches_with_options(
                    node,
                    nodes,
                    &expected_args[0],
                    &actual_args[0],
                    options,
                )
                    && semantic_type_is_assignable_with_options(
                    node,
                    nodes,
                    &expected_args[1],
                    &actual_args[1],
                    options,
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
    options: AssignabilityOptions,
) -> Option<bool> {
    if head == "Callable" {
        return callable_semantic_annotation_assignable_from_generic_args(
            node,
            nodes,
            expected_args,
            actual_args,
            options,
        );
    }

    let variances = variances_for_generic_head(head, expected_args.len());
    Some(expected_args.iter().zip(actual_args.iter()).zip(variances).all(
        |((expected_arg, actual_arg), variance)| match variance {
            GenericVariance::Invariant => {
                semantic_invariant_type_matches_with_options(
                    node,
                    nodes,
                    expected_arg,
                    actual_arg,
                    options,
                )
            }
            GenericVariance::Covariant => semantic_type_is_assignable_with_options(
                node,
                nodes,
                expected_arg,
                actual_arg,
                options,
            ),
            GenericVariance::Contravariant => semantic_type_is_assignable_with_options(
                node,
                nodes,
                actual_arg,
                expected_arg,
                options,
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

fn callable_param_semantic_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &SemanticType,
    actual: &SemanticType,
    options: AssignabilityOptions,
) -> bool {
    semantic_type_is_assignable_with_options(node, nodes, actual, expected, options)
}

fn callable_structural_params_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_params: &SemanticCallableParams,
    actual_params: &SemanticCallableParams,
    options: AssignabilityOptions,
) -> Option<bool> {
    match (expected_params, actual_params) {
        (SemanticCallableParams::Ellipsis, _) | (_, SemanticCallableParams::Ellipsis) => Some(true),
        (SemanticCallableParams::ParamList(expected), SemanticCallableParams::ParamList(actual)) => {
            if expected.len() != actual.len() {
                return Some(false);
            }
            Some(
                expected
                    .iter()
                    .zip(actual.iter())
                    .all(|(expected_param, actual_param)| {
                        callable_param_semantic_assignable(
                            node,
                            nodes,
                            expected_param,
                            actual_param,
                            options,
                        )
                    }),
            )
        }
        (SemanticCallableParams::Single(expected), SemanticCallableParams::Single(actual)) => {
            Some(callable_param_semantic_assignable(node, nodes, expected, actual, options))
        }
        (
            SemanticCallableParams::Concatenate(expected),
            SemanticCallableParams::Concatenate(actual),
        ) => {
            let (expected_tail, expected_prefixes) = expected.split_last()?;
            let (actual_tail, actual_prefixes) = actual.split_last()?;
            if expected_prefixes.len() != actual_prefixes.len() {
                return Some(false);
            }
            Some(
                expected_prefixes
                    .iter()
                    .zip(actual_prefixes.iter())
                    .all(|(expected_param, actual_param)| {
                        callable_param_semantic_assignable(
                            node,
                            nodes,
                            expected_param,
                            actual_param,
                            options,
                        )
                    })
                    && callable_param_semantic_assignable(
                        node,
                        nodes,
                        expected_tail,
                        actual_tail,
                        options,
                    ),
            )
        }
        _ => None,
    }
}

fn callable_semantic_annotation_assignable_from_generic_args(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_args: &[SemanticType],
    actual_args: &[SemanticType],
    options: AssignabilityOptions,
) -> Option<bool> {
    let [expected_params, expected_return] = expected_args else {
        return None;
    };
    let [actual_params, actual_return] = actual_args else {
        return None;
    };
    let expected_params = match expected_params {
        SemanticType::Name(name) if name == "..." => SemanticCallableParams::Ellipsis,
        SemanticType::Generic { head, args } if head == "Concatenate" => {
            SemanticCallableParams::Concatenate(args.clone())
        }
        _ => SemanticCallableParams::Single(Box::new(expected_params.clone())),
    };
    let actual_params = match actual_params {
        SemanticType::Name(name) if name == "..." => SemanticCallableParams::Ellipsis,
        SemanticType::Generic { head, args } if head == "Concatenate" => {
            SemanticCallableParams::Concatenate(args.clone())
        }
        _ => SemanticCallableParams::Single(Box::new(actual_params.clone())),
    };
    callable_semantic_annotation_assignable(
        node,
        nodes,
        &expected_params,
        expected_return,
        &actual_params,
        actual_return,
        options,
    )
}

fn callable_semantic_annotation_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_params: &SemanticCallableParams,
    expected_return: &SemanticType,
    actual_params: &SemanticCallableParams,
    actual_return: &SemanticType,
    options: AssignabilityOptions,
) -> Option<bool> {
    if !semantic_type_is_assignable_with_options(
        node,
        nodes,
        expected_return,
        actual_return,
        options,
    ) {
        return Some(false);
    }

    callable_structural_params_assignable(node, nodes, expected_params, actual_params, options)
}

fn recursive_type_alias_head_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    ty: &SemanticType,
) -> Option<String> {
    let head = match ty.strip_annotated() {
        SemanticType::Name(name) => name.clone(),
        SemanticType::Generic { head, .. } => head.clone(),
        _ => return None,
    };
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
    let (head, args) = match stripped {
        SemanticType::Name(name) => (name.clone(), Vec::new()),
        SemanticType::Generic { head, args } => (head.clone(), args.clone()),
        _ => return None,
    };
    let (alias_node, alias_decl) = resolve_direct_type_alias(nodes, node, &head)?;
    let substitutions = alias_type_param_substitutions_semantic(alias_decl, &args)?;
    let alias = declaration_type_alias_semantics(alias_decl)?;
    let expanded = substitute_semantic_type_params(
        &rewrite_imported_typing_semantic_type(alias_node, &alias.body),
        &substitutions,
    );
    if expanded == *stripped {
        return None;
    }
    let expanded = expand_semantic_type_aliases_in_provider_context(alias_node, nodes, expanded);
    (expanded != *stripped).then_some(expanded)
}

fn expand_semantic_type_aliases_in_provider_context(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    mut ty: SemanticType,
) -> SemanticType {
    let mut seen = BTreeSet::new();
    while seen.insert(ty.clone()) {
        let Some(next) = expand_semantic_type_alias_once(node, nodes, &ty) else {
            break;
        };
        ty = next;
    }
    ty
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

    let alias = match declaration_type_alias_semantics(alias_decl) {
        Some(alias) => alias,
        None => return false,
    };
    let result = semantic_type_mentions_alias(alias_node, nodes, &alias.body, target, visiting);
    visiting.remove(&key);
    result
}

#[allow(dead_code)]
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

    match ty {
        SemanticType::Name(name) => {
            name == target || type_alias_eventually_mentions(node, nodes, name, target, visiting)
        }
        _ => false,
    }
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

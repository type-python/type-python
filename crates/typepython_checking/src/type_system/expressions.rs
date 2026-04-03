#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_direct_boolop_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    left: Option<&typepython_syntax::DirectExprMetadata>,
    right: Option<&typepython_syntax::DirectExprMetadata>,
    guard: Option<&typepython_binding::GuardConditionSite>,
    operator: Option<&str>,
) -> Option<SemanticType> {
    let operator = operator?;
    if operator != "and" && operator != "or" {
        return None;
    }
    let left_type = resolve_direct_expression_semantic_type_from_metadata(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        left?,
    )?;
    let right_type = if let Some(guard) = guard {
        let base_bindings = resolve_guard_scope_semantic_bindings(
            node,
            nodes,
            signature,
            None,
            current_owner_name,
            current_owner_type_name,
            current_line,
            guard,
        );
        let narrowed_bindings = apply_guard_to_local_semantic_bindings(
            node,
            nodes,
            &base_bindings,
            guard,
            operator == "and",
        );
        resolve_direct_expression_semantic_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            right?,
            &narrowed_bindings,
        )?
    } else {
        resolve_direct_expression_semantic_type_from_metadata(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            right?,
        )?
    };
    Some(join_semantic_type_candidates(vec![left_type, right_type]))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_direct_binop_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    left: Option<&typepython_syntax::DirectExprMetadata>,
    right: Option<&typepython_syntax::DirectExprMetadata>,
    operator: Option<&str>,
) -> Option<SemanticType> {
    let operator = operator?;
    let left_type = resolve_direct_expression_semantic_type_from_metadata(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        left?,
    )?;
    let right_type = resolve_direct_expression_semantic_type_from_metadata(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        right?,
    )?;
    resolve_binop_result_semantic_type(&left_type, &right_type, operator)
}

pub(super) fn resolve_plus_result_semantic_type(
    left: &SemanticType,
    right: &SemanticType,
) -> Option<SemanticType> {
    if matches!(left.strip_annotated(), SemanticType::Name(name) if name == "str")
        && matches!(right.strip_annotated(), SemanticType::Name(name) if name == "str")
    {
        return Some(SemanticType::Name(String::from("str")));
    }
    if is_numeric_semantic_type(left) && is_numeric_semantic_type(right) {
        return Some(join_numeric_result_semantic_type(left, right));
    }
    let (left_head, left_args) = left.generic_parts()?;
    let (right_head, right_args) = right.generic_parts()?;
    match (left_head, right_head) {
        ("list", "list") if left_args.len() == 1 && right_args.len() == 1 => {
            Some(SemanticType::Generic {
                head: String::from("list"),
                args: vec![join_semantic_type_candidates(vec![
                    left_args[0].clone(),
                    right_args[0].clone(),
                ])],
            })
        }
        ("tuple", "tuple") => {
            let mut args = left_args.to_vec();
            args.extend(right_args.iter().cloned());
            Some(SemanticType::Generic { head: String::from("tuple"), args })
        }
        _ => None,
    }
}

pub(super) fn resolve_binop_result_semantic_type(
    left: &SemanticType,
    right: &SemanticType,
    operator: &str,
) -> Option<SemanticType> {
    match operator.trim() {
        "+" => resolve_plus_result_semantic_type(left, right),
        "-" | "*" | "/" | "//" | "%" if is_numeric_semantic_type(left) && is_numeric_semantic_type(right) => {
            Some(join_numeric_result_semantic_type(left, right))
        }
        _ => None,
    }
}

pub(super) fn is_numeric_semantic_type(ty: &SemanticType) -> bool {
    matches!(
        ty.strip_annotated(),
        SemanticType::Name(name) if matches!(name.as_str(), "int" | "float" | "complex")
    )
}

pub(super) fn join_numeric_result_semantic_type(
    left: &SemanticType,
    right: &SemanticType,
) -> SemanticType {
    let left = render_semantic_type(left);
    let right = render_semantic_type(right);
    if left == "complex" || right == "complex" {
        SemanticType::Name(String::from("complex"))
    } else if left == "float" || right == "float" || left == "/" || right == "/" {
        SemanticType::Name(String::from("float"))
    } else {
        SemanticType::Name(String::from("int"))
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "subscript resolution needs the same expression context as other direct expression forms"
)]
pub(super) fn resolve_direct_subscript_reference_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    _exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    target: &typepython_syntax::DirectExprMetadata,
    string_key: Option<&str>,
    index_text: Option<&str>,
) -> Option<SemanticType> {
    let target_type = resolve_direct_expression_semantic_type_from_metadata(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        target,
    )?;
    resolve_subscript_type_from_target_semantic_type(node, nodes, &target_type, string_key, index_text)
}

pub(super) fn resolve_subscript_type_from_target_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    target_type: &SemanticType,
    string_key: Option<&str>,
    index_text: Option<&str>,
) -> Option<SemanticType> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    resolve_subscript_type_from_target_semantic_type_with_context(
        &context,
        node,
        nodes,
        target_type,
        string_key,
        index_text,
    )
}

pub(super) fn resolve_subscript_type_from_target_semantic_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    target_type: &SemanticType,
    string_key: Option<&str>,
    index_text: Option<&str>,
) -> Option<SemanticType> {
    if let Some(key) = string_key
        && let Some(shape) = resolve_known_typed_dict_shape_from_type_with_context(
            context,
            node,
            nodes,
            &render_semantic_type(target_type),
        )
    {
        return typed_dict_known_or_extra_field(&shape, key)
            .map(|field| lower_type_text_or_name(field.value_type()));
    }

    if let Some((head, args)) = target_type.generic_parts() {
        let expanded_args = expanded_tuple_shape_semantic_args(args);
        return match head {
            "dict" | "Mapping" | "typing.Mapping" | "collections.abc.Mapping"
                if args.len() == 2 =>
            {
                Some(args[1].clone())
            }
            "list" | "Sequence" | "typing.Sequence" | "collections.abc.Sequence"
                if !args.is_empty() =>
            {
                Some(args[0].clone())
            }
            "tuple" if !expanded_args.is_empty() => {
                if args.len() == 2
                    && matches!(&args[1], SemanticType::Name(name) if name == "...")
                {
                    return Some(args[0].clone());
                }
                index_text
                    .and_then(|index| index.parse::<usize>().ok())
                    .and_then(|index| expanded_args.get(index).cloned())
                    .or_else(|| Some(join_semantic_type_candidates(expanded_args)))
            }
            _ => resolve_nominal_getitem_return_semantic_type(node, nodes, target_type),
        };
    }

    resolve_nominal_getitem_return_semantic_type(node, nodes, target_type)
}

pub(super) fn resolve_nominal_getitem_return_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    owner_type: &SemanticType,
) -> Option<SemanticType> {
    let owner_type_name = semantic_nominal_owner_name(owner_type)?;
    let nominal_owner_name = owner_type
        .generic_parts()
        .map(|(head, _)| head.to_owned())
        .unwrap_or_else(|| owner_type_name.clone());
    let (class_node, class_decl) = resolve_direct_base(nodes, node, &nominal_owner_name)?;
    let getitem = find_owned_callable_declaration(nodes, class_node, class_decl, "__getitem__")?;
    Some(rewrite_imported_typing_semantic_type(
        node,
        &declaration_signature_return_semantic_type_with_self(getitem, &owner_type_name)?,
    ))
}

fn semantic_type_from_scope_param(
    param: &SemanticCallableParam,
) -> SemanticType {
    if param.variadic {
        if let Some(annotation) = param.annotation.as_ref() {
            if matches!(annotation.strip_annotated(), SemanticType::Unpack(_)) {
                SemanticType::Generic {
                    head: String::from("tuple"),
                    args: vec![annotation.clone()],
                }
            } else {
                SemanticType::Generic {
                    head: String::from("tuple"),
                    args: vec![annotation.clone(), SemanticType::Name(String::from("..."))],
                }
            }
        } else {
            SemanticType::Generic {
                head: String::from("tuple"),
                args: vec![
                    SemanticType::Name(String::from("dynamic")),
                    SemanticType::Name(String::from("...")),
                ],
            }
        }
    } else if param.keyword_variadic {
        if let Some(annotation) = param.annotation.as_ref() {
            SemanticType::Generic {
                head: String::from("dict"),
                args: vec![SemanticType::Name(String::from("str")), annotation.clone()],
            }
        } else {
            SemanticType::Generic {
                head: String::from("dict"),
                args: vec![
                    SemanticType::Name(String::from("str")),
                    SemanticType::Name(String::from("dynamic")),
                ],
            }
        }
    } else {
        param.annotation_or_dynamic()
    }
}

pub(super) fn resolve_scope_param_semantic_type(
    node: &typepython_graph::ModuleNode,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    value_name: &str,
) -> Option<SemanticType> {
    let declaration = resolve_scope_owner_declaration(node, owner_name, owner_type_name)?;
    let callable = declaration_callable_semantics(declaration)?;
    let params = owner_type_name
        .map(|owner_type_name| callable_semantic_params_with_self_from_semantics(&callable, owner_type_name))
        .unwrap_or_else(|| callable_semantic_params_from_semantics(&callable));
    params
        .iter()
        .find(|param| param.name == value_name)
        .map(semantic_type_from_scope_param)
}

#[expect(
    clippy::too_many_arguments,
    reason = "semantic expression resolution mirrors the direct expression metadata shape"
)]
pub(super) fn resolve_direct_expression_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_type: Option<&str>,
    is_awaited: bool,
    value_callee: Option<&str>,
    value_name: Option<&str>,
    value_member_owner_name: Option<&str>,
    value_member_name: Option<&str>,
    value_member_through_instance: bool,
    value_method_owner_name: Option<&str>,
    value_method_name: Option<&str>,
    value_method_through_instance: bool,
    value_subscript_target: Option<&typepython_syntax::DirectExprMetadata>,
    value_subscript_string_key: Option<&str>,
    value_subscript_index: Option<&str>,
    value_if_true: Option<&typepython_syntax::DirectExprMetadata>,
    value_if_false: Option<&typepython_syntax::DirectExprMetadata>,
    value_if_guard: Option<&typepython_binding::GuardConditionSite>,
    value_bool_left: Option<&typepython_syntax::DirectExprMetadata>,
    value_bool_right: Option<&typepython_syntax::DirectExprMetadata>,
    value_binop_left: Option<&typepython_syntax::DirectExprMetadata>,
    value_binop_right: Option<&typepython_syntax::DirectExprMetadata>,
    value_binop_operator: Option<&str>,
) -> Option<SemanticType> {
    let resolved = value_type
        .filter(|value_type| !value_type.is_empty())
        .map(str::trim)
        .map(lower_type_text_or_name)
        .or_else(|| {
            value_callee.and_then(|callee| {
                resolve_direct_callable_return_semantic_type_for_line(
                    node,
                    nodes,
                    callee,
                    current_line,
                )
                .or_else(|| resolve_direct_callable_return_semantic_type(node, nodes, callee))
            })
        })
        .or_else(|| {
            value_name.and_then(|value_name| {
                resolve_direct_name_reference_semantic_type(
                    node,
                    nodes,
                    signature,
                    exclude_name,
                    current_owner_name,
                    current_owner_type_name,
                    current_line,
                    value_name,
                )
            })
        })
        .or_else(|| {
            value_method_owner_name.and_then(|owner_name| {
                value_method_name.and_then(|method_name| {
                    resolve_direct_method_return_semantic_type(
                        node,
                        nodes,
                        signature,
                        exclude_name,
                        current_owner_name,
                        current_owner_type_name,
                        current_line,
                        owner_name,
                        method_name,
                        value_method_through_instance,
                    )
                })
            })
        })
        .or_else(|| {
            value_member_owner_name.and_then(|owner_name| {
                value_member_name.and_then(|member_name| {
                    resolve_direct_member_reference_semantic_type(
                        node,
                        nodes,
                        signature,
                        exclude_name,
                        current_owner_name,
                        current_owner_type_name,
                        current_line,
                        owner_name,
                        member_name,
                        value_member_through_instance,
                    )
                })
            })
        })
        .or_else(|| {
            value_subscript_target.and_then(|target| {
                resolve_direct_subscript_reference_semantic_type(
                    node,
                    nodes,
                    signature,
                    exclude_name,
                    current_owner_name,
                    current_owner_type_name,
                    current_line,
                    target,
                    value_subscript_string_key,
                    value_subscript_index,
                )
            })
        })
        .or_else(|| {
            let true_branch = value_if_true?;
            let false_branch = value_if_false?;
            if let Some(guard) = value_if_guard {
                let base_bindings = resolve_guard_scope_semantic_bindings(
                    node,
                    nodes,
                    signature,
                    exclude_name,
                    current_owner_name,
                    current_owner_type_name,
                    current_line,
                    guard,
                );
                let true_bindings =
                    apply_guard_to_local_semantic_bindings(node, nodes, &base_bindings, guard, true);
                let false_bindings =
                    apply_guard_to_local_semantic_bindings(node, nodes, &base_bindings, guard, false);
                return Some(join_semantic_type_candidates(vec![
                    resolve_direct_expression_semantic_type_from_metadata_with_bindings(
                        node,
                        nodes,
                        signature,
                        current_owner_name,
                        current_owner_type_name,
                        current_line,
                        true_branch,
                        &true_bindings,
                    )?,
                    resolve_direct_expression_semantic_type_from_metadata_with_bindings(
                        node,
                        nodes,
                        signature,
                        current_owner_name,
                        current_owner_type_name,
                        current_line,
                        false_branch,
                        &false_bindings,
                    )?,
                ]));
            }
            Some(join_semantic_type_candidates(vec![
                resolve_direct_expression_semantic_type_from_metadata(
                    node,
                    nodes,
                    signature,
                    current_owner_name,
                    current_owner_type_name,
                    current_line,
                    true_branch,
                )?,
                resolve_direct_expression_semantic_type_from_metadata(
                    node,
                    nodes,
                    signature,
                    current_owner_name,
                    current_owner_type_name,
                    current_line,
                    false_branch,
                )?,
            ]))
        })
        .or_else(|| {
            resolve_direct_boolop_semantic_type(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                value_bool_left,
                value_bool_right,
                value_if_guard,
                value_binop_operator,
            )
        })
        .or_else(|| {
            resolve_direct_binop_semantic_type(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                value_binop_left,
                value_binop_right,
                value_binop_operator,
            )
        })?;

    if is_awaited {
        unwrap_awaitable_semantic_type(&resolved)
    } else {
        Some(resolved)
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "name reference resolution needs scope and source-position context"
)]
pub(super) fn resolve_direct_name_reference_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<SemanticType> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    resolve_direct_name_reference_semantic_type_with_context(
        &context,
        node,
        nodes,
        signature,
        exclude_name,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "name reference resolution needs scope and source-position context"
)]
pub(super) fn resolve_direct_name_reference_semantic_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<SemanticType> {
    if let Some(receiver_type) = resolve_receiver_name_semantic_type(
        node,
        current_owner_name,
        current_owner_type_name,
        value_name,
    ) {
        return Some(receiver_type);
    }
    let signature =
        signature.map(|signature| substitute_self_annotation(signature, current_owner_type_name));
    let base_type = resolve_unnarrowed_name_reference_semantic_type_with_context(
        context,
        node,
        nodes,
        signature.as_deref(),
        exclude_name,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    )?;
    let narrowed = apply_guard_narrowing_semantic(
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
        &base_type,
    );
    Some(narrowed)
}

pub(super) fn resolve_receiver_name_semantic_type(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    value_name: &str,
) -> Option<SemanticType> {
    let owner_type_name = current_owner_type_name?;
    let owner_name = current_owner_name?;
    let declaration = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Function
            && declaration.name == owner_name
            && declaration.owner.as_ref().is_some_and(|owner| owner.name == owner_type_name)
    })?;

    match (declaration.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance), value_name) {
        (typepython_syntax::MethodKind::Instance, "self")
        | (typepython_syntax::MethodKind::Property, "self")
        | (typepython_syntax::MethodKind::PropertySetter, "self") => {
            Some(SemanticType::Name(String::from(owner_type_name)))
        }
        (typepython_syntax::MethodKind::Class, "cls") => Some(SemanticType::Generic {
            head: String::from("type"),
            args: vec![SemanticType::Name(String::from(owner_type_name))],
        }),
        _ => None,
    }
}

pub(super) fn find_member_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
    predicate: impl Fn(&Declaration) -> bool + Copy,
) -> Option<&'a Declaration> {
    let mut visited = BTreeSet::new();
    find_member_declaration_with_visited(
        nodes,
        class_node,
        class_decl,
        member_name,
        predicate,
        &mut visited,
    )
}

pub(super) fn find_member_declaration_with_visited<'a>(
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

    if let Some(member) = class_node.declarations.iter().find(|declaration| {
        declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration.name == member_name
            && predicate(declaration)
    }) {
        return Some(member);
    }

    for base in &class_decl.bases {
        if let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) {
            if let Some(member) = find_member_declaration_with_visited(
                nodes,
                base_node,
                base_decl,
                member_name,
                predicate,
                visited,
            ) {
                return Some(member);
            }
        }
    }

    None
}

pub(super) fn find_owned_value_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<&'a Declaration> {
    find_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
        declaration.kind == DeclarationKind::Value
    })
}

pub(super) fn find_owned_readable_member_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<&'a Declaration> {
    find_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
        declaration.kind == DeclarationKind::Value
            || (declaration.kind == DeclarationKind::Function
                && declaration.method_kind == Some(typepython_syntax::MethodKind::Property))
    })
}

pub(super) fn resolve_readable_member_semantic_type(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    owner_type: &SemanticType,
) -> Option<SemanticType> {
    let owner_type_name = semantic_nominal_owner_name(owner_type)?;
    match declaration.kind {
        DeclarationKind::Value => {
            let detail = rewrite_imported_typing_aliases(
                node,
                &substitute_self_annotation(
                    &declaration_value_annotation_text(declaration)?,
                    Some(&owner_type_name),
                ),
            );
            normalized_direct_return_annotation(&detail).map(lower_type_text_or_name).or_else(|| {
                declaration.value_type.as_deref().map(|value| {
                    lower_type_text_or_name(&rewrite_imported_typing_aliases(
                        node,
                        &substitute_self_annotation(value, Some(&owner_type_name)),
                    ))
                })
            })
        }
        DeclarationKind::Function
            if declaration.method_kind == Some(typepython_syntax::MethodKind::Property) =>
        {
            Some(rewrite_imported_typing_semantic_type(
                node,
                &declaration_signature_return_semantic_type_with_self(declaration, &owner_type_name)?,
            ))
        }
        _ => None,
    }
}

pub(super) fn resolve_member_access_owner_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    access: &typepython_binding::MemberAccessSite,
) -> Option<SemanticType> {
    if access.through_instance {
        resolve_direct_callable_return_semantic_type(node, nodes, &access.owner_name)
            .or_else(|| Some(SemanticType::Name(access.owner_name.clone())))
    } else {
        resolve_direct_name_reference_semantic_type(
            node,
            nodes,
            None,
            None,
            access.current_owner_name.as_deref(),
            access.current_owner_type_name.as_deref(),
            access.line,
            &access.owner_name,
        )
        .or_else(|| Some(SemanticType::Name(access.owner_name.clone())))
    }
}

pub(super) fn find_owned_callable_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<&'a Declaration> {
    find_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
        matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
    })
}

pub(super) fn find_owned_callable_declarations<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Vec<&'a Declaration> {
    let mut visited = BTreeSet::new();
    find_owned_callable_declarations_with_visited(
        nodes,
        class_node,
        class_decl,
        member_name,
        &mut visited,
    )
}

pub(super) fn find_owned_callable_declarations_with_visited<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
    visited: &mut BTreeSet<(String, String)>,
) -> Vec<&'a Declaration> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visited.insert(key) {
        return Vec::new();
    }

    let local = class_node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
                && declaration.name == member_name
                && matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })
        .collect::<Vec<_>>();
    if !local.is_empty() {
        return local;
    }

    for base in &class_decl.bases {
        if let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) {
            let inherited = find_owned_callable_declarations_with_visited(
                nodes,
                base_node,
                base_decl,
                member_name,
                visited,
            );
            if !inherited.is_empty() {
                return inherited;
            }
        }
    }

    Vec::new()
}

#[expect(
    clippy::too_many_arguments,
    reason = "unnarrowed name resolution needs scope and source-position context"
)]
pub(super) fn resolve_unnarrowed_name_reference_semantic_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<SemanticType> {
    if let Some(param_type) =
        resolve_scope_param_semantic_type(node, current_owner_name, current_owner_type_name, value_name)
    {
        return Some(param_type);
    }

    if exclude_name.is_some_and(|name| name == value_name) {
        return None;
    }

    if let Some(exception_type) = resolve_exception_binding_semantic_type(
        node,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    ) {
        return Some(exception_type);
    }

    if let Some(loop_type) = resolve_for_loop_target_semantic_type(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    ) {
        return Some(loop_type);
    }

    if let Some(with_type) = resolve_with_target_name_semantic_type(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    ) {
        return Some(with_type);
    }

    if let Some(local_type) = resolve_local_assignment_reference_semantic_type(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    ) {
        return Some(local_type);
    }

    if current_owner_name.is_none() {
        if let Some(module_type) = resolve_module_level_assignment_reference_semantic_type(
            node,
            nodes,
            signature,
            current_line,
            value_name,
        ) {
            return Some(module_type);
        }
    }

    if let Some(local_value) = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Value
            && declaration.owner.is_none()
            && declaration.name == value_name
            && declaration_value_annotation_text(declaration).is_some()
    }) {
        let detail = rewrite_imported_typing_aliases(
            node,
            &substitute_self_annotation(
                &declaration_value_annotation_text(local_value)?,
                current_owner_type_name,
            ),
        );
        return normalized_direct_return_annotation(&detail).map(lower_type_text_or_name);
    }

    if let Some((provider_node, function)) = resolve_direct_function_with_node(node, nodes, value_name) {
        if let Some(callable_type) = resolve_decorated_function_callable_semantic_type_with_context(
            context,
            node,
            nodes,
            value_name,
        )
        {
            return Some(callable_type);
        }
        let param_types = if let Some(params) = declaration_semantic_signature_params(function) {
            params.into_iter().map(|param| param.annotation_or_dynamic()).collect::<Vec<_>>()
        } else {
            context
                .load_direct_function_signatures(provider_node)
                .get(&function.name)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|param| param_annotation_semantic_type(&param))
                .collect::<Vec<_>>()
        };
        let return_type = resolve_direct_callable_return_semantic_type(node, nodes, value_name)?;
        return Some(SemanticType::Callable {
            params: SemanticCallableParams::ParamList(param_types),
            return_type: Box::new(return_type),
        });
    }

    if let Some((_, class_decl)) = resolve_direct_base(nodes, node, value_name) {
        return Some(SemanticType::Name(class_decl.name.clone()));
    }

    if let Some(boundary_type) =
        unresolved_import_boundary_type_with_context(context, node, nodes, value_name)
    {
        return Some(SemanticType::Name(String::from(boundary_type)));
    }

    if let Some((head, _)) = value_name.split_once('.')
        && let Some(boundary_type) =
            unresolved_import_boundary_type_with_context(context, node, nodes, head)
    {
        return Some(SemanticType::Name(String::from(boundary_type)));
    }

    None
}

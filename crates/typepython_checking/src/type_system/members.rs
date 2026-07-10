fn scoped_type_param_bound_without_provenance(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    owner_type: &SemanticType,
) -> Option<SemanticType> {
    let SemanticType::Name(type_param_name) = owner_type.strip_annotated() else {
        return None;
    };
    let callable_type_param = resolve_scope_owner_declaration(
        node,
        current_owner_name,
        current_owner_type_name,
    )
    .and_then(|declaration| {
        declaration
            .type_params
            .iter()
            .find(|type_param| type_param.name == *type_param_name)
    });
    let class_type_param = current_owner_type_name.and_then(|owner_type_name| {
        node.declarations
            .iter()
            .find(|declaration| {
                declaration.kind == DeclarationKind::Class
                    && declaration.owner.is_none()
                    && declaration.name == owner_type_name
            })
            .and_then(|declaration| {
                declaration
                    .type_params
                    .iter()
                    .find(|type_param| type_param.name == *type_param_name)
            })
    });
    let type_param = callable_type_param.or(class_type_param)?;
    if type_param.kind != typepython_binding::GenericTypeParamKind::TypeVar {
        return None;
    }
    type_param
        .bound_expr
        .as_ref()
        .map(|bound| lower_type_expr(bound.expr.clone()))
}

#[expect(
    clippy::too_many_arguments,
    reason = "bound lookup needs graph, lexical scope, source receiver, and resolved type"
)]
pub(super) fn scoped_type_param_bound_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    receiver_name: &str,
    through_instance: bool,
    owner_type: &SemanticType,
    options: AssignabilityOptions,
) -> Option<SemanticType> {
    let SemanticType::Name(type_param_name) = owner_type.strip_annotated() else {
        return None;
    };
    let bound = scoped_type_param_bound_without_provenance(
        node,
        current_owner_name,
        current_owner_type_name,
        owner_type,
    )?;
    let has_scoped_provenance = if through_instance {
        scoped_callable_returns_type_param(
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            receiver_name,
            type_param_name,
        )
    } else {
        scoped_value_has_type_param_provenance(
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            current_line,
            receiver_name,
            type_param_name,
            options,
            &mut BTreeSet::new(),
        )
    };
    if !has_scoped_provenance {
        return None;
    }
    Some(bound)
}

#[expect(
    clippy::too_many_arguments,
    reason = "bound lookup needs graph, expression provenance, lexical scope, and checker options"
)]
pub(super) fn scoped_expression_type_param_bound_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    owner_type: &SemanticType,
    options: AssignabilityOptions,
) -> Option<SemanticType> {
    let SemanticType::Name(type_param_name) = owner_type.strip_annotated() else {
        return None;
    };
    let bound = scoped_type_param_bound_without_provenance(
        node,
        current_owner_name,
        current_owner_type_name,
        owner_type,
    )?;
    scoped_metadata_has_type_param_provenance(
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        current_line,
        metadata,
        type_param_name,
        options,
        &mut BTreeSet::new(),
    )
    .then_some(bound)
}

fn scoped_callable_returns_type_param(
    node: &typepython_graph::ModuleNode,
    _nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    callable_name: &str,
    type_param_name: &str,
) -> bool {
    resolve_scope_param_semantic_type(
        node,
        current_owner_name,
        current_owner_type_name,
        callable_name,
    )
    .and_then(|callable| callable.callable_parts().map(|(_, returns)| returns.clone()))
    .is_some_and(|returns| {
        matches!(returns.strip_annotated(), SemanticType::Name(name) if name == type_param_name)
    })
}

fn semantic_type_mentions_name(ty: &SemanticType, expected_name: &str) -> bool {
    match ty.strip_annotated() {
        SemanticType::Name(name) => name == expected_name,
        SemanticType::Generic { args, .. } | SemanticType::Union(args) => {
            args.iter().any(|arg| semantic_type_mentions_name(arg, expected_name))
        }
        SemanticType::Callable { params, return_type } => {
            let params_mention = match params {
                SemanticCallableParams::Ellipsis => false,
                SemanticCallableParams::ParamList(types)
                | SemanticCallableParams::Concatenate(types) => types
                    .iter()
                    .any(|ty| semantic_type_mentions_name(ty, expected_name)),
                SemanticCallableParams::Single(ty) => {
                    semantic_type_mentions_name(ty, expected_name)
                }
            };
            params_mention || semantic_type_mentions_name(return_type, expected_name)
        }
        SemanticType::Unpack(inner) => semantic_type_mentions_name(inner, expected_name),
        SemanticType::Annotated { .. } => unreachable!("annotations were stripped"),
    }
}

fn scoped_bound_method_returns_self(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    type_param_name: &str,
    method_name: &str,
    options: AssignabilityOptions,
) -> bool {
    let owner_type = SemanticType::Name(type_param_name.to_owned());
    let Some(bound_type) = scoped_type_param_bound_without_provenance(
        node,
        current_owner_name,
        current_owner_type_name,
        &owner_type,
    ) else {
        return false;
    };
    method_returns_self_on_owner_type(node, nodes, &bound_type, method_name, options)
}

fn method_returns_self_on_owner_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    owner_type: &SemanticType,
    method_name: &str,
    options: AssignabilityOptions,
) -> bool {
    if let Some(branches) = semantic_member_union_branches(owner_type, options.strict_nulls) {
        return !branches.is_empty()
            && branches.iter().all(|branch| {
                method_returns_self_on_owner_type(node, nodes, branch, method_name, options)
            });
    }
    let Some(owner_name) = semantic_nominal_owner_name(owner_type) else {
        return false;
    };
    let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &owner_name) else {
        return false;
    };
    let methods = find_owned_callable_declarations(nodes, class_node, class_decl, method_name);
    !methods.is_empty()
        && methods.iter().all(|method| {
            declaration_signature_return_semantic_type(method).is_some_and(|return_type| {
                matches!(return_type.strip_annotated(), SemanticType::Name(name) if name == "Self")
            })
        })
}

#[expect(
    clippy::too_many_arguments,
    reason = "expression provenance needs graph, lexical scope, source position, and cycle state"
)]
fn scoped_metadata_has_type_param_provenance(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    type_param_name: &str,
    options: AssignabilityOptions,
    visiting: &mut BTreeSet<String>,
) -> bool {
    if metadata.value_type_expr.as_ref().is_some_and(|value_type| {
        semantic_type_mentions_name(&lower_type_expr(value_type.clone()), type_param_name)
    }) {
        return true;
    }
    if let Some(value_name) = metadata.value_name.as_deref() {
        return scoped_value_has_type_param_provenance(
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            current_line,
            value_name,
            type_param_name,
            options,
            visiting,
        );
    }
    if let Some(callee) = metadata.value_callee.as_deref() {
        return scoped_callable_returns_type_param(
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            callee,
            type_param_name,
        );
    }
    if let (Some(receiver_name), Some(method_name)) = (
        metadata.value_method_owner_name.as_deref(),
        metadata.value_method_name.as_deref(),
    ) {
        let receiver_has_provenance = if metadata.value_method_through_instance {
            scoped_callable_returns_type_param(
                node,
                nodes,
                current_owner_name,
                current_owner_type_name,
                receiver_name,
                type_param_name,
            )
        } else {
            scoped_value_has_type_param_provenance(
                node,
                nodes,
                current_owner_name,
                current_owner_type_name,
                current_line,
                receiver_name,
                type_param_name,
                options,
                visiting,
            )
        };
        return receiver_has_provenance
            && scoped_bound_method_returns_self(
                node,
                nodes,
                current_owner_name,
                current_owner_type_name,
                type_param_name,
                method_name,
                options,
            );
    }
    if let Some(target) = metadata.value_subscript_target.as_deref()
        && let Some(target_name) = target.value_name.as_deref()
    {
        return resolve_scope_param_semantic_type(
            node,
            current_owner_name,
            current_owner_type_name,
            target_name,
        )
        .is_some_and(|target_type| semantic_type_mentions_name(&target_type, type_param_name));
    }
    if let (Some(true_value), Some(false_value)) =
        (metadata.value_if_true.as_deref(), metadata.value_if_false.as_deref())
    {
        return scoped_metadata_has_type_param_provenance(
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            current_line,
            true_value,
            type_param_name,
            options,
            &mut visiting.clone(),
        ) && scoped_metadata_has_type_param_provenance(
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            current_line,
            false_value,
            type_param_name,
            options,
            &mut visiting.clone(),
        );
    }
    false
}

#[expect(
    clippy::too_many_arguments,
    reason = "value provenance needs graph, lexical scope, source position, and cycle state"
)]
fn scoped_value_has_type_param_provenance(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
    type_param_name: &str,
    options: AssignabilityOptions,
    visiting: &mut BTreeSet<String>,
) -> bool {
    if !visiting.insert(value_name.to_owned()) {
        return false;
    }
    if resolve_scope_param_semantic_type(
        node,
        current_owner_name,
        current_owner_type_name,
        value_name,
    )
    .is_some_and(|value_type| {
        matches!(value_type.strip_annotated(), SemanticType::Name(name) if name == type_param_name)
    }) {
        return true;
    }
    if let Some(for_site) = node.for_loops.iter().rev().find(|site| {
        (site.target_name == value_name
            || site.target_names.iter().any(|target| target == value_name))
            && site.owner_name.as_deref() == current_owner_name
            && site.owner_type_name.as_deref() == current_owner_type_name
            && site.line < current_line
    }) && for_site.iter_name.as_deref().is_some_and(|iter_name| {
        resolve_scope_param_semantic_type(
            node,
            current_owner_name,
            current_owner_type_name,
            iter_name,
        )
        .is_some_and(|iter_type| semantic_type_mentions_name(&iter_type, type_param_name))
    }) {
        return true;
    }
    if let Some(with_site) = node.with_statements.iter().rev().find(|site| {
        site.target_name.as_deref() == Some(value_name)
            && site.owner_name.as_deref() == current_owner_name
            && site.owner_type_name.as_deref() == current_owner_type_name
            && site.line < current_line
    }) && with_site.context_name.as_deref().is_some_and(|context_name| {
        resolve_scope_param_semantic_type(
            node,
            current_owner_name,
            current_owner_type_name,
            context_name,
        )
        .is_some_and(|context_type| semantic_type_mentions_name(&context_type, type_param_name))
    }) {
        return true;
    }
    let Some(owner_name) = current_owner_name else {
        return false;
    };
    let Some(assignment) = node.assignments.iter().rev().find(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.as_deref() == Some(owner_name)
            && assignment.owner_type_name.as_deref() == current_owner_type_name
            && assignment.line < current_line
    }) else {
        return false;
    };
    if assignment.annotation_expr.as_ref().is_some_and(|annotation| {
        matches!(
            lower_type_expr(annotation.expr.clone()).strip_annotated(),
            SemanticType::Name(name) if name == type_param_name
        )
    }) {
        return true;
    }
    let Some(metadata) = assignment.value_metadata() else {
        return false;
    };
    if assignment.destructuring_index.is_some()
        && metadata.value_name.as_deref().is_some_and(|source_name| {
            resolve_scope_param_semantic_type(
                node,
                current_owner_name,
                current_owner_type_name,
                source_name,
            )
            .is_some_and(|source_type| semantic_type_mentions_name(&source_type, type_param_name))
        })
    {
        return true;
    }
    scoped_metadata_has_type_param_provenance(
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        assignment.line,
        &metadata,
        type_param_name,
        options,
        visiting,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "member reference resolution needs source metadata and scope context"
)]
#[allow(dead_code)]
pub(super) fn resolve_direct_member_reference_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    owner_name: &str,
    member_name: &str,
    through_instance: bool,
) -> Option<SemanticType> {
    resolve_direct_member_reference_semantic_type_with_options(
        node,
        nodes,
        signature,
        exclude_name,
        current_owner_name,
        current_owner_type_name,
        current_line,
        owner_name,
        member_name,
        through_instance,
        AssignabilityOptions::default(),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "member reference resolution needs source metadata and scope context"
)]
pub(super) fn resolve_direct_member_reference_semantic_type_with_options(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    owner_name: &str,
    member_name: &str,
    through_instance: bool,
    options: AssignabilityOptions,
) -> Option<SemanticType> {
    if !through_instance
        && let Some(reference_type) = resolve_imported_module_member_reference_semantic_type(
            node,
            nodes,
            owner_name,
            member_name,
        )
    {
        return Some(reference_type);
    }

    let owner_type = if through_instance {
        resolve_direct_callable_return_semantic_type(node, nodes, owner_name)
            .or_else(|| Some(SemanticType::Name(owner_name.to_owned())))
    } else {
        resolve_direct_name_reference_semantic_type_with_options(
            node,
            nodes,
            signature,
            exclude_name,
            current_owner_name,
            current_owner_type_name,
            current_line,
            owner_name,
            options,
        )
        .or_else(|| Some(SemanticType::Name(owner_name.to_owned())))
    }?;

    let bound_owner_type = scoped_type_param_bound_semantic_type(
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        current_line,
        owner_name,
        through_instance,
        &owner_type,
        options,
    );
    match bound_owner_type {
        Some(bound_owner_type) => resolve_member_semantic_type_on_owner_type_with_self_type(
            node,
            nodes,
            &bound_owner_type,
            Some(&owner_type),
            member_name,
            options,
        ),
        None => {
            resolve_member_semantic_type_on_owner_type(node, nodes, &owner_type, member_name, options)
        }
    }
}

pub(super) fn resolve_member_semantic_type_on_owner_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    owner_type: &SemanticType,
    member_name: &str,
    options: AssignabilityOptions,
) -> Option<SemanticType> {
    resolve_member_semantic_type_on_owner_type_with_self_type(
        node,
        nodes,
        owner_type,
        None,
        member_name,
        options,
    )
}

pub(super) fn resolve_member_semantic_type_on_owner_type_with_self_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    owner_type: &SemanticType,
    self_type: Option<&SemanticType>,
    member_name: &str,
    options: AssignabilityOptions,
) -> Option<SemanticType> {
    if let Some(branches) = semantic_member_union_branches(owner_type, options.strict_nulls) {
        let member_types = branches
            .iter()
            .map(|branch| {
                resolve_member_semantic_type_on_owner_type_with_self_type(
                    node,
                    nodes,
                    branch,
                    self_type.or(Some(branch)),
                    member_name,
                    options,
                )
            })
            .collect::<Option<Vec<_>>>()?;
        return Some(join_semantic_type_candidates(member_types));
    }
    let owner_type_name = semantic_nominal_owner_name(owner_type)?;
    let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
    let Some(member) = find_owned_readable_member_declaration(
        nodes,
        class_node,
        class_decl,
        member_name,
    ) else {
        if !options.framework_adapters {
            return None;
        }
        let context = CheckerContext::new_with_bound_surface_facts_and_options(
            nodes,
            None,
            None,
            CheckerOptions {
                experimental_framework_adapters: true,
                ..CheckerOptions::permissive_test_default()
            },
        );
        return framework_generated_member_semantic_type_with_context(
            &context,
            node,
            &owner_type_name,
            member_name,
        );
    };
    if is_enum_like_class(nodes, class_node, class_decl) {
        return Some(lower_type_text_or_name(&format!("Literal[{}.{}]", class_decl.name, member_name)));
    }
    resolve_readable_member_semantic_type_with_self_type(
        node,
        nodes,
        member,
        owner_type,
        self_type.or(Some(owner_type)),
    )
}

pub(super) fn resolve_method_return_semantic_type_on_owner_type_with_self_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    owner_type: &SemanticType,
    self_type: Option<&SemanticType>,
    method_name: &str,
    options: AssignabilityOptions,
) -> Option<SemanticType> {
    if let Some(branches) = semantic_member_union_branches(owner_type, options.strict_nulls) {
        let return_types = branches
            .iter()
            .map(|branch| {
                resolve_method_return_semantic_type_on_owner_type_with_self_type(
                    node,
                    nodes,
                    branch,
                    self_type.or(Some(branch)),
                    method_name,
                    options,
                )
            })
            .collect::<Option<Vec<_>>>()?;
        return Some(join_semantic_type_candidates(return_types));
    }
    let owner_type_name = semantic_nominal_owner_name(owner_type)?;
    let self_type = self_type.unwrap_or(owner_type);
    let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
    let owner_substitutions = owner_generic_substitutions(owner_type, class_decl);
    let methods = find_owned_callable_declarations(nodes, class_node, class_decl, method_name);
    let [method] = methods.as_slice() else {
        return None;
    };
    if method.kind == DeclarationKind::Overload {
        return None;
    }
    Some(rewrite_imported_typing_semantic_type(
        node,
        &substitute_self_semantic_type_with_type(
            &substitute_semantic_type_params(
                &declaration_signature_return_semantic_type(method)?,
                &owner_substitutions,
            ),
            Some(self_type),
        ),
    ))
}

pub(super) fn is_enum_like_class(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
) -> bool {
    declaration.rendered_class_bases().iter().any(|base| {
        matches!(
            base.as_str(),
            "Enum"
                | "IntEnum"
                | "StrEnum"
                | "Flag"
                | "IntFlag"
                | "enum.Enum"
                | "enum.IntEnum"
                | "enum.StrEnum"
                | "enum.Flag"
                | "enum.IntFlag"
        ) || resolve_direct_base(nodes, node, base)
            .is_some_and(|(base_node, base_decl)| is_enum_like_class(nodes, base_node, base_decl))
    })
}

pub(super) fn is_flag_enum_like_class(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
) -> bool {
    declaration.rendered_class_bases().iter().any(|base| {
        matches!(base.as_str(), "Flag" | "IntFlag" | "enum.Flag" | "enum.IntFlag")
            || resolve_direct_base(nodes, node, base).is_some_and(|(base_node, base_decl)| {
                is_flag_enum_like_class(nodes, base_node, base_decl)
            })
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "method return resolution needs source metadata and scope context"
)]
pub(super) fn resolve_direct_method_return_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    owner_name: &str,
    method_name: &str,
    through_instance: bool,
    options: AssignabilityOptions,
) -> Option<SemanticType> {
    if !through_instance
        && let Some(return_type) = resolve_imported_module_method_return_semantic_type(
            node,
            nodes,
            current_line,
            current_owner_name,
            current_owner_type_name,
            owner_name,
            method_name,
            options,
        )
    {
        return Some(return_type);
    }

    let receiver_type = if through_instance {
        resolve_direct_callable_return_semantic_type(node, nodes, owner_name)
            .or_else(|| Some(SemanticType::Name(owner_name.to_owned())))
    } else {
        resolve_direct_name_reference_semantic_type_with_options(
            node,
            nodes,
            signature,
            exclude_name,
            current_owner_name,
            current_owner_type_name,
            current_line,
            owner_name,
            options,
        )
        .or_else(|| Some(SemanticType::Name(owner_name.to_owned())))
    }?;

    let bound_owner_type = scoped_type_param_bound_semantic_type(
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        current_line,
        owner_name,
        through_instance,
        &receiver_type,
        options,
    );
    let owner_type = bound_owner_type.as_ref().unwrap_or(&receiver_type);
    if let Some(branches) = semantic_member_union_branches(owner_type, options.strict_nulls) {
        let return_types = branches
            .iter()
            .map(|branch| {
                if bound_owner_type.is_some() {
                    resolve_direct_method_return_on_owner_type(
                        node,
                        nodes,
                        current_owner_name,
                        current_owner_type_name,
                        current_line,
                        owner_name,
                        method_name,
                        through_instance,
                        &receiver_type,
                        Some(branch),
                        options,
                    )
                } else {
                    resolve_direct_method_return_on_owner_type(
                        node,
                        nodes,
                        current_owner_name,
                        current_owner_type_name,
                        current_line,
                        owner_name,
                        method_name,
                        through_instance,
                        branch,
                        None,
                        options,
                    )
                }
            })
            .collect::<Option<Vec<_>>>()?;
        return Some(join_semantic_type_candidates(return_types));
    }
    resolve_direct_method_return_on_owner_type(
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        current_line,
        owner_name,
        method_name,
        through_instance,
        &receiver_type,
        bound_owner_type.as_ref(),
        options,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "branch-wise method return resolution needs source call context and receiver/lookup types"
)]
fn resolve_direct_method_return_on_owner_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    owner_name: &str,
    method_name: &str,
    through_instance: bool,
    receiver_type: &SemanticType,
    lookup_owner_type: Option<&SemanticType>,
    options: AssignabilityOptions,
) -> Option<SemanticType> {
    let owner_type = lookup_owner_type.unwrap_or(receiver_type);
    let owner_type_name = semantic_nominal_owner_name(owner_type)?;
    let self_type = receiver_type;
    let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
    let owner_substitutions = owner_generic_substitutions(owner_type, class_decl);
    let methods = find_owned_callable_declarations(nodes, class_node, class_decl, method_name);
    if methods.is_empty() {
        return None;
    }
    if methods.iter().any(|declaration| declaration.kind == DeclarationKind::Overload)
    {
        let call = node.method_calls.iter().find(|call| {
            call.owner_name == owner_name
                && call.method == method_name
                && call.through_instance == through_instance
                && call.line == current_line
        })?;
        let call = typepython_binding::CallSite {
            callee: format!("{}.{}", class_decl.name, method_name),
            arg_count: call.arg_count,
            arg_values: call.arg_values.clone(),
            starred_arg_values: call.starred_arg_values.clone(),
            keyword_names: call.keyword_names.clone(),
            keyword_arg_values: call.keyword_arg_values.clone(),
            keyword_expansion_values: call.keyword_expansion_values.clone(),
            line: current_line,
        };
        let overloads = methods
            .iter()
            .filter(|declaration| declaration.kind == DeclarationKind::Overload)
            .map(|declaration| (*declaration, declaration_callable_semantics(declaration)))
            .collect::<Vec<_>>();
        match resolve_method_overload_selection(
            node,
            nodes,
            &call,
            current_owner_name,
            current_owner_type_name,
            receiver_type,
            lookup_owner_type,
            &overloads,
            options,
        ) {
            ResolvedOverloadSelection::Selected(candidate) => candidate.return_type,
            _ => None,
        }
    } else {
        let method = *methods.first()?;
        if let Some(call) = node.method_calls.iter().find(|call| {
            call.owner_name == owner_name
                && call.method == method_name
                && call.through_instance == through_instance
                && call.line == current_line
        }) {
            let call = typepython_binding::CallSite {
                callee: format!("{}.{}", class_decl.name, method_name),
                arg_count: call.arg_count,
                arg_values: call.arg_values.clone(),
                starred_arg_values: call.starred_arg_values.clone(),
                keyword_names: call.keyword_names.clone(),
                keyword_arg_values: call.keyword_arg_values.clone(),
                keyword_expansion_values: call.keyword_expansion_values.clone(),
                line: current_line,
            };
            if let Some(return_type) = resolve_method_call_candidate_detailed(
                node,
                nodes,
                method,
                &call,
                current_owner_name,
                current_owner_type_name,
                receiver_type,
                lookup_owner_type,
                declaration_callable_semantics(method).as_ref(),
                options,
            )
            .ok()
            .and_then(|resolved| resolved.return_type)
            {
                return Some(return_type);
            }
        }

        Some(rewrite_imported_typing_semantic_type(
            node,
            &substitute_self_semantic_type_with_type(
                &substitute_semantic_type_params(
                    &declaration_signature_return_semantic_type(method)?,
                    &owner_substitutions,
                ),
                Some(self_type),
            ),
        ))
    }
}

pub(super) fn unwrap_awaitable_semantic_type(ty: &SemanticType) -> Option<SemanticType> {
    match ty.strip_annotated() {
        SemanticType::Generic { head, args }
            if matches!(
                head.as_str(),
                "Awaitable" | "typing.Awaitable" | "collections.abc.Awaitable"
            ) && args.len() == 1 =>
        {
            Some(args[0].clone())
        }
        SemanticType::Generic { head, args }
            if matches!(
                head.as_str(),
                "Coroutine" | "typing.Coroutine" | "collections.abc.Coroutine"
            ) && args.len() == 3 =>
        {
            Some(args[2].clone())
        }
        _ => None,
    }
}

pub(super) fn unwrap_generator_yield_semantic_type(ty: &SemanticType) -> Option<SemanticType> {
    match ty.strip_annotated() {
        SemanticType::Generic { head, args }
            if matches!(
                head.as_str(),
                "Generator" | "typing.Generator" | "collections.abc.Generator"
            ) && !args.is_empty() =>
        {
            Some(args[0].clone())
        }
        _ => None,
    }
}

pub(super) fn unwrap_yield_from_semantic_type(ty: &SemanticType) -> Option<SemanticType> {
    match ty.strip_annotated() {
        SemanticType::Generic { head, args }
            if matches!(
                head.as_str(),
                "Generator"
                    | "typing.Generator"
                    | "collections.abc.Generator"
                    | "Iterator"
                    | "typing.Iterator"
                    | "collections.abc.Iterator"
                    | "Iterable"
                    | "typing.Iterable"
                    | "collections.abc.Iterable"
                    | "Sequence"
                    | "typing.Sequence"
                    | "collections.abc.Sequence"
                    | "list"
                    | "set"
                    | "frozenset"
            ) && !args.is_empty() =>
        {
            Some(args[0].clone())
        }
        SemanticType::Generic { head, args } if head == "tuple" => {
            let expanded = expanded_tuple_shape_semantic_args(args);
            if expanded.len() == 2
                && matches!(&expanded[1], SemanticType::Name(name) if name == "...")
            {
                return Some(expanded[0].clone());
            }
            expanded.first().cloned()
        }
        _ => None,
    }
}

pub(super) fn unwrap_for_iterable_semantic_type(ty: &SemanticType) -> Option<SemanticType> {
    match ty.strip_annotated() {
        SemanticType::Name(name) if name == "range" => Some(SemanticType::Name(String::from("int"))),
        _ => unwrap_yield_from_semantic_type(ty),
    }
}

pub(super) fn find_method_line(
    source: &str,
    owner_type_name: &str,
    method_name: &str,
) -> Option<usize> {
    typepython_syntax::collect_direct_method_signature_sites(source)
        .into_iter()
        .find(|site| site.owner_type_name == owner_type_name && site.name == method_name)
        .map(|site| site.line)
}

pub(super) fn find_function_line(source: &str, function_name: &str) -> Option<usize> {
    typepython_syntax::collect_direct_function_signature_sites(source)
        .into_iter()
        .find(|site| site.name == function_name)
        .map(|site| site.line)
}

pub(super) fn single_line_return_annotation_span(
    source: &str,
    owner_type_name: Option<&str>,
    function_name: &str,
) -> Option<Span> {
    let line = match owner_type_name {
        Some(owner_type_name) => find_method_line(source, owner_type_name, function_name)?,
        None => find_function_line(source, function_name)?,
    };
    let line_text = source.lines().nth(line.saturating_sub(1))?;
    let arrow = line_text.find("->")?;
    let colon = line_text[arrow + 2..].find(':')? + arrow + 2;
    let start_column = arrow
        + 3
        + line_text[arrow + 2..].chars().take_while(|character| character.is_whitespace()).count();
    let end_trimmed = line_text[..colon].trim_end();
    Some(Span::new(String::new(), line, start_column, line, end_trimmed.chars().count() + 1))
}

pub(super) fn override_insertion_span(
    source: &str,
    owner_type_name: &str,
    method_name: &str,
    path: &std::path::Path,
) -> Option<Span> {
    let line = find_method_line(source, owner_type_name, method_name)?;
    let line_text = source.lines().nth(line.saturating_sub(1))?;
    let indent = line_text.chars().take_while(|character| character.is_whitespace()).count() + 1;
    Some(Span::new(path.display().to_string(), line, indent, line, indent))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn attach_missing_none_return_suggestion(
    diagnostic: Diagnostic,
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected: &str,
    actual: &str,
) -> Diagnostic {
    let inferred_actual = inferred_return_type_for_owner(context, node, nodes, return_site, expected)
        .unwrap_or_else(|| normalize_type_text(actual));
    if union_branches(expected)
        .is_some_and(|branches| branches.iter().any(|branch| branch == "None"))
        || !union_branches(&inferred_actual)
            .is_some_and(|branches| branches.iter().any(|branch| branch == "None"))
    {
        return diagnostic;
    }
    let Some(without_none) = remove_none_branch(&inferred_actual) else {
        return diagnostic;
    };
    let expected_type = lower_type_text_or_name(expected);
    let without_none_type = lower_type_text_or_name(&without_none);
    if !context.semantic_type_is_assignable(node, &expected_type, &without_none_type) {
        return diagnostic;
    }
    if node.module_path.to_string_lossy().starts_with('<') {
        return diagnostic;
    }
    let Some(source) = context.load_source_text(node) else {
        return diagnostic;
    };
    let Some(mut span) = single_line_return_annotation_span(
        source.as_str(),
        return_site.owner_type_name.as_deref(),
        &return_site.owner_name,
    ) else {
        return diagnostic;
    };
    span.path = node.module_path.display().to_string();
    diagnostic.with_suggestion(
        "Add `| None` to the declared return type",
        span,
        format!("{} | None", expected.trim()),
        SuggestionApplicability::MachineApplicable,
    )
}

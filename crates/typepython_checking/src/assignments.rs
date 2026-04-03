use super::*;

pub(super) fn annotated_assignment_type_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for assignment in &node.assignments {
        let Some(annotation) = assignment.annotation.as_deref() else {
            continue;
        };
        let Some(expected) = normalized_assignment_annotation(annotation).map(normalize_type_text)
        else {
            continue;
        };

        if let Some(callable_result) =
            callable_assignment_result(node, nodes, assignment, &expected)
        {
            if let Some(diagnostic) = callable_result {
                diagnostics.push(diagnostic);
            }
            continue;
        }

        let assignment_metadata = direct_expr_metadata_from_assignment_site(assignment);
        if let Some(result) = resolve_contextual_typed_dict_literal_type_with_context(
            context,
            node,
            nodes,
            assignment.line,
            &assignment_metadata,
            Some(&expected),
        ) {
            diagnostics.extend(result.diagnostics);
            if !direct_type_is_assignable(node, nodes, &expected, &result.actual_type) {
                diagnostics.push(Diagnostic::error(
                    "TPY4001",
                    match (&assignment.owner_type_name, &assignment.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` assigns `{}` where local `{}` in `{}` expects `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            owner_name,
                            expected
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` assigns `{}` where local `{}` expects `{}`",
                            owner_name,
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            expected
                        ),
                        _ => format!(
                            "module `{}` assigns `{}` where `{}` expects `{}`",
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            expected
                        ),
                    },
                ));
            }
            continue;
        }

        if let Some(result) = resolve_contextual_collection_literal_type_with_context(
            context,
            node,
            nodes,
            assignment.line,
            &assignment_metadata,
            Some(&expected),
        ) {
            diagnostics.extend(result.diagnostics);
            if !direct_type_is_assignable(node, nodes, &expected, &result.actual_type) {
                diagnostics.push(Diagnostic::error(
                    "TPY4001",
                    match (&assignment.owner_type_name, &assignment.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` assigns `{}` where local `{}` in `{}` expects `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            owner_name,
                            expected
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` assigns `{}` where local `{}` expects `{}`",
                            owner_name,
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            expected
                        ),
                        _ => format!(
                            "module `{}` assigns `{}` where `{}` expects `{}`",
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            expected
                        ),
                    },
                ));
            }
            continue;
        }

        let signature = resolve_assignment_owner_signature(node, assignment);
        let Some(actual) = resolve_assignment_site_type(node, nodes, signature, assignment) else {
            continue;
        };
        if !direct_type_is_assignable(node, nodes, &expected, &actual) {
            diagnostics.push(Diagnostic::error(
                "TPY4001",
                match (&assignment.owner_type_name, &assignment.owner_name) {
                    (Some(owner_type_name), Some(owner_name)) => format!(
                        "type `{}` in module `{}` assigns `{}` where local `{}` in `{}` expects `{}`",
                        owner_type_name,
                        node.module_path.display(),
                        actual,
                        assignment.name,
                        owner_name,
                        expected
                    ),
                    (None, Some(owner_name)) => format!(
                        "function `{}` in module `{}` assigns `{}` where local `{}` expects `{}`",
                        owner_name,
                        node.module_path.display(),
                        actual,
                        assignment.name,
                        expected
                    ),
                    _ => format!(
                        "module `{}` assigns `{}` where `{}` expects `{}`",
                        node.module_path.display(),
                        actual,
                        assignment.name,
                        expected
                    ),
                },
            ));
        }
    }

    diagnostics
}

pub(super) fn simple_name_augmented_assignment_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.assignments
        .iter()
        .filter(|assignment| assignment.annotation.is_none())
        .filter(|assignment| {
            assignment
                .value_binop_left
                .as_deref()
                .and_then(|left| left.value_name.as_deref())
                == Some(assignment.name.as_str())
                && assignment.value_binop_right.is_some()
                && assignment.value_binop_operator.is_some()
        })
        .filter(|assignment| {
            node.invalidations.iter().any(|site| {
                site.kind == typepython_binding::InvalidationKind::RebindLike
                    && site.line == assignment.line
                    && site.owner_name == assignment.owner_name
                    && site.owner_type_name == assignment.owner_type_name
                    && site.names.iter().any(|name| name == &assignment.name)
            })
        })
        .filter_map(|assignment| {
            if current_augmented_assignment_target_is_final(node, assignment) {
                return Some(Diagnostic::error(
                    "TPY4006",
                    match (&assignment.owner_type_name, &assignment.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` reassigns Final binding `{}` in `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            assignment.name,
                            owner_name,
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` reassigns Final binding `{}`",
                            owner_name,
                            node.module_path.display(),
                            assignment.name,
                        ),
                        _ => format!(
                            "module `{}` reassigns Final binding `{}`",
                            node.module_path.display(),
                            assignment.name,
                        ),
                    },
                ));
            }
            let signature = resolve_assignment_owner_signature(node, assignment);
            let expected = resolve_current_augmented_assignment_target_type(
                node,
                nodes,
                signature,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                &assignment.name,
            )?;
            let actual = resolve_augmented_assignment_result_type(
                node,
                nodes,
                signature,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                assignment.value_binop_operator.as_deref(),
                &expected,
                assignment.value_binop_right.as_deref()?,
            )?;
            (!direct_type_matches(node, nodes, &expected, &actual)).then(|| {
                Diagnostic::error(
                    "TPY4001",
                    match (&assignment.owner_type_name, &assignment.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` augmented-assigns `{}` where local `{}` in `{}` expects `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            actual,
                            assignment.name,
                            owner_name,
                            expected
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` augmented-assigns `{}` where local `{}` expects `{}`",
                            owner_name,
                            node.module_path.display(),
                            actual,
                            assignment.name,
                            expected
                        ),
                        _ => format!(
                            "module `{}` augmented-assigns `{}` where `{}` expects `{}`",
                            node.module_path.display(),
                            actual,
                            assignment.name,
                            expected
                        ),
                    },
                )
            })
        })
        .collect()
}

pub(super) fn current_augmented_assignment_target_is_final(
    node: &typepython_graph::ModuleNode,
    assignment: &typepython_binding::AssignmentSite,
) -> bool {
    if assignment.owner_name.is_none()
        && node.declarations.iter().any(|declaration| {
            declaration.kind == DeclarationKind::Value
                && declaration.owner.is_none()
                && declaration.name == assignment.name
                && declaration.is_final
        })
    {
        return true;
    }

    node.assignments.iter().rev().any(|previous| {
        previous.name == assignment.name
            && previous.owner_name == assignment.owner_name
            && previous.owner_type_name == assignment.owner_type_name
            && previous.line < assignment.line
            && previous.annotation.as_deref().is_some_and(is_final_annotation_text)
    })
}

pub(super) fn final_attribute_reassignment_diagnostic(
    module_path: &std::path::Path,
    owner_type_name: &str,
    member_name: &str,
) -> Diagnostic {
    Diagnostic::error(
        "TPY4006",
        format!(
            "type `{}` in module `{}` reassigns Final binding `{}`",
            owner_type_name,
            module_path.display(),
            member_name,
        ),
    )
}

pub(super) fn is_final_annotation_text(annotation: &str) -> bool {
    let annotation = annotation.trim();
    annotation == "Final"
        || annotation.starts_with("Final[")
        || annotation == "typing.Final"
        || annotation.starts_with("typing.Final[")
}

pub(super) fn resolve_current_augmented_assignment_target_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<String> {
    if let Some(signature) = signature {
        let signature = rewrite_imported_typing_aliases(
            node,
            &substitute_self_annotation(signature, current_owner_type_name),
        );
        if let Some(param_type) = resolve_direct_return_name_type(&signature, value_name) {
            return Some(param_type);
        }
    }

    match current_owner_name {
        Some(owner_name) => resolve_local_assignment_reference_type(
            node,
            nodes,
            signature,
            Some(owner_name),
            current_owner_type_name,
            current_line,
            value_name,
        ),
        None => resolve_module_level_assignment_reference_type(
            node,
            nodes,
            signature,
            current_line,
            value_name,
        ),
    }
}

pub(super) fn direct_expr_metadata_from_assignment_site(
    assignment: &typepython_binding::AssignmentSite,
) -> typepython_syntax::DirectExprMetadata {
    typepython_syntax::DirectExprMetadata {
        value_type: assignment.value_type.clone(),
        is_awaited: assignment.is_awaited,
        value_callee: assignment.value_callee.clone(),
        value_name: assignment.value_name.clone(),
        value_member_owner_name: assignment.value_member_owner_name.clone(),
        value_member_name: assignment.value_member_name.clone(),
        value_member_through_instance: assignment.value_member_through_instance,
        value_method_owner_name: assignment.value_method_owner_name.clone(),
        value_method_name: assignment.value_method_name.clone(),
        value_method_through_instance: assignment.value_method_through_instance,
        value_subscript_target: assignment.value_subscript_target.clone(),
        value_subscript_string_key: assignment.value_subscript_string_key.clone(),
        value_subscript_index: assignment.value_subscript_index.clone(),
        value_if_true: assignment.value_if_true.clone(),
        value_if_false: assignment.value_if_false.clone(),
        value_if_guard: assignment.value_if_guard.as_ref().map(site_to_guard),
        value_bool_left: assignment.value_bool_left.clone(),
        value_bool_right: assignment.value_bool_right.clone(),
        value_binop_left: assignment.value_binop_left.clone(),
        value_binop_right: assignment.value_binop_right.clone(),
        value_binop_operator: assignment.value_binop_operator.clone(),
        value_lambda: assignment.value_lambda.clone(),
        value_list_comprehension: assignment.value_list_comprehension.clone(),
        value_generator_comprehension: assignment.value_generator_comprehension.clone(),
        value_list_elements: assignment.value_list_elements.clone(),
        value_set_elements: assignment.value_set_elements.clone(),
        value_dict_entries: assignment.value_dict_entries.clone(),
    }
}

#[derive(Debug, Clone)]
pub(super) struct TypedDictFieldShape {
    pub(super) value_type: String,
    pub(super) required: bool,
    pub(super) readonly: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TypedDictExtraItemsShape {
    pub(super) value_type: String,
    pub(super) readonly: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TypedDictShape {
    pub(super) name: String,
    pub(super) fields: BTreeMap<String, TypedDictFieldShape>,
    pub(super) closed: bool,
    pub(super) extra_items: Option<TypedDictExtraItemsShape>,
}

#[derive(Debug, Clone)]
pub(super) struct DataclassTransformFieldShape {
    pub(super) name: String,
    pub(super) keyword_name: String,
    pub(super) annotation: String,
    pub(super) required: bool,
    pub(super) kw_only: bool,
}

#[derive(Debug, Clone)]
pub(super) struct DataclassTransformClassShape {
    pub(super) fields: Vec<DataclassTransformFieldShape>,
    pub(super) frozen: bool,
    pub(super) has_explicit_init: bool,
}

pub(super) fn is_typed_dict_base_name(base: &str) -> bool {
    matches!(base.trim(), "TypedDict" | "typing.TypedDict" | "typing_extensions.TypedDict")
}

pub(super) fn typed_dict_known_or_extra_field<'a>(
    shape: &'a TypedDictShape,
    key: &str,
) -> Option<TypedDictFieldShapeRef<'a>> {
    if let Some(field) = shape.fields.get(key) {
        return Some(TypedDictFieldShapeRef::Known(field));
    }
    shape.extra_items.as_ref().map(TypedDictFieldShapeRef::Extra)
}

pub(super) fn typed_dict_shape_has_unbounded_extra_keys(shape: &TypedDictShape) -> bool {
    !shape.closed && shape.extra_items.is_none()
}

pub(super) enum TypedDictFieldShapeRef<'a> {
    Known(&'a TypedDictFieldShape),
    Extra(&'a TypedDictExtraItemsShape),
}

impl<'a> TypedDictFieldShapeRef<'a> {
    pub(super) fn value_type(&self) -> &str {
        match self {
            Self::Known(field) => &field.value_type,
            Self::Extra(field) => &field.value_type,
        }
    }

    pub(super) fn readonly(&self) -> bool {
        match self {
            Self::Known(field) => field.readonly,
            Self::Extra(field) => field.readonly,
        }
    }
}

pub(super) fn typed_dict_literal_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    let mut diagnostics = Vec::new();
    for site in typepython_syntax::collect_typed_dict_literal_sites(&source) {
        let Some(annotation) = normalized_assignment_annotation(&site.annotation) else {
            continue;
        };
        let annotation = rewrite_imported_typing_aliases(node, annotation);
        let Some(target_shape) = resolve_known_typed_dict_shape_from_type_with_context(
            context,
            node,
            nodes,
            &annotation,
        ) else {
            continue;
        };

        let signature = resolve_scope_owner_signature(
            node,
            site.owner_name.as_deref(),
            site.owner_type_name.as_deref(),
        );
        diagnostics.extend(typed_dict_literal_entry_diagnostics(
            context,
            node,
            nodes,
            site.line,
            &site.entries,
            &target_shape,
            signature,
            site.owner_name.as_deref(),
            site.owner_type_name.as_deref(),
        ));
    }

    diagnostics
}

#[allow(clippy::too_many_arguments)]
pub(super) fn typed_dict_literal_entry_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    line: usize,
    entries: &[typepython_syntax::TypedDictLiteralEntry],
    target_shape: &TypedDictShape,
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut guaranteed_keys = BTreeSet::new();

    for entry in entries {
        if entry.is_expansion {
            let Some(expansion_type) = resolve_assignment_expression_semantic_type(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                line,
                &entry.value,
            ) else {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    line,
                    format!(
                        "TypedDict literal for `{}` uses invalid `**` expansion",
                        target_shape.name
                    ),
                ));
                continue;
            };
            let expansion_type_rendered = render_semantic_type(&expansion_type);

            let Some(expansion_shape) = resolve_known_typed_dict_shape_from_type_with_context(
                context,
                node,
                nodes,
                &expansion_type_rendered,
            ) else {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    line,
                    format!(
                        "TypedDict literal for `{}` uses invalid `**` expansion of `{}`",
                        target_shape.name, expansion_type_rendered
                    ),
                ));
                continue;
            };

            if typed_dict_shape_has_unbounded_extra_keys(&expansion_shape)
                && target_shape.extra_items.is_none()
            {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    line,
                    format!(
                        "TypedDict literal for `{}` cannot expand open TypedDict `{}` because it may contain undeclared keys",
                        target_shape.name, expansion_shape.name
                    ),
                ));
                continue;
            }

            for (key, field) in &expansion_shape.fields {
                let Some(target_field) = typed_dict_known_or_extra_field(target_shape, key) else {
                    diagnostics.push(typed_dict_literal_diagnostic(
                        node,
                        line,
                        format!(
                            "TypedDict literal for `{}` expands unknown key `{}`",
                            target_shape.name, key
                        ),
                    ));
                    continue;
                };

                let expected_type = lower_type_text_or_name(target_field.value_type());
                let actual_type = lower_type_text_or_name(&field.value_type);
                if !semantic_type_matches(node, nodes, &expected_type, &actual_type) {
                    diagnostics.push(typed_dict_literal_diagnostic(
                        node,
                        line,
                        format!(
                            "TypedDict literal for `{}` expands `{}` with `{}` where `{}` expects `{}`",
                            target_shape.name,
                            key,
                            render_semantic_type(&actual_type),
                            key,
                            render_semantic_type(&expected_type)
                        ),
                    ));
                }

                if field.required {
                    guaranteed_keys.insert(key.clone());
                }
            }

            if let Some(extra_items) = &expansion_shape.extra_items
                && target_shape.extra_items.as_ref().is_none_or(|target_extra| {
                    !semantic_type_matches(
                        node,
                        nodes,
                        &lower_type_text_or_name(&target_extra.value_type),
                        &lower_type_text_or_name(&extra_items.value_type),
                    )
                })
            {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    line,
                    format!(
                        "TypedDict literal for `{}` expands `{}` with additional keys of type `{}` that are not accepted by the target",
                        target_shape.name,
                        expansion_shape.name,
                        extra_items.value_type
                    ),
                ));
            }

            continue;
        }

        let Some(key) = entry.key.as_deref() else {
            diagnostics.push(typed_dict_literal_diagnostic(
                node,
                line,
                format!("TypedDict literal for `{}` uses a non-literal key", target_shape.name),
            ));
            continue;
        };

        let Some(target_field) = typed_dict_known_or_extra_field(target_shape, key) else {
            diagnostics.push(typed_dict_literal_diagnostic(
                node,
                line,
                format!("TypedDict literal for `{}` uses unknown key `{}`", target_shape.name, key),
            ));
            continue;
        };

        if let Some(actual_type) = resolve_assignment_expression_semantic_type(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            line,
            &entry.value,
        ) {
            let expected_type = lower_type_text_or_name(target_field.value_type());
            if !semantic_type_matches(node, nodes, &expected_type, &actual_type) {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    line,
                    format!(
                        "TypedDict literal for `{}` assigns `{}` to key `{}` where `{}` expects `{}`",
                        target_shape.name,
                        render_semantic_type(&actual_type),
                        key,
                        key,
                        render_semantic_type(&expected_type)
                    ),
                ));
            }
        }

        guaranteed_keys.insert(key.to_owned());
    }

    for (key, field) in &target_shape.fields {
        if field.required && !guaranteed_keys.contains(key) {
            diagnostics.push(typed_dict_literal_diagnostic(
                node,
                line,
                format!(
                    "TypedDict literal for `{}` is missing required key `{}`",
                    target_shape.name, key
                ),
            ));
        }
    }

    diagnostics
}

pub(super) fn typed_dict_literal_diagnostic(
    node: &typepython_graph::ModuleNode,
    line: usize,
    message: String,
) -> Diagnostic {
    Diagnostic::error("TPY4013", message).with_span(Span::new(
        node.module_path.display().to_string(),
        line,
        1,
        line,
        1,
    ))
}

pub(super) fn direct_expr_metadata_for_known_type(
    value_type: &str,
) -> typepython_syntax::DirectExprMetadata {
    typepython_syntax::DirectExprMetadata {
        value_type: Some(String::from(value_type)),
        is_awaited: false,
        value_callee: None,
        value_name: None,
        value_member_owner_name: None,
        value_member_name: None,
        value_member_through_instance: false,
        value_method_owner_name: None,
        value_method_name: None,
        value_method_through_instance: false,
        value_subscript_target: None,
        value_subscript_string_key: None,
        value_subscript_index: None,
        value_if_true: None,
        value_if_false: None,
        value_if_guard: None,
        value_bool_left: None,
        value_bool_right: None,
        value_binop_left: None,
        value_binop_right: None,
        value_binop_operator: None,
        value_lambda: None,
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_assignment_expression_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
) -> Option<SemanticType> {
    resolve_direct_expression_semantic_type_from_metadata(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        line,
        metadata,
    )
}

fn contextual_result_semantic_type(result: &ContextualCallArgResult) -> SemanticType {
    lower_type_text_or_name(&result.actual_type)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_augmented_assignment_result_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    current_line: usize,
    operator: Option<&str>,
    left_type: &str,
    value: &typepython_syntax::DirectExprMetadata,
) -> Option<String> {
    resolve_augmented_assignment_result_semantic_type(
        node,
        nodes,
        signature,
        owner_name,
        owner_type_name,
        current_line,
        operator,
        left_type,
        value,
    )
    .map(|resolved| render_semantic_type(&resolved))
}

#[allow(clippy::too_many_arguments)]
fn resolve_augmented_assignment_result_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    current_line: usize,
    operator: Option<&str>,
    left_type: &str,
    value: &typepython_syntax::DirectExprMetadata,
) -> Option<SemanticType> {
    let left = direct_expr_metadata_for_known_type(left_type);
    resolve_direct_binop_type(
        node,
        nodes,
        signature,
        owner_name,
        owner_type_name,
        current_line,
        Some(&left),
        Some(value),
        operator.filter(|operator| !operator.is_empty()),
    )
    .map(|resolved| lower_type_text_or_name(&resolved))
    .or_else(|| {
        resolve_assignment_expression_semantic_type(
            node,
            nodes,
            signature,
            owner_name,
            owner_type_name,
            current_line,
            value,
        )
    })
}

pub(super) fn typed_dict_readonly_mutation_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_typed_dict_mutation_sites(&source)
        .into_iter()
        .filter_map(|site| {
            let signature = resolve_scope_owner_signature(
                node,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
            );
            let owner_type = resolve_assignment_expression_semantic_type(
                node,
                nodes,
                signature,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
                site.line,
                &site.target,
            )?;
            let owner_type_rendered = render_semantic_type(&owner_type);
            let key = site.key.as_deref()?;
            let target_shape = resolve_known_typed_dict_shape_from_type_with_context(
                context,
                node,
                nodes,
                &owner_type_rendered,
            )?;
            let Some(field) = typed_dict_known_or_extra_field(&target_shape, key) else {
                return Some(
                    Diagnostic::error(
                        "TPY4001",
                        format!(
                            "TypedDict item `{}` on `{}` in module `{}` is not a declared key",
                            key,
                            target_shape.name,
                            node.module_path.display()
                        ),
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        site.line,
                        1,
                        site.line,
                        1,
                    )),
                );
            };
            if field.readonly() {
                return Some(
                    Diagnostic::error(
                        "TPY4016",
                        match site.kind {
                            typepython_syntax::TypedDictMutationKind::Assignment => format!(
                                "TypedDict item `{}` on `{}` in module `{}` is read-only and cannot be assigned",
                                key,
                                target_shape.name,
                                node.module_path.display()
                            ),
                            typepython_syntax::TypedDictMutationKind::AugmentedAssignment => format!(
                                "TypedDict item `{}` on `{}` in module `{}` is read-only and cannot be updated with augmented assignment",
                                key,
                                target_shape.name,
                                node.module_path.display()
                            ),
                            typepython_syntax::TypedDictMutationKind::Delete => format!(
                                "TypedDict item `{}` on `{}` in module `{}` is read-only and cannot be deleted",
                                key,
                                target_shape.name,
                                node.module_path.display()
                            ),
                        },
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        site.line,
                        1,
                        site.line,
                        1,
                    )),
                );
            }

            match site.kind {
                typepython_syntax::TypedDictMutationKind::Assignment => {
                    let value = site.value.as_ref()?;
                    let contextual = resolve_contextual_call_arg_type_with_context(
                        context,
                        node,
                        nodes,
                        site.line,
                        value,
                        Some(field.value_type()),
                    );
                    if let Some(mut result) = contextual {
                        if let Some(diagnostic) = result.diagnostics.pop() {
                            return Some(diagnostic);
                        }
                        let expected = lower_type_text_or_name(field.value_type());
                        let actual = contextual_result_semantic_type(&result);
                        if !semantic_type_matches(node, nodes, &expected, &actual) {
                            return Some(
                                Diagnostic::error(
                                    "TPY4001",
                                    format!(
                                        "TypedDict item `{}` on `{}` in module `{}` assigns `{}` where `{}` expects `{}`",
                                        key,
                                        target_shape.name,
                                        node.module_path.display(),
                                        render_semantic_type(&actual),
                                        key,
                                        render_semantic_type(&expected)
                                    ),
                                )
                                .with_span(Span::new(
                                    node.module_path.display().to_string(),
                                    site.line,
                                    1,
                                    site.line,
                                    1,
                                )),
                            );
                        }
                        return None;
                    }

                    let actual = resolve_assignment_expression_semantic_type(
                        node,
                        nodes,
                        signature,
                        site.owner_name.as_deref(),
                        site.owner_type_name.as_deref(),
                        site.line,
                        value,
                    )?;
                    let expected = lower_type_text_or_name(field.value_type());
                    if !semantic_type_matches(node, nodes, &expected, &actual) {
                        return Some(
                            Diagnostic::error(
                                "TPY4001",
                                format!(
                                    "TypedDict item `{}` on `{}` in module `{}` assigns `{}` where `{}` expects `{}`",
                                    key,
                                    target_shape.name,
                                    node.module_path.display(),
                                    render_semantic_type(&actual),
                                    key,
                                    render_semantic_type(&expected)
                                ),
                            )
                            .with_span(Span::new(
                                node.module_path.display().to_string(),
                                site.line,
                                1,
                                site.line,
                                1,
                            )),
                        );
                    }
                }
                typepython_syntax::TypedDictMutationKind::AugmentedAssignment => {
                    let value = site.value.as_ref()?;
                    let actual = resolve_augmented_assignment_result_semantic_type(
                        node,
                        nodes,
                        signature,
                        site.owner_name.as_deref(),
                        site.owner_type_name.as_deref(),
                        site.line,
                        site.operator.as_deref(),
                        field.value_type(),
                        value,
                    )?;
                    let expected = lower_type_text_or_name(field.value_type());
                    if !semantic_type_matches(node, nodes, &expected, &actual) {
                        return Some(
                            Diagnostic::error(
                                "TPY4001",
                                format!(
                                    "augmented assignment on TypedDict item `{}` on `{}` in module `{}` produces `{}` where `{}` expects `{}`",
                                    key,
                                    target_shape.name,
                                    node.module_path.display(),
                                    render_semantic_type(&actual),
                                    key,
                                    render_semantic_type(&expected)
                                ),
                            )
                            .with_span(Span::new(
                                node.module_path.display().to_string(),
                                site.line,
                                1,
                                site.line,
                                1,
                            )),
                        );
                    }
                }
                typepython_syntax::TypedDictMutationKind::Delete => {}
            }

            None
        })
        .collect()
}

pub(super) enum WritableSubscriptSignature {
    Writable { key_type: SemanticType, value_type: SemanticType },
    ReadOnly,
}

pub(super) fn resolve_writable_subscript_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    owner_type_name: &str,
) -> Option<WritableSubscriptSignature> {
    let normalized = normalize_type_text(owner_type_name);
    if let Some((head, args)) = split_generic_type(&normalized) {
        match head {
            "Mapping" | "typing.Mapping" | "collections.abc.Mapping" if args.len() == 2 => {
                return Some(WritableSubscriptSignature::ReadOnly);
            }
            _ => {}
        }
    }

    let nominal_owner_name = split_generic_type(&normalized)
        .map(|(head, _)| head.to_owned())
        .unwrap_or_else(|| normalized.clone());
    let (class_node, class_decl) = resolve_direct_base(nodes, node, &nominal_owner_name)?;
    if let Some(setitem) =
        find_owned_callable_declaration(nodes, class_node, class_decl, "__setitem__")
    {
        let signature = rewrite_imported_typing_aliases(
            node,
            &substitute_self_annotation(&setitem.detail, Some(&normalized)),
        );
        let params = direct_param_types(&signature)?;
        let params = match setitem.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
            typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => {
                params
            }
            _ => params.into_iter().skip(1).collect(),
        };
        if params.len() == 2 {
            return Some(WritableSubscriptSignature::Writable {
                key_type: lower_type_text_or_name(&params[0]),
                value_type: lower_type_text_or_name(&params[1]),
            });
        }
    }

    find_owned_callable_declaration(nodes, class_node, class_decl, "__getitem__")
        .map(|_| WritableSubscriptSignature::ReadOnly)
}

pub(super) fn subscript_assignment_type_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_typed_dict_mutation_sites(&source)
        .into_iter()
        .filter_map(|site| {
            if site.kind == typepython_syntax::TypedDictMutationKind::Delete {
                return None;
            }

            let signature = resolve_scope_owner_signature(
                node,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
            );
            let owner_type = resolve_assignment_expression_semantic_type(
                node,
                nodes,
                signature,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
                site.line,
                &site.target,
            )?;
            let owner_type_rendered = render_semantic_type(&owner_type);

            if resolve_known_typed_dict_shape_from_type_with_context(
                context,
                node,
                nodes,
                &owner_type_rendered,
            )
            .is_some()
            {
                return None;
            }

            match resolve_writable_subscript_signature(node, nodes, &owner_type_rendered)? {
                WritableSubscriptSignature::ReadOnly => Some(
                    Diagnostic::error(
                        "TPY4001",
                        format!(
                            "subscript assignment target `{}` in module `{}` is not writable via `__setitem__`",
                            owner_type_rendered,
                            node.module_path.display(),
                        ),
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        site.line,
                        1,
                        site.line,
                        1,
                    )),
                ),
                WritableSubscriptSignature::Writable { key_type, value_type } => {
                    let actual_key = resolve_assignment_expression_semantic_type(
                        node,
                        nodes,
                        signature,
                        site.owner_name.as_deref(),
                        site.owner_type_name.as_deref(),
                        site.line,
                        &site.key_value,
                    )?;
                    if !semantic_type_is_assignable(node, nodes, &key_type, &actual_key) {
                        return Some(
                            Diagnostic::error(
                                "TPY4001",
                                format!(
                                    "subscript assignment on `{}` in module `{}` passes key `{}` where `__setitem__` expects `{}`",
                                    owner_type_rendered,
                                    node.module_path.display(),
                                    render_semantic_type(&actual_key),
                                    render_semantic_type(&key_type),
                                ),
                            )
                            .with_span(Span::new(
                                node.module_path.display().to_string(),
                                site.line,
                                1,
                                site.line,
                                1,
                            )),
                        );
                    }

                    let value = site.value.as_ref()?;
                    match site.kind {
                        typepython_syntax::TypedDictMutationKind::Assignment => {
                            let contextual = resolve_contextual_call_arg_type_with_context(
                                context,
                                node,
                                nodes,
                                site.line,
                                value,
                                Some(&render_semantic_type(&value_type)),
                            );
                            if let Some(mut result) = contextual {
                                if let Some(diagnostic) = result.diagnostics.pop() {
                                    return Some(diagnostic);
                                }
                                let actual_value = contextual_result_semantic_type(&result);
                                if !semantic_type_is_assignable(
                                    node,
                                    nodes,
                                    &value_type,
                                    &actual_value,
                                ) {
                                    return Some(
                                        Diagnostic::error(
                                            "TPY4001",
                                            format!(
                                                "subscript assignment on `{}` in module `{}` passes value `{}` where `__setitem__` expects `{}`",
                                                owner_type_rendered,
                                                node.module_path.display(),
                                                render_semantic_type(&actual_value),
                                                render_semantic_type(&value_type),
                                            ),
                                        )
                                        .with_span(Span::new(
                                            node.module_path.display().to_string(),
                                            site.line,
                                            1,
                                            site.line,
                                            1,
                                        )),
                                    );
                                }
                                return None;
                            }
                            let actual_value = resolve_assignment_expression_semantic_type(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                value,
                            )?;
                            if !semantic_type_is_assignable(node, nodes, &value_type, &actual_value)
                            {
                                return Some(
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "subscript assignment on `{}` in module `{}` passes value `{}` where `__setitem__` expects `{}`",
                                            owner_type_rendered,
                                            node.module_path.display(),
                                            render_semantic_type(&actual_value),
                                            render_semantic_type(&value_type),
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    )),
                                );
                            }
                        }
                        typepython_syntax::TypedDictMutationKind::AugmentedAssignment => {
                            let Some(readable_type) = resolve_subscript_type_from_target_semantic_type(
                                node,
                                nodes,
                                &owner_type,
                                site.key.as_deref(),
                                None,
                            ) else {
                                return Some(
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "augmented subscript assignment target `{}` in module `{}` is not readable via `__getitem__`",
                                            owner_type_rendered,
                                            node.module_path.display(),
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    )),
                                );
                            };
                            let actual_value = resolve_augmented_assignment_result_semantic_type(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                site.operator.as_deref(),
                                &render_semantic_type(&readable_type),
                                value,
                            )?;
                            if !semantic_type_is_assignable(node, nodes, &value_type, &actual_value)
                            {
                                return Some(
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "augmented subscript assignment on `{}` in module `{}` produces `{}` where `__setitem__` expects `{}`",
                                            owner_type_rendered,
                                            node.module_path.display(),
                                            render_semantic_type(&actual_value),
                                            render_semantic_type(&value_type),
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    )),
                                );
                            }
                        }
                        typepython_syntax::TypedDictMutationKind::Delete => {}
                    }

                    None
                }
            }
        })
        .collect()
}

pub(super) fn frozen_dataclass_transform_mutation_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_frozen_field_mutation_sites(&source)
        .into_iter()
        .filter_map(|site| {
            let signature = resolve_scope_owner_signature(
                node,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
            );
            let target_type = resolve_assignment_expression_semantic_type(
                node,
                nodes,
                signature,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
                site.line,
                &site.target,
            )?;
            let target_type_rendered = render_semantic_type(&target_type);
            let shape = resolve_known_dataclass_transform_shape_from_type_with_context(
                context,
                node,
                nodes,
                &target_type_rendered,
            )?;
            if !shape.frozen || !shape.fields.iter().any(|field| field.name == site.field_name) {
                return None;
            }

            let in_initializer = site.owner_name.as_deref() == Some("__init__")
                && site.owner_type_name.as_deref() == Some(target_type_rendered.as_str())
                && site.target.value_name.as_deref() == Some("self");
            if in_initializer {
                return None;
            }

            let message = match site.kind {
                typepython_syntax::FrozenFieldMutationKind::Assignment => format!(
                    "frozen dataclass-transform field `{}` on `{}` in module `{}` cannot be assigned after initialization",
                    site.field_name,
                    target_type_rendered,
                    node.module_path.display()
                ),
                typepython_syntax::FrozenFieldMutationKind::AugmentedAssignment => format!(
                    "frozen dataclass-transform field `{}` on `{}` in module `{}` cannot be updated with augmented assignment after initialization",
                    site.field_name,
                    target_type_rendered,
                    node.module_path.display()
                ),
                typepython_syntax::FrozenFieldMutationKind::Delete => format!(
                    "frozen dataclass-transform field `{}` on `{}` in module `{}` cannot be deleted after initialization",
                    site.field_name,
                    target_type_rendered,
                    node.module_path.display()
                ),
            };
            Some(Diagnostic::error("TPY4001", message).with_span(Span::new(
                node.module_path.display().to_string(),
                site.line,
                1,
                site.line,
                1,
            )))
        })
        .collect()
}

pub(super) fn frozen_plain_dataclass_mutation_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_frozen_field_mutation_sites(&source)
        .into_iter()
        .filter_map(|site| {
            let signature = resolve_scope_owner_signature(
                node,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
            );
            let target_type = resolve_assignment_expression_semantic_type(
                node,
                nodes,
                signature,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
                site.line,
                &site.target,
            )?;
            let target_type_rendered = render_semantic_type(&target_type);
            let shape =
                resolve_known_plain_dataclass_shape_from_type_with_context(
                    context,
                    node,
                    nodes,
                    &target_type_rendered,
                )?;
            if !shape.frozen || !shape.fields.iter().any(|field| field.name == site.field_name) {
                return None;
            }

            let in_initializer = site.owner_name.as_deref() == Some("__init__")
                && site.owner_type_name.as_deref() == Some(target_type_rendered.as_str())
                && site.target.value_name.as_deref() == Some("self");
            if in_initializer {
                return None;
            }

            let message = match site.kind {
                typepython_syntax::FrozenFieldMutationKind::Assignment => format!(
                    "frozen dataclass field `{}` on `{}` in module `{}` cannot be assigned after initialization",
                    site.field_name,
                    target_type_rendered,
                    node.module_path.display()
                ),
                typepython_syntax::FrozenFieldMutationKind::AugmentedAssignment => format!(
                    "frozen dataclass field `{}` on `{}` in module `{}` cannot be updated with augmented assignment after initialization",
                    site.field_name,
                    target_type_rendered,
                    node.module_path.display()
                ),
                typepython_syntax::FrozenFieldMutationKind::Delete => format!(
                    "frozen dataclass field `{}` on `{}` in module `{}` cannot be deleted after initialization",
                    site.field_name,
                    target_type_rendered,
                    node.module_path.display()
                ),
            };
            Some(Diagnostic::error("TPY4001", message).with_span(Span::new(
                node.module_path.display().to_string(),
                site.line,
                1,
                site.line,
                1,
            )))
        })
        .collect()
}

pub(super) enum WritableAttributeTarget<'a> {
    Value(&'a Declaration),
    PropertySetter(&'a Declaration),
    ReadOnlyProperty,
    NonWritable,
}

pub(super) fn find_owned_writable_member_target<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<WritableAttributeTarget<'a>> {
    if let Some(declaration) =
        find_owned_value_declaration(nodes, class_node, class_decl, member_name)
        && !declaration.is_class_var
    {
        return Some(WritableAttributeTarget::Value(declaration));
    }

    let callables = find_owned_callable_declarations(nodes, class_node, class_decl, member_name);
    if let Some(setter) = callables.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Function
            && declaration.method_kind == Some(typepython_syntax::MethodKind::PropertySetter)
    }) {
        return Some(WritableAttributeTarget::PropertySetter(setter));
    }
    if callables.iter().any(|declaration| {
        declaration.kind == DeclarationKind::Function
            && declaration.method_kind == Some(typepython_syntax::MethodKind::Property)
    }) {
        return Some(WritableAttributeTarget::ReadOnlyProperty);
    }

    Some(WritableAttributeTarget::NonWritable)
}

pub(super) fn resolve_writable_member_type(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    owner_type_name: &str,
) -> Option<SemanticType> {
    match declaration.kind {
        DeclarationKind::Value => resolve_readable_member_semantic_type(
            node,
            declaration,
            &lower_type_text_or_name(owner_type_name),
        ),
        DeclarationKind::Function
            if declaration.method_kind == Some(typepython_syntax::MethodKind::PropertySetter) =>
        {
            let signature = rewrite_imported_typing_aliases(
                node,
                &substitute_self_annotation(&declaration.detail, Some(owner_type_name)),
            );
            let params = direct_param_types(&signature)?;
            let params = params.into_iter().skip(1).collect::<Vec<_>>();
            (params.len() == 1).then(|| lower_type_text_or_name(&params[0]))
        }
        _ => None,
    }
}

pub(super) fn should_defer_attribute_assignment_to_frozen_checks(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    site: &typepython_syntax::FrozenFieldMutationSite,
    target_type: &str,
) -> bool {
    if let Some(shape) = resolve_known_dataclass_transform_shape_from_type_with_context(
        context,
        node,
        nodes,
        target_type,
    ) && shape.frozen
        && shape.fields.iter().any(|field| field.name == site.field_name)
    {
        let in_initializer = site.owner_name.as_deref() == Some("__init__")
            && site.owner_type_name.as_deref() == Some(target_type)
            && site.target.value_name.as_deref() == Some("self");
        return !in_initializer;
    }
    if let Some(shape) = resolve_known_plain_dataclass_shape_from_type_with_context(
        context,
        node,
        nodes,
        target_type,
    ) && shape.frozen
        && shape.fields.iter().any(|field| field.name == site.field_name)
    {
        let in_initializer = site.owner_name.as_deref() == Some("__init__")
            && site.owner_type_name.as_deref() == Some(target_type)
            && site.target.value_name.as_deref() == Some("self");
        return !in_initializer;
    }
    false
}

pub(super) fn attribute_assignment_type_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_frozen_field_mutation_sites(&source)
        .into_iter()
        .filter_map(|site| {
            if site.kind == typepython_syntax::FrozenFieldMutationKind::Delete {
                return None;
            }

            if site.owner_name.as_deref() == Some("__init__")
                && site.target.value_name.as_deref() == Some("self")
            {
                return None;
            }

            let signature = resolve_scope_owner_signature(
                node,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
            );
            let target_type = resolve_assignment_expression_semantic_type(
                node,
                nodes,
                signature,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
                site.line,
                &site.target,
            )?;
            let target_type_rendered = render_semantic_type(&target_type);

            if should_defer_attribute_assignment_to_frozen_checks(
                context,
                node,
                nodes,
                &site,
                &target_type_rendered,
            ) {
                return None;
            }

            let (class_node, class_decl) =
                resolve_direct_base(nodes, node, &target_type_rendered)?;
            match find_owned_writable_member_target(nodes, class_node, class_decl, &site.field_name) {
                Some(WritableAttributeTarget::Value(declaration)) => {
                    if declaration.is_final {
                        return Some(final_attribute_reassignment_diagnostic(
                            &node.module_path,
                            &target_type_rendered,
                            &site.field_name,
                        ));
                    }
                    let expected =
                        resolve_writable_member_type(node, declaration, &target_type_rendered)?;
                    let value = site.value.as_ref()?;
                    match site.kind {
                        typepython_syntax::FrozenFieldMutationKind::Assignment => {
                            let contextual = resolve_contextual_call_arg_type_with_context(
                                context,
                                node,
                                nodes,
                                site.line,
                                value,
                                Some(&render_semantic_type(&expected)),
                            );
                            if let Some(mut result) = contextual {
                                if let Some(diagnostic) = result.diagnostics.pop() {
                                    return Some(diagnostic);
                                }
                                let actual = contextual_result_semantic_type(&result);
                                return (!semantic_type_matches(node, nodes, &expected, &actual))
                                    .then(|| {
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "attribute assignment on `{}` in module `{}` assigns `{}` where member `{}` expects `{}`",
                                            target_type_rendered,
                                            node.module_path.display(),
                                            render_semantic_type(&actual),
                                            site.field_name,
                                            render_semantic_type(&expected),
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    ))
                                });
                            }
                            let actual = resolve_assignment_expression_semantic_type(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                value,
                            )?;
                            (!semantic_type_matches(node, nodes, &expected, &actual)).then(|| {
                                Diagnostic::error(
                                    "TPY4001",
                                    format!(
                                        "attribute assignment on `{}` in module `{}` assigns `{}` where member `{}` expects `{}`",
                                        target_type_rendered,
                                        node.module_path.display(),
                                        render_semantic_type(&actual),
                                        site.field_name,
                                        render_semantic_type(&expected),
                                    ),
                                )
                                .with_span(Span::new(
                                    node.module_path.display().to_string(),
                                    site.line,
                                    1,
                                    site.line,
                                    1,
                                ))
                            })
                        }
                        typepython_syntax::FrozenFieldMutationKind::AugmentedAssignment => {
                            let actual = resolve_augmented_assignment_result_semantic_type(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                site.operator.as_deref(),
                                &render_semantic_type(&expected),
                                value,
                            )?;
                            (!semantic_type_matches(node, nodes, &expected, &actual)).then(|| {
                                Diagnostic::error(
                                    "TPY4001",
                                    format!(
                                        "augmented attribute assignment on `{}` in module `{}` produces `{}` where member `{}` expects `{}`",
                                        target_type_rendered,
                                        node.module_path.display(),
                                        render_semantic_type(&actual),
                                        site.field_name,
                                        render_semantic_type(&expected),
                                    ),
                                )
                                .with_span(Span::new(
                                    node.module_path.display().to_string(),
                                    site.line,
                                    1,
                                    site.line,
                                    1,
                                ))
                            })
                        }
                        typepython_syntax::FrozenFieldMutationKind::Delete => None,
                    }
                }
                Some(WritableAttributeTarget::PropertySetter(declaration)) => {
                    let expected =
                        resolve_writable_member_type(node, declaration, &target_type_rendered)?;
                    let value = site.value.as_ref()?;
                    match site.kind {
                        typepython_syntax::FrozenFieldMutationKind::Assignment => {
                            let contextual = resolve_contextual_call_arg_type_with_context(
                                context,
                                node,
                                nodes,
                                site.line,
                                value,
                                Some(&render_semantic_type(&expected)),
                            );
                            if let Some(mut result) = contextual {
                                if let Some(diagnostic) = result.diagnostics.pop() {
                                    return Some(diagnostic);
                                }
                                let actual = contextual_result_semantic_type(&result);
                                return (!semantic_type_matches(node, nodes, &expected, &actual))
                                    .then(|| {
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "attribute assignment on `{}` in module `{}` assigns `{}` where member `{}` expects `{}`",
                                            target_type_rendered,
                                            node.module_path.display(),
                                            render_semantic_type(&actual),
                                            site.field_name,
                                            render_semantic_type(&expected),
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    ))
                                });
                            }
                            let actual = resolve_assignment_expression_semantic_type(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                value,
                            )?;
                            (!semantic_type_matches(node, nodes, &expected, &actual)).then(|| {
                                Diagnostic::error(
                                    "TPY4001",
                                    format!(
                                        "attribute assignment on `{}` in module `{}` assigns `{}` where member `{}` expects `{}`",
                                        target_type_rendered,
                                        node.module_path.display(),
                                        render_semantic_type(&actual),
                                        site.field_name,
                                        render_semantic_type(&expected),
                                    ),
                                )
                                .with_span(Span::new(
                                    node.module_path.display().to_string(),
                                    site.line,
                                    1,
                                    site.line,
                                    1,
                                ))
                            })
                        }
                        typepython_syntax::FrozenFieldMutationKind::AugmentedAssignment => {
                            let Some(readable) = find_owned_readable_member_declaration(
                                nodes,
                                class_node,
                                class_decl,
                                &site.field_name,
                            ) else {
                                return Some(
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "attribute `{}` on `{}` in module `{}` is not readable for augmented assignment",
                                            site.field_name,
                                            target_type_rendered,
                                            node.module_path.display(),
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    )),
                                );
                            };
                            let readable_type =
                                resolve_readable_member_semantic_type(node, readable, &target_type)?;
                            let actual = resolve_augmented_assignment_result_semantic_type(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                site.operator.as_deref(),
                                &render_semantic_type(&readable_type),
                                value,
                            )?;
                            (!semantic_type_matches(node, nodes, &expected, &actual)).then(|| {
                                Diagnostic::error(
                                    "TPY4001",
                                    format!(
                                        "augmented attribute assignment on `{}` in module `{}` produces `{}` where member `{}` expects `{}`",
                                        target_type_rendered,
                                        node.module_path.display(),
                                        render_semantic_type(&actual),
                                        site.field_name,
                                        render_semantic_type(&expected),
                                    ),
                                )
                                .with_span(Span::new(
                                    node.module_path.display().to_string(),
                                    site.line,
                                    1,
                                    site.line,
                                    1,
                                ))
                            })
                        }
                        typepython_syntax::FrozenFieldMutationKind::Delete => None,
                    }
                }
                Some(WritableAttributeTarget::ReadOnlyProperty) => Some(
                    Diagnostic::error(
                        "TPY4001",
                        format!(
                            "property `{}` on `{}` in module `{}` is not writable",
                            site.field_name,
                            target_type_rendered,
                            node.module_path.display(),
                        ),
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        site.line,
                        1,
                        site.line,
                        1,
                    )),
                ),
                Some(WritableAttributeTarget::NonWritable) | None => None,
            }
        })
        .collect()
}

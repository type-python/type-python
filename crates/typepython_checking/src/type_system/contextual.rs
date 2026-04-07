pub(super) fn resolve_direct_expression_semantic_type_from_metadata(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
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
            None,
        );
    }
    let value_if_guard = metadata.value_if_guard.as_ref().map(guard_to_site);
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
        value_if_guard.as_ref(),
        metadata.value_bool_left.as_deref(),
        metadata.value_bool_right.as_deref(),
        metadata.value_binop_left.as_deref(),
        metadata.value_binop_right.as_deref(),
        metadata.value_binop_operator.as_deref(),
    )
}

pub(super) fn resolve_known_typed_dict_shape_from_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<TypedDictShape> {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    resolve_known_typed_dict_shape_with_context(context, node, nodes, &type_name)
}

pub(super) fn resolve_known_typed_dict_shape_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<TypedDictShape> {
    let (class_node, class_decl) = resolve_direct_base(nodes, node, type_name)?;
    if !is_typed_dict_class(nodes, class_node, class_decl, &mut BTreeSet::new()) {
        return None;
    }

    let typed_dict_metadata = load_typed_dict_class_metadata(context, class_node);
    let mut fields = BTreeMap::new();
    collect_typed_dict_fields(
        context,
        nodes,
        class_node,
        class_decl,
        &typed_dict_metadata,
        &mut BTreeSet::new(),
        &mut fields,
    );
    let (closed, extra_items) = collect_typed_dict_openness(
        context,
        node,
        nodes,
        class_node,
        class_decl,
        &mut BTreeSet::new(),
    )?;
    Some(TypedDictShape { name: class_decl.name.clone(), fields, closed, extra_items })
}

pub(super) fn is_typed_dict_class(
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visited: &mut BTreeSet<(String, String)>,
) -> bool {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visited.insert(key) {
        return false;
    }

    is_typed_dict_base_name(&class_decl.name)
        || class_decl.bases.iter().any(|base| {
            is_typed_dict_base_name(base)
                || resolve_direct_base(nodes, class_node, base).is_some_and(
                    |(base_node, base_decl)| {
                        is_typed_dict_class(nodes, base_node, base_decl, visited)
                    },
                )
        })
}

pub(super) fn collect_typed_dict_fields(
    context: &CheckerContext<'_>,
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    typed_dict_metadata: &BTreeMap<String, typepython_syntax::TypedDictClassMetadata>,
    visited: &mut BTreeSet<(String, String)>,
    fields: &mut BTreeMap<String, TypedDictFieldShape>,
) {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visited.insert(key) {
        return;
    }

    for base in &class_decl.bases {
        if let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) {
            if is_typed_dict_class(nodes, base_node, base_decl, &mut BTreeSet::new()) {
                collect_typed_dict_fields(
                    context,
                    nodes,
                    base_node,
                    base_decl,
                    &load_typed_dict_class_metadata(context, base_node),
                    visited,
                    fields,
                );
            }
        }
    }

    let total_default = typed_dict_metadata
        .get(&class_decl.name)
        .and_then(|metadata| metadata.total)
        .unwrap_or(true);
    for declaration in class_node.declarations.iter().filter(|declaration| {
        declaration.kind == DeclarationKind::Value
            && declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration_value_annotation_text(declaration).is_some()
    }) {
        fields.insert(
            declaration.name.clone(),
            parse_typed_dict_field_shape(
                &rewrite_imported_typing_aliases(
                    class_node,
                    &declaration_value_annotation_text(declaration).unwrap_or_default(),
                ),
                total_default,
            ),
        );
    }
}

pub(super) fn collect_typed_dict_openness(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visited: &mut BTreeSet<(String, String)>,
) -> Option<(bool, Option<TypedDictExtraItemsShape>)> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visited.insert(key) {
        return Some((false, None));
    }

    let mut inherited_closed = false;
    let mut inherited_extra_items = None;
    for base in &class_decl.bases {
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) else {
            continue;
        };
        if !is_typed_dict_class(nodes, base_node, base_decl, &mut BTreeSet::new()) {
            continue;
        }
        let (base_closed, base_extra_items) =
            collect_typed_dict_openness(context, node, nodes, base_node, base_decl, visited)?;
        inherited_closed |= base_closed;
        if inherited_extra_items.is_none() {
            inherited_extra_items = base_extra_items;
        }
    }

    let metadata = load_typed_dict_class_metadata(context, class_node);
    let metadata = metadata.get(&class_decl.name);
    let mut closed = inherited_closed;
    let mut extra_items = inherited_extra_items;

    if let Some(annotation) = metadata.and_then(|metadata| metadata.extra_items.as_ref()) {
        if let Some(parsed) = parse_typed_dict_extra_items(node, &annotation.annotation) {
            if parsed.value_type == "Never" {
                closed = true;
                extra_items = None;
            } else {
                closed = false;
                extra_items = Some(parsed);
            }
        }
    }

    if let Some(explicit_closed) = metadata.and_then(|metadata| metadata.closed) {
        if explicit_closed {
            closed = true;
            extra_items = None;
        } else if extra_items.is_none() {
            closed = false;
        }
    }

    Some((closed, extra_items))
}

pub(super) fn load_typed_dict_class_metadata(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<String, typepython_syntax::TypedDictClassMetadata> {
    context.load_typed_dict_class_metadata(node)
}

pub(super) fn parse_typed_dict_extra_items(
    node: &typepython_graph::ModuleNode,
    annotation: &str,
) -> Option<TypedDictExtraItemsShape> {
    let mut value_type = normalize_type_text(&rewrite_imported_typing_aliases(node, annotation));
    let mut readonly = false;

    if let Some(inner) =
        value_type.strip_prefix("ReadOnly[").and_then(|inner| inner.strip_suffix(']'))
    {
        value_type = normalize_type_text(inner);
        readonly = true;
    }

    Some(TypedDictExtraItemsShape { value_type, readonly })
}

pub(super) fn parse_typed_dict_field_shape(
    annotation: &str,
    total_default: bool,
) -> TypedDictFieldShape {
    let mut value_type = normalize_type_text(annotation);
    let mut required = total_default;
    let mut readonly = false;

    loop {
        if let Some(inner) =
            value_type.strip_prefix("Required[").and_then(|inner| inner.strip_suffix(']'))
        {
            value_type = normalize_type_text(inner);
            required = true;
            continue;
        }
        if let Some(inner) =
            value_type.strip_prefix("NotRequired[").and_then(|inner| inner.strip_suffix(']'))
        {
            value_type = normalize_type_text(inner);
            required = false;
            continue;
        }
        if let Some(inner) =
            value_type.strip_prefix("ReadOnly[").and_then(|inner| inner.strip_suffix(']'))
        {
            value_type = normalize_type_text(inner);
            readonly = true;
            continue;
        }
        break;
    }

    TypedDictFieldShape { value_type, required, readonly }
}

pub(super) fn callable_assignment_result(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    assignment: &typepython_binding::AssignmentSite,
    expected: &str,
) -> Option<Option<Diagnostic>> {
    let (expected_params, expected_return) = parse_callable_annotation(expected)?;
    let expected_params = expected_params.map(|params| {
        params.into_iter().map(|param| lower_type_text_or_name(&param)).collect::<Vec<_>>()
    });
    let expected_return = lower_type_text_or_name(&expected_return);
    let (actual_params, actual_return) =
        resolve_callable_assignment_semantic_signature(node, nodes, assignment)?;

    let params_match = expected_params.as_ref().is_none_or(|expected_params| {
        expected_params.len() == actual_params.len()
            && expected_params.iter().zip(actual_params.iter()).all(
                |(expected_param, actual_param)| {
                    semantic_type_is_assignable(node, nodes, expected_param, actual_param)
                },
            )
    });

    let matches = params_match
        && semantic_type_is_assignable(node, nodes, &expected_return, &actual_return);

    Some((!matches).then(|| {
        let actual_signature = format_semantic_assignment_signature(&actual_params, &actual_return);
        Diagnostic::error(
            "TPY4001",
            match (&assignment.owner_type_name, &assignment.owner_name) {
                (Some(owner_type_name), Some(owner_name)) => format!(
                    "type `{}` in module `{}` assigns callable `{}` where local `{}` in `{}` expects `{}`",
                    owner_type_name,
                    node.module_path.display(),
                    actual_signature,
                    assignment.name,
                    owner_name,
                    expected
                ),
                (None, Some(owner_name)) => format!(
                    "function `{}` in module `{}` assigns callable `{}` where local `{}` expects `{}`",
                    owner_name,
                    node.module_path.display(),
                    actual_signature,
                    assignment.name,
                    expected
                ),
                _ => format!(
                    "module `{}` assigns callable `{}` where `{}` expects `{}`",
                    node.module_path.display(),
                    actual_signature,
                    assignment.name,
                    expected
                ),
            },
        )
    }))
}

fn format_semantic_assignment_signature(
    param_types: &[SemanticType],
    return_type: &SemanticType,
) -> String {
    diagnostic_type_text(&SemanticType::Callable {
        params: SemanticCallableParams::ParamList(param_types.to_vec()),
        return_type: Box::new(return_type.clone()),
    })
}

pub(super) fn resolve_callable_assignment_semantic_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    assignment: &typepython_binding::AssignmentSite,
) -> Option<(Vec<SemanticType>, SemanticType)> {
    if let Some(lambda) = assignment.value_lambda.as_deref() {
    let expected = normalized_assignment_annotation(assignment.annotation_text()?)?;
        return resolve_contextual_lambda_callable_semantic_signature(
            node,
            nodes,
            assignment.owner_name.as_deref(),
            assignment.owner_type_name.as_deref(),
            assignment.line,
            lambda,
            Some(expected),
            None,
        );
    }

    if let Some(value_name) = assignment.value_name.as_deref() {
        let function = resolve_direct_function(node, nodes, value_name)?;
        let actual_params = declaration_semantic_signature_params(function)
            .unwrap_or_default()
            .into_iter()
            .map(|param| param.annotation_or_dynamic())
            .collect::<Vec<_>>();
        let actual_return = resolve_direct_callable_return_semantic_type(node, nodes, value_name)?;
        return Some((actual_params, actual_return));
    }

    let owner_name = assignment.value_member_owner_name.as_deref()?;
    let member_name = assignment.value_member_name.as_deref()?;
    resolve_direct_member_callable_semantic_signature(
        node,
        nodes,
        assignment.owner_name.as_deref(),
        assignment.owner_type_name.as_deref(),
        assignment.line,
        owner_name,
        member_name,
        assignment.value_member_through_instance,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "member callable resolution needs the current scope and member context"
)]
pub(super) fn resolve_direct_member_callable_semantic_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    owner_name: &str,
    member_name: &str,
    through_instance: bool,
) -> Option<(Vec<SemanticType>, SemanticType)> {
    let owner_type = if through_instance {
        resolve_direct_callable_return_semantic_type(node, nodes, owner_name)
            .or_else(|| Some(lower_type_text_or_name(owner_name)))
    } else {
        resolve_direct_name_reference_semantic_type(
            node,
            nodes,
            None,
            None,
            current_owner_name,
            current_owner_type_name,
            current_line,
            owner_name,
        )
        .or_else(|| Some(lower_type_text_or_name(owner_name)))
    }?;
    let owner_type_name = semantic_nominal_owner_name(&owner_type)?;

    let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
    let method =
        find_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
            matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })?;

    let (actual_params, actual_return) = if let Some(callable_type) =
        resolve_decorated_callable_semantic_type_for_declaration_with_context(
            &CheckerContext::new(nodes, ImportFallback::Unknown, None),
            class_node,
            nodes,
            method,
        )
    {
        let (params, return_type) = callable_type.callable_parts()?;
        let SemanticCallableParams::ParamList(params) = params else {
            return None;
        };
        (
            params.clone(),
            return_type.clone(),
        )
    } else {
        let actual_params = declaration_semantic_signature_params_with_self(method, &owner_type_name)
            .unwrap_or_default()
            .into_iter()
            .map(|param| rewrite_imported_typing_semantic_type(node, &param.annotation_or_dynamic()))
            .collect::<Vec<_>>();
        let actual_return = rewrite_imported_typing_semantic_type(
            node,
            &declaration_signature_return_semantic_type_with_self(method, &owner_type_name)?,
        );
        (actual_params, actual_return)
    };
    let bound_params = match method.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
        typepython_syntax::MethodKind::Static => actual_params,
        typepython_syntax::MethodKind::Property => return None,
        typepython_syntax::MethodKind::Instance
        | typepython_syntax::MethodKind::Class
        | typepython_syntax::MethodKind::PropertySetter => {
            actual_params.into_iter().skip(1).collect()
        }
    };
    Some((bound_params, actual_return))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_contextual_lambda_callable_semantic_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    lambda: &typepython_syntax::LambdaMetadata,
    expected: Option<&str>,
    outer_bindings: Option<&BTreeMap<String, SemanticType>>,
) -> Option<(Vec<SemanticType>, SemanticType)> {
    let expected_params = expected
        .and_then(parse_callable_annotation)
        .and_then(|(expected_params, _)| expected_params)
        .map(|params| params.into_iter().map(|ty| lower_type_text_or_name(&ty)).collect::<Vec<_>>());
    if let Some(expected_params) = expected_params.as_ref()
        && expected_params.len() != lambda.params.len()
    {
        return Some((
            vec![SemanticType::Name(String::from("dynamic")); lambda.params.len()],
            SemanticType::Name(String::from("dynamic")),
        ));
    }
    let param_types = lambda
        .params
        .iter()
        .enumerate()
        .map(|(index, param)| {
            param
                .annotation
                .as_deref()
                .map(lower_type_text_or_name)
                .or_else(|| {
                    expected_params
                        .as_ref()
                        .and_then(|expected_params| expected_params.get(index).cloned())
                })
                .unwrap_or_else(|| SemanticType::Name(String::from("dynamic")))
        })
        .collect::<Vec<_>>();
    let mut local_bindings = outer_bindings.cloned().unwrap_or_default();
    local_bindings.extend(
        lambda.params.iter().map(|param| param.name.clone()).zip(param_types.iter().cloned()),
    );
    let actual_return = resolve_direct_expression_semantic_type_from_metadata_with_bindings(
        node,
        nodes,
        None,
        current_owner_name,
        current_owner_type_name,
        current_line,
        &lambda.body,
        &local_bindings,
    )?;
    Some((param_types, actual_return))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_contextual_lambda_callable_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    lambda: &typepython_syntax::LambdaMetadata,
    expected: Option<&str>,
    outer_bindings: Option<&BTreeMap<String, SemanticType>>,
) -> Option<SemanticType> {
    let (param_types, return_type) = resolve_contextual_lambda_callable_semantic_signature(
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        current_line,
        lambda,
        expected,
        outer_bindings,
    )?;
    Some(SemanticType::Callable {
        params: SemanticCallableParams::ParamList(param_types),
        return_type: Box::new(return_type),
    })
}

pub(super) struct ContextualTypedDictLiteralSemanticResult {
    pub(super) actual_type: SemanticType,
    pub(super) diagnostics: Vec<Diagnostic>,
}

pub(super) struct ContextualCallArgSemanticResult {
    pub(super) actual_type: SemanticType,
    pub(super) diagnostics: Vec<Diagnostic>,
}

pub(super) fn resolve_contextual_typed_dict_literal_semantic_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&str>,
) -> Option<ContextualTypedDictLiteralSemanticResult> {
    let entries = metadata.value_dict_entries.as_ref()?;
    let actual_type = lower_type_text_or_name(expected?);
    let target_shape =
        resolve_known_typed_dict_shape_from_type_with_context(
            context,
            node,
            nodes,
            &render_semantic_type(&actual_type),
        )?;
    let diagnostics = typed_dict_literal_entry_diagnostics(
        context,
        node,
        nodes,
        current_line,
        entries,
        &target_shape,
        None,
        None,
        None,
    );
    Some(ContextualTypedDictLiteralSemanticResult { actual_type, diagnostics })
}

pub(super) fn resolve_contextual_collection_literal_semantic_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&str>,
) -> Option<ContextualCallArgSemanticResult> {
    resolve_contextual_collection_literal_semantic_type_in_scope_with_context(
        context,
        node,
        nodes,
        None,
        None,
        None,
        current_line,
        metadata,
        expected,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "collection literal contextual typing needs optional scope context"
)]
pub(super) fn resolve_contextual_collection_literal_semantic_type_in_scope_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&str>,
) -> Option<ContextualCallArgSemanticResult> {
    let resolve_fallback = |metadata: &typepython_syntax::DirectExprMetadata| {
        resolve_direct_expression_semantic_type_from_metadata(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            metadata,
        )
    };

    let expected = lower_type_text_or_name(expected?);
    let (head, args) = expected.generic_parts()?;
    match head {
        "list" if args.len() == 1 => {
            let elements = metadata.value_list_elements.as_ref()?;
            let diagnostics = elements
                .iter()
                .flat_map(|element| {
                    resolve_contextual_call_arg_semantic_type_with_context(
                        context,
                        node,
                        nodes,
                        current_line,
                        element,
                        Some(&render_semantic_type(&args[0])),
                    )
                    .into_iter()
                    .flat_map(|result| result.diagnostics)
                })
                .collect::<Vec<_>>();
            let actual_element_types = if elements.is_empty() {
                vec![args[0].clone()]
            } else {
                elements
                    .iter()
                    .map(|element| {
                        resolve_contextual_call_arg_semantic_type_with_context(
                            context,
                            node,
                            nodes,
                            current_line,
                            element,
                            Some(&render_semantic_type(&args[0])),
                        )
                        .map(|result| result.actual_type)
                        .or_else(|| resolve_fallback(element))
                        .unwrap_or_else(|| SemanticType::Name(String::from("Any")))
                    })
                    .collect::<Vec<_>>()
            };
            Some(ContextualCallArgSemanticResult {
                actual_type: SemanticType::Generic {
                    head: String::from("list"),
                    args: vec![join_semantic_type_candidates(actual_element_types)],
                },
                diagnostics,
            })
        }
        "set" if args.len() == 1 => {
            let elements = metadata.value_set_elements.as_ref()?;
            let diagnostics = elements
                .iter()
                .flat_map(|element| {
                    resolve_contextual_call_arg_semantic_type_with_context(
                        context,
                        node,
                        nodes,
                        current_line,
                        element,
                        Some(&render_semantic_type(&args[0])),
                    )
                    .into_iter()
                    .flat_map(|result| result.diagnostics)
                })
                .collect::<Vec<_>>();
            let actual_element_types = if elements.is_empty() {
                vec![args[0].clone()]
            } else {
                elements
                    .iter()
                    .map(|element| {
                        resolve_contextual_call_arg_semantic_type_with_context(
                            context,
                            node,
                            nodes,
                            current_line,
                            element,
                            Some(&render_semantic_type(&args[0])),
                        )
                        .map(|result| result.actual_type)
                        .or_else(|| resolve_fallback(element))
                        .unwrap_or_else(|| SemanticType::Name(String::from("Any")))
                    })
                    .collect::<Vec<_>>()
            };
            Some(ContextualCallArgSemanticResult {
                actual_type: SemanticType::Generic {
                    head: String::from("set"),
                    args: vec![join_semantic_type_candidates(actual_element_types)],
                },
                diagnostics,
            })
        }
        "dict" if args.len() == 2 => {
            let entries = metadata.value_dict_entries.as_ref()?;
            if entries.iter().any(|entry| entry.is_expansion) {
                return None;
            }
            let diagnostics = entries
                .iter()
                .flat_map(|entry| {
                    let key_diagnostics = entry
                        .key_value
                        .as_deref()
                        .and_then(|key| {
                            resolve_contextual_call_arg_semantic_type_with_context(
                                context,
                                node,
                                nodes,
                                current_line,
                                key,
                                Some(&render_semantic_type(&args[0])),
                            )
                        })
                        .into_iter()
                        .flat_map(|result| result.diagnostics);
                    let value_diagnostics = resolve_contextual_call_arg_semantic_type_with_context(
                        context,
                        node,
                        nodes,
                        current_line,
                        &entry.value,
                        Some(&render_semantic_type(&args[1])),
                    )
                    .into_iter()
                    .flat_map(|result| result.diagnostics);
                    key_diagnostics.chain(value_diagnostics)
                })
                .collect::<Vec<_>>();
            let actual_key_types = if entries.is_empty() {
                vec![args[0].clone()]
            } else {
                entries
                    .iter()
                    .map(|entry| {
                        entry
                            .key_value
                            .as_deref()
                            .and_then(|key| {
                                resolve_contextual_call_arg_semantic_type_with_context(
                                    context,
                                    node,
                                    nodes,
                                    current_line,
                                    key,
                                    Some(&render_semantic_type(&args[0])),
                                )
                            })
                            .map(|result| result.actual_type)
                            .or_else(|| entry.key_value.as_deref().and_then(resolve_fallback))
                            .unwrap_or_else(|| SemanticType::Name(String::from("Any")))
                    })
                    .collect::<Vec<_>>()
            };
            let actual_value_types = if entries.is_empty() {
                vec![args[1].clone()]
            } else {
                entries
                    .iter()
                    .map(|entry| {
                        resolve_contextual_call_arg_semantic_type_with_context(
                            context,
                            node,
                            nodes,
                            current_line,
                            &entry.value,
                            Some(&render_semantic_type(&args[1])),
                        )
                        .map(|result| result.actual_type)
                        .or_else(|| resolve_fallback(&entry.value))
                        .unwrap_or_else(|| SemanticType::Name(String::from("Any")))
                    })
                    .collect::<Vec<_>>()
            };
            Some(ContextualCallArgSemanticResult {
                actual_type: SemanticType::Generic {
                    head: String::from("dict"),
                    args: vec![
                        join_semantic_type_candidates(actual_key_types),
                        join_semantic_type_candidates(actual_value_types),
                    ],
                },
                diagnostics,
            })
        }
        _ => None,
    }
}

pub(super) fn resolve_contextual_call_arg_semantic_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&str>,
) -> Option<ContextualCallArgSemanticResult> {
    if let Some(lambda) = metadata.value_lambda.as_deref()
        && let Some(actual_type) = resolve_contextual_lambda_callable_semantic_type(
            node,
            nodes,
            None,
            None,
            current_line,
            lambda,
            expected,
            None,
        )
    {
        return Some(ContextualCallArgSemanticResult { actual_type, diagnostics: Vec::new() });
    }
    if let Some(actual_type) = resolve_contextual_named_callable_semantic_type(node, nodes, metadata, expected) {
        return Some(ContextualCallArgSemanticResult { actual_type, diagnostics: Vec::new() });
    }
    if let Some(result) = resolve_contextual_typed_dict_literal_semantic_type_with_context(
        context,
        node,
        nodes,
        current_line,
        metadata,
        expected,
    ) {
        return Some(ContextualCallArgSemanticResult {
            actual_type: result.actual_type,
            diagnostics: result.diagnostics,
        });
    }
    resolve_contextual_collection_literal_semantic_type_with_context(
        context,
        node,
        nodes,
        current_line,
        metadata,
        expected,
    )
}

pub(super) fn resolve_contextual_call_arg_semantic_type_with_expected_semantic(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&SemanticType>,
) -> Option<ContextualCallArgSemanticResult> {
    let expected_text = expected.map(diagnostic_type_text);
    if let Some(lambda) = metadata.value_lambda.as_deref()
        && let Some(actual_type) = resolve_contextual_lambda_callable_semantic_type(
            node,
            nodes,
            None,
            None,
            current_line,
            lambda,
            expected_text.as_deref(),
            None,
        )
    {
        return Some(ContextualCallArgSemanticResult { actual_type, diagnostics: Vec::new() });
    }
    if let Some(actual_type) =
        resolve_contextual_named_callable_semantic_type_with_expected_semantic(
            node, nodes, metadata, expected,
        )
    {
        return Some(ContextualCallArgSemanticResult { actual_type, diagnostics: Vec::new() });
    }
    if let Some(result) = resolve_contextual_typed_dict_literal_semantic_type_with_context(
        context,
        node,
        nodes,
        current_line,
        metadata,
        expected_text.as_deref(),
    ) {
        return Some(ContextualCallArgSemanticResult {
            actual_type: result.actual_type,
            diagnostics: result.diagnostics,
        });
    }
    resolve_contextual_collection_literal_semantic_type_with_context(
        context,
        node,
        nodes,
        current_line,
        metadata,
        expected_text.as_deref(),
    )
}

pub(super) fn resolve_contextual_named_callable_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&str>,
) -> Option<SemanticType> {
    parse_callable_annotation_parts(expected?)?;
    let function_name = metadata.value_name.as_deref()?;
    let function = resolve_direct_function(node, nodes, function_name)?;
    let param_types = declaration_semantic_signature_params(function)
        .unwrap_or_default()
        .into_iter()
        .map(|param| param.annotation_or_dynamic())
        .collect::<Vec<_>>();
    let return_type = resolve_direct_callable_return_semantic_type(node, nodes, function_name)?;
    Some(SemanticType::Callable {
        params: SemanticCallableParams::ParamList(param_types),
        return_type: Box::new(return_type),
    })
}

pub(super) fn resolve_contextual_named_callable_semantic_type_with_expected_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&SemanticType>,
) -> Option<SemanticType> {
    expected?.callable_parts()?;
    let function_name = metadata.value_name.as_deref()?;
    let function = resolve_direct_function(node, nodes, function_name)?;
    let param_types = declaration_semantic_signature_params(function)
        .unwrap_or_default()
        .into_iter()
        .map(|param| param.annotation_or_dynamic())
        .collect::<Vec<_>>();
    let return_type = resolve_direct_callable_return_semantic_type(node, nodes, function_name)?;
    Some(SemanticType::Callable {
        params: SemanticCallableParams::ParamList(param_types),
        return_type: Box::new(return_type),
    })
}

pub(super) fn expected_positional_arg_types_from_semantic_params(
    params: &[SemanticCallableParam],
    arg_count: usize,
) -> Vec<Option<String>> {
    let positional_params = params
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .collect::<Vec<_>>();
    let variadic_type = params
        .iter()
        .find(|param| param.variadic)
        .and_then(|param| param.annotation_text.clone());

    (0..arg_count)
        .map(|index| {
            positional_params
                .get(index)
                .and_then(|param| param.annotation_text.clone())
                .or_else(|| variadic_type.clone())
        })
        .collect()
}

pub(super) fn expected_positional_arg_types_from_signature_sites(
    signature: &[typepython_syntax::DirectFunctionParamSite],
    arg_count: usize,
) -> Vec<Option<String>> {
    let positional_params = signature
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .collect::<Vec<_>>();
    let variadic_type =
        signature.iter().find(|param| param.variadic).and_then(|param| param.annotation.clone());

    (0..arg_count)
        .map(|index| {
            positional_params
                .get(index)
                .and_then(|param| param.annotation.clone())
                .or_else(|| variadic_type.clone())
        })
        .collect()
}

pub(super) fn expected_keyword_arg_types_from_signature_sites(
    signature: &[typepython_syntax::DirectFunctionParamSite],
    keyword_names: &[String],
) -> Vec<Option<String>> {
    let keyword_variadic_type = signature
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| param.annotation.clone());

    keyword_names
        .iter()
        .map(|keyword| {
            signature
                .iter()
                .find(|param| param.name == *keyword && !param.positional_only)
                .and_then(|param| param.annotation.clone())
                .or_else(|| keyword_variadic_type.clone())
        })
        .collect()
}

pub(super) fn resolve_scope_owner_declaration<'a>(
    node: &'a typepython_graph::ModuleNode,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<&'a Declaration> {
    let owner_name = owner_name?;
    node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Function
            && declaration.name == owner_name
            && match (owner_type_name, &declaration.owner) {
                (Some(owner_type_name), Some(owner)) => owner.name == *owner_type_name,
                (None, None) => true,
                _ => false,
            }
    })
}

pub(super) fn normalized_direct_return_annotation(annotation: &str) -> Option<&str> {
    let annotation = annotation.trim();
    (!annotation.is_empty()).then_some(annotation)
}

pub(super) fn substitute_self_annotation(text: &str, owner_type_name: Option<&str>) -> String {
    let Some(owner_type_name) = owner_type_name else {
        return text.trim().to_owned();
    };

    let mut output = String::new();
    let mut token = String::new();
    for character in text.trim().chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            token.push(character);
            continue;
        }
        if !token.is_empty() {
            if token == "Self" {
                output.push_str(owner_type_name);
            } else {
                output.push_str(&token);
            }
            token.clear();
        }
        output.push(character);
    }
    if !token.is_empty() {
        if token == "Self" {
            output.push_str(owner_type_name);
        } else {
            output.push_str(&token);
        }
    }
    output
}

pub(super) fn rewrite_imported_typing_aliases(
    node: &typepython_graph::ModuleNode,
    text: &str,
) -> String {
    let mut output = String::new();
    let mut token = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            token.push(character);
            continue;
        }
        if !token.is_empty() {
            output.push_str(&rewrite_imported_typing_token(node, &token));
            token.clear();
        }
        output.push(character);
    }
    if !token.is_empty() {
        output.push_str(&rewrite_imported_typing_token(node, &token));
    }
    output
}

pub(super) fn rewrite_imported_typing_token(
    node: &typepython_graph::ModuleNode,
    token: &str,
) -> String {
    let Some(import_decl) = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == token
    }) else {
        return token.to_owned();
    };

    let Some(import_target) = declaration_import_target_ref(import_decl) else {
        return token.to_owned();
    };
    let Some(symbol_target) = import_target.symbol_target else {
        return token.to_owned();
    };
    if matches!(
        symbol_target.module_key.as_str(),
        "typing" | "typing_extensions" | "collections.abc"
    )
        && matches!(
            symbol_target.symbol_name.as_str(),
            "Annotated"
                | "Any"
                | "Awaitable"
                | "Callable"
                | "ClassVar"
                | "Concatenate"
                | "Coroutine"
                | "Final"
                | "Generator"
                | "Literal"
                | "NewType"
                | "NotRequired"
                | "Optional"
                | "ParamSpec"
                | "Protocol"
                | "ReadOnly"
                | "Required"
                | "Sequence"
                | "TypeGuard"
                | "TypeIs"
                | "TypeVar"
                | "TypeVarTuple"
                | "TypedDict"
                | "Union"
                | "Unpack"
        )
    {
        return symbol_target.symbol_name;
    }

    token.to_owned()
}

pub(super) fn normalized_assignment_annotation(annotation: &str) -> Option<&str> {
    let annotation = annotation.trim();
    if annotation.is_empty() {
        return None;
    }
    if let Some(inner) = annotation.strip_prefix("Final[").and_then(|inner| inner.strip_suffix(']'))
    {
        return normalized_assignment_annotation(inner);
    }
    if let Some(inner) =
        annotation.strip_prefix("ClassVar[").and_then(|inner| inner.strip_suffix(']'))
    {
        return normalized_assignment_annotation(inner);
    }
    match annotation {
        "Final" | "ClassVar" => None,
        _ => Some(annotation),
    }
}

#[allow(dead_code)]
pub(super) fn alias_type_param_substitutions(
    alias_decl: &Declaration,
    args: &[String],
) -> Option<GenericSolution> {
    alias_type_param_substitutions_semantic(
        alias_decl,
        &args.iter().map(|arg| lower_type_text_or_name(arg)).collect::<Vec<_>>(),
    )
}

pub(super) fn alias_type_param_substitutions_semantic(
    alias_decl: &Declaration,
    args: &[SemanticType],
) -> Option<GenericSolution> {
    let type_pack_indexes = alias_decl
        .type_params
        .iter()
        .enumerate()
        .filter_map(|(index, type_param)| {
            (type_param.kind == typepython_binding::GenericTypeParamKind::TypeVarTuple)
                .then_some(index)
        })
        .collect::<Vec<_>>();
    if type_pack_indexes.len() > 1 {
        return None;
    }

    let mut substitutions = GenericSolution::default();
    if let Some(type_pack_index) = type_pack_indexes.first().copied() {
        let mut start = 0usize;
        for type_param in &alias_decl.type_params[..type_pack_index] {
            let argument = args
                .get(start)
                .cloned()
                .or_else(|| type_param.default.as_ref().map(|default| lower_type_text_or_name(default)))?;
            if args.get(start).is_some() {
                start += 1;
            }
            substitutions.types.insert(type_param.name.clone(), argument);
        }

        let mut end = args.len();
        let mut trailing = Vec::new();
        for type_param in alias_decl.type_params[type_pack_index + 1..].iter().rev() {
            let argument = if end > start {
                end -= 1;
                Some(args[end].clone())
            } else {
                None
            }
            .or_else(|| type_param.default.as_ref().map(|default| lower_type_text_or_name(default)))?;
            trailing.push((type_param.name.clone(), argument));
        }
        for (name, argument) in trailing.into_iter().rev() {
            substitutions.types.insert(name, argument);
        }
        substitutions.type_packs.insert(
            alias_decl.type_params[type_pack_index].name.clone(),
            TypePackBinding { types: args[start..end].to_vec(), variadic_tail: None },
        );
        return Some(substitutions);
    }

    if args.len() > alias_decl.type_params.len() {
        return None;
    }

    for (index, type_param) in alias_decl.type_params.iter().enumerate() {
        let argument = args
            .get(index)
            .cloned()
            .or_else(|| type_param.default.as_ref().map(|default| lower_type_text_or_name(default)))?;
        substitutions.types.insert(type_param.name.clone(), argument);
    }
    Some(substitutions)
}

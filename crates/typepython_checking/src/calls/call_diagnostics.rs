use crate::diagnostic_type_text as render_semantic_type;

pub(super) fn direct_call_arity_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.calls
        .iter()
        .filter_map(|call| {
            if let Some(shape) = resolve_synthesized_dataclass_class_shape_with_context(
                context,
                node,
                nodes,
                &call.callee,
            )
                && !shape.has_explicit_init
            {
                return dataclass_transform_constructor_arity_diagnostic(node, call, &shape);
            }
            if let Some(failure) = direct_imported_call_unresolved_typepack_failure(node, nodes, call) {
                return Some(unresolved_generic_call_diagnostic(
                    node,
                    call.line,
                    &call.callee,
                    &failure,
                ));
            }
            if let Some((_, function)) = resolve_function_provider_with_node(nodes, node, &call.callee)
                && let Some(failure) = direct_call_unresolved_typepack_failure(node, nodes, function, call)
            {
                return Some(unresolved_generic_call_diagnostic(
                    node,
                    call.line,
                    &call.callee,
                    &failure,
                ));
            }
            if !call.starred_arg_types.is_empty()
                && resolve_function_provider_with_node(nodes, node, &call.callee).is_some()
            {
                return None;
            }
            if resolve_direct_name_reference_semantic_type(
                node,
                nodes,
                None,
                None,
                None,
                None,
                call.line,
                &call.callee,
            )
            .as_ref()
            .is_some_and(semantic_callable_has_unresolved_paramlist)
            {
                return None;
            }
            if let Some(signature) =
                resolve_direct_callable_signature_sites_with_context(
                    context,
                    node,
                    nodes,
                    &call.callee,
                )
            {
                return direct_source_function_arity_diagnostic_with_context(
                    context,
                    node,
                    nodes,
                    call,
                    &signature,
                );
            }
            let (expected, _) = resolve_direct_callable_signature(node, nodes, &call.callee)?;
            (call.arg_count != expected).then(|| {
                Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` expects {} positional argument(s) but received {}",
                        call.callee,
                        node.module_path.display(),
                        expected,
                        call.arg_count
                    ),
                )
            })
        })
        .collect()
}

pub(super) fn direct_param_count(signature: &str) -> Option<usize> {
    Some(direct_signature_params(signature)?.len())
}

pub(super) fn direct_param_names(signature: &str) -> Option<Vec<String>> {
    Some(direct_signature_params(signature)?.into_iter().map(|param| param.name).collect())
}

pub(super) fn direct_param_types(signature: &str) -> Option<Vec<String>> {
    Some(direct_signature_params(signature)?.into_iter().map(|param| param.annotation).collect())
}

pub(super) fn direct_signature_params(signature: &str) -> Option<Vec<DirectSignatureParam>> {
    parse_direct_signature_params(signature)
}

pub(super) fn direct_call_type_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.calls
        .iter()
        .flat_map(|call| {
            if let Some(shape) =
                resolve_synthesized_dataclass_class_shape_with_context(
                    context,
                    node,
                    nodes,
                    &call.callee,
                )
                && !shape.has_explicit_init
            {
                return dataclass_transform_constructor_type_diagnostics(
                    node,
                    nodes,
                    call,
                    &shape,
                );
            }
            if let Some(failure) = direct_imported_call_unresolved_typepack_failure(node, nodes, call) {
                return vec![unresolved_generic_call_diagnostic(
                    node,
                    call.line,
                    &call.callee,
                    &failure,
                )];
            }
            if let Some((_, function)) = resolve_function_provider_with_node(nodes, node, &call.callee) {
                if let Some(failure) = direct_call_unresolved_typepack_failure(node, nodes, function, call) {
                    return vec![unresolved_generic_call_diagnostic(
                        node,
                        call.line,
                        &call.callee,
                        &failure,
                    )];
                }
                match resolve_direct_call_candidate_with_context_detailed(
                    context,
                    node,
                    nodes,
                    function,
                    call,
                ) {
                    Ok(candidate) => {
                        return direct_source_function_type_diagnostics_with_context(
                            context,
                            node,
                            nodes,
                            call,
                            &candidate.signature_sites,
                        );
                    }
                    Err(failure)
                        if declaration_has_runtime_generic_paramlist(function) =>
                    {
                        return vec![unresolved_generic_call_diagnostic(
                            node,
                            call.line,
                            &call.callee,
                            &failure,
                        )];
                    }
                    Err(_) => {}
                }
            }
            if let Some(signature) =
                resolve_direct_callable_signature_sites_with_context(context, node, nodes, &call.callee)
            {
                return direct_source_function_type_diagnostics_with_context(
                    context,
                    node,
                    nodes,
                    call,
                    &signature,
                );
            }
            let Some(param_types) = resolve_direct_callable_param_types(node, nodes, &call.callee)
            else {
                return Vec::new();
            };
            let param_names =
                direct_param_names_from_signature(node, nodes, &call.callee).unwrap_or_default();
            positional_and_keyword_type_diagnostics(
                node,
                nodes,
                call,
                call.arg_types.as_slice(),
                call.keyword_arg_types.as_slice(),
                &param_types,
                &param_names,
                None,
                None,
                None,
                &[],
                &[],
            )
        })
        .collect()
}

pub(super) fn direct_call_unresolved_typepack_failure(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    function: &Declaration,
    call: &typepython_binding::CallSite,
) -> Option<DirectCallResolutionFailure> {
    let _ = (node, nodes, function, call);
    None
}

fn direct_imported_call_unresolved_typepack_failure(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
) -> Option<DirectCallResolutionFailure> {
    let (_provider_node, function) = resolve_function_provider_with_node(nodes, node, &call.callee)?;
    direct_call_unresolved_typepack_failure(node, nodes, function, call)
}

pub(super) fn direct_call_keyword_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for call in &node.calls {
        if let Some(shape) = resolve_synthesized_dataclass_class_shape_with_context(
            context,
            node,
            nodes,
            &call.callee,
        )
            && !shape.has_explicit_init
        {
            diagnostics
                .extend(dataclass_transform_constructor_keyword_diagnostics(node, call, &shape));
            continue;
        }
        if let Some(signature) =
            resolve_direct_callable_signature_sites_with_context(context, node, nodes, &call.callee)
        {
            diagnostics.extend(direct_source_function_keyword_diagnostics_with_context(
                context, node, nodes, call, &signature,
            ));
            continue;
        }
        let Some((_, param_names)) = resolve_direct_callable_signature(node, nodes, &call.callee)
        else {
            continue;
        };
        for keyword in &call.keyword_names {
            if !param_names.iter().any(|param| param == keyword) {
                diagnostics.push(Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` uses unknown keyword `{}`",
                        call.callee,
                        node.module_path.display(),
                        keyword
                    ),
                ));
            }
        }
    }

    diagnostics
}

pub(super) fn direct_source_function_arity_diagnostic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Option<Diagnostic> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    direct_source_function_arity_diagnostic_with_context(&context, node, nodes, call, signature)
}

pub(super) fn direct_source_function_arity_diagnostic_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Option<Diagnostic> {
    let positional_params = signature
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .collect::<Vec<_>>();
    let has_variadic = signature.iter().any(|param| param.variadic);
    let expected_positional_arg_types =
        expected_positional_arg_types_from_signature_sites(signature, call.arg_count);
    let (positional_types, variadic_starred_types) =
        expanded_positional_arg_types(node, nodes, call, &expected_positional_arg_types);
    if !has_variadic
        && (positional_types.len() > positional_params.len() || !variadic_starred_types.is_empty())
    {
        if !variadic_starred_types.is_empty() && call.callee.contains('.') {
            return None;
        }
        return Some(Diagnostic::error(
            "TPY4001",
            format!(
                "call to `{}` in module `{}` expects at most {} positional argument(s) but received {}",
                call.callee,
                node.module_path.display(),
                positional_params.len(),
                positional_types.len()
            ),
        ));
    }

    let provided_keywords = call.keyword_names.iter().collect::<BTreeSet<_>>();
    let keyword_expansions = resolved_keyword_expansions_with_context(context, node, nodes, call);
    let unpack_shape =
        unpack_typed_dict_shape_from_signature_with_context(context, node, nodes, signature);
    let missing = signature
        .iter()
        .enumerate()
        .filter(|(index, param)| {
            if param.has_default {
                return false;
            }
            if param.keyword_only {
                return !provided_keywords.contains(&param.name)
                    && !keyword_expansions.iter().any(|expansion| match expansion {
                        KeywordExpansion::TypedDict(shape) => {
                            shape.fields.get(&param.name).is_some_and(|field| field.required)
                        }
                        KeywordExpansion::Mapping(_) => false,
                    });
            }
            if param.variadic || param.keyword_variadic {
                return false;
            }
            *index >= positional_types.len()
                && !provided_keywords.contains(&param.name)
                && !keyword_expansions.iter().any(|expansion| match expansion {
                    KeywordExpansion::TypedDict(shape) => {
                        shape.fields.get(&param.name).is_some_and(|field| field.required)
                    }
                    KeywordExpansion::Mapping(_) => false,
                })
        })
        .map(|(_, param)| param.name.clone())
        .collect::<Vec<_>>();
    let mut missing = missing;
    if let Some(shape) = unpack_shape {
        missing.extend(
            shape
                .fields
                .iter()
                .filter(|(key, field)| field.required && !provided_keywords.contains(*key))
                .map(|(key, _)| key.clone()),
        );
    }
    (!missing.is_empty()).then(|| {
        Diagnostic::error(
            "TPY4001",
            format!(
                "call to `{}` in module `{}` is missing required argument(s): {}",
                call.callee,
                node.module_path.display(),
                missing.join(", ")
            ),
        )
    })
}

pub(super) fn direct_source_function_keyword_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<Diagnostic> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    direct_source_function_keyword_diagnostics_with_context(&context, node, nodes, call, signature)
}

pub(super) fn direct_source_function_keyword_diagnostics_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<Diagnostic> {
    let unpack_shape =
        unpack_typed_dict_shape_from_signature_with_context(context, node, nodes, signature);
    let keyword_variadic_annotation = signature
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| param.rendered_annotation())
        .map(|annotation| normalize_type_text(&annotation));
    let param_names = signature
        .iter()
        .filter(|param| !param.keyword_variadic)
        .map(|param| param.name.as_str())
        .collect::<BTreeSet<_>>();
    let accepts_extra_keywords = signature.iter().any(|param| param.keyword_variadic)
        || keyword_variadic_annotation
            .as_deref()
            .is_some_and(|annotation| !annotation.starts_with("Unpack["))
        || unpack_shape.as_ref().is_some_and(|shape| shape.extra_items.is_some());
    let expected_positional_arg_types =
        expected_positional_arg_types_from_signature_sites(signature, call.arg_count);
    let (positional_types, _) =
        expanded_positional_arg_types(node, nodes, call, &expected_positional_arg_types);
    let keyword_expansions = resolved_keyword_expansions_with_context(context, node, nodes, call);
    let mut diagnostics = call.keyword_names
        .iter()
        .filter_map(|keyword| {
            let matching = signature.iter().find(|param| param.name == *keyword);
            Some(match matching {
                Some(param) if param.positional_only => Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` passes positional-only parameter `{}` as a keyword",
                        call.callee,
                        node.module_path.display(),
                        keyword
                    ),
                ),
                None if unpack_shape
                    .as_ref()
                    .is_some_and(|shape| shape.fields.contains_key(keyword.as_str())) =>
                {
                    return None;
                }
                None if unpack_shape.as_ref().is_some_and(|shape| {
                    !shape.fields.contains_key(keyword.as_str()) && shape.extra_items.is_none()
                }) => {
                    Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` uses unknown unpacked keyword `{}`",
                        call.callee,
                        node.module_path.display(),
                        keyword
                    ),
                )
                }
                None if !accepts_extra_keywords && !param_names.contains(keyword.as_str()) => {
                    Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` uses unknown keyword `{}`",
                        call.callee,
                        node.module_path.display(),
                        keyword
                    ),
                )
                }
                _ => return None,
            })
        })
        .collect::<Vec<_>>();

    let positional_param_names = signature
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .map(|param| param.name.as_str())
        .collect::<Vec<_>>();
    for keyword in &call.keyword_names {
        if positional_param_names
            .iter()
            .take(positional_types.len())
            .any(|name| *name == keyword.as_str())
        {
            diagnostics.push(Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` binds parameter `{}` both positionally and by keyword",
                    call.callee,
                    node.module_path.display(),
                    keyword
                ),
            ));
        }
    }

    diagnostics.extend(keyword_expansions.into_iter().flat_map(|expansion| match expansion {
        KeywordExpansion::TypedDict(shape) => shape
            .fields
            .iter()
            .filter_map(|(key, field)| {
                let duplicate = call.keyword_names.iter().any(|keyword| keyword == key)
                    || positional_param_names.iter().take(positional_types.len()).any(|name| *name == key.as_str());
                if duplicate {
                    return Some(Diagnostic::error(
                        "TPY4013",
                        format!(
                            "call to `{}` in module `{}` expands `**{}` with duplicate key `{}`",
                            call.callee,
                            node.module_path.display(),
                            shape.name,
                            key
                        ),
                    ));
                }
                match signature.iter().find(|param| param.name == *key) {
                    Some(param) if param.positional_only => Some(Diagnostic::error(
                        "TPY4013",
                        format!(
                            "call to `{}` in module `{}` cannot satisfy positional-only parameter `{}` via `**{}`",
                            call.callee,
                            node.module_path.display(),
                            key,
                            shape.name
                        ),
                    )),
                    Some(param) if !field.required && !param.has_default => Some(Diagnostic::error(
                        "TPY4013",
                        format!(
                            "call to `{}` in module `{}` cannot satisfy required parameter `{}` from optional TypedDict key in `**{}`",
                            call.callee,
                            node.module_path.display(),
                            key,
                            shape.name
                        ),
                    )),
                    None if !accepts_extra_keywords => Some(Diagnostic::error(
                        "TPY4013",
                        format!(
                            "call to `{}` in module `{}` uses unknown `**{}` key `{}`",
                            call.callee,
                            node.module_path.display(),
                            shape.name,
                            key
                        ),
                    )),
                    _ => None,
                }
            })
            .chain(
                (typed_dict_shape_has_unbounded_extra_keys(&shape) && !accepts_extra_keywords).then(
                    || {
                        Diagnostic::error(
                            "TPY4013",
                            format!(
                                "call to `{}` in module `{}` cannot expand open TypedDict `{}` because it may contain undeclared keys",
                                call.callee,
                                node.module_path.display(),
                                shape.name
                            ),
                        )
                    },
                ),
            )
            .chain(
                shape
                    .extra_items
                    .as_ref()
                    .into_iter()
                    .filter(|_| !accepts_extra_keywords)
                    .map(|extra| {
                        Diagnostic::error(
                            "TPY4013",
                            format!(
                                "call to `{}` in module `{}` cannot expand additional `**{}` keys of type `{}` without `**kwargs`",
                                call.callee,
                                node.module_path.display(),
                                shape.name,
                                extra.value_type
                            ),
                        )
                    }),
            )
            .collect::<Vec<_>>(),
        KeywordExpansion::Mapping(value_ty) => (!accepts_extra_keywords).then(|| {
            Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` cannot expand `**dict[str, {}]` without `**kwargs`",
                    call.callee,
                    node.module_path.display(),
                    render_semantic_type(&value_ty)
                ),
            )
        }).into_iter().collect(),
    }));

    diagnostics
}

pub(super) fn keyword_duplicates_positional_arguments(
    call: &typepython_binding::CallSite,
    params: &[SemanticCallableParam],
) -> bool {
    let positional_param_names = params
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .map(|param| param.name.as_str())
        .collect::<Vec<_>>();
    call.keyword_names.iter().any(|keyword| {
        positional_param_names.iter().take(call.arg_count).any(|name| *name == keyword.as_str())
    })
}

pub(super) fn direct_source_function_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<Diagnostic> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    direct_source_function_type_diagnostics_with_context(&context, node, nodes, call, signature)
}

pub(super) fn direct_source_function_type_diagnostics_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<Diagnostic> {
    let expected_positional_arg_types =
        expected_positional_arg_types_from_signature_sites(signature, call.arg_count);
    let expected_keyword_arg_types =
        expected_keyword_arg_types_from_signature_sites(signature, &call.keyword_names);
    let mut diagnostics = call
        .arg_values
        .iter()
        .enumerate()
        .flat_map(|(index, metadata)| {
            resolve_contextual_call_arg_semantic_type_with_context(
                context,
                node,
                nodes,
                call.line,
                metadata,
                expected_positional_arg_types.get(index).and_then(|expected| expected.as_deref()),
            )
            .into_iter()
            .flat_map(|result| result.diagnostics)
        })
        .collect::<Vec<_>>();
    diagnostics.extend(call.keyword_arg_values.iter().enumerate().flat_map(|(index, metadata)| {
        resolve_contextual_call_arg_semantic_type_with_context(
            context,
            node,
            nodes,
            call.line,
            metadata,
            expected_keyword_arg_types.get(index).and_then(|expected| expected.as_deref()),
        )
        .into_iter()
        .flat_map(|result| result.diagnostics)
    }));
    let resolved_keyword_arg_types =
        resolved_keyword_arg_semantic_types(node, nodes, call, &expected_keyword_arg_types);
    let (expanded_arg_types, variadic_starred_types) =
        expanded_positional_arg_semantic_types(node, nodes, call, &expected_positional_arg_types);
    let keyword_expansions = resolved_keyword_expansions_with_context(context, node, nodes, call);
    let param_types = signature
        .iter()
        .filter(|param| !param.keyword_variadic)
        .map(|param| {
            param
                .rendered_annotation()
                .map(|annotation| lower_type_text_or_name(&annotation))
                .unwrap_or_else(|| SemanticType::Name(String::new()))
        })
        .collect::<Vec<_>>();
    let param_names = signature
        .iter()
        .filter(|param| !param.keyword_variadic)
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    let variadic_type = signature
        .iter()
        .find(|param| param.variadic)
        .and_then(|param| param.rendered_annotation())
        .map(|annotation| lower_type_text_or_name(&annotation));
    let unpack_shape =
        unpack_typed_dict_shape_from_signature_with_context(context, node, nodes, signature);
    let keyword_variadic_type = signature
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| param.rendered_annotation())
        .filter(|annotation| !normalize_type_text(annotation).starts_with("Unpack["))
        .map(|annotation| lower_type_text_or_name(&annotation));
    diagnostics.extend(positional_and_keyword_semantic_type_diagnostics(
        node,
        nodes,
        call,
        &expanded_arg_types,
        &resolved_keyword_arg_types,
        &param_types,
        &param_names,
        variadic_type.as_ref(),
        keyword_variadic_type.as_ref(),
        unpack_shape.as_ref(),
        &variadic_starred_types,
        &keyword_expansions,
    ));
    diagnostics
}

pub(super) fn expanded_positional_arg_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<String>],
) -> (Vec<String>, Vec<String>) {
    let (positional_types, variadic_starred_types) =
        expanded_positional_arg_semantic_types(node, nodes, call, expected_types);
    let positional_types =
        positional_types.into_iter().map(|ty| diagnostic_type_text(&ty)).collect::<Vec<_>>();
    let variadic_starred_types = variadic_starred_types
        .into_iter()
        .map(|ty| diagnostic_type_text(&ty))
        .collect::<Vec<_>>();
    (positional_types, variadic_starred_types)
}

pub(super) fn expanded_positional_arg_semantic_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<String>],
) -> (Vec<SemanticType>, Vec<SemanticType>) {
    let mut positional_types = resolved_call_arg_semantic_types(node, nodes, call, expected_types);
    if positional_types.len() < call.arg_count {
        positional_types.extend(std::iter::repeat_n(
            SemanticType::Name(String::new()),
            call.arg_count - positional_types.len(),
        ));
    }
    let mut variadic_starred_types = Vec::new();
    for expansion in resolved_starred_positional_expansions(node, nodes, call) {
        match expansion {
            PositionalExpansion::Fixed(types) => positional_types.extend(
                types
                    .into_iter()
                    .map(|ty| ty.unwrap_or_else(|| SemanticType::Name(String::new()))),
            ),
            PositionalExpansion::Variadic(element_type) => variadic_starred_types.push(element_type),
        }
    }
    (positional_types, variadic_starred_types)
}

pub(super) fn expanded_positional_arg_semantic_types_with_expected_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<SemanticType>],
) -> (Vec<SemanticType>, Vec<SemanticType>) {
    let mut positional_types =
        resolved_call_arg_semantic_types_with_expected_semantic(node, nodes, call, expected_types);
    if positional_types.len() < call.arg_count {
        positional_types.extend(std::iter::repeat_n(
            SemanticType::Name(String::new()),
            call.arg_count - positional_types.len(),
        ));
    }
    let mut variadic_starred_types = Vec::new();
    for expansion in resolved_starred_positional_expansions(node, nodes, call) {
        match expansion {
            PositionalExpansion::Fixed(types) => positional_types.extend(
                types
                    .into_iter()
                    .map(|ty| ty.unwrap_or_else(|| SemanticType::Name(String::new()))),
            ),
            PositionalExpansion::Variadic(element_type) => variadic_starred_types.push(element_type),
        }
    }
    (positional_types, variadic_starred_types)
}

pub(super) fn resolved_call_arg_semantic_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<String>],
) -> Vec<SemanticType> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    if call.arg_values.is_empty() {
        return call.arg_types.iter().map(|ty| lower_type_text_or_name(ty)).collect();
    }
    call.arg_values
        .iter()
        .enumerate()
        .map(|(index, metadata)| {
            resolve_contextual_call_arg_semantic_type_with_context(
                &context,
                node,
                nodes,
                call.line,
                metadata,
                expected_types.get(index).and_then(|expected| expected.as_deref()),
            )
            .map(|result| result.actual_type)
            .or_else(|| {
                resolve_direct_expression_semantic_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .unwrap_or_else(|| {
                call
                    .arg_types
                    .get(index)
                    .map(|ty| lower_type_text_or_name(ty))
                    .unwrap_or_else(|| SemanticType::Name(String::new()))
            })
        })
        .collect()
}

pub(super) fn resolved_call_arg_semantic_types_with_expected_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<SemanticType>],
) -> Vec<SemanticType> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    if call.arg_values.is_empty() {
        return call.arg_types.iter().map(|ty| lower_type_text_or_name(ty)).collect();
    }
    call.arg_values
        .iter()
        .enumerate()
        .map(|(index, metadata)| {
            resolve_contextual_call_arg_semantic_type_with_expected_semantic(
                &context,
                node,
                nodes,
                call.line,
                metadata,
                expected_types.get(index).and_then(|expected| expected.as_ref()),
            )
            .map(|result| result.actual_type)
            .or_else(|| {
                resolve_direct_expression_semantic_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .unwrap_or_else(|| {
                call.arg_types
                    .get(index)
                    .map(|ty| lower_type_text_or_name(ty))
                    .unwrap_or_else(|| SemanticType::Name(String::new()))
            })
        })
        .collect()
}

pub(super) fn resolved_keyword_arg_semantic_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<String>],
) -> Vec<SemanticType> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    if call.keyword_arg_values.is_empty() {
        return call.keyword_arg_types.iter().map(|ty| lower_type_text_or_name(ty)).collect();
    }
    call.keyword_arg_values
        .iter()
        .enumerate()
        .map(|(index, metadata)| {
            resolve_contextual_call_arg_semantic_type_with_context(
                &context,
                node,
                nodes,
                call.line,
                metadata,
                expected_types.get(index).and_then(|expected| expected.as_deref()),
            )
            .map(|result| result.actual_type)
            .or_else(|| {
                resolve_direct_expression_semantic_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .unwrap_or_else(|| {
                call
                    .keyword_arg_types
                    .get(index)
                    .map(|ty| lower_type_text_or_name(ty))
                    .unwrap_or_else(|| SemanticType::Name(String::new()))
            })
        })
        .collect()
}

pub(super) fn resolved_keyword_arg_semantic_types_with_expected_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<SemanticType>],
) -> Vec<SemanticType> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    if call.keyword_arg_values.is_empty() {
        return call.keyword_arg_types.iter().map(|ty| lower_type_text_or_name(ty)).collect();
    }
    call.keyword_arg_values
        .iter()
        .enumerate()
        .map(|(index, metadata)| {
            resolve_contextual_call_arg_semantic_type_with_expected_semantic(
                &context,
                node,
                nodes,
                call.line,
                metadata,
                expected_types.get(index).and_then(|expected| expected.as_ref()),
            )
            .map(|result| result.actual_type)
            .or_else(|| {
                resolve_direct_expression_semantic_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .unwrap_or_else(|| {
                call.keyword_arg_types
                    .get(index)
                    .map(|ty| lower_type_text_or_name(ty))
                    .unwrap_or_else(|| SemanticType::Name(String::new()))
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn positional_and_keyword_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    arg_types: &[String],
    keyword_arg_types: &[String],
    param_types: &[String],
    param_names: &[String],
    variadic_type: Option<&str>,
    keyword_variadic_type: Option<&str>,
    unpack_shape: Option<&TypedDictShape>,
    variadic_starred_types: &[String],
    keyword_expansions: &[KeywordExpansion],
) -> Vec<Diagnostic> {
    let arg_types = arg_types.iter().map(|ty| lower_type_text_or_name(ty)).collect::<Vec<_>>();
    let keyword_arg_types =
        keyword_arg_types.iter().map(|ty| lower_type_text_or_name(ty)).collect::<Vec<_>>();
    let param_types = param_types.iter().map(|ty| lower_type_text_or_name(ty)).collect::<Vec<_>>();
    let variadic_type = variadic_type.map(lower_type_text_or_name);
    let keyword_variadic_type = keyword_variadic_type.map(lower_type_text_or_name);
    let variadic_starred_types = variadic_starred_types
        .iter()
        .map(|ty| lower_type_text_or_name(ty))
        .collect::<Vec<_>>();
    positional_and_keyword_semantic_type_diagnostics(
        node,
        nodes,
        call,
        &arg_types,
        &keyword_arg_types,
        &param_types,
        param_names,
        variadic_type.as_ref(),
        keyword_variadic_type.as_ref(),
        unpack_shape,
        &variadic_starred_types,
        keyword_expansions,
    )
}

fn semantic_type_missing(ty: &SemanticType) -> bool {
    matches!(ty, SemanticType::Name(name) if name.is_empty())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn positional_and_keyword_semantic_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    arg_types: &[SemanticType],
    keyword_arg_types: &[SemanticType],
    param_types: &[SemanticType],
    param_names: &[String],
    variadic_type: Option<&SemanticType>,
    keyword_variadic_type: Option<&SemanticType>,
    unpack_shape: Option<&TypedDictShape>,
    variadic_starred_types: &[SemanticType],
    keyword_expansions: &[KeywordExpansion],
) -> Vec<Diagnostic> {
    let unpack_extra_items_type = unpack_shape
        .and_then(|shape| shape.extra_items.as_ref())
        .map(|extra| lower_type_text_or_name(&extra.value_type));
    let mut diagnostics = arg_types
        .iter()
        .take(param_types.len())
        .zip(param_types.iter())
        .filter(|(arg_ty, param_ty)| {
            !semantic_type_missing(arg_ty)
                && !semantic_type_missing(param_ty)
                && !semantic_type_is_assignable(node, nodes, param_ty, arg_ty)
        })
        .map(|(arg_ty, param_ty)| {
            let arg_text = diagnostic_type_text(arg_ty);
            let param_text = diagnostic_type_text(param_ty);
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` where parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_text,
                    param_text
                ),
            );
            attach_type_mismatch_notes(diagnostic, node, nodes, &param_text, &arg_text)
        })
        .collect::<Vec<_>>();

    for arg_ty in arg_types.iter().skip(param_types.len()) {
        let Some(param_ty) = variadic_type else {
            break;
        };
        if !semantic_type_missing(arg_ty)
            && !semantic_type_missing(param_ty)
            && !semantic_type_is_assignable(node, nodes, param_ty, arg_ty)
        {
            let arg_text = diagnostic_type_text(arg_ty);
            let param_text = diagnostic_type_text(param_ty);
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` where variadic parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_text,
                    param_text
                ),
            );
            diagnostics
                .push(attach_type_mismatch_notes(diagnostic, node, nodes, &param_text, &arg_text));
        }
    }

    for arg_ty in variadic_starred_types {
        let Some(param_ty) = variadic_type else {
            continue;
        };
        if !semantic_type_missing(arg_ty)
            && !semantic_type_missing(param_ty)
            && !semantic_type_is_assignable(node, nodes, param_ty, arg_ty)
        {
            let arg_text = diagnostic_type_text(arg_ty);
            let param_text = diagnostic_type_text(param_ty);
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` expands `*args` element type `{}` where variadic parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_text,
                    param_text
                ),
            );
            diagnostics
                .push(attach_type_mismatch_notes(diagnostic, node, nodes, &param_text, &arg_text));
        }
    }

    for (keyword, arg_ty) in call.keyword_names.iter().zip(keyword_arg_types) {
        let Some(index) = param_names.iter().position(|param| param == keyword) else {
            if let Some(shape) = unpack_shape
                && let Some(field) = shape.fields.get(keyword.as_str())
            {
                let field_type = field.semantic_value_type();
                if !semantic_type_missing(arg_ty)
                    && field_type.is_some()
                    && !semantic_type_matches(
                        node,
                        nodes,
                        field_type.as_ref().expect("checked some above"),
                        arg_ty,
                    )
                {
                    let arg_text = diagnostic_type_text(arg_ty);
                    let diagnostic = Diagnostic::error(
                        "TPY4001",
                        format!(
                            "call to `{}` in module `{}` passes `{}` for unpacked keyword `{}` where TypedDict key expects `{}`",
                            call.callee,
                            node.module_path.display(),
                            arg_text,
                            keyword,
                            field.rendered_value_type()
                        ),
                    );
                    diagnostics.push(attach_type_mismatch_notes(
                        diagnostic,
                        node,
                        nodes,
                        &field.rendered_value_type(),
                        &arg_text,
                    ));
                }
                continue;
            }
            let Some(param_ty) = unpack_extra_items_type.as_ref().or(keyword_variadic_type) else {
                continue;
            };
            if !semantic_type_missing(arg_ty)
                && !semantic_type_missing(param_ty)
                && !semantic_type_matches(node, nodes, param_ty, arg_ty)
            {
                let arg_text = diagnostic_type_text(arg_ty);
                let param_text = diagnostic_type_text(param_ty);
                let diagnostic = Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` passes `{}` for keyword `{}` where variadic keyword parameter expects `{}`",
                        call.callee,
                        node.module_path.display(),
                        arg_text,
                        keyword,
                        param_text
                    ),
                );
                diagnostics.push(attach_type_mismatch_notes(
                    diagnostic,
                    node,
                    nodes,
                    &param_text,
                    &arg_text,
                ));
            }
            continue;
        };
        let Some(param_ty) = param_types.get(index) else {
            continue;
        };
        if !semantic_type_missing(arg_ty)
            && !semantic_type_missing(param_ty)
            && !semantic_type_matches(node, nodes, param_ty, arg_ty)
        {
            let arg_text = diagnostic_type_text(arg_ty);
            let param_text = diagnostic_type_text(param_ty);
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` for keyword `{}` where parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_text,
                    keyword,
                    param_text
                ),
            );
            diagnostics
                .push(attach_type_mismatch_notes(diagnostic, node, nodes, &param_text, &arg_text));
        }
    }

    for expansion in keyword_expansions {
        match expansion {
            KeywordExpansion::TypedDict(shape) => {
                for (key, field) in &shape.fields {
                    if let Some(index) = param_names.iter().position(|param| param == key) {
                        let param_ty = &param_types[index];
                        if let Some(field_type) = field.semantic_value_type()
                            && !semantic_type_missing(param_ty)
                            && !semantic_type_is_assignable(
                                node,
                                nodes,
                                param_ty,
                                &field_type,
                            )
                        {
                            let param_text = diagnostic_type_text(param_ty);
                            diagnostics.push(Diagnostic::error(
                                "TPY4013",
                                format!(
                                    "call to `{}` in module `{}` expands `**{}` key `{}` with type `{}` where parameter expects `{}`",
                                    call.callee,
                                    node.module_path.display(),
                                    shape.name,
                                    key,
                                    field.rendered_value_type(),
                                    param_text
                                ),
                            ));
                        }
                    } else if let Some(param_ty) =
                        unpack_extra_items_type.as_ref().or(keyword_variadic_type)
                    {
                        if let Some(field_type) = field.semantic_value_type()
                            && !semantic_type_missing(param_ty)
                            && !semantic_type_is_assignable(
                                node,
                                nodes,
                                param_ty,
                                &field_type,
                            )
                        {
                            let param_text = diagnostic_type_text(param_ty);
                            diagnostics.push(Diagnostic::error(
                                "TPY4013",
                                format!(
                                    "call to `{}` in module `{}` expands `**{}` key `{}` with type `{}` where `**kwargs` expects `{}`",
                                    call.callee,
                                    node.module_path.display(),
                                    shape.name,
                                    key,
                                    field.rendered_value_type(),
                                    param_text
                                ),
                            ));
                        }
                    }
                }
                if let Some(extra_items) = &shape.extra_items {
                    if let Some(param_ty) = unpack_extra_items_type.as_ref().or(keyword_variadic_type)
                    {
                        if let Some(extra_items_type) = extra_items.semantic_value_type()
                            && !semantic_type_missing(param_ty)
                            && !semantic_type_is_assignable(
                                node,
                                nodes,
                                param_ty,
                                &extra_items_type,
                            )
                        {
                            let param_text = diagnostic_type_text(param_ty);
                            diagnostics.push(Diagnostic::error(
                                "TPY4013",
                                format!(
                                    "call to `{}` in module `{}` expands additional `**{}` keys of type `{}` where extra keywords expect `{}`",
                                    call.callee,
                                    node.module_path.display(),
                                    shape.name,
                                    extra_items.rendered_value_type(),
                                    param_text
                                ),
                            ));
                        }
                    }
                }
            }
            KeywordExpansion::Mapping(value_ty) => {
                if let Some(param_ty) = keyword_variadic_type
                    && !semantic_type_missing(param_ty)
                    && !semantic_type_is_assignable(node, nodes, param_ty, value_ty)
                {
                    let param_text = diagnostic_type_text(param_ty);
                    diagnostics.push(Diagnostic::error(
                        "TPY4001",
                        format!(
                            "call to `{}` in module `{}` expands `**dict[str, {}]` where `**kwargs` expects `{}`",
                            call.callee,
                            node.module_path.display(),
                            render_semantic_type(value_ty),
                            param_text
                        ),
                    ));
                }
            }
        }
    }

    diagnostics
}

pub(super) fn unpack_typed_dict_shape_from_signature_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Option<TypedDictShape> {
    let annotation = signature
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| param.rendered_annotation())?;
    let annotation = normalize_type_text(&annotation);
    let inner = annotation.strip_prefix("Unpack[")?.strip_suffix(']')?;
    resolve_known_typed_dict_shape_from_type_with_context(context, node, nodes, inner)
}

pub(super) fn load_direct_init_signatures_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<String, Vec<typepython_syntax::DirectFunctionParamSite>> {
    context
        .load_direct_method_signatures(node)
        .into_iter()
        .filter(|((_, method_name), _)| method_name == "__init__")
        .map(|((owner_type_name, _), params)| (owner_type_name, params))
        .collect()
}

pub(super) fn direct_signature_sites_from_detail(
    detail: &str,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    parse_direct_callable_declaration(detail).map(|signature| signature.params).unwrap_or_default()
}

pub(super) fn resolve_direct_callable_signature_sites(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    resolve_direct_callable_signature_sites_with_context(&context, node, nodes, callee)
}

pub(super) fn resolve_direct_callable_signature_sites_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    if let Some(callable_type) = resolve_decorated_function_callable_semantic_type_with_context(
        context,
        node,
        nodes,
        callee,
    )
    {
        return direct_function_signature_sites_from_semantic_callable(&callable_type);
    }

    if let Some((provider_node, function)) = resolve_direct_function_with_node(node, nodes, callee) {
        if let Some(callable) = context.load_declaration_semantics(function).callable
        {
            return Some(callable_signature_sites_from_semantics(&callable));
        }
        let provider_signatures = context.load_direct_function_signatures(provider_node);
        if let Some(signature) = provider_signatures.get(&function.name) {
            return Some(signature.clone());
        }
    }

    if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, callee) {
        if let Some((metaclass_node, call, _)) =
            resolve_metaclass_call_declaration_with_context(context, node, nodes, callee)
        {
            let callable = context.load_declaration_semantics(call).callable?;
            let _ = metaclass_node;
            return Some(callable_signature_sites_from_semantics(&callable).into_iter().skip(1).collect());
        }
        let init = class_node.declarations.iter().find(|declaration| {
            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
                && declaration.name == "__init__"
                && declaration.kind == DeclarationKind::Function
        })?;
        let callable = context.load_declaration_semantics(init).callable?;
        return Some(callable_signature_sites_from_semantics(&callable).into_iter().skip(1).collect());
    }

    let direct_function_signatures = context.load_direct_function_signatures(node);
    if let Some(signature) = direct_function_signatures.get(callee) {
        return Some(signature.clone());
    }

    let direct_init_signatures = load_direct_init_signatures_with_context(context, node);
    if let Some(signature) = direct_init_signatures.get(callee) {
        return Some(signature.clone());
    }

    resolve_typing_callable_signature(callee).map(direct_signature_sites_from_detail)
}

pub(super) fn direct_param_names_from_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<Vec<String>> {
    if let Some(local) = resolve_direct_function(node, nodes, callee) {
        return declaration_signature_param_names(local);
    }

    if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, callee) {
        let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
        if let Some((_, call, _)) = resolve_metaclass_call_declaration_with_context(&context, node, nodes, callee) {
            return declaration_signature_param_names(call).map(|names| names.into_iter().skip(1).collect());
        }
        let init = class_node.declarations.iter().find(|declaration| {
            declaration.kind == DeclarationKind::Function
                && declaration.name == "__init__"
                && declaration.owner.as_ref().is_some_and(|owner| {
                    owner.kind == DeclarationOwnerKind::Class && owner.name == class_decl.name
                })
        })?;

        return declaration_signature_param_names(init).map(|names| names.into_iter().skip(1).collect());
    }

    None
}

pub(super) fn dataclass_transform_constructor_arity_diagnostic(
    node: &typepython_graph::ModuleNode,
    call: &typepython_binding::CallSite,
    shape: &DataclassTransformClassShape,
) -> Option<Diagnostic> {
    let positional_fields = shape.fields.iter().filter(|field| !field.kw_only).collect::<Vec<_>>();
    if call.arg_count > positional_fields.len() {
        return Some(Diagnostic::error(
            "TPY4001",
            format!(
                "call to `{}` in module `{}` expects at most {} positional argument(s) but received {}",
                call.callee,
                node.module_path.display(),
                positional_fields.len(),
                call.arg_count
            ),
        ));
    }

    let provided_keywords = call.keyword_names.iter().collect::<BTreeSet<_>>();
    let missing_required = shape
        .fields
        .iter()
        .enumerate()
        .filter(|(index, field)| {
            field.required
                && if field.kw_only {
                    !provided_keywords.contains(&field.keyword_name)
                } else {
                    *index >= call.arg_count && !provided_keywords.contains(&field.keyword_name)
                }
        })
        .map(|(_, field)| field.keyword_name.clone())
        .collect::<Vec<_>>();
    (!missing_required.is_empty()).then(|| {
        Diagnostic::error(
            "TPY4001",
            format!(
                "call to `{}` in module `{}` is missing required synthesized dataclass-transform field(s): {}",
                call.callee,
                node.module_path.display(),
                missing_required.join(", ")
            ),
        )
    })
}

pub(super) fn dataclass_transform_constructor_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    shape: &DataclassTransformClassShape,
) -> Vec<Diagnostic> {
    let positional_fields = shape.fields.iter().filter(|field| !field.kw_only).collect::<Vec<_>>();
    let mut diagnostics = positional_fields
        .iter()
        .take(call.arg_count)
        .zip(call.arg_types.iter().map(|ty| lower_type_text_or_name(ty)))
        .filter(|(field, arg_ty)| {
            !semantic_type_missing(arg_ty)
                && !field.rendered_annotation().is_empty()
                && !semantic_type_matches(
                    node,
                    nodes,
                    &lower_type_text_or_name(&field.rendered_annotation()),
                    arg_ty,
                )
        })
        .map(|(field, arg_ty)| {
            let arg_text = diagnostic_type_text(&arg_ty);
            Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` where synthesized dataclass-transform field `{}` expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_text,
                    field.name,
                    field.rendered_annotation()
                ),
            )
        })
        .collect::<Vec<_>>();

    for (keyword, arg_ty) in call
        .keyword_names
        .iter()
        .zip(call.keyword_arg_types.iter().map(|ty| lower_type_text_or_name(ty)))
    {
        let Some(field) = shape.fields.iter().find(|field| field.keyword_name == *keyword) else {
            continue;
        };
        if !semantic_type_missing(&arg_ty)
            && !field.rendered_annotation().is_empty()
            && !semantic_type_matches(
                node,
                nodes,
                &lower_type_text_or_name(&field.rendered_annotation()),
                &arg_ty,
            )
        {
            let arg_text = diagnostic_type_text(&arg_ty);
            diagnostics.push(Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` for synthesized keyword `{}` where field `{}` expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_text,
                    keyword,
                    field.name,
                    field.rendered_annotation()
                ),
            ));
        }
    }

    diagnostics
}

pub(super) fn dataclass_transform_constructor_keyword_diagnostics(
    node: &typepython_graph::ModuleNode,
    call: &typepython_binding::CallSite,
    shape: &DataclassTransformClassShape,
) -> Vec<Diagnostic> {
    let valid_names =
        shape.fields.iter().map(|field| field.keyword_name.as_str()).collect::<BTreeSet<_>>();
    let positional_field_names = shape
        .fields
        .iter()
        .filter(|field| !field.kw_only)
        .map(|field| field.keyword_name.as_str())
        .collect::<Vec<_>>();
    let mut diagnostics = call.keyword_names
        .iter()
        .filter(|keyword| !valid_names.contains(keyword.as_str()))
        .map(|keyword| {
            Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` uses unknown synthesized dataclass-transform keyword `{}`",
                    call.callee,
                    node.module_path.display(),
                    keyword
                ),
            )
        })
        .collect::<Vec<_>>();

    for keyword in &call.keyword_names {
        if positional_field_names.iter().take(call.arg_count).any(|name| *name == keyword.as_str())
        {
            diagnostics.push(Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` binds synthesized field `{}` both positionally and by keyword",
                    call.callee,
                    node.module_path.display(),
                    keyword
                ),
            ));
        }
    }

    diagnostics
}

pub(super) fn direct_unresolved_paramspec_call_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_direct_call_context_sites(&source)
        .into_iter()
        .filter_map(|call_site| {
            if let Some((_, function)) = resolve_function_provider_with_node(nodes, node, &call_site.callee)
                && let Some(bound_call) = node
                    .calls
                    .iter()
                    .find(|call| call.callee == call_site.callee && call.line == call_site.line)
                && let Some(failure) = direct_call_unresolved_typepack_failure(node, nodes, function, bound_call)
                    .or_else(|| direct_imported_call_unresolved_typepack_failure(node, nodes, bound_call))
            {
                return Some(unresolved_generic_call_diagnostic(
                    node,
                    call_site.line,
                    &call_site.callee,
                    &failure,
                ));
            }
            if node.declarations.iter().any(|declaration| {
                declaration.owner.is_none()
                    && declaration.kind == DeclarationKind::Function
                    && declaration.name == call_site.callee
            }) {
                return None;
            }
            let owner_has_paramspec = resolve_scope_owner_declaration(
                node,
                call_site.owner_name.as_deref(),
                call_site.owner_type_name.as_deref(),
            )
            .is_some_and(|declaration| {
                declaration.type_params.iter().any(|type_param| {
                    type_param.kind == typepython_binding::GenericTypeParamKind::ParamSpec
                })
            });
            let callable_type = resolve_direct_name_reference_semantic_type(
                node,
                nodes,
                None,
                None,
                call_site.owner_name.as_deref(),
                call_site.owner_type_name.as_deref(),
                call_site.line,
                &call_site.callee,
            )?;
            if owner_has_paramspec && semantic_callable_has_unresolved_paramlist(&callable_type) {
                return None;
            }
            if semantic_callable_unresolved_paramlist_is_empty_for_call_site(
                node,
                nodes,
                &callable_type,
                &call_site,
            ) {
                return None;
            }
            semantic_callable_has_unresolved_paramlist(&callable_type).then(|| {
                let failure = DirectCallResolutionFailure::UnresolvedCallableParamList {
                    callable: callable_type.clone(),
                };
                Diagnostic::error(
                    "TPY4014",
                    format!(
                        "call to `{}` in module `{}` is invalid because callable type `{}` still contains an unresolved ParamSpec or Concatenate tail",
                        call_site.callee,
                        node.module_path.display(),
                        diagnostic_type_text(&callable_type)
                    ),
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    call_site.line,
                    1,
                    call_site.line,
                    1,
                ))
                .with_note(failure.diagnostic_reason())
            })
        })
        .collect()
}

pub(super) fn semantic_callable_has_unresolved_paramlist(callable: &SemanticType) -> bool {
    let Some((params, _)) = callable.callable_parts() else {
        return false;
    };
    semantic_callable_params_are_unresolved(params)
}

pub(super) fn semantic_callable_params_are_unresolved(params: &SemanticCallableParams) -> bool {
    match params {
        SemanticCallableParams::Ellipsis | SemanticCallableParams::ParamList(_) => false,
        SemanticCallableParams::Single(_) | SemanticCallableParams::Concatenate(_) => true,
    }
}

fn semantic_callable_unresolved_paramlist_is_empty_for_call_site(
    _node: &typepython_graph::ModuleNode,
    _nodes: &[typepython_graph::ModuleNode],
    callable: &SemanticType,
    call: &typepython_syntax::DirectCallContextSite,
) -> bool {
    let Some((params, _)) = callable.callable_parts() else {
        return false;
    };
    match params {
        SemanticCallableParams::Single(_) => {
            call.positional_arg_count == 0
                && call.keyword_arg_count == 0
                && !call.has_starred_args
                && !call.has_unpacked_kwargs
        }
        SemanticCallableParams::Concatenate(types) if !types.is_empty() => {
            call.positional_arg_count == types.len().saturating_sub(1)
                && call.keyword_arg_count == 0
                && !call.has_starred_args
                && !call.has_unpacked_kwargs
        }
        SemanticCallableParams::Ellipsis
        | SemanticCallableParams::ParamList(_)
        | SemanticCallableParams::Concatenate(_) => false,
    }
}

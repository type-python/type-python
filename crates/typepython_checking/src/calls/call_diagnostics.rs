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
    let inner = signature.strip_prefix('(')?.split_once(')')?.0;
    if inner.is_empty() {
        return Some(Vec::new());
    }

    Some(direct_signature_params(signature)?.into_iter().map(|param| param.annotation).collect())
}

pub(super) fn direct_signature_params(signature: &str) -> Option<Vec<DirectSignatureParam>> {
    let inner = signature.strip_prefix('(')?.split_once(')')?.0;
    if inner.is_empty() {
        return Some(Vec::new());
    }

    let parts = split_top_level_type_args(inner);
    let slash_index = parts.iter().position(|part| part.trim() == "/");
    let star_index = parts.iter().position(|part| part.trim() == "*");
    let mut params = Vec::new();
    let mut keyword_only_active = false;
    for (index, part) in parts.into_iter().enumerate() {
        let part = part.trim();
        if part == "/" {
            continue;
        }
        if part == "*" {
            keyword_only_active = true;
            continue;
        }

        let has_default = part.ends_with('=');
        let part = part.trim_end_matches('=').trim();
        let (part, variadic, keyword_variadic) = if let Some(part) = part.strip_prefix("**") {
            (part.trim(), false, true)
        } else if let Some(part) = part.strip_prefix('*') {
            keyword_only_active = true;
            (part.trim(), true, false)
        } else {
            (part, false, false)
        };
        let (name, annotation) = part
            .split_once(':')
            .map(|(name, annotation)| (name.trim(), annotation.trim().to_owned()))
            .unwrap_or((part, String::new()));
        params.push(DirectSignatureParam {
            name: name.to_owned(),
            annotation,
            has_default,
            positional_only: slash_index.is_some_and(|slash_index| index < slash_index),
            keyword_only: !variadic
                && !keyword_variadic
                && (star_index.is_some_and(|star_index| index > star_index) || keyword_only_active),
            variadic,
            keyword_variadic,
        });
    }

    Some(params)
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
            if let Some(function) = resolve_direct_function(node, nodes, &call.callee)
                && let Some(signature) =
                    resolve_instantiated_direct_function_signature(node, nodes, function, call)
            {
                return direct_source_function_type_diagnostics_with_context(
                    context,
                    node,
                    nodes,
                    call,
                    &signature,
                );
            }
            if let Some(function) = resolve_direct_function(node, nodes, &call.callee)
                && function.type_params.iter().any(|type_param| {
                    matches!(
                        type_param.kind,
                        typepython_binding::GenericTypeParamKind::ParamSpec
                            | typepython_binding::GenericTypeParamKind::TypeVarTuple
                    )
                })
            {
                return vec![
                    Diagnostic::error(
                        "TPY4014",
                        format!(
                            "call to `{}` in module `{}` is invalid because generic parameter list of `{}` could not be resolved from this call",
                            call.callee, node.module_key, call.callee
                        ),
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        call.line,
                        1,
                        call.line,
                        1,
                    )),
                ];
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
        .and_then(|param| param.annotation.as_deref())
        .map(normalize_type_text);
    let param_names = signature
        .iter()
        .filter(|param| !param.keyword_variadic)
        .map(|param| param.name.as_str())
        .collect::<BTreeSet<_>>();
    let accepts_extra_keywords = keyword_variadic_annotation
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
                    value_ty
                ),
            )
        }).into_iter().collect(),
    }));

    diagnostics
}

pub(super) fn keyword_duplicates_positional_arguments(
    call: &typepython_binding::CallSite,
    params: &[DirectSignatureParam],
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
            resolve_contextual_call_arg_type(
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
        resolve_contextual_call_arg_type(
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
        resolved_keyword_arg_types(node, nodes, call, &expected_keyword_arg_types);
    let (expanded_arg_types, variadic_starred_types) =
        expanded_positional_arg_types(node, nodes, call, &expected_positional_arg_types);
    let keyword_expansions = resolved_keyword_expansions_with_context(context, node, nodes, call);
    let param_types = signature
        .iter()
        .filter(|param| !param.keyword_variadic)
        .map(|param| param.annotation.clone().unwrap_or_default())
        .collect::<Vec<_>>();
    let param_names = signature
        .iter()
        .filter(|param| !param.keyword_variadic)
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    let variadic_type =
        signature.iter().find(|param| param.variadic).and_then(|param| param.annotation.as_deref());
    let unpack_shape =
        unpack_typed_dict_shape_from_signature_with_context(context, node, nodes, signature);
    let keyword_variadic_type = signature
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| param.annotation.as_deref())
        .filter(|annotation| !normalize_type_text(annotation).starts_with("Unpack["));
    diagnostics.extend(positional_and_keyword_type_diagnostics(
        node,
        nodes,
        call,
        &expanded_arg_types,
        &resolved_keyword_arg_types,
        &param_types,
        &param_names,
        variadic_type,
        keyword_variadic_type,
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
    let mut positional_types = resolved_call_arg_types(node, nodes, call, expected_types);
    if positional_types.len() < call.arg_count {
        positional_types
            .extend(std::iter::repeat_n(String::new(), call.arg_count - positional_types.len()));
    }
    let mut variadic_starred_types = Vec::new();
    for expansion in resolved_starred_positional_expansions(node, nodes, call) {
        match expansion {
            PositionalExpansion::Fixed(types) => positional_types.extend(types),
            PositionalExpansion::Variadic(element_type) => {
                variadic_starred_types.push(element_type)
            }
        }
    }
    (positional_types, variadic_starred_types)
}

pub(super) fn resolved_call_arg_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<String>],
) -> Vec<String> {
    if call.arg_values.is_empty() {
        return call.arg_types.clone();
    }
    call.arg_values
        .iter()
        .enumerate()
        .map(|(index, metadata)| {
            resolve_contextual_call_arg_type(
                node,
                nodes,
                call.line,
                metadata,
                expected_types.get(index).and_then(|expected| expected.as_deref()),
            )
            .map(|result| result.actual_type)
            .or_else(|| {
                resolve_direct_expression_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .unwrap_or_else(|| call.arg_types.get(index).cloned().unwrap_or_default())
        })
        .collect()
}

pub(super) fn resolved_keyword_arg_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<String>],
) -> Vec<String> {
    if call.keyword_arg_values.is_empty() {
        return call.keyword_arg_types.clone();
    }
    call.keyword_arg_values
        .iter()
        .enumerate()
        .map(|(index, metadata)| {
            resolve_contextual_call_arg_type(
                node,
                nodes,
                call.line,
                metadata,
                expected_types.get(index).and_then(|expected| expected.as_deref()),
            )
            .map(|result| result.actual_type)
            .or_else(|| {
                resolve_direct_expression_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .unwrap_or_else(|| call.keyword_arg_types.get(index).cloned().unwrap_or_default())
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
    let unpack_extra_items_type = unpack_shape
        .and_then(|shape| shape.extra_items.as_ref())
        .map(|extra| extra.value_type.as_str());
    let mut diagnostics = arg_types
        .iter()
        .take(param_types.len())
        .zip(param_types.iter())
        .filter(|(arg_ty, param_ty)| {
            !arg_ty.is_empty()
                && !param_ty.is_empty()
                && !direct_type_is_assignable(node, nodes, param_ty, arg_ty)
        })
        .map(|(arg_ty, param_ty)| {
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` where parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    param_ty
                ),
            );
            attach_type_mismatch_notes(diagnostic, node, nodes, param_ty, arg_ty)
        })
        .collect::<Vec<_>>();

    for arg_ty in arg_types.iter().skip(param_types.len()) {
        let Some(param_ty) = variadic_type else {
            break;
        };
        if !arg_ty.is_empty()
            && !param_ty.is_empty()
            && !direct_type_is_assignable(node, nodes, param_ty, arg_ty)
        {
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` where variadic parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    param_ty
                ),
            );
            diagnostics.push(attach_type_mismatch_notes(diagnostic, node, nodes, param_ty, arg_ty));
        }
    }

    for arg_ty in variadic_starred_types {
        let Some(param_ty) = variadic_type else {
            continue;
        };
        if !arg_ty.is_empty()
            && !param_ty.is_empty()
            && !direct_type_is_assignable(node, nodes, param_ty, arg_ty)
        {
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` expands `*args` element type `{}` where variadic parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    param_ty
                ),
            );
            diagnostics.push(attach_type_mismatch_notes(diagnostic, node, nodes, param_ty, arg_ty));
        }
    }

    for (keyword, arg_ty) in call.keyword_names.iter().zip(keyword_arg_types) {
        let Some(index) = param_names.iter().position(|param| param == keyword) else {
            if let Some(shape) = unpack_shape
                && let Some(field) = shape.fields.get(keyword.as_str())
            {
                if !arg_ty.is_empty()
                    && !field.value_type.is_empty()
                    && !direct_type_matches(node, nodes, &field.value_type, arg_ty)
                {
                    let diagnostic = Diagnostic::error(
                        "TPY4001",
                        format!(
                            "call to `{}` in module `{}` passes `{}` for unpacked keyword `{}` where TypedDict key expects `{}`",
                            call.callee,
                            node.module_path.display(),
                            arg_ty,
                            keyword,
                            field.value_type
                        ),
                    );
                    diagnostics.push(attach_type_mismatch_notes(
                        diagnostic,
                        node,
                        nodes,
                        &field.value_type,
                        arg_ty,
                    ));
                }
                continue;
            }
            let Some(param_ty) = unpack_extra_items_type.or(keyword_variadic_type) else {
                continue;
            };
            if !arg_ty.is_empty()
                && !param_ty.is_empty()
                && !direct_type_matches(node, nodes, param_ty, arg_ty)
            {
                let diagnostic = Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` passes `{}` for keyword `{}` where variadic keyword parameter expects `{}`",
                        call.callee,
                        node.module_path.display(),
                        arg_ty,
                        keyword,
                        param_ty
                    ),
                );
                diagnostics
                    .push(attach_type_mismatch_notes(diagnostic, node, nodes, param_ty, arg_ty));
            }
            continue;
        };
        let Some(param_ty) = param_types.get(index) else {
            continue;
        };
        if !arg_ty.is_empty()
            && !param_ty.is_empty()
            && !direct_type_matches(node, nodes, param_ty, arg_ty)
        {
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` for keyword `{}` where parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    keyword,
                    param_ty
                ),
            );
            diagnostics.push(attach_type_mismatch_notes(diagnostic, node, nodes, param_ty, arg_ty));
        }
    }

    for expansion in keyword_expansions {
        match expansion {
            KeywordExpansion::TypedDict(shape) => {
                for (key, field) in &shape.fields {
                    if let Some(index) = param_names.iter().position(|param| param == key) {
                        let param_ty = &param_types[index];
                        if !field.value_type.is_empty()
                            && !param_ty.is_empty()
                            && !direct_type_is_assignable(node, nodes, param_ty, &field.value_type)
                        {
                            diagnostics.push(Diagnostic::error(
                                "TPY4013",
                                format!(
                                    "call to `{}` in module `{}` expands `**{}` key `{}` with type `{}` where parameter expects `{}`",
                                    call.callee,
                                    node.module_path.display(),
                                    shape.name,
                                    key,
                                    field.value_type,
                                    param_ty
                                ),
                            ));
                        }
                    } else if let Some(param_ty) = unpack_extra_items_type.or(keyword_variadic_type)
                    {
                        if !field.value_type.is_empty()
                            && !param_ty.is_empty()
                            && !direct_type_is_assignable(node, nodes, param_ty, &field.value_type)
                        {
                            diagnostics.push(Diagnostic::error(
                                "TPY4013",
                                format!(
                                    "call to `{}` in module `{}` expands `**{}` key `{}` with type `{}` where `**kwargs` expects `{}`",
                                    call.callee,
                                    node.module_path.display(),
                                    shape.name,
                                    key,
                                    field.value_type,
                                    param_ty
                                ),
                            ));
                        }
                    }
                }
                if let Some(extra_items) = &shape.extra_items {
                    if let Some(param_ty) = unpack_extra_items_type.or(keyword_variadic_type) {
                        if !extra_items.value_type.is_empty()
                            && !param_ty.is_empty()
                            && !direct_type_is_assignable(
                                node,
                                nodes,
                                param_ty,
                                &extra_items.value_type,
                            )
                        {
                            diagnostics.push(Diagnostic::error(
                                "TPY4013",
                                format!(
                                    "call to `{}` in module `{}` expands additional `**{}` keys of type `{}` where extra keywords expect `{}`",
                                    call.callee,
                                    node.module_path.display(),
                                    shape.name,
                                    extra_items.value_type,
                                    param_ty
                                ),
                            ));
                        }
                    }
                }
            }
            KeywordExpansion::Mapping(value_ty) => {
                if let Some(param_ty) = keyword_variadic_type
                    && !value_ty.is_empty()
                    && !param_ty.is_empty()
                    && !direct_type_is_assignable(node, nodes, param_ty, value_ty)
                {
                    diagnostics.push(Diagnostic::error(
                        "TPY4001",
                        format!(
                            "call to `{}` in module `{}` expands `**dict[str, {}]` where `**kwargs` expects `{}`",
                            call.callee,
                            node.module_path.display(),
                            value_ty,
                            param_ty
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
        .and_then(|param| param.annotation.as_deref())?;
    let annotation = normalize_type_text(annotation);
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
    direct_signature_params(detail)
        .unwrap_or_default()
        .into_iter()
        .map(|param| typepython_syntax::DirectFunctionParamSite {
            name: param.name,
            annotation: (!param.annotation.is_empty()).then_some(param.annotation),
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect()
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
    if let Some(callable_annotation) =
        resolve_decorated_function_callable_annotation_with_context(context, node, nodes, callee)
    {
        return direct_function_signature_sites_from_callable_annotation(&callable_annotation);
    }
    let direct_function_signatures = context.load_direct_function_signatures(node);
    if let Some(signature) = direct_function_signatures.get(callee) {
        return Some(signature.clone());
    }

    let direct_init_signatures = load_direct_init_signatures_with_context(context, node);
    if let Some(signature) = direct_init_signatures.get(callee) {
        return Some(signature.clone());
    }

    if let Some(function) = resolve_direct_function(node, nodes, callee) {
        return Some(direct_signature_sites_from_detail(&function.detail));
    }

    if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, callee) {
        let init = class_node.declarations.iter().find(|declaration| {
            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
                && declaration.name == "__init__"
                && declaration.kind == DeclarationKind::Function
        })?;
        return Some(
            direct_signature_sites_from_detail(&init.detail).into_iter().skip(1).collect(),
        );
    }

    resolve_typing_callable_signature(callee).map(direct_signature_sites_from_detail)
}

pub(super) fn direct_param_names_from_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<Vec<String>> {
    if let Some(local) = resolve_direct_function(node, nodes, callee) {
        return direct_param_names(&local.detail);
    }

    if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, callee) {
        let init = class_node.declarations.iter().find(|declaration| {
            declaration.kind == DeclarationKind::Function
                && declaration.name == "__init__"
                && declaration.owner.as_ref().is_some_and(|owner| {
                    owner.kind == DeclarationOwnerKind::Class && owner.name == class_decl.name
                })
        })?;

        return direct_param_names(&init.detail).map(|names| names.into_iter().skip(1).collect());
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
        .zip(call.arg_types.iter())
        .filter(|(field, arg_ty)| {
            !arg_ty.is_empty()
                && !field.annotation.is_empty()
                && !direct_type_matches(node, nodes, &field.annotation, arg_ty)
        })
        .map(|(field, arg_ty)| {
            Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` where synthesized dataclass-transform field `{}` expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    field.name,
                    field.annotation
                ),
            )
        })
        .collect::<Vec<_>>();

    for (keyword, arg_ty) in call.keyword_names.iter().zip(&call.keyword_arg_types) {
        let Some(field) = shape.fields.iter().find(|field| field.keyword_name == *keyword) else {
            continue;
        };
        if !arg_ty.is_empty()
            && !field.annotation.is_empty()
            && !direct_type_matches(node, nodes, &field.annotation, arg_ty)
        {
            diagnostics.push(Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` for synthesized keyword `{}` where field `{}` expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    keyword,
                    field.name,
                    field.annotation
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
            let signature = resolve_scope_owner_signature(
                node,
                call_site.owner_name.as_deref(),
                call_site.owner_type_name.as_deref(),
            );
            let callable_type = resolve_direct_name_reference_type(
                node,
                nodes,
                signature,
                None,
                call_site.owner_name.as_deref(),
                call_site.owner_type_name.as_deref(),
                call_site.line,
                &call_site.callee,
            )?;
            let callable_type = rewrite_imported_typing_aliases(node, &callable_type);
            if owner_has_paramspec && callable_has_unresolved_paramlist(&callable_type) {
                return None;
            }
            callable_has_unresolved_paramlist(&callable_type).then(|| {
                Diagnostic::error(
                    "TPY4014",
                    format!(
                        "call to `{}` in module `{}` is invalid because callable type `{}` still contains an unresolved ParamSpec or Concatenate tail",
                        call_site.callee,
                        node.module_path.display(),
                        callable_type
                    ),
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    call_site.line,
                    1,
                    call_site.line,
                    1,
                ))
            })
        })
        .collect()
}

pub(super) fn callable_has_unresolved_paramlist(text: &str) -> bool {
    let text = normalize_type_text(text);
    let Some(inner) = text.strip_prefix("Callable[").and_then(|inner| inner.strip_suffix(']'))
    else {
        return false;
    };
    let parts = split_top_level_type_args(inner);
    if parts.len() != 2 {
        return false;
    }

    callable_params_are_unresolved(parts[0])
}

pub(super) fn callable_params_are_unresolved(params: &str) -> bool {
    let params = params.trim();
    if params == "..." || params.is_empty() {
        return false;
    }
    if params.starts_with('[') && params.ends_with(']') {
        return false;
    }
    if let Some(inner) =
        params.strip_prefix("Concatenate[").and_then(|inner| inner.strip_suffix(']'))
    {
        return split_top_level_type_args(inner)
            .last()
            .is_some_and(|tail| callable_params_are_unresolved(tail));
    }

    true
}

use super::*;

pub(super) fn direct_member_access_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.member_accesses
        .iter()
        .filter_map(|access| {
            if !access.through_instance
                && let Some(module_node) =
                    resolve_imported_module_target(node, nodes, &access.owner_name)
            {
                let has_member = module_node.declarations.iter().any(|declaration| {
                    declaration.owner.is_none() && declaration.name == access.member
                });
                return (!has_member).then(|| {
                    Diagnostic::error(
                        "TPY4002",
                        format!(
                            "module `{}` in module `{}` has no member `{}`",
                            module_node.module_key,
                            node.module_path.display(),
                            access.member
                        ),
                    )
                });
            }

            let owner_type_name = resolve_member_access_owner_type(node, nodes, access)?;
            if let Some(branches) = union_branches(&owner_type_name) {
                let available = branches
                    .iter()
                    .filter(|branch| type_has_readable_member(node, nodes, branch, &access.member))
                    .cloned()
                    .collect::<Vec<_>>();
                if available.len() == branches.len() {
                    return None;
                }
                let mut diagnostic = Diagnostic::error(
                    "TPY4002",
                    format!(
                        "type `{}` in module `{}` has no member `{}` on every union branch",
                        owner_type_name,
                        node.module_path.display(),
                        access.member
                    ),
                );
                if let Some((span, replacement)) =
                    union_member_guard_suggestion(&node.module_path, access, &available)
                {
                    diagnostic = diagnostic.with_suggestion(
                        format!(
                            "Insert `isinstance` guard for `{}` before accessing `{}`",
                            access.owner_name, access.member
                        ),
                        span,
                        replacement,
                        SuggestionApplicability::MachineApplicable,
                    );
                }
                return Some(diagnostic);
            }
            let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
            let has_member = find_owned_readable_member_declaration(
                nodes,
                class_node,
                class_decl,
                &access.member,
            )
            .is_some();

            (!has_member).then(|| {
                Diagnostic::error(
                    "TPY4002",
                    format!(
                        "type `{}` in module `{}` has no member `{}`",
                        class_decl.name,
                        node.module_path.display(),
                        access.member
                    ),
                )
            })
        })
        .collect()
}

pub(super) fn type_has_readable_member(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
    member: &str,
) -> bool {
    let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, type_name) else {
        return false;
    };
    find_owned_readable_member_declaration(nodes, class_node, class_decl, member).is_some()
}

pub(super) fn union_member_guard_suggestion(
    module_path: &std::path::Path,
    access: &typepython_binding::MemberAccessSite,
    available_branches: &[String],
) -> Option<(Span, String)> {
    let guard_types = available_branches
        .iter()
        .filter_map(|branch| isinstance_guard_type_name(branch))
        .collect::<Vec<_>>();
    if guard_types.is_empty() {
        return None;
    }
    let source = fs::read_to_string(module_path).ok()?;
    let line_text = source.lines().nth(access.line.checked_sub(1)?)?;
    let indent = leading_space_count(line_text);
    let guard = if guard_types.len() == 1 {
        guard_types[0].clone()
    } else {
        format!("({})", guard_types.join(", "))
    };
    Some((
        Span::new(module_path.display().to_string(), access.line, 1, access.line, 1),
        format!("{}assert isinstance({}, {})\n", " ".repeat(indent), access.owner_name, guard),
    ))
}

pub(super) fn isinstance_guard_type_name(type_name: &str) -> Option<String> {
    let normalized = normalize_type_text(type_name);
    if normalized.is_empty()
        || matches!(normalized.as_str(), "None" | "dynamic" | "unknown")
        || normalized.contains('[')
        || normalized.contains('|')
    {
        return None;
    }
    Some(normalized)
}

pub(super) fn direct_method_call_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let direct_method_signatures = context.load_direct_method_signatures(node);

    for call in &node.method_calls {
        if !call.through_instance
            && let Some(module_diagnostics) =
                imported_module_method_call_diagnostics(node, nodes, call)
        {
            diagnostics.extend(module_diagnostics);
            continue;
        }

        let Some(owner_type_name) = resolve_method_call_owner_type(context, node, nodes, call)
        else {
            continue;
        };
        let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &owner_type_name)
        else {
            continue;
        };
        let candidates =
            find_owned_callable_declarations(nodes, class_node, class_decl, &call.method);
        let Some(target) = candidates.first().copied() else {
            continue;
        };

        let direct_call = typepython_binding::CallSite {
            callee: format!("{}.{}", class_decl.name, call.method),
            arg_count: call.arg_count,
            arg_types: call.arg_types.clone(),
            arg_values: call.arg_values.clone(),
            starred_arg_types: call.starred_arg_types.clone(),
            starred_arg_values: call.starred_arg_values.clone(),
            keyword_names: call.keyword_names.clone(),
            keyword_arg_types: call.keyword_arg_types.clone(),
            keyword_arg_values: call.keyword_arg_values.clone(),
            keyword_expansion_types: call.keyword_expansion_types.clone(),
            keyword_expansion_values: call.keyword_expansion_values.clone(),
            line: 1,
        };

        let overloads = candidates
            .iter()
            .copied()
            .filter(|declaration| declaration.kind == DeclarationKind::Overload)
            .collect::<Vec<_>>();
        if !overloads.is_empty() {
            let applicable = overloads
                .iter()
                .copied()
                .filter(|declaration| {
                    method_overload_is_applicable(
                        node,
                        nodes,
                        &direct_call,
                        declaration,
                        &owner_type_name,
                    )
                })
                .collect::<Vec<_>>();
            if applicable.len() >= 2 {
                diagnostics.push(Diagnostic::error(
                    "TPY4012",
                    format!(
                        "call to `{}.{}` in module `{}` is ambiguous across {} overloads after applicability filtering",
                        class_decl.name,
                        call.method,
                        node.module_path.display(),
                        applicable.len()
                    ),
                ));
                continue;
            }
            if let Some(applicable) = applicable.first().copied() {
                let signature = direct_method_signature_sites(applicable, &owner_type_name);
                if let Some(diagnostic) = direct_source_function_arity_diagnostic_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &signature,
                ) {
                    diagnostics.push(diagnostic);
                }
                diagnostics.extend(direct_source_function_keyword_diagnostics_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &signature,
                ));
                diagnostics.extend(direct_source_function_type_diagnostics_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &signature,
                ));
                continue;
            }
        }

        if let Some(signature) =
            direct_method_signatures.get(&(class_decl.name.clone(), call.method.clone()))
        {
            if let Some(diagnostic) = direct_source_function_arity_diagnostic_with_context(
                context,
                node,
                nodes,
                &direct_call,
                signature,
            ) {
                diagnostics.push(diagnostic);
            }
            diagnostics.extend(direct_source_function_keyword_diagnostics_with_context(
                context,
                node,
                nodes,
                &direct_call,
                signature,
            ));
            diagnostics.extend(direct_source_function_type_diagnostics_with_context(
                context,
                node,
                nodes,
                &direct_call,
                signature,
            ));
            continue;
        }

        let method_signature = substitute_self_annotation(&target.detail, Some(&class_decl.name));
        let fallback_signature = direct_signature_params(&method_signature)
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
            .collect::<Vec<_>>();
        let fallback_signature =
            match target.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
                typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => {
                    fallback_signature
                }
                typepython_syntax::MethodKind::Instance
                | typepython_syntax::MethodKind::Class
                | typepython_syntax::MethodKind::PropertySetter => {
                    fallback_signature.into_iter().skip(1).collect()
                }
            };
        if let Some(diagnostic) = direct_source_function_arity_diagnostic_with_context(
            context,
            node,
            nodes,
            &direct_call,
            &fallback_signature,
        ) {
            diagnostics.push(diagnostic);
        }
        diagnostics.extend(direct_source_function_keyword_diagnostics_with_context(
            context,
            node,
            nodes,
            &direct_call,
            &fallback_signature,
        ));
        diagnostics.extend(direct_source_function_type_diagnostics_with_context(
            context,
            node,
            nodes,
            &direct_call,
            &fallback_signature,
        ));
    }

    diagnostics
}

pub(super) fn direct_method_signature_sites(
    declaration: &Declaration,
    owner_type_name: &str,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    let method_signature = substitute_self_annotation(&declaration.detail, Some(owner_type_name));
    let params = direct_signature_params(&method_signature).unwrap_or_default();
    let params = match declaration.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
        typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => params,
        typepython_syntax::MethodKind::Instance
        | typepython_syntax::MethodKind::Class
        | typepython_syntax::MethodKind::PropertySetter => params.into_iter().skip(1).collect(),
    };

    params
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

pub(super) fn method_overload_is_applicable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    declaration: &Declaration,
    owner_type_name: &str,
) -> bool {
    let method_signature = substitute_self_annotation(&declaration.detail, Some(owner_type_name));
    let params = direct_signature_params(&method_signature).unwrap_or_default();
    let params = match declaration.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
        typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => params,
        typepython_syntax::MethodKind::Instance
        | typepython_syntax::MethodKind::Class
        | typepython_syntax::MethodKind::PropertySetter => params.into_iter().skip(1).collect(),
    };
    call_signature_params_are_applicable(node, nodes, call, &params)
}

pub(super) fn resolve_method_call_owner_type(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::MethodCallSite,
) -> Option<String> {
    if call.through_instance {
        return resolve_direct_callable_return_type(node, nodes, &call.owner_name)
            .map(|return_type| normalize_type_text(&return_type))
            .or_else(|| Some(call.owner_name.clone()));
    }

    resolve_direct_name_reference_type_with_context(
        context,
        node,
        nodes,
        None,
        None,
        None,
        None,
        call.line,
        &call.owner_name,
    )
    .or_else(|| Some(call.owner_name.clone()))
}

pub(super) fn direct_call_arity_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.calls
        .iter()
        .filter_map(|call| {
            if let Some(shape) = resolve_synthesized_dataclass_class_shape(node, nodes, &call.callee)
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
                resolve_synthesized_dataclass_class_shape(node, nodes, &call.callee)
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
        if let Some(shape) = resolve_synthesized_dataclass_class_shape(node, nodes, &call.callee)
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

pub(super) fn resolve_direct_callable_param_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<Vec<String>> {
    if let Some(local) = resolve_direct_function(node, nodes, callee) {
        return Some(direct_param_types(&local.detail).unwrap_or_default());
    }

    if let Some(shape) = resolve_synthesized_dataclass_class_shape(node, nodes, callee)
        && !shape.has_explicit_init
    {
        return Some(
            shape
                .fields
                .iter()
                .filter(|field| !field.kw_only)
                .map(|field| field.annotation.clone())
                .collect(),
        );
    }

    if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, callee) {
        let init = class_node.declarations.iter().find(|declaration| {
            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
                && declaration.name == "__init__"
                && declaration.kind == DeclarationKind::Function
        });
        let param_types = init
            .and_then(|declaration| direct_param_types(&declaration.detail))
            .unwrap_or_default();
        return Some(param_types.into_iter().skip(1).collect());
    }

    if let Some(signature) = resolve_typing_callable_signature(callee) {
        return direct_param_types(signature);
    }

    None
}

pub(super) fn resolve_instantiated_direct_function_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    function: &Declaration,
    call: &typepython_binding::CallSite,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    if function.type_params.is_empty() {
        return None;
    }

    let signature = direct_signature_sites_from_detail(&function.detail);
    let substitutions =
        infer_generic_type_param_substitutions(node, nodes, function, &signature, call)?;
    instantiate_direct_function_signature(&signature, &substitutions)
}

pub(super) fn resolve_direct_function_with_node<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    resolve_function_provider_with_node(nodes, node, callee)
}

pub(super) fn resolve_direct_function<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Option<&'a Declaration> {
    resolve_direct_function_with_node(node, nodes, callee).map(|(_, declaration)| declaration)
}

pub(super) fn resolve_function_provider_with_node<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    name: &str,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    if let Some(local) = node.declarations.iter().find(|declaration| {
        declaration.owner.is_none()
            && declaration.kind == DeclarationKind::Function
            && declaration.name == name
    }) {
        return Some((node, local));
    }

    if let Some((module_path, symbol_name)) = name.rsplit_once('.') {
        if let Some(target_node) =
            nodes.iter().find(|candidate| candidate.module_key == module_path)
            && let Some(target) = target_node.declarations.iter().find(|declaration| {
                declaration.owner.is_none()
                    && declaration.kind == DeclarationKind::Function
                    && declaration.name == symbol_name
            })
        {
            return Some((target_node, target));
        }

        if let Some((head, tail)) = module_path.split_once('.')
            && let Some(import) = node.declarations.iter().find(|declaration| {
                declaration.kind == DeclarationKind::Import && declaration.name == head
            })
        {
            let resolved_module = format!("{}.{}", import.detail, tail);
            if let Some(target_node) =
                nodes.iter().find(|candidate| candidate.module_key == resolved_module)
                && let Some(target) = target_node.declarations.iter().find(|declaration| {
                    declaration.owner.is_none()
                        && declaration.kind == DeclarationKind::Function
                        && declaration.name == symbol_name
                })
            {
                return Some((target_node, target));
            }
        }
    }

    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == name
    })?;
    let (module_key, symbol_name) = import.detail.rsplit_once('.')?;
    let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
    target_node
        .declarations
        .iter()
        .find(|declaration| {
            declaration.owner.is_none()
                && declaration.kind == DeclarationKind::Function
                && declaration.name == symbol_name
        })
        .map(|declaration| (target_node, declaration))
}

pub(super) fn resolve_decorated_callable_site_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
) -> Option<typepython_syntax::DecoratedCallableSite> {
    let info = context.load_decorator_transform_module_info(node)?;
    info.callables.into_iter().find(|site| {
        site.name == declaration.name
            && site.owner_type_name.as_deref()
                == declaration.owner.as_ref().map(|owner| owner.name.as_str())
    })
}

pub(super) fn callable_annotation_from_signature_sites_in_module(
    node: &typepython_graph::ModuleNode,
    signature: &[typepython_syntax::DirectFunctionParamSite],
    return_type: &str,
) -> String {
    let param_types = signature
        .iter()
        .map(|param| {
            param
                .annotation
                .as_deref()
                .map(|annotation| rewrite_imported_typing_aliases(node, annotation))
                .unwrap_or_else(|| String::from("dynamic"))
        })
        .collect::<Vec<_>>();
    let return_type = rewrite_imported_typing_aliases(node, return_type);
    format_callable_annotation(&param_types, &return_type)
}

pub(super) fn synthetic_direct_expr_metadata(
    value_type: &str,
) -> typepython_syntax::DirectExprMetadata {
    typepython_syntax::DirectExprMetadata {
        value_type: Some(value_type.to_owned()),
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

pub(super) fn synthetic_decorator_application_call(
    decorator_name: &str,
    callable_annotation: &str,
) -> typepython_binding::CallSite {
    typepython_binding::CallSite {
        callee: decorator_name.to_owned(),
        arg_count: 1,
        arg_types: vec![callable_annotation.to_owned()],
        arg_values: vec![synthetic_direct_expr_metadata(callable_annotation)],
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    }
}

pub(super) fn decorator_transform_accepts_callable_annotation_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> bool {
    direct_source_function_arity_diagnostic_with_context(context, node, nodes, call, signature)
        .is_none()
        && direct_source_function_keyword_diagnostics_with_context(
            context, node, nodes, call, signature,
        )
        .is_empty()
        && direct_source_function_type_diagnostics_with_context(
            context, node, nodes, call, signature,
        )
        .is_empty()
}

#[cfg(test)]
pub(super) fn apply_named_callable_decorator_transform(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    decorator_name: &str,
    current_callable: &str,
) -> Option<String> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    apply_named_callable_decorator_transform_with_context(
        &context,
        node,
        nodes,
        decorator_name,
        current_callable,
    )
}

pub(super) fn apply_named_callable_decorator_transform_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    decorator_name: &str,
    current_callable: &str,
) -> Option<String> {
    let (decorator_node, decorator) =
        resolve_function_provider_with_node(nodes, node, decorator_name)?;
    let call = synthetic_decorator_application_call(decorator_name, current_callable);
    let signature = if decorator.type_params.is_empty() {
        direct_signature_sites_from_detail(&decorator.detail)
    } else {
        resolve_instantiated_direct_function_signature(decorator_node, nodes, decorator, &call)?
    };
    if !decorator_transform_accepts_callable_annotation_with_context(
        context,
        decorator_node,
        nodes,
        &call,
        &signature,
    ) {
        return None;
    }

    let transformed = if decorator.type_params.is_empty() {
        rewrite_imported_typing_aliases(decorator_node, decorator.detail.split_once("->")?.1.trim())
    } else {
        let instantiated = resolve_instantiated_callable_return_type_from_declaration(
            decorator_node,
            nodes,
            decorator,
            &call,
        )?;
        rewrite_imported_typing_aliases(decorator_node, &instantiated)
    };
    let (params, return_type) = parse_callable_annotation(&transformed)?;
    Some(format_callable_annotation(&params?, &return_type))
}

pub(super) fn resolve_decorated_callable_annotation_for_declaration(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
) -> Option<String> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    resolve_decorated_callable_annotation_for_declaration_with_context(
        &context,
        node,
        nodes,
        declaration,
    )
}

pub(super) fn resolve_decorated_callable_annotation_for_declaration_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
) -> Option<String> {
    let decorated = resolve_decorated_callable_site_with_context(context, node, declaration)?;
    if decorated.decorators.is_empty() {
        return None;
    }

    let base_signature = direct_signature_sites_from_detail(&declaration.detail);
    let base_return = if declaration.is_async {
        format!("Awaitable[{}]", declaration.detail.split_once("->")?.1.trim())
    } else {
        declaration.detail.split_once("->")?.1.trim().to_owned()
    };
    let mut current =
        callable_annotation_from_signature_sites_in_module(node, &base_signature, &base_return);
    for decorator in decorated.decorators.iter().rev() {
        current = apply_named_callable_decorator_transform_with_context(
            context, node, nodes, decorator, &current,
        )?;
    }
    Some(current)
}

pub(super) fn resolve_decorated_function_callable_annotation(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<String> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    resolve_decorated_function_callable_annotation_with_context(&context, node, nodes, callee)
}

pub(super) fn resolve_decorated_function_callable_annotation_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<String> {
    let (function_node, function) = resolve_direct_function_with_node(node, nodes, callee)?;
    resolve_decorated_callable_annotation_for_declaration_with_context(
        context,
        function_node,
        nodes,
        function,
    )
}

pub(super) fn direct_function_signature_sites_from_callable_annotation(
    callable_annotation: &str,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    let (params, _return_type) = parse_callable_annotation(callable_annotation)?;
    Some(synthesize_param_list_binding(params?).into_iter().collect())
}

pub(super) fn decorated_function_return_type_from_callable_annotation(
    callable_annotation: &str,
) -> Option<String> {
    parse_callable_annotation(callable_annotation).map(|(_, return_type)| return_type)
}

pub(super) fn resolve_direct_callable_return_type<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Option<String> {
    if let Some(callable_annotation) =
        resolve_decorated_function_callable_annotation(node, nodes, callee)
    {
        return decorated_function_return_type_from_callable_annotation(&callable_annotation);
    }
    if let Some(function) = resolve_direct_function(node, nodes, callee) {
        let return_type = substitute_self_annotation(
            function.detail.split_once("->")?.1.trim(),
            function.owner.as_ref().map(|owner| owner.name.as_str()),
        );
        return Some(if function.is_async && !return_type.is_empty() {
            format!("Awaitable[{return_type}]")
        } else {
            return_type
        });
    }

    if let Some((_, class_decl)) = resolve_direct_base(nodes, node, callee) {
        return Some(class_decl.name.clone());
    }

    if let Some(signature) = resolve_typing_callable_signature(callee) {
        return Some(signature.split_once("->")?.1.trim().to_owned());
    }

    resolve_builtin_return_type(callee).map(str::to_owned)
}

pub(super) fn resolve_instantiated_callable_return_type_from_declaration(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
    call: &typepython_binding::CallSite,
) -> Option<String> {
    if declaration.type_params.is_empty() {
        return Some(declaration.detail.split_once("->")?.1.trim().to_owned());
    }
    let signature = direct_signature_sites_from_detail(&declaration.detail);
    let substitutions =
        infer_generic_type_param_substitutions(node, nodes, declaration, &signature, call)?;
    Some(substitute_generic_type_params(
        declaration.detail.split_once("->")?.1.trim(),
        &substitutions,
    ))
}

pub(super) fn resolve_direct_callable_return_type_for_line(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
    line: usize,
) -> Option<String> {
    let call = node
        .calls
        .iter()
        .find(|call| call.callee == callee && call.line == line)
        .or_else(|| node.calls.iter().find(|call| call.callee == callee))?;
    let overloads = resolve_direct_overloads(node, nodes, callee);
    if !overloads.is_empty() {
        let applicable = overloads
            .into_iter()
            .filter(|declaration| {
                overload_is_applicable_with_context(node, nodes, call, declaration)
            })
            .collect::<Vec<_>>();
        let selected = select_most_specific_overload(node, nodes, &applicable)?;
        return resolve_instantiated_callable_return_type_from_declaration(
            node, nodes, selected, call,
        );
    }
    if let Some(callable_annotation) =
        resolve_decorated_function_callable_annotation(node, nodes, callee)
    {
        return decorated_function_return_type_from_callable_annotation(&callable_annotation);
    }
    let function = resolve_direct_function(node, nodes, callee)?;
    resolve_instantiated_callable_return_type_from_declaration(node, nodes, function, call)
}

pub(super) fn resolve_direct_callable_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<(usize, Vec<String>)> {
    if let Some(signature) = resolve_direct_callable_signature_sites(node, nodes, callee) {
        return Some((
            signature
                .iter()
                .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
                .count(),
            signature.iter().map(|param| param.name.clone()).collect(),
        ));
    }
    if let Some(local) = resolve_direct_function(node, nodes, callee) {
        return Some((
            direct_param_count(&local.detail).unwrap_or_default(),
            direct_param_names(&local.detail).unwrap_or_default(),
        ));
    }

    if let Some(shape) = resolve_synthesized_dataclass_class_shape(node, nodes, callee)
        && !shape.has_explicit_init
    {
        return Some((
            shape.fields.iter().filter(|field| !field.kw_only).count(),
            shape.fields.iter().map(|field| field.keyword_name.clone()).collect(),
        ));
    }

    if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, callee) {
        let init = class_node.declarations.iter().find(|declaration| {
            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
                && declaration.name == "__init__"
                && declaration.kind == DeclarationKind::Function
        });
        let param_names = init
            .and_then(|declaration| direct_param_names(&declaration.detail))
            .unwrap_or_default();
        let arg_count = param_names.len().saturating_sub(1);
        return Some((arg_count, param_names.into_iter().skip(1).collect()));
    }

    if let Some(signature) = resolve_typing_callable_signature(callee) {
        return Some((
            direct_param_count(signature).unwrap_or_default(),
            direct_param_names(signature).unwrap_or_default(),
        ));
    }

    let function = resolve_direct_function(node, nodes, callee)?;
    Some((
        direct_param_count(&function.detail).unwrap_or_default(),
        direct_param_names(&function.detail).unwrap_or_default(),
    ))
}

pub(super) fn resolve_synthesized_dataclass_class_shape(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<DataclassTransformClassShape> {
    resolve_dataclass_transform_class_shape(node, nodes, callee)
        .or_else(|| resolve_plain_dataclass_class_shape(node, nodes, callee))
}

pub(super) fn resolve_plain_dataclass_class_shape(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<DataclassTransformClassShape> {
    let (class_node, class_decl) = resolve_direct_base(nodes, node, callee)?;
    resolve_plain_dataclass_class_shape_from_decl(
        nodes,
        class_node,
        class_decl,
        &mut BTreeSet::new(),
    )
}

pub(super) fn resolve_plain_dataclass_class_shape_from_decl(
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visiting: &mut BTreeSet<(String, String)>,
) -> Option<DataclassTransformClassShape> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visiting.insert(key) {
        return None;
    }

    let info = load_dataclass_transform_module_info(class_node)?;
    let class_site = info.classes.iter().find(|class_site| class_site.name == class_decl.name)?;
    let is_plain_dataclass = class_decl.class_kind == Some(DeclarationOwnerKind::DataClass)
        || class_site
            .decorators
            .iter()
            .any(|decorator| matches!(decorator.as_str(), "dataclass" | "dataclasses.dataclass"));
    if !is_plain_dataclass {
        return None;
    }

    let has_explicit_init = !class_site.plain_dataclass_init
        || class_site.methods.iter().any(|method| method == "__init__");

    let mut fields = Vec::new();
    for base in &class_site.bases {
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) else {
            continue;
        };
        let mut branch_visiting = visiting.clone();
        let inherited = resolve_plain_dataclass_class_shape_from_decl(
            nodes,
            base_node,
            base_decl,
            &mut branch_visiting,
        )
        .or_else(|| {
            resolve_dataclass_transform_class_shape_from_decl(
                nodes,
                base_node,
                base_decl,
                &mut branch_visiting,
            )
        });
        let Some(inherited) = inherited else {
            continue;
        };
        for field in inherited.fields {
            if let Some(index) = fields
                .iter()
                .position(|existing: &DataclassTransformFieldShape| existing.name == field.name)
            {
                fields.remove(index);
            }
            fields.push(field);
        }
    }

    let local_fields = class_site
        .fields
        .iter()
        .filter(|field| !field.is_class_var)
        .filter_map(|field| {
            let recognized_field_specifier = field
                .field_specifier_name
                .as_ref()
                .is_some_and(|name| matches!(name.as_str(), "field" | "dataclasses.field"));
            if recognized_field_specifier && field.field_specifier_init == Some(false) {
                return None;
            }
            Some(DataclassTransformFieldShape {
                name: field.name.clone(),
                keyword_name: field.name.clone(),
                annotation: rewrite_imported_typing_aliases(class_node, &field.annotation),
                required: if recognized_field_specifier {
                    !(field.field_specifier_has_default
                        || field.field_specifier_has_default_factory)
                } else {
                    !field.has_default
                },
                kw_only: if recognized_field_specifier {
                    field.field_specifier_kw_only.unwrap_or(class_site.plain_dataclass_kw_only)
                } else {
                    class_site.plain_dataclass_kw_only
                },
            })
        })
        .collect::<Vec<_>>();
    for field in local_fields {
        if let Some(index) = fields
            .iter()
            .position(|existing: &DataclassTransformFieldShape| existing.name == field.name)
        {
            fields.remove(index);
        }
        fields.push(field);
    }

    Some(DataclassTransformClassShape {
        fields,
        frozen: class_site.plain_dataclass_frozen,
        has_explicit_init,
    })
}

pub(super) fn resolve_known_plain_dataclass_shape_from_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<DataclassTransformClassShape> {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    resolve_plain_dataclass_class_shape(node, nodes, &type_name)
}

pub(super) fn resolve_dataclass_transform_class_shape(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<DataclassTransformClassShape> {
    let (class_node, class_decl) = resolve_direct_base(nodes, node, callee)?;
    resolve_dataclass_transform_class_shape_from_decl(
        nodes,
        class_node,
        class_decl,
        &mut BTreeSet::new(),
    )
}

pub(super) fn resolve_known_dataclass_transform_shape_from_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<DataclassTransformClassShape> {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    resolve_dataclass_transform_class_shape(node, nodes, &type_name)
}

pub(super) fn resolve_dataclass_transform_metadata_from_decl(
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visiting: &mut BTreeSet<(String, String)>,
) -> Option<typepython_syntax::DataclassTransformMetadata> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visiting.insert(key) {
        return None;
    }

    let info = load_dataclass_transform_module_info(class_node)?;
    let class_site = info.classes.iter().find(|class_site| class_site.name == class_decl.name)?;

    for decorator in &class_site.decorators {
        if let Some(provider) = resolve_dataclass_transform_provider(nodes, class_node, decorator) {
            return Some(provider.metadata.clone());
        }
    }
    if let Some(provider_name) = class_site
        .bases
        .iter()
        .find(|base| resolve_dataclass_transform_provider(nodes, class_node, base).is_some())
    {
        return resolve_dataclass_transform_provider(nodes, class_node, provider_name)
            .map(|provider| provider.metadata.clone());
    }
    if let Some(metaclass) = class_site.metaclass.as_deref() {
        if let Some(provider) = resolve_dataclass_transform_provider(nodes, class_node, metaclass) {
            return Some(provider.metadata.clone());
        }
    }

    class_site.bases.iter().find_map(|base| {
        let (base_node, base_decl) = resolve_direct_base(nodes, class_node, base)?;
        let mut branch_visiting = visiting.clone();
        resolve_dataclass_transform_metadata_from_decl(
            nodes,
            base_node,
            base_decl,
            &mut branch_visiting,
        )
    })
}

pub(super) fn resolve_dataclass_transform_class_shape_from_decl(
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visiting: &mut BTreeSet<(String, String)>,
) -> Option<DataclassTransformClassShape> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visiting.insert(key) {
        return None;
    }

    let has_explicit_init = class_node.declarations.iter().any(|declaration| {
        declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration.name == "__init__"
            && declaration.kind == DeclarationKind::Function
    });

    let info = load_dataclass_transform_module_info(class_node)?;
    let class_site = info.classes.iter().find(|class_site| class_site.name == class_decl.name)?;

    let mut metadata = None;
    for decorator in &class_site.decorators {
        if let Some(provider) = resolve_dataclass_transform_provider(nodes, class_node, decorator) {
            metadata = Some(provider.metadata.clone());
            break;
        }
    }
    if metadata.is_none() {
        if let Some(provider_name) = class_site
            .bases
            .iter()
            .find(|base| resolve_dataclass_transform_provider(nodes, class_node, base).is_some())
        {
            metadata = resolve_dataclass_transform_provider(nodes, class_node, provider_name)
                .map(|provider| provider.metadata.clone());
        }
    }
    if metadata.is_none() {
        metadata = class_site
            .metaclass
            .as_deref()
            .and_then(|metaclass| {
                resolve_dataclass_transform_provider(nodes, class_node, metaclass)
            })
            .map(|provider| provider.metadata.clone());
    }
    if metadata.is_none() {
        metadata = class_site.bases.iter().find_map(|base| {
            let (base_node, base_decl) = resolve_direct_base(nodes, class_node, base)?;
            let mut branch_visiting = visiting.clone();
            resolve_dataclass_transform_metadata_from_decl(
                nodes,
                base_node,
                base_decl,
                &mut branch_visiting,
            )
        });
    }
    let metadata = metadata?;

    let mut fields = Vec::new();
    for base in &class_site.bases {
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) else {
            continue;
        };
        let mut branch_visiting = visiting.clone();
        let Some(base_shape) = resolve_dataclass_transform_class_shape_from_decl(
            nodes,
            base_node,
            base_decl,
            &mut branch_visiting,
        ) else {
            continue;
        };
        for field in base_shape.fields {
            if let Some(index) = fields
                .iter()
                .position(|existing: &DataclassTransformFieldShape| existing.name == field.name)
            {
                fields.remove(index);
            }
            fields.push(field);
        }
    }

    for field in &class_site.fields {
        if field.is_class_var {
            continue;
        }
        let recognized_specifier = field.field_specifier_name.as_ref().is_some_and(|name| {
            metadata
                .field_specifiers
                .iter()
                .any(|candidate| candidate == name || candidate.ends_with(&format!(".{name}")))
        });
        if !recognized_specifier
            && field
                .value_metadata
                .as_ref()
                .and_then(|metadata| {
                    resolve_direct_expression_type_from_metadata(
                        class_node,
                        nodes,
                        None,
                        None,
                        Some(&class_decl.name),
                        field.line,
                        metadata,
                    )
                })
                .is_some_and(|value_type| is_descriptor_type(nodes, class_node, &value_type))
        {
            continue;
        }
        let init =
            if recognized_specifier { field.field_specifier_init.unwrap_or(true) } else { true };
        if !init {
            continue;
        }
        let required = if recognized_specifier {
            !(field.field_specifier_has_default
                || field.field_specifier_has_default_factory
                || (field.has_default && field.field_specifier_name.is_none()))
        } else {
            !field.has_default
        };
        let kw_only = if recognized_specifier {
            field.field_specifier_kw_only.unwrap_or(metadata.kw_only_default)
        } else {
            metadata.kw_only_default
        };
        let synthesized = DataclassTransformFieldShape {
            name: field.name.clone(),
            keyword_name: field.field_specifier_alias.clone().unwrap_or_else(|| field.name.clone()),
            annotation: rewrite_imported_typing_aliases(class_node, &field.annotation),
            required,
            kw_only,
        };
        if let Some(index) = fields.iter().position(|existing| existing.name == synthesized.name) {
            fields.remove(index);
        }
        fields.push(synthesized);
    }

    Some(DataclassTransformClassShape {
        fields,
        frozen: metadata.frozen_default,
        has_explicit_init,
    })
}

pub(super) fn is_descriptor_type(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    type_name: &str,
) -> bool {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &type_name) else {
        return false;
    };

    ["__get__", "__set__", "__delete__"].iter().any(|member_name| {
        find_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
            matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })
        .is_some()
    })
}

pub(super) fn load_dataclass_transform_module_info_uncached(
    node: &typepython_graph::ModuleNode,
) -> Option<typepython_syntax::DataclassTransformModuleInfo> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return None;
    }
    let source = fs::read_to_string(&node.module_path).ok()?;
    Some(typepython_syntax::collect_dataclass_transform_module_info(&source))
}

pub(super) fn resolve_dataclass_transform_provider<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    name: &str,
) -> Option<typepython_syntax::DataclassTransformProviderSite> {
    if let Some(local) = load_dataclass_transform_module_info(node)?
        .providers
        .into_iter()
        .find(|provider| provider.name == name)
    {
        return Some(local);
    }

    if let Some((module_alias, symbol_name)) = name.rsplit_once('.') {
        if let Some(import) = node.declarations.iter().find(|declaration| {
            declaration.kind == DeclarationKind::Import && declaration.name == module_alias
        }) {
            if let Some(target_node) =
                nodes.iter().find(|candidate| candidate.module_key == import.detail)
            {
                return load_dataclass_transform_module_info(target_node)?
                    .providers
                    .into_iter()
                    .find(|provider| provider.name == symbol_name);
            }
        }
    }

    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == name
    })?;
    let (module_key, symbol_name) = import.detail.rsplit_once('.')?;
    let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
    load_dataclass_transform_module_info(target_node)?
        .providers
        .into_iter()
        .find(|provider| provider.name == symbol_name)
}

pub(super) fn unresolved_import_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let project_roots: BTreeSet<_> = nodes
        .iter()
        .filter_map(|candidate| candidate.module_key.split('.').next())
        .map(str::to_owned)
        .collect();

    node.declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Import)
        .filter_map(|declaration| {
            let root = declaration.detail.split('.').next()?;
            if !project_roots.contains(root) {
                return None;
            }

            let resolves = nodes.iter().any(|candidate| candidate.module_key == declaration.detail)
                || declaration
                    .detail
                    .rsplit_once('.')
                    .and_then(|(module_key, symbol_name)| {
                        nodes.iter().find(|candidate| candidate.module_key == module_key).map(
                            |target| {
                                target.declarations.iter().any(|declaration| {
                                    declaration.owner.is_none() && declaration.name == symbol_name
                                })
                            },
                        )
                    })
                    .unwrap_or(false);

            (!resolves).then(|| {
                Diagnostic::error(
                    "TPY3001",
                    format!(
                        "module `{}` imports unresolved same-project target `{}`",
                        node.module_path.display(),
                        declaration.detail
                    ),
                )
            })
        })
        .collect()
}

pub(super) fn deprecated_use_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    report_deprecated: DiagnosticLevel,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for declaration in
        node.declarations.iter().filter(|declaration| declaration.kind == DeclarationKind::Import)
    {
        if let Some(target) = resolve_import_target(node, nodes, declaration) {
            if target.is_deprecated {
                if let Some(diagnostic) = deprecated_diagnostic(
                    report_deprecated,
                    format!(
                        "module `{}` imports deprecated declaration `{}`",
                        node.module_path.display(),
                        declaration.name
                    ),
                    target.deprecation_message.as_deref(),
                ) {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    for call in &node.calls {
        if let Some(target) = resolve_direct_function(node, nodes, &call.callee) {
            if target.is_deprecated {
                if let Some(diagnostic) = deprecated_diagnostic(
                    report_deprecated,
                    format!(
                        "module `{}` calls deprecated declaration `{}`",
                        node.module_path.display(),
                        call.callee
                    ),
                    target.deprecation_message.as_deref(),
                ) {
                    diagnostics.push(diagnostic);
                }
            }
        } else if let Some((_, target)) = resolve_direct_base(nodes, node, &call.callee) {
            if target.is_deprecated {
                if let Some(diagnostic) = deprecated_diagnostic(
                    report_deprecated,
                    format!(
                        "module `{}` instantiates deprecated declaration `{}`",
                        node.module_path.display(),
                        call.callee
                    ),
                    target.deprecation_message.as_deref(),
                ) {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    for access in &node.member_accesses {
        if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &access.owner_name)
        {
            if let Some(member) =
                find_owned_value_declaration(nodes, class_node, class_decl, &access.member)
            {
                if member.is_deprecated {
                    if let Some(diagnostic) = deprecated_diagnostic(
                        report_deprecated,
                        format!(
                            "module `{}` uses deprecated member `{}` on `{}`",
                            node.module_path.display(),
                            access.member,
                            access.owner_name
                        ),
                        member.deprecation_message.as_deref(),
                    ) {
                        diagnostics.push(diagnostic);
                    }
                }
            }
        }
    }

    for call in &node.method_calls {
        if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &call.owner_name) {
            if let Some(method) =
                find_owned_callable_declaration(nodes, class_node, class_decl, &call.method)
            {
                if method.is_deprecated {
                    if let Some(diagnostic) = deprecated_diagnostic(
                        report_deprecated,
                        format!(
                            "module `{}` calls deprecated member `{}` on `{}`",
                            node.module_path.display(),
                            call.method,
                            call.owner_name
                        ),
                        method.deprecation_message.as_deref(),
                    ) {
                        diagnostics.push(diagnostic);
                    }
                }
            }
        }
    }

    diagnostics
}

pub(super) fn deprecated_diagnostic(
    report_deprecated: DiagnosticLevel,
    message: String,
    deprecation_message: Option<&str>,
) -> Option<Diagnostic> {
    let diagnostic = match report_deprecated {
        DiagnosticLevel::Ignore => return None,
        DiagnosticLevel::Warning => Diagnostic::warning("TPY4101", message),
        DiagnosticLevel::Error => Diagnostic::error("TPY4101", message),
    };
    Some(match deprecation_message {
        Some(note) if !note.is_empty() => diagnostic.with_note(note),
        _ => diagnostic,
    })
}

pub(super) fn type_supports_mismatch_path(text: &str) -> bool {
    union_branches(text).is_some() || split_generic_type(text).is_some()
}

pub(super) fn attach_type_mismatch_notes(
    diagnostic: Diagnostic,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> Diagnostic {
    let expected = normalize_type_text(expected);
    let actual = normalize_type_text(actual);
    if !type_supports_mismatch_path(&expected) && !type_supports_mismatch_path(&actual) {
        return diagnostic;
    }

    let mut diagnostic = diagnostic
        .with_note(format!("source: `{actual}`"))
        .with_note(format!("target: `{expected}`"));
    let mut path = Vec::new();
    if let Some(detail) = first_type_mismatch_detail(node, nodes, &expected, &actual, &mut path, 8)
    {
        if !path.is_empty() {
            diagnostic = diagnostic.with_note(format!(
                "mismatch at: {}",
                path.iter().map(|segment| format!("-> {segment}")).collect::<Vec<_>>().join(" ")
            ));
        }
        diagnostic = diagnostic.with_note(detail);
    }
    diagnostic
}

pub(super) fn first_type_mismatch_detail(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
    path: &mut Vec<String>,
    depth: usize,
) -> Option<String> {
    if depth == 0 {
        return None;
    }

    let expected = normalize_type_text(expected);
    let actual = normalize_type_text(actual);
    if path.last().is_none_or(|segment| segment != &actual) {
        path.push(actual.clone());
    }

    if let (Some(expected_branches), Some(actual_branches)) =
        (union_branches(&expected), union_branches(&actual))
    {
        if let Some(unmatched) = actual_branches.iter().find(|branch| {
            !expected_branches
                .iter()
                .any(|target_branch| direct_type_is_assignable(node, nodes, target_branch, branch))
        }) {
            if path.last().is_none_or(|segment| segment != unmatched) {
                path.push(unmatched.clone());
            }
            return Some(format!(
                "union branch `{}` is not assignable to any target branch in `{}`",
                unmatched, expected
            ));
        }
    }

    if let (Some((expected_head, expected_args)), Some((actual_head, actual_args))) =
        (split_generic_type(&expected), split_generic_type(&actual))
    {
        let (expected_args, actual_args) = if expected_head == "tuple" && actual_head == "tuple" {
            (expanded_tuple_shape_args(&expected_args), expanded_tuple_shape_args(&actual_args))
        } else {
            (expected_args, actual_args)
        };
        if expected_head == "tuple"
            && actual_head == "tuple"
            && actual_args.len() == 2
            && actual_args[1] == "..."
            && !(expected_args.len() == 2 && expected_args[1] == "...")
        {
            return Some(format!(
                "`{actual}` is a variable-length tuple and is not assignable to fixed tuple `{expected}`"
            ));
        }

        if expected_head == actual_head && expected_args.len() == actual_args.len() {
            for (expected_arg, actual_arg) in expected_args.iter().zip(actual_args.iter()) {
                if !direct_type_is_assignable(node, nodes, expected_arg, actual_arg) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        expected_arg,
                        actual_arg,
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!("`{}` is not assignable to `{}`", actual_arg, expected_arg))
                    });
                }
            }
            if expected_head == "tuple" && expected_args != actual_args {
                return Some(format!("tuple shape `{actual}` is not assignable to `{expected}`"));
            }
        }

        match (expected_head, actual_head) {
            ("Sequence", "list") | ("Sequence", "tuple") if !expected_args.is_empty() => {
                let actual_element = if actual_head == "tuple" {
                    if actual_args.len() == 2 && actual_args[1] == "..." {
                        actual_args[0].clone()
                    } else {
                        join_branch_types(actual_args.clone())
                    }
                } else {
                    actual_args.first().cloned().unwrap_or_default()
                };
                if !direct_type_is_assignable(node, nodes, &expected_args[0], &actual_element) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        &expected_args[0],
                        &actual_element,
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!(
                            "element type `{}` is not assignable to `{}`",
                            actual_element, expected_args[0]
                        ))
                    });
                }
            }
            ("Mapping", "dict") if expected_args.len() == 2 && actual_args.len() == 2 => {
                if !direct_type_is_assignable(node, nodes, &expected_args[0], &actual_args[0]) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        &expected_args[0],
                        &actual_args[0],
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!(
                            "mapping key type `{}` is not assignable to `{}`",
                            actual_args[0], expected_args[0]
                        ))
                    });
                }
                if !direct_type_is_assignable(node, nodes, &expected_args[1], &actual_args[1]) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        &expected_args[1],
                        &actual_args[1],
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!(
                            "mapping value type `{}` is not assignable to `{}`",
                            actual_args[1], expected_args[1]
                        ))
                    });
                }
            }
            _ => {}
        }
    }

    None
}

pub(super) fn same_return_owner(
    left: &typepython_binding::ReturnSite,
    right: &typepython_binding::ReturnSite,
) -> bool {
    left.owner_name == right.owner_name && left.owner_type_name == right.owner_type_name
}

pub(super) fn describe_return_trace_expression(
    return_site: &typepython_binding::ReturnSite,
) -> String {
    if return_site.value_name.is_none()
        && return_site.value_member_name.is_none()
        && return_site.value_method_name.is_none()
        && return_site.value_callee.is_none()
        && return_site.value_lambda.is_none()
        && return_site.value_list_elements.is_none()
        && return_site.value_set_elements.is_none()
        && return_site.value_dict_entries.is_none()
        && return_site.value_subscript_target.is_none()
    {
        return String::from("bare return");
    }
    if let Some(name) = &return_site.value_name {
        return format!("return {name}");
    }
    if let (Some(owner), Some(member)) =
        (&return_site.value_member_owner_name, &return_site.value_member_name)
    {
        return format!("return {}.{}", owner, member);
    }
    if let (Some(owner), Some(method)) =
        (&return_site.value_method_owner_name, &return_site.value_method_name)
    {
        return format!("return {}.{}(...)", owner, method);
    }
    if let Some(callee) = &return_site.value_callee {
        return format!("return {}(...)", callee);
    }
    if let Some(key) = &return_site.value_subscript_string_key {
        return format!("return [...][\"{key}\"]");
    }
    if return_site.value_lambda.is_some() {
        return String::from("return lambda");
    }
    if return_site.value_dict_entries.is_some() {
        return String::from("return dict literal");
    }
    if return_site.value_list_elements.is_some() {
        return String::from("return list literal");
    }
    if return_site.value_set_elements.is_some() {
        return String::from("return set literal");
    }
    String::from("return expression")
}

pub(super) fn inferred_return_type_for_owner(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected: &str,
    signature: &str,
) -> Option<String> {
    let related_returns = node
        .returns
        .iter()
        .filter(|candidate| same_return_owner(candidate, return_site))
        .collect::<Vec<_>>();
    if related_returns.is_empty() {
        return None;
    }

    let mut trace_types = Vec::new();
    for candidate in related_returns {
        let contextual =
            resolve_contextual_return_type(node, nodes, candidate, expected, signature);
        let candidate_type = contextual
            .actual_type
            .or_else(|| candidate.value_type.clone())
            .unwrap_or_else(|| String::from("unknown"));
        trace_types.push(candidate_type);
    }

    Some(if trace_types.len() > 1 {
        join_branch_types(trace_types)
    } else {
        normalize_type_text(trace_types.first().expect("single return type should exist"))
    })
}

pub(super) fn attach_return_inference_trace(
    mut diagnostic: Diagnostic,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected: &str,
    actual: &str,
    signature: &str,
) -> Diagnostic {
    let related_returns = node
        .returns
        .iter()
        .filter(|candidate| same_return_owner(candidate, return_site))
        .collect::<Vec<_>>();
    if related_returns.is_empty() {
        return diagnostic;
    }

    let mut trace_types = Vec::new();
    let mut trace_lines = Vec::new();

    for candidate in related_returns {
        let contextual =
            resolve_contextual_return_type(node, nodes, candidate, expected, signature);
        let candidate_type = contextual
            .actual_type
            .or_else(|| candidate.value_type.clone())
            .unwrap_or_else(|| String::from("unknown"));
        trace_types.push(candidate_type.clone());
        trace_lines.push(format!(
            "line {}: {} -> {}",
            candidate.line,
            describe_return_trace_expression(candidate),
            normalize_type_text(&candidate_type)
        ));
    }

    let inferred_return_type =
        inferred_return_type_for_owner(node, nodes, return_site, expected, signature)
            .unwrap_or_else(|| normalize_type_text(actual));
    diagnostic = diagnostic
        .with_note(format!(
            "inferred return type: `{}`",
            normalize_type_text(&inferred_return_type)
        ))
        .with_note(format!("declared return type: `{}`", normalize_type_text(expected)))
        .with_note(String::from("inference trace:"));

    for line in trace_lines {
        diagnostic = diagnostic.with_note(line);
    }

    if node.returns.iter().filter(|candidate| same_return_owner(candidate, return_site)).count() > 1
    {
        diagnostic =
            diagnostic.with_note(format!("join: `{}`", normalize_type_text(&inferred_return_type)));
    }

    diagnostic
}

pub(super) fn resolve_import_target<'a>(
    _node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    declaration: &'a Declaration,
) -> Option<&'a Declaration> {
    let (module_key, symbol_name) = declaration.detail.rsplit_once('.')?;
    let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
    target_node
        .declarations
        .iter()
        .find(|target| target.owner.is_none() && target.name == symbol_name)
}

pub(super) fn resolve_imported_module_target<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    local_name: &str,
) -> Option<&'a typepython_graph::ModuleNode> {
    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == local_name
    })?;
    nodes.iter().find(|candidate| candidate.module_key == import.detail)
}

pub(super) fn resolve_imported_module_member_reference_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    owner_name: &str,
    member_name: &str,
) -> Option<SemanticType> {
    let module_node = resolve_imported_module_target(node, nodes, owner_name)?;
    let declaration = module_node
        .declarations
        .iter()
        .find(|declaration| declaration.owner.is_none() && declaration.name == member_name)?;
    match declaration.kind {
        DeclarationKind::Value => {
            let detail = rewrite_imported_typing_aliases(node, &declaration.detail);
            normalized_direct_return_annotation(&detail).map(lower_type_text_or_name)
        }
        DeclarationKind::Function => {
            let param_types = direct_signature_sites_from_detail(&declaration.detail)
                .into_iter()
                .map(|param| {
                    lower_type_text_or_name(
                        &param.annotation.unwrap_or_else(|| String::from("dynamic")),
                    )
                })
                .collect::<Vec<_>>();
            let return_type =
                lower_type_text_or_name(declaration.detail.split_once("->")?.1.trim());
            Some(SemanticType::Callable {
                params: SemanticCallableParams::ParamList(param_types),
                return_type: Box::new(return_type),
            })
        }
        _ => None,
    }
}

pub(super) fn resolve_imported_module_method_return_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    owner_name: &str,
    method_name: &str,
) -> Option<SemanticType> {
    let module_node = resolve_imported_module_target(node, nodes, owner_name)?;
    let methods = module_node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.owner.is_none()
                && declaration.name == method_name
                && matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })
        .collect::<Vec<_>>();
    let method = if methods.iter().any(|declaration| declaration.kind == DeclarationKind::Overload)
    {
        let call = node.method_calls.iter().find(|call| {
            call.owner_name == owner_name
                && call.method == method_name
                && !call.through_instance
                && call.line == current_line
        })?;
        let call = imported_module_method_call_site(module_node, call);
        let applicable = methods
            .iter()
            .copied()
            .filter(|declaration| {
                overload_is_applicable_with_context(node, nodes, &call, declaration)
            })
            .collect::<Vec<_>>();
        if applicable.len() == 1 {
            applicable[0]
        } else {
            return None;
        }
    } else {
        *methods.first()?
    };
    let return_text =
        rewrite_imported_typing_aliases(node, method.detail.split_once("->")?.1.trim());
    normalized_direct_return_annotation(&return_text).map(lower_type_text_or_name)
}

pub(super) fn imported_module_method_call_site(
    module_node: &typepython_graph::ModuleNode,
    call: &typepython_binding::MethodCallSite,
) -> typepython_binding::CallSite {
    typepython_binding::CallSite {
        callee: format!("{}.{}", module_node.module_key, call.method),
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
    }
}

pub(super) fn imported_module_method_call_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::MethodCallSite,
) -> Option<Vec<Diagnostic>> {
    let module_node = resolve_imported_module_target(node, nodes, &call.owner_name)?;
    let direct_call = imported_module_method_call_site(module_node, call);
    let callable_candidates = module_node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.owner.is_none()
                && declaration.name == call.method
                && matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })
        .collect::<Vec<_>>();
    if callable_candidates.is_empty() {
        let has_member = module_node
            .declarations
            .iter()
            .any(|declaration| declaration.owner.is_none() && declaration.name == call.method);
        return Some(if has_member {
            Vec::new()
        } else {
            vec![Diagnostic::error(
                "TPY4002",
                format!(
                    "module `{}` in module `{}` has no member `{}`",
                    module_node.module_key,
                    node.module_path.display(),
                    call.method
                ),
            )]
        });
    }

    let mut diagnostics = Vec::new();
    let overloads = callable_candidates
        .iter()
        .copied()
        .filter(|declaration| declaration.kind == DeclarationKind::Overload)
        .collect::<Vec<_>>();
    if !overloads.is_empty() {
        let applicable = overloads
            .iter()
            .copied()
            .filter(|declaration| {
                overload_is_applicable_with_context(node, nodes, &direct_call, declaration)
            })
            .collect::<Vec<_>>();
        if applicable.len() >= 2 {
            diagnostics.push(Diagnostic::error(
                "TPY4012",
                format!(
                    "call to `{}.{}` in module `{}` is ambiguous across {} overloads after applicability filtering",
                    module_node.module_key,
                    call.method,
                    node.module_path.display(),
                    applicable.len()
                ),
            ));
            return Some(diagnostics);
        }
        if let Some(applicable) = applicable.first().copied() {
            let signature = direct_signature_sites_from_detail(&applicable.detail);
            if let Some(diagnostic) =
                direct_source_function_arity_diagnostic(node, nodes, &direct_call, &signature)
            {
                diagnostics.push(diagnostic);
            }
            diagnostics.extend(direct_source_function_keyword_diagnostics(
                node,
                nodes,
                &direct_call,
                &signature,
            ));
            diagnostics.extend(direct_source_function_type_diagnostics(
                node,
                nodes,
                &direct_call,
                &signature,
            ));
            return Some(diagnostics);
        }
    }

    let signature = direct_signature_sites_from_detail(&callable_candidates[0].detail);
    if let Some(diagnostic) =
        direct_source_function_arity_diagnostic(node, nodes, &direct_call, &signature)
    {
        diagnostics.push(diagnostic);
    }
    diagnostics.extend(direct_source_function_keyword_diagnostics(
        node,
        nodes,
        &direct_call,
        &signature,
    ));
    diagnostics.extend(direct_source_function_type_diagnostics(
        node,
        nodes,
        &direct_call,
        &signature,
    ));
    Some(diagnostics)
}

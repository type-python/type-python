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

pub(super) fn resolve_direct_callable_return_semantic_type<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Option<SemanticType> {
    if let Some(callable_annotation) =
        resolve_decorated_function_callable_annotation(node, nodes, callee)
    {
        return decorated_function_return_type_from_callable_annotation(&callable_annotation)
            .map(|return_type| lower_type_text_or_name(&return_type));
    }
    if let Some(function) = resolve_direct_function(node, nodes, callee) {
        let return_text = substitute_self_annotation(
            function.detail.split_once("->")?.1.trim(),
            function.owner.as_ref().map(|owner| owner.name.as_str()),
        );
        return Some(if function.is_async && !return_text.is_empty() {
            SemanticType::Generic {
                head: String::from("Awaitable"),
                args: vec![lower_type_text_or_name(&return_text)],
            }
        } else {
            lower_type_text_or_name(&return_text)
        });
    }

    if let Some((_, class_decl)) = resolve_direct_base(nodes, node, callee) {
        return Some(SemanticType::Name(class_decl.name.clone()));
    }

    if let Some(signature) = resolve_typing_callable_signature(callee) {
        return Some(lower_type_text_or_name(signature.split_once("->")?.1.trim()));
    }

    resolve_builtin_return_type(callee).map(lower_type_text_or_name)
}

pub(super) fn resolve_instantiated_callable_return_type_from_declaration(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
    call: &typepython_binding::CallSite,
) -> Option<String> {
    resolve_instantiated_callable_return_semantic_type_from_declaration(node, nodes, declaration, call)
        .map(|return_type| render_semantic_type(&return_type))
}

pub(super) fn resolve_instantiated_callable_return_semantic_type_from_declaration(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
    call: &typepython_binding::CallSite,
) -> Option<SemanticType> {
    if declaration.type_params.is_empty() {
        return Some(lower_type_text_or_name(declaration.detail.split_once("->")?.1.trim()));
    }
    let signature = direct_signature_sites_from_detail(&declaration.detail);
    let substitutions =
        infer_generic_type_param_substitutions(node, nodes, declaration, &signature, call)?;
    Some(instantiate_semantic_annotation(
        declaration.detail.split_once("->")?.1.trim(),
        &substitutions,
    ))
}

#[allow(dead_code)]
pub(super) fn resolve_instantiated_callable_return_type_id_from_declaration(
    store: &mut TypeStore,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
    call: &typepython_binding::CallSite,
) -> Option<TypeId> {
    resolve_instantiated_callable_return_semantic_type_from_declaration(node, nodes, declaration, call)
        .map(|return_type| store.intern(return_type))
}

#[allow(
    dead_code,
    reason = "string-returning callable return helpers remain as compatibility bridges during semantic migration"
)]
pub(super) fn resolve_direct_callable_return_type_for_line(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
    line: usize,
) -> Option<String> {
    resolve_direct_callable_return_semantic_type_for_line(node, nodes, callee, line)
        .map(|return_type| render_semantic_type(&return_type))
}

pub(super) fn resolve_direct_callable_return_semantic_type_for_line(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
    line: usize,
) -> Option<SemanticType> {
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
        return resolve_instantiated_callable_return_semantic_type_from_declaration(
            node, nodes, selected, call,
        );
    }
    if let Some(callable_annotation) =
        resolve_decorated_function_callable_annotation(node, nodes, callee)
    {
        return decorated_function_return_type_from_callable_annotation(&callable_annotation)
            .map(|return_type| lower_type_text_or_name(&return_type));
    }
    let function = resolve_direct_function(node, nodes, callee)?;
    resolve_instantiated_callable_return_semantic_type_from_declaration(node, nodes, function, call)
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

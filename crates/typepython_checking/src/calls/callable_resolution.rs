pub(super) fn resolve_direct_callable_param_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<Vec<String>> {
    if let Some(local) = resolve_direct_function(node, nodes, callee) {
        return Some(declaration_signature_param_types(local).unwrap_or_default());
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
            .and_then(declaration_signature_param_types)
            .unwrap_or_default();
        return Some(param_types.into_iter().skip(1).collect());
    }

    if let Some(signature) = resolve_typing_callable_signature(callee) {
        return direct_param_types(signature);
    }

    None
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedDirectCallCandidate<'a> {
    pub(super) declaration: &'a Declaration,
    #[allow(dead_code)]
    pub(super) substitutions: GenericTypeParamSubstitutions,
    pub(super) signature_sites: Vec<typepython_syntax::DirectFunctionParamSite>,
    pub(super) signature_params: Vec<DirectSignatureParam>,
    pub(super) return_type: Option<SemanticType>,
}

fn unresolved_generic_instantiation_params(
    declaration: &Declaration,
    substitutions: &GenericTypeParamSubstitutions,
) -> Vec<String> {
    declaration
        .type_params
        .iter()
        .filter_map(|type_param| match type_param.kind {
            typepython_binding::GenericTypeParamKind::ParamSpec => (!substitutions
                .param_lists
                .contains_key(&type_param.name))
                .then(|| type_param.name.clone()),
            typepython_binding::GenericTypeParamKind::TypeVarTuple => (!substitutions
                .type_packs
                .contains_key(&type_param.name))
                .then(|| type_param.name.clone()),
            typepython_binding::GenericTypeParamKind::TypeVar => None,
        })
        .collect()
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum DirectCallResolutionFailure {
    GenericSolve(GenericSolveFailure),
    SignatureInstantiationFailed { declaration_name: String, unresolved: Vec<String> },
}

impl DirectCallResolutionFailure {
    pub(super) fn diagnostic_reason(&self) -> String {
        match self {
            Self::GenericSolve(failure) => failure.diagnostic_reason(),
            Self::SignatureInstantiationFailed { declaration_name, unresolved } => {
                if unresolved.is_empty() {
                    format!(
                        "reason: instantiated signature for `{}` could not be materialized from the inferred generic arguments",
                        declaration_name
                    )
                } else {
                    format!(
                        "reason: instantiated signature for `{}` still depends on unresolved generic parameter list item(s): {}",
                        declaration_name,
                        unresolved.join(", ")
                    )
                }
            }
        }
    }
}

fn resolve_callable_candidate_from_semantics<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &'a Declaration,
    call: &typepython_binding::CallSite,
    signature: Vec<typepython_syntax::DirectFunctionParamSite>,
    return_annotation_text: Option<String>,
) -> Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure> {
    let substitutions = if declaration.type_params.is_empty() {
        GenericTypeParamSubstitutions::default()
    } else {
        infer_generic_type_param_substitutions_detailed(node, nodes, declaration, &signature, call)
            .map_err(DirectCallResolutionFailure::GenericSolve)?
    };
    let signature_sites = if declaration.type_params.is_empty() {
        signature
    } else {
        instantiate_direct_function_signature(&signature, &substitutions).ok_or_else(|| {
            DirectCallResolutionFailure::SignatureInstantiationFailed {
                declaration_name: declaration.name.clone(),
                unresolved: unresolved_generic_instantiation_params(declaration, &substitutions),
            }
        })?
    };
    let return_type = return_annotation_text.map(|annotation| {
        if declaration.type_params.is_empty() {
            lower_type_text_or_name(&annotation)
        } else {
            instantiate_semantic_annotation(&annotation, &substitutions)
        }
    });
    let signature_params = direct_signature_params_from_sites(&signature_sites);

    Ok(ResolvedDirectCallCandidate {
        declaration,
        substitutions,
        signature_sites,
        signature_params,
        return_type,
    })
}

pub(super) fn resolve_direct_call_candidate_detailed<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &'a Declaration,
    call: &typepython_binding::CallSite,
) -> Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure> {
    resolve_callable_candidate_from_semantics(
        node,
        nodes,
        declaration,
        call,
        declaration_signature_sites(declaration),
        declaration_signature_return_annotation_text(declaration),
    )
}

pub(super) fn resolve_direct_call_candidate<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &'a Declaration,
    call: &typepython_binding::CallSite,
) -> Option<ResolvedDirectCallCandidate<'a>> {
    resolve_direct_call_candidate_detailed(node, nodes, declaration, call).ok()
}

pub(super) fn resolve_direct_call_candidate_with_context_detailed<'a>(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &'a Declaration,
    call: &typepython_binding::CallSite,
) -> Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure> {
    let callable = context.load_declaration_semantics(declaration).callable.ok_or_else(|| {
        DirectCallResolutionFailure::SignatureInstantiationFailed {
            declaration_name: declaration.name.clone(),
            unresolved: Vec::new(),
        }
    })?;
    resolve_callable_candidate_from_semantics(
        node,
        nodes,
        declaration,
        call,
        callable_signature_sites_from_semantics(&callable),
        callable_return_annotation_text_from_semantics(&callable),
    )
}

pub(super) fn resolve_applicable_direct_overload_candidates<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    overloads: &[&'a Declaration],
) -> Vec<ResolvedDirectCallCandidate<'a>> {
    overloads
        .iter()
        .copied()
        .filter_map(|declaration| resolve_direct_call_candidate(node, nodes, declaration, call))
        .filter(|candidate| {
            call_signature_params_are_applicable(node, nodes, call, &candidate.signature_params)
        })
        .collect()
}

pub(super) fn resolve_method_call_candidate_detailed<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &'a Declaration,
    call: &typepython_binding::CallSite,
    owner_type_name: &str,
    callable: Option<&SemanticCallableDeclaration>,
) -> Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure> {
    let callable = match callable {
        Some(callable) => callable.clone(),
        None => declaration_callable_semantics(declaration).ok_or_else(|| {
            DirectCallResolutionFailure::SignatureInstantiationFailed {
                declaration_name: declaration.name.clone(),
                unresolved: Vec::new(),
            }
        })?,
    };
    resolve_callable_candidate_from_semantics(
        node,
        nodes,
        declaration,
        call,
        method_signature_sites_from_semantics(declaration, &callable, owner_type_name),
        callable_return_annotation_text_with_self_from_semantics(&callable, owner_type_name),
    )
}

pub(super) fn resolve_applicable_method_overload_candidates<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    owner_type_name: &str,
    overloads: &[(&'a Declaration, Option<SemanticCallableDeclaration>)],
) -> Vec<ResolvedDirectCallCandidate<'a>> {
    overloads
        .iter()
        .filter_map(|(declaration, callable)| {
            resolve_method_call_candidate_detailed(
                node,
                nodes,
                declaration,
                call,
                owner_type_name,
                callable.as_ref(),
            )
            .ok()
        })
        .filter(|candidate| {
            call_signature_params_are_applicable(node, nodes, call, &candidate.signature_params)
        })
        .collect()
}

#[allow(dead_code)]
pub(super) fn resolve_instantiated_direct_function_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    function: &Declaration,
    call: &typepython_binding::CallSite,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    if function.type_params.is_empty() {
        return None;
    }

    resolve_direct_call_candidate(node, nodes, function, call).map(|candidate| candidate.signature_sites)
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
            && let Some(import_target) = resolve_imported_symbol_semantic_target(node, nodes, head)
            && let Some(module_node) = import_target.module_target()
        {
            let resolved_module = format!("{}.{}", module_node.module_key, tail);
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

    resolve_imported_symbol_semantic_target(node, nodes, name)?.function_provider()
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

pub(super) fn rewrite_imported_typing_semantic_callable_params(
    node: &typepython_graph::ModuleNode,
    params: &SemanticCallableParams,
) -> SemanticCallableParams {
    match params {
        SemanticCallableParams::Ellipsis => SemanticCallableParams::Ellipsis,
        SemanticCallableParams::ParamList(types) => SemanticCallableParams::ParamList(
            types
                .iter()
                .map(|ty| rewrite_imported_typing_semantic_type(node, ty))
                .collect(),
        ),
        SemanticCallableParams::Concatenate(types) => SemanticCallableParams::Concatenate(
            types
                .iter()
                .map(|ty| rewrite_imported_typing_semantic_type(node, ty))
                .collect(),
        ),
        SemanticCallableParams::Single(expr) => SemanticCallableParams::Single(Box::new(
            rewrite_imported_typing_semantic_type(node, expr),
        )),
    }
}

pub(super) fn rewrite_imported_typing_semantic_type(
    node: &typepython_graph::ModuleNode,
    ty: &SemanticType,
) -> SemanticType {
    match ty {
        SemanticType::Name(name) => {
            SemanticType::Name(rewrite_imported_typing_token(node, name))
        }
        SemanticType::Generic { head, args } => SemanticType::Generic {
            head: rewrite_imported_typing_token(node, head),
            args: args
                .iter()
                .map(|arg| rewrite_imported_typing_semantic_type(node, arg))
                .collect(),
        },
        SemanticType::Callable { params, return_type } => SemanticType::Callable {
            params: rewrite_imported_typing_semantic_callable_params(node, params),
            return_type: Box::new(rewrite_imported_typing_semantic_type(node, return_type)),
        },
        SemanticType::Union(branches) => SemanticType::Union(
            branches
                .iter()
                .map(|branch| rewrite_imported_typing_semantic_type(node, branch))
                .collect(),
        ),
        SemanticType::Annotated { value, metadata } => SemanticType::Annotated {
            value: Box::new(rewrite_imported_typing_semantic_type(node, value)),
            metadata: metadata.clone(),
        },
        SemanticType::Unpack(inner) => SemanticType::Unpack(Box::new(
            rewrite_imported_typing_semantic_type(node, inner),
        )),
    }
}

pub(super) fn semantic_callable_type_from_signature_sites_in_module(
    node: &typepython_graph::ModuleNode,
    signature: &[typepython_syntax::DirectFunctionParamSite],
    return_type: &SemanticType,
) -> SemanticType {
    SemanticType::Callable {
        params: SemanticCallableParams::ParamList(
            signature
                .iter()
                .map(|param| {
                    rewrite_imported_typing_semantic_type(
                        node,
                        &param
                            .annotation
                            .as_deref()
                            .map(lower_type_text_or_name)
                            .unwrap_or_else(|| SemanticType::Name(String::from("dynamic"))),
                    )
                })
                .collect(),
        ),
        return_type: Box::new(rewrite_imported_typing_semantic_type(node, return_type)),
    }
}

pub(super) fn direct_function_signature_sites_from_semantic_callable(
    callable: &SemanticType,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    let (params, _) = callable.callable_parts()?;
    let SemanticCallableParams::ParamList(param_types) = params else {
        return None;
    };
    Some(synthesize_param_list_binding(
        param_types.iter().map(diagnostic_type_text).collect(),
    ))
}

pub(super) fn decorated_function_return_semantic_type_from_semantic_callable(
    callable: &SemanticType,
) -> Option<SemanticType> {
    Some(callable.callable_parts()?.1.clone())
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

pub(super) fn synthetic_decorator_application_call_from_semantic(
    decorator_name: &str,
    callable: &SemanticType,
) -> typepython_binding::CallSite {
    let callable_annotation = diagnostic_type_text(callable);
    synthetic_decorator_application_call(decorator_name, &callable_annotation)
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

#[cfg(test)]
pub(super) fn apply_named_callable_decorator_transform_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    decorator_name: &str,
    current_callable: &SemanticType,
) -> Option<SemanticType> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    apply_named_callable_decorator_transform_semantic_with_context(
        &context,
        node,
        nodes,
        decorator_name,
        current_callable,
    )
}

pub(super) fn apply_named_callable_decorator_transform_semantic_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    decorator_name: &str,
    current_callable: &SemanticType,
) -> Option<SemanticType> {
    let (decorator_node, decorator) = resolve_function_provider_with_node(nodes, node, decorator_name)?;
    let call = synthetic_decorator_application_call_from_semantic(decorator_name, current_callable);
    let resolved = resolve_direct_call_candidate(decorator_node, nodes, decorator, &call)?;
    if !decorator_transform_accepts_callable_annotation_with_context(
        context,
        decorator_node,
        nodes,
        &call,
        &resolved.signature_sites,
    ) {
        return None;
    }
    Some(rewrite_imported_typing_semantic_type(
        decorator_node,
        resolved.return_type.as_ref()?,
    ))
}

#[allow(dead_code)]
pub(super) fn apply_named_callable_decorator_transform_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    decorator_name: &str,
    current_callable: &str,
) -> Option<String> {
    apply_named_callable_decorator_transform_semantic_with_context(
        context,
        node,
        nodes,
        decorator_name,
        &lower_type_text_or_name(current_callable),
    )
    .map(|callable| diagnostic_type_text(&callable))
}

pub(super) fn resolve_decorated_callable_annotation_for_declaration_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
) -> Option<String> {
    resolve_decorated_callable_semantic_type_for_declaration_with_context(
        context,
        node,
        nodes,
        declaration,
    )
    .map(|callable| diagnostic_type_text(&callable))
}

pub(super) fn resolve_decorated_callable_semantic_type_for_declaration_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
) -> Option<SemanticType> {
    let decorated = resolve_decorated_callable_site_with_context(context, node, declaration)?;
    if decorated.decorators.is_empty() {
        return None;
    }

    let callable = context.load_declaration_semantics(declaration).callable?;
    let base_signature = callable.params;
    let base_return = if declaration.is_async {
        SemanticType::Generic {
            head: String::from("Awaitable"),
            args: vec![callable.return_type?],
        }
    } else {
        callable.return_type?
    };
    let mut current = semantic_callable_type_from_signature_sites_in_module(node, &base_signature, &base_return);
    for decorator in decorated.decorators.iter().rev() {
        current = apply_named_callable_decorator_transform_semantic_with_context(
            context, node, nodes, decorator, &current,
        )?;
    }
    Some(current)
}

#[allow(dead_code)]
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
    resolve_decorated_function_callable_semantic_type_with_context(context, node, nodes, callee)
        .map(|callable| diagnostic_type_text(&callable))
}

pub(super) fn resolve_decorated_function_callable_semantic_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<SemanticType> {
    let (function_node, function) = resolve_direct_function_with_node(node, nodes, callee)?;
    resolve_decorated_callable_semantic_type_for_declaration_with_context(
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
    if let Some(callable_type) =
        resolve_decorated_function_callable_semantic_type_with_context(
            &CheckerContext::new(nodes, ImportFallback::Unknown, None),
            node,
            nodes,
            callee,
        )
    {
        return decorated_function_return_semantic_type_from_semantic_callable(&callable_type);
    }
    if let Some(function) = resolve_direct_function(node, nodes, callee) {
        let return_text = substitute_self_annotation(
            &declaration_signature_return_annotation_text(function)?,
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

#[allow(dead_code)]
pub(super) fn resolve_instantiated_callable_return_type_from_declaration(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
    call: &typepython_binding::CallSite,
) -> Option<String> {
    resolve_instantiated_callable_return_semantic_type_from_declaration(node, nodes, declaration, call)
        .map(|return_type| render_semantic_type(&return_type))
}

#[allow(dead_code)]
pub(super) fn resolve_instantiated_callable_return_semantic_type_from_declaration(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
    call: &typepython_binding::CallSite,
) -> Option<SemanticType> {
    resolve_direct_call_candidate(node, nodes, declaration, call)?.return_type
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
        let applicable = resolve_applicable_direct_overload_candidates(node, nodes, call, &overloads);
        let selected = select_most_specific_overload(node, nodes, &applicable)?;
        return selected.return_type.clone();
    }
    if let Some(callable_type) =
        resolve_decorated_function_callable_semantic_type_with_context(
            &CheckerContext::new(nodes, ImportFallback::Unknown, None),
            node,
            nodes,
            callee,
        )
    {
        return decorated_function_return_semantic_type_from_semantic_callable(&callable_type);
    }
    let function = resolve_direct_function(node, nodes, callee)?;
    resolve_direct_call_candidate(node, nodes, function, call)?.return_type
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
            declaration_signature_param_count(local).unwrap_or_default(),
            declaration_signature_param_names(local).unwrap_or_default(),
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
            .and_then(declaration_signature_param_names)
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
        declaration_signature_param_count(function).unwrap_or_default(),
        declaration_signature_param_names(function).unwrap_or_default(),
    ))
}

pub(super) fn resolve_direct_callable_param_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<Vec<String>> {
    if let Some(local) = resolve_direct_function(node, nodes, callee) {
        return Some(
            declaration_semantic_signature_params(local)
                .unwrap_or_default()
                .into_iter()
                .map(|param| param.annotation_text.unwrap_or_default())
                .collect(),
        );
    }

    if let Some(shape) = resolve_synthesized_dataclass_class_shape(node, nodes, callee)
        && !shape.has_explicit_init
    {
        return Some(
            shape
                .fields
                .iter()
                .filter(|field| !field.kw_only)
                .map(|field| field.rendered_annotation())
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
    pub(super) signature_params: Vec<SemanticCallableParam>,
    pub(super) semantic_param_types: Vec<SemanticType>,
    pub(super) return_type: Option<SemanticType>,
}

fn instantiate_semantic_callable_param(
    param: &SemanticCallableParam,
    substitutions: &GenericTypeParamSubstitutions,
) -> SemanticCallableParam {
    let annotation = param
        .annotation
        .as_ref()
        .map(|annotation| substitute_semantic_type_params(annotation, substitutions));
    SemanticCallableParam {
        name: param.name.clone(),
        annotation_text: annotation.as_ref().map(diagnostic_type_text),
        annotation,
        has_default: param.has_default,
        positional_only: param.positional_only,
        keyword_only: param.keyword_only,
        variadic: param.variadic,
        keyword_variadic: param.keyword_variadic,
    }
}

fn instantiate_semantic_callable_params(
    params: &[SemanticCallableParam],
    substitutions: &GenericTypeParamSubstitutions,
) -> Vec<SemanticCallableParam> {
    params
        .iter()
        .map(|param| instantiate_semantic_callable_param(param, substitutions))
        .collect()
}

fn rewrite_semantic_callable_params(
    node: &typepython_graph::ModuleNode,
    params: Vec<SemanticCallableParam>,
) -> Vec<SemanticCallableParam> {
    params
        .into_iter()
        .map(|param| SemanticCallableParam {
            annotation: param
                .annotation
                .as_ref()
                .map(|annotation| rewrite_imported_typing_semantic_type(node, annotation)),
            annotation_text: param.annotation.as_ref().map(|annotation| {
                diagnostic_type_text(&rewrite_imported_typing_semantic_type(node, annotation))
            }),
            ..param
        })
        .collect()
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
    UnresolvedCallableParamList { callable: SemanticType },
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
            Self::UnresolvedCallableParamList { callable } => format!(
                "reason: callable type `{}` still contains an unresolved ParamSpec or Concatenate tail",
                diagnostic_type_text(callable)
            ),
        }
    }
}

fn declaration_provider_node<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    declaration: &Declaration,
) -> &'a typepython_graph::ModuleNode {
    if node
        .declarations
        .iter()
        .any(|candidate| std::ptr::eq(candidate, declaration))
    {
        return node;
    }
    nodes
        .iter()
        .find(|candidate| {
            candidate
                .declarations
                .iter()
                .any(|candidate| std::ptr::eq(candidate, declaration))
        })
        .unwrap_or(node)
}

#[expect(
    clippy::too_many_arguments,
    reason = "call candidate resolution threads semantic call context and assignability policy"
)]
fn resolve_callable_candidate_from_semantics<'a>(
    context: Option<&CheckerContext<'_>>,
    node: &typepython_graph::ModuleNode,
    call_node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &'a Declaration,
    call: &typepython_binding::CallSite,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    signature: Vec<typepython_syntax::DirectFunctionParamSite>,
    semantic_params: Vec<SemanticCallableParam>,
    return_type: Option<SemanticType>,
    options: AssignabilityOptions,
) -> Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure> {
    let substitutions = if declaration.type_params.is_empty() {
        GenericTypeParamSubstitutions::default()
    } else if let Some(context) = context {
        infer_generic_type_param_substitutions_from_semantic_params_detailed_in_scope_with_context(
            context,
            node,
            call_node,
            nodes,
            declaration,
            &semantic_params,
            call,
            current_owner_name,
            current_owner_type_name,
        )
        .map_err(DirectCallResolutionFailure::GenericSolve)?
    } else {
        infer_generic_type_param_substitutions_from_semantic_params_detailed_in_scope_with_options(
            node,
            call_node,
            nodes,
            declaration,
            &semantic_params,
            call,
            current_owner_name,
            current_owner_type_name,
            options,
        )
        .map_err(DirectCallResolutionFailure::GenericSolve)?
    };
    let (signature_sites, signature_params) = if declaration.type_params.is_empty() {
        (signature, semantic_params)
    } else {
        (
            instantiate_direct_function_signature(&signature, &substitutions).ok_or_else(|| {
                DirectCallResolutionFailure::SignatureInstantiationFailed {
                    declaration_name: declaration.name.clone(),
                    unresolved: unresolved_generic_instantiation_params(declaration, &substitutions),
                }
            })?,
            instantiate_semantic_callable_params(&semantic_params, &substitutions),
        )
    };
    let return_type = return_type.map(|return_type| {
        let return_type = if declaration.type_params.is_empty() {
            return_type
        } else {
            substitute_semantic_type_params(&return_type, &substitutions)
        };
        rewrite_imported_typing_semantic_type(node, &return_type)
    });
    let signature_params = rewrite_semantic_callable_params(node, signature_params);
    let semantic_param_types = signature_params
        .iter()
        .map(SemanticCallableParam::annotation_or_dynamic)
        .collect();

    Ok(ResolvedDirectCallCandidate {
        declaration,
        substitutions,
        signature_sites,
        signature_params,
        semantic_param_types,
        return_type,
    })
}

pub(super) fn resolve_direct_call_candidate_detailed<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &'a Declaration,
    call: &typepython_binding::CallSite,
) -> Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure> {
    resolve_direct_call_candidate_detailed_with_options(
        node,
        nodes,
        declaration,
        call,
        AssignabilityOptions::default(),
    )
}

pub(super) fn resolve_direct_call_candidate_detailed_with_options<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &'a Declaration,
    call: &typepython_binding::CallSite,
    options: AssignabilityOptions,
) -> Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure> {
    resolve_direct_call_candidate_detailed_in_scope_with_options(
        node, nodes, declaration, call, None, None, options,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_direct_call_candidate_detailed_in_scope_with_options<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &'a Declaration,
    call: &typepython_binding::CallSite,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    options: AssignabilityOptions,
) -> Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure> {
    let provider_node = declaration_provider_node(node, nodes, declaration);
    resolve_callable_candidate_from_semantics(
        None,
        provider_node,
        node,
        nodes,
        declaration,
        call,
        current_owner_name,
        current_owner_type_name,
        declaration_signature_sites(declaration),
        declaration_semantic_signature_params(declaration).unwrap_or_default(),
        declaration_signature_return_semantic_type(declaration),
        options,
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
    let provider_node = declaration_provider_node(node, nodes, declaration);
    let call_scope = context
        .load_direct_call_context_sites(node)
        .into_iter()
        .find(|site| site.line == call.line && site.callee == call.callee);
    resolve_callable_candidate_from_semantics(
        Some(context),
        provider_node,
        node,
        nodes,
        declaration,
        call,
        call_scope.as_ref().and_then(|site| site.owner_name.as_deref()),
        call_scope.as_ref().and_then(|site| site.owner_type_name.as_deref()),
        callable_signature_sites_from_semantics(&callable),
        callable_semantic_params_from_semantics(&callable),
        callable.return_type,
        context.assignability_options(),
    )
}

#[allow(dead_code)]
pub(super) fn resolve_direct_overload_selection<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    overloads: &[&'a Declaration],
    options: AssignabilityOptions,
) -> ResolvedOverloadSelection<'a> {
    resolve_direct_overload_selection_in_scope(
        node, nodes, call, overloads, None, None, options,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_direct_overload_selection_in_scope<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    overloads: &[&'a Declaration],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    options: AssignabilityOptions,
) -> ResolvedOverloadSelection<'a> {
    let attempts = overloads
        .iter()
        .map(|declaration| {
            (
                *declaration,
                resolve_direct_call_candidate_detailed_in_scope_with_options(
                    node,
                    nodes,
                    declaration,
                    call,
                    current_owner_name,
                    current_owner_type_name,
                    options,
                ),
            )
        })
        .collect::<Vec<_>>();
    resolve_overload_selection_from_attempts_in_scope_with_options(
        node,
        nodes,
        call,
        attempts,
        current_owner_name,
        current_owner_type_name,
        options,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_method_call_candidate_detailed<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &'a Declaration,
    call: &typepython_binding::CallSite,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    owner_type: &SemanticType,
    lookup_owner_type: Option<&SemanticType>,
    callable: Option<&SemanticCallableDeclaration>,
    options: AssignabilityOptions,
) -> Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure> {
    let lookup_owner_type = lookup_owner_type.unwrap_or(owner_type);
    let owner_type_name = semantic_nominal_owner_name(lookup_owner_type).ok_or_else(|| {
        DirectCallResolutionFailure::SignatureInstantiationFailed {
            declaration_name: declaration.name.clone(),
            unresolved: Vec::new(),
        }
    })?;
    let self_type = owner_type;
    let (_, owner_class_decl) = resolve_direct_base(nodes, node, &owner_type_name).ok_or_else(|| {
        DirectCallResolutionFailure::SignatureInstantiationFailed {
            declaration_name: declaration.name.clone(),
            unresolved: Vec::new(),
        }
    })?;
    let owner_substitutions = without_shadowed_generic_params(
        owner_generic_substitutions(lookup_owner_type, owner_class_decl),
        declaration,
    );
    let callable = match callable {
        Some(callable) => callable.clone(),
        None => declaration_callable_semantics(declaration).ok_or_else(|| {
            DirectCallResolutionFailure::SignatureInstantiationFailed {
                declaration_name: declaration.name.clone(),
                unresolved: Vec::new(),
            }
        })?,
    };
    let provider_node = declaration_provider_node(node, nodes, declaration);
    let method_params = method_semantic_params_without_self_from_semantics(declaration, &callable);
    let method_params = substitute_semantic_callable_params(&method_params, &owner_substitutions);
    let method_params =
        substitute_self_semantic_params_with_type(&method_params, Some(self_type));
    let return_type = callable
        .return_type
        .as_ref()
        .map(|return_type| substitute_semantic_type_params(return_type, &owner_substitutions))
        .map(|return_type| substitute_self_semantic_type_with_type(&return_type, Some(self_type)));
    resolve_callable_candidate_from_semantics(
        None,
        provider_node,
        node,
        nodes,
        declaration,
        call,
        current_owner_name,
        current_owner_type_name,
        signature_sites_from_semantic_params(&method_params),
        method_params,
        return_type,
        options,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_method_overload_selection<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    owner_type: &SemanticType,
    lookup_owner_type: Option<&SemanticType>,
    overloads: &[(&'a Declaration, Option<SemanticCallableDeclaration>)],
    options: AssignabilityOptions,
) -> ResolvedOverloadSelection<'a> {
    let attempts = overloads
        .iter()
        .map(|(declaration, callable)| {
            (
                *declaration,
                resolve_method_call_candidate_detailed(
                    node,
                    nodes,
                    declaration,
                    call,
                    current_owner_name,
                    current_owner_type_name,
                    owner_type,
                    lookup_owner_type,
                    callable.as_ref(),
                    options,
                ),
            )
        })
        .collect::<Vec<_>>();
    resolve_overload_selection_from_attempts_in_scope_with_options(
        node,
        nodes,
        call,
        attempts,
        current_owner_name,
        current_owner_type_name,
        options,
    )
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

pub(super) fn undecidable_decorator_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    strict: bool,
) -> Vec<Diagnostic> {
    if !strict || node.module_kind != SourceKind::TypePython {
        return Vec::new();
    }

    node.declarations
        .iter()
        .filter(|declaration| {
            matches!(
                declaration.kind,
                DeclarationKind::Function | DeclarationKind::Overload
            )
        })
        .filter_map(|declaration| {
            let decorated = resolve_decorated_callable_site_with_context(context, node, declaration)?;
            if decorated.decorators.is_empty() {
                return None;
            }
            if decorated
                .decorators
                .iter()
                .all(|decorator| decorator_is_semantic_metadata_only(decorator))
            {
                return None;
            }
            if context.sync_async_dual_emit_enabled()
                && decorated.decorators.iter().any(|decorator| {
                decorator.rsplit('.').next().unwrap_or(decorator) == "dual_emit"
            })
            {
                return None;
            }
            if let Some(result) = framework_owned_decorator_diagnostic(
                context,
                node,
                declaration,
                &decorated,
            ) {
                return result;
            }
            match resolve_decorated_callable_semantic_type_for_declaration_with_context(
                context,
                node,
                nodes,
                declaration,
            ) {
                Some(_) => None,
                None => Some(Diagnostic::error(
                    "TPY4001",
                    format!(
                        "decorated declaration `{}` in module `{}` uses decorator{} `{}` that cannot be reduced to a statically known callable-to-callable transform",
                        declaration.name,
                        node.module_path.display(),
                        if decorated.decorators.len() == 1 { "" } else { "s" },
                        decorated.decorators.join("`, `"),
                    ),
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    decorated.line,
                    1,
                    decorated.line,
                    1,
                ))),
            }
        })
        .collect()
}

fn decorator_is_semantic_metadata_only(decorator: &str) -> bool {
    let target = decorator.split_once(':').map_or(decorator, |(target, _)| target);
    let short = target.rsplit('.').next().unwrap_or(target);
    matches!(
        short,
        "effect"
            | "effectful"
            | "effect_pure"
            | "pure"
            | "effect_unsafe"
            | "effect_io_fs"
            | "effect_io_net"
            | "effect_io_proc"
            | "effect_time"
            | "effect_random"
            | "effect_runtime_validation"
            | "effect_taint_sanitize"
            | "source"
            | "sink"
            | "sanitizer"
            | "trusted_validator"
            | "validator"
            | "must_use"
            | "must_call"
            | "must_close"
            | "must_dispose"
            | "must_await"
            | "must_consume"
    )
}

fn framework_owned_decorator_diagnostic(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    decorated: &typepython_syntax::DecoratedCallableSite,
) -> Option<Option<Diagnostic>> {
    let owner = declaration.owner.as_ref()?;
    let kind = decorated
        .decorators
        .iter()
        .find_map(|decorator| framework_owned_decorator_kind(decorator))?;
    if !framework_transform_class_supports_generated_members_with_context(context, node, &owner.name)
    {
        return None;
    }
    match kind.signature_diagnostic(node, declaration, decorated) {
        Some(diagnostic) => Some(Some(diagnostic)),
        None => Some(None),
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum FrameworkOwnedDecoratorKind {
    ComputedField,
    FieldValidator,
    ModelValidator,
    FieldSerializer,
    ModelSerializer,
}

impl FrameworkOwnedDecoratorKind {
    fn signature_diagnostic(
        self,
        node: &typepython_graph::ModuleNode,
        declaration: &Declaration,
        decorated: &typepython_syntax::DecoratedCallableSite,
    ) -> Option<Diagnostic> {
        let signature = declaration.callable_signature()?;
        let param_count = signature
            .params
            .iter()
            .filter(|param| !param.variadic && !param.keyword_variadic)
            .count();
        let missing_return = signature.returns.is_none();
        let expected = match self {
            Self::ComputedField => 1,
            Self::FieldValidator | Self::FieldSerializer => 2,
            Self::ModelValidator | Self::ModelSerializer => 1,
        };
        if param_count >= expected && !missing_return {
            return None;
        }
        let mut reasons = Vec::new();
        if param_count < expected {
            reasons.push(format!(
                "expected at least {} explicit parameter{}",
                expected,
                if expected == 1 { "" } else { "s" }
            ));
        }
        if missing_return {
            reasons.push(String::from("expected an explicit return annotation"));
        }
        Some(
            Diagnostic::error(
                "TPY4020",
                format!(
                    "framework-owned decorator `{}` on `{}` cannot be checked statically: {}",
                    self.label(),
                    declaration.name,
                    reasons.join(" and "),
                ),
            )
            .with_span(Span::new(
                node.module_path.display().to_string(),
                decorated.line,
                1,
                decorated.line,
                1,
            )),
        )
    }

    const fn label(self) -> &'static str {
        match self {
            Self::ComputedField => "computed_field",
            Self::FieldValidator => "field_validator",
            Self::ModelValidator => "model_validator",
            Self::FieldSerializer => "field_serializer",
            Self::ModelSerializer => "model_serializer",
        }
    }
}

fn framework_owned_decorator_kind(name: &str) -> Option<FrameworkOwnedDecoratorKind> {
    [
        ("computed_field", FrameworkOwnedDecoratorKind::ComputedField),
        ("field_validator", FrameworkOwnedDecoratorKind::FieldValidator),
        ("model_validator", FrameworkOwnedDecoratorKind::ModelValidator),
        ("field_serializer", FrameworkOwnedDecoratorKind::FieldSerializer),
        ("model_serializer", FrameworkOwnedDecoratorKind::ModelSerializer),
    ]
    .into_iter()
    .find_map(|(suffix, kind)| {
        (name == suffix || name.ends_with(&format!(".{suffix}"))).then_some(kind)
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
                        &semantic_type_from_direct_param_site(param)
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
    Some(
        param_types
            .iter()
            .enumerate()
            .map(|(index, ty)| typepython_syntax::DirectFunctionParamSite {
                name: format!("arg{index}"),
                annotation: Some(diagnostic_type_text(ty)),
                annotation_expr: Some(semantic_type_to_type_expr(ty)),
                has_default: false,
                positional_only: false,
                keyword_only: false,
                variadic: false,
                keyword_variadic: false,
            })
            .collect(),
    )
}

pub(super) fn decorated_function_return_semantic_type_from_semantic_callable(
    callable: &SemanticType,
) -> Option<SemanticType> {
    Some(callable.callable_parts()?.1.clone())
}

#[cfg(test)]
pub(super) fn synthetic_direct_expr_metadata(
    value_type: &str,
) -> typepython_syntax::DirectExprMetadata {
    typepython_syntax::DirectExprMetadata {
        value_type_expr: typepython_syntax::TypeExpr::parse(value_type),
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

#[cfg(test)]
pub(super) fn synthetic_decorator_application_call(
    decorator_name: &str,
    callable_annotation: &str,
) -> typepython_binding::CallSite {
    typepython_binding::CallSite {
        callee: decorator_name.to_owned(),
        arg_count: 1,
        arg_values: vec![synthetic_direct_expr_metadata(callable_annotation)],
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    }
}

fn synthetic_single_positional_call(callee: &str) -> typepython_binding::CallSite {
    typepython_binding::CallSite {
        callee: callee.to_owned(),
        arg_count: 1,
        arg_values: vec![typepython_syntax::DirectExprMetadata::from_type_text("")],
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    }
}

fn infer_direct_single_semantic_argument_substitutions_detailed(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
    signature: &[SemanticCallableParam],
    actual: &SemanticType,
    options: AssignabilityOptions,
) -> Result<GenericTypeParamSubstitutions, DirectCallResolutionFailure> {
    if declaration.type_params.is_empty() {
        return Ok(GenericTypeParamSubstitutions::default());
    }

    let type_names = declaration
        .type_params
        .iter()
        .filter(|type_param| type_param.kind == typepython_binding::GenericTypeParamKind::TypeVar)
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();
    let param_spec_names = declaration
        .type_params
        .iter()
        .filter(|type_param| type_param.kind == typepython_binding::GenericTypeParamKind::ParamSpec)
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();
    let type_pack_names = declaration
        .type_params
        .iter()
        .filter(|type_param| type_param.kind == typepython_binding::GenericTypeParamKind::TypeVarTuple)
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();

    let param = signature.iter().find(|param| !param.keyword_variadic).ok_or_else(|| {
        DirectCallResolutionFailure::SignatureInstantiationFailed {
            declaration_name: declaration.name.clone(),
            unresolved: Vec::new(),
        }
    })?;
    let annotation = param.annotation_or_dynamic();
    let mut substitutions = GenericTypeParamSubstitutions::default();
    let annotation_mentions_param_spec = annotation.callable_parts().is_some_and(|(params, _)| {
        callable_param_expr_mentions_param_spec_semantic(params, &param_spec_names)
    });
    if annotation_mentions_param_spec {
        let (expected_params_expr, expected_return) = annotation.callable_parts().ok_or_else(|| {
            DirectCallResolutionFailure::GenericSolve(GenericSolveFailure::ParamSpecInferenceFailed {
                annotation: annotation.clone(),
                actual: actual.clone(),
            })
        })?;
        let (actual_params, actual_return) = actual.callable_parts().ok_or_else(|| {
            DirectCallResolutionFailure::GenericSolve(GenericSolveFailure::ParamSpecInferenceFailed {
                annotation: annotation.clone(),
                actual: actual.clone(),
            })
        })?;
        let SemanticCallableParams::ParamList(actual_param_types) = actual_params else {
            return Err(DirectCallResolutionFailure::GenericSolve(
                GenericSolveFailure::ParamSpecInferenceFailed {
                    annotation: annotation.clone(),
                    actual: actual.clone(),
                },
            ));
        };
        let actual_binding =
            ParamListBinding { params: synthesize_semantic_param_list_binding(actual_param_types.clone()) };
        let param_spec_bindings = match expected_params_expr {
            SemanticCallableParams::Single(expr)
                if matches!(expr.as_ref(), SemanticType::Name(name) if param_spec_names.contains(name.trim())) =>
            {
                let mut bindings = GenericTypeParamSubstitutions::default();
                let SemanticType::Name(name) = expr.as_ref() else {
                    unreachable!("matched semantic callable paramspec name")
                };
                insert_param_spec_binding(&mut bindings, name.trim(), actual_binding).ok_or_else(|| {
                    DirectCallResolutionFailure::GenericSolve(
                        GenericSolveFailure::ParamSpecBindingConflict {
                            param_name: name.trim().to_owned(),
                        },
                    )
                })?;
                bindings
            }
            _ => infer_callable_param_expr_bindings(
                node,
                nodes,
                expected_params_expr,
                &actual_binding,
                &type_names,
                &param_spec_names,
                &substitutions,
                options,
            )
            .ok_or_else(|| {
                DirectCallResolutionFailure::GenericSolve(
                    GenericSolveFailure::ParamSpecInferenceFailed {
                        annotation: annotation.clone(),
                        actual: actual.clone(),
                    },
                )
            })?,
        };
        merge_nested_generic_bindings(
            node,
            nodes,
            &mut substitutions,
            options,
            param_spec_bindings,
        )
        .ok_or_else(|| {
            DirectCallResolutionFailure::GenericSolve(
                GenericSolveFailure::ParamSpecBindingConflict {
                    param_name: declaration.name.clone(),
                },
            )
        })?;
        let return_bindings = infer_generic_type_param_bindings(
            node,
            nodes,
            expected_return,
            actual_return,
            &type_names,
            &substitutions,
            &type_pack_names,
            options,
        )
        .ok_or_else(|| {
            DirectCallResolutionFailure::GenericSolve(GenericSolveFailure::TypeBindingInferenceFailed {
                annotation: expected_return.clone(),
                actual: actual_return.clone(),
            })
        })?;
        merge_nested_generic_bindings(
            node,
            nodes,
            &mut substitutions,
            options,
            return_bindings,
        )
        .ok_or_else(|| {
            DirectCallResolutionFailure::GenericSolve(
                GenericSolveFailure::ParamSpecBindingConflict {
                    param_name: declaration.name.clone(),
                },
            )
        })?;
    }
    if !annotation_mentions_param_spec {
        let generic_bindings = infer_generic_type_param_bindings(
            node,
            nodes,
            &annotation,
            actual,
            &type_names,
            &substitutions,
            &type_pack_names,
            options,
        )
        .ok_or_else(|| {
            DirectCallResolutionFailure::GenericSolve(GenericSolveFailure::TypeBindingInferenceFailed {
                annotation: annotation.clone(),
                actual: actual.clone(),
            })
        })?;
        merge_nested_generic_bindings(
            node,
            nodes,
            &mut substitutions,
            options,
            generic_bindings,
        )
        .ok_or_else(|| {
            DirectCallResolutionFailure::GenericSolve(GenericSolveFailure::TypeVarTupleBindingConflict {
                param_name: declaration.name.clone(),
            })
        })?;
    }
    finalize_generic_type_param_substitutions_detailed_with_options(
        node,
        nodes,
        declaration,
        substitutions,
        options,
    )
    .map_err(DirectCallResolutionFailure::GenericSolve)
}

fn generic_param_name_sets(
    declaration: &Declaration,
) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
    let type_names = declaration
        .type_params
        .iter()
        .filter(|type_param| type_param.kind == typepython_binding::GenericTypeParamKind::TypeVar)
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();
    let param_spec_names = declaration
        .type_params
        .iter()
        .filter(|type_param| type_param.kind == typepython_binding::GenericTypeParamKind::ParamSpec)
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();
    let type_pack_names = declaration
        .type_params
        .iter()
        .filter(|type_param| type_param.kind == typepython_binding::GenericTypeParamKind::TypeVarTuple)
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();
    (type_names, param_spec_names, type_pack_names)
}

fn augment_semantic_callable_paramlist_substitutions(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
    signature: &[SemanticCallableParam],
    actual: &SemanticType,
    mut substitutions: GenericTypeParamSubstitutions,
    options: AssignabilityOptions,
) -> GenericTypeParamSubstitutions {
    let (_type_names, param_spec_names, _type_pack_names) = generic_param_name_sets(declaration);
    if param_spec_names.is_empty() || !substitutions.param_lists.is_empty() {
        return substitutions;
    }

    let Some(param) = signature.iter().find(|param| !param.keyword_variadic) else {
        return substitutions;
    };
    let Some(annotation) = param.annotation.clone() else {
        return substitutions;
    };
    let Some((expected_params_expr, _)) = annotation.callable_parts() else {
        return substitutions;
    };
    let Some((actual_params, _)) = actual.callable_parts() else {
        return substitutions;
    };
    let SemanticCallableParams::ParamList(actual_param_types) = actual_params else {
        return substitutions;
    };
    let actual_binding =
        ParamListBinding { params: synthesize_semantic_param_list_binding(actual_param_types.clone()) };
    let Some(bindings) = infer_callable_param_expr_bindings(
        node,
        nodes,
        expected_params_expr,
        &actual_binding,
        &BTreeSet::new(),
        &param_spec_names,
        &substitutions,
        options,
    ) else {
        return substitutions;
    };
    let _ = merge_nested_generic_bindings(
        node,
        nodes,
        &mut substitutions,
        options,
        bindings,
    );
    substitutions
}

fn resolve_direct_single_semantic_argument_candidate_detailed<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &'a Declaration,
    actual: &SemanticType,
    options: AssignabilityOptions,
) -> Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure> {
    let callable = declaration_callable_semantics(declaration).ok_or_else(|| {
        DirectCallResolutionFailure::SignatureInstantiationFailed {
            declaration_name: declaration.name.clone(),
            unresolved: Vec::new(),
        }
    })?;
    let signature = callable_signature_sites_from_semantics(&callable);
    let semantic_signature = callable_semantic_params_from_semantics(&callable);
    let substitutions = infer_direct_single_semantic_argument_substitutions_detailed(
        node,
        nodes,
        declaration,
        &semantic_signature,
        actual,
        options,
    )?;
    let substitutions = augment_semantic_callable_paramlist_substitutions(
        node,
        nodes,
        declaration,
        &semantic_signature,
        actual,
        substitutions,
        options,
    );
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
    let return_type = callable.return_type.clone().map(|return_type| {
        if declaration.type_params.is_empty() {
            return_type
        } else {
            substitute_semantic_type_params(&return_type, &substitutions)
        }
    });
    let signature_params = if declaration.type_params.is_empty() {
        callable_semantic_params_from_semantics(&callable)
    } else {
        instantiate_semantic_callable_params(&callable_semantic_params_from_semantics(&callable), &substitutions)
    };
    let semantic_param_types = signature_params
        .iter()
        .map(SemanticCallableParam::annotation_or_dynamic)
        .collect();
    Ok(ResolvedDirectCallCandidate {
        declaration,
        substitutions,
        signature_sites,
        signature_params,
        semantic_param_types,
        return_type,
    })
}

fn decorator_candidate_accepts_semantic_callable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    candidate: &ResolvedDirectCallCandidate<'_>,
    current_callable: &SemanticType,
    options: AssignabilityOptions,
) -> bool {
    let synthetic_call = synthetic_single_positional_call(&candidate.declaration.name);
    if !call_signature_params_are_applicable_with_options(
        node,
        nodes,
        &synthetic_call,
        &candidate.signature_params,
        options,
    )
    {
        return false;
    }
    if !candidate.declaration.type_params.is_empty() {
        return true;
    }
    let Some(param) = candidate.signature_params.iter().find(|param| !param.keyword_variadic) else {
        return false;
    };
    let Some(annotation) = param.annotation.as_ref() else {
        return true;
    };
    semantic_type_is_assignable_with_options(node, nodes, annotation, current_callable, options)
}

fn substitute_single_paramspec_from_semantic_callable(
    declaration: &Declaration,
    current_callable: &SemanticType,
    ty: &SemanticType,
) -> SemanticType {
    let (_type_names, param_spec_names, _type_pack_names) = generic_param_name_sets(declaration);
    if param_spec_names.len() != 1 {
        return ty.clone();
    }
    let Some((actual_params, _)) = current_callable.callable_parts() else {
        return ty.clone();
    };
    let SemanticCallableParams::ParamList(actual_param_types) = actual_params else {
        return ty.clone();
    };
    let mut substitutions = GenericTypeParamSubstitutions::default();
    let Some(param_name) = param_spec_names.into_iter().next() else {
        return ty.clone();
    };
    if insert_param_spec_binding(
        &mut substitutions,
        &param_name,
        ParamListBinding { params: synthesize_semantic_param_list_binding(actual_param_types.clone()) },
    )
    .is_none()
    {
        return ty.clone();
    }
    substitute_semantic_type_params(ty, &substitutions)
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
    let options = context.assignability_options();
    let resolved =
        resolve_direct_single_semantic_argument_candidate_detailed(
            decorator_node,
            nodes,
            decorator,
            current_callable,
            options,
        ).ok()?;
    if !decorator_candidate_accepts_semantic_callable(
        decorator_node,
        nodes,
        &resolved,
        current_callable,
        options,
    ) {
        return None;
    }
    let transformed = substitute_single_paramspec_from_semantic_callable(
        decorator,
        current_callable,
        resolved.return_type.as_ref()?,
    );
    Some(rewrite_imported_typing_semantic_type(decorator_node, &transformed))
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
    let transform_decorators = decorated
        .decorators
        .iter()
        .filter(|decorator| !decorator_is_semantic_metadata_only(decorator))
        .collect::<Vec<_>>();

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
    for decorator in transform_decorators.into_iter().rev() {
        let Some(next) = apply_named_callable_decorator_transform_semantic_with_context(
            context,
            node,
            nodes,
            decorator,
            &current,
        ) else {
            if context.strict {
                return None;
            }
            return Some(dynamic_callable_boundary());
        };
        if next.callable_parts().is_none() {
            if context.strict {
                return Some(next);
            }
            return Some(dynamic_callable_boundary());
        }
        current = next;
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

fn dynamic_callable_boundary() -> SemanticType {
    SemanticType::Callable {
        params: SemanticCallableParams::Ellipsis,
        return_type: Box::new(SemanticType::Name(String::from("dynamic"))),
    }
}

pub(super) fn resolve_metaclass_call_declaration_with_context<'a>(
    context: &CheckerContext<'_>,
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration, String)> {
    let (class_node, class_decl) = resolve_direct_base(nodes, node, callee)?;
    let module_info = context.load_dataclass_transform_module_info(class_node)?;
    let metaclass_name = module_info
        .classes
        .iter()
        .find(|class_site| class_site.name == class_decl.name)
        .and_then(|class_site| class_site.metaclass.clone())?;
    let (metaclass_node, metaclass_decl) = resolve_direct_base(nodes, class_node, &metaclass_name)?;
    let call = find_owned_callable_declaration(nodes, metaclass_node, metaclass_decl, "__call__")?;
    Some((metaclass_node, call, metaclass_decl.name.clone()))
}

pub(super) fn resolve_direct_callable_return_semantic_type<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Option<SemanticType> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    if let Some(callable_type) =
        resolve_decorated_function_callable_semantic_type_with_context(
            &context,
            node,
            nodes,
            callee,
        )
    {
        return decorated_function_return_semantic_type_from_semantic_callable(&callable_type);
    }
    if let Some(function) = resolve_direct_function(node, nodes, callee) {
        let return_type = function.owner.as_ref().map_or_else(
            || declaration_signature_return_semantic_type(function),
            |owner| declaration_signature_return_semantic_type_with_self(function, &owner.name),
        )?;
        return Some(if function.is_async {
            SemanticType::Generic {
                head: String::from("Awaitable"),
                args: vec![return_type],
            }
        } else {
            return_type
        });
    }

    if let Some((metaclass_node, call, metaclass_name)) =
        resolve_metaclass_call_declaration_with_context(&context, node, nodes, callee)
    {
        return declaration_signature_return_semantic_type_with_self(call, &metaclass_name)
            .map(|return_type| rewrite_imported_typing_semantic_type(metaclass_node, &return_type));
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

pub(super) fn resolve_direct_callable_return_semantic_type_for_line_with_options(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
    line: usize,
    options: AssignabilityOptions,
) -> Option<SemanticType> {
    let context = checker_context_for_assignability_options(nodes, options);
    resolve_direct_callable_return_semantic_type_for_line_with_context(
        &context, node, nodes, callee, line,
    )
}

pub(super) fn resolve_direct_callable_return_semantic_type_for_line_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
    line: usize,
) -> Option<SemanticType> {
    let options = context.assignability_options();
    let call = node
        .calls
        .iter()
        .find(|call| call.callee == callee && call.line == line)
        .or_else(|| node.calls.iter().find(|call| call.callee == callee))?;
    let call_scope = context
        .load_direct_call_context_sites(node)
        .into_iter()
        .find(|site| site.line == call.line && site.callee == call.callee);
    let current_owner_name = call_scope.as_ref().and_then(|site| site.owner_name.as_deref());
    let current_owner_type_name =
        call_scope.as_ref().and_then(|site| site.owner_type_name.as_deref());
    let overloads = resolve_direct_overloads(node, nodes, callee);
    if !overloads.is_empty() {
        let attempts = overloads
            .iter()
            .map(|declaration| {
                (
                    *declaration,
                    resolve_direct_call_candidate_detailed_in_scope_with_options(
                        node,
                        nodes,
                        declaration,
                        call,
                        current_owner_name,
                        current_owner_type_name,
                        options,
                    ),
                )
            })
            .collect::<Vec<_>>();
        return match resolve_overload_selection_from_attempts_in_scope_with_options(
            node,
            nodes,
            call,
            attempts,
            current_owner_name,
            current_owner_type_name,
            options,
        ) {
            ResolvedOverloadSelection::Selected(candidate) => candidate.return_type,
            _ => None,
        };
    }
    if let Some(callable_type) =
        resolve_decorated_function_callable_semantic_type_with_context(context, node, nodes, callee)
    {
        return decorated_function_return_semantic_type_from_semantic_callable(&callable_type);
    }
    let function = resolve_direct_function(node, nodes, callee)?;
    resolve_direct_call_candidate_with_context_detailed(context, node, nodes, function, call)
        .ok()?
        .return_type
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

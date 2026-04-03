use std::collections::{BTreeMap, BTreeSet};

use super::*;

pub(crate) type GenericTypeParamSubstitutions = GenericSolution;

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub(crate) struct GenericSolution {
    pub(crate) types: BTreeMap<String, SemanticType>,
    pub(crate) param_lists: BTreeMap<String, ParamListBinding>,
    pub(crate) type_packs: BTreeMap<String, TypePackBinding>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ParamListBinding {
    pub(crate) params: Vec<typepython_syntax::DirectFunctionParamSite>,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub(crate) struct TypePackBinding {
    pub(crate) types: Vec<SemanticType>,
}

pub(crate) fn infer_generic_type_param_substitutions(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    function: &Declaration,
    signature: &[typepython_syntax::DirectFunctionParamSite],
    call: &typepython_binding::CallSite,
) -> Option<GenericTypeParamSubstitutions> {
    let type_names = function
        .type_params
        .iter()
        .filter(|type_param| type_param.kind == typepython_binding::GenericTypeParamKind::TypeVar)
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();
    let param_spec_names = function
        .type_params
        .iter()
        .filter(|type_param| type_param.kind == typepython_binding::GenericTypeParamKind::ParamSpec)
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();
    let type_pack_names = function
        .type_params
        .iter()
        .filter(|type_param| {
            type_param.kind == typepython_binding::GenericTypeParamKind::TypeVarTuple
        })
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();
    let mut substitutions = GenericTypeParamSubstitutions::default();
    let expected_positional_arg_types =
        expected_positional_arg_types_from_signature_sites(signature, call.arg_count);
    let (positional_types, variadic_starred_types) =
        expanded_positional_arg_types(node, nodes, call, &expected_positional_arg_types);
    let mut positional_index = 0;

    for param in signature.iter().filter(|param| !param.keyword_only && !param.keyword_variadic) {
        let Some(annotation) = param.annotation.as_deref() else {
            if param.variadic {
                positional_index = positional_types.len();
            } else if positional_index < positional_types.len() {
                positional_index += 1;
            }
            continue;
        };
        if param.variadic {
            if extract_param_spec_args_name(annotation).is_some() {
                positional_index = positional_types.len();
                continue;
            }
            if let Some(type_pack_name) =
                type_pack_name_from_unpack_annotation(annotation, &type_pack_names)
            {
                if !variadic_starred_types.is_empty() {
                    return None;
                }
                insert_type_pack_binding(
                    &mut substitutions,
                    &type_pack_name,
                    TypePackBinding {
                        types: positional_types[positional_index..]
                            .iter()
                            .map(|ty| lower_type_text_or_name(ty))
                            .collect(),
                    },
                )?;
                positional_index = positional_types.len();
                continue;
            }
            for actual in positional_types.iter().skip(positional_index) {
                bind_generic_type_params(
                    node,
                    nodes,
                    annotation,
                    actual,
                    &type_names,
                    &type_pack_names,
                    &mut substitutions,
                )?;
            }
            positional_index = positional_types.len();
            continue;
        }
        let Some(actual) = positional_types.get(positional_index) else {
            continue;
        };
        let annotation_mentions_param_spec = parse_callable_annotation_parts(annotation)
            .is_some_and(|(params_expr, _)| {
                callable_param_expr_mentions_param_spec(&params_expr, &param_spec_names)
            });
        bind_callable_param_spec_type_params(
            node,
            nodes,
            annotation,
            actual,
            call.arg_values.get(positional_index),
            &type_names,
            &param_spec_names,
            &mut substitutions,
        )?;
        if annotation_mentions_param_spec {
            positional_index += 1;
            continue;
        }
        bind_generic_type_params(
            node,
            nodes,
            annotation,
            actual,
            &type_names,
            &type_pack_names,
            &mut substitutions,
        )?;
        positional_index += 1;
    }

    for (index, (keyword, actual)) in
        call.keyword_names.iter().zip(&call.keyword_arg_types).enumerate()
    {
        let Some(param) = signature.iter().find(|param| param.name == *keyword) else {
            continue;
        };
        let Some(annotation) = param.annotation.as_deref() else {
            continue;
        };
        let annotation_mentions_param_spec = parse_callable_annotation_parts(annotation)
            .is_some_and(|(params_expr, _)| {
                callable_param_expr_mentions_param_spec(&params_expr, &param_spec_names)
            });
        bind_callable_param_spec_type_params(
            node,
            nodes,
            annotation,
            actual,
            call.keyword_arg_values.get(index),
            &type_names,
            &param_spec_names,
            &mut substitutions,
        )?;
        if annotation_mentions_param_spec {
            continue;
        }
        bind_generic_type_params(
            node,
            nodes,
            annotation,
            actual,
            &type_names,
            &type_pack_names,
            &mut substitutions,
        )?;
    }

    for type_param in &function.type_params {
        match type_param.kind {
            typepython_binding::GenericTypeParamKind::TypeVar => {
                if !substitutions.types.contains_key(&type_param.name)
                    && let Some(default) = &type_param.default
                {
                    substitutions
                        .types
                        .insert(type_param.name.clone(), lower_type_text_or_name(default));
                }
                let Some(actual) = substitutions.types.get(&type_param.name) else {
                    continue;
                };
                if !generic_type_param_accepts_actual(node, nodes, type_param, actual) {
                    return None;
                }
            }
            typepython_binding::GenericTypeParamKind::ParamSpec => {
                if substitutions.param_lists.contains_key(&type_param.name) {
                    continue;
                }
                let Some(default) = &type_param.default else {
                    continue;
                };
                substitutions
                    .param_lists
                    .insert(type_param.name.clone(), param_list_binding_from_default(default)?);
            }
            typepython_binding::GenericTypeParamKind::TypeVarTuple => {
                if substitutions.type_packs.contains_key(&type_param.name) {
                    continue;
                }
                let Some(default) = &type_param.default else {
                    continue;
                };
                substitutions
                    .type_packs
                    .insert(type_param.name.clone(), type_pack_binding_from_default(default)?);
            }
        }
    }

    Some(substitutions)
}

pub(crate) fn instantiate_direct_function_signature(
    signature: &[typepython_syntax::DirectFunctionParamSite],
    substitutions: &GenericTypeParamSubstitutions,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    let mut instantiated = Vec::new();
    let mut expanded_param_specs = BTreeSet::new();

    for param in signature {
        let param_spec_name = param.annotation.as_deref().and_then(|annotation| {
            if param.variadic {
                extract_param_spec_args_name(annotation)
            } else if param.keyword_variadic {
                extract_param_spec_kwargs_name(annotation)
            } else {
                None
            }
        });
        if let Some(param_spec_name) = param_spec_name {
            let binding = substitutions.param_lists.get(param_spec_name)?;
            if expanded_param_specs.insert(param_spec_name.to_owned()) {
                instantiated.extend(
                    binding
                        .params
                        .iter()
                        .cloned()
                        .map(|param| instantiate_direct_function_param(param, substitutions)),
                );
            }
            continue;
        }
        if param.variadic
            && let Some(annotation) = param.annotation.as_deref()
            && let Some(type_pack_name) = unpack_inner(annotation)
            && let Some(binding) = substitutions.type_packs.get(type_pack_name.trim())
        {
            instantiated.extend(binding.types.iter().enumerate().map(|(index, element_type)| {
                typepython_syntax::DirectFunctionParamSite {
                    name: format!("{}{}", param.name, index),
                    annotation: Some(render_semantic_type(element_type)),
                    has_default: false,
                    positional_only: true,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                }
            }));
            continue;
        }
        instantiated.push(instantiate_direct_function_param(param.clone(), substitutions));
    }

    Some(instantiated)
}

pub(crate) fn instantiate_direct_function_param(
    mut param: typepython_syntax::DirectFunctionParamSite,
    substitutions: &GenericTypeParamSubstitutions,
) -> typepython_syntax::DirectFunctionParamSite {
    param.annotation = param
        .annotation
        .as_deref()
        .map(|annotation| substitute_generic_type_params(annotation, substitutions));
    param
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn bind_callable_param_spec_type_params(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &str,
    actual: &str,
    actual_value: Option<&typepython_syntax::DirectExprMetadata>,
    type_names: &BTreeSet<String>,
    param_spec_names: &BTreeSet<String>,
    substitutions: &mut GenericTypeParamSubstitutions,
) -> Option<()> {
    if param_spec_names.is_empty() {
        return Some(());
    }

    let Some((expected_params_expr, expected_return)) = parse_callable_annotation_parts(annotation)
    else {
        return Some(());
    };
    if !callable_param_expr_mentions_param_spec(&expected_params_expr, param_spec_names) {
        return Some(());
    }

    let (actual_binding, actual_return) =
        resolve_callable_shape_from_actual(node, nodes, actual, actual_value)?;
    bind_callable_param_expr(
        node,
        nodes,
        &expected_params_expr,
        &actual_binding,
        type_names,
        param_spec_names,
        substitutions,
    )?;
    bind_generic_type_params(
        node,
        nodes,
        &expected_return,
        &actual_return,
        type_names,
        &BTreeSet::new(),
        substitutions,
    )
}

pub(crate) fn callable_param_expr_mentions_param_spec(
    params_expr: &str,
    param_spec_names: &BTreeSet<String>,
) -> bool {
    if param_spec_names.contains(params_expr.trim()) {
        return true;
    }
    if let Some(inner) =
        params_expr.trim().strip_prefix("Concatenate[").and_then(|inner| inner.strip_suffix(']'))
    {
        return split_top_level_type_args(inner)
            .last()
            .is_some_and(|tail| param_spec_names.contains(tail.trim()));
    }
    false
}

pub(crate) fn bind_callable_param_expr(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_params_expr: &str,
    actual_binding: &ParamListBinding,
    type_names: &BTreeSet<String>,
    param_spec_names: &BTreeSet<String>,
    substitutions: &mut GenericTypeParamSubstitutions,
) -> Option<()> {
    let expected_params_expr = expected_params_expr.trim();
    if let Some(param_spec_name) =
        param_spec_names.contains(expected_params_expr).then_some(expected_params_expr)
    {
        return insert_param_spec_binding(substitutions, param_spec_name, actual_binding.clone());
    }

    if let Some(inner) =
        expected_params_expr.strip_prefix("Concatenate[").and_then(|inner| inner.strip_suffix(']'))
    {
        let parts = split_top_level_type_args(inner);
        let (tail, prefixes) = parts.split_last()?;
        let tail = tail.trim();
        if !param_spec_names.contains(tail) || actual_binding.params.len() < prefixes.len() {
            return None;
        }
        for (expected_prefix, actual_param) in prefixes.iter().zip(actual_binding.params.iter()) {
            let actual_annotation = actual_param
                .annotation
                .as_deref()
                .filter(|annotation| !annotation.is_empty())
                .unwrap_or("dynamic");
            bind_generic_type_params(
                node,
                nodes,
                expected_prefix,
                actual_annotation,
                type_names,
                &BTreeSet::new(),
                substitutions,
            )?;
        }
        return insert_param_spec_binding(
            substitutions,
            tail,
            ParamListBinding { params: actual_binding.params[prefixes.len()..].to_vec() },
        );
    }

    Some(())
}

pub(crate) fn insert_param_spec_binding(
    substitutions: &mut GenericTypeParamSubstitutions,
    name: &str,
    binding: ParamListBinding,
) -> Option<()> {
    match substitutions.param_lists.get(name) {
        Some(existing) if existing != &binding => None,
        Some(_) => Some(()),
        None => {
            substitutions.param_lists.insert(name.to_owned(), binding);
            Some(())
        }
    }
}

pub(crate) fn resolve_callable_shape_from_actual(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    actual: &str,
    actual_value: Option<&typepython_syntax::DirectExprMetadata>,
) -> Option<(ParamListBinding, String)> {
    if let Some(actual_value) = actual_value
        && let Some(shape) = resolve_callable_shape_from_metadata(node, nodes, actual_value, actual)
    {
        return Some(shape);
    }

    let (params, return_type) = parse_callable_annotation(actual)?;
    Some((ParamListBinding { params: synthesize_param_list_binding(params?) }, return_type))
}

pub(crate) fn resolve_callable_shape_from_metadata(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    actual_value: &typepython_syntax::DirectExprMetadata,
    actual: &str,
) -> Option<(ParamListBinding, String)> {
    if let Some(lambda) = actual_value.value_lambda.as_deref() {
        let (param_types, return_type) = parse_callable_annotation(actual)?;
        let param_types = param_types?;
        if param_types.len() != lambda.params.len() {
            return None;
        }
        let params = lambda
            .params
            .iter()
            .zip(param_types)
            .map(|(param, annotation)| typepython_syntax::DirectFunctionParamSite {
                name: param.name.clone(),
                annotation: Some(annotation),
                has_default: param.has_default,
                positional_only: param.positional_only,
                keyword_only: param.keyword_only,
                variadic: param.variadic,
                keyword_variadic: param.keyword_variadic,
            })
            .collect();
        return Some((ParamListBinding { params }, return_type));
    }

    let function_name = actual_value.value_name.as_deref()?;
    if let Some(callable_annotation) =
        resolve_decorated_function_callable_annotation(node, nodes, function_name)
    {
        let signature =
            direct_function_signature_sites_from_callable_annotation(&callable_annotation)?;
        let return_type =
            decorated_function_return_type_from_callable_annotation(&callable_annotation)?;
        return Some((ParamListBinding { params: signature }, return_type));
    }
    let function = resolve_direct_function(node, nodes, function_name)?;
    let return_type = function.detail.split_once("->")?.1.trim().to_owned();
    Some((
        ParamListBinding { params: direct_signature_sites_from_detail(&function.detail) },
        return_type,
    ))
}

pub(crate) fn synthesize_param_list_binding(
    param_types: Vec<String>,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    param_types
        .into_iter()
        .enumerate()
        .map(|(index, annotation)| typepython_syntax::DirectFunctionParamSite {
            name: format!("arg{index}"),
            annotation: Some(annotation),
            has_default: false,
            positional_only: false,
            keyword_only: false,
            variadic: false,
            keyword_variadic: false,
        })
        .collect()
}

pub(crate) fn param_list_binding_from_default(default: &str) -> Option<ParamListBinding> {
    let default = normalize_callable_param_expr(default);
    if default == "..." {
        return Some(ParamListBinding { params: Vec::new() });
    }
    if let Some(inner) = default.strip_prefix('[').and_then(|inner| inner.strip_suffix(']')) {
        let params = if inner.trim().is_empty() {
            Vec::new()
        } else {
            synthesize_param_list_binding(
                split_top_level_type_args(inner).into_iter().map(normalize_type_text).collect(),
            )
        };
        return Some(ParamListBinding { params });
    }
    None
}

pub(crate) fn extract_param_spec_args_name(annotation: &str) -> Option<&str> {
    annotation.strip_suffix(".args").map(str::trim).filter(|name| !name.is_empty())
}

pub(crate) fn extract_param_spec_kwargs_name(annotation: &str) -> Option<&str> {
    annotation.strip_suffix(".kwargs").map(str::trim).filter(|name| !name.is_empty())
}

pub(crate) fn generic_type_param_accepts_actual(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_param: &typepython_binding::GenericTypeParam,
    actual: &SemanticType,
) -> bool {
    let actual = render_semantic_type(actual);
    if let Some(bound) = &type_param.bound {
        return direct_type_is_assignable(node, nodes, bound, &actual);
    }
    if !type_param.constraints.is_empty() {
        return type_param
            .constraints
            .iter()
            .any(|constraint| direct_type_is_assignable(node, nodes, constraint, &actual));
    }
    true
}

pub(crate) fn bind_generic_type_params(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &str,
    actual: &str,
    generic_names: &BTreeSet<String>,
    type_pack_names: &BTreeSet<String>,
    substitutions: &mut GenericTypeParamSubstitutions,
) -> Option<()> {
    let inferred = infer_generic_type_param_bindings(
        node,
        nodes,
        annotation,
        actual,
        generic_names,
        substitutions,
        type_pack_names,
    )?;
    for (name, actual_type) in inferred.types {
        match substitutions.types.get(&name) {
            Some(existing) if existing != &actual_type => {
                substitutions
                    .types
                    .insert(name, merge_generic_type_candidates(existing, &actual_type));
            }
            Some(_) => {}
            None => {
                substitutions.types.insert(name, actual_type);
            }
        }
    }
    for (name, binding) in inferred.type_packs {
        insert_type_pack_binding(substitutions, &name, binding)?;
    }
    Some(())
}

pub(crate) fn infer_generic_type_param_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &str,
    actual: &str,
    generic_names: &BTreeSet<String>,
    substitutions: &GenericTypeParamSubstitutions,
    type_pack_names: &BTreeSet<String>,
) -> Option<GenericTypeParamSubstitutions> {
    let annotation = lower_type_text_or_name(annotation);
    let actual = lower_type_text_or_name(actual);
    infer_generic_type_param_bindings_full(
        node,
        nodes,
        &annotation,
        &actual,
        generic_names,
        substitutions,
        type_pack_names,
    )
}

fn infer_generic_type_param_bindings_full(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &SemanticType,
    actual: &SemanticType,
    generic_names: &BTreeSet<String>,
    substitutions: &GenericTypeParamSubstitutions,
    type_pack_names: &BTreeSet<String>,
) -> Option<GenericTypeParamSubstitutions> {
    if let SemanticType::Name(name) = &actual
        && name.is_empty()
    {
        return Some(GenericTypeParamSubstitutions::default());
    }

    if let SemanticType::Name(name) = &annotation
        && generic_names.contains(name)
    {
        let candidate = substitutions
            .types
            .get(name)
            .map(|existing| merge_generic_type_candidates(existing, actual))
            .unwrap_or_else(|| actual.clone());
        let mut inferred = GenericTypeParamSubstitutions::default();
        inferred.types.insert(name.clone(), candidate);
        return Some(inferred);
    }

    if let Some(branches) = semantic_union_branches(actual)
        && branches.len() > 1
    {
        let mut candidates = Vec::new();
        for branch in branches {
            let candidate = infer_generic_type_param_bindings_full(
                node,
                nodes,
                annotation,
                &branch,
                generic_names,
                substitutions,
                type_pack_names,
            )?;
            let combined = combine_generic_substitutions(substitutions, &candidate);
            let substituted_annotation = substitute_semantic_type_params(annotation, &combined);
            if !direct_type_is_assignable(
                node,
                nodes,
                &render_semantic_type(&substituted_annotation),
                &render_semantic_type(&branch),
            ) {
                return None;
            }
            candidates.push(candidate);
        }
        return merge_union_branch_bindings(candidates);
    }

    if let Some(branches) = semantic_union_branches(annotation)
        && branches.len() > 1
    {
        let candidates = branches
            .into_iter()
            .filter_map(|branch| {
                let candidate = infer_generic_type_param_bindings_full(
                    node,
                    nodes,
                    &branch,
                    actual,
                    generic_names,
                    substitutions,
                    type_pack_names,
                )?;
                let combined = combine_generic_substitutions(substitutions, &candidate);
                let substituted_branch = substitute_semantic_type_params(&branch, &combined);
                direct_type_is_assignable(
                    node,
                    nodes,
                    &render_semantic_type(&substituted_branch),
                    &render_semantic_type(actual),
                )
                .then_some(candidate)
            })
            .collect::<Vec<_>>();
        return select_best_union_branch_binding(candidates);
    }

    infer_generic_type_param_bindings_semantic(
        node,
        nodes,
        annotation,
        actual,
        generic_names,
        substitutions,
        type_pack_names,
    )
}

pub(crate) fn infer_generic_type_param_bindings_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &SemanticType,
    actual: &SemanticType,
    generic_names: &BTreeSet<String>,
    substitutions: &GenericTypeParamSubstitutions,
    type_pack_names: &BTreeSet<String>,
) -> Option<GenericTypeParamSubstitutions> {
    match (annotation.generic_parts(), actual.generic_parts()) {
        (Some((expected_head, expected_args)), Some((actual_head, actual_args)))
            if expected_head == actual_head =>
        {
            infer_generic_type_arg_bindings(
                node,
                nodes,
                expected_args,
                actual_args,
                generic_names,
                substitutions,
                type_pack_names,
            )
        }
        _ => direct_type_is_assignable(
            node,
            nodes,
            &render_semantic_type(annotation),
            &render_semantic_type(actual),
        )
        .then_some(GenericTypeParamSubstitutions::default()),
    }
}

pub(crate) fn combine_generic_substitutions(
    existing: &GenericTypeParamSubstitutions,
    inferred: &GenericTypeParamSubstitutions,
) -> GenericTypeParamSubstitutions {
    let mut combined = existing.clone();
    combined.types.extend(inferred.types.clone());
    combined.param_lists.extend(inferred.param_lists.clone());
    combined.type_packs.extend(inferred.type_packs.clone());
    combined
}

pub(crate) fn select_best_union_branch_binding(
    candidates: Vec<GenericTypeParamSubstitutions>,
) -> Option<GenericTypeParamSubstitutions> {
    let min_len = candidates.iter().map(generic_binding_count).min()?;
    let mut filtered =
        candidates.into_iter().filter(|candidate| generic_binding_count(candidate) == min_len);
    let first = filtered.next()?;
    if filtered.all(|candidate| candidate == first) { Some(first) } else { None }
}

pub(crate) fn merge_union_branch_bindings(
    candidates: Vec<GenericTypeParamSubstitutions>,
) -> Option<GenericTypeParamSubstitutions> {
    let mut merged = GenericTypeParamSubstitutions::default();
    for candidate in candidates {
        for (name, actual_type) in candidate.types {
            match merged.types.get(&name) {
                Some(existing) if existing == &actual_type => {}
                Some(existing) => {
                    merged.types.insert(
                        name,
                        join_semantic_type_candidates(vec![existing.clone(), actual_type]),
                    );
                }
                None => {
                    merged.types.insert(name, actual_type);
                }
            }
        }
        for (name, binding) in candidate.type_packs {
            insert_type_pack_binding(&mut merged, &name, binding)?;
        }
    }
    Some(merged)
}

pub(crate) fn merge_generic_type_candidates(
    existing: &SemanticType,
    actual: &SemanticType,
) -> SemanticType {
    if existing == actual {
        existing.clone()
    } else {
        join_semantic_type_candidates(vec![existing.clone(), actual.clone()])
    }
}

pub(crate) fn substitute_semantic_type_params(
    annotation: &SemanticType,
    substitutions: &GenericTypeParamSubstitutions,
) -> SemanticType {
    match annotation {
        SemanticType::Name(name) => substitutions
            .types
            .get(name)
            .cloned()
            .unwrap_or_else(|| SemanticType::Name(name.clone())),
        SemanticType::Generic { head, args } => SemanticType::Generic {
            head: head.clone(),
            args: expand_substituted_semantic_generic_args(args, substitutions),
        },
        SemanticType::Callable { params, return_type } => SemanticType::Callable {
            params: substitute_semantic_callable_param_expr(params, substitutions),
            return_type: Box::new(substitute_semantic_type_params(return_type, substitutions)),
        },
        SemanticType::Union(branches) => join_semantic_type_candidates(
            branches
                .iter()
                .map(|branch| substitute_semantic_type_params(branch, substitutions))
                .collect(),
        ),
        SemanticType::Annotated { value, metadata } => SemanticType::Annotated {
            value: Box::new(substitute_semantic_type_params(value, substitutions)),
            metadata: metadata.clone(),
        },
        SemanticType::Unpack(inner) => {
            SemanticType::Unpack(Box::new(substitute_semantic_type_params(inner, substitutions)))
        }
    }
}

pub(crate) fn substitute_semantic_callable_param_expr(
    params: &SemanticCallableParams,
    substitutions: &GenericTypeParamSubstitutions,
) -> SemanticCallableParams {
    match params {
        SemanticCallableParams::Ellipsis => SemanticCallableParams::Ellipsis,
        SemanticCallableParams::ParamList(types) => SemanticCallableParams::ParamList(
            expand_substituted_semantic_generic_args(types, substitutions),
        ),
        SemanticCallableParams::Concatenate(types) => {
            if let Some((tail, prefixes)) = types.split_last()
                && let SemanticType::Name(name) = tail
                && let Some(binding) = substitutions.param_lists.get(name.trim())
            {
                let mut rendered = prefixes
                    .iter()
                    .map(|part| substitute_semantic_type_params(part, substitutions))
                    .collect::<Vec<_>>();
                rendered.extend(binding.params.iter().map(param_annotation_semantic_type));
                SemanticCallableParams::ParamList(rendered)
            } else {
                SemanticCallableParams::Concatenate(
                    types
                        .iter()
                        .map(|part| substitute_semantic_type_params(part, substitutions))
                        .collect(),
                )
            }
        }
        SemanticCallableParams::Single(expr) => {
            if let SemanticType::Name(name) = expr.as_ref()
                && let Some(binding) = substitutions.param_lists.get(name.trim())
            {
                return SemanticCallableParams::ParamList(
                    binding.params.iter().map(param_annotation_semantic_type).collect(),
                );
            }
            SemanticCallableParams::Single(Box::new(substitute_semantic_type_params(
                expr,
                substitutions,
            )))
        }
    }
}

pub(crate) fn param_annotation_semantic_type(
    param: &typepython_syntax::DirectFunctionParamSite,
) -> SemanticType {
    param
        .annotation
        .as_deref()
        .map(lower_type_text_or_name)
        .unwrap_or_else(|| SemanticType::Name(String::from("dynamic")))
}

pub(crate) fn substitute_generic_type_params(
    annotation: &str,
    substitutions: &GenericTypeParamSubstitutions,
) -> String {
    let annotation = lower_type_text_or_name(annotation);
    render_semantic_type(&substitute_semantic_type_params(&annotation, substitutions))
}

pub(crate) fn expand_substituted_semantic_generic_args(
    args: &[SemanticType],
    substitutions: &GenericTypeParamSubstitutions,
) -> Vec<SemanticType> {
    let mut rendered = Vec::new();
    for arg in args {
        if let Some(inner) = arg.unpacked_inner() {
            if let SemanticType::Name(name) = inner
                && let Some(binding) = substitutions.type_packs.get(name.trim())
            {
                rendered.extend(binding.types.iter().cloned());
                continue;
            }
            if let Some(elements) = unpacked_fixed_tuple_semantic_elements(inner) {
                rendered.extend(elements);
                continue;
            }
        }
        rendered.push(substitute_semantic_type_params(arg, substitutions));
    }
    rendered
}

pub(crate) fn unpacked_fixed_tuple_elements(text: &str) -> Option<Vec<String>> {
    unpacked_fixed_tuple_semantic_elements(&lower_type_text_or_name(text)).map(|elements| {
        elements.into_iter().map(|element| render_semantic_type(&element)).collect()
    })
}

pub(crate) fn insert_type_pack_binding(
    substitutions: &mut GenericTypeParamSubstitutions,
    name: &str,
    binding: TypePackBinding,
) -> Option<()> {
    match substitutions.type_packs.get(name) {
        Some(existing) if existing == &binding => Some(()),
        Some(existing) => {
            let merged = merge_type_pack_candidates(existing, &binding)?;
            substitutions.type_packs.insert(name.to_owned(), merged);
            Some(())
        }
        None => {
            substitutions.type_packs.insert(name.to_owned(), binding);
            Some(())
        }
    }
}

pub(crate) fn merge_type_pack_candidates(
    existing: &TypePackBinding,
    actual: &TypePackBinding,
) -> Option<TypePackBinding> {
    if existing.types.len() != actual.types.len() {
        return None;
    }
    Some(TypePackBinding {
        types: existing
            .types
            .iter()
            .zip(&actual.types)
            .map(|(left, right)| merge_generic_type_candidates(left, right))
            .collect(),
    })
}

pub(crate) fn type_pack_name_from_unpack_annotation(
    annotation: &str,
    type_pack_names: &BTreeSet<String>,
) -> Option<String> {
    let inner = unpack_inner(&normalize_type_text(annotation))?;
    let inner = inner.trim();
    type_pack_names.contains(inner).then(|| inner.to_owned())
}

pub(crate) fn type_pack_binding_from_default(default: &str) -> Option<TypePackBinding> {
    let normalized = normalize_type_text(default);
    if normalized == "tuple[()]" {
        return Some(TypePackBinding::default());
    }
    if let Some(elements) = unpacked_fixed_tuple_elements(&normalized) {
        return Some(TypePackBinding {
            types: elements.into_iter().map(|element| lower_type_text_or_name(&element)).collect(),
        });
    }
    None
}

pub(crate) fn generic_binding_count(solution: &GenericTypeParamSubstitutions) -> usize {
    solution.types.len() + solution.param_lists.len() + solution.type_packs.len()
}

pub(crate) fn infer_generic_type_arg_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_args: &[SemanticType],
    actual_args: &[SemanticType],
    generic_names: &BTreeSet<String>,
    substitutions: &GenericTypeParamSubstitutions,
    type_pack_names: &BTreeSet<String>,
) -> Option<GenericTypeParamSubstitutions> {
    let expected_args = expand_inferred_generic_args(expected_args, type_pack_names);
    let actual_args = expand_inferred_generic_args(actual_args, type_pack_names);
    let mut inferred = GenericTypeParamSubstitutions::default();

    if let Some((pack_index, pack_name)) = expected_type_pack_index(&expected_args, type_pack_names)
    {
        let suffix_len = expected_args.len().saturating_sub(pack_index + 1);
        if actual_args.len() < pack_index + suffix_len {
            return None;
        }
        for (expected_arg, actual_arg) in
            expected_args[..pack_index].iter().zip(actual_args[..pack_index].iter())
        {
            merge_nested_generic_bindings(
                &mut inferred,
                infer_generic_type_param_bindings_full(
                    node,
                    nodes,
                    expected_arg,
                    actual_arg,
                    generic_names,
                    substitutions,
                    type_pack_names,
                )?,
            )?;
        }
        let actual_pack_end = actual_args.len() - suffix_len;
        insert_type_pack_binding(
            &mut inferred,
            &pack_name,
            TypePackBinding { types: actual_args[pack_index..actual_pack_end].to_vec() },
        )?;
        for (expected_arg, actual_arg) in
            expected_args[pack_index + 1..].iter().zip(actual_args[actual_pack_end..].iter())
        {
            merge_nested_generic_bindings(
                &mut inferred,
                infer_generic_type_param_bindings_full(
                    node,
                    nodes,
                    expected_arg,
                    actual_arg,
                    generic_names,
                    substitutions,
                    type_pack_names,
                )?,
            )?;
        }
        return Some(inferred);
    }

    if expected_args.len() != actual_args.len() {
        return None;
    }
    for (expected_arg, actual_arg) in expected_args.iter().zip(actual_args.iter()) {
        merge_nested_generic_bindings(
            &mut inferred,
            infer_generic_type_param_bindings_full(
                node,
                nodes,
                expected_arg,
                actual_arg,
                generic_names,
                substitutions,
                type_pack_names,
            )?,
        )?;
    }
    Some(inferred)
}

pub(crate) fn merge_nested_generic_bindings(
    inferred: &mut GenericTypeParamSubstitutions,
    nested: GenericTypeParamSubstitutions,
) -> Option<()> {
    for (name, actual_type) in nested.types {
        match inferred.types.get(&name) {
            Some(existing) if existing != &actual_type => {
                let merged = merge_generic_type_candidates(existing, &actual_type);
                inferred.types.insert(name, merged);
            }
            Some(_) => {}
            None => {
                inferred.types.insert(name, actual_type);
            }
        }
    }
    for (name, binding) in nested.type_packs {
        insert_type_pack_binding(inferred, &name, binding)?;
    }
    Some(())
}

pub(crate) fn expand_inferred_generic_args(
    args: &[SemanticType],
    type_pack_names: &BTreeSet<String>,
) -> Vec<SemanticType> {
    let mut expanded = Vec::new();
    for arg in args {
        if let Some(inner) = arg.unpacked_inner() {
            if !matches!(inner, SemanticType::Name(name) if type_pack_names.contains(name.trim()))
                && let Some(elements) = unpacked_fixed_tuple_semantic_elements(inner)
            {
                expanded.extend(elements);
                continue;
            }
        }
        expanded.push(arg.clone());
    }
    expanded
}

pub(crate) fn expected_type_pack_index(
    args: &[SemanticType],
    type_pack_names: &BTreeSet<String>,
) -> Option<(usize, String)> {
    let matches = args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| match arg.unpacked_inner() {
            Some(SemanticType::Name(name)) if type_pack_names.contains(name.trim()) => {
                Some((index, name.trim().to_owned()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [(index, name)] => Some((*index, name.clone())),
        _ => None,
    }
}

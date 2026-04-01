use std::collections::{BTreeMap, BTreeSet};

use super::*;

pub(crate) type GenericTypeParamSubstitutions = GenericSolution;

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub(crate) struct GenericSolution {
    pub(crate) types: BTreeMap<String, String>,
    pub(crate) param_lists: BTreeMap<String, ParamListBinding>,
    pub(crate) type_packs: BTreeMap<String, TypePackBinding>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ParamListBinding {
    pub(crate) params: Vec<typepython_syntax::DirectFunctionParamSite>,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub(crate) struct TypePackBinding {
    pub(crate) types: Vec<String>,
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
    let mut substitutions = GenericTypeParamSubstitutions::default();

    for (index, (param, actual)) in signature
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .zip(call.arg_types.iter())
        .enumerate()
    {
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
            call.arg_values.get(index),
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
            &mut substitutions.types,
        )?;
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
            &mut substitutions.types,
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
                        .insert(type_param.name.clone(), normalize_type_text(default));
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
        &mut substitutions.types,
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
                &mut substitutions.types,
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
    actual: &str,
) -> bool {
    if actual.is_empty() {
        return true;
    }
    if let Some(bound) = &type_param.bound {
        return direct_type_is_assignable(node, nodes, bound, actual);
    }
    if !type_param.constraints.is_empty() {
        return type_param
            .constraints
            .iter()
            .any(|constraint| direct_type_is_assignable(node, nodes, constraint, actual));
    }
    true
}

pub(crate) fn bind_generic_type_params(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &str,
    actual: &str,
    generic_names: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, String>,
) -> Option<()> {
    let inferred = infer_generic_type_param_bindings(
        node,
        nodes,
        annotation,
        actual,
        generic_names,
        substitutions,
    )?;
    for (name, actual_type) in inferred {
        match substitutions.get(&name) {
            Some(existing) if existing != &actual_type => {
                substitutions.insert(name, merge_generic_type_candidates(existing, &actual_type));
            }
            Some(_) => {}
            None => {
                substitutions.insert(name, actual_type);
            }
        }
    }
    Some(())
}

pub(crate) fn infer_generic_type_param_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &str,
    actual: &str,
    generic_names: &BTreeSet<String>,
    substitutions: &BTreeMap<String, String>,
) -> Option<BTreeMap<String, String>> {
    let annotation = normalize_type_text(annotation);
    let actual = normalize_type_text(actual);
    if actual.is_empty() {
        return Some(BTreeMap::new());
    }

    if generic_names.contains(&annotation) {
        let candidate = substitutions
            .get(&annotation)
            .map(|existing| merge_generic_type_candidates(existing, &actual))
            .unwrap_or(actual);
        let mut inferred = BTreeMap::new();
        inferred.insert(annotation, candidate);
        return Some(inferred);
    }

    if let Some(branches) = union_branches(&actual)
        && branches.len() > 1
    {
        let mut candidates = Vec::new();
        for branch in branches {
            let candidate = infer_generic_type_param_bindings(
                node,
                nodes,
                &annotation,
                &branch,
                generic_names,
                substitutions,
            )?;
            let combined = combine_generic_substitutions(substitutions, &candidate);
            let substituted_annotation = substitute_type_substitutions(&annotation, &combined);
            if !direct_type_is_assignable(node, nodes, &substituted_annotation, &branch) {
                return None;
            }
            candidates.push(candidate);
        }
        return merge_union_branch_bindings(candidates);
    }

    if let Some(branches) = union_branches(&annotation)
        && branches.len() > 1
    {
        let candidates = branches
            .into_iter()
            .filter_map(|branch| {
                let candidate = infer_generic_type_param_bindings(
                    node,
                    nodes,
                    &branch,
                    &actual,
                    generic_names,
                    substitutions,
                )?;
                let combined = combine_generic_substitutions(substitutions, &candidate);
                let substituted_branch = substitute_type_substitutions(&branch, &combined);
                direct_type_is_assignable(node, nodes, &substituted_branch, &actual)
                    .then_some(candidate)
            })
            .collect::<Vec<_>>();
        return select_best_union_branch_binding(candidates);
    }

    match (split_generic_type(&annotation), split_generic_type(&actual)) {
        (Some((expected_head, expected_args)), Some((actual_head, actual_args)))
            if expected_head == actual_head && expected_args.len() == actual_args.len() =>
        {
            let mut inferred: BTreeMap<String, String> = BTreeMap::new();
            for (expected_arg, actual_arg) in expected_args.iter().zip(actual_args.iter()) {
                let nested = infer_generic_type_param_bindings(
                    node,
                    nodes,
                    expected_arg,
                    actual_arg,
                    generic_names,
                    substitutions,
                )?;
                for (name, actual_type) in nested {
                    match inferred.get(&name) {
                        Some(existing) if existing != &actual_type => {
                            let merged = merge_generic_type_candidates(existing, &actual_type);
                            inferred.insert(name, merged);
                        }
                        Some(_) => {}
                        None => {
                            inferred.insert(name, actual_type);
                        }
                    }
                }
            }
            Some(inferred)
        }
        _ => {
            direct_type_is_assignable(node, nodes, &annotation, &actual).then_some(BTreeMap::new())
        }
    }
}

pub(crate) fn combine_generic_substitutions(
    existing: &BTreeMap<String, String>,
    inferred: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut combined = existing.clone();
    combined.extend(inferred.clone());
    combined
}

pub(crate) fn select_best_union_branch_binding(
    candidates: Vec<BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    let min_len = candidates.iter().map(BTreeMap::len).min()?;
    let mut filtered = candidates.into_iter().filter(|candidate| candidate.len() == min_len);
    let first = filtered.next()?;
    if filtered.all(|candidate| candidate == first) { Some(first) } else { None }
}

pub(crate) fn merge_union_branch_bindings(
    candidates: Vec<BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    let mut merged: BTreeMap<String, String> = BTreeMap::new();
    for candidate in candidates {
        for (name, actual_type) in candidate {
            match merged.get(&name) {
                Some(existing) if existing == &actual_type => {}
                Some(existing) => {
                    merged.insert(name, join_type_candidates(vec![existing.clone(), actual_type]));
                }
                None => {
                    merged.insert(name, actual_type);
                }
            }
        }
    }
    Some(merged)
}

pub(crate) fn merge_generic_type_candidates(existing: &str, actual: &str) -> String {
    if existing == actual {
        existing.to_owned()
    } else {
        join_type_candidates(vec![existing.to_owned(), actual.to_owned()])
    }
}

pub(crate) fn substitute_type_substitutions(
    annotation: &str,
    substitutions: &BTreeMap<String, String>,
) -> String {
    let mut output = String::new();
    let mut token = String::new();
    for character in annotation.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            token.push(character);
            continue;
        }
        if !token.is_empty() {
            output.push_str(substitutions.get(&token).map(String::as_str).unwrap_or(&token));
            token.clear();
        }
        output.push(character);
    }
    if !token.is_empty() {
        output.push_str(substitutions.get(&token).map(String::as_str).unwrap_or(&token));
    }
    output
}

pub(crate) fn substitute_generic_type_params(
    annotation: &str,
    substitutions: &GenericTypeParamSubstitutions,
) -> String {
    if let Some((params_expr, return_type)) = parse_callable_annotation_parts(annotation) {
        let substituted_params = substitute_callable_param_expr(&params_expr, substitutions);
        let substituted_return = substitute_generic_type_params(&return_type, substitutions);
        return format!("Callable[{substituted_params}, {substituted_return}]");
    }

    substitute_type_substitutions(annotation, &substitutions.types)
}

pub(crate) fn substitute_callable_param_expr(
    params_expr: &str,
    substitutions: &GenericTypeParamSubstitutions,
) -> String {
    let params_expr = params_expr.trim();
    if params_expr == "..." || params_expr.is_empty() {
        return params_expr.to_owned();
    }
    if let Some(binding) = substitutions.param_lists.get(params_expr) {
        return render_param_list_binding_for_callable(binding, substitutions);
    }
    if let Some(inner) =
        params_expr.strip_prefix("Concatenate[").and_then(|inner| inner.strip_suffix(']'))
    {
        let parts = split_top_level_type_args(inner);
        if let Some((tail, prefixes)) = parts.split_last()
            && let Some(binding) = substitutions.param_lists.get(tail.trim())
        {
            let mut rendered = prefixes
                .iter()
                .map(|part| substitute_generic_type_params(part, substitutions))
                .collect::<Vec<_>>();
            rendered.extend(binding.params.iter().map(|param| {
                param
                    .annotation
                    .as_deref()
                    .map(|annotation| substitute_generic_type_params(annotation, substitutions))
                    .unwrap_or_else(|| String::from("dynamic"))
            }));
            return format!("[{}]", rendered.join(", "));
        }
    }
    if let Some(inner) = params_expr.strip_prefix('[').and_then(|inner| inner.strip_suffix(']')) {
        let rendered = if inner.trim().is_empty() {
            Vec::new()
        } else {
            split_top_level_type_args(inner)
                .into_iter()
                .map(|part| substitute_generic_type_params(part, substitutions))
                .collect::<Vec<_>>()
        };
        return format!("[{}]", rendered.join(", "));
    }

    params_expr.to_owned()
}

pub(crate) fn render_param_list_binding_for_callable(
    binding: &ParamListBinding,
    substitutions: &GenericTypeParamSubstitutions,
) -> String {
    let rendered = binding
        .params
        .iter()
        .map(|param| {
            param
                .annotation
                .as_deref()
                .map(|annotation| substitute_generic_type_params(annotation, substitutions))
                .unwrap_or_else(|| String::from("dynamic"))
        })
        .collect::<Vec<_>>();
    format!("[{}]", rendered.join(", "))
}

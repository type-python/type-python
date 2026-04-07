use super::*;

use crate::diagnostic_type_text as render_semantic_type;

pub(super) fn unsafe_boundary_diagnostics(
    node: &typepython_graph::ModuleNode,
    strict: bool,
    warn_unsafe: bool,
) -> Vec<Diagnostic> {
    if !strict || !warn_unsafe || node.module_kind != SourceKind::TypePython {
        return Vec::new();
    }
    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };
    typepython_syntax::collect_unsafe_operation_sites(&source)
        .into_iter()
        .filter(|site| !site.in_unsafe_block)
        .map(|site| {
            Diagnostic::warning(
                "TPY4019",
                match site.kind {
                    typepython_syntax::UnsafeOperationKind::EvalCall => String::from(
                        "unsafe boundary operation `eval(...)` must appear inside `unsafe:`",
                    ),
                    typepython_syntax::UnsafeOperationKind::ExecCall => String::from(
                        "unsafe boundary operation `exec(...)` must appear inside `unsafe:`",
                    ),
                    typepython_syntax::UnsafeOperationKind::GlobalsWrite => {
                        String::from("writes through `globals()` must appear inside `unsafe:`")
                    }
                    typepython_syntax::UnsafeOperationKind::LocalsWrite => {
                        String::from("writes through `locals()` must appear inside `unsafe:`")
                    }
                    typepython_syntax::UnsafeOperationKind::DictWrite => {
                        String::from("writes through `__dict__` must appear inside `unsafe:`")
                    }
                    typepython_syntax::UnsafeOperationKind::SetAttrNonLiteral => String::from(
                        "non-literal `setattr(obj, name, value)` must appear inside `unsafe:`",
                    ),
                    typepython_syntax::UnsafeOperationKind::DelAttrNonLiteral => String::from(
                        "non-literal `delattr(obj, name)` must appear inside `unsafe:`",
                    ),
                },
            )
            .with_span(Span::new(
                node.module_path.display().to_string(),
                site.line,
                1,
                site.line,
                1,
            ))
        })
        .collect()
}

pub(super) fn ambiguous_overload_call_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.calls
        .iter()
        .filter_map(|call| {
            let overloads = resolve_direct_overloads(node, nodes, &call.callee);
            if overloads.len() < 2 {
                return None;
            }

            match resolve_direct_overload_selection(node, nodes, call, &overloads) {
                ResolvedOverloadSelection::Ambiguous { applicable_count }
                    if applicable_count >= 2 =>
                {
                    Some(Diagnostic::error(
                        "TPY4012",
                        format!(
                            "call to `{}` in module `{}` is ambiguous across {} overloads after applicability filtering",
                            call.callee,
                            node.module_path.display(),
                            applicable_count
                        ),
                    ))
                }
                _ => None,
            }
        })
        .collect()
}

pub(super) fn resolve_direct_overloads<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Vec<&'a Declaration> {
    let local = node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.name == callee
                && declaration.owner.is_none()
                && declaration.kind == DeclarationKind::Overload
        })
        .collect::<Vec<_>>();
    if !local.is_empty() {
        return local;
    }

    let Some(import_target) = resolve_imported_symbol_semantic_target(node, nodes, callee) else {
        return Vec::new();
    };
    let Some(target) = import_target.declaration_target() else {
        return Vec::new();
    };
    let target_node = import_target.provider_node;
    target_node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.name == target.name
                && declaration.owner.is_none()
                && declaration.kind == DeclarationKind::Overload
        })
        .collect()
}

pub(super) fn overload_is_more_specific(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    candidate: &ResolvedDirectCallCandidate<'_>,
    baseline: &ResolvedDirectCallCandidate<'_>,
) -> bool {
    let candidate_params = &candidate.signature_params;
    let baseline_params = &baseline.signature_params;
    let candidate_semantic_params = &candidate.semantic_param_types;
    let baseline_semantic_params = &baseline.semantic_param_types;
    if candidate_params.len() != baseline_params.len() {
        return false;
    }

    let mut strictly_more_specific = false;
    for (index, (candidate_param, baseline_param)) in
        candidate_params.iter().zip(baseline_params.iter()).enumerate()
    {
        if candidate_param.name != baseline_param.name
            || candidate_param.has_default != baseline_param.has_default
            || candidate_param.positional_only != baseline_param.positional_only
            || candidate_param.keyword_only != baseline_param.keyword_only
            || candidate_param.variadic != baseline_param.variadic
            || candidate_param.keyword_variadic != baseline_param.keyword_variadic
        {
            return false;
        }
        if candidate_param.annotation.is_none() || baseline_param.annotation.is_none() {
            if candidate_param.annotation_text != baseline_param.annotation_text {
                return false;
            }
            continue;
        }
        let Some(candidate_param_type) = candidate_semantic_params.get(index) else {
            return false;
        };
        let Some(baseline_param_type) = baseline_semantic_params.get(index) else {
            return false;
        };
        if !semantic_type_is_assignable(node, nodes, baseline_param_type, candidate_param_type) {
            return false;
        }
        if candidate_param_type != baseline_param_type {
            strictly_more_specific = true;
        }
    }

    strictly_more_specific
}

fn select_most_specific_overload_index(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    applicable: &[ResolvedDirectCallCandidate<'_>],
) -> Option<usize> {
    if applicable.len() == 1 {
        return Some(0);
    }

    let best = applicable
        .iter()
        .enumerate()
        .filter(|candidate| {
            applicable.iter().all(|other| {
                std::ptr::eq::<Declaration>(candidate.1.declaration, other.declaration)
                    || overload_is_more_specific(node, nodes, candidate.1, other)
            })
        })
        .collect::<Vec<_>>();

    if best.len() == 1 { Some(best[0].0) } else { None }
}

#[derive(Debug, Clone)]
pub(super) enum ResolvedOverloadSelection<'a> {
    Selected(ResolvedDirectCallCandidate<'a>),
    Ambiguous { applicable_count: usize },
    NotApplicable { runtime_generic_failures: Vec<(&'a Declaration, DirectCallResolutionFailure)> },
}

pub(super) fn resolve_overload_selection_from_attempts<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    attempts: Vec<(
        &'a Declaration,
        Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure>,
    )>,
) -> ResolvedOverloadSelection<'a> {
    let mut applicable = Vec::new();
    let mut runtime_generic_failures = Vec::new();

    for (declaration, attempt) in attempts {
        match attempt {
            Ok(candidate)
                if call_signature_params_are_applicable(
                    node,
                    nodes,
                    call,
                    &candidate.signature_params,
                ) =>
            {
                applicable.push(candidate);
            }
            Ok(_) => {}
            Err(failure) if declaration_has_runtime_generic_paramlist(declaration) => {
                runtime_generic_failures.push((declaration, failure));
            }
            Err(_) => {}
        }
    }

    match applicable.len() {
        0 => ResolvedOverloadSelection::NotApplicable { runtime_generic_failures },
        1 => ResolvedOverloadSelection::Selected(
            applicable.pop().expect("single applicable overload"),
        ),
        applicable_count => match select_most_specific_overload_index(node, nodes, &applicable) {
            Some(index) => ResolvedOverloadSelection::Selected(applicable.swap_remove(index)),
            None => ResolvedOverloadSelection::Ambiguous { applicable_count },
        },
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(super) fn overload_is_applicable(
    call: &typepython_binding::CallSite,
    declaration: &Declaration,
) -> bool {
    let node = typepython_graph::ModuleNode {
        module_path: std::path::PathBuf::from("<overload-test>"),
        module_key: String::new(),
        module_kind: SourceKind::Python,
        declarations: Vec::new(),
        member_accesses: Vec::new(),
        returns: Vec::new(),
        yields: Vec::new(),
        if_guards: Vec::new(),
        asserts: Vec::new(),
        invalidations: Vec::new(),
        matches: Vec::new(),
        for_loops: Vec::new(),
        with_statements: Vec::new(),
        except_handlers: Vec::new(),
        assignments: Vec::new(),
        summary_fingerprint: 0,
        calls: Vec::new(),
        method_calls: Vec::new(),
    };
    overload_is_applicable_with_context(&node, &[], call, declaration)
}

#[allow(dead_code)]
pub(super) fn overload_is_applicable_with_context(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    declaration: &Declaration,
) -> bool {
    resolve_direct_call_candidate(node, nodes, declaration, call).is_some_and(|candidate| {
        call_signature_params_are_applicable(node, nodes, call, &candidate.signature_params)
    })
}

pub(super) fn call_signature_params_are_applicable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    params: &[SemanticCallableParam],
) -> bool {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    let positional_params = params
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .collect::<Vec<_>>();
    let has_variadic = params.iter().any(|param| param.variadic);
    let starred_positional = resolved_starred_positional_expansions(node, nodes, call);
    let expected_positional_arg_types =
        expected_positional_arg_semantic_types_from_params(params, call.arg_count);
    let expected_keyword_arg_types =
        expected_keyword_arg_semantic_types_from_params(params, &call.keyword_names);
    if call.arg_values.iter().enumerate().any(|(index, metadata)| {
        resolve_contextual_call_arg_semantic_type_with_expected_semantic(
            &context,
            node,
            nodes,
            call.line,
            metadata,
            expected_positional_arg_types.get(index).and_then(|expected| expected.as_ref()),
        )
        .is_some_and(|result| !result.diagnostics.is_empty())
    }) {
        return false;
    }
    if call.keyword_arg_values.iter().enumerate().any(|(index, metadata)| {
        resolve_contextual_call_arg_semantic_type_with_expected_semantic(
            &context,
            node,
            nodes,
            call.line,
            metadata,
            expected_keyword_arg_types.get(index).and_then(|expected| expected.as_ref()),
        )
        .is_some_and(|result| !result.diagnostics.is_empty())
    }) {
        return false;
    }
    let resolved_keyword_arg_types = resolved_keyword_arg_semantic_types_with_expected_semantic(
        node,
        nodes,
        call,
        &expected_keyword_arg_types,
    )
    .into_iter()
    .map(|ty| (!matches!(&ty, SemanticType::Name(name) if name.is_empty())).then_some(ty))
    .collect::<Vec<_>>();
    let mut positional_types = resolved_call_arg_semantic_types_with_expected_semantic(
        node,
        nodes,
        call,
        &expected_positional_arg_types,
    )
    .into_iter()
    .map(|ty| (!matches!(&ty, SemanticType::Name(name) if name.is_empty())).then_some(ty))
    .collect::<Vec<_>>();
    let mut variadic_starred_types = Vec::new();
    for expansion in &starred_positional {
        match expansion {
            PositionalExpansion::Fixed(types) => positional_types.extend(types.clone()),
            PositionalExpansion::Variadic(element_type) => {
                variadic_starred_types.push(element_type.clone())
            }
        }
    }
    if !has_variadic
        && (positional_types.len() > positional_params.len() || !variadic_starred_types.is_empty())
    {
        return false;
    }
    let provided_keywords = call.keyword_names.iter().collect::<BTreeSet<_>>();
    let accepts_extra_keywords = params.iter().any(|param| param.keyword_variadic);
    let keyword_expansions = resolved_keyword_expansions(node, nodes, call);
    if call.keyword_names.iter().any(|keyword| {
        !params.iter().any(|param| param.name == **keyword && !param.positional_only)
            && !accepts_extra_keywords
    }) {
        return false;
    }
    if keyword_expansions.iter().any(|expansion| match expansion {
        KeywordExpansion::TypedDict(shape) => {
            (typed_dict_shape_has_unbounded_extra_keys(shape) && !accepts_extra_keywords)
                || shape.fields.keys().any(|key| {
                    !params.iter().any(|param| param.name == *key && !param.positional_only)
                        && !accepts_extra_keywords
                })
        }
        KeywordExpansion::Mapping(_) => !accepts_extra_keywords,
    }) {
        return false;
    }
    if keyword_duplicates_positional_arguments(call, params) {
        return false;
    }
    let positional_param_names =
        positional_params.iter().map(|param| param.name.as_str()).collect::<Vec<_>>();
    if keyword_expansions.iter().any(|expansion| match expansion {
        KeywordExpansion::TypedDict(shape) => shape.fields.keys().any(|key| {
            call.keyword_names.iter().any(|existing| existing == key)
                || positional_param_names
                    .iter()
                    .take(positional_types.len())
                    .any(|name| *name == key.as_str())
        }),
        KeywordExpansion::Mapping(_) => false,
    }) {
        return false;
    }
    if params.iter().enumerate().any(|(index, param)| {
        !param.has_default
            && if param.keyword_only {
                !provided_keywords.contains(&param.name)
                    && !keyword_expansions.iter().any(|expansion| match expansion {
                        KeywordExpansion::TypedDict(shape) => {
                            shape.fields.get(&param.name).is_some_and(|field| field.required)
                        }
                        KeywordExpansion::Mapping(_) => false,
                    })
            } else if param.variadic || param.keyword_variadic {
                false
            } else {
                index >= positional_types.len()
                    && (param.positional_only
                        || (!provided_keywords.contains(&param.name)
                            && !keyword_expansions.iter().any(|expansion| match expansion {
                                KeywordExpansion::TypedDict(shape) => shape
                                    .fields
                                    .get(&param.name)
                                    .is_some_and(|field| field.required),
                                KeywordExpansion::Mapping(_) => false,
                            })))
            }
    }) {
        return false;
    }

    let param_types = params.iter().map(|param| param.annotation.clone()).collect::<Vec<_>>();
    let variadic_type =
        params.iter().find(|param| param.variadic).and_then(|param| param.annotation.clone());
    let keyword_variadic_type = params
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| param.annotation.clone());
    let positional_ok =
        positional_types.iter().take(positional_params.len()).zip(param_types.iter()).all(
            |(arg_ty, param_ty)| match (arg_ty, param_ty) {
                (Some(arg_ty), Some(param_ty)) => {
                    semantic_type_is_assignable(node, nodes, param_ty, arg_ty)
                }
                _ => true,
            },
        ) && positional_types.iter().skip(positional_params.len()).all(|arg_ty| {
            let Some(param_ty) = variadic_type.as_ref() else {
                return false;
            };
            arg_ty
                .as_ref()
                .is_none_or(|arg_ty| semantic_type_is_assignable(node, nodes, param_ty, arg_ty))
        }) && variadic_starred_types.iter().all(|arg_ty| {
            let Some(param_ty) = variadic_type.as_ref() else {
                return false;
            };
            semantic_type_matches(node, nodes, param_ty, arg_ty)
        });
    let keyword_ok =
        call.keyword_names.iter().zip(&resolved_keyword_arg_types).all(|(keyword, arg_ty)| {
            let Some(index) = params.iter().position(|param| param.name == *keyword) else {
                let Some(param_ty) = keyword_variadic_type.as_ref() else {
                    return false;
                };
                return arg_ty.as_ref().is_none_or(|arg_ty| {
                    semantic_type_is_assignable(node, nodes, param_ty, arg_ty)
                });
            };
            let param_ty = param_types[index].as_ref();
            match (arg_ty.as_ref(), param_ty) {
                (Some(arg_ty), Some(param_ty)) => {
                    semantic_type_is_assignable(node, nodes, param_ty, arg_ty)
                }
                _ => true,
            }
        }) && keyword_expansions.iter().all(|expansion| match expansion {
            KeywordExpansion::TypedDict(shape) => shape.fields.iter().all(|(key, field)| {
                if let Some(index) = params.iter().position(|param| param.name == *key) {
                    let param = &params[index];
                    if param.positional_only {
                        return false;
                    }
                    if !field.required && !param.has_default {
                        return false;
                    }
                    let param_ty = param_types[index].as_ref();
                    let field_ty = (!field.value_type.is_empty())
                        .then(|| lower_type_text_or_name(&field.value_type));
                    return match (param_ty, field_ty.as_ref()) {
                        (Some(param_ty), Some(field_ty)) => {
                            semantic_type_matches(node, nodes, param_ty, field_ty)
                        }
                        _ => true,
                    };
                }
                let Some(param_ty) = keyword_variadic_type.as_ref() else {
                    return false;
                };
                let field_ty = (!field.value_type.is_empty())
                    .then(|| lower_type_text_or_name(&field.value_type));
                field_ty
                    .as_ref()
                    .is_none_or(|field_ty| semantic_type_matches(node, nodes, param_ty, field_ty))
            }),
            KeywordExpansion::Mapping(value_ty) => {
                let Some(param_ty) = keyword_variadic_type.as_ref() else {
                    return false;
                };
                semantic_type_matches(node, nodes, param_ty, value_ty)
            }
        });

    positional_ok && keyword_ok
}

fn expected_positional_arg_semantic_types_from_params(
    params: &[SemanticCallableParam],
    arg_count: usize,
) -> Vec<Option<SemanticType>> {
    let positional_params = params
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .collect::<Vec<_>>();
    let variadic_type =
        params.iter().find(|param| param.variadic).and_then(|param| param.annotation.clone());

    (0..arg_count)
        .map(|index| {
            positional_params
                .get(index)
                .and_then(|param| param.annotation.clone())
                .or_else(|| variadic_type.clone())
        })
        .collect()
}

fn expected_keyword_arg_semantic_types_from_params(
    params: &[SemanticCallableParam],
    keyword_names: &[String],
) -> Vec<Option<SemanticType>> {
    let keyword_variadic_type = params
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| param.annotation.clone());

    keyword_names
        .iter()
        .map(|keyword| {
            params
                .iter()
                .find(|param| param.name == *keyword && !param.positional_only)
                .and_then(|param| param.annotation.clone())
                .or_else(|| keyword_variadic_type.clone())
        })
        .collect()
}

#[derive(Debug, Clone)]
pub(super) enum PositionalExpansion {
    Fixed(Vec<Option<SemanticType>>),
    Variadic(SemanticType),
}

#[derive(Debug, Clone)]
pub(super) enum KeywordExpansion {
    TypedDict(TypedDictShape),
    Mapping(SemanticType),
}

pub(super) fn resolved_starred_positional_expansions(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
) -> Vec<PositionalExpansion> {
    let mut expansions = Vec::new();
    let count = call.starred_arg_values.len().max(call.starred_arg_types.len());
    for index in 0..count {
        let value_type = call
            .starred_arg_values
            .get(index)
            .and_then(|metadata| {
                resolve_direct_expression_semantic_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .or_else(|| {
                call.starred_arg_types
                    .get(index)
                    .and_then(|ty| (!ty.is_empty()).then(|| lower_type_text_or_name(ty)))
            });
        if let Some(expansion) = value_type.as_ref().and_then(parse_positional_expansion) {
            expansions.push(expansion);
        }
    }
    expansions
}

pub(super) fn parse_positional_expansion(value_type: &SemanticType) -> Option<PositionalExpansion> {
    let normalized = diagnostic_type_text(value_type);
    if normalized == "tuple[()]" {
        return Some(PositionalExpansion::Fixed(Vec::new()));
    }
    let (head, args) = value_type.generic_parts()?;
    match head {
        "tuple"
            if args.len() == 2 && matches!(&args[1], SemanticType::Name(name) if name == "...") =>
        {
            Some(PositionalExpansion::Variadic(args[0].clone()))
        }
        "tuple" => Some(PositionalExpansion::Fixed(
            expanded_tuple_shape_semantic_args(args).into_iter().map(Some).collect(),
        )),
        "list" | "Sequence" if args.len() == 1 => {
            Some(PositionalExpansion::Variadic(args[0].clone()))
        }
        _ => None,
    }
}

pub(super) fn resolved_keyword_expansions(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
) -> Vec<KeywordExpansion> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    resolved_keyword_expansions_with_context(&context, node, nodes, call)
}

pub(super) fn resolved_keyword_expansions_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
) -> Vec<KeywordExpansion> {
    let mut expansions = Vec::new();
    let count = call.keyword_expansion_values.len().max(call.keyword_expansion_types.len());
    for index in 0..count {
        let value_type = call
            .keyword_expansion_values
            .get(index)
            .and_then(|metadata| {
                resolve_direct_expression_semantic_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .or_else(|| {
                call.keyword_expansion_types
                    .get(index)
                    .and_then(|ty| (!ty.is_empty()).then(|| lower_type_text_or_name(ty)))
            });
        if let Some(expansion) = value_type
            .as_ref()
            .and_then(|value_type| parse_keyword_expansion(context, node, nodes, value_type))
        {
            expansions.push(expansion);
        }
    }
    expansions
}

pub(super) fn parse_keyword_expansion(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    value_type: &SemanticType,
) -> Option<KeywordExpansion> {
    let normalized = diagnostic_type_text(value_type);
    if let Some(shape) =
        resolve_known_typed_dict_shape_from_type_with_context(context, node, nodes, &normalized)
    {
        return Some(KeywordExpansion::TypedDict(shape));
    }
    let (head, args) = value_type.generic_parts()?;
    match head {
        "dict"
            if args.len() == 2 && matches!(&args[0], SemanticType::Name(name) if name == "str") =>
        {
            Some(KeywordExpansion::Mapping(args[1].clone()))
        }
        _ => None,
    }
}

pub(super) fn direct_unknown_operation_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for access in &node.member_accesses {
        if name_is_unknown_boundary(context, node, nodes, &access.owner_name) {
            diagnostics.push(Diagnostic::error(
                "TPY4003",
                format!(
                    "member access `{}` in module `{}` is unsupported because `{}` has type `unknown`",
                    access.member,
                    node.module_path.display(),
                    access.owner_name
                ),
            ));
        }
    }

    for call in &node.method_calls {
        if name_is_unknown_boundary(context, node, nodes, &call.owner_name) {
            diagnostics.push(Diagnostic::error(
                "TPY4003",
                format!(
                    "method call `{}.{}` in module `{}` is unsupported because `{}` has type `unknown`",
                    call.owner_name,
                    call.method,
                    node.module_path.display(),
                    call.owner_name
                ),
            ));
        }
    }

    for call in &node.calls {
        if plain_dataclass_field_specifier_call(context, node, &call.callee, call.line) {
            continue;
        }
        if name_is_unknown_boundary(context, node, nodes, &call.callee) {
            diagnostics.push(Diagnostic::error(
                "TPY4003",
                format!(
                    "call to `{}` in module `{}` is unsupported because `{}` has type `unknown`",
                    call.callee,
                    node.module_path.display(),
                    call.callee
                ),
            ));
        }
    }

    diagnostics
}

pub(super) fn plain_dataclass_field_specifier_call(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    _callee: &str,
    line: usize,
) -> bool {
    let info = context.load_dataclass_transform_module_info(node).unwrap_or_default();
    info.classes.iter().any(|class_site| {
        class_site
            .decorators
            .iter()
            .any(|decorator| matches!(decorator.as_str(), "dataclass" | "dataclasses.dataclass"))
            && class_site.fields.iter().any(|field| {
                field.line == line
                    && field
                        .field_specifier_name
                        .as_ref()
                        .is_some_and(|name| matches!(name.as_str(), "field" | "dataclasses.field"))
            })
    })
}

pub(super) fn conditional_return_coverage_diagnostics(
    _context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_conditional_return_sites(&source)
        .into_iter()
        .filter_map(|site| {
            let expected = normalize_type_text(&site.target_type);
            let expected_branches = union_branches(&expected).unwrap_or_else(|| vec![expected.clone()]);
            let covered = site
                .case_input_types
                .iter()
                .map(|case_type| normalize_type_text(case_type))
                .collect::<Vec<_>>();
            let missing = expected_branches
                .into_iter()
                .filter(|branch| {
                    !covered
                        .iter()
                        .any(|covered_branch| direct_type_matches(node, nodes, branch, covered_branch))
                })
                .collect::<Vec<_>>();
            (!missing.is_empty()).then(|| {
                Diagnostic::error(
                    "TPY4018",
                    format!(
                        "conditional return for `{}` in module `{}` does not cover parameter `{}`; missing: {}",
                        site.function_name,
                        node.module_path.display(),
                        site.target_name,
                        missing.join(", ")
                    ),
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    site.line,
                    1,
                    site.line,
                    1,
                ))
            })
        })
        .collect()
}

pub(super) fn name_is_unknown_boundary(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    name: &str,
) -> bool {
    if resolve_typing_callable_signature(name).is_some()
        || resolve_builtin_return_type(name).is_some()
        || matches!(name, "eval" | "exec" | "setattr" | "delattr")
        || resolve_direct_function(node, nodes, name).is_some()
        || resolve_direct_base(nodes, node, name).is_some()
        || resolve_module_level_assignment_reference_semantic_type(node, nodes, None, usize::MAX, name)
            .is_some()
        || node.declarations.iter().any(|declaration| {
            declaration.owner.is_none()
                && declaration.kind == DeclarationKind::Value
                && declaration.name == name
                && declaration_value_annotation_semantic_type(declaration).is_some_and(|annotation| {
                    !matches!(annotation.strip_annotated(), SemanticType::Name(name) if name == "unknown")
                })
        })
    {
        return false;
    }

    if resolve_direct_name_reference_semantic_type_with_context(
        context,
        node,
        nodes,
        None,
        None,
        None,
        None,
        usize::MAX,
        name,
    )
    .is_some_and(|resolved| {
        matches!(resolved.strip_annotated(), SemanticType::Name(name) if name == "unknown")
    })
    {
        return true;
    }

    if let Some((head, _)) = name.split_once('.')
        && unresolved_import_boundary_type_with_context(context, node, nodes, head)
            .is_some_and(|boundary| boundary == "unknown")
    {
        return true;
    }

    if resolve_imported_module_target(node, nodes, name).is_some() {
        return false;
    }

    unresolved_import_boundary_type_with_context(context, node, nodes, name)
        .is_some_and(|boundary| boundary == "unknown")
}

pub(super) fn unresolved_import_boundary_type_with_context<'a>(
    context: &'a CheckerContext<'_>,
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    local_name: &str,
) -> Option<&'static str> {
    if resolve_imported_symbol_semantic_target(node, nodes, local_name).is_some() {
        return None;
    }
    Some(context.import_fallback_type())
}

pub(super) fn resolve_direct_type_alias<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    name: &str,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    if let Some(local) = node.declarations.iter().find(|declaration| {
        declaration.name == name
            && declaration.owner.is_none()
            && declaration.kind == DeclarationKind::TypeAlias
    }) {
        return Some((node, local));
    }

    if let Some((module_key, symbol_name)) = name.rsplit_once('.') {
        if let Some(target_node) = nodes.iter().find(|candidate| candidate.module_key == module_key)
            && let Some(target_decl) = target_node.declarations.iter().find(|declaration| {
                declaration.name == symbol_name
                    && declaration.owner.is_none()
                    && declaration.kind == DeclarationKind::TypeAlias
            })
        {
            return Some((target_node, target_decl));
        }

        if let Some(import) = node.declarations.iter().find(|declaration| {
            declaration.kind == DeclarationKind::Import && declaration.name == module_key
        }) && let Some(import_target) =
            resolve_imported_symbol_semantic_target_from_declaration(nodes, import)
            && let Some(target_node) = import_target.module_target()
            && let Some(target_decl) = target_node.declarations.iter().find(|declaration| {
                declaration.name == symbol_name
                    && declaration.owner.is_none()
                    && declaration.kind == DeclarationKind::TypeAlias
            })
        {
            return Some((target_node, target_decl));
        }
    }

    resolve_imported_symbol_semantic_target(node, nodes, name)?.type_alias_provider()
}

pub(super) fn direct_return_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for return_site in &node.returns {
        let Some(target) = node.declarations.iter().find(|declaration| {
            declaration.name == return_site.owner_name
                && declaration.kind == DeclarationKind::Function
                && match (&return_site.owner_type_name, &declaration.owner) {
                    (Some(owner_type), Some(owner)) => owner.name == *owner_type,
                    (None, None) => true,
                    _ => false,
                }
        }) else {
            continue;
        };

        let Some(expected_type) = target.owner.as_ref().map_or_else(
            || declaration_signature_return_semantic_type(target),
            |owner| declaration_signature_return_semantic_type_with_self(target, &owner.name),
        ) else {
            continue;
        };
        let expected_type = rewrite_imported_typing_semantic_type(node, &expected_type);
        if matches!(expected_type.strip_annotated(), SemanticType::Name(name) if name.is_empty()) {
            continue;
        }
        let expected = diagnostic_type_text(&expected_type);

        let contextual = resolve_contextual_return_type(node, nodes, return_site, &expected);
        diagnostics.extend(contextual.diagnostics);
        let Some(actual) = contextual.actual_type else {
            continue;
        };
        let actual_text = diagnostic_type_text(&actual);

        if !semantic_type_is_assignable(node, nodes, &expected_type, &actual) {
            let diagnostic = Diagnostic::error(
                "TPY4001",
                match &return_site.owner_type_name {
                    Some(owner_type) => format!(
                        "type `{}` in module `{}` returns `{}` where member `{}` expects `{}`",
                        owner_type,
                        node.module_path.display(),
                        actual_text,
                        return_site.owner_name,
                        expected
                    ),
                    None => format!(
                        "function `{}` in module `{}` returns `{}` where `{}` expects `{}`",
                        return_site.owner_name,
                        node.module_path.display(),
                        actual_text,
                        return_site.owner_name,
                        expected
                    ),
                },
            )
            .with_span(Span::new(
                node.module_path.display().to_string(),
                return_site.line,
                1,
                return_site.line,
                1,
            ));
            let diagnostic =
                attach_type_mismatch_notes(diagnostic, node, nodes, &expected, &actual_text);
            let diagnostic = attach_return_inference_trace(
                diagnostic,
                node,
                nodes,
                return_site,
                &expected,
                &actual_text,
            );
            diagnostics.push(attach_missing_none_return_suggestion(
                diagnostic,
                node,
                nodes,
                return_site,
                &expected,
                &actual_text,
            ));
        }
    }

    diagnostics
}

pub(super) struct ContextualReturnTypeResult {
    pub(super) actual_type: Option<SemanticType>,
    pub(super) diagnostics: Vec<Diagnostic>,
}

pub(super) fn resolve_contextual_return_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected: &str,
) -> ContextualReturnTypeResult {
    let metadata = direct_expr_metadata_from_return_site(return_site);
    if let Some(lambda) = metadata.value_lambda.as_deref()
        && let Some(actual_type) = resolve_contextual_lambda_callable_semantic_type(
            node,
            nodes,
            None,
            None,
            return_site.line,
            lambda,
            Some(expected),
            None,
        )
    {
        return ContextualReturnTypeResult {
            actual_type: Some(actual_type),
            diagnostics: Vec::new(),
        };
    }
    if let Some(result) = resolve_contextual_typed_dict_literal_semantic_type_with_context(
        &CheckerContext::new(nodes, ImportFallback::Unknown, None),
        node,
        nodes,
        return_site.line,
        &metadata,
        Some(expected),
    ) {
        return ContextualReturnTypeResult {
            actual_type: Some(result.actual_type),
            diagnostics: result.diagnostics,
        };
    }
    if let Some(result) = resolve_contextual_collection_literal_semantic_type_in_scope_with_context(
        &CheckerContext::new(nodes, ImportFallback::Unknown, None),
        node,
        nodes,
        None,
        Some(return_site.owner_name.as_str()),
        return_site.owner_type_name.as_deref(),
        return_site.line,
        &metadata,
        Some(expected),
    ) {
        return ContextualReturnTypeResult {
            actual_type: Some(result.actual_type),
            diagnostics: result.diagnostics,
        };
    }
    ContextualReturnTypeResult {
        actual_type: return_site.value_metadata().as_ref().and_then(|metadata| {
            resolve_direct_expression_semantic_type_from_metadata(
                node,
                nodes,
                None,
                Some(return_site.owner_name.as_str()),
                return_site.owner_type_name.as_deref(),
                return_site.line,
                metadata,
            )
        }),
        diagnostics: Vec::new(),
    }
}

pub(super) fn direct_expr_metadata_from_return_site(
    return_site: &typepython_binding::ReturnSite,
) -> typepython_syntax::DirectExprMetadata {
    return_site.value_metadata().unwrap_or(typepython_syntax::DirectExprMetadata {
        value_type: None,
        value_type_expr: None,
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
    })
}

pub(super) fn direct_expr_metadata_from_yield_site(
    yield_site: &typepython_binding::YieldSite,
) -> typepython_syntax::DirectExprMetadata {
    yield_site.value_metadata().unwrap_or(typepython_syntax::DirectExprMetadata {
        value_type: None,
        value_type_expr: None,
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
    })
}

pub(super) struct ContextualYieldTypeResult {
    pub(super) actual_type: Option<SemanticType>,
    pub(super) diagnostics: Vec<Diagnostic>,
}

pub(super) fn resolve_contextual_yield_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    yield_site: &typepython_binding::YieldSite,
    expected: &str,
) -> ContextualYieldTypeResult {
    let metadata = direct_expr_metadata_from_yield_site(yield_site);
    if !yield_site.is_yield_from {
        if let Some(lambda) = metadata.value_lambda.as_deref()
            && let Some(actual_type) = resolve_contextual_lambda_callable_semantic_type(
                node,
                nodes,
                None,
                None,
                yield_site.line,
                lambda,
                Some(expected),
                None,
            )
        {
            return ContextualYieldTypeResult {
                actual_type: Some(actual_type),
                diagnostics: Vec::new(),
            };
        }
        if let Some(result) = resolve_contextual_typed_dict_literal_semantic_type_with_context(
            &CheckerContext::new(nodes, ImportFallback::Unknown, None),
            node,
            nodes,
            yield_site.line,
            &metadata,
            Some(expected),
        ) {
            return ContextualYieldTypeResult {
                actual_type: Some(result.actual_type),
                diagnostics: result.diagnostics,
            };
        }
        if let Some(result) =
            resolve_contextual_collection_literal_semantic_type_in_scope_with_context(
                &CheckerContext::new(nodes, ImportFallback::Unknown, None),
                node,
                nodes,
                None,
                Some(yield_site.owner_name.as_str()),
                yield_site.owner_type_name.as_deref(),
                yield_site.line,
                &metadata,
                Some(expected),
            )
        {
            return ContextualYieldTypeResult {
                actual_type: Some(result.actual_type),
                diagnostics: result.diagnostics,
            };
        }
    }
    ContextualYieldTypeResult {
        actual_type: resolve_direct_expression_semantic_type(
            node,
            nodes,
            None,
            None,
            Some(yield_site.owner_name.as_str()),
            yield_site.owner_type_name.as_deref(),
            yield_site.line,
        yield_site.value_metadata().as_ref().and_then(typepython_syntax::DirectExprMetadata::rendered_value_type).as_deref(),
            false,
            yield_site.value_callee.as_deref(),
            yield_site.value_name.as_deref(),
            yield_site.value_member_owner_name.as_deref(),
            yield_site.value_member_name.as_deref(),
            yield_site.value_member_through_instance,
            yield_site.value_method_owner_name.as_deref(),
            yield_site.value_method_name.as_deref(),
            yield_site.value_method_through_instance,
            yield_site.value_subscript_target.as_deref(),
            yield_site.value_subscript_string_key.as_deref(),
            yield_site.value_subscript_index.as_deref(),
            yield_site.value_if_true.as_deref(),
            yield_site.value_if_false.as_deref(),
            yield_site.value_if_guard.as_ref(),
            yield_site.value_bool_left.as_deref(),
            yield_site.value_bool_right.as_deref(),
            yield_site.value_binop_left.as_deref(),
            yield_site.value_binop_right.as_deref(),
            yield_site.value_binop_operator.as_deref(),
        ),
        diagnostics: Vec::new(),
    }
}

pub(super) fn direct_yield_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for yield_site in &node.yields {
        let target = node.declarations.iter().find(|declaration| {
            declaration.name == yield_site.owner_name
                && declaration.kind == DeclarationKind::Function
                && match (&yield_site.owner_type_name, &declaration.owner) {
                    (Some(owner_type_name), Some(owner)) => owner.name == *owner_type_name,
                    (None, None) => true,
                    _ => false,
                }
        });
        let Some(target) = target else {
            continue;
        };

        let Some(returns) = target.owner.as_ref().map_or_else(
            || declaration_signature_return_semantic_type(target),
            |owner| declaration_signature_return_semantic_type_with_self(target, &owner.name),
        ) else {
            continue;
        };
        let Some(expected_type) = unwrap_generator_yield_semantic_type(&returns) else {
            continue;
        };
        let expected_type = rewrite_imported_typing_semantic_type(node, &expected_type);
        let expected = diagnostic_type_text(&expected_type);
        let contextual = resolve_contextual_yield_type(node, nodes, yield_site, &expected);
        diagnostics.extend(contextual.diagnostics);
        let Some(actual) = contextual.actual_type else {
            continue;
        };

        let actual = if yield_site.is_yield_from {
            unwrap_yield_from_semantic_type(&actual).unwrap_or(actual)
        } else {
            actual
        };
        let actual_text = diagnostic_type_text(&actual);

        if !semantic_type_is_assignable(node, nodes, &expected_type, &actual) {
            diagnostics.push(Diagnostic::error(
                    "TPY4001",
                    match &yield_site.owner_type_name {
                        Some(owner_type_name) => format!(
                            "type `{}` in module `{}` yields `{}` where member `{}` expects `Generator[{}, ...]`",
                            owner_type_name,
                            node.module_path.display(),
                            actual_text,
                            yield_site.owner_name,
                            expected
                        ),
                        None => format!(
                            "function `{}` in module `{}` yields `{}` where `Generator[{}, ...]` expects `{}`",
                            yield_site.owner_name,
                            node.module_path.display(),
                            actual_text,
                            expected,
                            expected
                        ),
                    },
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    yield_site.line,
                    1,
                    yield_site.line,
                    1,
                ))
                );
        }
    }

    diagnostics
}

pub(super) fn for_loop_target_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.for_loops
        .iter()
        .filter(|for_loop| !for_loop.target_names.is_empty())
        .filter_map(|for_loop| {
            let iter_type = resolve_direct_expression_semantic_type(
                node,
                nodes,
                None,
                None,
                for_loop.owner_name.as_deref(),
                for_loop.owner_type_name.as_deref(),
                for_loop.line,
                for_loop.iter_type.as_deref(),
                for_loop.iter_is_awaited,
                for_loop.iter_callee.as_deref(),
                for_loop.iter_name.as_deref(),
                for_loop.iter_member_owner_name.as_deref(),
                for_loop.iter_member_name.as_deref(),
                for_loop.iter_member_through_instance,
                for_loop.iter_method_owner_name.as_deref(),
                for_loop.iter_method_name.as_deref(),
                for_loop.iter_method_through_instance,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )?;
            let element_type = unwrap_for_iterable_semantic_type(&iter_type)?;
            let tuple_elements = unpacked_fixed_tuple_semantic_elements(&element_type)?;

            (tuple_elements.len() != for_loop.target_names.len()).then(|| {
                Diagnostic::error(
                    "TPY4001",
                    match (&for_loop.owner_type_name, &for_loop.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` destructures `for` target `({})` with {} name(s) from tuple element type `{}` with {} element(s) in `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            for_loop.target_names.join(", "),
                            for_loop.target_names.len(),
                            render_semantic_type(&element_type),
                            tuple_elements.len(),
                            owner_name,
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` destructures `for` target `({})` with {} name(s) from tuple element type `{}` with {} element(s)",
                            owner_name,
                            node.module_path.display(),
                            for_loop.target_names.join(", "),
                            for_loop.target_names.len(),
                            render_semantic_type(&element_type),
                            tuple_elements.len(),
                        ),
                        _ => format!(
                            "module `{}` destructures `for` target `({})` with {} name(s) from tuple element type `{}` with {} element(s)",
                            node.module_path.display(),
                            for_loop.target_names.join(", "),
                            for_loop.target_names.len(),
                            render_semantic_type(&element_type),
                            tuple_elements.len(),
                        ),
                    },
                )
            })
        })
        .collect()
}

pub(super) fn destructuring_assignment_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.assignments
        .iter()
        .filter(|assignment| assignment.destructuring_index == Some(0))
        .filter_map(|assignment| {
            let target_names = assignment.destructuring_target_names.as_ref()?;
            let actual = resolve_direct_expression_semantic_type_from_metadata(
                node,
                nodes,
                None,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                assignment.value_metadata().as_ref()?,
            )?;
            let tuple_elements = unpacked_fixed_tuple_semantic_elements(&actual)?;
            (tuple_elements.len() != target_names.len()).then(|| {
                Diagnostic::error(
                    "TPY4001",
                    match (&assignment.owner_type_name, &assignment.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` destructures assignment target `({})` with {} name(s) from tuple type `{}` with {} element(s) in `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            target_names.join(", "),
                            target_names.len(),
                            render_semantic_type(&actual),
                            tuple_elements.len(),
                            owner_name,
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` destructures assignment target `({})` with {} name(s) from tuple type `{}` with {} element(s)",
                            owner_name,
                            node.module_path.display(),
                            target_names.join(", "),
                            target_names.len(),
                            render_semantic_type(&actual),
                            tuple_elements.len(),
                        ),
                        _ => format!(
                            "module `{}` destructures assignment target `({})` with {} name(s) from tuple type `{}` with {} element(s)",
                            node.module_path.display(),
                            target_names.join(", "),
                            target_names.len(),
                            render_semantic_type(&actual),
                            tuple_elements.len(),
                        ),
                    },
                )
            })
        })
        .collect()
}

pub(super) fn with_statement_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.with_statements
        .iter()
        .filter(|with_site| {
            resolve_with_target_semantic_type_for_signature(node, nodes, None, with_site).is_none()
        })
        .map(|with_site| {
                Diagnostic::error(
                    "TPY4001",
                    match (&with_site.owner_type_name, &with_site.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` uses `with` target `{}` with an expression that lacks compatible `__enter__`/`__exit__` members in `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            display_with_target_name(with_site),
                            owner_name,
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` uses `with` target `{}` with an expression that lacks compatible `__enter__`/`__exit__` members",
                            owner_name,
                            node.module_path.display(),
                            display_with_target_name(with_site),
                        ),
                        _ => format!(
                            "module `{}` uses `with` target `{}` with an expression that lacks compatible `__enter__`/`__exit__` members",
                            node.module_path.display(),
                            display_with_target_name(with_site),
                        ),
                    },
                )
        })
        .collect()
}

pub(super) fn display_with_target_name(with_site: &typepython_binding::WithSite) -> &str {
    with_site.target_name.as_deref().unwrap_or("<ignored>")
}

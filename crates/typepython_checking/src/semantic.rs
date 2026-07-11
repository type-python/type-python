use super::*;

use crate::diagnostic_type_text as render_semantic_type;

pub(super) fn implicit_dynamic_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<Diagnostic> {
    if !context.no_implicit_dynamic || node.module_kind != SourceKind::TypePython {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    if let Some(source_text) = context.load_source_text(node) {
        let metadata = typepython_syntax::collect_module_surface_metadata(&source_text);
        for signature in metadata.direct_function_signatures {
            diagnostics.extend(
                signature
                    .params
                    .iter()
                    .filter(|param| param.rendered_annotation().is_none())
                    .map(|param| {
                        implicit_dynamic_diagnostic(
                            node,
                            signature.line,
                            format!(
                                "parameter `{}` on function `{}` falls back to `dynamic`; add an explicit annotation or use `dynamic` explicitly",
                                param.name, signature.name
                            ),
                        )
                    }),
            );
        }
        for signature in metadata.direct_method_signatures {
            let skip_receiver = match signature.method_kind {
                typepython_syntax::MethodKind::Static => false,
                typepython_syntax::MethodKind::Instance
                | typepython_syntax::MethodKind::Class
                | typepython_syntax::MethodKind::Property
                | typepython_syntax::MethodKind::PropertySetter => true,
            };
            diagnostics.extend(
                signature
                    .params
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !skip_receiver || *index > 0)
                    .map(|(_, param)| param)
                    .filter(|param| param.rendered_annotation().is_none())
                    .map(|param| {
                        implicit_dynamic_diagnostic(
                            node,
                            signature.line,
                            format!(
                                "parameter `{}` on member `{}.{}` falls back to `dynamic`; add an explicit annotation or use `dynamic` explicitly",
                                param.name, signature.owner_type_name, signature.name
                            ),
                        )
                    }),
            );
        }
    }

    diagnostics.extend(implicit_dynamic_assignment_lambda_diagnostics(node));
    diagnostics.extend(implicit_dynamic_return_lambda_diagnostics(node));
    diagnostics.extend(implicit_dynamic_yield_lambda_diagnostics(node));
    diagnostics
}

fn implicit_dynamic_assignment_lambda_diagnostics(
    node: &typepython_graph::ModuleNode,
) -> Vec<Diagnostic> {
    node.assignments
        .iter()
        .filter_map(|assignment| {
            let lambda = assignment.value_lambda.as_deref()?;
            let expected = assignment.annotation_text();
            Some(implicit_dynamic_lambda_param_diagnostics(
                node,
                assignment.line,
                lambda,
                expected,
                format!("lambda assigned to `{}`", assignment.name),
            ))
        })
        .flatten()
        .collect()
}

fn implicit_dynamic_return_lambda_diagnostics(
    node: &typepython_graph::ModuleNode,
) -> Vec<Diagnostic> {
    node.returns
        .iter()
        .filter_map(|return_site| {
            let lambda = return_site.value_lambda.as_deref()?;
            let target = node.declarations.iter().find(|declaration| {
                declaration.name == return_site.owner_name
                    && declaration.kind == DeclarationKind::Function
                    && match (&return_site.owner_type_name, &declaration.owner) {
                        (Some(owner_type), Some(owner)) => owner.name == *owner_type,
                        (None, None) => true,
                        _ => false,
                    }
            })?;
            let expected = target
                .owner
                .as_ref()
                .map_or_else(
                    || declaration_signature_return_semantic_type(target),
                    |owner| {
                        declaration_signature_return_semantic_type_with_self(target, &owner.name)
                    },
                )
                .map(|ty| render_semantic_type(&rewrite_imported_typing_semantic_type(node, &ty)));
            Some(implicit_dynamic_lambda_param_diagnostics(
                node,
                return_site.line,
                lambda,
                expected.as_deref(),
                format!("lambda returned from `{}`", return_site.owner_name),
            ))
        })
        .flatten()
        .collect()
}

fn implicit_dynamic_yield_lambda_diagnostics(
    node: &typepython_graph::ModuleNode,
) -> Vec<Diagnostic> {
    node.yields
        .iter()
        .filter_map(|yield_site| {
            let lambda = yield_site.value_lambda.as_deref()?;
            let target = node.declarations.iter().find(|declaration| {
                declaration.name == yield_site.owner_name
                    && declaration.kind == DeclarationKind::Function
                    && match (&yield_site.owner_type_name, &declaration.owner) {
                        (Some(owner_type_name), Some(owner)) => owner.name == *owner_type_name,
                        (None, None) => true,
                        _ => false,
                    }
            })?;
            let expected = target
                .owner
                .as_ref()
                .map_or_else(
                    || declaration_signature_return_semantic_type(target),
                    |owner| {
                        declaration_signature_return_semantic_type_with_self(target, &owner.name)
                    },
                )
                .and_then(|returns| unwrap_generator_yield_semantic_type(&returns))
                .map(|ty| render_semantic_type(&rewrite_imported_typing_semantic_type(node, &ty)));
            Some(implicit_dynamic_lambda_param_diagnostics(
                node,
                yield_site.line,
                lambda,
                expected.as_deref(),
                format!("lambda yielded from `{}`", yield_site.owner_name),
            ))
        })
        .flatten()
        .collect()
}

fn implicit_dynamic_lambda_param_diagnostics(
    node: &typepython_graph::ModuleNode,
    line: usize,
    lambda: &typepython_syntax::LambdaMetadata,
    expected: Option<&str>,
    context_label: String,
) -> Vec<Diagnostic> {
    let expected_params = expected
        .and_then(parse_callable_annotation)
        .and_then(|(params, _)| params)
        .filter(|params| params.len() == lambda.params.len());

    lambda
        .params
        .iter()
        .enumerate()
        .filter(|(_, param)| param.rendered_annotation().is_none())
        .filter(|(index, _)| {
            expected_params.as_ref().is_none_or(|params| params.get(*index).is_none())
        })
        .map(|(_, param)| {
            implicit_dynamic_diagnostic(
                node,
                line,
                format!(
                    "parameter `{}` on {} falls back to `dynamic`; add a lambda parameter annotation, provide a concrete `Callable[...]` context, or use `dynamic` explicitly",
                    param.name, context_label
                ),
            )
        })
        .collect()
}

fn implicit_dynamic_diagnostic(
    node: &typepython_graph::ModuleNode,
    line: usize,
    message: String,
) -> Diagnostic {
    Diagnostic::error("TPY4029", message).with_span(Span::new(
        node.module_path.display().to_string(),
        line,
        1,
        line,
        1,
    ))
}

pub(super) fn unsafe_boundary_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    strict: bool,
    warn_unsafe: bool,
) -> Vec<Diagnostic> {
    if !strict || !warn_unsafe || node.module_kind != SourceKind::TypePython {
        return Vec::new();
    }
    context
        .load_unsafe_operation_sites(node)
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

pub(super) fn unsupported_framework_transform_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    strict: bool,
) -> Vec<Diagnostic> {
    if !strict
        || !context.framework_adapters_enabled()
        || node.module_kind != SourceKind::TypePython
    {
        return Vec::new();
    }

    context
        .load_framework_transform_module_info(node)
        .unwrap_or_default()
        .providers
        .into_iter()
        .filter(|provider| !framework_transform_provider_has_supported_semantics(provider))
        .map(|provider| {
            Diagnostic::error(
                "TPY4020",
                format!(
                    "framework transform provider `{}` in module `{}` does not advertise a supported static capability set",
                    provider.name,
                    node.module_path.display(),
                ),
            )
            .with_span(Span::new(
                node.module_path.display().to_string(),
                provider.line,
                1,
                provider.line,
                1,
            ))
        })
        .collect()
}

pub(super) fn dynamic_framework_alias_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    strict: bool,
) -> Vec<Diagnostic> {
    if !strict
        || !context.framework_adapters_enabled()
        || node.module_kind != SourceKind::TypePython
    {
        return Vec::new();
    }

    context
        .load_dataclass_transform_module_info(node)
        .unwrap_or_default()
        .classes
        .into_iter()
        .flat_map(|class_site| {
            class_site
                .fields
                .into_iter()
                .filter(|field| field.field_specifier_has_dynamic_alias)
                .map(move |field| {
                    Diagnostic::error(
                        "TPY4021",
                        format!(
                            "field `{}` on framework-transformed class `{}` uses a dynamic alias that prevents safe constructor typing",
                            field.name, class_site.name
                        ),
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        field.line,
                        1,
                        field.line,
                        1,
                    ))
                    .with_note("use a string-literal alias or omit the alias so TypePython can synthesize a checker-portable constructor")
                })
        })
        .collect()
}

pub(super) fn untyped_framework_field_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    strict: bool,
) -> Vec<Diagnostic> {
    if !strict
        || !context.framework_adapters_enabled()
        || node.module_kind != SourceKind::TypePython
    {
        return Vec::new();
    }

    context
        .load_dataclass_transform_module_info(node)
        .unwrap_or_default()
        .classes
        .into_iter()
        .filter(|class_site| {
            resolve_framework_transform_class_shape_with_context(context, node, nodes, &class_site.name)
                .is_some()
        })
        .flat_map(|class_site| {
            class_site.untyped_fields.into_iter().map(move |field| {
                Diagnostic::warning(
                    "TPY4024",
                    format!(
                        "field `{}` on framework-transformed class `{}` is missing a type annotation",
                        field.name, class_site.name
                    ),
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    field.line,
                    1,
                    field.line,
                    1,
                ))
                .with_note("add an explicit field annotation so TypePython can synthesize checker-portable constructors and stubs")
            })
        })
        .collect()
}

pub(super) fn ignored_lifecycle_result_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    strict: bool,
) -> Vec<Diagnostic> {
    if !strict || node.module_kind != SourceKind::TypePython {
        return Vec::new();
    }

    let marked_callables =
        visible_lifecycle_result_callables(context, node).into_iter().collect::<Vec<_>>();

    if marked_callables.is_empty() {
        return Vec::new();
    }

    node.calls
        .iter()
        .filter_map(|call| {
            let (_, obligation) = marked_callables.iter().find(|(name, _)| name == &call.callee)?;
            Some(
                Diagnostic::warning(
                    "TPY4022",
                    format!(
                        "result of `{}` call is ignored but `{}` requires the value to be {}",
                        call.callee, obligation.marker, obligation.action,
                    ),
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    call.line,
                    1,
                    call.line,
                    1,
                ))
                .with_note(obligation.note),
            )
        })
        .collect()
}

pub(super) fn unclosed_lifecycle_resource_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    strict: bool,
) -> Vec<Diagnostic> {
    if !strict || node.module_kind != SourceKind::TypePython {
        return Vec::new();
    }

    let marked_factories =
        visible_lifecycle_resource_factories(context, node).into_iter().collect::<Vec<_>>();

    if marked_factories.is_empty() {
        return Vec::new();
    }

    node.assignments
        .iter()
        .filter_map(|assignment| {
            let callee = assignment.value_callee.as_ref()?;
            let (_, obligation) = marked_factories.iter().find(|(name, _)| name == callee)?;
            if lifecycle_resource_is_satisfied(
                node,
                assignment.owner_name.as_deref(),
                &assignment.name,
            ) {
                return None;
            }
            Some(
                Diagnostic::warning(
                    "TPY4023",
                    format!(
                        "resource `{}` created by `{}` is not {} before scope exit",
                        assignment.name, callee, obligation.action,
                    ),
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    assignment.line,
                    1,
                    assignment.line,
                    1,
                ))
                .with_note(obligation.note),
            )
        })
        .collect()
}

pub(super) fn unsupported_dual_emit_async_construct_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<Diagnostic> {
    if !context.sync_async_dual_emit_enabled() || node.module_kind != SourceKind::TypePython {
        return Vec::new();
    }

    context
        .load_unsupported_dual_emit_async_construct_sites(node)
        .into_iter()
        .map(|site| {
            let construct = match site.kind {
                typepython_syntax::UnsupportedDualEmitAsyncConstructKind::AsyncFor => "async for",
                typepython_syntax::UnsupportedDualEmitAsyncConstructKind::AsyncWith => "async with",
            };
            Diagnostic::error(
                "TPY4025",
                format!(
                    "`@dual_emit` function `{}` contains unsupported `{}` syntax",
                    site.function_name, construct
                ),
            )
            .with_span(Span::new(
                node.module_path.display().to_string(),
                site.line,
                1,
                site.line,
                1,
            ))
            .with_note("split the implementation manually or avoid async context managers and async iteration until dual emit can lower them safely")
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct LifecycleObligation {
    marker: &'static str,
    action: &'static str,
    note: &'static str,
}

fn lifecycle_obligation(decorators: &[String]) -> Option<LifecycleObligation> {
    decorators
        .iter()
        .find_map(|decorator| lifecycle_obligation_from_marker(lifecycle_marker_name(decorator)))
}

fn lifecycle_resource_obligation(decorators: &[String]) -> Option<LifecycleObligation> {
    decorators.iter().find_map(|decorator| {
        lifecycle_resource_obligation_from_marker(lifecycle_marker_name(decorator))
    })
}

fn visible_lifecycle_result_callables(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<String, LifecycleObligation> {
    visible_lifecycle_facts(context, node, lifecycle_obligation, lifecycle_obligation_from_marker)
}

fn visible_lifecycle_resource_factories(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<String, LifecycleObligation> {
    visible_lifecycle_facts(
        context,
        node,
        lifecycle_resource_obligation,
        lifecycle_resource_obligation_from_marker,
    )
}

fn visible_lifecycle_facts(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    local_obligation: fn(&[String]) -> Option<LifecycleObligation>,
    summary_obligation: fn(&str) -> Option<LifecycleObligation>,
) -> BTreeMap<String, LifecycleObligation> {
    let mut facts = BTreeMap::new();
    if let Some(info) = context.load_decorator_transform_module_info(node) {
        for site in info.callables {
            if site.owner_type_name.is_some() {
                continue;
            }
            if let Some(obligation) = local_obligation(&site.decorators) {
                facts.insert(site.name, obligation);
            }
        }
    }
    facts.extend(imported_lifecycle_facts(context, node, summary_obligation));
    facts
}

fn imported_lifecycle_facts(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    summary_obligation: fn(&str) -> Option<LifecycleObligation>,
) -> BTreeMap<String, LifecycleObligation> {
    node.declarations
        .iter()
        .filter(|declaration| declaration.owner.is_none())
        .filter(|declaration| declaration.kind == DeclarationKind::Import)
        .filter_map(|declaration| {
            let target = resolve_imported_symbol_semantic_target_from_declaration(
                context.nodes,
                declaration,
            )?;
            let target_declaration = target.declaration_target()?;
            if target_declaration.owner.is_some()
                || target_declaration.kind != DeclarationKind::Function
            {
                return None;
            }
            let fact =
                collect_effect_summary_facts(context, target.provider_node).into_iter().find(
                    |fact| fact.owner_type_name.is_none() && fact.name == target_declaration.name,
                )?;
            let obligation = fact
                .sources
                .iter()
                .find_map(|source| summary_obligation(lifecycle_marker_name(source)))?;
            Some((declaration.name.clone(), obligation))
        })
        .collect()
}

fn lifecycle_marker_name(source: &str) -> &str {
    source.trim_start_matches('@').rsplit('.').next().unwrap_or(source)
}

fn lifecycle_obligation_from_marker(marker: &str) -> Option<LifecycleObligation> {
    match marker {
        "must_use" => Some(LifecycleObligation {
            marker: "@must_use",
            action: "used, assigned, returned, or passed onward",
            note: "assign the result, return it, pass it to another function, or bind it to `_` intentionally",
        }),
        "must_await" => Some(LifecycleObligation {
            marker: "@must_await",
            action: "awaited or returned from an async function",
            note: "await the result, return it from the async function, or bind it to `_` intentionally",
        }),
        _ => None,
    }
}

fn lifecycle_resource_obligation_from_marker(marker: &str) -> Option<LifecycleObligation> {
    match marker {
        "must_close" => Some(LifecycleObligation {
            marker: "@must_close",
            action: "closed",
            note: "call `.close()` on the resource, use it in a `with` block, return it, or pass it onward intentionally",
        }),
        "must_consume" => Some(LifecycleObligation {
            marker: "@must_consume",
            action: "consumed",
            note: "consume the stream, close it, return it, or pass it onward intentionally",
        }),
        _ => None,
    }
}

fn lifecycle_resource_is_satisfied(
    node: &typepython_graph::ModuleNode,
    owner_name: Option<&str>,
    name: &str,
) -> bool {
    node.method_calls.iter().any(|call| {
        call.current_owner_name.as_deref() == owner_name
            && call.owner_name == name
            && matches!(call.method.as_str(), "close" | "consume" | "read")
    }) || node.returns.iter().any(|return_site| {
        return_site.owner_name.as_str() == owner_name.unwrap_or_default()
            && return_site.value_name.as_deref() == Some(name)
    })
}

fn framework_transform_provider_has_supported_semantics(
    provider: &typepython_syntax::FrameworkTransformProviderSite,
) -> bool {
    match provider.provider_kind {
        Some(typepython_syntax::FrameworkTransformProviderKind::FunctionDecorator) => {
            provider.capabilities.contains(
                &typepython_syntax::FrameworkTransformCapability::FunctionToObjectReplacement,
            ) || provider.capabilities.iter().any(|capability| {
                matches!(
                    capability,
                    typepython_syntax::FrameworkTransformCapability::TaintSource
                        | typepython_syntax::FrameworkTransformCapability::TaintSink
                        | typepython_syntax::FrameworkTransformCapability::TaintSanitizer
                        | typepython_syntax::FrameworkTransformCapability::ValidatorWitness
                        | typepython_syntax::FrameworkTransformCapability::EffectUnsafe
                        | typepython_syntax::FrameworkTransformCapability::EffectIoFs
                        | typepython_syntax::FrameworkTransformCapability::EffectIoNet
                        | typepython_syntax::FrameworkTransformCapability::EffectIoProc
                        | typepython_syntax::FrameworkTransformCapability::EffectTime
                        | typepython_syntax::FrameworkTransformCapability::EffectRandom
                        | typepython_syntax::FrameworkTransformCapability::EffectRuntimeValidation
                        | typepython_syntax::FrameworkTransformCapability::EffectTaintSanitize
                )
            })
        }
        Some(
            typepython_syntax::FrameworkTransformProviderKind::ClassDecorator
            | typepython_syntax::FrameworkTransformProviderKind::BaseClass
            | typepython_syntax::FrameworkTransformProviderKind::Metaclass,
        ) => {
            provider
                .capabilities
                .contains(&typepython_syntax::FrameworkTransformCapability::FieldCollection)
                && provider.capabilities.contains(
                    &typepython_syntax::FrameworkTransformCapability::ConstructorGeneration,
                )
        }
        None => false,
    }
}

pub(super) fn ambiguous_overload_call_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let call_context_sites = context.load_direct_call_context_sites(node);
    node.calls
        .iter()
        .filter_map(|call| {
            let overloads = resolve_direct_overloads(node, nodes, &call.callee);
            if overloads.len() < 2 {
                return None;
            }

            let call_scope = call_context_sites
                .iter()
                .find(|site| {
                    site.matches_source_call(&call.callee, call.line, call.source_range)
                });
            match resolve_direct_overload_selection_in_scope(
                node,
                nodes,
                call,
                &overloads,
                call_scope.and_then(|site| site.owner_name.as_deref()),
                call_scope.and_then(|site| site.owner_type_name.as_deref()),
                context.assignability_options(),
            ) {
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

fn overload_is_more_specific_with_options(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    candidate: &ResolvedDirectCallCandidate<'_>,
    baseline: &ResolvedDirectCallCandidate<'_>,
    options: AssignabilityOptions,
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
            if candidate_param.rendered_annotation_text()
                != baseline_param.rendered_annotation_text()
            {
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
        if !semantic_type_is_assignable_with_options(
            node,
            nodes,
            baseline_param_type,
            candidate_param_type,
            options,
        ) {
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
    options: AssignabilityOptions,
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
                    || overload_is_more_specific_with_options(
                        node,
                        nodes,
                        candidate.1,
                        other,
                        options,
                    )
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

#[allow(dead_code)]
pub(super) fn resolve_overload_selection_from_attempts_with_options<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    attempts: Vec<(
        &'a Declaration,
        Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure>,
    )>,
    options: AssignabilityOptions,
) -> ResolvedOverloadSelection<'a> {
    resolve_overload_selection_from_attempts_in_scope_with_options(
        node, nodes, call, attempts, None, None, options,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_overload_selection_from_attempts_in_scope_with_options<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    attempts: Vec<(
        &'a Declaration,
        Result<ResolvedDirectCallCandidate<'a>, DirectCallResolutionFailure>,
    )>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    options: AssignabilityOptions,
) -> ResolvedOverloadSelection<'a> {
    let mut applicable = Vec::new();
    let mut runtime_generic_failures = Vec::new();

    for (declaration, attempt) in attempts {
        match attempt {
            Ok(candidate)
                if call_signature_params_are_applicable_in_scope_with_options(
                    node,
                    nodes,
                    call,
                    &candidate.signature_params,
                    current_owner_name,
                    current_owner_type_name,
                    options,
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
        1 => ResolvedOverloadSelection::Selected(applicable.swap_remove(0)),
        applicable_count => {
            match select_most_specific_overload_index(node, nodes, &applicable, options) {
                Some(index) => ResolvedOverloadSelection::Selected(applicable.swap_remove(index)),
                None => ResolvedOverloadSelection::Ambiguous { applicable_count },
            }
        }
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
    call_signature_params_are_applicable_with_options(
        node,
        nodes,
        call,
        params,
        AssignabilityOptions::default(),
    )
}

pub(super) fn call_signature_params_are_applicable_with_options(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    params: &[SemanticCallableParam],
    options: AssignabilityOptions,
) -> bool {
    call_signature_params_are_applicable_in_scope_with_options(
        node, nodes, call, params, None, None, options,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn call_signature_params_are_applicable_in_scope_with_options(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    params: &[SemanticCallableParam],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    options: AssignabilityOptions,
) -> bool {
    let context = checker_context_for_assignability_options(nodes, options);
    let positional_params = params
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .collect::<Vec<_>>();
    let has_variadic = params.iter().any(|param| param.variadic);
    let starred_positional = resolved_starred_positional_expansions_in_scope_with_options(
        node,
        nodes,
        call,
        current_owner_name,
        current_owner_type_name,
        options,
    );
    let expected_positional_arg_types =
        expected_positional_arg_semantic_types_from_params(params, call.arg_count);
    let expected_keyword_arg_types =
        expected_keyword_arg_semantic_types_from_params(params, &call.keyword_names);
    if call.arg_values.iter().enumerate().any(|(index, metadata)| {
        resolve_contextual_call_arg_semantic_type_with_expected_semantic_in_scope(
            &context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            call.line,
            metadata,
            expected_positional_arg_types.get(index).and_then(|expected| expected.as_ref()),
        )
        .is_some_and(|result| !result.diagnostics.is_empty())
    }) {
        return false;
    }
    if call.keyword_arg_values.iter().enumerate().any(|(index, metadata)| {
        resolve_contextual_call_arg_semantic_type_with_expected_semantic_in_scope(
            &context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            call.line,
            metadata,
            expected_keyword_arg_types.get(index).and_then(|expected| expected.as_ref()),
        )
        .is_some_and(|result| !result.diagnostics.is_empty())
    }) {
        return false;
    }
    let resolved_keyword_arg_types =
        resolved_keyword_arg_semantic_types_with_expected_semantic_in_scope_with_options(
            node,
            nodes,
            call,
            &expected_keyword_arg_types,
            current_owner_name,
            current_owner_type_name,
            options,
        )
        .into_iter()
        .map(|ty| (!matches!(&ty, SemanticType::Name(name) if name.is_empty())).then_some(ty))
        .collect::<Vec<_>>();
    let mut positional_types =
        resolved_call_arg_semantic_types_with_expected_semantic_in_scope_with_options(
            node,
            nodes,
            call,
            &expected_positional_arg_types,
            current_owner_name,
            current_owner_type_name,
            options,
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
    let keyword_expansions = resolved_keyword_expansions_in_scope_with_context(
        &context,
        node,
        nodes,
        call,
        current_owner_name,
        current_owner_type_name,
    );
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
                    semantic_type_is_assignable_with_options(node, nodes, param_ty, arg_ty, options)
                }
                _ => true,
            },
        ) && positional_types.iter().skip(positional_params.len()).all(|arg_ty| {
            let Some(param_ty) = variadic_type.as_ref() else {
                return false;
            };
            arg_ty.as_ref().is_none_or(|arg_ty| {
                semantic_type_is_assignable_with_options(node, nodes, param_ty, arg_ty, options)
            })
        }) && variadic_starred_types.iter().all(|arg_ty| {
            let Some(param_ty) = variadic_type.as_ref() else {
                return false;
            };
            semantic_type_matches_with_options(node, nodes, param_ty, arg_ty, options)
        });
    let keyword_ok =
        call.keyword_names.iter().zip(&resolved_keyword_arg_types).all(|(keyword, arg_ty)| {
            let Some(index) = params.iter().position(|param| param.name == *keyword) else {
                let Some(param_ty) = keyword_variadic_type.as_ref() else {
                    return false;
                };
                return arg_ty.as_ref().is_none_or(|arg_ty| {
                    semantic_type_is_assignable_with_options(node, nodes, param_ty, arg_ty, options)
                });
            };
            let param_ty = param_types[index].as_ref();
            match (arg_ty.as_ref(), param_ty) {
                (Some(arg_ty), Some(param_ty)) => {
                    semantic_type_is_assignable_with_options(node, nodes, param_ty, arg_ty, options)
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
                    let field_ty = field.semantic_value_type();
                    return match (param_ty, field_ty.as_ref()) {
                        (Some(param_ty), Some(field_ty)) => semantic_type_matches_with_options(
                            node, nodes, param_ty, field_ty, options,
                        ),
                        _ => true,
                    };
                }
                let Some(param_ty) = keyword_variadic_type.as_ref() else {
                    return false;
                };
                let field_ty = field.semantic_value_type();
                field_ty.as_ref().is_none_or(|field_ty| {
                    semantic_type_matches_with_options(node, nodes, param_ty, field_ty, options)
                })
            }),
            KeywordExpansion::Mapping(value_ty) => {
                let Some(param_ty) = keyword_variadic_type.as_ref() else {
                    return false;
                };
                semantic_type_matches_with_options(node, nodes, param_ty, value_ty, options)
            }
        });

    positional_ok && keyword_ok
}

pub(crate) fn checker_context_for_assignability_options<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    options: AssignabilityOptions,
) -> CheckerContext<'a> {
    CheckerContext::new_with_bound_surface_facts_and_options(
        nodes,
        None,
        None,
        CheckerOptions {
            strict_nulls: options.strict_nulls,
            experimental_taint: options.taint,
            experimental_framework_adapters: options.framework_adapters,
            ..CheckerOptions::permissive_test_default()
        },
    )
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

#[allow(dead_code)]
pub(super) fn resolved_starred_positional_expansions(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
) -> Vec<PositionalExpansion> {
    resolved_starred_positional_expansions_with_options(
        node,
        nodes,
        call,
        AssignabilityOptions::default(),
    )
}

pub(super) fn resolved_starred_positional_expansions_with_options(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    options: AssignabilityOptions,
) -> Vec<PositionalExpansion> {
    resolved_starred_positional_expansions_in_scope_with_options(
        node, nodes, call, None, None, options,
    )
}

pub(super) fn resolved_starred_positional_expansions_in_scope_with_options(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    options: AssignabilityOptions,
) -> Vec<PositionalExpansion> {
    let mut expansions = Vec::new();
    let starred_arg_types = call.starred_arg_type_texts();
    let count = call.starred_arg_values.len().max(starred_arg_types.len());
    for index in 0..count {
        let value_type = call
            .starred_arg_values
            .get(index)
            .and_then(|metadata| {
                resolve_direct_expression_semantic_type_from_metadata_with_options(
                    node,
                    nodes,
                    None,
                    current_owner_name,
                    current_owner_type_name,
                    call.line,
                    metadata,
                    options,
                )
            })
            .or_else(|| {
                starred_arg_types
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

#[allow(dead_code)]
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
    resolved_keyword_expansions_in_scope_with_context(context, node, nodes, call, None, None)
}

pub(super) fn resolved_keyword_expansions_in_scope_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
) -> Vec<KeywordExpansion> {
    let mut expansions = Vec::new();
    let keyword_expansion_types = call.keyword_expansion_type_texts();
    let count = call.keyword_expansion_values.len().max(keyword_expansion_types.len());
    for index in 0..count {
        let value_type = call
            .keyword_expansion_values
            .get(index)
            .and_then(|metadata| {
                resolve_direct_expression_semantic_type_from_metadata_with_options(
                    node,
                    nodes,
                    None,
                    current_owner_name,
                    current_owner_type_name,
                    call.line,
                    metadata,
                    context.assignability_options(),
                )
            })
            .or_else(|| {
                keyword_expansion_types
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
    let mut seen_expression_operations = std::collections::BTreeSet::new();

    for access in &node.member_accesses {
        if allowed_runtime_inspection_member_with_context(
            context,
            node,
            nodes,
            access.current_owner_name.as_deref(),
            access.current_owner_type_name.as_deref(),
            access.line,
            &access.owner_name,
            &access.member,
        ) {
            continue;
        }
        if name_is_unknown_boundary_with_context(
            context,
            node,
            nodes,
            access.current_owner_name.as_deref(),
            access.current_owner_type_name.as_deref(),
            access.line,
            &access.owner_name,
        ) {
            let key = format!("member:{}:{}.{}", access.line, access.owner_name, access.member);
            if !seen_expression_operations.insert(key) {
                continue;
            }
            let mut diagnostic = Diagnostic::error(
                "TPY4003",
                format!(
                    "member access `{}` in module `{}` is unsupported because `{}` has type `unknown`",
                    access.member,
                    node.module_path.display(),
                    access.owner_name
                ),
            );
            if let Some(note) = validator_witness_trust_boundary_note(
                context,
                node,
                nodes,
                access.current_owner_name.as_deref(),
                access.current_owner_type_name.as_deref(),
                access.line,
                &access.owner_name,
            ) {
                diagnostic = diagnostic.with_note(note);
            }
            diagnostics.push(diagnostic);
        }
    }

    for call in &node.method_calls {
        if name_is_unknown_boundary_with_context(
            context,
            node,
            nodes,
            call.current_owner_name.as_deref(),
            call.current_owner_type_name.as_deref(),
            call.line,
            &call.owner_name,
        ) {
            let key = format!("method:{}:{}.{}", call.line, call.owner_name, call.method);
            if !seen_expression_operations.insert(key) {
                continue;
            }
            let mut diagnostic = Diagnostic::error(
                "TPY4003",
                format!(
                    "method call `{}.{}` in module `{}` is unsupported because `{}` has type `unknown`",
                    call.owner_name,
                    call.method,
                    node.module_path.display(),
                    call.owner_name
                ),
            );
            if let Some(note) = validator_witness_trust_boundary_note(
                context,
                node,
                nodes,
                call.current_owner_name.as_deref(),
                call.current_owner_type_name.as_deref(),
                call.line,
                &call.owner_name,
            ) {
                diagnostic = diagnostic.with_note(note);
            }
            diagnostics.push(diagnostic);
        }
    }

    let direct_call_context_sites = context.load_direct_call_context_sites(node);
    if direct_call_context_sites.is_empty() {
        for call in &node.calls {
            if plain_dataclass_field_specifier_call(context, node, &call.callee, call.line) {
                continue;
            }
            if name_is_unknown_boundary(context, node, nodes, &call.callee) {
                push_unknown_direct_call_diagnostic(
                    &mut diagnostics,
                    &mut seen_expression_operations,
                    node,
                    call.line,
                    &call.callee,
                );
            }
        }
    } else {
        for call in &direct_call_context_sites {
            if plain_dataclass_field_specifier_call(context, node, &call.callee, call.line) {
                continue;
            }
            if name_is_unknown_boundary_with_context(
                context,
                node,
                nodes,
                call.owner_name.as_deref(),
                call.owner_type_name.as_deref(),
                call.line,
                &call.callee,
            ) {
                push_unknown_direct_call_diagnostic(
                    &mut diagnostics,
                    &mut seen_expression_operations,
                    node,
                    call.line,
                    &call.callee,
                );
            }
        }
    }

    for assignment in &node.assignments {
        if let Some(metadata) = assignment.value_metadata() {
            collect_unknown_direct_expression_operation_diagnostics(
                context,
                node,
                nodes,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                &metadata,
                &mut diagnostics,
                &mut seen_expression_operations,
            );
        }
    }
    for return_site in &node.returns {
        if let Some(metadata) = return_site.value_metadata() {
            collect_unknown_direct_expression_operation_diagnostics(
                context,
                node,
                nodes,
                Some(return_site.owner_name.as_str()),
                return_site.owner_type_name.as_deref(),
                return_site.line,
                &metadata,
                &mut diagnostics,
                &mut seen_expression_operations,
            );
        }
    }
    for yield_site in &node.yields {
        if let Some(metadata) = yield_site.value_metadata() {
            collect_unknown_direct_expression_operation_diagnostics(
                context,
                node,
                nodes,
                Some(yield_site.owner_name.as_str()),
                yield_site.owner_type_name.as_deref(),
                yield_site.line,
                &metadata,
                &mut diagnostics,
                &mut seen_expression_operations,
            );
        }
    }
    for call in &node.calls {
        let call_context = direct_call_context_sites
            .iter()
            .find(|site| site.matches_source_call(&call.callee, call.line, call.source_range));
        for metadata in call
            .arg_values
            .iter()
            .chain(call.starred_arg_values.iter())
            .chain(call.keyword_arg_values.iter())
            .chain(call.keyword_expansion_values.iter())
        {
            collect_unknown_direct_expression_operation_diagnostics(
                context,
                node,
                nodes,
                call_context.and_then(|site| site.owner_name.as_deref()),
                call_context.and_then(|site| site.owner_type_name.as_deref()),
                call.line,
                metadata,
                &mut diagnostics,
                &mut seen_expression_operations,
            );
        }
    }
    for call in &node.method_calls {
        for metadata in call
            .arg_values
            .iter()
            .chain(call.starred_arg_values.iter())
            .chain(call.keyword_arg_values.iter())
            .chain(call.keyword_expansion_values.iter())
        {
            collect_unknown_direct_expression_operation_diagnostics(
                context,
                node,
                nodes,
                call.current_owner_name.as_deref(),
                call.current_owner_type_name.as_deref(),
                call.line,
                metadata,
                &mut diagnostics,
                &mut seen_expression_operations,
            );
        }
    }
    for match_site in &node.matches {
        if let Some(metadata) = match_site.subject_metadata() {
            collect_unknown_direct_expression_operation_diagnostics(
                context,
                node,
                nodes,
                match_site.owner_name.as_deref(),
                match_site.owner_type_name.as_deref(),
                match_site.line,
                &metadata,
                &mut diagnostics,
                &mut seen_expression_operations,
            );
        }
    }
    for for_site in &node.for_loops {
        if let Some(metadata) = for_site.iter_metadata() {
            collect_unknown_direct_expression_operation_diagnostics(
                context,
                node,
                nodes,
                for_site.owner_name.as_deref(),
                for_site.owner_type_name.as_deref(),
                for_site.line,
                &metadata,
                &mut diagnostics,
                &mut seen_expression_operations,
            );
        }
    }
    for with_site in &node.with_statements {
        if let Some(metadata) = with_site.context_metadata() {
            collect_unknown_direct_expression_operation_diagnostics(
                context,
                node,
                nodes,
                with_site.owner_name.as_deref(),
                with_site.owner_type_name.as_deref(),
                with_site.line,
                &metadata,
                &mut diagnostics,
                &mut seen_expression_operations,
            );
        }
    }
    for site in context.load_frozen_field_mutation_sites(node) {
        if direct_expr_metadata_resolves_to_unknown(
            context,
            node,
            nodes,
            site.owner_name.as_deref(),
            site.owner_type_name.as_deref(),
            site.line,
            &site.target,
            &std::collections::BTreeSet::new(),
        ) {
            let label = direct_expr_operation_label(&site.target);
            let operation = match site.kind {
                typepython_syntax::FrozenFieldMutationKind::Assignment => "attribute assignment",
                typepython_syntax::FrozenFieldMutationKind::AugmentedAssignment => {
                    "augmented attribute assignment"
                }
                typepython_syntax::FrozenFieldMutationKind::Delete => "attribute deletion",
            };
            push_unique_unknown_operation_diagnostic(
                &mut diagnostics,
                &mut seen_expression_operations,
                format!("member:{}:{}.{}", site.line, label, site.field_name),
                format!(
                    "{} `{}.{}` in module `{}` is unsupported because `{}` has type `unknown`",
                    operation,
                    label,
                    site.field_name,
                    node.module_path.display(),
                    label,
                ),
            );
        }
    }
    for expression_site in context.source_facts.expression_use_sites(node) {
        let suppressed_names = expression_site
            .suppressed_names
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
            context,
            node,
            nodes,
            expression_site.owner_name.as_deref(),
            expression_site.owner_type_name.as_deref(),
            expression_site.line,
            &expression_site.value,
            &suppressed_names,
            &mut diagnostics,
            &mut seen_expression_operations,
        );
    }

    diagnostics
}

#[allow(clippy::too_many_arguments)]
fn collect_unknown_direct_expression_operation_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    diagnostics: &mut Vec<Diagnostic>,
    seen: &mut std::collections::BTreeSet<String>,
) {
    let suppressed_names = std::collections::BTreeSet::new();
    collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
        context,
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        line,
        metadata,
        &suppressed_names,
        diagnostics,
        seen,
    );
}

#[allow(clippy::too_many_arguments)]
fn collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    suppressed_names: &std::collections::BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
    seen: &mut std::collections::BTreeSet<String>,
) {
    if let Some(callee) = metadata.value_callee.as_deref()
        && !plain_dataclass_field_specifier_call(context, node, callee, line)
        && name_is_unknown_boundary_with_context(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            callee,
        )
    {
        push_unknown_direct_call_diagnostic(diagnostics, seen, node, line, callee);
    }

    if let Some(owner_name) = metadata.value_member_owner_name.as_deref()
        && let Some(member_name) = metadata.value_member_name.as_deref()
        && !allowed_runtime_inspection_member_with_context(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            owner_name,
            member_name,
        )
        && direct_operation_owner_resolves_to_unknown(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            owner_name,
            metadata.value_member_through_instance,
            suppressed_names,
        )
    {
        let owner_label =
            direct_owner_operation_label(owner_name, metadata.value_member_through_instance);
        push_unique_unknown_operation_diagnostic(
            diagnostics,
            seen,
            format!("member:{line}:{owner_name}.{member_name}"),
            format!(
                "member access `{}` in module `{}` is unsupported because `{}` has type `unknown`",
                member_name,
                node.module_path.display(),
                owner_label,
            ),
        );
    }

    if let Some(owner_name) = metadata.value_method_owner_name.as_deref()
        && let Some(method_name) = metadata.value_method_name.as_deref()
    {
        let owner_label =
            direct_owner_operation_label(owner_name, metadata.value_method_through_instance);
        if direct_operation_owner_resolves_to_unknown(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            owner_name,
            metadata.value_method_through_instance,
            suppressed_names,
        ) {
            push_unique_unknown_operation_diagnostic(
                diagnostics,
                seen,
                format!("method:{line}:{owner_name}.{method_name}"),
                format!(
                    "method call `{}.{}` in module `{}` is unsupported because `{}` has type `unknown`",
                    owner_label,
                    method_name,
                    node.module_path.display(),
                    owner_label,
                ),
            );
        } else if !suppressed_names.contains(owner_name)
            && resolve_direct_member_reference_semantic_type_with_options(
                node,
                nodes,
                None,
                None,
                current_owner_name,
                current_owner_type_name,
                line,
                owner_name,
                method_name,
                metadata.value_method_through_instance,
                context.assignability_options(),
            )
            .is_some_and(|resolved| semantic_type_is_unknown(&resolved))
        {
            push_unique_unknown_operation_diagnostic(
                diagnostics,
                seen,
                format!("call:{line}:{owner_name}.{method_name}"),
                format!(
                    "call to `{}.{}` in module `{}` is unsupported because `{}.{}` has type `unknown`",
                    owner_label,
                    method_name,
                    node.module_path.display(),
                    owner_label,
                    method_name,
                ),
            );
        }
    }

    if let Some(target) = metadata.value_subscript_target.as_deref() {
        if direct_expr_metadata_resolves_to_unknown(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            target,
            suppressed_names,
        ) {
            let label = direct_expr_operation_label(target);
            push_unique_unknown_operation_diagnostic(
                diagnostics,
                seen,
                format!("subscript:{line}:{label}"),
                format!(
                    "subscript access in module `{}` is unsupported because `{}` has type `unknown`",
                    node.module_path.display(),
                    label,
                ),
            );
        }
        collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            target,
            suppressed_names,
            diagnostics,
            seen,
        );
    }

    if let Some(operator) = metadata.value_binop_operator.as_deref()
        && let Some((operation, member_name)) = direct_expr_member_operation(operator)
        && let Some(owner) = metadata.value_binop_left.as_deref()
    {
        if direct_expr_metadata_resolves_to_unknown(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            owner,
            suppressed_names,
        ) {
            let owner_label = direct_expr_operation_label(owner);
            let (key_prefix, message) = match operation {
                DirectExprMemberOperation::MemberAccess => (
                    "member",
                    format!(
                        "member access `{}` in module `{}` is unsupported because `{}` has type `unknown`",
                        member_name,
                        node.module_path.display(),
                        owner_label,
                    ),
                ),
                DirectExprMemberOperation::MethodCall => (
                    "method",
                    format!(
                        "method call `{}.{}` in module `{}` is unsupported because `{}` has type `unknown`",
                        owner_label,
                        member_name,
                        node.module_path.display(),
                        owner_label,
                    ),
                ),
            };
            push_unique_unknown_operation_diagnostic(
                diagnostics,
                seen,
                format!("{key_prefix}:{line}:{owner_label}.{member_name}"),
                message,
            );
        } else if operation == DirectExprMemberOperation::MethodCall
            && resolve_direct_expression_semantic_type_from_metadata_with_options(
                node,
                nodes,
                None,
                current_owner_name,
                current_owner_type_name,
                line,
                owner,
                context.assignability_options(),
            )
            .and_then(|owner_type| {
                resolve_member_semantic_type_on_owner_type(
                    node,
                    nodes,
                    &owner_type,
                    member_name,
                    context.assignability_options(),
                )
            })
            .is_some_and(|resolved| semantic_type_is_unknown(&resolved))
        {
            let owner_label = direct_expr_operation_label(owner);
            push_unique_unknown_operation_diagnostic(
                diagnostics,
                seen,
                format!("call:{line}:{owner_label}.{member_name}"),
                format!(
                    "call to `{}.{}` in module `{}` is unsupported because `{}.{}` has type `unknown`",
                    owner_label,
                    member_name,
                    node.module_path.display(),
                    owner_label,
                    member_name,
                ),
            );
        }
    }

    if let Some(operator) = metadata.value_binop_operator.as_deref()
        && operator == DIRECT_CALL_OPERATOR
        && let Some(callee) = metadata.value_binop_left.as_deref()
        && direct_expr_metadata_resolves_to_unknown(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            callee,
            suppressed_names,
        )
    {
        let label = direct_expr_operation_label(callee);
        push_unique_unknown_operation_diagnostic(
            diagnostics,
            seen,
            format!("call:{line}:{label}"),
            format!(
                "call to `{}` in module `{}` is unsupported because `{}` has type `unknown`",
                label,
                node.module_path.display(),
                label,
            ),
        );
    }

    if let Some(operator) = metadata.value_binop_operator.as_deref()
        && direct_expr_member_operation(operator).is_none()
        && operator != DIRECT_CALL_OPERATOR
    {
        let single_operand_operation = direct_expr_operator_is_single_operand(operator)
            || metadata.value_binop_right.is_none();
        for (side, operand) in direct_expr_operator_operands(operator, metadata) {
            if let Some(operand) = operand
                && direct_expr_metadata_resolves_to_unknown(
                    context,
                    node,
                    nodes,
                    current_owner_name,
                    current_owner_type_name,
                    line,
                    operand,
                    suppressed_names,
                )
            {
                let label = direct_expr_operation_label(operand);
                push_unique_unknown_operation_diagnostic(
                    diagnostics,
                    seen,
                    format!("binop:{line}:{operator}:{side}:{label}"),
                    format!(
                        "{} in module `{}` is unsupported because the {} `{}` has type `unknown`",
                        direct_expr_operation_description(operator, single_operand_operation),
                        node.module_path.display(),
                        side,
                        label,
                    ),
                );
            }
        }
    }

    for child in [
        metadata.value_if_true.as_deref(),
        metadata.value_if_false.as_deref(),
        metadata.value_bool_left.as_deref(),
        metadata.value_bool_right.as_deref(),
        metadata.value_binop_left.as_deref(),
        metadata.value_binop_right.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            child,
            suppressed_names,
            diagnostics,
            seen,
        );
    }

    if let Some(lambda) = metadata.value_lambda.as_deref() {
        collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            &lambda.body,
            suppressed_names,
            diagnostics,
            seen,
        );
    }
    for comprehension in [
        metadata.value_list_comprehension.as_deref(),
        metadata.value_generator_comprehension.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let mut comprehension_suppressed_names = suppressed_names.clone();
        for clause in &comprehension.clauses {
            collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
                context,
                node,
                nodes,
                current_owner_name,
                current_owner_type_name,
                line,
                &clause.iter,
                suppressed_names,
                diagnostics,
                seen,
            );
            comprehension_suppressed_names.insert(clause.target_name.clone());
            comprehension_suppressed_names.extend(clause.target_names.iter().cloned());
        }
        if let Some(key) = comprehension.key.as_deref() {
            collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
                context,
                node,
                nodes,
                current_owner_name,
                current_owner_type_name,
                line,
                key,
                &comprehension_suppressed_names,
                diagnostics,
                seen,
            );
        }
        collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            &comprehension.element,
            &comprehension_suppressed_names,
            diagnostics,
            seen,
        );
    }
    for element in metadata
        .value_list_elements
        .iter()
        .flatten()
        .chain(metadata.value_set_elements.iter().flatten())
    {
        collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            element,
            suppressed_names,
            diagnostics,
            seen,
        );
    }
    if let Some(entries) = metadata.value_dict_entries.as_ref() {
        for entry in entries {
            if let Some(key) = entry.key_value.as_deref() {
                collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
                    context,
                    node,
                    nodes,
                    current_owner_name,
                    current_owner_type_name,
                    line,
                    key,
                    suppressed_names,
                    diagnostics,
                    seen,
                );
            }
            collect_unknown_direct_expression_operation_diagnostics_with_suppressed(
                context,
                node,
                nodes,
                current_owner_name,
                current_owner_type_name,
                line,
                &entry.value,
                suppressed_names,
                diagnostics,
                seen,
            );
        }
    }
}

fn direct_expr_operator_operands<'a>(
    operator: &str,
    metadata: &'a typepython_syntax::DirectExprMetadata,
) -> Vec<(&'static str, Option<&'a typepython_syntax::DirectExprMetadata>)> {
    if direct_expr_operator_is_single_operand(operator) || metadata.value_binop_right.is_none() {
        vec![("operand", metadata.value_binop_left.as_deref())]
    } else {
        vec![
            ("left operand", metadata.value_binop_left.as_deref()),
            ("right operand", metadata.value_binop_right.as_deref()),
        ]
    }
}

fn direct_expr_operator_is_single_operand(operator: &str) -> bool {
    matches!(operator, "truthiness" | "not" | "~")
}

fn direct_expr_operation_description(operator: &str, single_operand_operation: bool) -> String {
    match operator {
        "truthiness" => String::from("truthiness check"),
        "and" | "or" => format!("boolean operation `{operator}`"),
        "not" | "~" => format!("unary operation `{operator}`"),
        "==" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "not in" => {
            format!("comparison `{operator}`")
        }
        "+" | "-" if single_operand_operation => format!("unary operation `{operator}`"),
        _ => format!("binary operation `{operator}`"),
    }
}

pub(crate) const DIRECT_CALL_OPERATOR: &str = "direct-call";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum DirectExprMemberOperation {
    MemberAccess,
    MethodCall,
}

pub(crate) fn direct_expr_member_operation(
    operator: &str,
) -> Option<(DirectExprMemberOperation, &str)> {
    operator
        .strip_prefix("member-access:")
        .map(|member| (DirectExprMemberOperation::MemberAccess, member))
        .or_else(|| {
            operator
                .strip_prefix("method-call:")
                .map(|method| (DirectExprMemberOperation::MethodCall, method))
        })
}

#[allow(clippy::too_many_arguments)]
fn allowed_runtime_inspection_member_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    line: usize,
    owner_name: &str,
    member_name: &str,
) -> bool {
    if !matches!(member_name, "version_info" | "platform") {
        return false;
    }
    if name_has_contextual_local_binding(
        context,
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        line,
        owner_name,
    ) || resolve_module_value_binding_semantic_type(context, node, nodes, line, owner_name)
        .is_some()
    {
        return false;
    }
    name_resolves_to_sys_module_import(node, owner_name)
}

fn name_resolves_to_sys_module_import(node: &typepython_graph::ModuleNode, name: &str) -> bool {
    node.declarations.iter().any(|declaration| {
        declaration.owner.is_none()
            && declaration.name == name
            && declaration.kind == DeclarationKind::Import
            && declaration_import_target_ref(declaration).is_some_and(|target| {
                target.module_target == "sys" && target.symbol_target.is_none()
            })
    })
}

#[allow(clippy::too_many_arguments)]
fn direct_operation_owner_resolves_to_unknown(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    line: usize,
    owner_name: &str,
    through_instance: bool,
    suppressed_names: &std::collections::BTreeSet<String>,
) -> bool {
    if suppressed_names.contains(owner_name) {
        return false;
    }
    if through_instance {
        return resolve_direct_call_result_semantic_type_with_context(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            None,
            owner_name,
        )
        .is_some_and(|resolved| semantic_type_is_unknown(&resolved));
    }
    name_is_unknown_boundary_with_context(
        context,
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        line,
        owner_name,
    ) || resolve_direct_name_reference_semantic_type_with_context(
        context,
        node,
        nodes,
        None,
        None,
        current_owner_name,
        current_owner_type_name,
        line,
        owner_name,
    )
    .is_some_and(|resolved| semantic_type_is_unknown(&resolved))
}

#[allow(clippy::too_many_arguments)]
fn resolve_direct_call_result_semantic_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    line: usize,
    call_source_range: Option<typepython_syntax::SourceRange>,
    callee: &str,
) -> Option<SemanticType> {
    let has_contextual_local_binding = name_has_contextual_local_binding(
        context,
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        line,
        callee,
    );
    if has_contextual_local_binding {
        let callable = resolve_direct_name_reference_semantic_type_with_context(
            context,
            node,
            nodes,
            None,
            None,
            current_owner_name,
            current_owner_type_name,
            line,
            callee,
        )
        .or_else(|| {
            source_scope_param_semantic_type_with_context(
                context,
                node,
                current_owner_name,
                current_owner_type_name,
                callee,
            )
        })?;
        return callable.callable_parts().map(|(_, return_type)| return_type.clone());
    }

    if let Some(callable) = resolve_module_level_assignment_reference_semantic_type_with_options(
        node,
        nodes,
        None,
        line,
        callee,
        context.assignability_options(),
    ) {
        return callable.callable_parts().map(|(_, return_type)| return_type.clone());
    }

    resolve_direct_callable_return_semantic_type_for_call_with_context(
        context,
        node,
        nodes,
        callee,
        line,
        call_source_range,
    )
    .or_else(|| resolve_direct_callable_return_semantic_type(node, nodes, callee))
}

#[allow(clippy::too_many_arguments)]
fn direct_expr_metadata_resolves_to_unknown(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    suppressed_names: &std::collections::BTreeSet<String>,
) -> bool {
    if let Some(name) = metadata.value_name.as_deref() {
        if suppressed_names.contains(name) {
            return false;
        }
        return name_is_unknown_boundary_with_context(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            name,
        ) || resolve_direct_name_reference_semantic_type_with_context(
            context,
            node,
            nodes,
            None,
            None,
            current_owner_name,
            current_owner_type_name,
            line,
            name,
        )
        .is_some_and(|resolved| semantic_type_is_unknown(&resolved));
    }
    if let Some(callee) = metadata.value_callee.as_deref() {
        return resolve_direct_call_result_semantic_type_with_context(
            context,
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            line,
            metadata.call_source_range,
            callee,
        )
        .is_some_and(|resolved| semantic_type_is_unknown(&resolved))
            || metadata.rendered_value_type().is_some_and(|rendered| {
                semantic_type_is_unknown(&lower_type_text_or_name(&rendered))
            });
    }
    if let Some(owner_name) = metadata.value_member_owner_name.as_deref()
        && let Some(member_name) = metadata.value_member_name.as_deref()
    {
        return resolve_direct_member_reference_semantic_type_with_options(
            node,
            nodes,
            None,
            None,
            current_owner_name,
            current_owner_type_name,
            line,
            owner_name,
            member_name,
            metadata.value_member_through_instance,
            context.assignability_options(),
        )
        .is_some_and(|resolved| semantic_type_is_unknown(&resolved));
    }
    if let Some(owner_name) = metadata.value_method_owner_name.as_deref()
        && let Some(method_name) = metadata.value_method_name.as_deref()
    {
        return resolve_direct_method_return_semantic_type(
            node,
            nodes,
            None,
            None,
            current_owner_name,
            current_owner_type_name,
            line,
            metadata.call_source_range,
            owner_name,
            method_name,
            metadata.value_method_through_instance,
            context.assignability_options(),
        )
        .is_some_and(|resolved| semantic_type_is_unknown(&resolved));
    }
    if metadata.value_subscript_target.is_some()
        && let Some(resolved) = resolve_direct_expression_semantic_type_from_metadata_with_options(
            node,
            nodes,
            None,
            current_owner_name,
            current_owner_type_name,
            line,
            metadata,
            context.assignability_options(),
        )
    {
        return semantic_type_is_unknown(&resolved);
    }
    if let Some(operator) = metadata.value_binop_operator.as_deref()
        && (direct_expr_member_operation(operator).is_some() || operator == DIRECT_CALL_OPERATOR)
        && metadata.value_binop_left.is_some()
        && let Some(resolved) = resolve_direct_expression_semantic_type_from_metadata_with_options(
            node,
            nodes,
            None,
            current_owner_name,
            current_owner_type_name,
            line,
            metadata,
            context.assignability_options(),
        )
    {
        return semantic_type_is_unknown(&resolved);
    }
    if let Some(rendered) = metadata.rendered_value_type() {
        return semantic_type_is_unknown(&lower_type_text_or_name(&rendered));
    }
    false
}

fn semantic_type_is_unknown(ty: &SemanticType) -> bool {
    matches!(ty.strip_annotated(), SemanticType::Name(name) if name == "unknown")
}

fn direct_owner_operation_label(owner_name: &str, through_instance: bool) -> String {
    if through_instance { format!("{owner_name}()") } else { owner_name.to_owned() }
}

fn direct_expr_operation_label(metadata: &typepython_syntax::DirectExprMetadata) -> String {
    if let Some(name) = metadata.value_name.as_deref() {
        return name.to_owned();
    }
    if let Some(callee) = metadata.value_callee.as_deref() {
        return format!("{callee}()");
    }
    if let Some(owner) = metadata.value_member_owner_name.as_deref()
        && let Some(member) = metadata.value_member_name.as_deref()
    {
        let owner = direct_owner_operation_label(owner, metadata.value_member_through_instance);
        return format!("{owner}.{member}");
    }
    if let Some(owner) = metadata.value_method_owner_name.as_deref()
        && let Some(method) = metadata.value_method_name.as_deref()
    {
        let owner = direct_owner_operation_label(owner, metadata.value_method_through_instance);
        return format!("{owner}.{method}()");
    }
    if let Some(target) = metadata.value_subscript_target.as_deref() {
        let target_label = direct_expr_operation_label(target);
        if let Some(index) = metadata.value_subscript_index.as_deref() {
            return format!("{target_label}[{index}]");
        }
        if let Some(key) = metadata.value_subscript_string_key.as_deref() {
            return format!("{target_label}[{key:?}]");
        }
        return format!("{target_label}[...]");
    }
    if let Some(operator) = metadata.value_binop_operator.as_deref()
        && let Some((operation, member_name)) = direct_expr_member_operation(operator)
        && let Some(owner) = metadata.value_binop_left.as_deref()
    {
        let owner_label = direct_expr_operation_label(owner);
        return match operation {
            DirectExprMemberOperation::MemberAccess => format!("{owner_label}.{member_name}"),
            DirectExprMemberOperation::MethodCall => format!("{owner_label}.{member_name}()"),
        };
    }
    if let Some(operator) = metadata.value_binop_operator.as_deref()
        && operator == DIRECT_CALL_OPERATOR
        && let Some(callee) = metadata.value_binop_left.as_deref()
    {
        return format!("{}()", direct_expr_operation_label(callee));
    }
    String::from("expression")
}

fn push_unique_unknown_operation_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    seen: &mut std::collections::BTreeSet<String>,
    key: String,
    message: String,
) {
    if seen.insert(key) {
        diagnostics.push(Diagnostic::error("TPY4003", message));
    }
}

fn push_unknown_direct_call_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    seen: &mut std::collections::BTreeSet<String>,
    node: &typepython_graph::ModuleNode,
    line: usize,
    callee: &str,
) {
    push_unique_unknown_operation_diagnostic(
        diagnostics,
        seen,
        format!("call:{line}:{callee}"),
        format!(
            "call to `{}` in module `{}` is unsupported because `{}` has type `unknown`",
            callee,
            node.module_path.display(),
            callee
        ),
    );
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
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    context
        .load_conditional_return_sites(node)
        .into_iter()
        .filter_map(|site| {
            let expected = lower_type_text_or_name(&site.target_type);
            let expected_branches =
                semantic_union_branches(&expected).unwrap_or_else(|| vec![expected.clone()]);
            let covered = site
                .case_input_types
                .iter()
                .map(|case_type| lower_type_text_or_name(case_type))
                .collect::<Vec<_>>();
            let missing = expected_branches
                .into_iter()
                .filter(|branch| {
                    !covered
                        .iter()
                        .any(|covered_branch| {
                            semantic_type_matches(node, nodes, branch, covered_branch)
                        })
                })
                .map(|branch| render_semantic_type(&branch))
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
    name_is_unknown_boundary_with_context(context, node, nodes, None, None, usize::MAX, name)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn name_has_contextual_local_binding(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    line: usize,
    name: &str,
) -> bool {
    if current_owner_name.is_none() {
        return false;
    }
    resolve_scope_param_semantic_type(node, current_owner_name, current_owner_type_name, name)
        .is_some()
        || source_scope_param_semantic_type_with_context(
            context,
            node,
            current_owner_name,
            current_owner_type_name,
            name,
        )
        .is_some()
        || resolve_exception_binding_semantic_type(
            node,
            current_owner_name,
            current_owner_type_name,
            line,
            name,
        )
        .is_some()
        || resolve_for_loop_target_semantic_type_with_options(
            node,
            nodes,
            None,
            current_owner_name,
            current_owner_type_name,
            line,
            name,
            context.assignability_options(),
        )
        .is_some()
        || resolve_with_target_name_semantic_type_with_options(
            node,
            nodes,
            None,
            current_owner_name,
            current_owner_type_name,
            line,
            name,
            context.assignability_options(),
        )
        .is_some()
        || resolve_local_assignment_reference_semantic_type_with_options(
            node,
            nodes,
            None,
            current_owner_name,
            current_owner_type_name,
            line,
            name,
            context.assignability_options(),
        )
        .is_some()
}

fn resolve_module_value_binding_semantic_type(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    line: usize,
    name: &str,
) -> Option<SemanticType> {
    resolve_module_level_assignment_reference_semantic_type_with_options(
        node,
        nodes,
        None,
        line,
        name,
        context.assignability_options(),
    )
}

fn name_has_module_import_binding(node: &typepython_graph::ModuleNode, name: &str) -> bool {
    node.declarations.iter().any(|declaration| {
        declaration.owner.is_none()
            && declaration.name == name
            && declaration.kind == DeclarationKind::Import
    })
}

fn name_has_supported_typing_import_binding(
    node: &typepython_graph::ModuleNode,
    name: &str,
) -> bool {
    (resolve_typing_callable_signature(name).is_some() || name == "cast")
        && node.declarations.iter().any(|declaration| {
            declaration.owner.is_none()
                && declaration.name == name
                && declaration.kind == DeclarationKind::Import
                && declaration_import_target_ref(declaration).is_some_and(|target| {
                    target.symbol_target.as_ref().is_some_and(|symbol| {
                        matches!(symbol.module_key.as_str(), "typing" | "typing_extensions")
                            && symbol.symbol_name == name
                    })
                })
        })
}

fn source_param_semantic_type(param: &typepython_syntax::DirectFunctionParamSite) -> SemanticType {
    semantic_type_from_direct_param_site(param)
        .unwrap_or_else(|| SemanticType::Name(String::from("dynamic")))
}

pub(super) fn source_scope_param_semantic_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    name: &str,
) -> Option<SemanticType> {
    let owner_name = current_owner_name?;
    let source_text = context.load_source_text(node)?;
    let metadata = typepython_syntax::collect_module_surface_metadata(&source_text);
    if let Some(owner_type_name) = current_owner_type_name {
        return metadata
            .direct_method_signatures
            .iter()
            .find(|signature| {
                signature.owner_type_name == owner_type_name && signature.name == owner_name
            })
            .map(|signature| match signature.method_kind {
                typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => {
                    signature.params.as_slice()
                }
                typepython_syntax::MethodKind::Instance
                | typepython_syntax::MethodKind::Class
                | typepython_syntax::MethodKind::PropertySetter => {
                    signature.params.get(1..).unwrap_or_default()
                }
            })
            .and_then(|params| params.iter().find(|param| param.name == name))
            .map(source_param_semantic_type);
    }
    metadata
        .direct_function_signatures
        .iter()
        .find(|signature| signature.name == owner_name)
        .and_then(|signature| signature.params.iter().find(|param| param.name == name))
        .map(source_param_semantic_type)
}

pub(super) fn name_is_match_capture_in_scope(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    name: &str,
) -> bool {
    node.matches.iter().any(|match_site| {
        match_site.owner_name.as_deref() == current_owner_name
            && match_site
                .cases
                .iter()
                .any(|case| case.capture_names.iter().any(|capture_name| capture_name == name))
    })
}

pub(super) fn name_is_unknown_boundary_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    line: usize,
    name: &str,
) -> bool {
    let has_contextual_local_binding = name_has_contextual_local_binding(
        context,
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        line,
        name,
    );

    if has_contextual_local_binding {
        if let Some(resolved) = resolve_direct_name_reference_semantic_type_with_context(
            context,
            node,
            nodes,
            None,
            None,
            current_owner_name,
            current_owner_type_name,
            line,
            name,
        ) {
            return semantic_type_is_unknown(&resolved);
        }

        if let Some(resolved) = source_scope_param_semantic_type_with_context(
            context,
            node,
            current_owner_name,
            current_owner_type_name,
            name,
        ) {
            return semantic_type_is_unknown(&resolved);
        }
    }

    // Match capture names bind locally at runtime but carry no declaration the
    // binder can see; their types come from the matched pattern, so never
    // treat them as unresolved-import unknowns.
    if !has_contextual_local_binding
        && name_is_match_capture_in_scope(node, current_owner_name, name)
    {
        return false;
    }

    let module_value_binding =
        resolve_module_value_binding_semantic_type(context, node, nodes, line, name);
    let has_module_value_binding = module_value_binding.is_some();
    if let Some(resolved) = module_value_binding {
        return semantic_type_is_unknown(&resolved);
    }

    if !has_contextual_local_binding
        && !has_module_value_binding
        && name_has_module_import_binding(node, name)
        && !name_has_supported_typing_import_binding(node, name)
        && unresolved_import_boundary_type_with_context(context, node, nodes, name)
            .is_some_and(|boundary| boundary == "unknown")
    {
        return true;
    }

    if resolve_typing_callable_signature(name).is_some()
        || resolve_builtin_return_type(name).is_some()
        || matches!(
            name,
            "eval" | "exec" | "setattr" | "delattr" | "isinstance" | "framework_transform"
        )
    {
        return false;
    }

    if let Some(resolved) = resolve_direct_name_reference_semantic_type_with_context(
        context,
        node,
        nodes,
        None,
        None,
        current_owner_name,
        current_owner_type_name,
        line,
        name,
    ) {
        return semantic_type_is_unknown(&resolved);
    }

    if let Some(resolved) = source_scope_param_semantic_type_with_context(
        context,
        node,
        current_owner_name,
        current_owner_type_name,
        name,
    ) {
        return semantic_type_is_unknown(&resolved);
    }

    if !has_contextual_local_binding
        && (resolve_direct_function(node, nodes, name).is_some()
            || resolve_direct_base(nodes, node, name).is_some())
    {
        return false;
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
    context: &CheckerContext<'_>,
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

        let contextual =
            resolve_contextual_return_type(context, node, nodes, return_site, &expected);
        diagnostics.extend(contextual.diagnostics);
        let Some(actual) = contextual.actual_type else {
            continue;
        };
        let actual_text = diagnostic_type_text(&actual);

        if !context.semantic_type_is_assignable(node, &expected_type, &actual) {
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
            let diagnostic = attach_type_mismatch_notes_with_options(
                diagnostic,
                node,
                nodes,
                &expected,
                &actual_text,
                context.assignability_options(),
            );
            let diagnostic = attach_return_inference_trace(
                diagnostic,
                context,
                node,
                nodes,
                return_site,
                &expected,
                &actual_text,
            );
            diagnostics.push(attach_missing_none_return_suggestion(
                diagnostic,
                context,
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
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected: &str,
) -> ContextualReturnTypeResult {
    let metadata = direct_expr_metadata_from_return_site(return_site);
    if let Some(lambda) = metadata.value_lambda.as_deref()
        && let Some(actual_type) = resolve_contextual_lambda_callable_semantic_type_with_options(
            node,
            nodes,
            None,
            None,
            return_site.line,
            lambda,
            Some(expected),
            None,
            context.assignability_options(),
        )
    {
        return ContextualReturnTypeResult {
            actual_type: Some(actual_type),
            diagnostics: Vec::new(),
        };
    }
    if let Some(result) = resolve_contextual_typed_dict_literal_semantic_type_with_context(
        context,
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
        context,
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
            resolve_direct_expression_semantic_type_from_metadata_with_options(
                node,
                nodes,
                None,
                Some(return_site.owner_name.as_str()),
                return_site.owner_type_name.as_deref(),
                return_site.line,
                metadata,
                context.assignability_options(),
            )
        }),
        diagnostics: Vec::new(),
    }
}

pub(super) fn direct_expr_metadata_from_return_site(
    return_site: &typepython_binding::ReturnSite,
) -> typepython_syntax::DirectExprMetadata {
    return_site.value_metadata().unwrap_or(typepython_syntax::DirectExprMetadata {
        value_type_expr: None,
        is_awaited: false,
        value_callee: None,
        call_source_range: None,
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
        value_type_expr: None,
        is_awaited: false,
        value_callee: None,
        call_source_range: None,
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
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    yield_site: &typepython_binding::YieldSite,
    expected: &str,
) -> ContextualYieldTypeResult {
    let metadata = direct_expr_metadata_from_yield_site(yield_site);
    if !yield_site.is_yield_from {
        if let Some(lambda) = metadata.value_lambda.as_deref()
            && let Some(actual_type) = resolve_contextual_lambda_callable_semantic_type_with_options(
                node,
                nodes,
                None,
                None,
                yield_site.line,
                lambda,
                Some(expected),
                None,
                context.assignability_options(),
            )
        {
            return ContextualYieldTypeResult {
                actual_type: Some(actual_type),
                diagnostics: Vec::new(),
            };
        }
        if let Some(result) = resolve_contextual_typed_dict_literal_semantic_type_with_context(
            context,
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
                context,
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
        actual_type: yield_site.value_metadata().as_ref().and_then(|metadata| {
            resolve_direct_expression_semantic_type_from_metadata_with_options(
                node,
                nodes,
                None,
                Some(yield_site.owner_name.as_str()),
                yield_site.owner_type_name.as_deref(),
                yield_site.line,
                metadata,
                context.assignability_options(),
            )
        }),
        diagnostics: Vec::new(),
    }
}

pub(super) fn direct_yield_type_diagnostics(
    context: &CheckerContext<'_>,
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
        let contextual = resolve_contextual_yield_type(context, node, nodes, yield_site, &expected);
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

        if !context.semantic_type_is_assignable(node, &expected_type, &actual) {
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
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.for_loops
        .iter()
        .filter(|for_loop| !for_loop.target_names.is_empty())
        .filter_map(|for_loop| {
            let iter_type = resolve_direct_expression_semantic_type_from_metadata_with_options(
                node,
                nodes,
                None,
                for_loop.owner_name.as_deref(),
                for_loop.owner_type_name.as_deref(),
                for_loop.line,
                for_loop.iter_metadata().as_ref()?,
                context.assignability_options(),
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
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.assignments
        .iter()
        .filter(|assignment| assignment.destructuring_index == Some(0))
        .filter_map(|assignment| {
            let target_names = assignment.destructuring_target_names.as_ref()?;
            let actual = resolve_direct_expression_semantic_type_from_metadata_with_options(
                node,
                nodes,
                None,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                assignment.value_metadata().as_ref()?,
                context.assignability_options(),
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
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.with_statements
        .iter()
        .filter(|with_site| {
            resolve_with_target_semantic_type_for_signature_with_options(
                node,
                nodes,
                None,
                with_site,
                context.assignability_options(),
            )
            .is_none()
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

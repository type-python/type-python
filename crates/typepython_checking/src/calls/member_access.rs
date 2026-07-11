pub(super) fn direct_member_access_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let source = context.load_source_text(node);
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

            let owner_type = resolve_member_access_owner_semantic_type(node, nodes, access)?;
            let owner_type = scoped_type_param_bound_semantic_type(
                node,
                nodes,
                access.current_owner_name.as_deref(),
                access.current_owner_type_name.as_deref(),
                access.line,
                &access.owner_name,
                access.through_instance,
                &owner_type,
                context.assignability_options(),
            )
            .unwrap_or(owner_type);
            if semantic_union_branches(&owner_type).is_some() {
                return union_owner_member_diagnostic(
                    context,
                    node,
                    source.as_deref(),
                    &owner_type,
                    &access.owner_name,
                    &access.member,
                    access.line,
                );
            }
            let owner_type_name = semantic_nominal_owner_name(&owner_type)?;
            let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
            let before_line = instance_initializer_access_cutoff(
                &access.owner_name,
                access.current_owner_name.as_deref(),
                access.current_owner_type_name.as_deref(),
                class_decl,
                access.line,
            );
            let has_member = find_owned_readable_member_declaration(
                nodes,
                class_node,
                class_decl,
                &access.member,
            )
            .is_some()
                || has_owned_instance_assignment_member_before_line_with_context(
                    context,
                    class_node,
                    class_decl,
                    &access.member,
                    before_line,
                )
                || !find_owned_callable_declarations(nodes, class_node, class_decl, &access.member)
                    .is_empty()
                || standard_object_member(&access.member)
                || class_surface_is_open(nodes, class_node, class_decl, &mut BTreeSet::new())
                || framework_generated_member_semantic_type_with_context(
                    context,
                    node,
                    &owner_type_name,
                    &access.member,
                )
                .is_some();

            (!has_member).then(|| {
                let diagnostic = Diagnostic::error(
                    "TPY4002",
                    format!(
                        "type `{}` in module `{}` has no member `{}`",
                        class_decl.name,
                        node.module_path.display(),
                        access.member
                    ),
                );
                let mut visiting = BTreeSet::new();
                if let Some(shape) = resolve_framework_transform_class_shape_from_decl_with_context(
                    context,
                    nodes,
                    class_node,
                    class_decl,
                    &mut visiting,
                )
                    && let Some(line) = shape.origin_line
                {
                    diagnostic.with_span(Span::new(
                        shape
                            .origin_path
                            .clone()
                            .unwrap_or_else(|| class_node.module_path.display().to_string()),
                        line,
                        1,
                        line,
                        1,
                    ))
                } else {
                    diagnostic
                }
            })
        })
        .collect()
}

fn union_owner_member_diagnostic(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    source: Option<&str>,
    owner_type: &SemanticType,
    owner_name: &str,
    member: &str,
    line: usize,
) -> Option<Diagnostic> {
    let branches = semantic_member_union_branches(owner_type, context.strict_nulls)?;
    if branches.iter().any(|branch| {
        matches!(branch.strip_annotated(), SemanticType::Name(name) if matches!(name.as_str(), "Any" | "dynamic"))
    }) {
        return None;
    }
    let available = branches
        .iter()
        .filter_map(|branch| {
            let branch_name = semantic_nominal_owner_name(branch)?;
            type_has_readable_member_with_context(context, node, &branch_name, member)
                .then_some(branch_name)
        })
        .collect::<Vec<_>>();
    if available.len() == branches.len() {
        return None;
    }
    let mut diagnostic = Diagnostic::error(
        "TPY4002",
        format!(
            "type `{}` in module `{}` has no member `{}` on every union branch",
            diagnostic_type_text(owner_type),
            node.module_path.display(),
            member
        ),
    );
    if let Some((span, replacement)) =
        union_member_guard_suggestion(source, &node.module_path, owner_name, line, &available)
    {
        diagnostic = diagnostic.with_suggestion(
            format!("Insert `isinstance` guard for `{owner_name}` before accessing `{member}`"),
            span,
            replacement,
            SuggestionApplicability::MachineApplicable,
        );
    }
    Some(diagnostic)
}

#[allow(dead_code)]
pub(super) fn type_has_readable_member(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
    member: &str,
) -> bool {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    type_has_readable_member_with_context(&context, node, type_name, member)
}

pub(super) fn type_has_readable_member_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    type_name: &str,
    member: &str,
) -> bool {
    if matches!(type_name, "Any" | "dynamic") || standard_object_member(member) {
        return true;
    }
    let Some((class_node, class_decl)) = resolve_direct_base(context.nodes, node, type_name) else {
        return false;
    };
    find_owned_readable_member_declaration(context.nodes, class_node, class_decl, member).is_some()
        || has_owned_instance_assignment_member_with_context(context, class_node, class_decl, member)
        || !find_owned_callable_declarations(context.nodes, class_node, class_decl, member)
            .is_empty()
        || class_surface_is_open(context.nodes, class_node, class_decl, &mut BTreeSet::new())
        || framework_generated_member_semantic_type_with_context(context, node, type_name, member)
            .is_some()
}

// A class surface is open when any base in its hierarchy cannot be resolved
// (for example an import typed as `unknown`): the full member set is not
// statically knowable, so missing-member reports would assert knowledge the
// checker does not have.
pub(super) fn class_surface_is_open(
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visited: &mut BTreeSet<(String, String)>,
) -> bool {
    if !visited.insert((class_node.module_key.clone(), class_decl.name.clone())) {
        return false;
    }
    for base in class_decl.rendered_class_bases() {
        let base_head = base.split('[').next().unwrap_or(&base).trim();
        if matches!(base_head, "object" | "Generic" | "Protocol" | "typing.Generic" | "typing.Protocol")
        {
            continue;
        }
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base_head)
        else {
            return true;
        };
        if class_surface_is_open(nodes, base_node, base_decl, visited) {
            return true;
        }
    }
    false
}

// Apparent members of every type per spec section 14.4: the standard `object`
// surface mirrored from the bundled stdlib `builtins.pyi` stub.
pub(super) fn standard_object_member(member: &str) -> bool {
    matches!(
        member,
        "__annotations__"
            | "__class__"
            | "__delattr__"
            | "__dict__"
            | "__dir__"
            | "__doc__"
            | "__eq__"
            | "__format__"
            | "__getattribute__"
            | "__getstate__"
            | "__hash__"
            | "__init__"
            | "__init_subclass__"
            | "__module__"
            | "__ne__"
            | "__new__"
            | "__reduce__"
            | "__reduce_ex__"
            | "__repr__"
            | "__setattr__"
            | "__sizeof__"
            | "__str__"
            | "__subclasshook__"
    )
}

pub(super) fn union_member_guard_suggestion(
    source: Option<&str>,
    module_path: &std::path::Path,
    owner_name: &str,
    line: usize,
    available_branches: &[String],
) -> Option<(Span, String)> {
    let guard_types = available_branches
        .iter()
        .filter_map(|branch| isinstance_guard_type_name(branch))
        .collect::<Vec<_>>();
    if guard_types.is_empty() {
        return None;
    }
    let source = source?;
    let line_text = source.lines().nth(line.checked_sub(1)?)?;
    let indent = leading_space_count(line_text);
    let guard = if guard_types.len() == 1 {
        guard_types[0].clone()
    } else {
        format!("({})", guard_types.join(", "))
    };
    Some((
        Span::new(module_path.display().to_string(), line, 1, line, 1),
        format!("{}assert isinstance({}, {})\n", " ".repeat(indent), owner_name, guard),
    ))
}

pub(super) fn isinstance_guard_type_name(type_name: &str) -> Option<String> {
    let normalized = normalize_type_text(type_name);
    if normalized.is_empty()
        || matches!(normalized.as_str(), "None" | "Any" | "dynamic" | "unknown")
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

    for call in &node.method_calls {
        if !call.through_instance
            && let Some(module_diagnostics) =
                imported_module_method_call_diagnostics(context, node, nodes, call)
        {
            diagnostics.extend(module_diagnostics);
            continue;
        }

        let scope_owner_type =
            resolve_method_call_owner_scope_semantic_type(context, node, nodes, call).map(
                |owner_type| {
                    scoped_type_param_bound_semantic_type(
                        node,
                        nodes,
                        call.current_owner_name.as_deref(),
                        call.current_owner_type_name.as_deref(),
                        call.line,
                        &call.owner_name,
                        call.through_instance,
                        &owner_type,
                        context.assignability_options(),
                    )
                    .unwrap_or(owner_type)
                },
            );
        if let Some(scope_owner_type) = &scope_owner_type
            && semantic_union_branches(scope_owner_type).is_some()
        {
            let source = context.load_source_text(node);
            if let Some(diagnostic) = union_owner_member_diagnostic(
                context,
                node,
                source.as_deref(),
                scope_owner_type,
                &call.owner_name,
                &call.method,
                call.line,
            ) {
                diagnostics.push(diagnostic);
                continue;
            }
        }
        if let Some(scope_owner_type) = &scope_owner_type
            && let Some(scope_owner_type_name) = semantic_nominal_owner_name(scope_owner_type)
            && let Some((scope_class_node, scope_class_decl)) =
                resolve_direct_base(nodes, node, &scope_owner_type_name)
            && find_owned_callable_declarations(
                nodes,
                scope_class_node,
                scope_class_decl,
                &call.method,
            )
            .is_empty()
        {
            let before_line = instance_initializer_access_cutoff(
                &call.owner_name,
                call.current_owner_name.as_deref(),
                call.current_owner_type_name.as_deref(),
                scope_class_decl,
                call.line,
            );
            let has_member = find_owned_readable_member_declaration(
                nodes,
                scope_class_node,
                scope_class_decl,
                &call.method,
            )
            .is_some()
                || has_owned_instance_assignment_member_before_line_with_context(
                    context,
                    scope_class_node,
                    scope_class_decl,
                    &call.method,
                    before_line,
                )
                || standard_object_member(&call.method)
                || class_surface_is_open(
                    nodes,
                    scope_class_node,
                    scope_class_decl,
                    &mut BTreeSet::new(),
                )
                || framework_generated_member_semantic_type_with_context(
                    context,
                    node,
                    &scope_owner_type_name,
                    &call.method,
                )
                .is_some();
            if !has_member {
                diagnostics.push(Diagnostic::error(
                    "TPY4002",
                    format!(
                        "type `{}` in module `{}` has no member `{}`",
                        scope_class_decl.name,
                        node.module_path.display(),
                        call.method
                    ),
                ));
                continue;
            }
        }
        let Some(receiver_type) = resolve_method_call_owner_type(context, node, nodes, call) else {
            continue;
        };
        let bound_owner_type = scoped_type_param_bound_semantic_type(
            node,
            nodes,
            call.current_owner_name.as_deref(),
            call.current_owner_type_name.as_deref(),
            call.line,
            &call.owner_name,
            call.through_instance,
            &receiver_type,
            context.assignability_options(),
        );
        let owner_type = bound_owner_type.as_ref().unwrap_or(&receiver_type);
        let owner_variants = if let Some(branches) =
            semantic_member_union_branches(owner_type, context.strict_nulls)
        {
            branches
                .into_iter()
                .map(|branch| {
                    if bound_owner_type.is_some() {
                        (receiver_type.clone(), Some(branch))
                    } else {
                        (branch, None)
                    }
                })
                .collect::<Vec<_>>()
        } else {
            vec![(receiver_type, bound_owner_type)]
        };
        let mut owner_variants = owner_variants.into_iter();
        let mut branch_diagnostics = owner_variants.next().map_or_else(Vec::new, |variant| {
            direct_method_call_variant_diagnostics(
                context,
                node,
                nodes,
                call,
                &variant.0,
                variant.1.as_ref(),
            )
        });
        for (receiver_type, bound_owner_type) in owner_variants {
            let variant_diagnostics = direct_method_call_variant_diagnostics(
                context,
                node,
                nodes,
                call,
                &receiver_type,
                bound_owner_type.as_ref(),
            );
            let mut matched = vec![false; branch_diagnostics.len()];
            for diagnostic in variant_diagnostics {
                let existing = branch_diagnostics
                    .iter()
                    .take(matched.len())
                    .enumerate()
                    .find_map(|(index, candidate)| {
                        (!matched[index] && candidate == &diagnostic).then_some(index)
                    });
                if let Some(index) = existing {
                    matched[index] = true;
                } else {
                    branch_diagnostics.push(diagnostic);
                }
            }
        }
        diagnostics.extend(branch_diagnostics);
    }

    diagnostics
}

fn direct_method_call_variant_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::MethodCallSite,
    receiver_type: &SemanticType,
    bound_owner_type: Option<&SemanticType>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let owner_type = bound_owner_type.unwrap_or(receiver_type);
    let Some(owner_type_name) = semantic_nominal_owner_name(owner_type) else {
        return diagnostics;
    };
    let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &owner_type_name) else {
        return diagnostics;
    };
    let candidates = find_resolved_callable_declarations(
        nodes,
        class_node,
        class_decl,
        owner_type,
        &call.method,
    );
    let Some(target) = candidates.first() else {
        let before_line = instance_initializer_access_cutoff(
            &call.owner_name,
            call.current_owner_name.as_deref(),
            call.current_owner_type_name.as_deref(),
            class_decl,
            call.line,
        );
        let member_type = find_resolved_readable_member_declaration(
            nodes,
            class_node,
            class_decl,
            owner_type,
            &call.method,
        )
        .and_then(|member| {
            resolve_resolved_readable_member_semantic_type_with_self_type(
                node,
                nodes,
                &member,
                receiver_type,
            )
        })
        .or_else(|| {
            owned_instance_assignment_member_semantic_type_with_context(
                context,
                class_node,
                class_decl,
                &call.method,
                before_line,
            )
        });
        if let Some(member_type) = member_type
            && !semantic_type_may_be_callable(context, class_node, &member_type)
        {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4001",
                    format!(
                        "member `{}` on type `{}` has non-callable type `{}`",
                        call.method,
                        class_decl.name,
                        diagnostic_type_text(&member_type),
                    ),
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    call.line,
                    1,
                    call.line,
                    1,
                )),
            );
        }
        return diagnostics;
    };

    let direct_call = typepython_binding::CallSite {
        callee: format!("{}.{}", class_decl.name, call.method),
        source_range: call.source_range,
        arg_count: call.arg_count,
        arg_values: call.arg_values.clone(),
        starred_arg_values: call.starred_arg_values.clone(),
        keyword_names: call.keyword_names.clone(),
        keyword_arg_values: call.keyword_arg_values.clone(),
        keyword_expansion_values: call.keyword_expansion_values.clone(),
        line: call.line,
    };

    let overloads = candidates
        .iter()
        .filter(|member| member.declaration.kind == DeclarationKind::Overload)
        .map(|member| {
            (
                member.declaration,
                context.load_declaration_semantics(member.declaration).callable,
            )
        })
        .collect::<Vec<_>>();
    if !overloads.is_empty() {
        match resolve_method_overload_selection(
            node,
            nodes,
            &direct_call,
            call.current_owner_name.as_deref(),
            call.current_owner_type_name.as_deref(),
            receiver_type,
            Some(&target.declaring_type),
            &overloads,
            context.assignability_options(),
        ) {
            ResolvedOverloadSelection::Selected(candidate) => {
                let signature = candidate.signature_sites;
                if let Some(diagnostic) =
                    direct_source_function_arity_diagnostic_in_scope_with_context(
                        context,
                        node,
                        nodes,
                        &direct_call,
                        &signature,
                        call.current_owner_name.as_deref(),
                        call.current_owner_type_name.as_deref(),
                    )
                {
                    diagnostics.push(diagnostic);
                }
                diagnostics.extend(
                    direct_source_function_keyword_diagnostics_in_scope_with_context(
                        context,
                        node,
                        nodes,
                        &direct_call,
                        &signature,
                        call.current_owner_name.as_deref(),
                        call.current_owner_type_name.as_deref(),
                    ),
                );
                diagnostics.extend(direct_source_function_type_diagnostics_in_scope_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &signature,
                    call.current_owner_name.as_deref(),
                    call.current_owner_type_name.as_deref(),
                ));
                return diagnostics;
            }
            ResolvedOverloadSelection::Ambiguous { applicable_count } => {
                diagnostics.push(Diagnostic::error(
                    "TPY4012",
                    format!(
                        "call to `{}.{}` in module `{}` is ambiguous across {} overloads after applicability filtering",
                        class_decl.name,
                        call.method,
                        node.module_path.display(),
                        applicable_count
                    ),
                ));
                return diagnostics;
            }
            ResolvedOverloadSelection::NotApplicable { runtime_generic_failures } => {
                if let Some((_declaration, failure)) = runtime_generic_failures.first() {
                    let callee = format!("{}.{}", class_decl.name, call.method);
                    diagnostics.push(unresolved_generic_call_diagnostic(
                        node,
                        call.line,
                        &callee,
                        failure,
                    ));
                    return diagnostics;
                }
            }
        }
    }

    let target_callable = context.load_declaration_semantics(target.declaration).callable;
    match resolve_method_call_candidate_detailed(
        node,
        nodes,
        target.declaration,
        &direct_call,
        call.current_owner_name.as_deref(),
        call.current_owner_type_name.as_deref(),
        receiver_type,
        Some(&target.declaring_type),
        target_callable.as_ref(),
        context.assignability_options(),
    ) {
        Ok(resolved) => {
            if let Some(diagnostic) =
                direct_source_function_arity_diagnostic_in_scope_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &resolved.signature_sites,
                    call.current_owner_name.as_deref(),
                    call.current_owner_type_name.as_deref(),
                )
            {
                diagnostics.push(diagnostic);
            }
            diagnostics.extend(
                direct_source_function_keyword_diagnostics_in_scope_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &resolved.signature_sites,
                    call.current_owner_name.as_deref(),
                    call.current_owner_type_name.as_deref(),
                ),
            );
            diagnostics.extend(direct_source_function_type_diagnostics_in_scope_with_context(
                context,
                node,
                nodes,
                &direct_call,
                &resolved.signature_sites,
                call.current_owner_name.as_deref(),
                call.current_owner_type_name.as_deref(),
            ));
            return diagnostics;
        }
        Err(failure) if declaration_has_runtime_generic_paramlist(target.declaration) => {
            let callee = format!("{}.{}", class_decl.name, call.method);
            diagnostics.push(unresolved_generic_call_diagnostic(
                node,
                call.line,
                &callee,
                &failure,
            ));
            return diagnostics;
        }
        Err(_) => {}
    }

    let fallback_signature = target_callable
        .as_ref()
        .map(|callable| {
            let owner_substitutions = without_shadowed_generic_params(
                owner_generic_substitutions(&target.declaring_type, target.declaring_class),
                target.declaration,
            );
            let params =
                method_semantic_params_without_self_from_semantics(target.declaration, callable);
            let params = substitute_semantic_callable_params(&params, &owner_substitutions);
            let params = substitute_self_semantic_params_with_type(&params, Some(receiver_type));
            signature_sites_from_semantic_params(&params)
        })
        .unwrap_or_default();
    if let Some(diagnostic) = direct_source_function_arity_diagnostic_in_scope_with_context(
        context,
        node,
        nodes,
        &direct_call,
        &fallback_signature,
        call.current_owner_name.as_deref(),
        call.current_owner_type_name.as_deref(),
    ) {
        diagnostics.push(diagnostic);
    }
    diagnostics.extend(direct_source_function_keyword_diagnostics_in_scope_with_context(
        context,
        node,
        nodes,
        &direct_call,
        &fallback_signature,
        call.current_owner_name.as_deref(),
        call.current_owner_type_name.as_deref(),
    ));
    diagnostics.extend(direct_source_function_type_diagnostics_in_scope_with_context(
        context,
        node,
        nodes,
        &direct_call,
        &fallback_signature,
        call.current_owner_name.as_deref(),
        call.current_owner_type_name.as_deref(),
    ));
    diagnostics
}

fn instance_initializer_access_cutoff(
    receiver_name: &str,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    class_decl: &Declaration,
    line: usize,
) -> Option<usize> {
    (receiver_name == "self"
        && current_owner_name == Some("__init__")
        && current_owner_type_name == Some(class_decl.name.as_str()))
    .then_some(line)
}

fn semantic_type_may_be_callable(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    semantic_type: &SemanticType,
) -> bool {
    match semantic_type.strip_annotated() {
        SemanticType::Callable { .. } => true,
        SemanticType::Name(name) if matches!(name.as_str(), "Any" | "dynamic" | "unknown") => {
            true
        }
        SemanticType::Name(name)
            if matches!(
                name.as_str(),
                "None"
                    | "bool"
                    | "int"
                    | "float"
                    | "complex"
                    | "str"
                    | "bytes"
                    | "bytearray"
                    | "memoryview"
                    | "range"
                    | "slice"
                    | "list"
                    | "dict"
                    | "tuple"
                    | "set"
                    | "frozenset"
            ) =>
        {
            false
        }
        SemanticType::Generic { head, .. }
            if matches!(
                head.as_str(),
                "list" | "dict" | "tuple" | "set" | "frozenset"
            ) =>
        {
            false
        }
        SemanticType::Union(branches) => {
            branches.iter().all(|branch| semantic_type_may_be_callable(context, node, branch))
        }
        SemanticType::Generic { head, .. }
            if matches!(head.as_str(), "type" | "typing.Type") =>
        {
            true
        }
        SemanticType::Name(name) | SemanticType::Generic { head: name, .. } => {
            let Some((class_node, class_decl)) = resolve_direct_base(context.nodes, node, name)
            else {
                return true;
            };
            !find_owned_callable_declarations(
                context.nodes,
                class_node,
                class_decl,
                "__call__",
            )
            .is_empty()
                || class_surface_is_open(
                    context.nodes,
                    class_node,
                    class_decl,
                    &mut BTreeSet::new(),
                )
        }
        SemanticType::Annotated { .. } => unreachable!("annotations were stripped"),
        SemanticType::Unpack(_) => false,
    }
}

pub(super) fn resolve_method_call_owner_type(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::MethodCallSite,
) -> Option<SemanticType> {
    if call.through_instance {
        return resolve_direct_callable_return_semantic_type(node, nodes, &call.owner_name)
            .or_else(|| Some(SemanticType::Name(call.owner_name.clone())));
    }

    resolve_direct_name_reference_semantic_type_with_context(
        context,
        node,
        nodes,
        None,
        None,
        call.current_owner_name.as_deref(),
        call.current_owner_type_name.as_deref(),
        call.line,
        &call.owner_name,
    )
    .or_else(|| Some(SemanticType::Name(call.owner_name.clone())))
}

fn resolve_method_call_owner_scope_semantic_type(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::MethodCallSite,
) -> Option<SemanticType> {
    if call.through_instance {
        return resolve_direct_callable_return_semantic_type(node, nodes, &call.owner_name);
    }
    resolve_direct_name_reference_semantic_type_with_context(
        context,
        node,
        nodes,
        None,
        None,
        call.current_owner_name.as_deref(),
        call.current_owner_type_name.as_deref(),
        call.line,
        &call.owner_name,
    )
}

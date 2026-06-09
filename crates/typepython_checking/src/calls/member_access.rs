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
            if let Some(branches) = semantic_union_branches(&owner_type) {
                let member_required_branches = branches
                    .iter()
                    .filter(|branch| context.strict_nulls || !semantic_branch_is_none(branch))
                    .collect::<Vec<_>>();
                let available = branches
                    .iter()
                    .filter(|branch| context.strict_nulls || !semantic_branch_is_none(branch))
                    .filter_map(|branch| {
                        let branch_name = semantic_nominal_owner_name(branch)?;
                        type_has_readable_member_with_context(
                            context,
                            node,
                            &branch_name,
                            &access.member,
                        )
                        .then_some(branch_name)
                    })
                    .collect::<Vec<_>>();
                if available.len() == member_required_branches.len() {
                    return None;
                }
                let mut diagnostic = Diagnostic::error(
                    "TPY4002",
                    format!(
                        "type `{}` in module `{}` has no member `{}` on every union branch",
                        diagnostic_type_text(&owner_type),
                        node.module_path.display(),
                        access.member
                    ),
                );
                if let Some((span, replacement)) = union_member_guard_suggestion(
                    source.as_deref(),
                    &node.module_path,
                    access,
                    &available,
                )
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
            let owner_type_name = semantic_nominal_owner_name(&owner_type)?;
            let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
            let has_member = find_owned_readable_member_declaration(
                nodes,
                class_node,
                class_decl,
                &access.member,
            )
            .is_some()
                || !find_owned_callable_declarations(nodes, class_node, class_decl, &access.member)
                    .is_empty()
                || standard_object_member(&access.member)
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

fn semantic_branch_is_none(branch: &SemanticType) -> bool {
    matches!(branch.strip_annotated(), SemanticType::Name(name) if name == "None")
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
    if standard_object_member(member) {
        return true;
    }
    let Some((class_node, class_decl)) = resolve_direct_base(context.nodes, node, type_name) else {
        return false;
    };
    find_owned_readable_member_declaration(context.nodes, class_node, class_decl, member).is_some()
        || !find_owned_callable_declarations(context.nodes, class_node, class_decl, member)
            .is_empty()
        || framework_generated_member_semantic_type_with_context(context, node, type_name, member)
            .is_some()
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
    let source = source?;
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

    for call in &node.method_calls {
        if !call.through_instance
            && let Some(module_diagnostics) =
                imported_module_method_call_diagnostics(context, node, nodes, call)
        {
            diagnostics.extend(module_diagnostics);
            continue;
        }

        let Some(owner_type) = resolve_method_call_owner_type(context, node, nodes, call) else {
            continue;
        };
        let Some(owner_type_name) = semantic_nominal_owner_name(&owner_type) else {
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
            arg_values: call.arg_values.clone(),
            starred_arg_values: call.starred_arg_values.clone(),
            keyword_names: call.keyword_names.clone(),
            keyword_arg_values: call.keyword_arg_values.clone(),
            keyword_expansion_values: call.keyword_expansion_values.clone(),
            line: 1,
        };

        let overloads = candidates
            .iter()
            .filter(|declaration| declaration.kind == DeclarationKind::Overload)
            .map(|declaration| (*declaration, context.load_declaration_semantics(declaration).callable))
            .collect::<Vec<_>>();
        if !overloads.is_empty() {
            match resolve_method_overload_selection(
                node,
                nodes,
                &direct_call,
                &owner_type,
                &overloads,
                context.assignability_options(),
            ) {
                ResolvedOverloadSelection::Selected(candidate) => {
                    let signature = candidate.signature_sites;
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
                    continue;
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
                        continue;
                    }
                }
            }
        }

        let target_callable = context.load_declaration_semantics(target).callable;
        match resolve_method_call_candidate_detailed(
            node,
            nodes,
            target,
            &direct_call,
            &owner_type,
            target_callable.as_ref(),
            context.assignability_options(),
        ) {
            Ok(resolved) => {
                if let Some(diagnostic) = direct_source_function_arity_diagnostic_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &resolved.signature_sites,
                ) {
                    diagnostics.push(diagnostic);
                }
                diagnostics.extend(direct_source_function_keyword_diagnostics_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &resolved.signature_sites,
                ));
                diagnostics.extend(direct_source_function_type_diagnostics_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &resolved.signature_sites,
                ));
                continue;
            }
            Err(failure) if declaration_has_runtime_generic_paramlist(target) => {
                let callee = format!("{}.{}", class_decl.name, call.method);
                diagnostics.push(unresolved_generic_call_diagnostic(
                    node,
                    call.line,
                    &callee,
                    &failure,
                ));
                continue;
            }
            Err(_) => {}
        }

        let fallback_signature = target_callable
            .as_ref()
            .map(|callable| method_signature_sites_from_semantics(target, callable, &class_decl.name))
            .unwrap_or_default();
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
        None,
        None,
        call.line,
        &call.owner_name,
    )
    .or_else(|| Some(SemanticType::Name(call.owner_name.clone())))
}

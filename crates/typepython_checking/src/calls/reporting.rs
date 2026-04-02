pub(super) fn unresolved_import_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let project_roots: BTreeSet<_> = nodes
        .iter()
        .filter_map(|candidate| candidate.module_key.split('.').next())
        .map(str::to_owned)
        .collect();

    node.declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Import)
        .filter_map(|declaration| {
            let root = declaration.detail.split('.').next()?;
            if !project_roots.contains(root) {
                return None;
            }

            let resolves = nodes.iter().any(|candidate| candidate.module_key == declaration.detail)
                || declaration
                    .detail
                    .rsplit_once('.')
                    .and_then(|(module_key, symbol_name)| {
                        nodes.iter().find(|candidate| candidate.module_key == module_key).map(
                            |target| {
                                target.declarations.iter().any(|declaration| {
                                    declaration.owner.is_none() && declaration.name == symbol_name
                                })
                            },
                        )
                    })
                    .unwrap_or(false);

            (!resolves).then(|| {
                Diagnostic::error(
                    "TPY3001",
                    format!(
                        "module `{}` imports unresolved same-project target `{}`",
                        node.module_path.display(),
                        declaration.detail
                    ),
                )
            })
        })
        .collect()
}

pub(super) fn deprecated_use_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    report_deprecated: DiagnosticLevel,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for declaration in
        node.declarations.iter().filter(|declaration| declaration.kind == DeclarationKind::Import)
    {
        if let Some(target) = resolve_import_target(node, nodes, declaration) {
            if target.is_deprecated {
                if let Some(diagnostic) = deprecated_diagnostic(
                    report_deprecated,
                    format!(
                        "module `{}` imports deprecated declaration `{}`",
                        node.module_path.display(),
                        declaration.name
                    ),
                    target.deprecation_message.as_deref(),
                ) {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    for call in &node.calls {
        if let Some(target) = resolve_direct_function(node, nodes, &call.callee) {
            if target.is_deprecated {
                if let Some(diagnostic) = deprecated_diagnostic(
                    report_deprecated,
                    format!(
                        "module `{}` calls deprecated declaration `{}`",
                        node.module_path.display(),
                        call.callee
                    ),
                    target.deprecation_message.as_deref(),
                ) {
                    diagnostics.push(diagnostic);
                }
            }
        } else if let Some((_, target)) = resolve_direct_base(nodes, node, &call.callee) {
            if target.is_deprecated {
                if let Some(diagnostic) = deprecated_diagnostic(
                    report_deprecated,
                    format!(
                        "module `{}` instantiates deprecated declaration `{}`",
                        node.module_path.display(),
                        call.callee
                    ),
                    target.deprecation_message.as_deref(),
                ) {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    for access in &node.member_accesses {
        if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &access.owner_name)
        {
            if let Some(member) =
                find_owned_value_declaration(nodes, class_node, class_decl, &access.member)
            {
                if member.is_deprecated {
                    if let Some(diagnostic) = deprecated_diagnostic(
                        report_deprecated,
                        format!(
                            "module `{}` uses deprecated member `{}` on `{}`",
                            node.module_path.display(),
                            access.member,
                            access.owner_name
                        ),
                        member.deprecation_message.as_deref(),
                    ) {
                        diagnostics.push(diagnostic);
                    }
                }
            }
        }
    }

    for call in &node.method_calls {
        if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &call.owner_name) {
            if let Some(method) =
                find_owned_callable_declaration(nodes, class_node, class_decl, &call.method)
            {
                if method.is_deprecated {
                    if let Some(diagnostic) = deprecated_diagnostic(
                        report_deprecated,
                        format!(
                            "module `{}` calls deprecated member `{}` on `{}`",
                            node.module_path.display(),
                            call.method,
                            call.owner_name
                        ),
                        method.deprecation_message.as_deref(),
                    ) {
                        diagnostics.push(diagnostic);
                    }
                }
            }
        }
    }

    diagnostics
}

pub(super) fn deprecated_diagnostic(
    report_deprecated: DiagnosticLevel,
    message: String,
    deprecation_message: Option<&str>,
) -> Option<Diagnostic> {
    let diagnostic = match report_deprecated {
        DiagnosticLevel::Ignore => return None,
        DiagnosticLevel::Warning => Diagnostic::warning("TPY4101", message),
        DiagnosticLevel::Error => Diagnostic::error("TPY4101", message),
    };
    Some(match deprecation_message {
        Some(note) if !note.is_empty() => diagnostic.with_note(note),
        _ => diagnostic,
    })
}

pub(super) fn type_supports_mismatch_path(text: &str) -> bool {
    union_branches(text).is_some() || split_generic_type(text).is_some()
}

pub(super) fn attach_type_mismatch_notes(
    diagnostic: Diagnostic,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> Diagnostic {
    let expected = normalize_type_text(expected);
    let actual = normalize_type_text(actual);
    if !type_supports_mismatch_path(&expected) && !type_supports_mismatch_path(&actual) {
        return diagnostic;
    }

    let mut diagnostic = diagnostic
        .with_note(format!("source: `{actual}`"))
        .with_note(format!("target: `{expected}`"));
    let mut path = Vec::new();
    if let Some(detail) = first_type_mismatch_detail(node, nodes, &expected, &actual, &mut path, 8)
    {
        if !path.is_empty() {
            diagnostic = diagnostic.with_note(format!(
                "mismatch at: {}",
                path.iter().map(|segment| format!("-> {segment}")).collect::<Vec<_>>().join(" ")
            ));
        }
        diagnostic = diagnostic.with_note(detail);
    }
    diagnostic
}

pub(super) fn first_type_mismatch_detail(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
    path: &mut Vec<String>,
    depth: usize,
) -> Option<String> {
    if depth == 0 {
        return None;
    }

    let expected = normalize_type_text(expected);
    let actual = normalize_type_text(actual);
    if path.last().is_none_or(|segment| segment != &actual) {
        path.push(actual.clone());
    }

    if let (Some(expected_branches), Some(actual_branches)) =
        (union_branches(&expected), union_branches(&actual))
    {
        if let Some(unmatched) = actual_branches.iter().find(|branch| {
            !expected_branches
                .iter()
                .any(|target_branch| direct_type_is_assignable(node, nodes, target_branch, branch))
        }) {
            if path.last().is_none_or(|segment| segment != unmatched) {
                path.push(unmatched.clone());
            }
            return Some(format!(
                "union branch `{}` is not assignable to any target branch in `{}`",
                unmatched, expected
            ));
        }
    }

    if let (Some((expected_head, expected_args)), Some((actual_head, actual_args))) =
        (split_generic_type(&expected), split_generic_type(&actual))
    {
        let (expected_args, actual_args) = if expected_head == "tuple" && actual_head == "tuple" {
            (expanded_tuple_shape_args(&expected_args), expanded_tuple_shape_args(&actual_args))
        } else {
            (expected_args, actual_args)
        };
        if expected_head == "tuple"
            && actual_head == "tuple"
            && actual_args.len() == 2
            && actual_args[1] == "..."
            && !(expected_args.len() == 2 && expected_args[1] == "...")
        {
            return Some(format!(
                "`{actual}` is a variable-length tuple and is not assignable to fixed tuple `{expected}`"
            ));
        }

        if expected_head == actual_head && expected_args.len() == actual_args.len() {
            for (expected_arg, actual_arg) in expected_args.iter().zip(actual_args.iter()) {
                if !direct_type_is_assignable(node, nodes, expected_arg, actual_arg) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        expected_arg,
                        actual_arg,
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!("`{}` is not assignable to `{}`", actual_arg, expected_arg))
                    });
                }
            }
            if expected_head == "tuple" && expected_args != actual_args {
                return Some(format!("tuple shape `{actual}` is not assignable to `{expected}`"));
            }
        }

        match (expected_head, actual_head) {
            ("Sequence", "list") | ("Sequence", "tuple") if !expected_args.is_empty() => {
                let actual_element = if actual_head == "tuple" {
                    if actual_args.len() == 2 && actual_args[1] == "..." {
                        actual_args[0].clone()
                    } else {
                        join_branch_types(actual_args.clone())
                    }
                } else {
                    actual_args.first().cloned().unwrap_or_default()
                };
                if !direct_type_is_assignable(node, nodes, &expected_args[0], &actual_element) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        &expected_args[0],
                        &actual_element,
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!(
                            "element type `{}` is not assignable to `{}`",
                            actual_element, expected_args[0]
                        ))
                    });
                }
            }
            ("Mapping", "dict") if expected_args.len() == 2 && actual_args.len() == 2 => {
                if !direct_type_is_assignable(node, nodes, &expected_args[0], &actual_args[0]) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        &expected_args[0],
                        &actual_args[0],
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!(
                            "mapping key type `{}` is not assignable to `{}`",
                            actual_args[0], expected_args[0]
                        ))
                    });
                }
                if !direct_type_is_assignable(node, nodes, &expected_args[1], &actual_args[1]) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        &expected_args[1],
                        &actual_args[1],
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!(
                            "mapping value type `{}` is not assignable to `{}`",
                            actual_args[1], expected_args[1]
                        ))
                    });
                }
            }
            _ => {}
        }
    }

    None
}

pub(super) fn same_return_owner(
    left: &typepython_binding::ReturnSite,
    right: &typepython_binding::ReturnSite,
) -> bool {
    left.owner_name == right.owner_name && left.owner_type_name == right.owner_type_name
}

pub(super) fn describe_return_trace_expression(
    return_site: &typepython_binding::ReturnSite,
) -> String {
    if return_site.value_name.is_none()
        && return_site.value_member_name.is_none()
        && return_site.value_method_name.is_none()
        && return_site.value_callee.is_none()
        && return_site.value_lambda.is_none()
        && return_site.value_list_elements.is_none()
        && return_site.value_set_elements.is_none()
        && return_site.value_dict_entries.is_none()
        && return_site.value_subscript_target.is_none()
    {
        return String::from("bare return");
    }
    if let Some(name) = &return_site.value_name {
        return format!("return {name}");
    }
    if let (Some(owner), Some(member)) =
        (&return_site.value_member_owner_name, &return_site.value_member_name)
    {
        return format!("return {}.{}", owner, member);
    }
    if let (Some(owner), Some(method)) =
        (&return_site.value_method_owner_name, &return_site.value_method_name)
    {
        return format!("return {}.{}(...)", owner, method);
    }
    if let Some(callee) = &return_site.value_callee {
        return format!("return {}(...)", callee);
    }
    if let Some(key) = &return_site.value_subscript_string_key {
        return format!("return [...][\"{key}\"]");
    }
    if return_site.value_lambda.is_some() {
        return String::from("return lambda");
    }
    if return_site.value_dict_entries.is_some() {
        return String::from("return dict literal");
    }
    if return_site.value_list_elements.is_some() {
        return String::from("return list literal");
    }
    if return_site.value_set_elements.is_some() {
        return String::from("return set literal");
    }
    String::from("return expression")
}

pub(super) fn inferred_return_type_for_owner(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected: &str,
    signature: &str,
) -> Option<String> {
    let related_returns = node
        .returns
        .iter()
        .filter(|candidate| same_return_owner(candidate, return_site))
        .collect::<Vec<_>>();
    if related_returns.is_empty() {
        return None;
    }

    let mut trace_types = Vec::new();
    for candidate in related_returns {
        let contextual =
            resolve_contextual_return_type(node, nodes, candidate, expected, signature);
        let candidate_type = contextual
            .actual_type
            .or_else(|| candidate.value_type.clone())
            .unwrap_or_else(|| String::from("unknown"));
        trace_types.push(candidate_type);
    }

    Some(if trace_types.len() > 1 {
        join_branch_types(trace_types)
    } else {
        normalize_type_text(trace_types.first().expect("single return type should exist"))
    })
}

pub(super) fn attach_return_inference_trace(
    mut diagnostic: Diagnostic,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected: &str,
    actual: &str,
    signature: &str,
) -> Diagnostic {
    let related_returns = node
        .returns
        .iter()
        .filter(|candidate| same_return_owner(candidate, return_site))
        .collect::<Vec<_>>();
    if related_returns.is_empty() {
        return diagnostic;
    }

    let mut trace_types = Vec::new();
    let mut trace_lines = Vec::new();

    for candidate in related_returns {
        let contextual =
            resolve_contextual_return_type(node, nodes, candidate, expected, signature);
        let candidate_type = contextual
            .actual_type
            .or_else(|| candidate.value_type.clone())
            .unwrap_or_else(|| String::from("unknown"));
        trace_types.push(candidate_type.clone());
        trace_lines.push(format!(
            "line {}: {} -> {}",
            candidate.line,
            describe_return_trace_expression(candidate),
            normalize_type_text(&candidate_type)
        ));
    }

    let inferred_return_type =
        inferred_return_type_for_owner(node, nodes, return_site, expected, signature)
            .unwrap_or_else(|| normalize_type_text(actual));
    diagnostic = diagnostic
        .with_note(format!(
            "inferred return type: `{}`",
            normalize_type_text(&inferred_return_type)
        ))
        .with_note(format!("declared return type: `{}`", normalize_type_text(expected)))
        .with_note(String::from("inference trace:"));

    for line in trace_lines {
        diagnostic = diagnostic.with_note(line);
    }

    if node.returns.iter().filter(|candidate| same_return_owner(candidate, return_site)).count() > 1
    {
        diagnostic =
            diagnostic.with_note(format!("join: `{}`", normalize_type_text(&inferred_return_type)));
    }

    diagnostic
}

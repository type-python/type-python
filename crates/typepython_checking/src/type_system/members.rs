#[expect(
    clippy::too_many_arguments,
    reason = "member reference resolution needs source metadata and scope context"
)]
pub(super) fn resolve_direct_member_reference_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    owner_name: &str,
    member_name: &str,
    through_instance: bool,
) -> Option<SemanticType> {
    if !through_instance
        && let Some(reference_type) = resolve_imported_module_member_reference_semantic_type(
            node,
            nodes,
            owner_name,
            member_name,
        )
    {
        return Some(reference_type);
    }

    let owner_type = if through_instance {
        resolve_direct_callable_return_semantic_type(node, nodes, owner_name)
            .or_else(|| Some(SemanticType::Name(owner_name.to_owned())))
    } else {
        resolve_direct_name_reference_semantic_type(
            node,
            nodes,
            signature,
            exclude_name,
            current_owner_name,
            current_owner_type_name,
            current_line,
            owner_name,
        )
        .or_else(|| Some(SemanticType::Name(owner_name.to_owned())))
    }?;

    let owner_type_name = render_semantic_type(&owner_type);
    let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
    let member =
        find_owned_readable_member_declaration(nodes, class_node, class_decl, member_name)?;
    if is_enum_like_class(nodes, class_node, class_decl) {
        return Some(lower_type_text_or_name(&format!("Literal[{}.{}]", class_decl.name, member_name)));
    }
    resolve_readable_member_semantic_type(node, member, &owner_type)
}

pub(super) fn is_enum_like_class(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
) -> bool {
    declaration.bases.iter().any(|base| {
        matches!(
            base.as_str(),
            "Enum"
                | "IntEnum"
                | "StrEnum"
                | "Flag"
                | "IntFlag"
                | "enum.Enum"
                | "enum.IntEnum"
                | "enum.StrEnum"
                | "enum.Flag"
                | "enum.IntFlag"
        ) || resolve_direct_base(nodes, node, base)
            .is_some_and(|(base_node, base_decl)| is_enum_like_class(nodes, base_node, base_decl))
    })
}

pub(super) fn is_flag_enum_like_class(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
) -> bool {
    declaration.bases.iter().any(|base| {
        matches!(base.as_str(), "Flag" | "IntFlag" | "enum.Flag" | "enum.IntFlag")
            || resolve_direct_base(nodes, node, base).is_some_and(|(base_node, base_decl)| {
                is_flag_enum_like_class(nodes, base_node, base_decl)
            })
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "method return resolution needs source metadata and scope context"
)]
pub(super) fn resolve_direct_method_return_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    owner_name: &str,
    method_name: &str,
    through_instance: bool,
) -> Option<SemanticType> {
    if !through_instance
        && let Some(return_type) = resolve_imported_module_method_return_semantic_type(
            node,
            nodes,
            current_line,
            owner_name,
            method_name,
        )
    {
        return Some(return_type);
    }

    let owner_type = if through_instance {
        resolve_direct_callable_return_semantic_type(node, nodes, owner_name)
            .or_else(|| Some(SemanticType::Name(owner_name.to_owned())))
    } else {
        resolve_direct_name_reference_semantic_type(
            node,
            nodes,
            signature,
            exclude_name,
            current_owner_name,
            current_owner_type_name,
            current_line,
            owner_name,
        )
        .or_else(|| Some(SemanticType::Name(owner_name.to_owned())))
    }?;

    let owner_type_name = render_semantic_type(&owner_type);
    let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
    let methods = find_owned_callable_declarations(nodes, class_node, class_decl, method_name);
    if methods.is_empty() {
        return None;
    }
    if methods.iter().any(|declaration| declaration.kind == DeclarationKind::Overload)
    {
        let call = node.method_calls.iter().find(|call| {
            call.owner_name == owner_name
                && call.method == method_name
                && call.through_instance == through_instance
                && call.line == current_line
        })?;
        let call = typepython_binding::CallSite {
            callee: format!("{}.{}", class_decl.name, method_name),
            arg_count: call.arg_count,
            arg_types: call.arg_types.clone(),
            arg_values: call.arg_values.clone(),
            starred_arg_types: call.starred_arg_types.clone(),
            starred_arg_values: call.starred_arg_values.clone(),
            keyword_names: call.keyword_names.clone(),
            keyword_arg_types: call.keyword_arg_types.clone(),
            keyword_arg_values: call.keyword_arg_values.clone(),
            keyword_expansion_types: call.keyword_expansion_types.clone(),
            keyword_expansion_values: call.keyword_expansion_values.clone(),
            line: 1,
        };
        let overloads = methods
            .iter()
            .filter(|declaration| declaration.kind == DeclarationKind::Overload)
            .map(|declaration| (*declaration, declaration_callable_semantics(declaration)))
            .collect::<Vec<_>>();
        let applicable = resolve_applicable_method_overload_candidates(
            node,
            nodes,
            &call,
            &owner_type_name,
            &overloads,
        );
        return select_most_specific_overload(node, nodes, &applicable)?.return_type.clone();
    } else {
        let method = *methods.first()?;
        if let Some(call) = node.method_calls.iter().find(|call| {
            call.owner_name == owner_name
                && call.method == method_name
                && call.through_instance == through_instance
                && call.line == current_line
        }) {
            let call = typepython_binding::CallSite {
                callee: format!("{}.{}", class_decl.name, method_name),
                arg_count: call.arg_count,
                arg_types: call.arg_types.clone(),
                arg_values: call.arg_values.clone(),
                starred_arg_types: call.starred_arg_types.clone(),
                starred_arg_values: call.starred_arg_values.clone(),
                keyword_names: call.keyword_names.clone(),
                keyword_arg_types: call.keyword_arg_types.clone(),
                keyword_arg_values: call.keyword_arg_values.clone(),
                keyword_expansion_types: call.keyword_expansion_types.clone(),
                keyword_expansion_values: call.keyword_expansion_values.clone(),
                line: 1,
            };
            if let Some(return_type) = resolve_method_call_candidate_detailed(
                node,
                nodes,
                method,
                &call,
                &owner_type_name,
                declaration_callable_semantics(method).as_ref(),
            )
            .ok()
            .and_then(|resolved| resolved.return_type)
            {
                return Some(return_type);
            }
        }

        let callable = declaration_callable_semantics(method)?;
        let return_text = rewrite_imported_typing_aliases(
            node,
            &callable_return_annotation_text_with_self_from_semantics(&callable, &owner_type_name)?,
        );
        normalized_direct_return_annotation(&return_text).map(lower_type_text_or_name)
    }
}

pub(super) fn unwrap_awaitable_semantic_type(ty: &SemanticType) -> Option<SemanticType> {
    match ty.strip_annotated() {
        SemanticType::Generic { head, args }
            if matches!(
                head.as_str(),
                "Awaitable" | "typing.Awaitable" | "collections.abc.Awaitable"
            ) && args.len() == 1 =>
        {
            Some(args[0].clone())
        }
        SemanticType::Generic { head, args }
            if matches!(
                head.as_str(),
                "Coroutine" | "typing.Coroutine" | "collections.abc.Coroutine"
            ) && args.len() == 3 =>
        {
            Some(args[2].clone())
        }
        _ => None,
    }
}

pub(super) fn unwrap_generator_yield_type(text: &str) -> Option<String> {
    let text = normalize_type_text(text);
    let inner = text.strip_prefix("Generator[").and_then(|inner| inner.strip_suffix(']'))?;
    let args = split_top_level_type_args(inner);
    args.first().map(|arg| normalize_type_text(arg))
}

pub(super) fn unwrap_yield_from_semantic_type(ty: &SemanticType) -> Option<SemanticType> {
    match ty.strip_annotated() {
        SemanticType::Generic { head, args }
            if matches!(
                head.as_str(),
                "Generator"
                    | "typing.Generator"
                    | "collections.abc.Generator"
                    | "Iterator"
                    | "typing.Iterator"
                    | "collections.abc.Iterator"
                    | "Iterable"
                    | "typing.Iterable"
                    | "collections.abc.Iterable"
                    | "Sequence"
                    | "typing.Sequence"
                    | "collections.abc.Sequence"
                    | "list"
                    | "set"
                    | "frozenset"
            ) && !args.is_empty() =>
        {
            Some(args[0].clone())
        }
        SemanticType::Generic { head, args } if head == "tuple" => {
            let expanded = expanded_tuple_shape_semantic_args(args);
            if expanded.len() == 2
                && matches!(&expanded[1], SemanticType::Name(name) if name == "...")
            {
                return Some(expanded[0].clone());
            }
            expanded.first().cloned()
        }
        _ => None,
    }
}

pub(super) fn unwrap_for_iterable_semantic_type(ty: &SemanticType) -> Option<SemanticType> {
    match ty.strip_annotated() {
        SemanticType::Name(name) if name == "range" => Some(SemanticType::Name(String::from("int"))),
        _ => unwrap_yield_from_semantic_type(ty),
    }
}

pub(super) fn find_method_line(
    source: &str,
    owner_type_name: &str,
    method_name: &str,
) -> Option<usize> {
    typepython_syntax::collect_direct_method_signature_sites(source)
        .into_iter()
        .find(|site| site.owner_type_name == owner_type_name && site.name == method_name)
        .map(|site| site.line)
}

pub(super) fn find_function_line(source: &str, function_name: &str) -> Option<usize> {
    typepython_syntax::collect_direct_function_signature_sites(source)
        .into_iter()
        .find(|site| site.name == function_name)
        .map(|site| site.line)
}

pub(super) fn single_line_return_annotation_span(
    source: &str,
    owner_type_name: Option<&str>,
    function_name: &str,
) -> Option<Span> {
    let line = match owner_type_name {
        Some(owner_type_name) => find_method_line(source, owner_type_name, function_name)?,
        None => find_function_line(source, function_name)?,
    };
    let line_text = source.lines().nth(line.saturating_sub(1))?;
    let arrow = line_text.find("->")?;
    let colon = line_text[arrow + 2..].find(':')? + arrow + 2;
    let start_column = arrow
        + 3
        + line_text[arrow + 2..].chars().take_while(|character| character.is_whitespace()).count();
    let end_trimmed = line_text[..colon].trim_end();
    Some(Span::new(String::new(), line, start_column, line, end_trimmed.chars().count() + 1))
}

pub(super) fn override_insertion_span(
    source: &str,
    owner_type_name: &str,
    method_name: &str,
    path: &std::path::Path,
) -> Option<Span> {
    let line = find_method_line(source, owner_type_name, method_name)?;
    let line_text = source.lines().nth(line.saturating_sub(1))?;
    let indent = line_text.chars().take_while(|character| character.is_whitespace()).count() + 1;
    Some(Span::new(path.display().to_string(), line, indent, line, indent))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn attach_missing_none_return_suggestion(
    diagnostic: Diagnostic,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected_text: &str,
    expected: &str,
    actual: &str,
    signature: &str,
) -> Diagnostic {
    let inferred_actual =
        inferred_return_type_for_owner(node, nodes, return_site, expected, signature)
            .unwrap_or_else(|| normalize_type_text(actual));
    if union_branches(expected)
        .is_some_and(|branches| branches.iter().any(|branch| branch == "None"))
        || !union_branches(&inferred_actual)
            .is_some_and(|branches| branches.iter().any(|branch| branch == "None"))
    {
        return diagnostic;
    }
    let Some(without_none) = remove_none_branch(&inferred_actual) else {
        return diagnostic;
    };
    if !direct_type_is_assignable(node, nodes, expected, &without_none) {
        return diagnostic;
    }
    if node.module_path.to_string_lossy().starts_with('<') {
        return diagnostic;
    }
    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return diagnostic;
    };
    let Some(mut span) = single_line_return_annotation_span(
        &source,
        return_site.owner_type_name.as_deref(),
        &return_site.owner_name,
    ) else {
        return diagnostic;
    };
    span.path = node.module_path.display().to_string();
    diagnostic.with_suggestion(
        "Add `| None` to the declared return type",
        span,
        format!("{} | None", expected_text.trim()),
        SuggestionApplicability::MachineApplicable,
    )
}

use super::*;

pub(super) fn parse_python_source(source: SourceFile, options: ParseOptions) -> SyntaxTree {
    let mut statements = Vec::new();
    let mut diagnostics = DiagnosticReport::default();
    let type_ignore_directives = parse_type_ignore_directives(&source.text);

    with_source_line_index(&source.text, || match parse_module(&source.text) {
        Ok(parsed) => {
            collect_invalid_annotation_placement_diagnostics(
                &source.path,
                &source.text,
                parsed.suite(),
                false,
                &mut diagnostics,
            );
            statements.extend(extract_ast_backed_statements(
                &source.path,
                &source.logical_module,
                &source.text,
                &source.text,
                parsed.suite(),
                &[],
                &mut diagnostics,
            ));
            collect_return_statements(&source.text, parsed.suite(), None, None, &mut statements);
            collect_yield_statements(&source.text, parsed.suite(), None, &mut statements);
            collect_if_statements(&source.text, parsed.suite(), None, None, &mut statements);
            statements.extend(collect_guarded_import_statements_from_suite(
                &source.path,
                &source.logical_module,
                &source.text,
                &source.text,
                parsed.suite(),
                options,
                &mut diagnostics,
            ));
            collect_assert_statements(&source.text, parsed.suite(), None, None, &mut statements);
            collect_invalidation_statements(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            collect_match_statements(&source.text, parsed.suite(), None, None, &mut statements);
            collect_for_statements(&source.text, parsed.suite(), None, None, &mut statements);
            collect_with_statements(&source.text, parsed.suite(), None, None, &mut statements);
            collect_except_handler_statements(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            collect_nested_call_statements(&source.text, parsed.suite(), &mut statements);
            collect_nested_method_call_statements(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            collect_nested_member_access_statements(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            collect_function_body_assignments(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            collect_function_body_bare_assignments(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            collect_function_body_namedexpr_assignments(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            statements.sort_by_key(statement_line);
        }
        Err(error) => {
            let code = parse_error_code(&error.error.to_string());
            diagnostics.push(
                Diagnostic::error(code, format!("Python syntax error: {}", error.error)).with_span(
                    parse_error_span(
                        &source.path,
                        &source.text,
                        error.location.start().to_usize(),
                        error.location.end().to_usize(),
                    ),
                ),
            );
        }
    });

    SyntaxTree { source, statements, type_ignore_directives, diagnostics }
}

pub(super) fn parse_typepython_source(source: SourceFile, options: ParseOptions) -> SyntaxTree {
    let mut statements = Vec::new();
    let mut diagnostics = DiagnosticReport::default();
    let type_ignore_directives = parse_type_ignore_directives(&source.text);

    for (index, line) in source.text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(statement) =
            parse_extension_statement(&source.path, trimmed, line_number, &mut diagnostics)
        {
            statements.push(statement);
        }
    }

    if !diagnostics.has_errors() {
        let normalized_typepython_source = normalize_typepython_source(&source.text, &statements);
        let normalized = if options.enable_conditional_returns {
            normalize_conditional_return_source(&normalized_typepython_source)
        } else {
            normalized_typepython_source
        };
        let (normalized, annotated_lambda_sites) = normalize_annotated_lambda_source(&normalized);
        with_active_annotated_lambda_sites(annotated_lambda_sites, || {
            with_source_line_index(&normalized, || match parse_module(&normalized) {
                Ok(parsed) => {
                    collect_invalid_annotation_placement_diagnostics(
                        &source.path,
                        &normalized,
                        parsed.suite(),
                        false,
                        &mut diagnostics,
                    );
                    refresh_custom_statements_from_ast(
                        &source.path,
                        &normalized,
                        parsed.suite(),
                        &mut statements,
                        &mut diagnostics,
                    );
                    let mut guarded_lines = std::collections::BTreeSet::new();
                    let mut selected_guarded_lines = std::collections::BTreeSet::new();
                    collect_supported_guarded_branch_statement_lines(
                        &normalized,
                        parsed.suite(),
                        options,
                        &mut guarded_lines,
                        &mut selected_guarded_lines,
                    );
                    statements.retain(|statement| {
                        !is_custom_guard_filtered_statement(statement)
                            || !guarded_lines.contains(&statement_line(statement))
                            || selected_guarded_lines.contains(&statement_line(statement))
                    });
                    statements.extend(extract_ast_backed_statements(
                        &source.path,
                        &source.logical_module,
                        &normalized,
                        &normalized,
                        parsed.suite(),
                        &statements,
                        &mut diagnostics,
                    ));
                    collect_return_statements(
                        &normalized,
                        parsed.suite(),
                        None,
                        None,
                        &mut statements,
                    );
                    collect_yield_statements(&normalized, parsed.suite(), None, &mut statements);
                    collect_if_statements(&normalized, parsed.suite(), None, None, &mut statements);
                    statements.extend(collect_guarded_import_statements_from_suite(
                        &source.path,
                        &source.logical_module,
                        &normalized,
                        &normalized,
                        parsed.suite(),
                        options,
                        &mut diagnostics,
                    ));
                    collect_assert_statements(
                        &normalized,
                        parsed.suite(),
                        None,
                        None,
                        &mut statements,
                    );
                    collect_invalidation_statements(
                        &normalized,
                        parsed.suite(),
                        None,
                        None,
                        &mut statements,
                    );
                    collect_match_statements(
                        &normalized,
                        parsed.suite(),
                        None,
                        None,
                        &mut statements,
                    );
                    collect_for_statements(
                        &normalized,
                        parsed.suite(),
                        None,
                        None,
                        &mut statements,
                    );
                    collect_with_statements(
                        &normalized,
                        parsed.suite(),
                        None,
                        None,
                        &mut statements,
                    );
                    collect_except_handler_statements(
                        &normalized,
                        parsed.suite(),
                        None,
                        None,
                        &mut statements,
                    );
                    collect_nested_call_statements(&normalized, parsed.suite(), &mut statements);
                    collect_nested_method_call_statements(
                        &normalized,
                        parsed.suite(),
                        None,
                        None,
                        &mut statements,
                    );
                    collect_function_body_assignments(
                        &normalized,
                        parsed.suite(),
                        None,
                        None,
                        &mut statements,
                    );
                    collect_function_body_bare_assignments(
                        &normalized,
                        parsed.suite(),
                        None,
                        None,
                        &mut statements,
                    );
                    collect_function_body_namedexpr_assignments(
                        &normalized,
                        parsed.suite(),
                        None,
                        None,
                        &mut statements,
                    );
                    statements.sort_by_key(statement_line);
                }
                Err(error) => {
                    let code = parse_error_code(&error.error.to_string());
                    diagnostics.push(
                        Diagnostic::error(
                            code,
                            format!("TypePython syntax error: {}", error.error),
                        )
                        .with_span(parse_error_span(
                            &source.path,
                            &source.text,
                            error.location.start().to_usize(),
                            error.location.end().to_usize(),
                        )),
                    );
                }
            });
        });
    }

    SyntaxTree { source, statements, type_ignore_directives, diagnostics }
}

pub(super) fn parse_error_code(message: &str) -> &'static str {
    if matches!(
        message,
        "Invalid assignment target"
            | "Invalid delete target"
            | "Assignment expression target must be an identifier"
    ) {
        "TPY4011"
    } else {
        "TPY2001"
    }
}

pub fn apply_type_ignore_directives(
    syntax_trees: &[SyntaxTree],
    diagnostics: &mut DiagnosticReport,
) {
    let directives_by_path = syntax_trees
        .iter()
        .map(|tree| {
            (tree.source.path.to_string_lossy().into_owned(), tree.type_ignore_directives.clone())
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    diagnostics.diagnostics.retain(|diagnostic| {
        let Some(span) = &diagnostic.span else {
            return true;
        };
        let Some(directives) = directives_by_path.get(&span.path) else {
            return true;
        };
        !directives.iter().any(|directive| {
            directive.line == span.line
                && match &directive.codes {
                    None => true,
                    Some(codes) => codes.iter().any(|code| code == &diagnostic.code),
                }
        })
    });
}

pub(super) fn parse_type_ignore_directives(text: &str) -> Vec<TypeIgnoreDirective> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| parse_type_ignore_directive_line(index + 1, line))
        .collect()
}

pub(super) fn parse_type_ignore_directive_line(line_number: usize, line: &str) -> Option<TypeIgnoreDirective> {
    let (_, comment) = line.split_once('#')?;
    let comment = comment.trim();
    let remainder = comment.strip_prefix("type: ignore")?.trim();
    let codes = if remainder.is_empty() {
        None
    } else {
        let inner = remainder.strip_prefix('[')?.strip_suffix(']')?;
        Some(
            inner
                .split(',')
                .map(str::trim)
                .filter(|code| !code.is_empty())
                .map(str::to_owned)
                .collect(),
        )
    };
    Some(TypeIgnoreDirective { line: line_number, codes })
}

pub(super) fn collect_invalid_annotation_placement_diagnostics(
    path: &Path,
    source: &str,
    suite: &[Stmt],
    in_function_body: bool,
    diagnostics: &mut DiagnosticReport,
) {
    for statement in suite {
        match statement {
            Stmt::AnnAssign(assign)
                if in_function_body && is_classvar_annotation(&assign.annotation) =>
            {
                let line = offset_to_line_column(source, assign.range.start().to_usize()).0;
                diagnostics.push(
                    Diagnostic::error(
                        "TPY4001",
                        "ClassVar[...] is not allowed inside function or method bodies",
                    )
                    .with_span(Span::new(
                        path.display().to_string(),
                        line,
                        1,
                        line,
                        1,
                    )),
                );
            }
            Stmt::FunctionDef(function) => {
                collect_invalid_parameter_annotation_diagnostics(
                    path,
                    source,
                    &function.parameters,
                    diagnostics,
                );
                collect_invalid_annotation_placement_diagnostics(
                    path,
                    source,
                    &function.body,
                    true,
                    diagnostics,
                )
            }
            Stmt::ClassDef(class_def) => collect_invalid_annotation_placement_diagnostics(
                path,
                source,
                &class_def.body,
                false,
                diagnostics,
            ),
            _ => {}
        }
    }
}

pub(super) fn collect_invalid_parameter_annotation_diagnostics(
    path: &Path,
    source: &str,
    parameters: &ruff_python_ast::Parameters,
    diagnostics: &mut DiagnosticReport,
) {
    for parameter in parameters.iter() {
        let Some(annotation) = parameter.annotation() else {
            continue;
        };
        let line = offset_to_line_column(source, parameter.range().start().to_usize()).0;

        if is_classvar_annotation(annotation) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4001",
                    "ClassVar[...] is not allowed in function or method parameter annotations",
                )
                .with_span(Span::new(path.display().to_string(), line, 1, line, 1)),
            );
        }
        if is_final_annotation(annotation) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4010",
                    "Final[...] in function or method parameter position is deferred beyond v1",
                )
                .with_span(Span::new(path.display().to_string(), line, 1, line, 1)),
            );
        }
    }
}

pub(super) fn refresh_custom_statements_from_ast(
    path: &Path,
    normalized: &str,
    suite: &[Stmt],
    statements: &mut [SyntaxStatement],
    diagnostics: &mut DiagnosticReport,
) {
    for statement in statements.iter_mut() {
        match statement {
            SyntaxStatement::Interface(existing) => {
                if let Some(ast_statement) =
                    ast_class_def_for_line(normalized, suite, existing.line)
                {
                    for body_statement in &ast_statement.body {
                        if !is_valid_interface_body_statement(body_statement) {
                            diagnostics.push(
                                Diagnostic::error(
                                    "TPY2001",
                                    format!(
                                        "interface `{}` body must not contain executable statements",
                                        existing.name
                                    ),
                                )
                                .with_span(Span::new(
                                    path.display().to_string(),
                                    offset_to_line_column(
                                        normalized,
                                        body_statement.range().start().to_usize(),
                                    )
                                    .0,
                                    1,
                                    offset_to_line_column(
                                        normalized,
                                        body_statement.range().end().to_usize(),
                                    )
                                    .0,
                                    1,
                                )),
                            );
                            break;
                        }
                    }
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "interface declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.is_final_decorator =
                            ast_statement.decorator_list.iter().any(is_final_decorator);
                        existing.deprecation_message =
                            deprecated_decorator_message(&ast_statement.decorator_list);
                        existing.is_deprecated = existing.deprecation_message.is_some();
                        existing.header_suffix = ast_statement
                            .arguments
                            .as_ref()
                            .and_then(|arguments| slice_range(normalized, arguments.range()))
                            .map(str::to_owned)
                            .unwrap_or_default();
                        existing.bases = ast_statement
                            .arguments
                            .as_ref()
                            .map(|arguments| extract_class_bases(normalized, arguments))
                            .unwrap_or_default();
                        existing.members = extract_class_members(normalized, &ast_statement.body);
                        existing.is_abstract_class = is_abstract_class(existing);
                    }
                }
            }
            SyntaxStatement::DataClass(existing) => {
                if let Some(ast_statement) =
                    ast_class_def_for_line(normalized, suite, existing.line)
                {
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "data class declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.is_final_decorator =
                            ast_statement.decorator_list.iter().any(is_final_decorator);
                        existing.deprecation_message =
                            deprecated_decorator_message(&ast_statement.decorator_list);
                        existing.is_deprecated = existing.deprecation_message.is_some();
                        existing.header_suffix = ast_statement
                            .arguments
                            .as_ref()
                            .and_then(|arguments| slice_range(normalized, arguments.range()))
                            .map(str::to_owned)
                            .unwrap_or_default();
                        existing.bases = ast_statement
                            .arguments
                            .as_ref()
                            .map(|arguments| extract_class_bases(normalized, arguments))
                            .unwrap_or_default();
                        existing.members = extract_class_members(normalized, &ast_statement.body);
                        existing.is_abstract_class = is_abstract_class(existing);
                    }
                }
            }
            SyntaxStatement::SealedClass(existing) => {
                if let Some(ast_statement) =
                    ast_class_def_for_line(normalized, suite, existing.line)
                {
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "sealed class declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.is_final_decorator =
                            ast_statement.decorator_list.iter().any(is_final_decorator);
                        existing.deprecation_message =
                            deprecated_decorator_message(&ast_statement.decorator_list);
                        existing.is_deprecated = existing.deprecation_message.is_some();
                        existing.header_suffix = ast_statement
                            .arguments
                            .as_ref()
                            .and_then(|arguments| slice_range(normalized, arguments.range()))
                            .map(str::to_owned)
                            .unwrap_or_default();
                        existing.bases = ast_statement
                            .arguments
                            .as_ref()
                            .map(|arguments| extract_class_bases(normalized, arguments))
                            .unwrap_or_default();
                        existing.members = extract_class_members(normalized, &ast_statement.body);
                        existing.is_abstract_class = is_abstract_class(existing);
                    }
                }
            }
            SyntaxStatement::OverloadDef(existing) => {
                if let Some(ast_statement) =
                    ast_function_def_for_line(normalized, suite, existing.line)
                {
                    if !is_stub_like_function_body(&ast_statement.body) {
                        diagnostics.push(
                            Diagnostic::error(
                                "TPY2001",
                                format!(
                                    "overload declaration `{}` body must not contain executable statements",
                                    existing.name
                                ),
                            )
                            .with_span(Span::new(
                                path.display().to_string(),
                                offset_to_line_column(
                                    normalized,
                                    ast_statement.range.start().to_usize(),
                                )
                                .0,
                                1,
                                offset_to_line_column(
                                    normalized,
                                    ast_statement.range.end().to_usize(),
                                )
                                .0,
                                1,
                            )),
                        );
                    }
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "overload declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.params =
                            extract_function_params(normalized, &ast_statement.parameters);
                        existing.returns = ast_statement
                            .returns
                            .as_ref()
                            .and_then(|returns| slice_range(normalized, returns.range()))
                            .map(str::to_owned);
                        existing.is_async = ast_statement.is_async;
                        existing.is_override =
                            ast_statement.decorator_list.iter().any(is_override_decorator);
                        existing.deprecation_message =
                            deprecated_decorator_message(&ast_statement.decorator_list);
                        existing.is_deprecated = existing.deprecation_message.is_some();
                    }
                }
            }
            _ => {}
        }
    }
}

pub(super) fn is_valid_interface_body_statement(statement: &Stmt) -> bool {
    match statement {
        Stmt::AnnAssign(_) | Stmt::Pass(_) => true,
        Stmt::Expr(expr) => {
            matches!(expr.value.as_ref(), Expr::StringLiteral(_) | Expr::EllipsisLiteral(_))
        }
        Stmt::FunctionDef(function) => is_stub_like_function_body(&function.body),
        _ => false,
    }
}

pub(super) fn is_stub_like_function_body(body: &[Stmt]) -> bool {
    body.iter().all(|statement| {
        matches!(statement, Stmt::Pass(_))
            || matches!(statement, Stmt::Expr(expr) if matches!(expr.value.as_ref(), Expr::StringLiteral(_) | Expr::EllipsisLiteral(_)))
    })
}

pub(super) fn extract_class_members(normalized: &str, body: &[Stmt]) -> Vec<ClassMember> {
    let mut members = Vec::new();

    for statement in body {
        match statement {
            Stmt::FunctionDef(function) => members.push(ClassMember {
                name: function.name.as_str().to_owned(),
                kind: if function.decorator_list.iter().any(is_overload_decorator) {
                    ClassMemberKind::Overload
                } else {
                    ClassMemberKind::Method
                },
                method_kind: Some(method_kind_from_decorators(&function.decorator_list)),
                annotation: None,
                annotation_expr: None,
                value_type: None,
                params: extract_function_params(normalized, &function.parameters),
                returns: function
                    .returns
                    .as_ref()
                    .and_then(|returns| slice_range(normalized, returns.range()))
                    .map(str::to_owned),
                returns_expr: function
                    .returns
                    .as_ref()
                    .and_then(|returns| slice_range(normalized, returns.range()))
                    .and_then(TypeExpr::parse),
                is_async: function.is_async,
                is_override: function.decorator_list.iter().any(is_override_decorator),
                is_abstract_method: function.decorator_list.iter().any(is_abstractmethod_decorator),
                is_final_decorator: function.decorator_list.iter().any(is_final_decorator),
                deprecation_message: deprecated_decorator_message(&function.decorator_list),
                is_deprecated: deprecated_decorator_message(&function.decorator_list).is_some(),
                is_final: false,
                is_class_var: false,
                line: offset_to_line_column(normalized, function.range.start().to_usize()).0,
            }),
            Stmt::AnnAssign(assign) => {
                let is_final = is_final_annotation(&assign.annotation);
                let is_class_var = is_classvar_annotation(&assign.annotation);
                members.extend(extract_assignment_names(&assign.target).into_iter().map(|name| {
                    ClassMember {
                        name,
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: slice_range(normalized, assign.annotation.range())
                            .map(str::to_owned),
                        annotation_expr: slice_range(normalized, assign.annotation.range())
                            .and_then(TypeExpr::parse),
                        value_type: assign.value.as_deref().map(infer_literal_arg_type),
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final,
                        is_class_var,
                        line: offset_to_line_column(normalized, assign.range.start().to_usize()).0,
                    }
                }));
            }
            Stmt::Assign(assign) => {
                let line = offset_to_line_column(normalized, assign.range.start().to_usize()).0;
                members.extend(assign.targets.iter().flat_map(extract_assignment_names).map(
                    |name| ClassMember {
                        name,
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: None,
                        annotation_expr: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line,
                    },
                ));
            }
            _ => {}
        }
    }

    members
}

pub(super) fn ast_class_def_for_line<'a>(
    normalized: &str,
    suite: &'a [Stmt],
    line: usize,
) -> Option<&'a ruff_python_ast::StmtClassDef> {
    suite.iter().find_map(|stmt| match stmt {
        Stmt::ClassDef(class_def)
            if offset_to_line_column(normalized, class_def.range.start().to_usize()).0 == line =>
        {
            Some(class_def)
        }
        _ => None,
    })
}

pub(super) fn ast_function_def_for_line<'a>(
    normalized: &str,
    suite: &'a [Stmt],
    line: usize,
) -> Option<&'a ruff_python_ast::StmtFunctionDef> {
    suite.iter().find_map(|stmt| match stmt {
        Stmt::FunctionDef(function_def)
            if offset_to_line_column(normalized, function_def.range.start().to_usize()).0
                == line =>
        {
            Some(function_def)
        }
        _ => None,
    })
}

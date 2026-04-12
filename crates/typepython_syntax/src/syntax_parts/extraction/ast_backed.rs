use super::*;

pub(in super::super) fn extract_ast_backed_statements(
    path: &Path,
    current_module_key: &str,
    source: &str,
    normalized: &str,
    suite: &[Stmt],
    existing: &[SyntaxStatement],
    diagnostics: &mut DiagnosticReport,
) -> Vec<SyntaxStatement> {
    let existing_lines: std::collections::BTreeSet<_> =
        existing.iter().map(statement_line).collect();
    let mut statements = Vec::new();

    for stmt in suite {
        let line = offset_to_line_column(normalized, stmt.range().start().to_usize()).0;
        if existing_lines.contains(&line) {
            continue;
        }
        if let Some(statement) = extract_ast_backed_statement(
            path,
            current_module_key,
            source,
            normalized,
            stmt,
            line,
            diagnostics,
        ) {
            statements.push(statement);
        }
        if let Some(call_statement) = extract_supplemental_call_statement(source, stmt, line) {
            statements.push(call_statement);
        }
        if let Some(method_call) = extract_method_call_statement(source, stmt, line, None, None) {
            statements.push(method_call);
        }
        if let Some(member_access) = extract_member_access_statement(source, stmt, line, None, None)
        {
            statements.push(member_access);
        }
    }

    statements
}

pub(in super::super) struct GuardedStatementCollector<'a> {
    path: &'a Path,
    current_module_key: &'a str,
    source: &'a str,
    normalized: &'a str,
    options: ParseOptions,
    diagnostics: &'a mut DiagnosticReport,
}

impl<'a> GuardedStatementCollector<'a> {
    fn collect_from_suite(
        &mut self,
        suite: &[Stmt],
        statements: &mut Vec<SyntaxStatement>,
        include_selected_statements: bool,
    ) {
        for stmt in suite {
            let line = offset_to_line_column(self.normalized, stmt.range().start().to_usize()).0;
            match stmt {
                Stmt::If(if_stmt) => {
                    if let Some(selected_suite) =
                        selected_guarded_import_suite(if_stmt, self.source, self.options)
                    {
                        self.collect_from_suite(selected_suite, statements, true);
                    } else {
                        self.collect_from_suite(
                            &if_stmt.body,
                            statements,
                            include_selected_statements,
                        );
                        for clause in &if_stmt.elif_else_clauses {
                            self.collect_from_suite(
                                &clause.body,
                                statements,
                                include_selected_statements,
                            );
                        }
                    }
                }
                Stmt::Try(try_stmt) => {
                    self.collect_from_suite(
                        &try_stmt.body,
                        statements,
                        include_selected_statements,
                    );
                    for handler in &try_stmt.handlers {
                        let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                        self.collect_from_suite(
                            &handler.body,
                            statements,
                            include_selected_statements,
                        );
                    }
                    self.collect_from_suite(
                        &try_stmt.orelse,
                        statements,
                        include_selected_statements,
                    );
                    self.collect_from_suite(
                        &try_stmt.finalbody,
                        statements,
                        include_selected_statements,
                    );
                }
                _ => {
                    if !include_selected_statements {
                        continue;
                    }
                    let existing_lines: std::collections::BTreeSet<_> =
                        statements.iter().map(statement_line).collect();
                    if existing_lines.contains(&line) {
                        continue;
                    }
                    if let Some(statement) = extract_ast_backed_statement(
                        self.path,
                        self.current_module_key,
                        self.source,
                        self.normalized,
                        stmt,
                        line,
                        self.diagnostics,
                    ) {
                        match statement {
                            SyntaxStatement::Import(_)
                            | SyntaxStatement::ClassDef(_)
                            | SyntaxStatement::FunctionDef(_)
                            | SyntaxStatement::OverloadDef(_) => statements.push(statement),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

pub(in super::super) fn collect_guarded_import_statements_from_suite(
    path: &Path,
    current_module_key: &str,
    source: &str,
    normalized: &str,
    suite: &[Stmt],
    options: ParseOptions,
    diagnostics: &mut DiagnosticReport,
) -> Vec<SyntaxStatement> {
    let mut statements = Vec::new();
    let mut collector = GuardedStatementCollector {
        path,
        current_module_key,
        source,
        normalized,
        options,
        diagnostics,
    };
    collector.collect_from_suite(suite, &mut statements, false);
    statements
}

pub(in super::super) fn selected_guarded_import_suite<'a>(
    stmt: &'a ruff_python_ast::StmtIf,
    source: &str,
    options: ParseOptions,
) -> Option<&'a [Stmt]> {
    if evaluate_guarded_import_expr(source, &stmt.test, options)? {
        return Some(&stmt.body);
    }

    for clause in &stmt.elif_else_clauses {
        match clause.test.as_ref() {
            Some(test) => {
                if evaluate_guarded_import_expr(source, test, options)? {
                    return Some(&clause.body);
                }
            }
            None => return Some(&clause.body),
        }
    }

    Some(&[])
}

pub(in super::super) fn collect_supported_guarded_branch_statement_lines(
    source: &str,
    suite: &[Stmt],
    options: ParseOptions,
    guarded_lines: &mut std::collections::BTreeSet<usize>,
    selected_lines: &mut std::collections::BTreeSet<usize>,
) {
    for stmt in suite {
        match stmt {
            Stmt::If(if_stmt) => {
                if let Some(selected_suite) =
                    selected_guarded_import_suite(if_stmt, source, options)
                {
                    collect_statement_start_lines(source, &if_stmt.body, guarded_lines);
                    for clause in &if_stmt.elif_else_clauses {
                        collect_statement_start_lines(source, &clause.body, guarded_lines);
                    }
                    collect_statement_start_lines(source, selected_suite, selected_lines);
                    collect_supported_guarded_branch_statement_lines(
                        source,
                        selected_suite,
                        options,
                        guarded_lines,
                        selected_lines,
                    );
                    continue;
                }
                collect_supported_guarded_branch_statement_lines(
                    source,
                    &if_stmt.body,
                    options,
                    guarded_lines,
                    selected_lines,
                );
                for clause in &if_stmt.elif_else_clauses {
                    collect_supported_guarded_branch_statement_lines(
                        source,
                        &clause.body,
                        options,
                        guarded_lines,
                        selected_lines,
                    );
                }
            }
            Stmt::Try(try_stmt) => {
                collect_supported_guarded_branch_statement_lines(
                    source,
                    &try_stmt.body,
                    options,
                    guarded_lines,
                    selected_lines,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_supported_guarded_branch_statement_lines(
                        source,
                        &handler.body,
                        options,
                        guarded_lines,
                        selected_lines,
                    );
                }
                collect_supported_guarded_branch_statement_lines(
                    source,
                    &try_stmt.orelse,
                    options,
                    guarded_lines,
                    selected_lines,
                );
                collect_supported_guarded_branch_statement_lines(
                    source,
                    &try_stmt.finalbody,
                    options,
                    guarded_lines,
                    selected_lines,
                );
            }
            Stmt::FunctionDef(function) => collect_supported_guarded_branch_statement_lines(
                source,
                &function.body,
                options,
                guarded_lines,
                selected_lines,
            ),
            Stmt::ClassDef(class_def) => collect_supported_guarded_branch_statement_lines(
                source,
                &class_def.body,
                options,
                guarded_lines,
                selected_lines,
            ),
            _ => {}
        }
    }
}

pub(in super::super) fn collect_statement_start_lines(
    source: &str,
    suite: &[Stmt],
    lines: &mut std::collections::BTreeSet<usize>,
) {
    for stmt in suite {
        lines.insert(offset_to_line_column(source, stmt.range().start().to_usize()).0);
        for_each_nested_suite(stmt, |nested| collect_statement_start_lines(source, nested, lines));
    }
}

pub(in super::super) fn is_custom_guard_filtered_statement(statement: &SyntaxStatement) -> bool {
    matches!(
        statement,
        SyntaxStatement::TypeAlias(_)
            | SyntaxStatement::Interface(_)
            | SyntaxStatement::DataClass(_)
            | SyntaxStatement::SealedClass(_)
            | SyntaxStatement::OverloadDef(_)
            | SyntaxStatement::Unsafe(_)
    )
}

pub(in super::super) fn evaluate_guarded_import_expr(
    source: &str,
    expr: &Expr,
    options: ParseOptions,
) -> Option<bool> {
    if is_type_checking_guard_expr(expr) {
        return Some(true);
    }
    evaluate_version_guard_expr(expr, options)
        .or_else(|| evaluate_platform_guard_expr(source, expr, options))
}

pub(in super::super) fn is_type_checking_guard_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "TYPE_CHECKING",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "TYPE_CHECKING"
                && matches!(
                    attribute.value.as_ref(),
                    Expr::Name(name)
                        if matches!(name.id.as_str(), "typing" | "typing_extensions")
                )
        }
        _ => false,
    }
}

pub(in super::super) fn evaluate_version_guard_expr(
    expr: &Expr,
    options: ParseOptions,
) -> Option<bool> {
    let target = options.target_python?;
    let Expr::Compare(compare) = expr else {
        return None;
    };
    if compare.ops.len() != 1 || compare.comparators.len() != 1 {
        return None;
    }
    let Expr::Attribute(attribute) = compare.left.as_ref() else {
        return None;
    };
    if attribute.attr.as_str() != "version_info" {
        return None;
    }
    let Expr::Name(module) = attribute.value.as_ref() else {
        return None;
    };
    if module.id.as_str() != "sys" {
        return None;
    }
    let expected = parse_guarded_python_version(compare.comparators.first()?)?;
    let op = compare.ops.first()?;
    Some(match op {
        ruff_python_ast::CmpOp::Eq => target == expected,
        ruff_python_ast::CmpOp::NotEq => target != expected,
        ruff_python_ast::CmpOp::Lt => target < expected,
        ruff_python_ast::CmpOp::LtE => target <= expected,
        ruff_python_ast::CmpOp::Gt => target > expected,
        ruff_python_ast::CmpOp::GtE => target >= expected,
        _ => return None,
    })
}

pub(in super::super) fn parse_guarded_python_version(expr: &Expr) -> Option<ParsePythonVersion> {
    match expr {
        Expr::Tuple(tuple) => {
            let major_number = &tuple.elts.first()?.as_number_literal_expr()?.value;
            let major = parse_small_python_version_component(major_number)?;
            let minor = tuple
                .elts
                .get(1)
                .and_then(|expr| expr.as_number_literal_expr())
                .and_then(|number| parse_small_python_version_component(&number.value))
                .unwrap_or(0);
            Some(ParsePythonVersion { major, minor })
        }
        Expr::NumberLiteral(number) => Some(ParsePythonVersion {
            major: parse_small_python_version_component(&number.value)?,
            minor: 0,
        }),
        _ => None,
    }
}

pub(in super::super) fn parse_small_python_version_component(
    number: &ruff_python_ast::Number,
) -> Option<u8> {
    match number {
        ruff_python_ast::Number::Int(value) => value.as_u8(),
        _ => None,
    }
}

pub(in super::super) fn evaluate_platform_guard_expr(
    source: &str,
    expr: &Expr,
    options: ParseOptions,
) -> Option<bool> {
    let platform = options.target_platform?;
    let Expr::Compare(compare) = expr else {
        return None;
    };
    if compare.ops.len() != 1 || compare.comparators.len() != 1 {
        return None;
    }
    let Expr::Attribute(attribute) = compare.left.as_ref() else {
        return None;
    };
    if attribute.attr.as_str() != "platform" {
        return None;
    }
    let Expr::Name(module) = attribute.value.as_ref() else {
        return None;
    };
    if module.id.as_str() != "sys" {
        return None;
    }
    let comparator = compare.comparators.first()?;
    let Expr::StringLiteral(_) = comparator else {
        return None;
    };
    let expected = extract_string_literal_value(source, comparator)?;
    let op = compare.ops.first()?;
    Some(match op {
        ruff_python_ast::CmpOp::Eq => platform.sys_platform_name() == expected,
        ruff_python_ast::CmpOp::NotEq => platform.sys_platform_name() != expected,
        _ => return None,
    })
}

pub(in super::super) fn extract_ast_backed_statement(
    path: &Path,
    current_module_key: &str,
    source: &str,
    normalized: &str,
    stmt: &Stmt,
    line: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    match stmt {
        Stmt::ClassDef(stmt) => {
            let deprecation_message = deprecated_decorator_message(&stmt.decorator_list);
            let mut statement = NamedBlockStatement {
                name: stmt.name.as_str().to_owned(),
                type_params: extract_ast_type_params(
                    path,
                    source,
                    stmt.type_params.as_deref(),
                    line,
                    "class declaration",
                    diagnostics,
                )?,
                header_suffix: stmt
                    .arguments
                    .as_ref()
                    .and_then(|arguments| slice_range(source, arguments.range()))
                    .map(str::to_owned)
                    .unwrap_or_default(),
                bases: stmt
                    .arguments
                    .as_ref()
                    .map(|arguments| extract_class_bases(source, arguments))
                    .unwrap_or_default(),
                is_final_decorator: stmt.decorator_list.iter().any(is_final_decorator),
                is_deprecated: deprecation_message.is_some(),
                deprecation_message,
                members: extract_class_members(normalized, &stmt.body),
                is_abstract_class: false,
                line,
            };
            statement.is_abstract_class = is_abstract_class(&statement);
            Some(SyntaxStatement::ClassDef(statement))
        }
        Stmt::FunctionDef(stmt) => {
            let is_overload = stmt.decorator_list.iter().any(is_overload_decorator);
            let deprecation_message = deprecated_decorator_message(&stmt.decorator_list);
            let statement = FunctionStatement {
                name: stmt.name.as_str().to_owned(),
                type_params: extract_ast_type_params(
                    path,
                    source,
                    stmt.type_params.as_deref(),
                    line,
                    if is_overload { "overload declaration" } else { "function declaration" },
                    diagnostics,
                )?,
                params: extract_function_params(source, &stmt.parameters),
                returns: stmt
                    .returns
                    .as_ref()
                    .and_then(|returns| slice_range(source, returns.range()))
                    .map(str::to_owned),
                returns_expr: stmt
                    .returns
                    .as_ref()
                    .and_then(|returns| slice_range(source, returns.range()))
                    .and_then(TypeExpr::parse),
                is_async: stmt.is_async,
                is_override: stmt.decorator_list.iter().any(is_override_decorator),
                is_deprecated: deprecation_message.is_some(),
                deprecation_message,
                line,
            };

            Some(if is_overload {
                SyntaxStatement::OverloadDef(statement)
            } else {
                SyntaxStatement::FunctionDef(statement)
            })
        }
        Stmt::Import(stmt) => {
            let bindings = stmt
                .names
                .iter()
                .map(|alias| ImportBinding {
                    local_name: alias
                        .asname
                        .as_ref()
                        .map(|identifier| identifier.as_str().to_owned())
                        .unwrap_or_else(|| {
                            alias
                                .name
                                .as_str()
                                .split('.')
                                .next()
                                .unwrap_or(alias.name.as_str())
                                .to_owned()
                        }),
                    source_path: alias.name.as_str().to_owned(),
                })
                .collect::<Vec<_>>();
            (!bindings.is_empty())
                .then_some(SyntaxStatement::Import(ImportStatement { bindings, line }))
        }
        Stmt::ImportFrom(stmt) => {
            let bindings = stmt
                .names
                .iter()
                .map(|alias| {
                    let imported_name = alias.name.as_str();
                    let module = stmt.module.as_ref().map(|id| id.as_str()).unwrap_or("");
                    let module =
                        normalize_import_module(path, current_module_key, stmt.level, module);
                    ImportBinding {
                        local_name: alias
                            .asname
                            .as_ref()
                            .unwrap_or(&alias.name)
                            .as_str()
                            .to_owned(),
                        source_path: if module.is_empty() {
                            imported_name.to_owned()
                        } else {
                            format!("{module}.{imported_name}")
                        },
                    }
                })
                .collect::<Vec<_>>();
            (!bindings.is_empty())
                .then_some(SyntaxStatement::Import(ImportStatement { bindings, line }))
        }
        Stmt::Assign(stmt) => {
            let destructuring_target_names = (stmt.targets.len() == 1)
                .then(|| extract_simple_destructuring_target_names(&stmt.targets[0]))
                .flatten();
            let names = destructuring_target_names.clone().unwrap_or_else(|| {
                stmt.targets.iter().flat_map(extract_assignment_names).collect::<Vec<_>>()
            });
            if !names.is_empty() {
                let value = extract_direct_expr_metadata(source, &stmt.value);
                Some(SyntaxStatement::Value(ValueStatement {
                    names,
                    destructuring_target_names,
                    annotation: None,
                    annotation_expr: None,
                    value_type: value.value_type,
                    value_type_expr: value.value_type_expr,
                    is_awaited: value.is_awaited,
                    value_callee: value.value_callee,
                    value_name: value.value_name,
                    value_member_owner_name: value.value_member_owner_name,
                    value_member_name: value.value_member_name,
                    value_member_through_instance: value.value_member_through_instance,
                    value_method_owner_name: value.value_method_owner_name,
                    value_method_name: value.value_method_name,
                    value_method_through_instance: value.value_method_through_instance,
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
                    value_lambda: value.value_lambda,
                    value_list_comprehension: value.value_list_comprehension,
                    value_generator_comprehension: value.value_generator_comprehension,
                    value_list_elements: value.value_list_elements,
                    value_set_elements: value.value_set_elements,
                    value_dict_entries: value.value_dict_entries,
                    owner_name: None,
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line,
                }))
            } else {
                None
            }
        }
        Stmt::AnnAssign(stmt) => {
            let names = extract_assignment_names(&stmt.target);
            if !names.is_empty() {
                let value = stmt
                    .value
                    .as_deref()
                    .map(|expr| extract_direct_expr_metadata(source, expr))
                    .unwrap_or(DirectExprMetadata {
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
                    });
                Some(SyntaxStatement::Value(ValueStatement {
                    names,
                    destructuring_target_names: None,
                    annotation: slice_range(source, stmt.annotation.range()).map(str::to_owned),
                    annotation_expr: slice_range(source, stmt.annotation.range())
                        .and_then(TypeExpr::parse),
                    value_type: value.value_type,
                    value_type_expr: value.value_type_expr,
                    is_awaited: value.is_awaited,
                    value_callee: value.value_callee,
                    value_name: value.value_name,
                    value_member_owner_name: value.value_member_owner_name,
                    value_member_name: value.value_member_name,
                    value_member_through_instance: value.value_member_through_instance,
                    value_method_owner_name: value.value_method_owner_name,
                    value_method_name: value.value_method_name,
                    value_method_through_instance: value.value_method_through_instance,
                    value_subscript_target: value.value_subscript_target,
                    value_subscript_string_key: value.value_subscript_string_key,
                    value_subscript_index: value.value_subscript_index,
                    value_if_true: value.value_if_true,
                    value_if_false: value.value_if_false,
                    value_if_guard: value.value_if_guard,
                    value_bool_left: value.value_bool_left,
                    value_bool_right: value.value_bool_right,
                    value_binop_left: value.value_binop_left,
                    value_binop_right: value.value_binop_right,
                    value_binop_operator: value.value_binop_operator,
                    value_lambda: value.value_lambda,
                    value_list_comprehension: value.value_list_comprehension,
                    value_generator_comprehension: value.value_generator_comprehension,
                    value_list_elements: value.value_list_elements,
                    value_set_elements: value.value_set_elements,
                    value_dict_entries: value.value_dict_entries,
                    owner_name: None,
                    owner_type_name: None,
                    is_final: is_final_annotation(&stmt.annotation),
                    is_class_var: is_classvar_annotation(&stmt.annotation),
                    line,
                }))
            } else {
                None
            }
        }
        Stmt::AugAssign(stmt) => extract_augmented_assignment_value_statement(
            source,
            &stmt.target,
            &stmt.value,
            stmt.op,
            None,
            None,
            line,
        ),
        Stmt::If(stmt) => Some(SyntaxStatement::If(IfStatement {
            owner_name: None,
            owner_type_name: None,
            guard: extract_guard_condition(source, &stmt.test),
            line,
            true_start_line: suite_start_line(source, &stmt.body),
            true_end_line: suite_end_line(source, &stmt.body),
            false_start_line: if_false_start_line(source, stmt),
            false_end_line: if_false_end_line(source, stmt),
        })),
        Stmt::Assert(stmt) => Some(SyntaxStatement::Assert(AssertStatement {
            owner_name: None,
            owner_type_name: None,
            guard: extract_guard_condition(source, &stmt.test),
            line,
        })),
        Stmt::Delete(stmt) => {
            let names = stmt.targets.iter().flat_map(extract_assignment_names).collect::<Vec<_>>();
            (!names.is_empty()).then_some(SyntaxStatement::Invalidate(InvalidationStatement {
                kind: InvalidationKind::Delete,
                owner_name: None,
                owner_type_name: None,
                names,
                line,
            }))
        }
        Stmt::Global(stmt) => {
            let names = stmt.names.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>();
            (!names.is_empty()).then_some(SyntaxStatement::Invalidate(InvalidationStatement {
                kind: InvalidationKind::ScopeChange,
                owner_name: None,
                owner_type_name: None,
                names,
                line,
            }))
        }
        Stmt::Nonlocal(stmt) => {
            let names = stmt.names.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>();
            (!names.is_empty()).then_some(SyntaxStatement::Invalidate(InvalidationStatement {
                kind: InvalidationKind::ScopeChange,
                owner_name: None,
                owner_type_name: None,
                names,
                line,
            }))
        }
        Stmt::Expr(stmt) => extract_call_statement(source, &stmt.value, line),
        _ => None,
    }
}

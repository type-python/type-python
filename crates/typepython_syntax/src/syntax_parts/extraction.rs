fn extract_ast_backed_statements(
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
        if let Some(method_call) = extract_method_call_statement(source, stmt, line) {
            statements.push(method_call);
        }
        if let Some(member_access) = extract_member_access_statement(source, stmt, line, None, None)
        {
            statements.push(member_access);
        }
    }

    statements
}

fn collect_guarded_import_statements_from_suite(
    path: &Path,
    current_module_key: &str,
    source: &str,
    normalized: &str,
    suite: &[Stmt],
    options: ParseOptions,
    statements: &mut Vec<SyntaxStatement>,
    diagnostics: &mut DiagnosticReport,
) {
    for stmt in suite {
        let line = offset_to_line_column(normalized, stmt.range().start().to_usize()).0;
        match stmt {
            Stmt::If(if_stmt) => {
                if let Some(selected_suite) = selected_guarded_import_suite(if_stmt, source, options) {
                    collect_guarded_import_statements_from_suite(
                        path,
                        current_module_key,
                        source,
                        normalized,
                        selected_suite,
                        options,
                        statements,
                        diagnostics,
                    );
                }
            }
            Stmt::Try(try_stmt) => {
                collect_guarded_import_statements_from_suite(
                    path,
                    current_module_key,
                    source,
                    normalized,
                    &try_stmt.body,
                    options,
                    statements,
                    diagnostics,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_guarded_import_statements_from_suite(
                        path,
                        current_module_key,
                        source,
                        normalized,
                        &handler.body,
                        options,
                        statements,
                        diagnostics,
                    );
                }
                collect_guarded_import_statements_from_suite(
                    path,
                    current_module_key,
                    source,
                    normalized,
                    &try_stmt.orelse,
                    options,
                    statements,
                    diagnostics,
                );
                collect_guarded_import_statements_from_suite(
                    path,
                    current_module_key,
                    source,
                    normalized,
                    &try_stmt.finalbody,
                    options,
                    statements,
                    diagnostics,
                );
            }
            _ => {
                let existing_lines: std::collections::BTreeSet<_> =
                    statements.iter().map(statement_line).collect();
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

fn selected_guarded_import_suite<'a>(
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

fn collect_supported_guarded_branch_statement_lines(
    source: &str,
    suite: &[Stmt],
    options: ParseOptions,
    guarded_lines: &mut std::collections::BTreeSet<usize>,
    selected_lines: &mut std::collections::BTreeSet<usize>,
) {
    for stmt in suite {
        match stmt {
            Stmt::If(if_stmt) => {
                if let Some(selected_suite) = selected_guarded_import_suite(if_stmt, source, options) {
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

fn collect_statement_start_lines(
    source: &str,
    suite: &[Stmt],
    lines: &mut std::collections::BTreeSet<usize>,
) {
    for stmt in suite {
        lines.insert(offset_to_line_column(source, stmt.range().start().to_usize()).0);
        for_each_nested_suite(stmt, |nested| collect_statement_start_lines(source, nested, lines));
    }
}

fn is_custom_guard_filtered_statement(statement: &SyntaxStatement) -> bool {
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

fn evaluate_guarded_import_expr(source: &str, expr: &Expr, options: ParseOptions) -> Option<bool> {
    if is_type_checking_guard_expr(expr) {
        return Some(true);
    }
    evaluate_version_guard_expr(expr, options)
        .or_else(|| evaluate_platform_guard_expr(source, expr, options))
}

fn is_type_checking_guard_expr(expr: &Expr) -> bool {
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

fn evaluate_version_guard_expr(expr: &Expr, options: ParseOptions) -> Option<bool> {
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

fn parse_guarded_python_version(expr: &Expr) -> Option<ParsePythonVersion> {
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
        Expr::NumberLiteral(number) => {
            Some(ParsePythonVersion {
                major: parse_small_python_version_component(&number.value)?,
                minor: 0,
            })
        }
        _ => None,
    }
}

fn parse_small_python_version_component(number: &ruff_python_ast::Number) -> Option<u8> {
    match number {
        ruff_python_ast::Number::Int(value) => value.as_u8(),
        _ => None,
    }
}

fn evaluate_platform_guard_expr(source: &str, expr: &Expr, options: ParseOptions) -> Option<bool> {
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

fn extract_ast_backed_statement(
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
                    .unwrap_or(DirectExprMetadata { value_type: None, value_type_expr: None, is_awaited: false,
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
                    value_dict_entries: None, });
                Some(SyntaxStatement::Value(ValueStatement {
                    names,
                    destructuring_target_names: None,
                    annotation: slice_range(source, stmt.annotation.range()).map(str::to_owned),
                    annotation_expr: slice_range(source, stmt.annotation.range()).and_then(TypeExpr::parse),
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

fn extract_guard_condition(source: &str, expr: &Expr) -> Option<GuardCondition> {
    match expr {
        Expr::UnaryOp(expr) if expr.op == ruff_python_ast::UnaryOp::Not => {
            extract_guard_condition(source, &expr.operand)
                .map(|guard| GuardCondition::Not(Box::new(guard)))
        }
        Expr::BoolOp(expr) => {
            let conditions = expr
                .values
                .iter()
                .map(|value| extract_guard_condition(source, value))
                .collect::<Option<Vec<_>>>()?;
            match expr.op {
                ruff_python_ast::BoolOp::And => Some(GuardCondition::And(conditions)),
                ruff_python_ast::BoolOp::Or => Some(GuardCondition::Or(conditions)),
            }
        }
        Expr::Name(name) => Some(GuardCondition::TruthyName { name: name.id.as_str().to_owned() }),
        Expr::Compare(compare) if compare.ops.len() == 1 && compare.comparators.len() == 1 => {
            let Expr::Name(name) = compare.left.as_ref() else {
                return None;
            };
            let right = compare.comparators.first()?;
            match (compare.ops.first()?, right) {
                (ruff_python_ast::CmpOp::Is, Expr::NoneLiteral(_)) => {
                    Some(GuardCondition::IsNone {
                        name: name.id.as_str().to_owned(),
                        negated: false,
                    })
                }
                (ruff_python_ast::CmpOp::IsNot, Expr::NoneLiteral(_)) => {
                    Some(GuardCondition::IsNone {
                        name: name.id.as_str().to_owned(),
                        negated: true,
                    })
                }
                _ => None,
            }
        }
        Expr::Call(call) => {
            let Expr::Name(callee) = call.func.as_ref() else {
                return None;
            };
            if call.arguments.args.is_empty() {
                return None;
            }
            let Expr::Name(name) = &call.arguments.args[0] else {
                return None;
            };
            if callee.id.as_str() != "isinstance" {
                return Some(GuardCondition::PredicateCall {
                    name: name.id.as_str().to_owned(),
                    callee: callee.id.as_str().to_owned(),
                });
            }
            if call.arguments.args.len() != 2 {
                return None;
            }
            let guard_types = match &call.arguments.args[1] {
                Expr::Tuple(tuple) => tuple
                    .elts
                    .iter()
                    .filter_map(|elt| slice_range(source, elt.range()).map(str::to_owned))
                    .collect::<Vec<_>>(),
                other => slice_range(source, other.range())
                    .map(|text| vec![text.to_owned()])
                    .unwrap_or_default(),
            };
            (!guard_types.is_empty()).then_some(GuardCondition::IsInstance {
                name: name.id.as_str().to_owned(),
                types: guard_types,
            })
        }
        _ => None,
    }
}

fn suite_start_line(source: &str, suite: &[Stmt]) -> usize {
    suite_start_line_optional(source, suite).unwrap_or(0)
}

fn suite_end_line(source: &str, suite: &[Stmt]) -> usize {
    suite_end_line_optional(source, suite).unwrap_or(0)
}

fn suite_start_line_optional(source: &str, suite: &[Stmt]) -> Option<usize> {
    suite.first().map(|stmt| offset_to_line_column(source, stmt.range().start().to_usize()).0)
}

fn suite_end_line_optional(source: &str, suite: &[Stmt]) -> Option<usize> {
    suite.last().map(|stmt| offset_to_line_column(source, stmt.range().end().to_usize()).0)
}

fn if_false_start_line(source: &str, stmt: &ruff_python_ast::StmtIf) -> Option<usize> {
    stmt.elif_else_clauses
        .first()
        .and_then(|clause| suite_start_line_optional(source, &clause.body))
}

fn if_false_end_line(source: &str, stmt: &ruff_python_ast::StmtIf) -> Option<usize> {
    stmt.elif_else_clauses.last().and_then(|clause| suite_end_line_optional(source, &clause.body))
}

fn for_each_if_false_suite(stmt: &ruff_python_ast::StmtIf, mut callback: impl FnMut(&[Stmt])) {
    for clause in &stmt.elif_else_clauses {
        callback(&clause.body);
    }
}

fn for_each_nested_suite(stmt: &Stmt, mut callback: impl FnMut(&[Stmt])) {
    match stmt {
        Stmt::If(if_stmt) => {
            callback(&if_stmt.body);
            for_each_if_false_suite(if_stmt, |suite| callback(suite));
        }
        Stmt::Try(try_stmt) => {
            callback(&try_stmt.body);
            for handler in &try_stmt.handlers {
                let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                callback(&handler.body);
            }
            callback(&try_stmt.orelse);
            callback(&try_stmt.finalbody);
        }
        Stmt::Match(match_stmt) => {
            for case in &match_stmt.cases {
                callback(&case.body);
            }
        }
        Stmt::For(for_stmt) => {
            callback(&for_stmt.body);
            callback(&for_stmt.orelse);
        }
        Stmt::While(while_stmt) => {
            callback(&while_stmt.body);
            callback(&while_stmt.orelse);
        }
        Stmt::With(with_stmt) => callback(&with_stmt.body),
        _ => {}
    }
}

fn extract_call_statement(source: &str, expr: &Expr, line: usize) -> Option<SyntaxStatement> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Name(name) = call.func.as_ref() else {
        return None;
    };

    Some(SyntaxStatement::Call(CallStatement {
        callee: name.id.as_str().to_owned(),
        arg_count: call
            .arguments
            .args
            .iter()
            .filter(|expr| !matches!(expr, Expr::Starred(_)))
            .count(),
        arg_types: call
            .arguments
            .args
            .iter()
            .filter(|expr| !matches!(expr, Expr::Starred(_)))
            .map(infer_literal_arg_type)
            .collect(),
        arg_values: call
            .arguments
            .args
            .iter()
            .filter(|expr| !matches!(expr, Expr::Starred(_)))
            .map(|expr| extract_direct_expr_metadata(source, expr))
            .collect(),
        starred_arg_types: call
            .arguments
            .args
            .iter()
            .filter_map(|expr| match expr {
                Expr::Starred(starred) => Some(infer_literal_arg_type(&starred.value)),
                _ => None,
            })
            .collect(),
        starred_arg_values: call
            .arguments
            .args
            .iter()
            .filter_map(|expr| match expr {
                Expr::Starred(starred) => {
                    Some(extract_direct_expr_metadata(source, &starred.value))
                }
                _ => None,
            })
            .collect(),
        keyword_names: call
            .arguments
            .keywords
            .iter()
            .filter_map(|keyword| keyword.arg.as_ref().map(|name| name.as_str().to_owned()))
            .collect(),
        keyword_arg_types: call
            .arguments
            .keywords
            .iter()
            .filter(|keyword| keyword.arg.is_some())
            .map(|keyword| infer_literal_arg_type(&keyword.value))
            .collect(),
        keyword_arg_values: call
            .arguments
            .keywords
            .iter()
            .filter(|keyword| keyword.arg.is_some())
            .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
            .collect(),
        keyword_expansion_types: call
            .arguments
            .keywords
            .iter()
            .filter(|keyword| keyword.arg.is_none())
            .map(|keyword| infer_literal_arg_type(&keyword.value))
            .collect(),
        keyword_expansion_values: call
            .arguments
            .keywords
            .iter()
            .filter(|keyword| keyword.arg.is_none())
            .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
            .collect(),
        line,
    }))
}

fn extract_method_call_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
) -> Option<SyntaxStatement> {
    let expr = match stmt {
        Stmt::Expr(expr) => expr.value.as_ref(),
        Stmt::Assign(assign) => &assign.value,
        Stmt::AnnAssign(assign) => assign.value.as_deref()?,
        Stmt::Return(ret) => ret.value.as_deref()?,
        _ => return None,
    };

    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Attribute(attribute) = call.func.as_ref() else {
        return None;
    };

    match attribute.value.as_ref() {
        Expr::Name(name) => Some(SyntaxStatement::MethodCall(MethodCallStatement {
            owner_name: name.id.as_str().to_owned(),
            method: attribute.attr.as_str().to_owned(),
            through_instance: false,
            arg_count: call
                .arguments
                .args
                .iter()
                .filter(|expr| !matches!(expr, Expr::Starred(_)))
                .count(),
            arg_types: call
                .arguments
                .args
                .iter()
                .filter(|expr| !matches!(expr, Expr::Starred(_)))
                .map(infer_literal_arg_type)
                .collect(),
            arg_values: call
                .arguments
                .args
                .iter()
                .filter(|expr| !matches!(expr, Expr::Starred(_)))
                .map(|expr| extract_direct_expr_metadata(source, expr))
                .collect(),
            starred_arg_types: call
                .arguments
                .args
                .iter()
                .filter_map(|expr| match expr {
                    Expr::Starred(starred) => Some(infer_literal_arg_type(&starred.value)),
                    _ => None,
                })
                .collect(),
            starred_arg_values: call
                .arguments
                .args
                .iter()
                .filter_map(|expr| match expr {
                    Expr::Starred(starred) => {
                        Some(extract_direct_expr_metadata(source, &starred.value))
                    }
                    _ => None,
                })
                .collect(),
            keyword_names: call
                .arguments
                .keywords
                .iter()
                .filter_map(|keyword| keyword.arg.as_ref().map(|name| name.as_str().to_owned()))
                .collect(),
            keyword_arg_types: call
                .arguments
                .keywords
                .iter()
                .filter(|keyword| keyword.arg.is_some())
                .map(|keyword| infer_literal_arg_type(&keyword.value))
                .collect(),
            keyword_arg_values: call
                .arguments
                .keywords
                .iter()
                .filter(|keyword| keyword.arg.is_some())
                .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
                .collect(),
            keyword_expansion_types: call
                .arguments
                .keywords
                .iter()
                .filter(|keyword| keyword.arg.is_none())
                .map(|keyword| infer_literal_arg_type(&keyword.value))
                .collect(),
            keyword_expansion_values: call
                .arguments
                .keywords
                .iter()
                .filter(|keyword| keyword.arg.is_none())
                .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
                .collect(),
            line,
        })),
        Expr::Call(inner_call) => {
            let Expr::Name(name) = inner_call.func.as_ref() else {
                return None;
            };
            Some(SyntaxStatement::MethodCall(MethodCallStatement {
                owner_name: name.id.as_str().to_owned(),
                method: attribute.attr.as_str().to_owned(),
                through_instance: true,
                arg_count: call
                    .arguments
                    .args
                    .iter()
                    .filter(|expr| !matches!(expr, Expr::Starred(_)))
                    .count(),
                arg_types: call
                    .arguments
                    .args
                    .iter()
                    .filter(|expr| !matches!(expr, Expr::Starred(_)))
                    .map(infer_literal_arg_type)
                    .collect(),
                arg_values: call
                    .arguments
                    .args
                    .iter()
                    .filter(|expr| !matches!(expr, Expr::Starred(_)))
                    .map(|expr| extract_direct_expr_metadata(source, expr))
                    .collect(),
                starred_arg_types: call
                    .arguments
                    .args
                    .iter()
                    .filter_map(|expr| match expr {
                        Expr::Starred(starred) => Some(infer_literal_arg_type(&starred.value)),
                        _ => None,
                    })
                    .collect(),
                starred_arg_values: call
                    .arguments
                    .args
                    .iter()
                    .filter_map(|expr| match expr {
                        Expr::Starred(starred) => {
                            Some(extract_direct_expr_metadata(source, &starred.value))
                        }
                        _ => None,
                    })
                    .collect(),
                keyword_names: call
                    .arguments
                    .keywords
                    .iter()
                    .filter_map(|keyword| keyword.arg.as_ref().map(|name| name.as_str().to_owned()))
                    .collect(),
                keyword_arg_types: call
                    .arguments
                    .keywords
                    .iter()
                    .filter(|keyword| keyword.arg.is_some())
                    .map(|keyword| infer_literal_arg_type(&keyword.value))
                    .collect(),
                keyword_arg_values: call
                    .arguments
                    .keywords
                    .iter()
                    .filter(|keyword| keyword.arg.is_some())
                    .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
                    .collect(),
                keyword_expansion_types: call
                    .arguments
                    .keywords
                    .iter()
                    .filter(|keyword| keyword.arg.is_none())
                    .map(|keyword| infer_literal_arg_type(&keyword.value))
                    .collect(),
                keyword_expansion_values: call
                    .arguments
                    .keywords
                    .iter()
                    .filter(|keyword| keyword.arg.is_none())
                    .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
                    .collect(),
                line,
            }))
        }
        _ => None,
    }
}

fn collect_nested_method_call_statements(
    source: &str,
    suite: &[Stmt],
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_nested_method_call_statements(source, &function.body, statements);
            }
            Stmt::ClassDef(class_def) => {
                collect_nested_method_call_statements(source, &class_def.body, statements);
            }
            _ => {
                for_each_nested_suite(stmt, |nested_suite| {
                    collect_nested_method_call_statements(source, nested_suite, statements);
                });
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if let Some(method_call) = extract_method_call_statement(source, stmt, line) {
                    statements.push(method_call);
                }
            }
        }
    }
}

fn infer_literal_arg_type(expr: &Expr) -> String {
    infer_direct_literal_type(expr).unwrap_or_default()
}

fn infer_direct_literal_type(expr: &Expr) -> Option<String> {
    match expr {
        Expr::NumberLiteral(_) => Some(String::from("int")),
        Expr::StringLiteral(_) => Some(String::from("str")),
        Expr::BooleanLiteral(_) => Some(String::from("bool")),
        Expr::NoneLiteral(_) => Some(String::from("None")),
        Expr::Compare(_) => Some(String::from("bool")),
        Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::Not => {
            Some(String::from("bool"))
        }
        Expr::BoolOp(bool_op) => {
            let mut types =
                bool_op.values.iter().map(infer_direct_literal_type).collect::<Option<Vec<_>>>()?;
            if types.is_empty() {
                None
            } else {
                Some(
                    if bool_op.op == ruff_python_ast::BoolOp::And
                        || bool_op.op == ruff_python_ast::BoolOp::Or
                    {
                        join_union_literal_type_candidates(types)
                    } else {
                        types.remove(0)
                    },
                )
            }
        }
        Expr::BinOp(bin_op) => infer_direct_binop_type(bin_op),
        Expr::List(list) => {
            let element_types =
                list.elts.iter().map(infer_direct_literal_type).collect::<Option<Vec<_>>>()?;
            Some(format!("list[{}]", join_literal_type_candidates(element_types)))
        }
        Expr::Tuple(tuple) => {
            let element_types =
                tuple.elts.iter().map(infer_direct_literal_type).collect::<Option<Vec<_>>>()?;
            Some(if element_types.is_empty() {
                String::from("tuple[()]")
            } else {
                format!("tuple[{}]", element_types.join(", "))
            })
        }
        Expr::Set(set) => {
            let element_types =
                set.elts.iter().map(infer_direct_literal_type).collect::<Option<Vec<_>>>()?;
            Some(format!("set[{}]", join_literal_type_candidates(element_types)))
        }
        Expr::Dict(dict) => {
            let mut key_types = Vec::new();
            let mut value_types = Vec::new();
            for item in &dict.items {
                let key = item.key.as_ref()?;
                key_types.push(infer_direct_literal_type(key)?);
                value_types.push(infer_direct_literal_type(&item.value)?);
            }
            Some(format!(
                "dict[{}, {}]",
                join_literal_type_candidates(key_types),
                join_literal_type_candidates(value_types)
            ))
        }
        _ => None,
    }
}

fn extract_list_comprehension_clauses(
    source: &str,
    generators: &[ruff_python_ast::Comprehension],
) -> Vec<ComprehensionClauseMetadata> {
    generators
        .iter()
        .map(|generator| {
            let target_names = extract_assignment_names(&generator.target);
            ComprehensionClauseMetadata {
                target_name: target_names.first().cloned().unwrap_or_default(),
                target_names,
                iter: Box::new(extract_direct_expr_metadata(source, &generator.iter)),
                filters: generator
                    .ifs
                    .iter()
                    .filter_map(|expr| extract_guard_condition(source, expr))
                    .collect(),
            }
        })
        .collect()
}

fn infer_direct_binop_type(bin_op: &ruff_python_ast::ExprBinOp) -> Option<String> {
    let left = infer_direct_literal_type(&bin_op.left)?;
    let right = infer_direct_literal_type(&bin_op.right)?;
    match bin_op.op {
        ruff_python_ast::Operator::Add => {
            if left == "str" && right == "str" {
                return Some(String::from("str"));
            }
            if is_direct_numeric_type(&left) && is_direct_numeric_type(&right) {
                return Some(join_numeric_type(&left, &right));
            }
            let (left_head, left_args) = split_direct_generic_type(&left)?;
            let (right_head, right_args) = split_direct_generic_type(&right)?;
            match (left_head.as_str(), right_head.as_str()) {
                ("list", "list") if left_args.len() == 1 && right_args.len() == 1 => Some(format!(
                    "list[{}]",
                    join_literal_type_candidates(vec![left_args[0].clone(), right_args[0].clone()])
                )),
                ("tuple", "tuple") => {
                    let mut args = left_args;
                    args.extend(right_args);
                    Some(format!("tuple[{}]", args.join(", ")))
                }
                _ => None,
            }
        }
        ruff_python_ast::Operator::Sub
        | ruff_python_ast::Operator::Mult
        | ruff_python_ast::Operator::Div
        | ruff_python_ast::Operator::FloorDiv
        | ruff_python_ast::Operator::Mod
            if is_direct_numeric_type(&left) && is_direct_numeric_type(&right) =>
        {
            Some(if bin_op.op == ruff_python_ast::Operator::Div {
                String::from("float")
            } else {
                join_numeric_type(&left, &right)
            })
        }
        _ => None,
    }
}

fn is_direct_numeric_type(text: &str) -> bool {
    matches!(text, "int" | "float" | "complex")
}

fn join_numeric_type(left: &str, right: &str) -> String {
    if left == "complex" || right == "complex" {
        String::from("complex")
    } else if left == "float" || right == "float" {
        String::from("float")
    } else {
        String::from("int")
    }
}

fn split_direct_generic_type(text: &str) -> Option<(String, Vec<String>)> {
    let (head, inner) = text.split_once('[')?;
    let inner = inner.strip_suffix(']')?;
    Some((head.to_owned(), inner.split(',').map(|part| part.trim().to_owned()).collect()))
}

fn join_union_literal_type_candidates(types: Vec<String>) -> String {
    let joined = join_literal_type_candidates(types);
    if joined.contains(" | ") { format!("Union[{}]", joined.replace(" | ", ", ")) } else { joined }
}

fn join_literal_type_candidates(types: Vec<String>) -> String {
    let mut unique = Vec::new();
    for value in types {
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    if unique.is_empty() { String::from("Any") } else { unique.join(" | ") }
}

fn extract_supplemental_call_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
) -> Option<SyntaxStatement> {
    match stmt {
        Stmt::Assign(assign) => extract_call_statement(source, &assign.value, line),
        Stmt::AnnAssign(assign) => {
            assign.value.as_deref().and_then(|value| extract_call_statement(source, value, line))
        }
        _ => None,
    }
}

fn extract_member_access_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    match stmt {
        Stmt::Expr(expr) => {
            extract_member_access_from_expr(source, &expr.value, line, owner_name, owner_type_name)
        }
        Stmt::Assign(assign) => extract_member_access_from_expr(
            source,
            &assign.value,
            line,
            owner_name,
            owner_type_name,
        ),
        Stmt::AnnAssign(assign) => assign.value.as_deref().and_then(|value| {
            extract_member_access_from_expr(source, value, line, owner_name, owner_type_name)
        }),
        _ => None,
    }
}

fn extract_member_access_from_expr(
    _source: &str,
    expr: &Expr,
    line: usize,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Expr::Attribute(attribute) = expr else {
        return None;
    };

    match attribute.value.as_ref() {
        Expr::Name(name) => Some(SyntaxStatement::MemberAccess(MemberAccessStatement {
            current_owner_name: current_owner_name.map(str::to_owned),
            current_owner_type_name: current_owner_type_name.map(str::to_owned),
            owner_name: name.id.as_str().to_owned(),
            member: attribute.attr.as_str().to_owned(),
            through_instance: false,
            line,
        })),
        Expr::Call(call) => {
            let Expr::Name(name) = call.func.as_ref() else {
                return None;
            };
            Some(SyntaxStatement::MemberAccess(MemberAccessStatement {
                current_owner_name: current_owner_name.map(str::to_owned),
                current_owner_type_name: current_owner_type_name.map(str::to_owned),
                owner_name: name.id.as_str().to_owned(),
                member: attribute.attr.as_str().to_owned(),
                through_instance: true,
                line,
            }))
        }
        _ => None,
    }
}

fn collect_nested_call_statements(
    source: &str,
    suite: &[Stmt],
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_calls_from_suite(source, &function.body, statements);
                collect_nested_call_statements(source, &function.body, statements);
            }
            Stmt::ClassDef(class_def) => {
                collect_calls_from_suite(source, &class_def.body, statements);
                collect_nested_call_statements(source, &class_def.body, statements);
            }
            _ => {
                for_each_nested_suite(stmt, |suite| {
                    collect_calls_from_suite(source, suite, statements);
                    collect_nested_call_statements(source, suite, statements);
                });
            }
        }
    }
}

fn collect_nested_member_access_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_nested_member_access_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_nested_member_access_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            _ => {
                for_each_nested_suite(stmt, |nested_suite| {
                    collect_nested_member_access_statements(
                        source,
                        nested_suite,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                });
                if owner_name.is_some() || owner_type_name.is_some() {
                    let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                    if let Some(member_access) = extract_member_access_statement(
                        source,
                        stmt,
                        line,
                        owner_name,
                        owner_type_name,
                    ) {
                        statements.push(member_access);
                    }
                }
            }
        }
    }
}

fn collect_return_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_return_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_return_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            _ => {
                for_each_nested_suite(stmt, |suite| {
                    collect_return_statements(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                });
                let Some(owner_name) = owner_name else {
                    continue;
                };
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if let Some(return_statement) =
                    extract_return_statement(source, stmt, line, owner_name, owner_type_name)
                {
                    statements.push(return_statement);
                }
            }
        }
    }
}

fn collect_yield_statements(
    source: &str,
    suite: &[Stmt],
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                for body_stmt in &function.body {
                    let line =
                        offset_to_line_column(source, body_stmt.range().start().to_usize()).0;
                    if let Some(yield_statement) = extract_yield_statement(
                        source,
                        body_stmt,
                        line,
                        function.name.as_str(),
                        owner_type_name,
                    ) {
                        statements.push(yield_statement);
                    }
                }
                collect_yield_statements(source, &function.body, owner_type_name, statements);
            }
            Stmt::ClassDef(class_def) => {
                collect_yield_statements(
                    source,
                    &class_def.body,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::If(if_stmt) => {
                collect_yield_statements(source, &if_stmt.body, owner_type_name, statements);
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_yield_statements(source, suite, owner_type_name, statements);
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_yield_statements(source, &case.body, owner_type_name, statements);
                }
            }
            _ => {}
        }
    }
}

fn collect_if_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_if_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_if_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_if_statements(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_if_statements(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
                collect_if_statements(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_if_statements(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    statements,
                );
            }
            Stmt::If(if_stmt) => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if !statements.iter().any(|statement| statement_line(statement) == line) {
                    statements.push(SyntaxStatement::If(IfStatement {
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        guard: extract_guard_condition(source, &if_stmt.test),
                        line,
                        true_start_line: suite_start_line(source, &if_stmt.body),
                        true_end_line: suite_end_line(source, &if_stmt.body),
                        false_start_line: if_false_start_line(source, if_stmt),
                        false_end_line: if_false_end_line(source, if_stmt),
                    }));
                }
                collect_if_statements(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_if_statements(source, suite, owner_name, owner_type_name, statements);
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_if_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            _ => {}
        }
    }
}

fn collect_assert_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_assert_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_assert_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_assert_statements(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_assert_statements(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
                collect_assert_statements(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_assert_statements(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    statements,
                );
            }
            Stmt::If(if_stmt) => {
                collect_assert_statements(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_assert_statements(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_assert_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            Stmt::Assert(assert_stmt) => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if !statements.iter().any(|statement| statement_line(statement) == line) {
                    statements.push(SyntaxStatement::Assert(AssertStatement {
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        guard: extract_guard_condition(source, &assert_stmt.test),
                        line,
                    }));
                }
            }
            _ => {}
        }
    }
}

fn collect_invalidation_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_invalidation_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_invalidation_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_invalidation_statements(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_invalidation_statements(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
                collect_invalidation_statements(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_invalidation_statements(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    statements,
                );
            }
            Stmt::If(if_stmt) => {
                collect_invalidation_statements(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_invalidation_statements(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_invalidation_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            Stmt::AugAssign(stmt) => {
                let names = extract_assignment_names(&stmt.target);
                if !names.is_empty() {
                    let line = offset_to_line_column(source, stmt.range.start().to_usize()).0;
                    statements.push(SyntaxStatement::Invalidate(InvalidationStatement {
                        kind: InvalidationKind::RebindLike,
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        names,
                        line,
                    }));
                }
            }
            Stmt::Delete(stmt) => {
                let names =
                    stmt.targets.iter().flat_map(extract_assignment_names).collect::<Vec<_>>();
                if !names.is_empty() {
                    let line = offset_to_line_column(source, stmt.range.start().to_usize()).0;
                    statements.push(SyntaxStatement::Invalidate(InvalidationStatement {
                        kind: InvalidationKind::Delete,
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        names,
                        line,
                    }));
                }
            }
            Stmt::Global(stmt) => {
                let names =
                    stmt.names.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>();
                if !names.is_empty() {
                    let line = offset_to_line_column(source, stmt.range.start().to_usize()).0;
                    statements.push(SyntaxStatement::Invalidate(InvalidationStatement {
                        kind: InvalidationKind::ScopeChange,
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        names,
                        line,
                    }));
                }
            }
            Stmt::Nonlocal(stmt) => {
                let names =
                    stmt.names.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>();
                if !names.is_empty() {
                    let line = offset_to_line_column(source, stmt.range.start().to_usize()).0;
                    statements.push(SyntaxStatement::Invalidate(InvalidationStatement {
                        kind: InvalidationKind::ScopeChange,
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        names,
                        line,
                    }));
                }
            }
            _ => {}
        }
    }
}

fn collect_match_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_match_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_match_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_match_statements(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_match_statements(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
                collect_match_statements(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_match_statements(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    statements,
                );
            }
            _ => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if let Some(match_statement) =
                    extract_match_statement(source, stmt, line, owner_name, owner_type_name)
                {
                    statements.push(match_statement);
                }
            }
        }
    }
}

fn collect_for_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_for_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_for_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_for_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            _ => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if let Some(for_statement) =
                    extract_for_statement(source, stmt, line, owner_name, owner_type_name)
                {
                    statements.push(for_statement);
                }
            }
        }
    }
}

fn collect_with_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_with_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_with_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_with_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            _ => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                statements.extend(extract_with_statements(
                    source,
                    stmt,
                    line,
                    owner_name,
                    owner_type_name,
                ));
            }
        }
    }
}

fn collect_except_handler_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_except_handler_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_except_handler_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Try(try_stmt) => {
                for handler in &try_stmt.handlers {
                    let line = offset_to_line_column(source, handler.range().start().to_usize()).0;
                    if let Some(statement) = extract_except_handler_statement(
                        source,
                        handler,
                        line,
                        owner_name,
                        owner_type_name,
                    ) {
                        statements.push(statement);
                    }
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_except_handler_statements(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
                collect_except_handler_statements(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_except_handler_statements(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_except_handler_statements(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    statements,
                );
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_except_handler_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            _ => {}
        }
    }
}

fn collect_function_body_assignments(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_function_body_assignments(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_function_body_assignments(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            _ => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                let Some(owner_name) = owner_name else {
                    continue;
                };
                if let Some(assignment) = extract_function_body_assignment_statement(
                    source,
                    stmt,
                    line,
                    owner_name,
                    owner_type_name,
                ) {
                    statements.push(assignment);
                }
                for_each_nested_suite(stmt, |suite| {
                    collect_function_body_assignments(
                        source,
                        suite,
                        Some(owner_name),
                        owner_type_name,
                        statements,
                    )
                });
            }
        }
    }
}

fn collect_function_body_bare_assignments(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_function_body_bare_assignments(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_function_body_bare_assignments(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            _ => {
                let Some(owner_name) = owner_name else {
                    continue;
                };
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if let Some(assignment) = extract_function_body_bare_assignment_statement(
                    source,
                    stmt,
                    line,
                    owner_name,
                    owner_type_name,
                ) {
                    statements.push(assignment);
                }
                for_each_nested_suite(stmt, |suite| {
                    collect_function_body_bare_assignments(
                        source,
                        suite,
                        Some(owner_name),
                        owner_type_name,
                        statements,
                    )
                });
            }
        }
    }
}

fn collect_function_body_namedexpr_assignments(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_function_body_namedexpr_assignments(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_function_body_namedexpr_assignments(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            _ => {
                let Some(owner_name) = owner_name else {
                    continue;
                };
                let mut collector = NamedExprAssignmentCollector {
                    source,
                    owner_name,
                    owner_type_name,
                    statements: Vec::new(),
                };
                collector.visit_stmt(stmt);
                statements.extend(collector.statements);
            }
        }
    }
}

struct NamedExprAssignmentCollector<'a> {
    source: &'a str,
    owner_name: &'a str,
    owner_type_name: Option<&'a str>,
    statements: Vec<SyntaxStatement>,
}

impl<'a> NamedExprAssignmentCollector<'a> {
    fn push_namedexpr_assignment(&mut self, named_expr: &ruff_python_ast::ExprNamed) {
        let Expr::Name(name) = named_expr.target.as_ref() else {
            return;
        };
        let line = offset_to_line_column(self.source, named_expr.range().start().to_usize()).0;
        let value = extract_direct_expr_metadata(self.source, &named_expr.value);
        self.statements.push(SyntaxStatement::Value(ValueStatement {
            names: vec![name.id.as_str().to_owned()],
            destructuring_target_names: None,
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
            owner_name: Some(self.owner_name.to_owned()),
            owner_type_name: self.owner_type_name.map(str::to_owned),
            is_final: false,
            is_class_var: false,
            line,
        }));
    }
}

impl<'a> visitor::Visitor<'a> for NamedExprAssignmentCollector<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if matches!(stmt, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
            return;
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_)
            | Expr::ListComp(_)
            | Expr::SetComp(_)
            | Expr::DictComp(_)
            | Expr::Generator(_) => return,
            Expr::Named(named_expr) => {
                self.push_namedexpr_assignment(named_expr);
                self.visit_expr(&named_expr.value);
                return;
            }
            _ => {}
        }
        visitor::walk_expr(self, expr);
    }
}

fn extract_function_body_assignment_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: &str,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Stmt::AnnAssign(assign) = stmt else {
        return None;
    };
    let names = extract_assignment_names(&assign.target);
    if names.is_empty() {
        return None;
    }
    let value =
        assign.value.as_deref().map(|expr| extract_direct_expr_metadata(source, expr)).unwrap_or(
            DirectExprMetadata { value_type: None, value_type_expr: None, is_awaited: false,
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
            value_dict_entries: None, },
        );
    Some(SyntaxStatement::Value(ValueStatement {
        names,
        destructuring_target_names: None,
        annotation: slice_range(source, assign.annotation.range()).map(str::to_owned),
        annotation_expr: slice_range(source, assign.annotation.range()).and_then(TypeExpr::parse),
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
        owner_name: Some(owner_name.to_owned()),
        owner_type_name: owner_type_name.map(str::to_owned),
        is_final: is_final_annotation(&assign.annotation),
        is_class_var: is_classvar_annotation(&assign.annotation),
        line,
    }))
}

fn extract_function_body_bare_assignment_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: &str,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    match stmt {
        Stmt::Assign(assign) => {
            let destructuring_target_names = (assign.targets.len() == 1)
                .then(|| extract_simple_destructuring_target_names(&assign.targets[0]))
                .flatten();
            let names = destructuring_target_names.clone().unwrap_or_else(|| {
                assign.targets.iter().flat_map(extract_assignment_names).collect::<Vec<_>>()
            });
            if names.is_empty() {
                return None;
            }
            let value = extract_direct_expr_metadata(source, &assign.value);
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
                value_method_owner_name: None,
                value_method_name: None,
                value_method_through_instance: false,
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
                value_lambda: None,
                value_list_comprehension: None,
                value_generator_comprehension: None,
                value_list_elements: value.value_list_elements,
                value_set_elements: value.value_set_elements,
                value_dict_entries: value.value_dict_entries,
                owner_name: Some(owner_name.to_owned()),
                owner_type_name: owner_type_name.map(str::to_owned),
                is_final: false,
                is_class_var: false,
                line,
            }))
        }
        Stmt::AugAssign(assign) => extract_augmented_assignment_value_statement(
            source,
            &assign.target,
            &assign.value,
            assign.op,
            Some(owner_name),
            owner_type_name,
            line,
        ),
        _ => None,
    }
}

fn extract_augmented_assignment_value_statement(
    source: &str,
    target: &Expr,
    value: &Expr,
    operator: ruff_python_ast::Operator,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    line: usize,
) -> Option<SyntaxStatement> {
    let names = extract_assignment_names(target);
    if names.is_empty() {
        return None;
    }
    let right = extract_direct_expr_metadata(source, value);
    Some(SyntaxStatement::Value(ValueStatement {
        names,
        destructuring_target_names: None,
        annotation: None,
        annotation_expr: None,
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
        value_binop_left: Some(Box::new(extract_direct_expr_metadata(source, target))),
        value_binop_right: Some(Box::new(right)),
        value_binop_operator: Some(direct_operator_text(operator)),
        value_lambda: None,
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None,
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        is_final: false,
        is_class_var: false,
        line,
    }))
}

fn extract_yield_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: &str,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Stmt::Expr(expr_stmt) = stmt else {
        return None;
    };

    let (value, is_yield_from) = match expr_stmt.value.as_ref() {
        Expr::Yield(yield_expr) => (
            yield_expr
                .value
                .as_deref()
                .map(|expr| extract_direct_expr_metadata(source, expr))
                .unwrap_or(DirectExprMetadata { value_type: None, value_type_expr: None, is_awaited: false,
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
                value_dict_entries: None, }),
            false,
        ),
        Expr::YieldFrom(yield_expr) => {
            (extract_direct_expr_metadata(source, &yield_expr.value), true)
        }
        _ => return None,
    };

    Some(SyntaxStatement::Yield(YieldStatement {
        owner_name: owner_name.to_owned(),
        owner_type_name: owner_type_name.map(str::to_owned),
        value_type: value.value_type,
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
        value_list_elements: value.value_list_elements,
        value_set_elements: value.value_set_elements,
        value_dict_entries: value.value_dict_entries,
        is_yield_from,
        line,
    }))
}

fn extract_match_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Stmt::Match(match_stmt) = stmt else {
        return None;
    };
    let subject = extract_direct_expr_metadata(source, &match_stmt.subject);
    Some(SyntaxStatement::Match(MatchStatement {
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        subject_type: subject.value_type,
        subject_is_awaited: subject.is_awaited,
        subject_callee: subject.value_callee,
        subject_name: subject.value_name,
        subject_member_owner_name: subject.value_member_owner_name,
        subject_member_name: subject.value_member_name,
        subject_member_through_instance: subject.value_member_through_instance,
        subject_method_owner_name: subject.value_method_owner_name,
        subject_method_name: subject.value_method_name,
        subject_method_through_instance: subject.value_method_through_instance,
        cases: match_stmt
            .cases
            .iter()
            .map(|case| MatchCaseStatement {
                patterns: extract_match_patterns(source, &case.pattern),
                has_guard: case.guard.is_some(),
                line: offset_to_line_column(source, case.range.start().to_usize()).0,
            })
            .collect(),
        line,
    }))
}

fn extract_match_patterns(source: &str, pattern: &ruff_python_ast::Pattern) -> Vec<MatchPattern> {
    use ruff_python_ast::Pattern;

    if pattern.is_wildcard() {
        return vec![MatchPattern::Wildcard];
    }

    match pattern {
        Pattern::MatchOr(pattern) => pattern
            .patterns
            .iter()
            .flat_map(|pattern| extract_match_patterns(source, pattern))
            .collect(),
        Pattern::MatchClass(pattern) => slice_range(source, pattern.cls.range())
            .map(|text| vec![MatchPattern::Class(text.to_owned())])
            .unwrap_or_else(|| vec![MatchPattern::Unsupported]),
        Pattern::MatchValue(pattern) => slice_range(source, pattern.value.range())
            .map(|text| vec![MatchPattern::Literal(text.to_owned())])
            .unwrap_or_else(|| vec![MatchPattern::Unsupported]),
        Pattern::MatchSingleton(pattern) => slice_range(source, pattern.range())
            .map(|text| vec![MatchPattern::Literal(text.to_owned())])
            .unwrap_or_else(|| vec![MatchPattern::Unsupported]),
        _ => vec![MatchPattern::Unsupported],
    }
}

fn extract_for_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Stmt::For(for_stmt) = stmt else {
        return None;
    };
    if for_stmt.is_async {
        return None;
    }
    let (target_name, target_names) = match &*for_stmt.target {
        Expr::Name(name) => (name.id.as_str().to_owned(), Vec::new()),
        Expr::Tuple(tuple) => {
            let names = tuple
                .elts
                .iter()
                .map(|elt| {
                    let Expr::Name(name) = elt else {
                        return None;
                    };
                    Some(name.id.as_str().to_owned())
                })
                .collect::<Option<Vec<_>>>()?;
            (String::new(), names)
        }
        _ => return None,
    };
    let iter = extract_direct_expr_metadata(source, &for_stmt.iter);
    Some(SyntaxStatement::For(ForStatement {
        target_name,
        target_names,
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        iter_type: iter.value_type,
        iter_is_awaited: iter.is_awaited,
        iter_callee: iter.value_callee,
        iter_name: iter.value_name,
        iter_member_owner_name: iter.value_member_owner_name,
        iter_member_name: iter.value_member_name,
        iter_member_through_instance: iter.value_member_through_instance,
        iter_method_owner_name: iter.value_method_owner_name,
        iter_method_name: iter.value_method_name,
        iter_method_through_instance: iter.value_method_through_instance,
        line,
    }))
}

fn extract_with_statements(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Vec<SyntaxStatement> {
    let Stmt::With(with_stmt) = stmt else {
        return Vec::new();
    };
    if with_stmt.is_async {
        return Vec::new();
    }
    with_stmt
        .items
        .iter()
        .filter_map(|item| {
            let target_name = match item.optional_vars.as_deref() {
                Some(Expr::Name(name)) => Some(name.id.as_str().to_owned()),
                Some(_) => return None,
                None => None,
            };
            let context = extract_direct_expr_metadata(source, &item.context_expr);
            Some(SyntaxStatement::With(WithStatement {
                target_name,
                owner_name: owner_name.map(str::to_owned),
                owner_type_name: owner_type_name.map(str::to_owned),
                context_type: context.value_type,
                context_is_awaited: context.is_awaited,
                context_callee: context.value_callee,
                context_name: context.value_name,
                context_member_owner_name: context.value_member_owner_name,
                context_member_name: context.value_member_name,
                context_member_through_instance: context.value_member_through_instance,
                context_method_owner_name: context.value_method_owner_name,
                context_method_name: context.value_method_name,
                context_method_through_instance: context.value_method_through_instance,
                line,
            }))
        })
        .collect()
}

fn extract_except_handler_statement(
    source: &str,
    handler: &ruff_python_ast::ExceptHandler,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
    Some(SyntaxStatement::ExceptHandler(ExceptionHandlerStatement {
        exception_type: handler
            .type_
            .as_ref()
            .and_then(|expr| slice_range(source, expr.range()))
            .map(str::to_owned)
            .unwrap_or_else(|| String::from("BaseException")),
        binding_name: handler.name.as_ref().map(|name| name.as_str().to_owned()),
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        line,
        end_line: offset_to_line_column(source, handler.range.end().to_usize()).0,
    }))
}

fn collect_calls_from_suite(source: &str, suite: &[Stmt], statements: &mut Vec<SyntaxStatement>) {
    for stmt in suite {
        let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
        if let Some(call) =
            extract_supplemental_call_statement(source, stmt, line).or_else(|| match stmt {
                Stmt::Expr(expr) => extract_call_statement(source, &expr.value, line),
                _ => None,
            })
        {
            statements.push(call);
        }
        if let Some(method_call) = extract_method_call_statement(source, stmt, line) {
            statements.push(method_call);
        }
    }
}

fn extract_return_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: &str,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Stmt::Return(return_stmt) = stmt else {
        return None;
    };

    let value = return_stmt
        .value
        .as_deref()
        .map(|expr| extract_direct_expr_metadata(source, expr))
        .unwrap_or(DirectExprMetadata { value_type: None, value_type_expr: None, is_awaited: false,
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
        value_dict_entries: None, });

    Some(SyntaxStatement::Return(ReturnStatement {
        owner_name: owner_name.to_owned(),
        owner_type_name: owner_type_name.map(str::to_owned),
        value_type: value.value_type,
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
        value_list_elements: value.value_list_elements,
        value_set_elements: value.value_set_elements,
        value_dict_entries: value.value_dict_entries,
        line,
    }))
}

fn extract_direct_expr_metadata(source: &str, expr: &Expr) -> DirectExprMetadata {
    if let Expr::Await(await_expr) = expr {
        let mut metadata = extract_direct_expr_metadata(source, &await_expr.value);
        metadata.is_awaited = true;
        return metadata;
    }

    if let Expr::Named(named_expr) = expr {
        return extract_direct_expr_metadata(source, &named_expr.value);
    }

    if let Expr::Dict(dict) = expr {
        return DirectExprMetadata { value_type: Some(infer_literal_arg_type(expr)), value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)), is_awaited: false,
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
        value_dict_entries: Some(extract_typed_dict_literal_entries(source, dict)), };
    }

    if let Expr::List(list) = expr {
        return DirectExprMetadata { value_type: Some(infer_literal_arg_type(expr)), value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)), is_awaited: false,
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
        value_list_elements: Some(
            list.elts.iter().map(|item| extract_direct_expr_metadata(source, item)).collect(),
        ),
        value_set_elements: None,
        value_dict_entries: None, };
    }

    if let Expr::Set(set) = expr {
        return DirectExprMetadata { value_type: Some(infer_literal_arg_type(expr)), value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)), is_awaited: false,
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
        value_set_elements: Some(
            set.elts.iter().map(|item| extract_direct_expr_metadata(source, item)).collect(),
        ),
        value_dict_entries: None, };
    }

    if let Expr::Lambda(lambda) = expr {
        let mut params = lambda
            .parameters
            .as_ref()
            .map(|parameters| extract_function_params(source, parameters))
            .unwrap_or_default();
        let (line, column) = offset_to_line_column(source, expr.range().start().to_usize());
        if let Some(site) = annotated_lambda_site_at(line, column)
            && site.param_names.len() == params.len()
            && site.param_names.iter().zip(params.iter()).all(|(name, param)| name == &param.name)
        {
            for (param, annotation) in params.iter_mut().zip(site.annotations) {
                param.annotation = annotation;
            }
        }
        return DirectExprMetadata { value_type: Some(String::new()), value_type_expr: None, is_awaited: false,
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
        value_lambda: Some(Box::new(LambdaMetadata {
            params,
            body: Box::new(extract_direct_expr_metadata(source, &lambda.body)),
        })),
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None, };
    }

    if let Expr::ListComp(comp) = expr {
        return DirectExprMetadata { value_type: Some(String::new()), value_type_expr: None, is_awaited: false,
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
        value_list_comprehension: Some(Box::new(ComprehensionMetadata {
            kind: ComprehensionKind::List,
            clauses: extract_list_comprehension_clauses(source, &comp.generators),
            key: None,
            element: Box::new(extract_direct_expr_metadata(source, &comp.elt)),
        })),
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None, };
    }

    if let Expr::SetComp(comp) = expr {
        return DirectExprMetadata { value_type: Some(String::new()), value_type_expr: None, is_awaited: false,
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
        value_list_comprehension: Some(Box::new(ComprehensionMetadata {
            kind: ComprehensionKind::Set,
            clauses: extract_list_comprehension_clauses(source, &comp.generators),
            key: None,
            element: Box::new(extract_direct_expr_metadata(source, &comp.elt)),
        })),
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None, };
    }

    if let Expr::DictComp(comp) = expr {
        return DirectExprMetadata { value_type: Some(String::new()), value_type_expr: None, is_awaited: false,
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
        value_list_comprehension: Some(Box::new(ComprehensionMetadata {
            kind: ComprehensionKind::Dict,
            clauses: extract_list_comprehension_clauses(source, &comp.generators),
            key: Some(Box::new(extract_direct_expr_metadata(source, &comp.key))),
            element: Box::new(extract_direct_expr_metadata(source, &comp.value)),
        })),
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None, };
    }

    if let Expr::Generator(comp) = expr {
        return DirectExprMetadata { value_type: Some(String::new()), value_type_expr: None, is_awaited: false,
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
        value_generator_comprehension: Some(Box::new(ComprehensionMetadata {
            kind: ComprehensionKind::Generator,
            clauses: extract_list_comprehension_clauses(source, &comp.generators),
            key: None,
            element: Box::new(extract_direct_expr_metadata(source, &comp.elt)),
        })),
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None, };
    }

    if let Some((owner_name, method_name, through_instance)) = extract_direct_method_call(expr) {
        return DirectExprMetadata { value_type: Some(infer_literal_arg_type(expr)), value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)), is_awaited: false,
        value_callee: None,
        value_name: None,
        value_member_owner_name: None,
        value_member_name: None,
        value_member_through_instance: false,
        value_method_owner_name: Some(owner_name),
        value_method_name: Some(method_name),
        value_method_through_instance: through_instance,
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
        value_dict_entries: None, };
    }

    if let Expr::BoolOp(bool_op) = expr {
        let mut values = bool_op.values.iter();
        let left_expr = values.next();
        let left_guard = left_expr.and_then(|expr| extract_guard_condition(source, expr));
        let left = left_expr.map(|expr| extract_direct_expr_metadata(source, expr));
        let right = values.next().map(|expr| extract_direct_expr_metadata(source, expr));
        return DirectExprMetadata { value_type: Some(String::new()), value_type_expr: None, is_awaited: false,
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
        value_if_guard: left_guard,
        value_bool_left: left.map(Box::new),
        value_bool_right: right.map(Box::new),
        value_binop_left: None,
        value_binop_right: None,
        value_binop_operator: Some(match bool_op.op {
            ruff_python_ast::BoolOp::And => String::from("and"),
            ruff_python_ast::BoolOp::Or => String::from("or"),
        }),
        value_lambda: None,
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None, };
    }

    if let Expr::BinOp(bin_op) = expr {
        return DirectExprMetadata { value_type: Some(String::new()), value_type_expr: None, is_awaited: false,
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
        value_binop_left: Some(Box::new(extract_direct_expr_metadata(source, &bin_op.left))),
        value_binop_right: Some(Box::new(extract_direct_expr_metadata(source, &bin_op.right))),
        value_binop_operator: Some(direct_operator_text(bin_op.op)),
        value_lambda: None,
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None, };
    }

    if let Expr::If(if_expr) = expr {
        return DirectExprMetadata { value_type: Some(infer_literal_arg_type(expr)), value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)), is_awaited: false,
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
        value_if_true: Some(Box::new(extract_direct_expr_metadata(source, &if_expr.body))),
        value_if_false: Some(Box::new(extract_direct_expr_metadata(source, &if_expr.orelse))),
        value_if_guard: extract_guard_condition(source, &if_expr.test),
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
        value_dict_entries: None, };
    }

    if let Expr::Subscript(subscript) = expr {
        return DirectExprMetadata { value_type: Some(infer_literal_arg_type(expr)), value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)), is_awaited: false,
        value_callee: None,
        value_name: None,
        value_member_owner_name: None,
        value_member_name: None,
        value_member_through_instance: false,
        value_method_owner_name: None,
        value_method_name: None,
        value_method_through_instance: false,
        value_subscript_target: Some(Box::new(extract_direct_expr_metadata(
            source,
            &subscript.value,
        ))),
        value_subscript_string_key: extract_string_literal_value(source, &subscript.slice),
        value_subscript_index: match infer_literal_arg_type(&subscript.slice).as_str() {
            "int" => slice_range(source, subscript.slice.range()).map(str::to_owned),
            _ => None,
        },
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
        value_dict_entries: None, };
    }

    let member = extract_direct_member_access(expr);
    DirectExprMetadata { value_type: Some(infer_literal_arg_type(expr)), value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)), is_awaited: false,
    value_callee: extract_direct_callee(expr),
    value_name: extract_direct_name(expr),
    value_member_owner_name: member.as_ref().map(|(owner_name, _, _)| owner_name.clone()),
    value_member_name: member.as_ref().map(|(_, member, _)| member.clone()),
    value_member_through_instance: member
        .as_ref()
        .map(|(_, _, through_instance)| *through_instance)
        .unwrap_or(false),
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
    value_dict_entries: None, }
}

fn extract_direct_method_call(expr: &Expr) -> Option<(String, String, bool)> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Attribute(attribute) = call.func.as_ref() else {
        return None;
    };

    match attribute.value.as_ref() {
        Expr::Name(name) => {
            Some((name.id.as_str().to_owned(), attribute.attr.as_str().to_owned(), false))
        }
        Expr::Call(inner_call) => {
            let Expr::Name(name) = inner_call.func.as_ref() else {
                return None;
            };
            Some((name.id.as_str().to_owned(), attribute.attr.as_str().to_owned(), true))
        }
        _ => None,
    }
}

fn extract_direct_callee(expr: &Expr) -> Option<String> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Name(name) = call.func.as_ref() else {
        return None;
    };
    Some(name.id.as_str().to_owned())
}

fn extract_direct_name(expr: &Expr) -> Option<String> {
    let Expr::Name(name) = expr else {
        return None;
    };
    Some(name.id.as_str().to_owned())
}

fn extract_direct_member_access(expr: &Expr) -> Option<(String, String, bool)> {
    let Expr::Attribute(attribute) = expr else {
        return None;
    };

    match attribute.value.as_ref() {
        Expr::Name(name) => {
            Some((name.id.as_str().to_owned(), attribute.attr.as_str().to_owned(), false))
        }
        Expr::Call(call) => {
            let Expr::Name(name) = call.func.as_ref() else {
                return None;
            };
            Some((name.id.as_str().to_owned(), attribute.attr.as_str().to_owned(), true))
        }
        _ => None,
    }
}

fn extract_ast_type_params(
    path: &Path,
    source: &str,
    type_params: Option<&ruff_python_ast::TypeParams>,
    line: usize,
    label: &str,
    diagnostics: &mut DiagnosticReport,
) -> Option<Vec<TypeParam>> {
    let mut parsed = Vec::new();

    for type_param in type_params.into_iter().flat_map(|type_params| type_params.iter()) {
        match type_param {
            AstTypeParam::TypeVar(type_var) => {
                let (bound, constraints) = extract_ast_type_param_bound_and_constraints(
                    source,
                    type_var.bound.as_deref(),
                )?;
                parsed.push(TypeParam {
                    kind: TypeParamKind::TypeVar,
                    name: type_var.name.as_str().to_owned(),
                    bound_expr: bound.as_deref().and_then(TypeExpr::parse),
                    bound,
                    constraint_exprs: constraints.iter().filter_map(|constraint| TypeExpr::parse(constraint)).collect(),
                    constraints,
                    default_expr: type_var
                        .default
                        .as_ref()
                        .and_then(|default| slice_range(source, default.range()))
                        .and_then(TypeExpr::parse),
                    default: type_var
                        .default
                        .as_ref()
                        .and_then(|default| slice_range(source, default.range()))
                        .map(str::to_owned),
                });
            }
            AstTypeParam::ParamSpec(param_spec) => {
                parsed.push(TypeParam {
                    kind: TypeParamKind::ParamSpec,
                    name: param_spec.name.as_str().to_owned(),
                    bound_expr: None,
                    bound: None,
                    constraint_exprs: Vec::new(),
                    constraints: Vec::new(),
                    default_expr: param_spec
                        .default
                        .as_ref()
                        .and_then(|default| slice_range(source, default.range()))
                        .and_then(TypeExpr::parse),
                    default: param_spec
                        .default
                        .as_ref()
                        .and_then(|default| slice_range(source, default.range()))
                        .map(str::to_owned),
                });
            }
            AstTypeParam::TypeVarTuple(type_var_tuple) => {
                parsed.push(TypeParam {
                    kind: TypeParamKind::TypeVarTuple,
                    name: type_var_tuple.name.as_str().to_owned(),
                    bound_expr: None,
                    bound: None,
                    constraint_exprs: Vec::new(),
                    constraints: Vec::new(),
                    default_expr: type_var_tuple
                        .default
                        .as_ref()
                        .and_then(|default| slice_range(source, default.range()))
                        .and_then(TypeExpr::parse),
                    default: type_var_tuple
                        .default
                        .as_ref()
                        .and_then(|default| slice_range(source, default.range()))
                        .map(str::to_owned),
                });
            }
        }
    }

    if !validate_type_param_names(path, line, label, &parsed, diagnostics)
        || !validate_type_param_default_order(path, line, label, &parsed, diagnostics)
    {
        return None;
    }

    Some(parsed)
}

fn extract_ast_type_param_bound_and_constraints(
    source: &str,
    bound: Option<&Expr>,
) -> Option<(Option<String>, Vec<String>)> {
    let Some(bound) = bound else {
        return Some((None, Vec::new()));
    };
    if let Expr::Tuple(tuple_expr) = bound {
        let constraints = tuple_expr
            .elts
            .iter()
            .map(|constraint| slice_range(source, constraint.range()).map(str::to_owned))
            .collect::<Option<Vec<_>>>()?;
        return Some((None, constraints));
    }
    Some((slice_range(source, bound.range()).map(str::to_owned), Vec::new()))
}

fn extract_function_params(
    source: &str,
    parameters: &ruff_python_ast::Parameters,
) -> Vec<FunctionParam> {
    let positional_only = parameters.posonlyargs.iter().map(|parameter| FunctionParam {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        annotation_expr: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .and_then(TypeExpr::parse),
        has_default: parameter.default().is_some(),
        positional_only: true,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    });
    let positional = parameters.args.iter().map(|parameter| FunctionParam {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        annotation_expr: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .and_then(TypeExpr::parse),
        has_default: parameter.default().is_some(),
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    });
    let variadic = parameters.vararg.iter().map(|parameter| FunctionParam {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        annotation_expr: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .and_then(TypeExpr::parse),
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: true,
        keyword_variadic: false,
    });
    let keyword_only = parameters.kwonlyargs.iter().map(|parameter| FunctionParam {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        annotation_expr: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .and_then(TypeExpr::parse),
        has_default: parameter.default().is_some(),
        positional_only: false,
        keyword_only: true,
        variadic: false,
        keyword_variadic: false,
    });
    let keyword_variadic = parameters.kwarg.iter().map(|parameter| FunctionParam {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        annotation_expr: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .and_then(TypeExpr::parse),
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: true,
    });

    positional_only
        .chain(positional)
        .chain(variadic)
        .chain(keyword_only)
        .chain(keyword_variadic)
        .collect()
}

fn extract_class_bases(source: &str, arguments: &ruff_python_ast::Arguments) -> Vec<String> {
    arguments
        .args
        .iter()
        .filter_map(|argument| slice_range(source, argument.range()).map(str::to_owned))
        .collect()
}

fn extract_assignment_names(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::Name(name) => vec![name.id.as_str().to_owned()],
        Expr::Tuple(tuple) => tuple.elts.iter().flat_map(extract_assignment_names).collect(),
        Expr::List(list) => list.elts.iter().flat_map(extract_assignment_names).collect(),
        Expr::Starred(starred) => extract_assignment_names(&starred.value),
        _ => Vec::new(),
    }
}

fn extract_simple_destructuring_target_names(expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::Tuple(tuple) => tuple
            .elts
            .iter()
            .map(|element| match element {
                Expr::Name(name) => Some(name.id.as_str().to_owned()),
                _ => None,
            })
            .collect(),
        Expr::List(list) => list
            .elts
            .iter()
            .map(|element| match element {
                Expr::Name(name) => Some(name.id.as_str().to_owned()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn is_overload_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "overload",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "overload"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if name.id.as_str() == "typing")
        }
        _ => false,
    }
}

fn is_override_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "override",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "override"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        _ => false,
    }
}

fn is_abstractmethod_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "abstractmethod",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "abstractmethod"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if name.id.as_str() == "abc")
        }
        _ => false,
    }
}

fn is_final_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "final",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "final"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        _ => false,
    }
}

fn deprecated_decorator_message(decorators: &[ruff_python_ast::Decorator]) -> Option<String> {
    decorators.iter().find_map(deprecated_decorator_arg)
}

fn deprecated_decorator_arg(decorator: &ruff_python_ast::Decorator) -> Option<String> {
    match &decorator.expression {
        Expr::Name(name) if name.id.as_str() == "deprecated" => Some(String::new()),
        Expr::Attribute(attribute)
            if attribute.attr.as_str() == "deprecated"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing_extensions" | "warnings")) =>
        {
            Some(String::new())
        }
        Expr::Call(call) => {
            let target = match &*call.func {
                Expr::Name(name) => name.id.as_str() == "deprecated",
                Expr::Attribute(attribute) => {
                    attribute.attr.as_str() == "deprecated"
                        && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing_extensions" | "warnings"))
                }
                _ => false,
            };
            if !target {
                return None;
            }
            call.arguments
                .args
                .first()
                .and_then(|arg| match arg {
                    Expr::StringLiteral(string) => Some(string.value.to_str().to_owned()),
                    _ => None,
                })
                .or(Some(String::new()))
        }
        _ => None,
    }
}

fn method_kind_from_decorators(decorators: &[ruff_python_ast::Decorator]) -> MethodKind {
    for decorator in decorators {
        match &decorator.expression {
            Expr::Name(name) if name.id.as_str() == "classmethod" => return MethodKind::Class,
            Expr::Name(name) if name.id.as_str() == "staticmethod" => return MethodKind::Static,
            Expr::Name(name) if name.id.as_str() == "property" => return MethodKind::Property,
            Expr::Attribute(attribute) if attribute.attr.as_str() == "setter" => {
                return MethodKind::PropertySetter;
            }
            _ => {}
        }
    }

    MethodKind::Instance
}

fn is_abstract_class(statement: &NamedBlockStatement) -> bool {
    statement.bases.iter().any(|base| matches!(base.as_str(), "ABC" | "abc.ABC"))
        || statement.members.iter().any(|member| member.is_abstract_method)
}

fn is_final_annotation(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "Final",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "Final"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        Expr::Subscript(subscript) => is_final_annotation(&subscript.value),
        _ => false,
    }
}

fn is_classvar_annotation(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "ClassVar",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "ClassVar"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        Expr::Subscript(subscript) => is_classvar_annotation(&subscript.value),
        _ => false,
    }
}

fn normalize_import_module(
    path: &Path,
    current_module_key: &str,
    level: u32,
    module: &str,
) -> String {
    if level == 0 {
        return module.to_owned();
    }

    let mut parts: Vec<_> = current_module_key.split('.').filter(|part| !part.is_empty()).collect();
    if path.file_stem().and_then(|stem| stem.to_str()) != Some("__init__") {
        parts.pop();
    }
    for _ in 1..level {
        parts.pop();
    }
    if !module.is_empty() {
        parts.extend(module.split('.'));
    }
    parts.join(".")
}

fn slice_range(source: &str, range: ruff_text_size::TextRange) -> Option<&str> {
    source.get(range.start().to_usize()..range.end().to_usize())
}

fn parse_extension_statement(
    path: &Path,
    trimmed_line: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    if let Some(rest) = strip_soft_keyword(trimmed_line, "typealias") {
        return parse_typealias(path, trimmed_line, rest, line_number, diagnostics);
    }
    if let Some(rest) = strip_soft_keyword(trimmed_line, "interface") {
        return parse_named_block(
            path,
            trimmed_line,
            rest,
            line_number,
            diagnostics,
            "interface declaration",
            SyntaxStatement::Interface,
        );
    }
    if let Some(rest) = trimmed_line.strip_prefix("data class ") {
        return parse_named_block(
            path,
            trimmed_line,
            rest,
            line_number,
            diagnostics,
            "data class declaration",
            SyntaxStatement::DataClass,
        );
    }
    if let Some(rest) = trimmed_line.strip_prefix("sealed class ") {
        return parse_named_block(
            path,
            trimmed_line,
            rest,
            line_number,
            diagnostics,
            "sealed class declaration",
            SyntaxStatement::SealedClass,
        );
    }
    if let Some(rest) = trimmed_line.strip_prefix("overload def ") {
        return parse_overload(path, trimmed_line, rest, line_number, diagnostics);
    }
    if trimmed_line.starts_with("unsafe") {
        return parse_unsafe(path, trimmed_line, line_number, diagnostics);
    }

    None
}

fn strip_soft_keyword<'source>(line: &'source str, keyword: &str) -> Option<&'source str> {
    let rest = line.strip_prefix(keyword)?;
    match rest.chars().next() {
        Some(character) if character == '_' || character.is_ascii_alphanumeric() => None,
        _ => Some(rest),
    }
}

fn parse_typealias(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    let (head, tail) = match split_top_level_once(rest, '=') {
        Some(parts) => parts,
        None => {
            diagnostics.push(parse_error(
                path,
                line_number,
                line,
                "typealias declaration must contain `=`",
            ));
            return None;
        }
    };

    if tail.trim().is_empty() {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            "typealias declaration must define a target type expression",
        ));
        return None;
    }

    let Some((name, suffix)) = extract_decl_head(head) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            "typealias declaration must name an alias before `=`",
        ));
        return None;
    };
    let parsed_type_params =
        parse_type_params(path, line_number, line, suffix, diagnostics, "typealias declaration")?;

    Some(SyntaxStatement::TypeAlias(TypeAliasStatement {
        name,
        type_params: parsed_type_params.type_params,
        value: tail.trim().to_owned(),
        value_expr: TypeExpr::parse(tail.trim()),
        line: line_number,
    }))
}

fn parse_named_block(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
    label: &str,
    constructor: fn(NamedBlockStatement) -> SyntaxStatement,
) -> Option<SyntaxStatement> {
    if !line.ends_with(':') {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must end with `:`"),
        ));
        return None;
    }

    let header = &rest[..rest.len().saturating_sub(1)].trim_end();
    let Some((name, suffix)) = extract_decl_head(header) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must include a valid name"),
        ));
        return None;
    };
    let parsed_type_params =
        parse_type_params(path, line_number, line, suffix, diagnostics, label)?;

    Some(constructor(NamedBlockStatement {
        name,
        type_params: parsed_type_params.type_params,
        header_suffix: parsed_type_params.remainder.trim().to_owned(),
        bases: Vec::new(),
        is_final_decorator: false,
        is_deprecated: false,
        deprecation_message: None,
        is_abstract_class: false,
        members: Vec::new(),
        line: line_number,
    }))
}

fn parse_overload(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    parse_function(
        path,
        line,
        rest,
        line_number,
        diagnostics,
        "overload declaration",
        SyntaxStatement::OverloadDef,
    )
}

fn parse_function(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
    label: &str,
    constructor: fn(FunctionStatement) -> SyntaxStatement,
) -> Option<SyntaxStatement> {
    let Some((signature, _suite)) = split_top_level_once(rest.trim_end(), ':') else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must end with `:`"),
        ));
        return None;
    };

    let Some((name_part, _)) = split_top_level_once(signature.trim_end(), '(') else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must include a parameter list"),
        ));
        return None;
    };
    let Some((name, suffix)) = extract_decl_head(name_part) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must include a function name"),
        ));
        return None;
    };
    let parsed_type_params =
        parse_type_params(path, line_number, line, suffix, diagnostics, label)?;

    Some(constructor(FunctionStatement {
        name,
        type_params: parsed_type_params.type_params,
        params: Vec::new(),
        returns: None,
        returns_expr: None,
        is_async: false,
        is_override: false,
        is_deprecated: false,
        deprecation_message: None,
        line: line_number,
    }))
}

fn parse_unsafe(
    path: &Path,
    line: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    if line == "unsafe:" {
        return Some(SyntaxStatement::Unsafe(UnsafeStatement { line: line_number }));
    }

    if line.starts_with("unsafe:") {
        return Some(SyntaxStatement::Unsafe(UnsafeStatement { line: line_number }));
    }

    diagnostics.push(parse_error(
        path,
        line_number,
        line,
        "unsafe block must start with `unsafe:`",
    ));
    None
}

fn extract_decl_head(header: &str) -> Option<(String, &str)> {
    let header = header.trim();
    if header.is_empty() {
        return None;
    }

    let end = header
        .find(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
        .unwrap_or(header.len());
    let candidate = &header[..end];
    is_valid_identifier(candidate).then(|| (candidate.to_owned(), &header[end..]))
}

fn parse_type_params<'source>(
    path: &Path,
    line_number: usize,
    line: &str,
    suffix: &'source str,
    diagnostics: &mut DiagnosticReport,
    label: &str,
) -> Option<ParsedTypeParams<'source>> {
    let suffix = suffix.trim_start();
    if suffix.is_empty() || !suffix.starts_with('[') {
        return Some(ParsedTypeParams { type_params: Vec::new(), remainder: suffix });
    }

    let Some((content, remainder)) = split_bracketed(suffix) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} has an unterminated type parameter list"),
        ));
        return None;
    };
    if remainder.trim_start().starts_with('[') {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must not contain multiple type parameter lists"),
        ));
        return None;
    }

    let mut type_params = Vec::new();
    for item in split_top_level(content, ',') {
        match parse_type_param(path, line_number, line, item, label) {
            Ok(type_param) => type_params.push(type_param),
            Err(diagnostic) => {
                diagnostics.push(*diagnostic);
                return None;
            }
        }
    }

    if !validate_type_param_names(path, line_number, label, &type_params, diagnostics)
        || !validate_type_param_default_order(path, line_number, label, &type_params, diagnostics)
    {
        return None;
    }

    Some(ParsedTypeParams { type_params, remainder })
}

fn validate_type_param_names(
    path: &Path,
    line_number: usize,
    label: &str,
    type_params: &[TypeParam],
    diagnostics: &mut DiagnosticReport,
) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    for type_param in type_params {
        if !seen.insert(type_param.name.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4004",
                    format!("{label} declares type parameter `{}` more than once", type_param.name),
                )
                .with_span(Span::new(
                    path.display().to_string(),
                    line_number,
                    1,
                    line_number,
                    1,
                )),
            );
            return false;
        }
    }
    true
}

fn validate_type_param_default_order(
    path: &Path,
    line_number: usize,
    label: &str,
    type_params: &[TypeParam],
    diagnostics: &mut DiagnosticReport,
) -> bool {
    let mut seen_default = false;
    for type_param in type_params {
        if type_param.default.is_some() {
            seen_default = true;
            continue;
        }
        if seen_default {
            diagnostics.push(
                Diagnostic::error(
                    "TPY2001",
                    format!(
                        "{label} type parameter `{}` without a default cannot follow a parameter with a default",
                        type_param.name
                    ),
                )
                .with_span(Span::new(path.display().to_string(), line_number, 1, line_number, 1)),
            );
            return false;
        }
    }
    true
}

fn split_top_level_once(input: &str, separator: char) -> Option<(&str, &str)> {
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;

    for (index, character) in input.char_indices() {
        if character == separator && bracket_depth == 0 && paren_depth == 0 {
            let tail_start = index + character.len_utf8();
            return Some((&input[..index], &input[tail_start..]));
        }

        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
    }

    None
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut start = 0usize;

    for (index, character) in input.char_indices() {
        if character == ',' && bracket_depth == 0 && paren_depth == 0 {
            parts.push(input[start..index].trim());
            start = index + character.len_utf8();
            continue;
        }

        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
    }

    let tail = input[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

fn parse_type_param(
    path: &Path,
    line_number: usize,
    line: &str,
    item: &str,
    label: &str,
) -> Result<TypeParam, Box<Diagnostic>> {
    let item = item.trim();
    if item.is_empty() {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} contains an empty type parameter entry"),
        )));
    }

    let (item, default) = match split_top_level_once(item, '=') {
        Some((head, default)) => (head.trim(), Some(default.trim())),
        None => (item, None),
    };
    let (kind, item) = if let Some(item) = item.strip_prefix("**") {
        (TypeParamKind::ParamSpec, item.trim())
    } else if let Some(item) = item.strip_prefix('*') {
        (TypeParamKind::TypeVarTuple, item.trim())
    } else {
        (TypeParamKind::TypeVar, item)
    };
    let (name_part, bound_or_constraints) = match split_top_level_once(item, ':') {
        Some((name_part, bound)) => (name_part.trim(), Some(bound.trim())),
        None => (item, None),
    };
    if !is_valid_identifier(name_part) {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} contains an invalid type parameter name"),
        )));
    }

    if kind == TypeParamKind::ParamSpec && bound_or_constraints.is_some() {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} ParamSpec `{name_part}` must not declare bounds or constraints"),
        )));
    }
    if kind == TypeParamKind::TypeVarTuple && bound_or_constraints.is_some() {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} TypeVarTuple `{name_part}` must not declare bounds or constraints"),
        )));
    }

    let (bound, constraints) = match bound_or_constraints {
        Some("") => {
            return Err(Box::new(parse_error(
                path,
                line_number,
                line,
                format!("{label} type parameter bound must not be empty"),
            )));
        }
        Some(bound) if bound.starts_with('(') => {
            (None, parse_type_param_constraints(path, line_number, line, bound, label)?)
        }
        Some(bound) => (Some(bound.to_owned()), Vec::new()),
        None => (None, Vec::new()),
    };
    let default = match default {
        Some("") => {
            return Err(Box::new(parse_error(
                path,
                line_number,
                line,
                format!("{label} type parameter default must not be empty"),
            )));
        }
        Some(default) => Some(default.to_owned()),
        None => None,
    };

    Ok(TypeParam {
        kind,
        name: name_part.to_owned(),
        bound_expr: bound.as_deref().and_then(TypeExpr::parse),
        bound,
        constraint_exprs: constraints.iter().filter_map(|constraint| TypeExpr::parse(constraint)).collect(),
        constraints,
        default_expr: default.as_deref().and_then(TypeExpr::parse),
        default,
    })
}

fn parse_type_param_constraints(
    path: &Path,
    line_number: usize,
    line: &str,
    constraints: &str,
    label: &str,
) -> Result<Vec<String>, Box<Diagnostic>> {
    let Some(inner) = constraints.strip_prefix('(').and_then(|inner| inner.strip_suffix(')'))
    else {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} type parameter constraint list must be parenthesized"),
        )));
    };
    let parsed = split_top_level_commas(inner);
    if parsed.is_empty() {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} type parameter constraint list must not be empty"),
        )));
    }
    if parsed.iter().any(|constraint| constraint.is_empty()) {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} type parameter constraint list must not contain empty entries"),
        )));
    }
    Ok(parsed.into_iter().map(str::to_owned).collect())
}

fn split_bracketed(input: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;

    for (index, character) in input.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((&input[1..index], &input[index + 1..]));
                }
            }
            _ => {}
        }
    }

    None
}

fn split_top_level(input: &str, separator: char) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;

    for (index, character) in input.char_indices() {
        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ if character == separator && bracket_depth == 0 && paren_depth == 0 => {
                items.push(&input[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }

    items.push(&input[start..]);
    items
}

fn is_valid_identifier(candidate: &str) -> bool {
    let mut characters = candidate.chars();
    match characters.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }

    characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn parse_error(
    path: &Path,
    line_number: usize,
    line: &str,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic::error("TPY2001", message.into()).with_span(Span::new(
        path.display().to_string(),
        line_number,
        1,
        line_number,
        line.chars().count().max(1),
    ))
}

fn parse_error_span(path: &Path, source: &str, start: usize, end: usize) -> Span {
    let start = start.min(source.len());
    let end = end.max(start).min(source.len());
    let (line, column) = offset_to_line_column(source, start);
    let (end_line, end_column) = offset_to_line_column(source, end);

    Span::new(path.display().to_string(), line, column, end_line, end_column)
}

fn offset_to_line_column(source: &str, offset: usize) -> (usize, usize) {
    let active_lookup = ACTIVE_SOURCE_LINE_INDICES.with(|active| {
        active
            .borrow()
            .iter()
            .rev()
            .find(|index| index.ptr == source.as_ptr() as usize && index.len == source.len())
            .map(|index| offset_to_line_column_from_line_starts(source, offset, &index.line_starts))
    });
    if let Some(line_and_column) = active_lookup {
        return line_and_column;
    }

    let mut line = 1usize;
    let mut column = 1usize;

    for (index, character) in source.char_indices() {
        if index >= offset {
            break;
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    (line, column)
}

fn with_active_annotated_lambda_sites<T>(
    sites: Vec<AnnotatedLambdaSite>,
    action: impl FnOnce() -> T,
) -> T {
    struct LambdaSiteGuard {
        previous: Vec<AnnotatedLambdaSite>,
    }

    impl Drop for LambdaSiteGuard {
        fn drop(&mut self) {
            ACTIVE_ANNOTATED_LAMBDA_SITES.with(|active| {
                active.replace(std::mem::take(&mut self.previous));
            });
        }
    }

    let previous = ACTIVE_ANNOTATED_LAMBDA_SITES.with(|active| active.replace(sites));
    let _guard = LambdaSiteGuard { previous };
    action()
}

fn annotated_lambda_site_at(line: usize, column: usize) -> Option<AnnotatedLambdaSite> {
    ACTIVE_ANNOTATED_LAMBDA_SITES.with(|active| {
        active.borrow().iter().find(|site| site.line == line && site.column == column).cloned()
    })
}

fn normalize_annotated_lambda_source(source: &str) -> (String, Vec<AnnotatedLambdaSite>) {
    let mut normalized = source.as_bytes().to_vec();
    let mut sites = Vec::new();
    let mut search_from = 0usize;

    while let Some(lambda_start) = find_next_lambda_keyword(source, search_from) {
        search_from = lambda_start + "lambda".len();
        let Some(candidate) = parse_annotated_lambda_at(source, lambda_start) else {
            continue;
        };

        normalized[candidate.open_paren] = b' ';
        normalized[candidate.close_paren] = b' ';
        for (start, end) in candidate.annotation_spans {
            normalized[start..end].fill(b' ');
        }

        sites.push(AnnotatedLambdaSite {
            line: candidate.line,
            column: candidate.column,
            param_names: candidate.param_names,
            annotations: candidate.annotations,
        });
        search_from = candidate.close_paren + 1;
    }

    (String::from_utf8(normalized).expect("lambda normalization must preserve utf-8"), sites)
}

fn normalize_annotated_lambda_source_lossy(source: &str) -> String {
    normalize_annotated_lambda_source(source).0
}

struct AnnotatedLambdaCandidate {
    open_paren: usize,
    close_paren: usize,
    line: usize,
    column: usize,
    param_names: Vec<String>,
    annotations: Vec<Option<String>>,
    annotation_spans: Vec<(usize, usize)>,
}

fn find_next_lambda_keyword(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = start;
    let mut in_comment = false;
    let mut string_quote = None::<u8>;
    let mut string_triple = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_comment {
            if byte == b'\n' {
                in_comment = false;
            }
            index += 1;
            continue;
        }

        if let Some(quote) = string_quote {
            if string_triple {
                if byte == quote
                    && bytes.get(index + 1) == Some(&quote)
                    && bytes.get(index + 2) == Some(&quote)
                {
                    string_quote = None;
                    string_triple = false;
                    index += 3;
                    continue;
                }
                index += 1;
                continue;
            }

            if escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == quote {
                string_quote = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'#' => {
                in_comment = true;
                index += 1;
            }
            b'\'' | b'"' => {
                string_quote = Some(byte);
                string_triple =
                    bytes.get(index + 1) == Some(&byte) && bytes.get(index + 2) == Some(&byte);
                index += if string_triple { 3 } else { 1 };
            }
            _ if source.get(index..).is_some_and(|s| s.starts_with("lambda"))
                && is_lambda_keyword_boundary(bytes, index, index + "lambda".len()) =>
            {
                return Some(index);
            }
            _ => index += 1,
        }
    }

    None
}

fn is_lambda_keyword_boundary(bytes: &[u8], start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !is_identifier_byte(bytes[start - 1]);
    let after_ok = end >= bytes.len() || !is_identifier_byte(bytes[end]);
    before_ok && after_ok
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn parse_annotated_lambda_at(
    source: &str,
    lambda_start: usize,
) -> Option<AnnotatedLambdaCandidate> {
    let bytes = source.as_bytes();
    let mut cursor = lambda_start + "lambda".len();
    while let Some(byte) = bytes.get(cursor) {
        if byte.is_ascii_whitespace() {
            cursor += 1;
        } else {
            break;
        }
    }
    if bytes.get(cursor) != Some(&b'(') {
        return None;
    }

    let close_paren = find_matching_delimiter(source, cursor, b'(', b')')?;
    let mut body_colon = close_paren + 1;
    while let Some(byte) = bytes.get(body_colon) {
        if byte.is_ascii_whitespace() {
            body_colon += 1;
        } else {
            break;
        }
    }
    if bytes.get(body_colon) != Some(&b':') {
        return None;
    }

    let params_source = &source[cursor + 1..close_paren];
    let parsed = parse_annotated_lambda_params(params_source, cursor + 1);
    let (line, column) = offset_to_line_column(source, lambda_start);
    Some(AnnotatedLambdaCandidate {
        open_paren: cursor,
        close_paren,
        line,
        column,
        param_names: parsed.param_names,
        annotations: parsed.annotations,
        annotation_spans: parsed.annotation_spans,
    })
}

fn find_matching_delimiter(source: &str, open_index: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut index = open_index;
    let mut in_comment = false;
    let mut string_quote = None::<u8>;
    let mut string_triple = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_comment {
            if byte == b'\n' {
                in_comment = false;
            }
            index += 1;
            continue;
        }
        if let Some(quote) = string_quote {
            if string_triple {
                if byte == quote
                    && bytes.get(index + 1) == Some(&quote)
                    && bytes.get(index + 2) == Some(&quote)
                {
                    string_quote = None;
                    string_triple = false;
                    index += 3;
                    continue;
                }
                index += 1;
                continue;
            }
            if escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == quote {
                string_quote = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'#' => {
                in_comment = true;
                index += 1;
            }
            b'\'' | b'"' => {
                string_quote = Some(byte);
                string_triple =
                    bytes.get(index + 1) == Some(&byte) && bytes.get(index + 2) == Some(&byte);
                index += if string_triple { 3 } else { 1 };
            }
            _ if byte == open => {
                depth += 1;
                index += 1;
            }
            _ if byte == close => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
                index += 1;
            }
            _ => index += 1,
        }
    }

    None
}

struct ParsedAnnotatedLambdaParams {
    param_names: Vec<String>,
    annotations: Vec<Option<String>>,
    annotation_spans: Vec<(usize, usize)>,
}

fn parse_annotated_lambda_params(
    params_source: &str,
    absolute_start: usize,
) -> ParsedAnnotatedLambdaParams {
    let mut param_names = Vec::new();
    let mut annotations = Vec::new();
    let mut annotation_spans = Vec::new();

    for (start, end) in top_level_comma_ranges(params_source) {
        let item = &params_source[start..end];
        let trimmed = item.trim();
        if trimmed.is_empty() || trimmed == "/" || trimmed == "*" {
            continue;
        }

        let default_index = find_top_level_char(item, b'=');
        let header_end = default_index.unwrap_or(item.len());
        let annotation_index = find_top_level_char(&item[..header_end], b':');
        let name_end = annotation_index.unwrap_or(header_end);
        let mut name = item[..name_end].trim();
        if let Some(rest) = name.strip_prefix("**") {
            name = rest.trim();
        } else if let Some(rest) = name.strip_prefix('*') {
            name = rest.trim();
        }

        let annotation = annotation_index.and_then(|index| {
            let annotation_end = default_index.unwrap_or(item.len());
            let annotation = item[index + 1..annotation_end].trim();
            (!annotation.is_empty()).then(|| annotation.to_owned())
        });

        if let Some(index) = annotation_index {
            annotation_spans.push((
                absolute_start + start + index,
                absolute_start + start + default_index.unwrap_or(item.len()),
            ));
        }

        param_names.push(name.to_owned());
        annotations.push(annotation);
    }

    ParsedAnnotatedLambdaParams { param_names, annotations, annotation_spans }
}

fn top_level_comma_ranges(input: &str) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut index = 0usize;
    let bytes = input.as_bytes();
    let mut in_comment = false;
    let mut string_quote = None::<u8>;
    let mut string_triple = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_comment {
            if byte == b'\n' {
                in_comment = false;
            }
            index += 1;
            continue;
        }
        if let Some(quote) = string_quote {
            if string_triple {
                if byte == quote
                    && bytes.get(index + 1) == Some(&quote)
                    && bytes.get(index + 2) == Some(&quote)
                {
                    string_quote = None;
                    string_triple = false;
                    index += 3;
                    continue;
                }
                index += 1;
                continue;
            }
            if escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == quote {
                string_quote = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'#' => {
                in_comment = true;
                index += 1;
            }
            b'\'' | b'"' => {
                string_quote = Some(byte);
                string_triple =
                    bytes.get(index + 1) == Some(&byte) && bytes.get(index + 2) == Some(&byte);
                index += if string_triple { 3 } else { 1 };
            }
            b'(' => {
                paren_depth += 1;
                index += 1;
            }
            b')' => {
                paren_depth = paren_depth.saturating_sub(1);
                index += 1;
            }
            b'[' => {
                bracket_depth += 1;
                index += 1;
            }
            b']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                index += 1;
            }
            b'{' => {
                brace_depth += 1;
                index += 1;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                index += 1;
            }
            b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                parts.push((start, index));
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }
    parts.push((start, input.len()));
    parts
}

fn find_top_level_char(input: &str, target: u8) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut index = 0usize;
    let mut in_comment = false;
    let mut string_quote = None::<u8>;
    let mut string_triple = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_comment {
            if byte == b'\n' {
                in_comment = false;
            }
            index += 1;
            continue;
        }
        if let Some(quote) = string_quote {
            if string_triple {
                if byte == quote
                    && bytes.get(index + 1) == Some(&quote)
                    && bytes.get(index + 2) == Some(&quote)
                {
                    string_quote = None;
                    string_triple = false;
                    index += 3;
                    continue;
                }
                index += 1;
                continue;
            }
            if escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == quote {
                string_quote = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'#' => {
                in_comment = true;
                index += 1;
            }
            b'\'' | b'"' => {
                string_quote = Some(byte);
                string_triple =
                    bytes.get(index + 1) == Some(&byte) && bytes.get(index + 2) == Some(&byte);
                index += if string_triple { 3 } else { 1 };
            }
            b'(' => {
                paren_depth += 1;
                index += 1;
            }
            b')' => {
                paren_depth = paren_depth.saturating_sub(1);
                index += 1;
            }
            b'[' => {
                bracket_depth += 1;
                index += 1;
            }
            b']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                index += 1;
            }
            b'{' => {
                brace_depth += 1;
                index += 1;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                index += 1;
            }
            _ if byte == target && paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                return Some(index);
            }
            _ => index += 1,
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        AssertStatement, CallStatement, ClassMember, ClassMemberKind, ComprehensionKind,
        ComprehensionMetadata, DirectExprMetadata, ExceptionHandlerStatement, ForStatement,
        FunctionParam, FunctionStatement, GuardCondition, IfStatement, ImportBinding,
        ImportStatement, InvalidationKind, InvalidationStatement, LambdaMetadata,
        MatchCaseStatement, MatchPattern, MatchStatement, MemberAccessStatement,
        MethodCallStatement, MethodKind, NamedBlockStatement, ParseOptions, ParsePythonVersion,
        ParseTargetPlatform, ReturnStatement, SourceFile, SourceKind, SyntaxStatement,
        TypeAliasStatement, TypeExpr,
        TypeIgnoreDirective, TypeParam, TypeParamKind, TypedDictLiteralEntry, UnsafeStatement,
        ValueStatement, WithStatement, YieldStatement, parse, parse_with_options,
    };
    use std::path::PathBuf;

    macro_rules! assert_eq {
        ($tree:ident . statements, $expected:expr $(,)?) => {{
            let actual = normalize_expected_statements($tree.statements.clone());
            let expected = normalize_expected_statements($expected);
            ::std::assert_eq!(actual, expected);
        }};
        ($actual:expr, $expected:expr $(,)?) => {{
            ::std::assert_eq!($actual, $expected);
        }};
    }

    fn normalize_expected_statements(statements: Vec<SyntaxStatement>) -> Vec<SyntaxStatement> {
        statements.into_iter().map(normalize_statement).collect()
    }

    fn normalize_statement(statement: SyntaxStatement) -> SyntaxStatement {
        match statement {
            SyntaxStatement::TypeAlias(mut statement) => {
                statement.type_params =
                    statement.type_params.into_iter().map(normalize_type_param).collect();
                if statement.value_expr.is_none() {
                    statement.value_expr = TypeExpr::parse(&statement.value);
                }
                SyntaxStatement::TypeAlias(statement)
            }
            SyntaxStatement::Interface(statement) => {
                SyntaxStatement::Interface(normalize_named_block(statement))
            }
            SyntaxStatement::DataClass(statement) => {
                SyntaxStatement::DataClass(normalize_named_block(statement))
            }
            SyntaxStatement::SealedClass(statement) => {
                SyntaxStatement::SealedClass(normalize_named_block(statement))
            }
            SyntaxStatement::OverloadDef(statement) => {
                SyntaxStatement::OverloadDef(normalize_function_statement(statement))
            }
            SyntaxStatement::ClassDef(statement) => {
                SyntaxStatement::ClassDef(normalize_named_block(statement))
            }
            SyntaxStatement::FunctionDef(statement) => {
                SyntaxStatement::FunctionDef(normalize_function_statement(statement))
            }
            SyntaxStatement::Import(statement) => SyntaxStatement::Import(statement),
            SyntaxStatement::Value(statement) => SyntaxStatement::Value(normalize_value_statement(statement)),
            SyntaxStatement::Call(statement) => {
                SyntaxStatement::Call(normalize_call_statement(statement))
            }
            SyntaxStatement::MemberAccess(statement) => SyntaxStatement::MemberAccess(statement),
            SyntaxStatement::MethodCall(statement) => {
                SyntaxStatement::MethodCall(normalize_method_call_statement(statement))
            }
            SyntaxStatement::Return(statement) => {
                SyntaxStatement::Return(normalize_return_statement(statement))
            }
            SyntaxStatement::Yield(statement) => {
                SyntaxStatement::Yield(normalize_yield_statement(statement))
            }
            SyntaxStatement::If(statement) => SyntaxStatement::If(statement),
            SyntaxStatement::Assert(statement) => SyntaxStatement::Assert(statement),
            SyntaxStatement::Invalidate(statement) => SyntaxStatement::Invalidate(statement),
            SyntaxStatement::Match(statement) => SyntaxStatement::Match(statement),
            SyntaxStatement::For(statement) => SyntaxStatement::For(statement),
            SyntaxStatement::With(statement) => SyntaxStatement::With(statement),
            SyntaxStatement::ExceptHandler(statement) => SyntaxStatement::ExceptHandler(statement),
            SyntaxStatement::Unsafe(statement) => SyntaxStatement::Unsafe(statement),
        }
    }

    fn normalize_named_block(mut statement: NamedBlockStatement) -> NamedBlockStatement {
        statement.type_params =
            statement.type_params.into_iter().map(normalize_type_param).collect();
        statement.members = statement.members.into_iter().map(normalize_class_member).collect();
        statement
    }

    fn normalize_function_statement(mut statement: FunctionStatement) -> FunctionStatement {
        statement.type_params =
            statement.type_params.into_iter().map(normalize_type_param).collect();
        statement.params = statement.params.into_iter().map(normalize_function_param).collect();
        if statement.returns_expr.is_none() {
            statement.returns_expr = statement.returns.as_deref().and_then(TypeExpr::parse);
        }
        statement
    }

    fn normalize_function_param(mut param: FunctionParam) -> FunctionParam {
        if param.annotation_expr.is_none() {
            param.annotation_expr = param.annotation.as_deref().and_then(TypeExpr::parse);
        }
        param
    }

    fn normalize_type_param(mut param: TypeParam) -> TypeParam {
        if param.bound_expr.is_none() {
            param.bound_expr = param.bound.as_deref().and_then(TypeExpr::parse);
        }
        if param.constraint_exprs.is_empty() {
            param.constraint_exprs =
                param.constraints.iter().filter_map(|constraint| TypeExpr::parse(constraint)).collect();
        }
        if param.default_expr.is_none() {
            param.default_expr = param.default.as_deref().and_then(TypeExpr::parse);
        }
        param
    }

    fn normalize_class_member(mut member: ClassMember) -> ClassMember {
        member.params = member.params.into_iter().map(normalize_function_param).collect();
        if member.annotation_expr.is_none() {
            member.annotation_expr = member.annotation.as_deref().and_then(TypeExpr::parse);
        }
        if member.returns_expr.is_none() {
            member.returns_expr = member.returns.as_deref().and_then(TypeExpr::parse);
        }
        member
    }

    fn normalize_value_statement(mut statement: ValueStatement) -> ValueStatement {
        if statement.annotation_expr.is_none() {
            statement.annotation_expr = statement.annotation.as_deref().and_then(TypeExpr::parse);
        }
        if statement.value_type_expr.is_none() {
            statement.value_type_expr = statement.value_type.as_deref().and_then(TypeExpr::parse);
        }
        statement.value_subscript_target =
            normalize_direct_expr_option(statement.value_subscript_target);
        statement.value_if_true = normalize_direct_expr_option(statement.value_if_true);
        statement.value_if_false = normalize_direct_expr_option(statement.value_if_false);
        statement.value_bool_left = normalize_direct_expr_option(statement.value_bool_left);
        statement.value_bool_right = normalize_direct_expr_option(statement.value_bool_right);
        statement.value_binop_left = normalize_direct_expr_option(statement.value_binop_left);
        statement.value_binop_right = normalize_direct_expr_option(statement.value_binop_right);
        statement.value_lambda = normalize_lambda_option(statement.value_lambda);
        statement.value_list_comprehension =
            normalize_comprehension_option(statement.value_list_comprehension);
        statement.value_generator_comprehension =
            normalize_comprehension_option(statement.value_generator_comprehension);
        statement.value_list_elements = normalize_direct_expr_vec(statement.value_list_elements);
        statement.value_set_elements = normalize_direct_expr_vec(statement.value_set_elements);
        statement.value_dict_entries = normalize_typed_dict_literal_entries(statement.value_dict_entries);
        statement
    }

    fn normalize_call_statement(mut statement: CallStatement) -> CallStatement {
        statement.arg_values = statement.arg_values.into_iter().map(normalize_direct_expr).collect();
        statement.starred_arg_values =
            statement.starred_arg_values.into_iter().map(normalize_direct_expr).collect();
        statement.keyword_arg_values =
            statement.keyword_arg_values.into_iter().map(normalize_direct_expr).collect();
        statement.keyword_expansion_values = statement
            .keyword_expansion_values
            .into_iter()
            .map(normalize_direct_expr)
            .collect();
        statement
    }

    fn normalize_method_call_statement(mut statement: MethodCallStatement) -> MethodCallStatement {
        statement.arg_values = statement.arg_values.into_iter().map(normalize_direct_expr).collect();
        statement.starred_arg_values =
            statement.starred_arg_values.into_iter().map(normalize_direct_expr).collect();
        statement.keyword_arg_values =
            statement.keyword_arg_values.into_iter().map(normalize_direct_expr).collect();
        statement.keyword_expansion_values = statement
            .keyword_expansion_values
            .into_iter()
            .map(normalize_direct_expr)
            .collect();
        statement
    }

    fn normalize_return_statement(mut statement: ReturnStatement) -> ReturnStatement {
        statement.value_subscript_target =
            normalize_direct_expr_option(statement.value_subscript_target);
        statement.value_if_true = normalize_direct_expr_option(statement.value_if_true);
        statement.value_if_false = normalize_direct_expr_option(statement.value_if_false);
        statement.value_bool_left = normalize_direct_expr_option(statement.value_bool_left);
        statement.value_bool_right = normalize_direct_expr_option(statement.value_bool_right);
        statement.value_binop_left = normalize_direct_expr_option(statement.value_binop_left);
        statement.value_binop_right = normalize_direct_expr_option(statement.value_binop_right);
        statement.value_lambda = normalize_lambda_option(statement.value_lambda);
        statement.value_list_elements = normalize_direct_expr_vec(statement.value_list_elements);
        statement.value_set_elements = normalize_direct_expr_vec(statement.value_set_elements);
        statement.value_dict_entries = normalize_typed_dict_literal_entries(statement.value_dict_entries);
        statement
    }

    fn normalize_yield_statement(mut statement: YieldStatement) -> YieldStatement {
        statement.value_subscript_target =
            normalize_direct_expr_option(statement.value_subscript_target);
        statement.value_if_true = normalize_direct_expr_option(statement.value_if_true);
        statement.value_if_false = normalize_direct_expr_option(statement.value_if_false);
        statement.value_bool_left = normalize_direct_expr_option(statement.value_bool_left);
        statement.value_bool_right = normalize_direct_expr_option(statement.value_bool_right);
        statement.value_binop_left = normalize_direct_expr_option(statement.value_binop_left);
        statement.value_binop_right = normalize_direct_expr_option(statement.value_binop_right);
        statement.value_lambda = normalize_lambda_option(statement.value_lambda);
        statement.value_list_elements = normalize_direct_expr_vec(statement.value_list_elements);
        statement.value_set_elements = normalize_direct_expr_vec(statement.value_set_elements);
        statement.value_dict_entries = normalize_typed_dict_literal_entries(statement.value_dict_entries);
        statement
    }

    fn normalize_lambda_option(
        metadata: Option<Box<LambdaMetadata>>,
    ) -> Option<Box<LambdaMetadata>> {
        metadata.map(|metadata| Box::new(normalize_lambda_metadata(*metadata)))
    }

    fn normalize_lambda_metadata(mut metadata: LambdaMetadata) -> LambdaMetadata {
        metadata.params = metadata.params.into_iter().map(normalize_function_param).collect();
        metadata.body = Box::new(normalize_direct_expr(*metadata.body));
        metadata
    }

    fn normalize_comprehension_option(
        metadata: Option<Box<ComprehensionMetadata>>,
    ) -> Option<Box<ComprehensionMetadata>> {
        metadata.map(|metadata| Box::new(normalize_comprehension_metadata(*metadata)))
    }

    fn normalize_comprehension_metadata(mut metadata: ComprehensionMetadata) -> ComprehensionMetadata {
        metadata.clauses = metadata
            .clauses
            .into_iter()
            .map(|mut clause| {
                clause.iter = Box::new(normalize_direct_expr(*clause.iter));
                clause
            })
            .collect();
        metadata.key = metadata.key.map(|key| Box::new(normalize_direct_expr(*key)));
        metadata.element = Box::new(normalize_direct_expr(*metadata.element));
        metadata
    }

    fn normalize_direct_expr_option(
        metadata: Option<Box<DirectExprMetadata>>,
    ) -> Option<Box<DirectExprMetadata>> {
        metadata.map(|metadata| Box::new(normalize_direct_expr(*metadata)))
    }

    fn normalize_direct_expr_vec(
        values: Option<Vec<DirectExprMetadata>>,
    ) -> Option<Vec<DirectExprMetadata>> {
        values.map(|values| values.into_iter().map(normalize_direct_expr).collect())
    }

    fn normalize_typed_dict_literal_entries(
        entries: Option<Vec<TypedDictLiteralEntry>>,
    ) -> Option<Vec<TypedDictLiteralEntry>> {
        entries.map(|entries| {
            entries
                .into_iter()
                .map(|mut entry| {
                    entry.key_value = entry
                        .key_value
                        .map(|value| Box::new(normalize_direct_expr(*value)));
                    entry.value = normalize_direct_expr(entry.value);
                    entry
                })
                .collect()
        })
    }

    fn normalize_direct_expr(mut metadata: DirectExprMetadata) -> DirectExprMetadata {
        if metadata.value_type_expr.is_none() {
            metadata.value_type_expr = metadata.value_type.as_deref().and_then(TypeExpr::parse);
        }
        metadata.value_subscript_target =
            normalize_direct_expr_option(metadata.value_subscript_target);
        metadata.value_if_true = normalize_direct_expr_option(metadata.value_if_true);
        metadata.value_if_false = normalize_direct_expr_option(metadata.value_if_false);
        metadata.value_bool_left = normalize_direct_expr_option(metadata.value_bool_left);
        metadata.value_bool_right = normalize_direct_expr_option(metadata.value_bool_right);
        metadata.value_binop_left = normalize_direct_expr_option(metadata.value_binop_left);
        metadata.value_binop_right = normalize_direct_expr_option(metadata.value_binop_right);
        metadata.value_lambda = normalize_lambda_option(metadata.value_lambda);
        metadata.value_list_comprehension =
            normalize_comprehension_option(metadata.value_list_comprehension);
        metadata.value_generator_comprehension =
            normalize_comprehension_option(metadata.value_generator_comprehension);
        metadata.value_list_elements = normalize_direct_expr_vec(metadata.value_list_elements);
        metadata.value_set_elements = normalize_direct_expr_vec(metadata.value_set_elements);
        metadata.value_dict_entries = normalize_typed_dict_literal_entries(metadata.value_dict_entries);
        metadata
    }

    #[test]
    fn parse_recognizes_typepython_extension_headers() {
        let tree = parse(SourceFile {
            path: PathBuf::from("example.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: concat!(
                "typealias Pair[T] = tuple[T, T]\n",
                "interface Service:\n",
                "    pass\n",
                "data class Box:\n",
                "    pass\n",
                "sealed class Result:\n",
                "    pass\n",
                "overload def parse(value):\n",
                "    ...\n",
                "unsafe:\n",
                "    pass\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("Pair"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: None,
                        constraint_exprs: Vec::new(),
                        constraints: Vec::new(),
                        default_expr: None,
                        default: None,
                    }],
                    value: String::from("tuple[T, T]"),
                    value_expr: None,
                    line: 1,
                }),
                SyntaxStatement::Interface(NamedBlockStatement {
                    name: String::from("Service"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 2,
                }),
                SyntaxStatement::DataClass(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 4,
                }),
                SyntaxStatement::SealedClass(NamedBlockStatement {
                    name: String::from("Result"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 6,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: None,
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 8,
                }),
                SyntaxStatement::Unsafe(UnsafeStatement { line: 10 }),
            ]
        );
    }

    #[test]
    fn parse_captures_type_params_and_bounds() {
        let tree = parse(SourceFile {
            path: PathBuf::from("generic.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: concat!(
                "typealias Pair[T: Hashable] = tuple[T, T]\n",
                "interface Box[T]:\n",
                "    pass\n",
                "data class Node[T: Sequence[str]]:\n",
                "    pass\n",
                "sealed class Result[T]:\n",
                "    pass\n",
                "overload def first[T: Sequence[str]](value):\n",
                "    ...\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("Pair"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: Some(String::from("Hashable")),
                        constraint_exprs: Vec::new(),
                        constraints: Vec::new(),
                        default_expr: None,
                        default: None,
                    }],
                    value: String::from("tuple[T, T]"),
                    value_expr: None,
                    line: 1,
                }),
                SyntaxStatement::Interface(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: None,
                        constraint_exprs: Vec::new(),
                        constraints: Vec::new(),
                        default_expr: None,
                        default: None,
                    }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 2,
                }),
                SyntaxStatement::DataClass(NamedBlockStatement {
                    name: String::from("Node"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: Some(String::from("Sequence[str]")),
                        constraint_exprs: Vec::new(),
                        constraints: Vec::new(),
                        default_expr: None,
                        default: None,
                    }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 4,
                }),
                SyntaxStatement::SealedClass(NamedBlockStatement {
                    name: String::from("Result"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: None,
                        constraint_exprs: Vec::new(),
                        constraints: Vec::new(),
                        default_expr: None,
                        default: None,
                    }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 6,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("first"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: Some(String::from("Sequence[str]")),
                        constraint_exprs: Vec::new(),
                        constraints: Vec::new(),
                        default_expr: None,
                        default: None,
                    }],
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: None,
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 8,
                }),
            ]
        );
    }

    #[test]
    fn parse_reports_malformed_extension_headers() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: concat!(
                "typealias Pair tuple[int, int]\n",
                "interface:\n",
                "overload def parse\n",
                "unsafe\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.has_errors());
        let rendered = tree.diagnostics.as_text();
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("typealias declaration must contain `=`"));
        assert!(rendered.contains("interface declaration must include a valid name"));
        assert!(rendered.contains("overload declaration must end with `:`"));
        assert!(rendered.contains("unsafe block must start with `unsafe:`"));
    }

    #[test]
    fn parse_captures_type_param_constraints_and_defaults() {
        let tree = parse(SourceFile {
            path: PathBuf::from("generic-defaults.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: concat!(
                "typealias Pair[T = int] = tuple[T, T]\n",
                "interface Box[T: (str, bytes) = str]:\n",
                "    pass\n",
                "overload def first[T: (A, B)](value):\n",
                "    ...\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.is_empty(), "{}", tree.diagnostics.as_text());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("Pair"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: None,
                        constraint_exprs: Vec::new(),
                        constraints: Vec::new(),
                        default_expr: None,
                        default: Some(String::from("int")),
                    }],
                    value: String::from("tuple[T, T]"),
                    value_expr: None,
                    line: 1,
                }),
                SyntaxStatement::Interface(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: None,
                        constraint_exprs: Vec::new(),
                        constraints: vec![String::from("str"), String::from("bytes")],
                        default_expr: None,
                        default: Some(String::from("str")),
                    }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 2,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("first"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: None,
                        constraint_exprs: Vec::new(),
                        constraints: vec![String::from("A"), String::from("B")],
                        default_expr: None,
                        default: None,
                    }],
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: None,
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_reports_malformed_type_parameter_lists() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken-generics.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: concat!(
                "typealias Pair[T = ] = tuple[T, T]\n",
                "interface Box[T:] :\n",
                "overload def first[T: (, B)](value):\n",
                "class LaterDefault[T = int, U]:\n",
                "    pass\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.has_errors());
        let rendered = tree.diagnostics.as_text();
        assert!(rendered.contains("type parameter default must not be empty"));
        assert!(rendered.contains("type parameter bound must not be empty"));
        assert!(rendered.contains("type parameter constraint list must not contain empty entries"));
    }

    #[test]
    fn parse_reports_type_param_default_ordering() {
        let tree = parse(SourceFile {
            path: PathBuf::from("type-param-default-order.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("class LaterDefault[T = int, U]:\n    pass\n"),
        });

        assert!(tree.diagnostics.has_errors());
        let rendered = tree.diagnostics.as_text();
        assert!(rendered.contains("without a default cannot follow a parameter with a default"));
    }

    #[test]
    fn parse_reports_duplicate_type_parameter_names() {
        let tree = parse(SourceFile {
            path: PathBuf::from("duplicate-generics.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("class Box[T, T]:\n    pass\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4004"));
        assert!(rendered.contains("declares type parameter `T` more than once"));
    }

    #[test]
    fn parse_captures_interface_base_list_suffix() {
        let tree = parse(SourceFile {
            path: PathBuf::from("interface-bases.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("interface SupportsClose(Closable):\n    pass\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: Vec::new(),
                header_suffix: String::from("(Closable)"),
                bases: vec![String::from("Closable")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_rejects_executable_interface_bodies() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-interface.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("interface SupportsClose:\n    value = 1\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("body must not contain executable statements"));
    }

    #[test]
    fn parse_accepts_overload_simple_suite_form() {
        let tree = parse(SourceFile {
            path: PathBuf::from("overload-simple-suite.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("overload def parse(x: str) -> int: ...\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::OverloadDef(FunctionStatement {
                name: String::from("parse"),
                type_params: Vec::new(),
                params: vec![FunctionParam {
                    name: String::from("x"),
                    annotation: Some(String::from("str")),
                    annotation_expr: None,
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false
                }],
                returns: Some(String::from("int")),
                returns_expr: None,
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_rejects_executable_overload_bodies() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-overload.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("overload def parse(x: str) -> int:\n    return 1\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("body must not contain executable statements"));
    }

    #[test]
    fn parse_leaves_python_files_without_extension_analysis() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def unsafe(value):\n    return value\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("unsafe"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: None,
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("unsafe"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                })
            ]
        );
    }

    #[test]
    fn parse_reports_invalid_python_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def broken(:\n    return 1\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("Python syntax error"));
    }

    #[test]
    fn parse_accepts_valid_stub_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.pyi"),
            kind: SourceKind::Stub,
            logical_module: String::new(),
            text: String::from("def helper() -> int: ...\n"),
        });

        assert!(tree.diagnostics.is_empty());
    }

    #[test]
    fn parse_classifies_decorated_overloads_in_stub_sources() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.pyi"),
            kind: SourceKind::Stub,
            logical_module: String::new(),
            text: String::from(
                "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("overload"),
                        source_path: String::from("typing.overload"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("x"),
                        annotation: Some(String::from("str")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("int")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_reports_invalid_typepython_body_syntax_after_normalization() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("typealias UserId = int\ndef broken():\n    return )\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("TypePython syntax error"));
    }

    #[test]
    fn parse_reports_invalid_assignment_target_as_tpy4011() {
        let tree = parse(SourceFile {
            path: PathBuf::from("invalid_assign.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("def build() -> None:\n    (x + 1) = 2\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4011"));
        assert!(rendered.contains("Invalid assignment target"));
    }

    #[test]
    fn parse_reports_invalid_delete_target_as_tpy4011() {
        let tree = parse(SourceFile {
            path: PathBuf::from("invalid_del.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("def build() -> None:\n    del (x + 1)\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4011"));
        assert!(rendered.contains("Invalid delete target"));
    }

    #[test]
    fn parse_accepts_generic_python_headers_in_typepython_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("generic.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class Box[T]:\n    pass\n\ndef first[T](value: T) -> T:\n    return value\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: None,
                        constraint_exprs: Vec::new(),
                        constraints: Vec::new(),
                        default_expr: None,
                        default: None,
                    }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 1,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("first"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: None,
                        constraint_exprs: Vec::new(),
                        constraints: Vec::new(),
                        default_expr: None,
                        default: None,
                    }],
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("T")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("T")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 4,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("first"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 5,
                }),
            ]
        );
    }

    #[test]
    fn parse_accepts_generic_python_headers_with_constraints_and_defaults() {
        let tree = parse(SourceFile {
            path: PathBuf::from("generic-defaults.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class Box[T: (str, bytes) = str]:\n    pass\n\ndef first[T = int](value: T = 1) -> T:\n    return value\n",
            ),
        });

        assert!(tree.diagnostics.is_empty(), "{}", tree.diagnostics.as_text());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: None,
                        constraint_exprs: Vec::new(),
                        constraints: vec![String::from("str"), String::from("bytes")],
                        default_expr: None,
                        default: Some(String::from("str")),
                    }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 1,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("first"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound_expr: None,
                        bound: None,
                        constraint_exprs: Vec::new(),
                        constraints: Vec::new(),
                        default_expr: None,
                        default: Some(String::from("int")),
                    }],
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("T")),
                        annotation_expr: None,
                        has_default: true,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("T")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 4,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("first"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 5,
                }),
            ]
        );
    }

    #[test]
    fn parse_accepts_paramspec_type_params() {
        let tree = parse(SourceFile {
            path: PathBuf::from("paramspec.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "typealias Callback[**P, R] = Callable[P, R]\n\ndef invoke[**P, R](cb: Callable[P, R], *args: P.args, **kwargs: P.kwargs) -> R:\n    return cb(*args, **kwargs)\n",
            ),
        });

        assert!(tree.diagnostics.is_empty(), "{}", tree.diagnostics.as_text());
        let SyntaxStatement::TypeAlias(alias) = &tree.statements[0] else {
            panic!("expected type alias");
        };
        assert_eq!(alias.type_params[0].kind, TypeParamKind::ParamSpec);
        assert_eq!(alias.type_params[0].name, "P");
        assert_eq!(alias.type_params[1].kind, TypeParamKind::TypeVar);

        let SyntaxStatement::FunctionDef(function) = &tree.statements[1] else {
            panic!("expected function definition");
        };
        assert_eq!(function.type_params[0].kind, TypeParamKind::ParamSpec);
        assert_eq!(function.type_params[1].kind, TypeParamKind::TypeVar);
        assert_eq!(function.params[1].annotation.as_deref(), Some("P.args"));
        assert_eq!(function.params[2].annotation.as_deref(), Some("P.kwargs"));
    }

    #[test]
    fn render_type_params_supports_typevartuple_kind() {
        assert_eq!(
            super::render_type_params(&[
                TypeParam {
                    name: String::from("Ts"),
                    kind: TypeParamKind::TypeVarTuple,
                    bound_expr: None,
                    bound: None,
                    constraint_exprs: Vec::new(),
                    constraints: Vec::new(),
                    default_expr: None,
                    default: None,
                },
                TypeParam {
                    name: String::from("R"),
                    kind: TypeParamKind::TypeVar,
                    bound_expr: None,
                    bound: None,
                    constraint_exprs: Vec::new(),
                    constraints: Vec::new(),
                    default_expr: None,
                    default: None,
                },
            ]),
            "[*Ts, R]"
        );
    }

    #[test]
    fn parse_accepts_source_authored_typevartuple_syntax() {
        let tree = parse(SourceFile {
            path: PathBuf::from("variadic.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "typealias Pack[*Ts] = tuple[*Ts]\n\ndef collect[*Ts](*args: *Ts) -> tuple[*Ts]:\n    return args\n",
            ),
        });

        assert!(tree.diagnostics.is_empty(), "{}", tree.diagnostics.as_text());
        let SyntaxStatement::TypeAlias(alias) = &tree.statements[0] else {
            panic!("expected type alias");
        };
        assert_eq!(alias.type_params[0].kind, TypeParamKind::TypeVarTuple);
        assert_eq!(alias.value, "tuple[*Ts]");

        let SyntaxStatement::FunctionDef(function) = &tree.statements[1] else {
            panic!("expected function definition");
        };
        assert_eq!(function.type_params[0].kind, TypeParamKind::TypeVarTuple);
        assert_eq!(function.params[0].annotation.as_deref(), Some("*Ts"));
        assert_eq!(function.returns.as_deref(), Some("tuple[*Ts]"));
    }

    #[test]
    fn parse_extracts_imports_and_values_from_ast_body() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "from pkg import foo, bar as baz\nimport tools.helpers, more.tools as alias\nvalue: int = 1\na = b = 2\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        println!("{:?}", tree.statements);
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![
                        ImportBinding {
                            local_name: String::from("foo"),
                            source_path: String::from("pkg.foo"),
                        },
                        ImportBinding {
                            local_name: String::from("baz"),
                            source_path: String::from("pkg.bar"),
                        },
                    ],
                    line: 1,
                }),
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![
                        ImportBinding {
                            local_name: String::from("tools"),
                            source_path: String::from("tools.helpers"),
                        },
                        ImportBinding {
                            local_name: String::from("alias"),
                            source_path: String::from("more.tools"),
                        },
                    ],
                    line: 2,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("value")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("int")),
                    annotation_expr: None,
                    value_type: Some(String::from("int")),
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
                    owner_name: None,
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line: 3,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("a"), String::from("b")],
                    destructuring_target_names: None,
                    annotation: None,
                    annotation_expr: None,
                    value_type: Some(String::from("int")),
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
                    owner_name: None,
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_annotated_assignment_direct_rhs_forms() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "value: int = helper()\ncopy: str = source\nfield: str = box.value\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("value")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("int")),
                    annotation_expr: None,
                    value_type: Some(String::new()),
                    value_type_expr: None,
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    owner_name: None,
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line: 1,
                }),
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("helper"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("copy")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("str")),
                    annotation_expr: None,
                    value_type: Some(String::new()),
                    value_type_expr: None,
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("source")),
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
                    owner_name: None,
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("field")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("str")),
                    annotation_expr: None,
                    value_type: Some(String::new()),
                    value_type_expr: None,
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("box")),
                    value_member_name: Some(String::from("value")),
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
                    owner_name: None,
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line: 3,
                }),
                SyntaxStatement::MemberAccess(MemberAccessStatement {
                    current_owner_name: None,
                    current_owner_type_name: None,
                    owner_name: String::from("box"),
                    member: String::from("value"),
                    through_instance: false,
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_function_body_annotated_assignments() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(value: str) -> None:\n    result: int = value\n    item: str = helper()\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("str")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("None")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("result")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("int")),
                    annotation_expr: None,
                    value_type: Some(String::new()),
                    value_type_expr: None,
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }),
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("helper"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 3,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("item")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("str")),
                    annotation_expr: None,
                    value_type: Some(String::new()),
                    value_type_expr: None,
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_function_body_bare_assignments() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build() -> None:\n    value = helper()\n    field = box.item\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::FunctionDef(FunctionStatement { name, returns, line, .. })
                if name == "build" && returns.as_deref() == Some("None") && *line == 1
        )));
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Call(CallStatement { callee, line, .. })
                if callee == "helper" && *line == 2
        )));
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Value(ValueStatement {
                names,
                value_callee,
                owner_name,
                line,
                ..
            }) if names == &[String::from("value")]
                && value_callee.as_deref() == Some("helper")
                && owner_name.as_deref() == Some("build")
                && *line == 2
        )));
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Value(ValueStatement {
                names,
                value_member_owner_name,
                value_member_name,
                owner_name,
                line,
                ..
            }) if names == &[String::from("field")]
                && value_member_owner_name.as_deref() == Some("box")
                && value_member_name.as_deref() == Some("item")
                && owner_name.as_deref() == Some("build")
                && *line == 3
        )));
    }

    #[test]
    fn parse_distinguishes_destructuring_from_chained_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("destructure.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("a = b = pair\nleft, right = pair\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Value(chain), SyntaxStatement::Value(destructure)] =
            tree.statements.as_slice()
        else {
            panic!("expected two value statements");
        };
        assert_eq!(chain.names, vec![String::from("a"), String::from("b")]);
        assert_eq!(chain.destructuring_target_names, None);
        assert_eq!(destructure.names, vec![String::from("left"), String::from("right")]);
        assert_eq!(
            destructure.destructuring_target_names,
            Some(vec![String::from("left"), String::from("right")])
        );
    }

    #[test]
    fn parse_extracts_function_body_namedexpr_assignments() {
        let tree = parse(SourceFile {
            path: PathBuf::from("namedexpr.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build() -> int:\n    if (tmp := 1):\n        return tmp\n    return 0\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        let walrus_assignment = tree.statements.iter().find_map(|statement| match statement {
            SyntaxStatement::Value(statement)
                if statement.names == vec![String::from("tmp")]
                    && statement.owner_name.as_deref() == Some("build") =>
            {
                Some(statement)
            }
            _ => None,
        });
        let walrus_assignment = walrus_assignment.expect("named expression assignment statement");
        assert_eq!(walrus_assignment.annotation, None);
        assert_eq!(walrus_assignment.value_type.as_deref(), Some("int"));
        assert_eq!(walrus_assignment.value_name, None);
        assert_eq!(walrus_assignment.line, 2);
    }

    #[test]
    fn parse_reports_namedexpr_non_name_target_as_invalid_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("namedexpr-invalid.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("value: int = (box.item := 1)\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(rendered.contains("TPY4011"));
        assert!(rendered.contains("Assignment expression target must be an identifier"));
    }

    #[test]
    fn parse_normalizes_relative_import_provenance() {
        let tree = parse(SourceFile {
            path: PathBuf::from("src/app/child.py"),
            kind: SourceKind::Python,
            logical_module: String::from("app.child"),
            text: String::from("from .base import Base\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Import(ImportStatement {
                bindings: vec![ImportBinding {
                    local_name: String::from("Base"),
                    source_path: String::from("app.base.Base"),
                }],
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_extracts_top_level_direct_calls() {
        let tree = parse(SourceFile {
            path: PathBuf::from("calls.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("Builder()\nvalue = Factory()\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("Builder"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("value")],
                    destructuring_target_names: None,
                    annotation: None,
                    annotation_expr: None,
                    value_type: Some(String::new()),
                    value_type_expr: None,
                    is_awaited: false,
                    value_callee: Some(String::from("Factory")),
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
                    owner_name: None,
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }),
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("Factory"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_direct_call_keyword_names() {
        let tree = parse(SourceFile {
            path: PathBuf::from("call-keywords.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build(x=1, y=2)\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Call(CallStatement {
                callee: String::from("build"),
                arg_count: 0,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: vec![String::from("x"), String::from("y")],
                keyword_arg_types: vec![String::from("int"), String::from("int")],
                keyword_arg_values: vec![
                    DirectExprMetadata {
                        value_type: Some(String::from("int")),
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
                    },
                    DirectExprMetadata {
                        value_type: Some(String::from("int")),
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
                    },
                ],
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_collects_nested_calls_returns_and_assignments_in_control_flow_suites() {
        let tree = parse(SourceFile {
            path: PathBuf::from("nested-control-flow.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(flag, items, ctx):\n    while flag:\n        helper()\n        value = helper()\n    for item in items:\n        helper()\n    with ctx:\n        helper()\n    try:\n        helper()\n    except Exception:\n        helper()\n    finally:\n        return 1\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        let call_lines = tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Call(statement) if statement.callee == "helper" => {
                    Some(statement.line)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let value_lines = tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Value(statement)
                    if statement.names == vec![String::from("value")] =>
                {
                    Some(statement.line)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let return_lines = tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Return(statement) if statement.owner_name == "build" => {
                    Some(statement.line)
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(call_lines, vec![3, 4, 6, 8, 10, 12]);
        assert_eq!(value_lines, vec![4]);
        assert_eq!(return_lines, vec![14]);
    }

    #[test]
    fn parse_retains_direct_call_literal_arg_types() {
        let tree = parse(SourceFile {
            path: PathBuf::from("call-types.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build(1, \"x\")\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Call(CallStatement {
                callee: String::from("build"),
                arg_count: 2,
                arg_types: vec![String::from("int"), String::from("str")],
                arg_values: vec![
                    DirectExprMetadata {
                        value_type: Some(String::from("int")),
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
                    },
                    DirectExprMetadata {
                        value_type: Some(String::from("str")),
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
                    },
                ],
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_retains_direct_call_container_literal_arg_types() {
        let tree = parse(SourceFile {
            path: PathBuf::from("call-container-types.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build([1, 2], (1, \"x\"), {\"x\": 1}, {1, 2})\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Call(statement)] = tree.statements.as_slice() else {
            panic!("expected direct call statement");
        };
        assert_eq!(statement.callee, "build");
        assert_eq!(
            statement.arg_types,
            vec![
                String::from("list[int]"),
                String::from("tuple[int, str]"),
                String::from("dict[str, int]"),
                String::from("set[int]"),
            ]
        );
        let list_elements =
            statement.arg_values[0].value_list_elements.as_ref().expect("list elements");
        assert_eq!(list_elements.len(), 2);
        assert_eq!(list_elements[0].value_type.as_deref(), Some("int"));
        let dict_entries =
            statement.arg_values[2].value_dict_entries.as_ref().expect("dict entries");
        assert_eq!(dict_entries.len(), 1);
        assert_eq!(dict_entries[0].key.as_deref(), Some("x"));
        assert!(!dict_entries[0].is_expansion);
        let set_elements =
            statement.arg_values[3].value_set_elements.as_ref().expect("set elements");
        assert_eq!(set_elements.len(), 2);
        assert_eq!(set_elements[0].value_type.as_deref(), Some("int"));
    }

    #[test]
    fn parse_retains_starred_call_expansion_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("call-starred.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build(*[1, 2], **{\"x\": 1})\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Call(statement)] = tree.statements.as_slice() else {
            panic!("expected direct call statement");
        };
        assert_eq!(statement.starred_arg_types, vec![String::from("list[int]")]);
        assert_eq!(statement.keyword_expansion_types, vec![String::from("dict[str, int]")]);
        let dict_entries = statement.keyword_expansion_values[0]
            .value_dict_entries
            .as_ref()
            .expect("keyword expansion dict entries");
        assert_eq!(dict_entries.len(), 1);
        assert_eq!(dict_entries[0].key.as_deref(), Some("x"));
        assert!(!dict_entries[0].is_expansion);
    }

    #[test]
    fn parse_retains_typed_dict_literal_entries_in_call_args() {
        let tree = parse(SourceFile {
            path: PathBuf::from("call-typeddict.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build({\"id\": 1}, user={\"name\": \"Ada\"})\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Call(statement)] = tree.statements.as_slice() else {
            panic!("expected direct call statement");
        };
        let positional_entries =
            statement.arg_values[0].value_dict_entries.as_ref().expect("positional dict entries");
        assert_eq!(positional_entries.len(), 1);
        assert_eq!(positional_entries[0].key.as_deref(), Some("id"));
        let keyword_entries = statement.keyword_arg_values[0]
            .value_dict_entries
            .as_ref()
            .expect("keyword dict entries");
        assert_eq!(keyword_entries.len(), 1);
        assert_eq!(keyword_entries[0].key.as_deref(), Some("name"));
    }

    #[test]
    fn parse_retains_lambda_metadata_in_call_args() {
        let tree = parse(SourceFile {
            path: PathBuf::from("lambda.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build(lambda x: x)\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Call(CallStatement {
                callee: String::from("build"),
                arg_count: 1,
                arg_types: vec![String::new()],
                arg_values: vec![DirectExprMetadata {
                    value_type: Some(String::new()),
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
                    value_lambda: Some(Box::new(LambdaMetadata {
                        params: vec![FunctionParam {
                            name: String::from("x"),
                            annotation: None,
                            annotation_expr: None,
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false,
                        }],
                        body: Box::new(DirectExprMetadata {
                            value_type: Some(String::new()),
                            value_type_expr: None,
                            is_awaited: false,
                            value_callee: None,
                            value_name: Some(String::from("x")),
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
                        }),
                    })),
                    value_list_comprehension: None,
                    value_generator_comprehension: None,
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                }],
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_accepts_typepython_lambda_parameter_annotations() {
        let tree = parse(SourceFile {
            path: PathBuf::from("lambda-annotated.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("build(lambda (x: int, y: str): x)\n"),
        });

        assert!(tree.diagnostics.is_empty(), "{}", tree.diagnostics.as_text());
        let SyntaxStatement::Call(call) = &tree.statements[0] else {
            panic!("expected call statement");
        };
        let lambda = call.arg_values[0].value_lambda.as_ref().expect("lambda metadata");
        assert_eq!(
            lambda.params,
            vec![
                FunctionParam {
                    name: String::from("x"),
                    annotation: Some(String::from("int")),
                    annotation_expr: None,
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                },
                FunctionParam {
                    name: String::from("y"),
                    annotation: Some(String::from("str")),
                    annotation_expr: None,
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                },
            ]
        );
    }

    #[test]
    fn parse_retains_list_comprehension_metadata_in_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("listcomp.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("values = [x + 1 for x in [1, 2] if x is not None]\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Value(statement)] = tree.statements.as_slice() else {
            panic!("expected value statement");
        };
        assert_eq!(statement.names, vec![String::from("values")]);
        let comprehension =
            statement.value_list_comprehension.as_deref().expect("list comprehension");
        assert_eq!(comprehension.kind, ComprehensionKind::List);
        assert_eq!(comprehension.clauses.len(), 1);
        assert_eq!(comprehension.clauses[0].target_names, vec![String::from("x")]);
        let iter_elements =
            comprehension.clauses[0].iter.value_list_elements.as_ref().expect("iter list elements");
        assert_eq!(iter_elements.len(), 2);
        assert_eq!(iter_elements[0].value_type.as_deref(), Some("int"));
        assert_eq!(
            comprehension.clauses[0].filters,
            vec![GuardCondition::IsNone { name: String::from("x"), negated: true }]
        );
        assert_eq!(comprehension.element.value_binop_operator.as_deref(), Some("+"));
    }

    #[test]
    fn parse_retains_generator_comprehension_metadata_in_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("gencomp.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("values = (x + 1 for x in [1, 2] if x is not None)\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Value(statement)] = tree.statements.as_slice() else {
            panic!("expected value statement");
        };
        assert_eq!(statement.names, vec![String::from("values")]);
        let comprehension =
            statement.value_generator_comprehension.as_deref().expect("generator comprehension");
        assert_eq!(comprehension.kind, ComprehensionKind::Generator);
        assert_eq!(comprehension.clauses.len(), 1);
        assert_eq!(comprehension.clauses[0].target_names, vec![String::from("x")]);
        let iter_elements =
            comprehension.clauses[0].iter.value_list_elements.as_ref().expect("iter list elements");
        assert_eq!(iter_elements.len(), 2);
        assert_eq!(iter_elements[0].value_type.as_deref(), Some("int"));
        assert_eq!(
            comprehension.clauses[0].filters,
            vec![GuardCondition::IsNone { name: String::from("x"), negated: true }]
        );
        assert_eq!(comprehension.element.value_binop_operator.as_deref(), Some("+"));
    }

    #[test]
    fn parse_retains_set_comprehension_metadata_in_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("setcomp.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("values = {x + 1 for x in [1, 2]}\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let SyntaxStatement::Value(statement) = &tree.statements[0] else {
            panic!("expected value statement");
        };
        let comprehension =
            statement.value_list_comprehension.as_deref().expect("set comprehension");
        assert_eq!(comprehension.kind, ComprehensionKind::Set);
        assert!(comprehension.key.is_none());
        assert_eq!(comprehension.clauses.len(), 1);
        assert_eq!(comprehension.clauses[0].target_names, vec![String::from("x")]);
    }

    #[test]
    fn parse_retains_dict_comprehension_metadata_in_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("dictcomp.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("values = {x: x + 1 for x in [1, 2]}\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let SyntaxStatement::Value(statement) = &tree.statements[0] else {
            panic!("expected value statement");
        };
        let comprehension =
            statement.value_list_comprehension.as_deref().expect("dict comprehension");
        assert_eq!(comprehension.kind, ComprehensionKind::Dict);
        assert!(comprehension.key.is_some());
        assert_eq!(comprehension.clauses.len(), 1);
        assert_eq!(comprehension.clauses[0].target_names, vec![String::from("x")]);
    }

    #[test]
    fn parse_retains_ifexp_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("ifexp.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("value: int = 1 if True else 2\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("value")],
                destructuring_target_names: None,
                annotation: Some(String::from("int")),
                annotation_expr: None,
                value_type: Some(String::new()),
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
                value_if_true: Some(Box::new(DirectExprMetadata {
                    value_type: Some(String::from("int")),
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
                })),
                value_if_false: Some(Box::new(DirectExprMetadata {
                    value_type: Some(String::from("int")),
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
                })),
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
                owner_name: None,
                owner_type_name: None,
                is_final: false,
                is_class_var: false,
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_retains_ifexp_guard_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("ifexp-guard.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("value: str = data if data is not None else \"\"\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let SyntaxStatement::Value(statement) = &tree.statements[0] else {
            panic!("expected value statement");
        };
        assert_eq!(
            statement.value_if_guard,
            Some(GuardCondition::IsNone { name: String::from("data"), negated: true })
        );
    }

    #[test]
    fn parse_extracts_nested_direct_calls() {
        let tree = parse(SourceFile {
            path: PathBuf::from("nested-calls.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def build() -> None:\n    Factory()\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("None")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("Factory"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_return_literals() {
        let tree = parse(SourceFile {
            path: PathBuf::from("returns.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def build() -> int:\n    return \"x\"\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("int")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::from("str")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_bool_and_none_return_literals() {
        let tree = parse(SourceFile {
            path: PathBuf::from("returns.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def truthy() -> bool:\n    return True\n\ndef missing() -> None:\n    return None\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("truthy"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("bool")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("truthy"),
                    owner_type_name: None,
                    value_type: Some(String::from("bool")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("missing"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("None")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 4,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("missing"),
                    owner_type_name: None,
                    value_type: Some(String::from("None")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 5,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_return_call_callee() {
        let tree = parse(SourceFile {
            path: PathBuf::from("returns.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def build() -> int:\n    return helper()\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("int")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_return_member_access() {
        let tree = parse(SourceFile {
            path: PathBuf::from("returns.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def build(box: Box) -> str:\n    return box.value\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("box"),
                        annotation: Some(String::from("Box")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("str")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("box")),
                    value_member_name: Some(String::from("value")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_member_accesses() {
        let tree = parse(SourceFile {
            path: PathBuf::from("member-access.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("Box.missing\nBox().value\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::MemberAccess(MemberAccessStatement {
                    current_owner_name: None,
                    current_owner_type_name: None,
                    owner_name: String::from("Box"),
                    member: String::from("missing"),
                    through_instance: false,
                    line: 1,
                }),
                SyntaxStatement::MemberAccess(MemberAccessStatement {
                    current_owner_name: None,
                    current_owner_type_name: None,
                    owner_name: String::from("Box"),
                    member: String::from("value"),
                    through_instance: true,
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_method_calls() {
        let tree = parse(SourceFile {
            path: PathBuf::from("method-call.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("Box.run(1)\nBox().build(x=1)\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::MethodCall(MethodCallStatement {
                    owner_name: String::from("Box"),
                    method: String::from("run"),
                    through_instance: false,
                    arg_count: 1,
                    arg_types: vec![String::from("int")],
                    arg_values: vec![DirectExprMetadata {
                        value_type: Some(String::from("int")),
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
                    }],
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }),
                SyntaxStatement::MethodCall(MethodCallStatement {
                    owner_name: String::from("Box"),
                    method: String::from("build"),
                    through_instance: true,
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: vec![String::from("x")],
                    keyword_arg_types: vec![String::from("int")],
                    keyword_arg_values: vec![DirectExprMetadata {
                        value_type: Some(String::from("int")),
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
                    }],
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_nested_direct_method_calls() {
        let tree = parse(SourceFile {
            path: PathBuf::from("nested-method-call.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def run() -> None:\n    Box.run(1)\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::MethodCall(MethodCallStatement {
                owner_name,
                method,
                through_instance: false,
                ..
            }) if owner_name == "Box" && method == "run"
        )));
    }

    #[test]
    fn parse_extracts_class_like_members_from_ast_body() {
        let tree = parse(SourceFile {
            path: PathBuf::from("members.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class Box:\n    value: int\n    total = 1\n    def get(self) -> int: ...\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        println!("{:?}", tree.statements);
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("Box"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![
                    ClassMember {
                        name: String::from("value"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("int")),
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
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("total"),
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
                        line: 3,
                    },
                    ClassMember {
                        name: String::from("get"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        annotation_expr: None,
                        value_type: None,
                        params: vec![FunctionParam {
                            name: String::from("self"),
                            annotation: None,
                            annotation_expr: None,
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        }],
                        returns: Some(String::from("int")),
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 4,
                    },
                ],
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_marks_decorated_class_methods_as_overload_members() {
        let tree = parse(SourceFile {
            path: PathBuf::from("class-overloads.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "from typing import overload\n\nclass Parser:\n    @overload\n    def parse(self, x: str) -> int: ...\n\n    def parse(self, x):\n        return 0\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("overload"),
                        source_path: String::from("typing.overload"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Parser"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![
                    ClassMember {
                        name: String::from("parse"),
                        kind: ClassMemberKind::Overload,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        annotation_expr: None,
                        value_type: None,
                        params: vec![
                                FunctionParam {
                                    name: String::from("self"),
                                    annotation: None,
                                    annotation_expr: None,
                                    has_default: false,
                                    positional_only: false,
                                    keyword_only: false,
                                    variadic: false,
                                    keyword_variadic: false
                                },
                                FunctionParam {
                                    name: String::from("x"),
                                    annotation: Some(String::from("str")),
                                    annotation_expr: None,
                                    has_default: false,
                                    positional_only: false,
                                    keyword_only: false,
                                    variadic: false,
                                    keyword_variadic: false
                                },
                            ],
                            returns: Some(String::from("int")),
                            returns_expr: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 4,
                        },
                        ClassMember {
                            name: String::from("parse"),
                            kind: ClassMemberKind::Method,
                            method_kind: Some(MethodKind::Instance),
                            annotation: None,
                            annotation_expr: None,
                            value_type: None,
                            params: vec![
                                FunctionParam {
                                    name: String::from("self"),
                                    annotation: None,
                                    annotation_expr: None,
                                    has_default: false,
                                    positional_only: false,
                                    keyword_only: false,
                                    variadic: false,
                                    keyword_variadic: false
                                },
                                FunctionParam {
                                    name: String::from("x"),
                                    annotation: None,
                                    annotation_expr: None,
                                    has_default: false,
                                    positional_only: false,
                                    keyword_only: false,
                                    variadic: false,
                                    keyword_variadic: false
                                },
                            ],
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
                            line: 7,
                        },
                    ],
                    line: 3,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("parse"),
                    owner_type_name: Some(String::from("Parser")),
                    value_type: Some(String::from("int")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 8,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_final_value_declarations() {
        let tree = parse(SourceFile {
            path: PathBuf::from("finals.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import Final\nMAX_SIZE: Final = 100\nclass Box:\n    limit: Final[int] = 1\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("Final"),
                        source_path: String::from("typing.Final"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("MAX_SIZE")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("Final")),
                    annotation_expr: None,
                    value_type: Some(String::from("int")),
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
                    owner_name: None,
                    owner_type_name: None,
                    is_final: true,
                    is_class_var: false,
                    line: 2,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("limit"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("Final[int]")),
                        annotation_expr: None,
                        value_type: Some(String::from("int")),
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: true,
                        is_class_var: false,
                        line: 4,
                    }],
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_collects_imports_inside_type_checking_guards() {
        let tree = parse(SourceFile {
            path: PathBuf::from("type-checking-imports.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from app.models import User\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Import(ImportStatement { bindings, line })
                if *line == 3
                    && bindings == &vec![ImportBinding {
                        local_name: String::from("User"),
                        source_path: String::from("app.models.User"),
                    }]
        )), "{:?}", tree.statements);
    }

    #[test]
    fn parse_collects_imports_inside_qualified_type_checking_guards() {
        let tree = parse(SourceFile {
            path: PathBuf::from("qualified-type-checking-imports.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "import typing\nif typing.TYPE_CHECKING:\n    from app.models import User\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Import(ImportStatement { bindings, line })
                if *line == 3
                    && bindings == &vec![ImportBinding {
                        local_name: String::from("User"),
                        source_path: String::from("app.models.User"),
                    }]
        )), "{:?}", tree.statements);
    }

    #[test]
    fn parse_collects_imports_inside_version_guards_for_selected_target() {
        let tree = parse_with_options(
            SourceFile {
                path: PathBuf::from("version-guard-imports.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::from(
                    "import sys\nif sys.version_info >= (3, 11):\n    from app.models import NewUser\nelse:\n    from app.models import OldUser\n",
                ),
            },
            ParseOptions {
                target_python: Some(ParsePythonVersion { major: 3, minor: 11 }),
                ..ParseOptions::default()
            },
        );

        assert!(tree.diagnostics.is_empty());
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Import(ImportStatement { bindings, line })
                if *line == 3
                    && bindings == &vec![ImportBinding {
                        local_name: String::from("NewUser"),
                        source_path: String::from("app.models.NewUser"),
                    }]
        )), "{:?}", tree.statements);
        assert!(!tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Import(ImportStatement { bindings, line })
                if *line == 5
                    && bindings == &vec![ImportBinding {
                        local_name: String::from("OldUser"),
                        source_path: String::from("app.models.OldUser"),
                    }]
        )), "{:?}", tree.statements);
    }

    #[test]
    fn parse_collects_imports_inside_platform_guards_for_selected_target() {
        let tree = parse_with_options(
            SourceFile {
                path: PathBuf::from("platform-guard-imports.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::from(
                    "import sys\nif sys.platform == \"darwin\":\n    from app.models import MacOnly\nelse:\n    from app.models import Other\n",
                ),
            },
            ParseOptions {
                target_platform: Some(ParseTargetPlatform::Darwin),
                ..ParseOptions::default()
            },
        );

        assert!(tree.diagnostics.is_empty());
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Import(ImportStatement { bindings, line })
                if *line == 3
                    && bindings == &vec![ImportBinding {
                        local_name: String::from("MacOnly"),
                        source_path: String::from("app.models.MacOnly"),
                    }]
        )), "{:?}", tree.statements);
    }

    #[test]
    fn parse_collects_class_declarations_inside_type_checking_guards() {
        let tree = parse_with_options(
            SourceFile {
                path: PathBuf::from("type-checking-class.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::from(
                    "import typing\nif typing.TYPE_CHECKING:\n    class User:\n        pass\n",
                ),
            },
            ParseOptions::default(),
        );

        assert!(tree.diagnostics.is_empty());
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::ClassDef(NamedBlockStatement { name, line, .. })
                if name == "User" && *line == 3
        )), "{:?}", tree.statements);
    }

    #[test]
    fn parse_filters_guarded_typealias_declarations_by_selected_branch() {
        let tree = parse_with_options(
            SourceFile {
                path: PathBuf::from("guarded-typealias.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "import typing\nif typing.TYPE_CHECKING:\n    typealias UserId = int\nelse:\n    typealias UserId = str\n",
                ),
            },
            ParseOptions::default(),
        );

        assert!(tree.diagnostics.is_empty());
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::TypeAlias(TypeAliasStatement { name, value, line, .. })
                if name == "UserId" && value == "int" && *line == 3
        )), "{:?}", tree.statements);
        assert!(!tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::TypeAlias(TypeAliasStatement { name, value, line, .. })
                if name == "UserId" && value == "str" && *line == 5
        )), "{:?}", tree.statements);
    }

    #[test]
    fn parse_marks_final_decorated_classes_and_methods() {
        let tree = parse(SourceFile {
            path: PathBuf::from("final-decorators.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import final\n\n@final\nclass Base:\n    @final\n    def run(self) -> None:\n        pass\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("final"),
                        source_path: String::from("typing.final"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Base"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: true,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("run"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        annotation_expr: None,
                        value_type: None,
                        params: vec![FunctionParam {
                            name: String::from("self"),
                            annotation: None,
                            annotation_expr: None,
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        }],
                    returns: Some(String::from("None")),
                    returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: true,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 5,
                    }],
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_classvar_value_declarations() {
        let tree = parse(SourceFile {
            path: PathBuf::from("classvars.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import ClassVar\nVALUE: ClassVar[int] = 1\nclass Box:\n    cache: ClassVar[int] = 2\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("ClassVar"),
                        source_path: String::from("typing.ClassVar"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("VALUE")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("ClassVar[int]")),
                    annotation_expr: None,
                    value_type: Some(String::from("int")),
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
                    owner_name: None,
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: true,
                    line: 2,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("cache"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("ClassVar[int]")),
                        annotation_expr: None,
                        value_type: Some(String::from("int")),
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
                        is_class_var: true,
                        line: 4,
                    }],
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_rejects_classvar_inside_function_body() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-classvar.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import ClassVar\n\ndef build() -> None:\n    value: ClassVar[int] = 1\n",
            ),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("ClassVar[...] is not allowed inside function or method bodies"));
    }

    #[test]
    fn parse_rejects_classvar_in_parameter_position() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-classvar-param.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import ClassVar\n\ndef build(value: ClassVar[int]) -> None:\n    pass\n",
            ),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("parameter annotations"));
    }

    #[test]
    fn parse_rejects_final_in_parameter_position() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-final-param.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import Final\n\ndef build(value: Final[int]) -> None:\n    pass\n",
            ),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4010"));
        assert!(rendered.contains("deferred beyond v1"));
    }

    #[test]
    fn parse_accepts_async_constructs_in_typepython_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("async-deferred.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "async def fetch() -> int:\n    await work()\n    async for item in stream:\n        pass\n    async with manager:\n        pass\n\ndef produce():\n    yield 1\n\ndef relay():\n    yield from produce()\n",
            ),
        });

        assert!(tree.diagnostics.is_empty(), "{}", tree.diagnostics.as_text());
        assert!(!tree.statements.is_empty());
    }

    #[test]
    fn parse_allows_async_constructs_in_python_passthrough_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("async.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("async def fetch() -> int:\n    return 1\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(!rendered.contains("TPY4010"));
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("fetch"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("int")),
                    returns_expr: None,
                    is_async: true,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("fetch"),
                    owner_type_name: None,
                    value_type: Some(String::from("int")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_direct_await_in_python_passthrough_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("await.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "async def fetch() -> int:\n    return 1\n\nasync def build() -> int:\n    return await fetch()\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("fetch"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("int")),
                    returns_expr: None,
                    is_async: true,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("fetch"),
                    owner_type_name: None,
                    value_type: Some(String::from("int")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("int")),
                    returns_expr: None,
                    is_async: true,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 4,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: true,
                    value_callee: Some(String::from("fetch")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 5,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_direct_yield_in_python_passthrough_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("yield.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def produce() -> Generator[int, None, None]:\n    yield 1\n\ndef relay() -> Generator[int, None, None]:\n    yield from values\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("produce"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("Generator[int, None, None]")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Yield(YieldStatement {
                    owner_name: String::from("produce"),
                    owner_type_name: None,
                    value_type: Some(String::from("int")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    is_yield_from: false,
                    line: 2,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("relay"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("Generator[int, None, None]")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 4,
                }),
                SyntaxStatement::Yield(YieldStatement {
                    owner_name: String::from("relay"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    value_callee: None,
                    value_name: Some(String::from("values")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    is_yield_from: true,
                    line: 5,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_direct_method_call_result_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("methods.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(box: Box) -> str:\n    result: str = box.get()\n    return box.get()\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Value(ValueStatement {
                names,
                annotation,
                value_method_owner_name,
                value_method_name,
                owner_name,
                line,
                ..
            }) if names == &[String::from("result")]
                && annotation.as_deref() == Some("str")
                && value_method_owner_name.as_deref() == Some("box")
                && value_method_name.as_deref() == Some("get")
                && owner_name.as_deref() == Some("build")
                && *line == 2
        )));
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Return(ReturnStatement {
                owner_name,
                value_method_owner_name,
                value_method_name,
                line,
                ..
            }) if owner_name == "build"
                && value_method_owner_name.as_deref() == Some("box")
                && value_method_name.as_deref() == Some("get")
                && *line == 3
        )));
        assert!(
            tree.statements
                .iter()
                .any(|statement| matches!(
                    statement,
                    SyntaxStatement::MethodCall(MethodCallStatement {
                        owner_name,
                        method,
                        through_instance: false,
                        line,
                        ..
                    }) if owner_name == "box" && method == "get" && *line == 3
                ))
            , "{:?}", tree.statements
        );
    }

    #[test]
    fn parse_retains_direct_method_call_result_metadata_through_instance() {
        let tree = parse(SourceFile {
            path: PathBuf::from("methods.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def build() -> str:\n    return make_box().get()\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Return(ReturnStatement {
                owner_name,
                value_method_owner_name,
                value_method_name,
                value_method_through_instance: true,
                line,
                ..
            }) if owner_name == "build"
                && value_method_owner_name.as_deref() == Some("make_box")
                && value_method_name.as_deref() == Some("get")
                && *line == 2
        )), "{:?}", tree.statements);
        assert!(tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::MethodCall(MethodCallStatement {
                owner_name,
                method,
                through_instance: true,
                line,
                ..
            }) if owner_name == "make_box" && method == "get" && *line == 2
        )), "{:?}", tree.statements);
    }

    #[test]
    fn parse_retains_simple_for_loop_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("for_loop.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(values: list[int]) -> int:\n    for item in values:\n        pass\n    return item\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("values"),
                        annotation: Some(String::from("list[int]")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("int")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::For(ForStatement {
                    target_name: String::from("item"),
                    target_names: Vec::new(),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    iter_type: Some(String::new()),
                    iter_is_awaited: false,
                    iter_callee: None,
                    iter_name: Some(String::from("values")),
                    iter_member_owner_name: None,
                    iter_member_name: None,
                    iter_member_through_instance: false,
                    iter_method_owner_name: None,
                    iter_method_name: None,
                    iter_method_through_instance: false,
                    line: 2,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("item")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_tuple_for_loop_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("for_loop.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(pairs: tuple[tuple[int, str]]) -> str:\n    for a, b in pairs:\n        pass\n    return b\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("pairs"),
                        annotation: Some(String::from("tuple[tuple[int, str]]")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("str")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::For(ForStatement {
                    target_name: String::new(),
                    target_names: vec![String::from("a"), String::from("b")],
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    iter_type: Some(String::new()),
                    iter_is_awaited: false,
                    iter_callee: None,
                    iter_name: Some(String::from("pairs")),
                    iter_member_owner_name: None,
                    iter_member_name: None,
                    iter_member_through_instance: false,
                    iter_method_owner_name: None,
                    iter_method_name: None,
                    iter_method_through_instance: false,
                    line: 2,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("b")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_simple_with_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("with_stmt.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(manager: Manager) -> str:\n    with manager as value:\n        pass\n    return value\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("manager"),
                        annotation: Some(String::from("Manager")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("str")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::With(WithStatement {
                    target_name: Some(String::from("value")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    context_type: Some(String::new()),
                    context_is_awaited: false,
                    context_callee: None,
                    context_name: Some(String::from("manager")),
                    context_member_owner_name: None,
                    context_member_name: None,
                    context_member_through_instance: false,
                    context_method_owner_name: None,
                    context_method_name: None,
                    context_method_through_instance: false,
                    line: 2,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_with_item_without_target() {
        let tree = parse(SourceFile {
            path: PathBuf::from("with_stmt.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(manager: Manager) -> None:\n    with manager:\n        pass\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("manager"),
                        annotation: Some(String::from("Manager")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("None")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::With(WithStatement {
                    target_name: None,
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    context_type: Some(String::new()),
                    context_is_awaited: false,
                    context_callee: None,
                    context_name: Some(String::from("manager")),
                    context_member_owner_name: None,
                    context_member_name: None,
                    context_member_through_instance: false,
                    context_method_owner_name: None,
                    context_method_name: None,
                    context_method_through_instance: false,
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_multiple_with_items() {
        let tree = parse(SourceFile {
            path: PathBuf::from("with_stmt.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(a: A, b: B) -> str:\n    with a as x, b as y:\n        pass\n    return y\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![
                        FunctionParam {
                            name: String::from("a"),
                            annotation: Some(String::from("A")),
                            annotation_expr: None,
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        },
                        FunctionParam {
                            name: String::from("b"),
                            annotation: Some(String::from("B")),
                            annotation_expr: None,
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        },
                    ],
                    returns: Some(String::from("str")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::With(WithStatement {
                    target_name: Some(String::from("x")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    context_type: Some(String::new()),
                    context_is_awaited: false,
                    context_callee: None,
                    context_name: Some(String::from("a")),
                    context_member_owner_name: None,
                    context_member_name: None,
                    context_member_through_instance: false,
                    context_method_owner_name: None,
                    context_method_name: None,
                    context_method_through_instance: false,
                    line: 2,
                }),
                SyntaxStatement::With(WithStatement {
                    target_name: Some(String::from("y")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    context_type: Some(String::new()),
                    context_is_awaited: false,
                    context_callee: None,
                    context_name: Some(String::from("b")),
                    context_member_owner_name: None,
                    context_member_name: None,
                    context_member_through_instance: false,
                    context_method_owner_name: None,
                    context_method_name: None,
                    context_method_through_instance: false,
                    line: 2,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("y")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_except_handler_binding() {
        let tree = parse(SourceFile {
            path: PathBuf::from("try_stmt.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build() -> ValueError:\n    try:\n        risky()\n    except ValueError as e:\n        return e\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("ValueError")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("risky"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 3,
                }),
                SyntaxStatement::ExceptHandler(ExceptionHandlerStatement {
                    exception_type: String::from("ValueError"),
                    binding_name: Some(String::from("e")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                    end_line: 5,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("e")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 5,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_function_signature_shapes() {
        let tree = parse(SourceFile {
            path: PathBuf::from("signatures.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import overload\n\n@overload\ndef parse(value: str) -> int: ...\n\ndef build(value: int) -> str:\n    return \"x\"\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        println!("{:?}", tree.statements);
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("overload"),
                        source_path: String::from("typing.overload"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("str")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("int")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 3,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("int")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("str")),
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 6,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::from("str")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 7,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_override_decorated_functions_and_members() {
        let tree = parse(SourceFile {
            path: PathBuf::from("override.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import override\n\n@override\ndef top_level() -> None:\n    pass\n\nclass Child(Base):\n    @override\n    def run(self) -> None:\n        pass\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("override"),
                        source_path: String::from("typing.override"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("top_level"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("None")),
                    returns_expr: None,
                    is_async: false,
                    is_override: true,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 3,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Child"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(Base)"),
                    bases: vec![String::from("Base")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("run"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        annotation_expr: None,
                        value_type: None,
                        params: vec![FunctionParam {
                            name: String::from("self"),
                            annotation: None,
                            annotation_expr: None,
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        }],
                        returns: Some(String::from("None")),
                        returns_expr: None,
                        is_async: false,
                        is_override: true,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 8,
                    }],
                    line: 7,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_abstract_class_methods() {
        let tree = parse(SourceFile {
            path: PathBuf::from("abstracts.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from abc import abstractmethod\n\nclass Base:\n    @abstractmethod\n    def run(self) -> None:\n        pass\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("abstractmethod"),
                        source_path: String::from("abc.abstractmethod"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Base"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: true,
                    members: vec![ClassMember {
                        name: String::from("run"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        annotation_expr: None,
                        value_type: None,
                        params: vec![FunctionParam {
                            name: String::from("self"),
                            annotation: None,
                            annotation_expr: None,
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        }],
                        returns: Some(String::from("None")),
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: true,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 4,
                    }],
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_method_kinds_from_decorators() {
        let tree = parse(SourceFile {
            path: PathBuf::from("member-kinds.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "class Box:\n    @classmethod\n    def make(cls) -> None:\n        pass\n\n    @staticmethod\n    def build() -> None:\n        pass\n\n    @property\n    def name(self) -> str:\n        return \"x\"\n\n    @name.setter\n    def name(self, value: str) -> None:\n        pass\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![
                        ClassMember {
                            name: String::from("make"),
                            kind: ClassMemberKind::Method,
                            method_kind: Some(MethodKind::Class),
                        annotation: None,
                        annotation_expr: None,
                        value_type: None,
                            params: vec![FunctionParam {
                                name: String::from("cls"),
                            annotation: None,
                            annotation_expr: None,
                            has_default: false,
                                positional_only: false,
                                keyword_only: false,
                                variadic: false,
                                keyword_variadic: false
                            }],
                            returns: Some(String::from("None")),
                            returns_expr: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 2,
                        },
                        ClassMember {
                            name: String::from("build"),
                            kind: ClassMemberKind::Method,
                            method_kind: Some(MethodKind::Static),
                        annotation: None,
                        annotation_expr: None,
                        value_type: None,
                            params: Vec::new(),
                            returns: Some(String::from("None")),
                            returns_expr: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 6,
                        },
                        ClassMember {
                            name: String::from("name"),
                            kind: ClassMemberKind::Method,
                            method_kind: Some(MethodKind::Property),
                        annotation: None,
                        annotation_expr: None,
                        value_type: None,
                            params: vec![FunctionParam {
                                name: String::from("self"),
                                annotation: None,
                                annotation_expr: None,
                                has_default: false,
                                positional_only: false,
                                keyword_only: false,
                                variadic: false,
                                keyword_variadic: false
                            }],
                        returns: Some(String::from("str")),
                        returns_expr: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 10,
                        },
                        ClassMember {
                            name: String::from("name"),
                            kind: ClassMemberKind::Method,
                            method_kind: Some(MethodKind::PropertySetter),
                                annotation: None,
                                annotation_expr: None,
                            value_type: None,
                            params: vec![
                                FunctionParam {
                                    name: String::from("self"),
                                    annotation: None,
                                    annotation_expr: None,
                                    has_default: false,
                                    positional_only: false,
                                    keyword_only: false,
                                    variadic: false,
                                    keyword_variadic: false
                                },
                                FunctionParam {
                                    name: String::from("value"),
                                    annotation: Some(String::from("str")),
                                    annotation_expr: None,
                                    has_default: false,
                                    positional_only: false,
                                    keyword_only: false,
                                    variadic: false,
                                    keyword_variadic: false
                                },
                            ],
                    returns: Some(String::from("None")),
                    returns_expr: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 14,
                        },
                    ],
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("name"),
                    owner_type_name: Some(String::from("Box")),
                    value_type: Some(String::from("str")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 12,
                })
            ]
        );
    }

    #[test]
    fn parse_retains_match_statement_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("match_case.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "match value:\n    case Add():\n        pass\n    case Mul() | Div():\n        pass\n    case _:\n        pass\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Match(MatchStatement {
                owner_name: None,
                owner_type_name: None,
                subject_type: Some(String::new()),
                subject_is_awaited: false,
                subject_callee: None,
                subject_name: Some(String::from("value")),
                subject_member_owner_name: None,
                subject_member_name: None,
                subject_member_through_instance: false,
                subject_method_owner_name: None,
                subject_method_name: None,
                subject_method_through_instance: false,
                cases: vec![
                    MatchCaseStatement {
                        patterns: vec![MatchPattern::Class(String::from("Add"))],
                        has_guard: false,
                        line: 2,
                    },
                    MatchCaseStatement {
                        patterns: vec![
                            MatchPattern::Class(String::from("Mul")),
                            MatchPattern::Class(String::from("Div")),
                        ],
                        has_guard: false,
                        line: 4,
                    },
                    MatchCaseStatement {
                        patterns: vec![MatchPattern::Wildcard],
                        has_guard: false,
                        line: 6,
                    },
                ],
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_retains_if_and_assert_guard_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("guards.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(value: str | None) -> str:\n    if value is not None:\n        return value\n    assert value is None\n    return \"fallback\"\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements[1],
            SyntaxStatement::If(IfStatement {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(GuardCondition::IsNone { name: String::from("value"), negated: true }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: None,
                false_end_line: None,
            })
        );
        assert_eq!(
            tree.statements[3],
            SyntaxStatement::Assert(AssertStatement {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(GuardCondition::IsNone { name: String::from("value"), negated: false }),
                line: 4,
            })
        );
    }

    #[test]
    fn parse_retains_invalidation_statement_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("invalidate.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(value: int | None) -> int:\n    if value is not None:\n        value += 1\n        del value\n        global value\n        nonlocal value\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements
                .iter()
                .filter(|statement| matches!(statement, SyntaxStatement::Invalidate(_)))
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                SyntaxStatement::Invalidate(InvalidationStatement {
                    kind: InvalidationKind::RebindLike,
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    names: vec![String::from("value")],
                    line: 3,
                }),
                SyntaxStatement::Invalidate(InvalidationStatement {
                    kind: InvalidationKind::Delete,
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    names: vec![String::from("value")],
                    line: 4,
                }),
                SyntaxStatement::Invalidate(InvalidationStatement {
                    kind: InvalidationKind::ScopeChange,
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    names: vec![String::from("value")],
                    line: 5,
                }),
                SyntaxStatement::Invalidate(InvalidationStatement {
                    kind: InvalidationKind::ScopeChange,
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    names: vec![String::from("value")],
                    line: 6,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_type_ignore_directives() {
        let tree = parse(SourceFile {
            path: PathBuf::from("ignore.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("x = 1  # type: ignore[TPY4001]\ny = 2  # type: ignore\n"),
        });

        assert_eq!(
            tree.type_ignore_directives,
            vec![
                TypeIgnoreDirective { line: 1, codes: Some(vec![String::from("TPY4001")]) },
                TypeIgnoreDirective { line: 2, codes: None },
            ]
        );
    }

    #[test]
    fn parse_rejects_conditional_return_syntax_by_default() {
        let tree = parse(SourceFile {
            path: PathBuf::from("conditional-return.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n",
            ),
        });

        assert!(tree.diagnostics.has_errors());
    }

    #[test]
    fn parse_accepts_conditional_return_syntax_when_enabled() {
        let tree = parse_with_options(
            SourceFile {
                path: PathBuf::from("conditional-return.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n",
                ),
            },
            ParseOptions { enable_conditional_returns: true, ..ParseOptions::default() },
        );

        assert!(tree.diagnostics.is_empty());
        let sites = crate::collect_conditional_return_sites(&tree.source.text);
        assert_eq!(
            sites,
            vec![crate::ConditionalReturnSite {
                function_name: String::from("decode"),
                target_name: String::from("x"),
                target_type: String::from("str | bytes | None"),
                case_input_types: vec![
                    String::from("str"),
                    String::from("bytes"),
                    String::from("None"),
                ],
                line: 1,
            }]
        );
    }

    #[test]
    fn parse_accepts_multiline_conditional_return_syntax_when_enabled() {
        let tree = parse_with_options(
            SourceFile {
                path: PathBuf::from("conditional-return-multiline.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "def decode(\n    x: str | bytes | None,\n) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n\nvalue: int = 1\n",
                ),
            },
            ParseOptions { enable_conditional_returns: true, ..ParseOptions::default() },
        );

        assert!(tree.diagnostics.is_empty(), "{}", tree.diagnostics.as_text());
        let sites = crate::collect_conditional_return_sites(&tree.source.text);
        assert_eq!(
            sites,
            vec![crate::ConditionalReturnSite {
                function_name: String::from("decode"),
                target_name: String::from("x"),
                target_type: String::from("str | bytes | None"),
                case_input_types: vec![
                    String::from("str"),
                    String::from("bytes"),
                    String::from("None"),
                ],
                line: 1,
            }]
        );
    }

    #[test]
    fn prepare_source_for_external_formatter_normalizes_and_restores_typepython_lines() {
        let source = SourceFile {
            path: PathBuf::from("formatting.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: String::from(
                "typealias  Pair[T]=tuple[T,T]\ninterface Box[T]:\n    pass\ndata class User:\n    name:str\noverload def parse( value : str = \"x\") -> int:\n    ...\nunsafe:\n    run()\n",
            ),
        };

        let prepared = crate::prepare_source_for_external_formatter(&source)
            .expect("valid TypePython source should prepare for external formatting");
        assert!(prepared.formatter_input().contains("# __typepython_format__:typealias"));
        assert!(prepared.formatter_input().contains("Pair[T]=tuple[T,T]"));
        assert!(prepared.formatter_input().contains("class Box[T]:"));
        assert!(prepared.formatter_input().contains("class User:"));
        assert!(prepared.formatter_input().contains("def parse( value : str = \"x\") -> int:"));
        assert!(prepared.formatter_input().contains("if True:"));

        let restored = prepared.restore(
            "# __typepython_format__:typealias\nPair[T] = tuple[T, T]\n# __typepython_format__:interface\nclass Box[T]:\n    pass\n# __typepython_format__:data_class\nclass User:\n    name: str\n# __typepython_format__:overload_def\ndef parse(value: str = \"x\") -> int:\n    ...\n# __typepython_format__:unsafe\nif True:\n    run()\n",
        );
        assert!(restored.contains("typealias Pair[T] = tuple[T, T]"));
        assert!(restored.contains("interface Box[T]:"));
        assert!(restored.contains("data class User:"));
        assert!(restored.contains("overload def parse(value: str = \"x\") -> int:"));
        assert!(restored.contains("unsafe:"));
    }

    #[test]
    fn prepare_source_for_external_formatter_reports_parse_errors() {
        let source = SourceFile {
            path: PathBuf::from("broken.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: String::from("interface Broken\n    pass\n"),
        };

        let diagnostics = crate::prepare_source_for_external_formatter(&source)
            .expect_err("invalid TypePython syntax should not prepare for formatting");
        assert!(diagnostics.has_errors());
    }

    // ─── Property-based / fuzz tests ────────────────────────────────────────

    mod fuzz {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn parse_does_not_panic_on_arbitrary_typepython_input(input in "\\PC{0,500}") {
                let _ = parse(SourceFile {
                    path: PathBuf::from("fuzz.tpy"),
                    kind: SourceKind::TypePython,
                    logical_module: String::new(),
                    text: input,
                });
            }

            #[test]
            fn parse_does_not_panic_on_arbitrary_python_input(input in "\\PC{0,500}") {
                let _ = parse(SourceFile {
                    path: PathBuf::from("fuzz.py"),
                    kind: SourceKind::Python,
                    logical_module: String::new(),
                    text: input,
                });
            }

            #[test]
            fn parse_does_not_panic_on_arbitrary_stub_input(input in "\\PC{0,500}") {
                let _ = parse(SourceFile {
                    path: PathBuf::from("fuzz.pyi"),
                    kind: SourceKind::Stub,
                    logical_module: String::new(),
                    text: input,
                });
            }

            #[test]
            fn parse_does_not_panic_on_python_like_constructs(
                indent in "[\\s]{0,4}",
                keyword in "(def|class|if|for|while|with|try|match|import|from|return|yield|raise|async|await)",
                rest in "[a-zA-Z0-9_\\s:,.()\\[\\]\\->=!+*/@]{0,100}"
            ) {
                let input = format!("{indent}{keyword} {rest}\n");
                let _ = parse(SourceFile {
                    path: PathBuf::from("fuzz-keyword.tpy"),
                    kind: SourceKind::TypePython,
                    logical_module: String::new(),
                    text: input,
                });
            }

            #[test]
            fn parse_does_not_panic_on_typepython_keyword_constructs(
                keyword in "(typealias|interface|sealed class|data class|overload def|unsafe)",
                name in "[A-Z][a-zA-Z0-9_]{0,20}",
                rest in "[a-zA-Z0-9_\\s:,.()\\[\\]\\->=]{0,80}"
            ) {
                let input = format!("{keyword} {name}{rest}\n");
                let _ = parse(SourceFile {
                    path: PathBuf::from("fuzz-tpy-keyword.tpy"),
                    kind: SourceKind::TypePython,
                    logical_module: String::new(),
                    text: input,
                });
            }

            #[test]
            fn parse_does_not_panic_on_deeply_nested_input(depth in 1usize..20) {
                let mut source = String::new();
                for i in 0..depth {
                    let indent = "    ".repeat(i);
                    source.push_str(&format!("{indent}if True:\n"));
                }
                let final_indent = "    ".repeat(depth);
                source.push_str(&format!("{final_indent}pass\n"));
                let _ = parse(SourceFile {
                    path: PathBuf::from("fuzz-nested.tpy"),
                    kind: SourceKind::TypePython,
                    logical_module: String::new(),
                    text: source,
                });
            }

            #[test]
            fn parse_does_not_panic_on_unicode_identifiers(
                name in "[\\p{L}][\\p{L}\\p{N}_]{0,30}"
            ) {
                let source = format!("def {name}() -> None:\n    pass\n");
                let _ = parse(SourceFile {
                    path: PathBuf::from("fuzz-unicode.tpy"),
                    kind: SourceKind::TypePython,
                    logical_module: String::new(),
                    text: source,
                });
            }
        }
    }
}

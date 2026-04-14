use super::*;

pub(in super::super) fn collect_return_statements(
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

pub(in super::super) fn collect_yield_statements(
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

pub(in super::super) fn collect_if_statements(
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

pub(in super::super) fn collect_assert_statements(
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

pub(in super::super) fn collect_invalidation_statements(
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

pub(in super::super) fn collect_match_statements(
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

pub(in super::super) fn collect_for_statements(
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

pub(in super::super) fn collect_with_statements(
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

pub(in super::super) fn collect_except_handler_statements(
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

pub(in super::super) fn collect_function_body_assignments(
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

pub(in super::super) fn collect_function_body_bare_assignments(
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

pub(in super::super) fn collect_function_body_namedexpr_assignments(
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

pub(in super::super) struct NamedExprAssignmentCollector<'a> {
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

pub(in super::super) fn extract_function_body_assignment_statement(
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
            DirectExprMetadata {
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
        );
    Some(SyntaxStatement::Value(ValueStatement {
        names,
        destructuring_target_names: None,
        annotation: slice_range(source, assign.annotation.range()).map(str::to_owned),
        annotation_expr: slice_range(source, assign.annotation.range()).and_then(TypeExpr::parse),
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

pub(in super::super) fn extract_function_body_bare_assignment_statement(
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

pub(in super::super) fn extract_augmented_assignment_value_statement(
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

pub(in super::super) fn extract_yield_statement(
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
                .unwrap_or(DirectExprMetadata {
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
                }),
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
        value_type: value.rendered_value_type(),
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

pub(in super::super) fn extract_match_statement(
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
        subject_type: subject.rendered_value_type(),
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

pub(in super::super) fn extract_match_patterns(
    source: &str,
    pattern: &ruff_python_ast::Pattern,
) -> Vec<MatchPattern> {
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

pub(in super::super) fn extract_for_statement(
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
        iter_type: iter.rendered_value_type(),
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

pub(in super::super) fn extract_with_statements(
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
                context_type: context.rendered_value_type(),
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

pub(in super::super) fn extract_except_handler_statement(
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

pub(in super::super) fn collect_calls_from_suite(
    source: &str,
    suite: &[Stmt],
    statements: &mut Vec<SyntaxStatement>,
) {
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
    }
}

pub(in super::super) fn extract_return_statement(
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
        .unwrap_or(DirectExprMetadata {
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

    Some(SyntaxStatement::Return(ReturnStatement {
        owner_name: owner_name.to_owned(),
        owner_type_name: owner_type_name.map(str::to_owned),
        value_type: value.rendered_value_type(),
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

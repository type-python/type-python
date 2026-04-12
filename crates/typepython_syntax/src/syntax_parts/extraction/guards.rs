use super::*;

pub(in super::super) fn extract_guard_condition(
    source: &str,
    expr: &Expr,
) -> Option<GuardCondition> {
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

pub(in super::super) fn suite_start_line(source: &str, suite: &[Stmt]) -> usize {
    suite_start_line_optional(source, suite).unwrap_or(0)
}

pub(in super::super) fn suite_end_line(source: &str, suite: &[Stmt]) -> usize {
    suite_end_line_optional(source, suite).unwrap_or(0)
}

pub(in super::super) fn suite_start_line_optional(source: &str, suite: &[Stmt]) -> Option<usize> {
    suite.first().map(|stmt| offset_to_line_column(source, stmt.range().start().to_usize()).0)
}

pub(in super::super) fn suite_end_line_optional(source: &str, suite: &[Stmt]) -> Option<usize> {
    suite.last().map(|stmt| offset_to_line_column(source, stmt.range().end().to_usize()).0)
}

pub(in super::super) fn if_false_start_line(
    source: &str,
    stmt: &ruff_python_ast::StmtIf,
) -> Option<usize> {
    stmt.elif_else_clauses
        .first()
        .and_then(|clause| suite_start_line_optional(source, &clause.body))
}

pub(in super::super) fn if_false_end_line(
    source: &str,
    stmt: &ruff_python_ast::StmtIf,
) -> Option<usize> {
    stmt.elif_else_clauses.last().and_then(|clause| suite_end_line_optional(source, &clause.body))
}

pub(in super::super) fn for_each_if_false_suite(
    stmt: &ruff_python_ast::StmtIf,
    mut callback: impl FnMut(&[Stmt]),
) {
    for clause in &stmt.elif_else_clauses {
        callback(&clause.body);
    }
}

pub(in super::super) fn for_each_nested_suite(stmt: &Stmt, mut callback: impl FnMut(&[Stmt])) {
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

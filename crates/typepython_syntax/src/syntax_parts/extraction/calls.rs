use super::*;

pub(in super::super) fn extract_call_statement(
    source: &str,
    expr: &Expr,
    line: usize,
) -> Option<SyntaxStatement> {
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

pub(in super::super) fn extract_method_call_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
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
            current_owner_name: current_owner_name.map(str::to_owned),
            current_owner_type_name: current_owner_type_name.map(str::to_owned),
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
                current_owner_name: current_owner_name.map(str::to_owned),
                current_owner_type_name: current_owner_type_name.map(str::to_owned),
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

pub(in super::super) fn collect_nested_method_call_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_nested_method_call_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_nested_method_call_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            _ => {
                for_each_nested_suite(stmt, |nested_suite| {
                    collect_nested_method_call_statements(
                        source,
                        nested_suite,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                });
                if owner_name.is_some() || owner_type_name.is_some() {
                    let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                    if let Some(method_call) = extract_method_call_statement(
                        source,
                        stmt,
                        line,
                        owner_name,
                        owner_type_name,
                    ) {
                        statements.push(method_call);
                    }
                }
            }
        }
    }
}

pub(in super::super) fn infer_literal_arg_type(expr: &Expr) -> String {
    infer_direct_literal_type(expr).unwrap_or_default()
}

pub(in super::super) fn infer_direct_literal_type(expr: &Expr) -> Option<String> {
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

pub(in super::super) fn extract_list_comprehension_clauses(
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

pub(in super::super) fn infer_direct_binop_type(
    bin_op: &ruff_python_ast::ExprBinOp,
) -> Option<String> {
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

pub(in super::super) fn is_direct_numeric_type(text: &str) -> bool {
    matches!(text, "int" | "float" | "complex")
}

pub(in super::super) fn join_numeric_type(left: &str, right: &str) -> String {
    if left == "complex" || right == "complex" {
        String::from("complex")
    } else if left == "float" || right == "float" {
        String::from("float")
    } else {
        String::from("int")
    }
}

pub(in super::super) fn split_direct_generic_type(text: &str) -> Option<(String, Vec<String>)> {
    let (head, inner) = text.split_once('[')?;
    let inner = inner.strip_suffix(']')?;
    Some((head.to_owned(), inner.split(',').map(|part| part.trim().to_owned()).collect()))
}

pub(in super::super) fn join_union_literal_type_candidates(types: Vec<String>) -> String {
    let joined = join_literal_type_candidates(types);
    if joined.contains(" | ") { format!("Union[{}]", joined.replace(" | ", ", ")) } else { joined }
}

pub(in super::super) fn join_literal_type_candidates(types: Vec<String>) -> String {
    let mut unique = Vec::new();
    for value in types {
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    if unique.is_empty() { String::from("Any") } else { unique.join(" | ") }
}

pub(in super::super) fn extract_supplemental_call_statement(
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

pub(in super::super) fn extract_member_access_statement(
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

pub(in super::super) fn extract_member_access_from_expr(
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

pub(in super::super) fn collect_nested_call_statements(
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

pub(in super::super) fn collect_nested_member_access_statements(
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

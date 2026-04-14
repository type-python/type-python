use super::*;

pub(in super::super) fn extract_direct_expr_metadata(
    source: &str,
    expr: &Expr,
) -> DirectExprMetadata {
    if let Expr::Await(await_expr) = expr {
        let mut metadata = extract_direct_expr_metadata(source, &await_expr.value);
        metadata.is_awaited = true;
        return metadata;
    }

    if let Expr::Named(named_expr) = expr {
        return extract_direct_expr_metadata(source, &named_expr.value);
    }

    if let Expr::Dict(dict) = expr {
        return DirectExprMetadata {
            value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)),
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
            value_dict_entries: Some(extract_typed_dict_literal_entries(source, dict)),
        };
    }

    if let Expr::List(list) = expr {
        return DirectExprMetadata {
            value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)),
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
            value_list_elements: Some(
                list.elts.iter().map(|item| extract_direct_expr_metadata(source, item)).collect(),
            ),
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::Set(set) = expr {
        return DirectExprMetadata {
            value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)),
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
            value_set_elements: Some(
                set.elts.iter().map(|item| extract_direct_expr_metadata(source, item)).collect(),
            ),
            value_dict_entries: None,
        };
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
        return DirectExprMetadata {
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
                params,
                body: Box::new(extract_direct_expr_metadata(source, &lambda.body)),
            })),
            value_list_comprehension: None,
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::ListComp(comp) = expr {
        return DirectExprMetadata {
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
            value_list_comprehension: Some(Box::new(ComprehensionMetadata {
                kind: ComprehensionKind::List,
                clauses: extract_list_comprehension_clauses(source, &comp.generators),
                key: None,
                element: Box::new(extract_direct_expr_metadata(source, &comp.elt)),
            })),
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::SetComp(comp) = expr {
        return DirectExprMetadata {
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
            value_list_comprehension: Some(Box::new(ComprehensionMetadata {
                kind: ComprehensionKind::Set,
                clauses: extract_list_comprehension_clauses(source, &comp.generators),
                key: None,
                element: Box::new(extract_direct_expr_metadata(source, &comp.elt)),
            })),
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::DictComp(comp) = expr {
        return DirectExprMetadata {
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
            value_list_comprehension: Some(Box::new(ComprehensionMetadata {
                kind: ComprehensionKind::Dict,
                clauses: extract_list_comprehension_clauses(source, &comp.generators),
                key: Some(Box::new(extract_direct_expr_metadata(source, &comp.key))),
                element: Box::new(extract_direct_expr_metadata(source, &comp.value)),
            })),
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::Generator(comp) = expr {
        return DirectExprMetadata {
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
            value_generator_comprehension: Some(Box::new(ComprehensionMetadata {
                kind: ComprehensionKind::Generator,
                clauses: extract_list_comprehension_clauses(source, &comp.generators),
                key: None,
                element: Box::new(extract_direct_expr_metadata(source, &comp.elt)),
            })),
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Some((owner_name, method_name, through_instance)) = extract_direct_method_call(expr) {
        return DirectExprMetadata {
            value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)),
            is_awaited: false,
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
            value_dict_entries: None,
        };
    }

    if let Expr::BoolOp(bool_op) = expr {
        let mut values = bool_op.values.iter();
        let left_expr = values.next();
        let left_guard = left_expr.and_then(|expr| extract_guard_condition(source, expr));
        let left = left_expr.map(|expr| extract_direct_expr_metadata(source, expr));
        let right = values.next().map(|expr| extract_direct_expr_metadata(source, expr));
        return DirectExprMetadata {
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
            value_dict_entries: None,
        };
    }

    if let Expr::BinOp(bin_op) = expr {
        return DirectExprMetadata {
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
            value_binop_left: Some(Box::new(extract_direct_expr_metadata(source, &bin_op.left))),
            value_binop_right: Some(Box::new(extract_direct_expr_metadata(source, &bin_op.right))),
            value_binop_operator: Some(direct_operator_text(bin_op.op)),
            value_lambda: None,
            value_list_comprehension: None,
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::If(if_expr) = expr {
        return DirectExprMetadata {
            value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)),
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
            value_dict_entries: None,
        };
    }

    if let Expr::Subscript(subscript) = expr {
        return DirectExprMetadata {
            value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)),
            is_awaited: false,
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
            value_dict_entries: None,
        };
    }

    let member = extract_direct_member_access(expr);
    DirectExprMetadata {
        value_type_expr: TypeExpr::parse(&infer_literal_arg_type(expr)),
        is_awaited: false,
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
        value_dict_entries: None,
    }
}

pub(in super::super) fn extract_direct_method_call(expr: &Expr) -> Option<(String, String, bool)> {
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

pub(in super::super) fn extract_direct_callee(expr: &Expr) -> Option<String> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Name(name) = call.func.as_ref() else {
        return None;
    };
    Some(name.id.as_str().to_owned())
}

pub(in super::super) fn extract_direct_name(expr: &Expr) -> Option<String> {
    let Expr::Name(name) = expr else {
        return None;
    };
    Some(name.id.as_str().to_owned())
}

pub(in super::super) fn extract_direct_member_access(
    expr: &Expr,
) -> Option<(String, String, bool)> {
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

pub(in super::super) fn extract_ast_type_params(
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
                    constraint_exprs: constraints
                        .iter()
                        .filter_map(|constraint| TypeExpr::parse(constraint))
                        .collect(),
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

pub(in super::super) fn extract_ast_type_param_bound_and_constraints(
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

pub(in super::super) fn extract_function_params(
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

pub(in super::super) fn extract_class_bases(
    source: &str,
    arguments: &ruff_python_ast::Arguments,
) -> Vec<String> {
    arguments
        .args
        .iter()
        .filter_map(|argument| slice_range(source, argument.range()).map(str::to_owned))
        .collect()
}

pub(in super::super) fn extract_assignment_names(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::Name(name) => vec![name.id.as_str().to_owned()],
        Expr::Tuple(tuple) => tuple.elts.iter().flat_map(extract_assignment_names).collect(),
        Expr::List(list) => list.elts.iter().flat_map(extract_assignment_names).collect(),
        Expr::Starred(starred) => extract_assignment_names(&starred.value),
        _ => Vec::new(),
    }
}

pub(in super::super) fn extract_simple_destructuring_target_names(
    expr: &Expr,
) -> Option<Vec<String>> {
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

pub(in super::super) fn is_overload_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "overload",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "overload"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if name.id.as_str() == "typing")
        }
        _ => false,
    }
}

pub(in super::super) fn is_override_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "override",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "override"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        _ => false,
    }
}

pub(in super::super) fn is_abstractmethod_decorator(
    decorator: &ruff_python_ast::Decorator,
) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "abstractmethod",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "abstractmethod"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if name.id.as_str() == "abc")
        }
        _ => false,
    }
}

pub(in super::super) fn is_final_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "final",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "final"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        _ => false,
    }
}

pub(in super::super) fn deprecated_decorator_message(
    decorators: &[ruff_python_ast::Decorator],
) -> Option<String> {
    decorators.iter().find_map(deprecated_decorator_arg)
}

pub(in super::super) fn deprecated_decorator_arg(
    decorator: &ruff_python_ast::Decorator,
) -> Option<String> {
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

pub(in super::super) fn method_kind_from_decorators(
    decorators: &[ruff_python_ast::Decorator],
) -> MethodKind {
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

pub(in super::super) fn is_abstract_class(statement: &NamedBlockStatement) -> bool {
    statement.bases.iter().any(|base| matches!(base.as_str(), "ABC" | "abc.ABC"))
        || statement.members.iter().any(|member| member.is_abstract_method)
}

pub(in super::super) fn is_final_annotation(expr: &Expr) -> bool {
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

pub(in super::super) fn is_classvar_annotation(expr: &Expr) -> bool {
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

pub(in super::super) fn normalize_import_module(
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

pub(in super::super) fn slice_range(
    source: &str,
    range: ruff_text_size::TextRange,
) -> Option<&str> {
    source.get(range.start().to_usize()..range.end().to_usize())
}

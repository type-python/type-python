use super::*;

/// Transforms a parsed syntax tree into its bound module surface.
#[must_use]
pub fn bind(tree: &SyntaxTree) -> BindingTable {
    BindingTable {
        module_path: tree.source.path.clone(),
        module_key: tree.source.logical_module.clone(),
        module_kind: tree.source.kind,
        surface_facts: bind_module_surface_facts(tree),
        declarations: tree.statements.iter().flat_map(bind_statement).collect(),
        calls: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Call(statement) => Some(CallSite {
                    callee: statement.callee.clone(),
                    arg_count: statement.arg_count,
                    arg_values: statement.arg_values.clone(),
                    starred_arg_values: statement.starred_arg_values.clone(),
                    keyword_names: statement.keyword_names.clone(),
                    keyword_arg_values: statement.keyword_arg_values.clone(),
                    keyword_expansion_values: statement.keyword_expansion_values.clone(),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        method_calls: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::MethodCall(statement) => Some(MethodCallSite {
                    current_owner_name: statement.current_owner_name.clone(),
                    current_owner_type_name: statement.current_owner_type_name.clone(),
                    owner_name: statement.owner_name.clone(),
                    method: statement.method.clone(),
                    through_instance: statement.through_instance,
                    arg_count: statement.arg_count,
                    arg_values: statement.arg_values.clone(),
                    starred_arg_values: statement.starred_arg_values.clone(),
                    keyword_names: statement.keyword_names.clone(),
                    keyword_arg_values: statement.keyword_arg_values.clone(),
                    keyword_expansion_values: statement.keyword_expansion_values.clone(),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        member_accesses: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::MemberAccess(statement) => Some(MemberAccessSite {
                    current_owner_name: statement.current_owner_name.clone(),
                    current_owner_type_name: statement.current_owner_type_name.clone(),
                    owner_name: statement.owner_name.clone(),
                    member: statement.member.clone(),
                    through_instance: statement.through_instance,
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        returns: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Return(statement) => Some(ReturnSite {
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    value: direct_expr_metadata_from_return_statement(statement),
                    value_type: statement.value_type.clone(),
                    is_awaited: statement.is_awaited,
                    value_callee: statement.value_callee.clone(),
                    value_name: statement.value_name.clone(),
                    value_member_owner_name: statement.value_member_owner_name.clone(),
                    value_member_name: statement.value_member_name.clone(),
                    value_member_through_instance: statement.value_member_through_instance,
                    value_method_owner_name: statement.value_method_owner_name.clone(),
                    value_method_name: statement.value_method_name.clone(),
                    value_method_through_instance: statement.value_method_through_instance,
                    value_subscript_target: statement.value_subscript_target.clone(),
                    value_subscript_string_key: statement.value_subscript_string_key.clone(),
                    value_subscript_index: statement.value_subscript_index.clone(),
                    value_if_true: statement.value_if_true.clone(),
                    value_if_false: statement.value_if_false.clone(),
                    value_if_guard: statement.value_if_guard.as_ref().map(map_guard_condition),
                    value_bool_left: statement.value_bool_left.clone(),
                    value_bool_right: statement.value_bool_right.clone(),
                    value_binop_left: statement.value_binop_left.clone(),
                    value_binop_right: statement.value_binop_right.clone(),
                    value_binop_operator: statement.value_binop_operator.clone(),
                    value_lambda: statement.value_lambda.clone(),
                    value_list_elements: statement.value_list_elements.clone(),
                    value_set_elements: statement.value_set_elements.clone(),
                    value_dict_entries: statement.value_dict_entries.clone(),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        yields: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Yield(statement) => Some(YieldSite {
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    value: direct_expr_metadata_from_yield_statement(statement),
                    value_type: statement.value_type.clone(),
                    value_callee: statement.value_callee.clone(),
                    value_name: statement.value_name.clone(),
                    value_member_owner_name: statement.value_member_owner_name.clone(),
                    value_member_name: statement.value_member_name.clone(),
                    value_member_through_instance: statement.value_member_through_instance,
                    value_method_owner_name: statement.value_method_owner_name.clone(),
                    value_method_name: statement.value_method_name.clone(),
                    value_method_through_instance: statement.value_method_through_instance,
                    value_subscript_target: statement.value_subscript_target.clone(),
                    value_subscript_string_key: statement.value_subscript_string_key.clone(),
                    value_subscript_index: statement.value_subscript_index.clone(),
                    value_if_true: statement.value_if_true.clone(),
                    value_if_false: statement.value_if_false.clone(),
                    value_if_guard: statement.value_if_guard.as_ref().map(map_guard_condition),
                    value_bool_left: statement.value_bool_left.clone(),
                    value_bool_right: statement.value_bool_right.clone(),
                    value_binop_left: statement.value_binop_left.clone(),
                    value_binop_right: statement.value_binop_right.clone(),
                    value_binop_operator: statement.value_binop_operator.clone(),
                    value_lambda: statement.value_lambda.clone(),
                    value_list_elements: statement.value_list_elements.clone(),
                    value_set_elements: statement.value_set_elements.clone(),
                    value_dict_entries: statement.value_dict_entries.clone(),
                    is_yield_from: statement.is_yield_from,
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        if_guards: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::If(statement) => Some(IfGuardSite {
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    guard: statement.guard.as_ref().map(map_guard_condition),
                    line: statement.line,
                    true_start_line: statement.true_start_line,
                    true_end_line: statement.true_end_line,
                    false_start_line: statement.false_start_line,
                    false_end_line: statement.false_end_line,
                }),
                _ => None,
            })
            .collect(),
        asserts: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Assert(statement) => Some(AssertGuardSite {
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    guard: statement.guard.as_ref().map(map_guard_condition),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        invalidations: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Invalidate(statement) => Some(InvalidationSite {
                    kind: match statement.kind {
                        typepython_syntax::InvalidationKind::RebindLike => {
                            InvalidationKind::RebindLike
                        }
                        typepython_syntax::InvalidationKind::Delete => InvalidationKind::Delete,
                        typepython_syntax::InvalidationKind::ScopeChange => {
                            InvalidationKind::ScopeChange
                        }
                    },
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    names: statement.names.clone(),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        matches: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Match(statement) => Some(MatchSite {
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    subject: direct_expr_metadata_from_match_statement(statement),
                    subject_type: statement.subject_type.clone(),
                    subject_is_awaited: statement.subject_is_awaited,
                    subject_callee: statement.subject_callee.clone(),
                    subject_name: statement.subject_name.clone(),
                    subject_member_owner_name: statement.subject_member_owner_name.clone(),
                    subject_member_name: statement.subject_member_name.clone(),
                    subject_member_through_instance: statement.subject_member_through_instance,
                    subject_method_owner_name: statement.subject_method_owner_name.clone(),
                    subject_method_name: statement.subject_method_name.clone(),
                    subject_method_through_instance: statement.subject_method_through_instance,
                    cases: statement
                        .cases
                        .iter()
                        .map(|case| MatchCaseSite {
                            patterns: case
                                .patterns
                                .iter()
                                .map(|pattern| match pattern {
                                    typepython_syntax::MatchPattern::Wildcard => {
                                        MatchPatternSite::Wildcard
                                    }
                                    typepython_syntax::MatchPattern::Literal(value) => {
                                        MatchPatternSite::Literal(value.clone())
                                    }
                                    typepython_syntax::MatchPattern::Class(value) => {
                                        MatchPatternSite::Class(value.clone())
                                    }
                                    typepython_syntax::MatchPattern::Unsupported => {
                                        MatchPatternSite::Unsupported
                                    }
                                })
                                .collect(),
                            has_guard: case.has_guard,
                            line: case.line,
                        })
                        .collect(),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        for_loops: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::For(statement) => Some(ForSite {
                    target_name: statement.target_name.clone(),
                    target_names: statement.target_names.clone(),
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    iter: direct_expr_metadata_from_for_statement(statement),
                    iter_type: statement.iter_type.clone(),
                    iter_is_awaited: statement.iter_is_awaited,
                    iter_callee: statement.iter_callee.clone(),
                    iter_name: statement.iter_name.clone(),
                    iter_member_owner_name: statement.iter_member_owner_name.clone(),
                    iter_member_name: statement.iter_member_name.clone(),
                    iter_member_through_instance: statement.iter_member_through_instance,
                    iter_method_owner_name: statement.iter_method_owner_name.clone(),
                    iter_method_name: statement.iter_method_name.clone(),
                    iter_method_through_instance: statement.iter_method_through_instance,
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        with_statements: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::With(statement) => Some(WithSite {
                    target_name: statement.target_name.clone(),
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    context: direct_expr_metadata_from_with_statement(statement),
                    context_type: statement.context_type.clone(),
                    context_is_awaited: statement.context_is_awaited,
                    context_callee: statement.context_callee.clone(),
                    context_name: statement.context_name.clone(),
                    context_member_owner_name: statement.context_member_owner_name.clone(),
                    context_member_name: statement.context_member_name.clone(),
                    context_member_through_instance: statement.context_member_through_instance,
                    context_method_owner_name: statement.context_method_owner_name.clone(),
                    context_method_name: statement.context_method_name.clone(),
                    context_method_through_instance: statement.context_method_through_instance,
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        except_handlers: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::ExceptHandler(statement) => Some(ExceptHandlerSite {
                    exception_type: statement.exception_type.clone(),
                    binding_name: statement.binding_name.clone(),
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    line: statement.line,
                    end_line: statement.end_line,
                }),
                _ => None,
            })
            .collect(),
        assignments: tree
            .statements
            .iter()
            .flat_map(|statement| match statement {
                SyntaxStatement::Value(statement) => statement
                    .names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| AssignmentSite {
                        name: name.clone(),
                        destructuring_target_names: statement.destructuring_target_names.clone(),
                        destructuring_index: statement
                            .destructuring_target_names
                            .as_ref()
                            .map(|_| index),
                        annotation: statement.annotation.clone(),
                        annotation_expr: statement
                            .annotation_expr
                            .clone()
                            .map(BoundTypeExpr::from_expr)
                            .or_else(|| statement.annotation.clone().map(BoundTypeExpr::new)),
                        value: direct_expr_metadata_from_value_statement(statement),
                        value_type: statement.value_type.clone(),
                        is_awaited: statement.is_awaited,
                        value_callee: statement.value_callee.clone(),
                        value_name: statement.value_name.clone(),
                        value_member_owner_name: statement.value_member_owner_name.clone(),
                        value_member_name: statement.value_member_name.clone(),
                        value_member_through_instance: statement.value_member_through_instance,
                        value_method_owner_name: statement.value_method_owner_name.clone(),
                        value_method_name: statement.value_method_name.clone(),
                        value_method_through_instance: statement.value_method_through_instance,
                        value_subscript_target: statement.value_subscript_target.clone(),
                        value_subscript_string_key: statement.value_subscript_string_key.clone(),
                        value_subscript_index: statement.value_subscript_index.clone(),
                        value_if_true: statement.value_if_true.clone(),
                        value_if_false: statement.value_if_false.clone(),
                        value_if_guard: statement.value_if_guard.as_ref().map(map_guard_condition),
                        value_bool_left: statement.value_bool_left.clone(),
                        value_bool_right: statement.value_bool_right.clone(),
                        value_binop_left: statement.value_binop_left.clone(),
                        value_binop_right: statement.value_binop_right.clone(),
                        value_binop_operator: statement.value_binop_operator.clone(),
                        value_lambda: statement.value_lambda.clone(),
                        value_list_comprehension: statement.value_list_comprehension.clone(),
                        value_generator_comprehension: statement
                            .value_generator_comprehension
                            .clone(),
                        value_list_elements: statement.value_list_elements.clone(),
                        value_set_elements: statement.value_set_elements.clone(),
                        value_dict_entries: statement.value_dict_entries.clone(),
                        owner_name: statement.owner_name.clone(),
                        owner_type_name: statement.owner_type_name.clone(),
                        line: statement.line,
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect(),
    }
}

fn bind_module_surface_facts(tree: &SyntaxTree) -> ModuleSurfaceFacts {
    let collected = typepython_syntax::collect_module_surface_metadata(&tree.source.text);
    let typed_dict_class_metadata = collected
        .typed_dict_classes
        .into_iter()
        .map(|metadata| (metadata.name.clone(), metadata))
        .collect();
    let direct_function_signatures = collected
        .direct_function_signatures
        .into_iter()
        .map(|signature| (signature.name, signature.params))
        .collect();
    let direct_method_signatures = collected
        .direct_method_signatures
        .into_iter()
        .map(|signature| {
            let params = match signature.method_kind {
                MethodKind::Static | MethodKind::Property => signature.params,
                MethodKind::Instance | MethodKind::Class | MethodKind::PropertySetter => {
                    signature.params.into_iter().skip(1).collect()
                }
            };
            ((signature.owner_type_name, signature.name), params)
        })
        .collect();

    ModuleSurfaceFacts {
        typed_dict_class_metadata,
        direct_function_signatures,
        direct_method_signatures,
        decorator_transform_module_info: collected.decorator_transform,
        dataclass_transform_module_info: collected.dataclass_transform,
    }
}

fn bind_statement(statement: &SyntaxStatement) -> Vec<Declaration> {
    match statement {
        SyntaxStatement::TypeAlias(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::TypeAlias,
            detail: statement.value.clone(),
            metadata: DeclarationMetadata::TypeAlias {
                value: statement
                    .value_expr
                    .clone()
                    .map(BoundTypeExpr::from_expr)
                    .unwrap_or_else(|| BoundTypeExpr::new(statement.value.clone())),
            },
            value_type_expr: None,
            method_kind: None,
            class_kind: None,
            owner: None,
            is_async: false,
            is_override: false,
            is_abstract_method: false,
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_final: false,
            is_class_var: false,
            bases: Vec::new(),
            type_params: bind_type_params(&statement.type_params),
        }],
        SyntaxStatement::Interface(statement) => {
            bind_named_block(statement, DeclarationOwnerKind::Interface)
        }
        SyntaxStatement::DataClass(statement) => {
            bind_named_block(statement, DeclarationOwnerKind::DataClass)
        }
        SyntaxStatement::SealedClass(statement) => {
            bind_named_block(statement, DeclarationOwnerKind::SealedClass)
        }
        SyntaxStatement::ClassDef(statement) => {
            bind_named_block(statement, DeclarationOwnerKind::Class)
        }
        SyntaxStatement::OverloadDef(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::Overload,
            detail: BoundCallableSignature::from_function_parts_with_expr(
                &statement.params,
                statement.returns.as_deref(),
                statement.returns_expr.as_ref(),
            )
            .rendered(),
            metadata: DeclarationMetadata::Callable {
                signature: BoundCallableSignature::from_function_parts_with_expr(
                    &statement.params,
                    statement.returns.as_deref(),
                    statement.returns_expr.as_ref(),
                ),
            },
            value_type_expr: None,
            method_kind: None,
            class_kind: None,
            owner: None,
            is_async: statement.is_async,
            is_override: false,
            is_abstract_method: false,
            is_final_decorator: false,
            is_deprecated: statement.is_deprecated,
            deprecation_message: statement.deprecation_message.clone(),
            is_final: false,
            is_class_var: false,
            bases: Vec::new(),
            type_params: bind_type_params(&statement.type_params),
        }],
        SyntaxStatement::FunctionDef(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::Function,
            detail: BoundCallableSignature::from_function_parts_with_expr(
                &statement.params,
                statement.returns.as_deref(),
                statement.returns_expr.as_ref(),
            )
            .rendered(),
            metadata: DeclarationMetadata::Callable {
                signature: BoundCallableSignature::from_function_parts_with_expr(
                    &statement.params,
                    statement.returns.as_deref(),
                    statement.returns_expr.as_ref(),
                ),
            },
            value_type_expr: None,
            method_kind: None,
            class_kind: None,
            owner: None,
            is_async: statement.is_async,
            is_override: statement.is_override,
            is_abstract_method: false,
            is_final_decorator: false,
            is_deprecated: statement.is_deprecated,
            deprecation_message: statement.deprecation_message.clone(),
            is_final: false,
            is_class_var: false,
            bases: Vec::new(),
            type_params: bind_type_params(&statement.type_params),
        }],
        SyntaxStatement::Import(statement) => statement
            .bindings
            .iter()
            .map(|binding| Declaration {
                name: binding.local_name.clone(),
                kind: DeclarationKind::Import,
                detail: binding.source_path.clone(),
                metadata: DeclarationMetadata::Import {
                    target: BoundImportTarget::new(binding.source_path.clone()),
                },
                value_type_expr: None,
                method_kind: None,
                class_kind: None,
                owner: None,
                is_async: false,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: false,
                bases: Vec::new(),
                type_params: Vec::new(),
            })
            .collect(),
        SyntaxStatement::Value(statement) => (statement.owner_name.is_none()
            && !value_statement_is_rebind_like_update(statement))
        .then_some(statement)
        .into_iter()
        .flat_map(|statement| {
            statement
                .names
                .iter()
                .cloned()
                .map(|name| Declaration {
                    name,
                    kind: DeclarationKind::Value,
                    detail: statement.annotation.clone().unwrap_or_default(),
                    metadata: DeclarationMetadata::Value {
                        annotation: statement
                            .annotation_expr
                            .clone()
                            .map(BoundTypeExpr::from_expr)
                            .or_else(|| statement.annotation.clone().map(BoundTypeExpr::new)),
                    },
                    value_type_expr: statement
                        .value_type_expr
                        .clone()
                        .map(BoundTypeExpr::from_expr)
                        .or_else(|| {
                            statement
                                .value_type
                                .clone()
                                .filter(|value| !value.is_empty())
                                .map(BoundTypeExpr::new)
                        }),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: statement.is_final,
                    is_class_var: statement.is_class_var,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                })
                .collect::<Vec<_>>()
        })
        .collect(),
        SyntaxStatement::Call(_) => Vec::new(),
        SyntaxStatement::MethodCall(_) => Vec::new(),
        SyntaxStatement::MemberAccess(_) => Vec::new(),
        SyntaxStatement::Return(_) => Vec::new(),
        SyntaxStatement::Yield(_) => Vec::new(),
        SyntaxStatement::If(_) => Vec::new(),
        SyntaxStatement::Assert(_) => Vec::new(),
        SyntaxStatement::Invalidate(_) => Vec::new(),
        SyntaxStatement::Match(_) => Vec::new(),
        SyntaxStatement::For(_) => Vec::new(),
        SyntaxStatement::With(_) => Vec::new(),
        SyntaxStatement::ExceptHandler(_) => Vec::new(),
        SyntaxStatement::Unsafe(_) => Vec::new(),
    }
}

fn value_statement_is_rebind_like_update(statement: &typepython_syntax::ValueStatement) -> bool {
    statement.annotation.is_none()
        && statement.names.len() == 1
        && statement.value_binop_operator.is_some()
        && statement.value_binop_left.as_deref().and_then(|left| left.value_name.as_deref())
            == statement.names.first().map(String::as_str)
}

fn map_guard_condition(condition: &typepython_syntax::GuardCondition) -> GuardConditionSite {
    match condition {
        typepython_syntax::GuardCondition::IsNone { name, negated } => {
            GuardConditionSite::IsNone { name: name.clone(), negated: *negated }
        }
        typepython_syntax::GuardCondition::IsInstance { name, types } => {
            GuardConditionSite::IsInstance { name: name.clone(), types: types.clone() }
        }
        typepython_syntax::GuardCondition::PredicateCall { name, callee } => {
            GuardConditionSite::PredicateCall { name: name.clone(), callee: callee.clone() }
        }
        typepython_syntax::GuardCondition::TruthyName { name } => {
            GuardConditionSite::TruthyName { name: name.clone() }
        }
        typepython_syntax::GuardCondition::Not(condition) => {
            GuardConditionSite::Not(Box::new(map_guard_condition(condition)))
        }
        typepython_syntax::GuardCondition::And(conditions) => {
            GuardConditionSite::And(conditions.iter().map(map_guard_condition).collect())
        }
        typepython_syntax::GuardCondition::Or(conditions) => {
            GuardConditionSite::Or(conditions.iter().map(map_guard_condition).collect())
        }
    }
}

fn bind_named_block(
    statement: &typepython_syntax::NamedBlockStatement,
    owner_kind: DeclarationOwnerKind,
) -> Vec<Declaration> {
    let owner = DeclarationOwner { name: statement.name.clone(), kind: owner_kind };
    let mut declarations = vec![Declaration {
        name: statement.name.clone(),
        kind: DeclarationKind::Class,
        detail: statement.bases.join(","),
        metadata: DeclarationMetadata::Class { bases: statement.bases.clone() },
        value_type_expr: None,
        method_kind: None,
        class_kind: Some(owner_kind),
        owner: None,
        is_async: false,
        is_override: false,
        is_abstract_method: false,
        is_final_decorator: statement.is_final_decorator,
        is_deprecated: statement.is_deprecated,
        deprecation_message: statement.deprecation_message.clone(),
        is_final: false,
        is_class_var: false,
        bases: statement.bases.clone(),
        type_params: bind_type_params(&statement.type_params),
    }];
    declarations.extend(statement.members.iter().map(|member| {
        Declaration {
            name: member.name.clone(),
            kind: match member.kind {
                typepython_syntax::ClassMemberKind::Field => DeclarationKind::Value,
                typepython_syntax::ClassMemberKind::Method => DeclarationKind::Function,
                typepython_syntax::ClassMemberKind::Overload => DeclarationKind::Overload,
            },
            detail: match member.kind {
                typepython_syntax::ClassMemberKind::Field => {
                    member.annotation.clone().unwrap_or_default()
                }
                typepython_syntax::ClassMemberKind::Method
                | typepython_syntax::ClassMemberKind::Overload => {
                    BoundCallableSignature::from_function_parts_with_expr(
                        &member.params,
                        member.returns.as_deref(),
                        member.returns_expr.as_ref(),
                    )
                    .rendered()
                }
            },
            metadata: match member.kind {
                typepython_syntax::ClassMemberKind::Field => DeclarationMetadata::Value {
                    annotation: member
                        .annotation_expr
                        .clone()
                        .map(BoundTypeExpr::from_expr)
                        .or_else(|| member.annotation.clone().map(BoundTypeExpr::new)),
                },
                typepython_syntax::ClassMemberKind::Method
                | typepython_syntax::ClassMemberKind::Overload => DeclarationMetadata::Callable {
                    signature: BoundCallableSignature::from_function_parts_with_expr(
                        &member.params,
                        member.returns.as_deref(),
                        member.returns_expr.as_ref(),
                    ),
                },
            },
            value_type_expr: member
                .value_type
                .clone()
                .filter(|value| !value.is_empty())
                .map(BoundTypeExpr::new),
            method_kind: member.method_kind,
            class_kind: None,
            owner: Some(owner.clone()),
            is_async: member.is_async,
            is_override: member.is_override,
            is_abstract_method: member.is_abstract_method,
            is_final_decorator: member.is_final_decorator,
            is_deprecated: member.is_deprecated,
            deprecation_message: member.deprecation_message.clone(),
            is_final: member.is_final,
            is_class_var: member.is_class_var,
            bases: Vec::new(),
            type_params: Vec::new(),
        }
    }));
    declarations
}

pub(super) fn direct_param_site_from_function_param(
    param: &typepython_syntax::FunctionParam,
) -> typepython_syntax::DirectFunctionParamSite {
    typepython_syntax::DirectFunctionParamSite {
        name: param.name.clone(),
        annotation: param.annotation.clone(),
        annotation_expr: param.annotation_expr.clone(),
        has_default: param.has_default,
        positional_only: param.positional_only,
        keyword_only: param.keyword_only,
        variadic: param.variadic,
        keyword_variadic: param.keyword_variadic,
    }
}

pub(super) fn render_signature_from_direct_params(
    params: &[typepython_syntax::DirectFunctionParamSite],
    returns: Option<&str>,
) -> String {
    let mut rendered_params = Vec::new();
    let positional_only_count = params.iter().filter(|param| param.positional_only).count();
    let has_variadic = params.iter().any(|param| param.variadic);
    let keyword_only_index = params.iter().position(|param| param.keyword_only);

    for (index, param) in params.iter().enumerate() {
        if keyword_only_index == Some(index) && !has_variadic {
            rendered_params.push(String::from("*"));
        }

        let mut rendered = match param.rendered_annotation() {
            Some(annotation) => format!("{}:{}", param.name, annotation),
            None => param.name.clone(),
        };
        if param.has_default {
            rendered.push('=');
        }
        if param.keyword_variadic {
            rendered = format!("**{rendered}");
        } else if param.variadic {
            rendered = format!("*{rendered}");
        }
        rendered_params.push(rendered);

        if positional_only_count > 0 && index + 1 == positional_only_count {
            rendered_params.push(String::from("/"));
        }
    }

    format!("({})->{}", rendered_params.join(","), returns.unwrap_or(""))
}

fn direct_expr_metadata_from_value_statement(
    statement: &typepython_syntax::ValueStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
        value_type_expr: statement.value_type_expr.clone(),
        is_awaited: statement.is_awaited,
        value_callee: statement.value_callee.clone(),
        value_name: statement.value_name.clone(),
        value_member_owner_name: statement.value_member_owner_name.clone(),
        value_member_name: statement.value_member_name.clone(),
        value_member_through_instance: statement.value_member_through_instance,
        value_method_owner_name: statement.value_method_owner_name.clone(),
        value_method_name: statement.value_method_name.clone(),
        value_method_through_instance: statement.value_method_through_instance,
        value_subscript_target: statement.value_subscript_target.clone(),
        value_subscript_string_key: statement.value_subscript_string_key.clone(),
        value_subscript_index: statement.value_subscript_index.clone(),
        value_if_true: statement.value_if_true.clone(),
        value_if_false: statement.value_if_false.clone(),
        value_if_guard: statement.value_if_guard.clone(),
        value_bool_left: statement.value_bool_left.clone(),
        value_bool_right: statement.value_bool_right.clone(),
        value_binop_left: statement.value_binop_left.clone(),
        value_binop_right: statement.value_binop_right.clone(),
        value_binop_operator: statement.value_binop_operator.clone(),
        value_lambda: statement.value_lambda.clone(),
        value_list_comprehension: statement.value_list_comprehension.clone(),
        value_generator_comprehension: statement.value_generator_comprehension.clone(),
        value_list_elements: statement.value_list_elements.clone(),
        value_set_elements: statement.value_set_elements.clone(),
        value_dict_entries: statement.value_dict_entries.clone(),
    })
}

fn direct_expr_metadata_from_return_statement(
    statement: &typepython_syntax::ReturnStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
        value_type_expr: statement
            .value_type
            .as_deref()
            .and_then(typepython_syntax::TypeExpr::parse),
        is_awaited: statement.is_awaited,
        value_callee: statement.value_callee.clone(),
        value_name: statement.value_name.clone(),
        value_member_owner_name: statement.value_member_owner_name.clone(),
        value_member_name: statement.value_member_name.clone(),
        value_member_through_instance: statement.value_member_through_instance,
        value_method_owner_name: statement.value_method_owner_name.clone(),
        value_method_name: statement.value_method_name.clone(),
        value_method_through_instance: statement.value_method_through_instance,
        value_subscript_target: statement.value_subscript_target.clone(),
        value_subscript_string_key: statement.value_subscript_string_key.clone(),
        value_subscript_index: statement.value_subscript_index.clone(),
        value_if_true: statement.value_if_true.clone(),
        value_if_false: statement.value_if_false.clone(),
        value_if_guard: statement.value_if_guard.clone(),
        value_bool_left: statement.value_bool_left.clone(),
        value_bool_right: statement.value_bool_right.clone(),
        value_binop_left: statement.value_binop_left.clone(),
        value_binop_right: statement.value_binop_right.clone(),
        value_binop_operator: statement.value_binop_operator.clone(),
        value_lambda: statement.value_lambda.clone(),
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: statement.value_list_elements.clone(),
        value_set_elements: statement.value_set_elements.clone(),
        value_dict_entries: statement.value_dict_entries.clone(),
    })
}

fn direct_expr_metadata_from_yield_statement(
    statement: &typepython_syntax::YieldStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
        value_type_expr: statement
            .value_type
            .as_deref()
            .and_then(typepython_syntax::TypeExpr::parse),
        is_awaited: false,
        value_callee: statement.value_callee.clone(),
        value_name: statement.value_name.clone(),
        value_member_owner_name: statement.value_member_owner_name.clone(),
        value_member_name: statement.value_member_name.clone(),
        value_member_through_instance: statement.value_member_through_instance,
        value_method_owner_name: statement.value_method_owner_name.clone(),
        value_method_name: statement.value_method_name.clone(),
        value_method_through_instance: statement.value_method_through_instance,
        value_subscript_target: statement.value_subscript_target.clone(),
        value_subscript_string_key: statement.value_subscript_string_key.clone(),
        value_subscript_index: statement.value_subscript_index.clone(),
        value_if_true: statement.value_if_true.clone(),
        value_if_false: statement.value_if_false.clone(),
        value_if_guard: statement.value_if_guard.clone(),
        value_bool_left: statement.value_bool_left.clone(),
        value_bool_right: statement.value_bool_right.clone(),
        value_binop_left: statement.value_binop_left.clone(),
        value_binop_right: statement.value_binop_right.clone(),
        value_binop_operator: statement.value_binop_operator.clone(),
        value_lambda: statement.value_lambda.clone(),
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: statement.value_list_elements.clone(),
        value_set_elements: statement.value_set_elements.clone(),
        value_dict_entries: statement.value_dict_entries.clone(),
    })
}

fn direct_expr_metadata_from_match_statement(
    statement: &typepython_syntax::MatchStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
        value_type_expr: statement
            .subject_type
            .as_deref()
            .and_then(typepython_syntax::TypeExpr::parse),
        is_awaited: statement.subject_is_awaited,
        value_callee: statement.subject_callee.clone(),
        value_name: statement.subject_name.clone(),
        value_member_owner_name: statement.subject_member_owner_name.clone(),
        value_member_name: statement.subject_member_name.clone(),
        value_member_through_instance: statement.subject_member_through_instance,
        value_method_owner_name: statement.subject_method_owner_name.clone(),
        value_method_name: statement.subject_method_name.clone(),
        value_method_through_instance: statement.subject_method_through_instance,
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
    })
}

fn direct_expr_metadata_from_for_statement(
    statement: &typepython_syntax::ForStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
        value_type_expr: statement
            .iter_type
            .as_deref()
            .and_then(typepython_syntax::TypeExpr::parse),
        is_awaited: statement.iter_is_awaited,
        value_callee: statement.iter_callee.clone(),
        value_name: statement.iter_name.clone(),
        value_member_owner_name: statement.iter_member_owner_name.clone(),
        value_member_name: statement.iter_member_name.clone(),
        value_member_through_instance: statement.iter_member_through_instance,
        value_method_owner_name: statement.iter_method_owner_name.clone(),
        value_method_name: statement.iter_method_name.clone(),
        value_method_through_instance: statement.iter_method_through_instance,
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
    })
}

fn direct_expr_metadata_from_with_statement(
    statement: &typepython_syntax::WithStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
        value_type_expr: statement
            .context_type
            .as_deref()
            .and_then(typepython_syntax::TypeExpr::parse),
        is_awaited: statement.context_is_awaited,
        value_callee: statement.context_callee.clone(),
        value_name: statement.context_name.clone(),
        value_member_owner_name: statement.context_member_owner_name.clone(),
        value_member_name: statement.context_member_name.clone(),
        value_member_through_instance: statement.context_member_through_instance,
        value_method_owner_name: statement.context_method_owner_name.clone(),
        value_method_name: statement.context_method_name.clone(),
        value_method_through_instance: statement.context_method_through_instance,
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
    })
}

pub(super) fn direct_expr_metadata_from_parts(
    metadata: typepython_syntax::DirectExprMetadata,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_present(&metadata).then_some(metadata)
}

pub(super) fn direct_expr_metadata_present(
    metadata: &typepython_syntax::DirectExprMetadata,
) -> bool {
    metadata.value_type_expr.is_some()
        || metadata.is_awaited
        || metadata.value_callee.is_some()
        || metadata.value_name.is_some()
        || metadata.value_member_owner_name.is_some()
        || metadata.value_member_name.is_some()
        || metadata.value_member_through_instance
        || metadata.value_method_owner_name.is_some()
        || metadata.value_method_name.is_some()
        || metadata.value_method_through_instance
        || metadata.value_subscript_target.is_some()
        || metadata.value_subscript_string_key.is_some()
        || metadata.value_subscript_index.is_some()
        || metadata.value_if_true.is_some()
        || metadata.value_if_false.is_some()
        || metadata.value_if_guard.is_some()
        || metadata.value_bool_left.is_some()
        || metadata.value_bool_right.is_some()
        || metadata.value_binop_left.is_some()
        || metadata.value_binop_right.is_some()
        || metadata.value_binop_operator.is_some()
        || metadata.value_lambda.is_some()
        || metadata.value_list_comprehension.is_some()
        || metadata.value_generator_comprehension.is_some()
        || metadata.value_list_elements.is_some()
        || metadata.value_set_elements.is_some()
        || metadata.value_dict_entries.is_some()
}

pub(super) fn guard_condition_site_to_guard(
    condition: &GuardConditionSite,
) -> typepython_syntax::GuardCondition {
    match condition {
        GuardConditionSite::IsNone { name, negated } => {
            typepython_syntax::GuardCondition::IsNone { name: name.clone(), negated: *negated }
        }
        GuardConditionSite::IsInstance { name, types } => {
            typepython_syntax::GuardCondition::IsInstance {
                name: name.clone(),
                types: types.clone(),
            }
        }
        GuardConditionSite::PredicateCall { name, callee } => {
            typepython_syntax::GuardCondition::PredicateCall {
                name: name.clone(),
                callee: callee.clone(),
            }
        }
        GuardConditionSite::TruthyName { name } => {
            typepython_syntax::GuardCondition::TruthyName { name: name.clone() }
        }
        GuardConditionSite::Not(inner) => {
            typepython_syntax::GuardCondition::Not(Box::new(guard_condition_site_to_guard(inner)))
        }
        GuardConditionSite::And(conditions) => typepython_syntax::GuardCondition::And(
            conditions.iter().map(guard_condition_site_to_guard).collect(),
        ),
        GuardConditionSite::Or(conditions) => typepython_syntax::GuardCondition::Or(
            conditions.iter().map(guard_condition_site_to_guard).collect(),
        ),
    }
}

fn bind_type_params(type_params: &[typepython_syntax::TypeParam]) -> Vec<GenericTypeParam> {
    type_params
        .iter()
        .map(|param| GenericTypeParam {
            kind: match param.kind {
                typepython_syntax::TypeParamKind::TypeVar => GenericTypeParamKind::TypeVar,
                typepython_syntax::TypeParamKind::ParamSpec => GenericTypeParamKind::ParamSpec,
                typepython_syntax::TypeParamKind::TypeVarTuple => {
                    GenericTypeParamKind::TypeVarTuple
                }
            },
            name: param.name.clone(),
            bound: param.bound.clone(),
            bound_expr: param.bound_expr.clone().map(|expr| BoundTypeExpr { expr }),
            constraints: param.constraints.clone(),
            constraint_exprs: param
                .constraint_exprs
                .clone()
                .into_iter()
                .map(|expr| BoundTypeExpr { expr })
                .collect(),
            default: param.default.clone(),
            default_expr: param.default_expr.clone().map(|expr| BoundTypeExpr { expr }),
        })
        .collect()
}

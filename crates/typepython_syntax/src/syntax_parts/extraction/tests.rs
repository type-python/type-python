use super::{
    AssertStatement, CallStatement, ClassMember, ClassMemberKind, ComprehensionKind,
    ComprehensionMetadata, DirectExprMetadata, ExceptionHandlerStatement, ForStatement,
    FunctionParam, FunctionStatement, GuardCondition, IfStatement, ImportBinding, ImportStatement,
    InvalidationKind, InvalidationStatement, LambdaMetadata, MatchCaseStatement, MatchPattern,
    MatchStatement, MemberAccessStatement, MethodCallStatement, MethodKind, NamedBlockStatement,
    ParseOptions, ParsePythonVersion, ParseTargetPlatform, ReturnStatement, SourceFile, SourceKind,
    SyntaxStatement, TypeAliasStatement, TypeExpr, TypeIgnoreDirective, TypeParam, TypeParamKind,
    TypedDictLiteralEntry, UnsafeStatement, ValueStatement, WithStatement, YieldStatement,
    direct_expr_metadata_vec_from_type_texts, parse, parse_with_options,
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
        SyntaxStatement::Value(statement) => {
            SyntaxStatement::Value(normalize_value_statement(statement))
        }
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
    statement.type_params = statement.type_params.into_iter().map(normalize_type_param).collect();
    statement.members = statement.members.into_iter().map(normalize_class_member).collect();
    statement
}

fn normalize_function_statement(mut statement: FunctionStatement) -> FunctionStatement {
    statement.type_params = statement.type_params.into_iter().map(normalize_type_param).collect();
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
    statement.value_dict_entries =
        normalize_typed_dict_literal_entries(statement.value_dict_entries);
    statement
}

fn normalize_call_statement(mut statement: CallStatement) -> CallStatement {
    statement.arg_values = statement.arg_values.into_iter().map(normalize_direct_expr).collect();
    statement.starred_arg_values =
        statement.starred_arg_values.into_iter().map(normalize_direct_expr).collect();
    statement.keyword_arg_values =
        statement.keyword_arg_values.into_iter().map(normalize_direct_expr).collect();
    statement.keyword_expansion_values =
        statement.keyword_expansion_values.into_iter().map(normalize_direct_expr).collect();
    statement
}

fn normalize_method_call_statement(mut statement: MethodCallStatement) -> MethodCallStatement {
    statement.arg_values = statement.arg_values.into_iter().map(normalize_direct_expr).collect();
    statement.starred_arg_values =
        statement.starred_arg_values.into_iter().map(normalize_direct_expr).collect();
    statement.keyword_arg_values =
        statement.keyword_arg_values.into_iter().map(normalize_direct_expr).collect();
    statement.keyword_expansion_values =
        statement.keyword_expansion_values.into_iter().map(normalize_direct_expr).collect();
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
    statement.value_dict_entries =
        normalize_typed_dict_literal_entries(statement.value_dict_entries);
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
    statement.value_dict_entries =
        normalize_typed_dict_literal_entries(statement.value_dict_entries);
    statement
}

fn normalize_lambda_option(metadata: Option<Box<LambdaMetadata>>) -> Option<Box<LambdaMetadata>> {
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
                entry.key_value =
                    entry.key_value.map(|value| Box::new(normalize_direct_expr(*value)));
                entry.value = normalize_direct_expr(entry.value);
                entry
            })
            .collect()
    })
}

fn normalize_direct_expr(mut metadata: DirectExprMetadata) -> DirectExprMetadata {
    metadata.value_subscript_target = normalize_direct_expr_option(metadata.value_subscript_target);
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
                value_type_expr: Some(TypeExpr::Name(String::from("int"))),
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
                value_type_expr: Some(TypeExpr::Name(String::from("int"))),
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
        text: String::from("value: int = helper()\ncopy: str = source\nfield: str = box.value\n"),
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
                arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                starred_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_names: Vec::new(),
                keyword_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_expansion_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                line: 1,
            }),
            SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("copy")],
                destructuring_target_names: None,
                annotation: Some(String::from("str")),
                annotation_expr: None,
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
                arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                starred_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_names: Vec::new(),
                keyword_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_expansion_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                line: 3,
            }),
            SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("item")],
                destructuring_target_names: None,
                annotation: Some(String::from("str")),
                annotation_expr: None,
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
        text: String::from("def build() -> None:\n    value = helper()\n    field = box.item\n"),
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
    assert_eq!(
        walrus_assignment.value_type_expr.as_ref().map(TypeExpr::render).as_deref(),
        Some("int")
    );
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
                arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                starred_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_names: Vec::new(),
                keyword_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_expansion_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                line: 1,
            }),
            SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("value")],
                destructuring_target_names: None,
                annotation: None,
                annotation_expr: None,
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
                arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                starred_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_names: Vec::new(),
                keyword_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_expansion_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
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
            arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
            starred_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
            keyword_names: vec![String::from("x"), String::from("y")],
            keyword_arg_values: vec![
                DirectExprMetadata {
                    value_type_expr: Some(TypeExpr::Name(String::from("int"))),
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
                    value_type_expr: Some(TypeExpr::Name(String::from("int"))),
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
            keyword_expansion_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
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
            SyntaxStatement::Value(statement) if statement.names == vec![String::from("value")] => {
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
            arg_values: vec![
                DirectExprMetadata {
                    value_type_expr: Some(TypeExpr::Name(String::from("int"))),
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
                    value_type_expr: Some(TypeExpr::Name(String::from("str"))),
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
            starred_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
            keyword_names: Vec::new(),
            keyword_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
            keyword_expansion_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
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
        statement
            .arg_values
            .iter()
            .map(|metadata| metadata.rendered_value_type().unwrap_or_default())
            .collect::<Vec<_>>(),
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
    assert_eq!(list_elements[0].rendered_value_type().as_deref(), Some("int"));
    let dict_entries = statement.arg_values[2].value_dict_entries.as_ref().expect("dict entries");
    assert_eq!(dict_entries.len(), 1);
    assert_eq!(dict_entries[0].key.as_deref(), Some("x"));
    assert!(!dict_entries[0].is_expansion);
    let set_elements = statement.arg_values[3].value_set_elements.as_ref().expect("set elements");
    assert_eq!(set_elements.len(), 2);
    assert_eq!(set_elements[0].rendered_value_type().as_deref(), Some("int"));
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
    assert_eq!(
        statement
            .starred_arg_values
            .iter()
            .map(|metadata| metadata.rendered_value_type().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![String::from("list[int]")]
    );
    assert_eq!(
        statement
            .keyword_expansion_values
            .iter()
            .map(|metadata| metadata.rendered_value_type().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![String::from("dict[str, int]")]
    );
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
    let keyword_entries =
        statement.keyword_arg_values[0].value_dict_entries.as_ref().expect("keyword dict entries");
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
            arg_values: vec![DirectExprMetadata {
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
            starred_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
            keyword_names: Vec::new(),
            keyword_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
            keyword_expansion_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
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
    let comprehension = statement.value_list_comprehension.as_deref().expect("list comprehension");
    assert_eq!(comprehension.kind, ComprehensionKind::List);
    assert_eq!(comprehension.clauses.len(), 1);
    assert_eq!(comprehension.clauses[0].target_names, vec![String::from("x")]);
    let iter_elements =
        comprehension.clauses[0].iter.value_list_elements.as_ref().expect("iter list elements");
    assert_eq!(iter_elements.len(), 2);
    assert_eq!(iter_elements[0].rendered_value_type().as_deref(), Some("int"));
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
    assert_eq!(iter_elements[0].rendered_value_type().as_deref(), Some("int"));
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
    let comprehension = statement.value_list_comprehension.as_deref().expect("set comprehension");
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
    let comprehension = statement.value_list_comprehension.as_deref().expect("dict comprehension");
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
                value_type_expr: Some(TypeExpr::Name(String::from("int"))),
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
                value_type_expr: Some(TypeExpr::Name(String::from("int"))),
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
                arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                starred_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_names: Vec::new(),
                keyword_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_expansion_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
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
                value_type_expr: TypeExpr::parse("str"),
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
                value_type_expr: TypeExpr::parse("bool"),
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
                value_type_expr: TypeExpr::parse("None"),
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
                current_owner_name: None,
                current_owner_type_name: None,
                owner_name: String::from("Box"),
                method: String::from("run"),
                through_instance: false,
                arg_count: 1,
                arg_values: vec![DirectExprMetadata {
                    value_type_expr: Some(TypeExpr::Name(String::from("int"))),
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
                starred_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_names: Vec::new(),
                keyword_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_expansion_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                line: 1,
            }),
            SyntaxStatement::MethodCall(MethodCallStatement {
                current_owner_name: None,
                current_owner_type_name: None,
                owner_name: String::from("Box"),
                method: String::from("build"),
                through_instance: true,
                arg_count: 0,
                arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                starred_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_names: vec![String::from("x")],
                keyword_arg_values: vec![DirectExprMetadata {
                    value_type_expr: Some(TypeExpr::Name(String::from("int"))),
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
                keyword_expansion_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
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
                value_type_expr: TypeExpr::parse("int"),
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
                value_type_expr: Some(TypeExpr::Name(String::from("int"))),
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
    assert!(
        tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Import(ImportStatement { bindings, line })
                if *line == 3
                    && bindings == &vec![ImportBinding {
                        local_name: String::from("User"),
                        source_path: String::from("app.models.User"),
                    }]
        )),
        "{:?}",
        tree.statements
    );
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
    assert!(
        tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Import(ImportStatement { bindings, line })
                if *line == 3
                    && bindings == &vec![ImportBinding {
                        local_name: String::from("User"),
                        source_path: String::from("app.models.User"),
                    }]
        )),
        "{:?}",
        tree.statements
    );
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
    assert!(
        tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Import(ImportStatement { bindings, line })
                if *line == 3
                    && bindings == &vec![ImportBinding {
                        local_name: String::from("NewUser"),
                        source_path: String::from("app.models.NewUser"),
                    }]
        )),
        "{:?}",
        tree.statements
    );
    assert!(
        !tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Import(ImportStatement { bindings, line })
                if *line == 5
                    && bindings == &vec![ImportBinding {
                        local_name: String::from("OldUser"),
                        source_path: String::from("app.models.OldUser"),
                    }]
        )),
        "{:?}",
        tree.statements
    );
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
    assert!(
        tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::Import(ImportStatement { bindings, line })
                if *line == 3
                    && bindings == &vec![ImportBinding {
                        local_name: String::from("MacOnly"),
                        source_path: String::from("app.models.MacOnly"),
                    }]
        )),
        "{:?}",
        tree.statements
    );
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
    assert!(
        tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::ClassDef(NamedBlockStatement { name, line, .. })
                if name == "User" && *line == 3
        )),
        "{:?}",
        tree.statements
    );
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
    assert!(
        tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::TypeAlias(TypeAliasStatement { name, value, line, .. })
                if name == "UserId" && value == "int" && *line == 3
        )),
        "{:?}",
        tree.statements
    );
    assert!(
        !tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::TypeAlias(TypeAliasStatement { name, value, line, .. })
                if name == "UserId" && value == "str" && *line == 5
        )),
        "{:?}",
        tree.statements
    );
}

#[test]
fn parse_python_native_type_alias_statement() {
    let tree = parse(SourceFile {
        path: PathBuf::from("native-type-alias.py"),
        kind: SourceKind::Python,
        logical_module: String::new(),
        text: String::from("type Pair[T] = tuple[T, T]\n"),
    });

    assert!(tree.diagnostics.is_empty());
    assert!(tree.statements.iter().any(|statement| matches!(
        statement,
        SyntaxStatement::TypeAlias(TypeAliasStatement { name, value, type_params, line, .. })
            if name == "Pair"
                && value == "tuple[T, T]"
                && type_params.len() == 1
                && *line == 1
    )));
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
                value_type_expr: Some(TypeExpr::Name(String::from("int"))),
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
                value_type_expr: TypeExpr::parse("int"),
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
                value_type_expr: TypeExpr::parse("int"),
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
                value_type_expr: None,
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
                value_type_expr: TypeExpr::parse("int"),
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
                value_type_expr: None,
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
        tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::MethodCall(MethodCallStatement {
                owner_name,
                method,
                through_instance: false,
                line,
                ..
            }) if owner_name == "box" && method == "get" && *line == 3
        )),
        "{:?}",
        tree.statements
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
    assert!(
        tree.statements.iter().any(|statement| matches!(
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
        )),
        "{:?}",
        tree.statements
    );
    assert!(
        tree.statements.iter().any(|statement| matches!(
            statement,
            SyntaxStatement::MethodCall(MethodCallStatement {
                owner_name,
                method,
                through_instance: true,
                line,
                ..
            }) if owner_name == "make_box" && method == "get" && *line == 2
        )),
        "{:?}",
        tree.statements
    );
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
                iter_type_expr: None,
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
                value_type_expr: None,
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
                iter_type_expr: None,
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
                value_type_expr: None,
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
                context_type_expr: None,
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
                context_type_expr: None,
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
                context_type_expr: None,
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
                context_type_expr: None,
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
                value_type_expr: None,
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
                arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                starred_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_names: Vec::new(),
                keyword_arg_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
                keyword_expansion_values: direct_expr_metadata_vec_from_type_texts(Vec::new()),
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
                value_type_expr: None,
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
                value_type_expr: TypeExpr::parse("str"),
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
                value_type_expr: TypeExpr::parse("str"),
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
            subject_type_expr: None,
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

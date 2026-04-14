use super::{
    AssertGuardSite, AssignmentSite, BoundCallableSignature, BoundImportTarget, BoundTypeExpr,
    CallSite, Declaration, DeclarationKind, DeclarationMetadata, DeclarationOwner,
    DeclarationOwnerKind, ExceptHandlerSite, ForSite, GenericTypeParam, GenericTypeParamKind,
    GuardConditionSite, IfGuardSite, InvalidationKind, InvalidationSite, MatchCaseSite,
    MatchPatternSite, MatchSite, WithSite, YieldSite, bind,
};
use std::path::PathBuf;
use typepython_diagnostics::DiagnosticReport;
use typepython_syntax::{
    ClassMember, ClassMemberKind, DirectExprMetadata, FunctionParam, FunctionStatement,
    ImportStatement, MethodKind, NamedBlockStatement, SourceFile, SourceKind, SyntaxStatement,
    SyntaxTree, TypeAliasStatement, TypeParam, TypeParamKind, ValueStatement,
};

fn metadata_type_alias(text: &str) -> DeclarationMetadata {
    DeclarationMetadata::TypeAlias { value: BoundTypeExpr::new(text) }
}

fn metadata_value(annotation: Option<&str>) -> DeclarationMetadata {
    DeclarationMetadata::Value { annotation: annotation.map(BoundTypeExpr::new) }
}

fn metadata_class(bases: &[&str]) -> DeclarationMetadata {
    DeclarationMetadata::Class { bases: bases.iter().map(|base| String::from(*base)).collect() }
}

fn metadata_import(target: &str) -> DeclarationMetadata {
    DeclarationMetadata::Import { target: BoundImportTarget::new(target) }
}

fn metadata_empty_callable() -> DeclarationMetadata {
    DeclarationMetadata::Callable {
        signature: BoundCallableSignature { params: Vec::new(), returns: None },
    }
}

#[test]
fn declaration_text_accessors_prefer_structured_metadata() {
    let value = Declaration {
        metadata: DeclarationMetadata::Value { annotation: Some(BoundTypeExpr::new("list[int]")) },
        name: String::from("items"),
        kind: DeclarationKind::Value,
        detail: String::from("str"),
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
    };
    assert_eq!(value.value_annotation_text().as_deref(), Some("list[int]"));

    let alias = Declaration {
        metadata: DeclarationMetadata::TypeAlias { value: BoundTypeExpr::new("int | None") },
        name: String::from("MaybeInt"),
        kind: DeclarationKind::TypeAlias,
        detail: String::from("str"),
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
    };
    assert_eq!(alias.type_alias_body_text().as_deref(), Some("int | None"));

    let function = Declaration {
        metadata: DeclarationMetadata::Callable {
            signature: BoundCallableSignature {
                params: vec![typepython_syntax::DirectFunctionParamSite {
                    name: String::from("value"),
                    annotation: Some(String::from("list[int]")),
                    annotation_expr: Some(typepython_syntax::TypeExpr::Generic {
                        head: String::from("list"),
                        args: vec![typepython_syntax::TypeExpr::Name(String::from("int"))],
                    }),
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                }],
                returns: Some(BoundTypeExpr::new("tuple[int]")),
            },
        },
        name: String::from("build"),
        kind: DeclarationKind::Function,
        detail: String::from("(value:str)->str"),
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
    };
    assert_eq!(
        function.callable_signature_text().as_deref(),
        Some("(value:list[int])->tuple[int]")
    );

    let import = Declaration {
        metadata: DeclarationMetadata::Import { target: BoundImportTarget::new("pkg.sub.Symbol") },
        name: String::from("Symbol"),
        kind: DeclarationKind::Import,
        detail: String::from("wrong.target"),
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
    };
    assert_eq!(import.import_raw_target_text().as_deref(), Some("pkg.sub.Symbol"));
    assert_eq!(import.import_module_target_text().as_deref(), Some("pkg.sub.Symbol"));
}

#[test]
fn call_site_arg_type_accessors_prefer_direct_expr_metadata() {
    let call = CallSite {
        callee: String::from("build"),
        arg_count: 1,
        arg_values: vec![DirectExprMetadata {
            value_type: Some(String::from("str")),
            value_type_expr: Some(typepython_syntax::TypeExpr::Generic {
                head: String::from("list"),
                args: vec![typepython_syntax::TypeExpr::Name(String::from("int"))],
            }),
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
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("tuple[str, ...]"),
        ]),
        keyword_names: vec![String::from("count")],
        keyword_arg_values: vec![DirectExprMetadata {
            value_type: Some(String::from("str")),
            value_type_expr: Some(typepython_syntax::TypeExpr::Name(String::from("int"))),
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
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            vec![String::from("dict[str, str]")],
        ),
        line: 1,
    };

    assert_eq!(call.positional_arg_type_texts(), vec![String::from("list[int]")]);
    assert_eq!(call.starred_arg_type_texts(), vec![String::from("tuple[str, ...]")]);
    assert_eq!(call.keyword_arg_type_texts(), vec![String::from("int")]);
    assert_eq!(call.keyword_expansion_type_texts(), vec![String::from("dict[str, str]")]);
}

#[test]
fn bind_collects_top_level_aliases_classes_and_functions() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/__init__.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: String::new(),
        },
        statements: vec![
            SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("UserId"),
                type_params: vec![TypeParam {
                    name: String::from("T"),
                    kind: TypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
                value: String::from("Box[T]"),
                value_expr: None,
                line: 1,
            }),
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("User"),
                type_params: vec![TypeParam {
                    name: String::from("T"),
                    kind: TypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
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
            SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("helper"),
                type_params: vec![TypeParam {
                    name: String::from("T"),
                    kind: TypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
                params: Vec::new(),
                returns: None,
                returns_expr: None,
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 3,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    println!("{} {:?}", table.module_key, table.declarations);
    assert_eq!(table.module_key, "app");
    assert_eq!(
        table.declarations,
        vec![
            Declaration {
                metadata: metadata_type_alias("Box[T]"),
                name: String::from("UserId"),
                kind: DeclarationKind::TypeAlias,
                detail: String::from("Box[T]"),
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
                type_params: vec![GenericTypeParam {
                    name: String::from("T"),
                    kind: GenericTypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
            },
            Declaration {
                metadata: metadata_class(&[]),
                name: String::from("User"),
                kind: DeclarationKind::Class,
                detail: String::new(),
                value_type_expr: None,
                method_kind: None,
                class_kind: Some(DeclarationOwnerKind::Class),
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
                type_params: vec![GenericTypeParam {
                    name: String::from("T"),
                    kind: GenericTypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
            },
            Declaration {
                metadata: metadata_empty_callable(),
                name: String::from("helper"),
                kind: DeclarationKind::Function,
                detail: String::from("()->"),
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
                type_params: vec![GenericTypeParam {
                    name: String::from("T"),
                    kind: GenericTypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
            },
        ]
    );
}

#[test]
fn bind_marks_async_functions() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/fetch.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::FunctionDef(FunctionStatement {
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
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 1);
    assert!(table.declarations[0].is_async);
    assert_eq!(table.declarations[0].detail, String::from("()->int"));
}

#[test]
fn bind_marks_overload_definitions_separately() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/__init__.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![
            SyntaxStatement::OverloadDef(FunctionStatement {
                name: String::from("parse"),
                type_params: vec![TypeParam {
                    name: String::from("T"),
                    kind: TypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
                params: Vec::new(),
                returns: None,
                returns_expr: None,
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 1,
            }),
            SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("parse"),
                type_params: Vec::new(),
                params: Vec::new(),
                returns: None,
                returns_expr: None,
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 2,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.declarations,
        vec![
            Declaration {
                metadata: metadata_empty_callable(),
                name: String::from("parse"),
                kind: DeclarationKind::Overload,
                detail: String::from("()->"),
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
                type_params: vec![GenericTypeParam {
                    name: String::from("T"),
                    kind: GenericTypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
            },
            Declaration {
                metadata: metadata_empty_callable(),
                name: String::from("parse"),
                kind: DeclarationKind::Function,
                detail: String::from("()->"),
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
            },
        ]
    );
}

#[test]
fn bind_collects_imports_and_values_from_syntax_tree() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/helpers.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![
            SyntaxStatement::Import(ImportStatement {
                bindings: vec![
                    typepython_syntax::ImportBinding {
                        local_name: String::from("local_foo"),
                        source_path: String::from("pkg.foo"),
                    },
                    typepython_syntax::ImportBinding {
                        local_name: String::from("bar"),
                        source_path: String::from("pkg.bar"),
                    },
                ],
                line: 1,
            }),
            SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("value"), String::from("count")],
                destructuring_target_names: None,
                annotation: None,
                annotation_expr: None,
                value_type_expr: None,
                value_type: None,
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
                line: 2,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.declarations,
        vec![
            Declaration {
                metadata: metadata_import("pkg.foo"),
                name: String::from("local_foo"),
                kind: DeclarationKind::Import,
                detail: String::from("pkg.foo"),
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
            },
            Declaration {
                metadata: metadata_import("pkg.bar"),
                name: String::from("bar"),
                kind: DeclarationKind::Import,
                detail: String::from("pkg.bar"),
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
            },
            Declaration {
                metadata: metadata_value(None),
                name: String::from("value"),
                kind: DeclarationKind::Value,
                detail: String::new(),
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
            },
            Declaration {
                metadata: metadata_value(None),
                name: String::from("count"),
                kind: DeclarationKind::Value,
                detail: String::new(),
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
            },
        ]
    );
}

#[test]
fn bind_collects_assignment_sites_from_syntax_tree() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/helpers.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![
            SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("value")],
                destructuring_target_names: None,
                annotation: Some(String::from("int")),
                annotation_expr: None,
                value_type_expr: None,
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
            SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("copy")],
                destructuring_target_names: None,
                annotation: Some(String::from("str")),
                annotation_expr: None,
                value_type_expr: None,
                value_type: Some(String::new()),
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
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.assignments,
        vec![
            AssignmentSite {
                annotation_expr: Some(BoundTypeExpr::new("int")),
                value: Some(DirectExprMetadata {
                    value_type_expr: None,
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
                    value_list_comprehension: None,
                    value_generator_comprehension: None,
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                }),
                name: String::from("value"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("int")),
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
                value_list_comprehension: None,
                value_generator_comprehension: None,
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                owner_name: None,
                owner_type_name: None,
                line: 1,
            },
            AssignmentSite {
                annotation_expr: Some(BoundTypeExpr::new("str")),
                value: Some(DirectExprMetadata {
                    value_type_expr: None,
                    value_type: Some(String::new()),
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
                }),
                name: String::from("copy"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("str")),
                value_type: Some(String::new()),
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
                line: 2,
            },
        ]
    );
}

#[test]
fn bind_keeps_local_assignments_out_of_declarations() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/helpers.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![
            SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("build"),
                type_params: Vec::new(),
                params: vec![typepython_syntax::FunctionParam {
                    name: String::from("value"),
                    annotation: Some(String::from("str")),
                    annotation_expr: None,
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
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
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 1);
    assert_eq!(table.declarations[0].name, "build");
    assert_eq!(
        table.assignments,
        vec![AssignmentSite {
            annotation_expr: Some(BoundTypeExpr::new("int")),
            value: Some(DirectExprMetadata {
                value_type_expr: None,
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
                value_list_comprehension: None,
                value_generator_comprehension: None,
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
            }),
            name: String::from("result"),
            destructuring_target_names: None,
            destructuring_index: None,
            annotation: Some(String::from("int")),
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
            value_list_comprehension: None,
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
            owner_name: Some(String::from("build")),
            owner_type_name: None,
            line: 2,
        }]
    );
}

#[test]
fn bind_collects_local_bare_assignments() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/helpers.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![
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
            SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("result")],
                destructuring_target_names: None,
                annotation: None,
                annotation_expr: None,
                value_type_expr: None,
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
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 1);
    assert_eq!(table.declarations[0].name, "build");
    assert_eq!(
        table.assignments,
        vec![AssignmentSite {
            annotation_expr: None,
            value: Some(DirectExprMetadata {
                value_type_expr: None,
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
                value_list_comprehension: None,
                value_generator_comprehension: None,
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
            }),
            name: String::from("result"),
            destructuring_target_names: None,
            destructuring_index: None,
            annotation: None,
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
            value_list_comprehension: None,
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
            owner_name: Some(String::from("build")),
            owner_type_name: None,
            line: 2,
        }]
    );
}

#[test]
fn bind_tracks_destructuring_assignment_indexes() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/helpers.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::Value(ValueStatement {
            names: vec![String::from("left"), String::from("right")],
            destructuring_target_names: Some(vec![String::from("left"), String::from("right")]),
            annotation: None,
            annotation_expr: None,
            value_type_expr: None,
            value_type: Some(String::from("tuple[int, str]")),
            is_awaited: false,
            value_callee: None,
            value_name: Some(String::from("pair")),
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
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.assignments.len(), 2);
    assert_eq!(table.assignments[0].name, "left");
    assert_eq!(table.assignments[0].destructuring_index, Some(0));
    assert_eq!(
        table.assignments[0].destructuring_target_names,
        Some(vec![String::from("left"), String::from("right")])
    );
    assert_eq!(table.assignments[1].name, "right");
    assert_eq!(table.assignments[1].destructuring_index, Some(1));
}

#[test]
fn bind_collects_yield_sites_from_syntax_tree() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/gen.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::Yield(typepython_syntax::YieldStatement {
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
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.yields,
        vec![YieldSite {
            value: Some(DirectExprMetadata {
                value_type_expr: Some(typepython_syntax::TypeExpr::Name(String::from("int"))),
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
                value_list_comprehension: None,
                value_generator_comprehension: None,
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
            }),
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
        }]
    );
}

#[test]
fn bind_collects_for_sites_from_syntax_tree() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/loop.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::For(typepython_syntax::ForStatement {
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
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.for_loops,
        vec![ForSite {
            iter: Some(DirectExprMetadata {
                value_type_expr: None,
                value_type: Some(String::new()),
                is_awaited: false,
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
                value_list_comprehension: None,
                value_generator_comprehension: None,
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
            }),
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
        }]
    );
}

#[test]
fn bind_collects_match_sites_from_syntax_tree() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/match.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::Match(typepython_syntax::MatchStatement {
            owner_name: Some(String::from("build")),
            owner_type_name: None,
            subject_type: Some(String::new()),
            subject_is_awaited: false,
            subject_callee: None,
            subject_name: Some(String::from("expr")),
            subject_member_owner_name: None,
            subject_member_name: None,
            subject_member_through_instance: false,
            subject_method_owner_name: None,
            subject_method_name: None,
            subject_method_through_instance: false,
            cases: vec![typepython_syntax::MatchCaseStatement {
                patterns: vec![typepython_syntax::MatchPattern::Class(String::from("Add"))],
                has_guard: false,
                line: 3,
            }],
            line: 2,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.matches,
        vec![MatchSite {
            subject: Some(DirectExprMetadata {
                value_type_expr: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("expr")),
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
            owner_name: Some(String::from("build")),
            owner_type_name: None,
            subject_type: Some(String::new()),
            subject_is_awaited: false,
            subject_callee: None,
            subject_name: Some(String::from("expr")),
            subject_member_owner_name: None,
            subject_member_name: None,
            subject_member_through_instance: false,
            subject_method_owner_name: None,
            subject_method_name: None,
            subject_method_through_instance: false,
            cases: vec![MatchCaseSite {
                patterns: vec![MatchPatternSite::Class(String::from("Add"))],
                has_guard: false,
                line: 3,
            }],
            line: 2,
        }]
    );
}

#[test]
fn bind_collects_if_and_assert_guard_sites_from_syntax_tree() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/guards.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![
            SyntaxStatement::If(typepython_syntax::IfStatement {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_syntax::GuardCondition::IsNone {
                    name: String::from("value"),
                    negated: true,
                }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: None,
                false_end_line: None,
            }),
            SyntaxStatement::Assert(typepython_syntax::AssertStatement {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_syntax::GuardCondition::TruthyName {
                    name: String::from("ready"),
                }),
                line: 4,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.if_guards,
        vec![IfGuardSite {
            owner_name: Some(String::from("build")),
            owner_type_name: None,
            guard: Some(GuardConditionSite::IsNone { name: String::from("value"), negated: true }),
            line: 2,
            true_start_line: 3,
            true_end_line: 3,
            false_start_line: None,
            false_end_line: None,
        }]
    );
    assert_eq!(
        table.asserts,
        vec![AssertGuardSite {
            owner_name: Some(String::from("build")),
            owner_type_name: None,
            guard: Some(GuardConditionSite::TruthyName { name: String::from("ready") }),
            line: 4,
        }]
    );
}

#[test]
fn bind_collects_invalidation_sites_from_syntax_tree() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/invalidate.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::Invalidate(typepython_syntax::InvalidationStatement {
            kind: typepython_syntax::InvalidationKind::Delete,
            owner_name: Some(String::from("build")),
            owner_type_name: None,
            names: vec![String::from("value")],
            line: 3,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.invalidations,
        vec![InvalidationSite {
            kind: InvalidationKind::Delete,
            owner_name: Some(String::from("build")),
            owner_type_name: None,
            names: vec![String::from("value")],
            line: 3,
        }]
    );
}

#[test]
fn bind_collects_with_sites_from_syntax_tree() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/with.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::With(typepython_syntax::WithStatement {
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
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.with_statements,
        vec![WithSite {
            context: Some(DirectExprMetadata {
                value_type_expr: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("manager")),
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
        }]
    );
}

#[test]
fn bind_collects_except_handler_sites_from_syntax_tree() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/try.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::ExceptHandler(
            typepython_syntax::ExceptionHandlerStatement {
                exception_type: String::from("ValueError"),
                binding_name: Some(String::from("e")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 4,
                end_line: 5,
            },
        )],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.except_handlers,
        vec![ExceptHandlerSite {
            exception_type: String::from("ValueError"),
            binding_name: Some(String::from("e")),
            owner_name: Some(String::from("build")),
            owner_type_name: None,
            line: 4,
            end_line: 5,
        }]
    );
}

#[test]
fn bind_collects_class_like_member_declarations_with_owner() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/models.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
            name: String::from("SupportsClose"),
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
                    line: 2,
                },
                ClassMember {
                    name: String::from("close"),
                    kind: ClassMemberKind::Method,
                    method_kind: Some(MethodKind::Instance),
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
                    name: String::from("close"),
                    kind: ClassMemberKind::Overload,
                    method_kind: Some(MethodKind::Instance),
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
                    line: 4,
                },
            ],
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.declarations,
        vec![
            Declaration {
                metadata: metadata_class(&[]),
                name: String::from("SupportsClose"),
                kind: DeclarationKind::Class,
                detail: String::new(),
                value_type_expr: None,
                method_kind: None,
                class_kind: Some(DeclarationOwnerKind::Interface),
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
            },
            Declaration {
                metadata: metadata_value(None),
                name: String::from("value"),
                kind: DeclarationKind::Value,
                detail: String::new(),
                value_type_expr: None,
                method_kind: None,
                class_kind: None,
                owner: Some(DeclarationOwner {
                    name: String::from("SupportsClose"),
                    kind: DeclarationOwnerKind::Interface,
                }),
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
            },
            Declaration {
                metadata: metadata_empty_callable(),
                name: String::from("close"),
                kind: DeclarationKind::Function,
                detail: String::from("()->"),
                value_type_expr: None,
                method_kind: Some(MethodKind::Instance),
                class_kind: None,
                owner: Some(DeclarationOwner {
                    name: String::from("SupportsClose"),
                    kind: DeclarationOwnerKind::Interface,
                }),
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
            },
            Declaration {
                metadata: metadata_empty_callable(),
                name: String::from("close"),
                kind: DeclarationKind::Overload,
                detail: String::from("()->"),
                value_type_expr: None,
                method_kind: Some(MethodKind::Instance),
                class_kind: None,
                owner: Some(DeclarationOwner {
                    name: String::from("SupportsClose"),
                    kind: DeclarationOwnerKind::Interface,
                }),
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
            },
        ]
    );
}

#[test]
fn bind_marks_final_values_and_fields() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/finals.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![
            SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("MAX_SIZE")],
                destructuring_target_names: None,
                annotation: Some(String::from("Final")),
                annotation_expr: None,
                value_type_expr: None,
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
                value_list_comprehension: None,
                value_generator_comprehension: None,
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                owner_name: None,
                owner_type_name: None,
                is_final: true,
                is_class_var: false,
                line: 1,
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
                    annotation: None,
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
                    line: 2,
                }],
                line: 2,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.declarations,
        vec![
            Declaration {
                metadata: metadata_value(Some("Final")),
                name: String::from("MAX_SIZE"),
                kind: DeclarationKind::Value,
                detail: String::from("Final"),
                value_type_expr: Some(BoundTypeExpr::new("int")),
                method_kind: None,
                class_kind: None,
                owner: None,
                is_async: false,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: true,
                is_class_var: false,
                bases: Vec::new(),
                type_params: Vec::new(),
            },
            Declaration {
                metadata: metadata_class(&[]),
                name: String::from("Box"),
                kind: DeclarationKind::Class,
                detail: String::new(),
                value_type_expr: None,
                method_kind: None,
                class_kind: Some(DeclarationOwnerKind::Class),
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
            },
            Declaration {
                metadata: metadata_value(None),
                name: String::from("limit"),
                kind: DeclarationKind::Value,
                detail: String::new(),
                value_type_expr: Some(BoundTypeExpr::new("int")),
                method_kind: None,
                class_kind: None,
                owner: Some(DeclarationOwner {
                    name: String::from("Box"),
                    kind: DeclarationOwnerKind::Class,
                }),
                is_async: false,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: true,
                is_class_var: false,
                bases: Vec::new(),
                type_params: Vec::new(),
            },
        ]
    );
}

#[test]
fn bind_marks_classvar_values_and_fields() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/classvars.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![
            SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("VALUE")],
                destructuring_target_names: None,
                annotation: Some(String::from("ClassVar[int]")),
                annotation_expr: None,
                value_type_expr: None,
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
                value_list_comprehension: None,
                value_generator_comprehension: None,
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                owner_name: None,
                owner_type_name: None,
                is_final: false,
                is_class_var: true,
                line: 1,
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
                    annotation: None,
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
                    line: 2,
                }],
                line: 2,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.declarations,
        vec![
            Declaration {
                metadata: metadata_value(Some("ClassVar[int]")),
                name: String::from("VALUE"),
                kind: DeclarationKind::Value,
                detail: String::from("ClassVar[int]"),
                value_type_expr: Some(BoundTypeExpr::new("int")),
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
                is_class_var: true,
                bases: Vec::new(),
                type_params: Vec::new(),
            },
            Declaration {
                metadata: metadata_class(&[]),
                name: String::from("Box"),
                kind: DeclarationKind::Class,
                detail: String::new(),
                value_type_expr: None,
                method_kind: None,
                class_kind: Some(DeclarationOwnerKind::Class),
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
            },
            Declaration {
                metadata: metadata_value(None),
                name: String::from("cache"),
                kind: DeclarationKind::Value,
                detail: String::new(),
                value_type_expr: Some(BoundTypeExpr::new("int")),
                method_kind: None,
                class_kind: None,
                owner: Some(DeclarationOwner {
                    name: String::from("Box"),
                    kind: DeclarationOwnerKind::Class,
                }),
                is_async: false,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: true,
                bases: Vec::new(),
                type_params: Vec::new(),
            },
        ]
    );
}

#[test]
fn bind_marks_override_functions_and_members() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/override.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![
            SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("top_level"),
                type_params: Vec::new(),
                params: Vec::new(),
                returns: None,
                returns_expr: None,
                is_async: false,
                is_override: true,
                is_deprecated: false,
                deprecation_message: None,
                line: 1,
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
                    params: Vec::new(),
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: true,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }],
                line: 2,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.declarations,
        vec![
            Declaration {
                metadata: metadata_empty_callable(),
                name: String::from("top_level"),
                kind: DeclarationKind::Function,
                detail: String::from("()->"),
                value_type_expr: None,
                method_kind: None,
                class_kind: None,
                owner: None,
                is_async: false,
                is_override: true,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: false,
                bases: Vec::new(),
                type_params: Vec::new(),
            },
            Declaration {
                metadata: metadata_class(&["Base"]),
                name: String::from("Child"),
                kind: DeclarationKind::Class,
                detail: String::from("Base"),
                value_type_expr: None,
                method_kind: None,
                class_kind: Some(DeclarationOwnerKind::Class),
                owner: None,
                is_async: false,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: false,
                bases: vec![String::from("Base")],
                type_params: Vec::new(),
            },
            Declaration {
                metadata: metadata_empty_callable(),
                name: String::from("run"),
                kind: DeclarationKind::Function,
                detail: String::from("()->"),
                value_type_expr: None,
                method_kind: Some(MethodKind::Instance),
                class_kind: None,
                owner: Some(DeclarationOwner {
                    name: String::from("Child"),
                    kind: DeclarationOwnerKind::Class,
                }),
                is_async: false,
                is_override: true,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: false,
                bases: Vec::new(),
                type_params: Vec::new(),
            },
        ]
    );
}

#[test]
fn bind_collects_data_class_declarations_with_owner() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/models.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::DataClass(NamedBlockStatement {
            name: String::from("Point"),
            type_params: Vec::new(),
            header_suffix: String::new(),
            bases: Vec::new(),
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: vec![
                ClassMember {
                    name: String::from("x"),
                    kind: ClassMemberKind::Field,
                    method_kind: None,
                    annotation: Some(String::from("float")),
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
                    name: String::from("y"),
                    kind: ClassMemberKind::Field,
                    method_kind: None,
                    annotation: Some(String::from("float")),
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
                    name: String::from("distance"),
                    kind: ClassMemberKind::Method,
                    method_kind: Some(MethodKind::Instance),
                    annotation: None,
                    annotation_expr: None,
                    value_type: None,
                    params: Vec::new(),
                    returns: Some(String::from("float")),
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
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 4);
    assert_eq!(table.declarations[0].name, "Point");
    assert_eq!(table.declarations[0].class_kind, Some(DeclarationOwnerKind::DataClass));
    assert_eq!(table.declarations[1].name, "x");
    assert_eq!(
        table.declarations[1].owner,
        Some(DeclarationOwner {
            name: String::from("Point"),
            kind: DeclarationOwnerKind::DataClass,
        })
    );
    assert_eq!(table.declarations[2].name, "y");
    assert_eq!(
        table.declarations[2].owner,
        Some(DeclarationOwner {
            name: String::from("Point"),
            kind: DeclarationOwnerKind::DataClass,
        })
    );
    assert_eq!(table.declarations[3].name, "distance");
    assert_eq!(table.declarations[3].kind, DeclarationKind::Function);
    assert_eq!(
        table.declarations[3].owner,
        Some(DeclarationOwner {
            name: String::from("Point"),
            kind: DeclarationOwnerKind::DataClass,
        })
    );
}

#[test]
fn bind_collects_sealed_class_declarations_with_owner() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/models.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::SealedClass(NamedBlockStatement {
            name: String::from("Shape"),
            type_params: Vec::new(),
            header_suffix: String::new(),
            bases: Vec::new(),
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: vec![
                ClassMember {
                    name: String::from("sides"),
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
                    name: String::from("area"),
                    kind: ClassMemberKind::Method,
                    method_kind: Some(MethodKind::Instance),
                    annotation: None,
                    annotation_expr: None,
                    value_type: None,
                    params: Vec::new(),
                    returns: Some(String::from("float")),
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
            ],
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 3);
    assert_eq!(table.declarations[0].name, "Shape");
    assert_eq!(table.declarations[0].class_kind, Some(DeclarationOwnerKind::SealedClass));
    assert_eq!(
        table.declarations[1].owner,
        Some(DeclarationOwner {
            name: String::from("Shape"),
            kind: DeclarationOwnerKind::SealedClass,
        })
    );
}

#[test]
fn bind_marks_abstract_methods() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/models.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
            name: String::from("Readable"),
            type_params: Vec::new(),
            header_suffix: String::new(),
            bases: Vec::new(),
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: vec![ClassMember {
                name: String::from("read"),
                kind: ClassMemberKind::Method,
                method_kind: Some(MethodKind::Instance),
                annotation: None,
                annotation_expr: None,
                value_type: None,
                params: Vec::new(),
                returns: Some(String::from("bytes")),
                returns_expr: None,
                is_async: false,
                is_override: false,
                is_abstract_method: true,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: false,
                line: 2,
            }],
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 2);
    assert!(table.declarations[1].is_abstract_method);
    assert_eq!(table.declarations[1].name, "read");
}

#[test]
fn bind_marks_deprecated_declarations() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/deprecated.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![
            SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("old_func"),
                type_params: Vec::new(),
                params: Vec::new(),
                returns: None,
                returns_expr: None,
                is_async: false,
                is_override: false,
                is_deprecated: true,
                deprecation_message: Some(String::from("use new_func")),
                line: 1,
            }),
            SyntaxStatement::OverloadDef(FunctionStatement {
                name: String::from("old_func"),
                type_params: Vec::new(),
                params: Vec::new(),
                returns: None,
                returns_expr: None,
                is_async: false,
                is_override: false,
                is_deprecated: true,
                deprecation_message: None,
                line: 2,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 2);
    assert!(table.declarations[0].is_deprecated);
    assert_eq!(table.declarations[0].deprecation_message, Some(String::from("use new_func")));
    assert!(table.declarations[1].is_deprecated);
    assert_eq!(table.declarations[1].deprecation_message, None);
    assert_eq!(table.declarations[1].kind, DeclarationKind::Overload);
}

#[test]
fn bind_marks_final_decorator_on_class() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/finals.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::ClassDef(NamedBlockStatement {
            name: String::from("Singleton"),
            type_params: Vec::new(),
            header_suffix: String::new(),
            bases: Vec::new(),
            is_final_decorator: true,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: Vec::new(),
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 1);
    assert!(table.declarations[0].is_final_decorator);
    assert_eq!(table.declarations[0].name, "Singleton");
}

#[test]
fn bind_collects_generic_type_params_with_bounds_and_constraints() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/__init__.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
            name: String::from("Sorted"),
            type_params: vec![
                TypeParam {
                    name: String::from("T"),
                    kind: TypeParamKind::TypeVar,
                    bound: Some(String::from("Comparable")),
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                },
                TypeParam {
                    name: String::from("U"),
                    kind: TypeParamKind::TypeVar,
                    bound: None,
                    constraints: vec![String::from("int"), String::from("str")],
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                },
                TypeParam {
                    name: String::from("V"),
                    kind: TypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: Some(String::from("str")),
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                },
            ],
            value: String::from("list[T]"),
            value_expr: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 1);
    assert_eq!(
        table.declarations[0].type_params,
        vec![
            GenericTypeParam {
                name: String::from("T"),
                kind: GenericTypeParamKind::TypeVar,
                bound: Some(String::from("Comparable")),
                constraints: Vec::new(),
                default: None,
                bound_expr: None,
                constraint_exprs: Vec::new(),
                default_expr: None,
            },
            GenericTypeParam {
                name: String::from("U"),
                kind: GenericTypeParamKind::TypeVar,
                bound: None,
                constraints: vec![String::from("int"), String::from("str")],
                default: None,
                bound_expr: None,
                constraint_exprs: Vec::new(),
                default_expr: None,
            },
            GenericTypeParam {
                name: String::from("V"),
                kind: GenericTypeParamKind::TypeVar,
                bound: None,
                constraints: Vec::new(),
                default: Some(String::from("str")),
                bound_expr: None,
                constraint_exprs: Vec::new(),
                default_expr: None,
            },
        ]
    );
}

#[test]
fn bind_collects_paramspec_type_params() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/__init__.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::FunctionDef(FunctionStatement {
            name: String::from("decorator"),
            type_params: vec![TypeParam {
                name: String::from("P"),
                kind: TypeParamKind::ParamSpec,
                bound: None,
                constraints: Vec::new(),
                default: None,
                bound_expr: None,
                constraint_exprs: Vec::new(),
                default_expr: None,
            }],
            params: Vec::new(),
            returns: None,
            returns_expr: None,
            is_async: false,
            is_override: false,
            is_deprecated: false,
            deprecation_message: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 1);
    assert_eq!(
        table.declarations[0].type_params,
        vec![GenericTypeParam {
            name: String::from("P"),
            kind: GenericTypeParamKind::ParamSpec,
            bound: None,
            constraints: Vec::new(),
            default: None,
            bound_expr: None,
            constraint_exprs: Vec::new(),
            default_expr: None,
        }]
    );
}

#[test]
fn bind_collects_typevartuple_type_params() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/__init__.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
            name: String::from("Pack"),
            type_params: vec![TypeParam {
                name: String::from("Ts"),
                kind: TypeParamKind::TypeVarTuple,
                bound: None,
                constraints: Vec::new(),
                default: None,
                bound_expr: None,
                constraint_exprs: Vec::new(),
                default_expr: None,
            }],
            value: String::from("tuple[Unpack[Ts]]"),
            value_expr: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 1);
    assert_eq!(
        table.declarations[0].type_params,
        vec![GenericTypeParam {
            name: String::from("Ts"),
            kind: GenericTypeParamKind::TypeVarTuple,
            bound: None,
            constraints: Vec::new(),
            default: None,
            bound_expr: None,
            constraint_exprs: Vec::new(),
            default_expr: None,
        }]
    );
}

#[test]
fn bind_formats_signature_with_positional_only_params() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/funcs.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::FunctionDef(FunctionStatement {
            name: String::from("func"),
            type_params: Vec::new(),
            params: vec![
                FunctionParam {
                    name: String::from("x"),
                    annotation: Some(String::from("int")),
                    annotation_expr: None,
                    has_default: false,
                    positional_only: true,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                },
                FunctionParam {
                    name: String::from("y"),
                    annotation: Some(String::from("int")),
                    annotation_expr: None,
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                },
            ],
            returns: Some(String::from("int")),
            returns_expr: None,
            is_async: false,
            is_override: false,
            is_deprecated: false,
            deprecation_message: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 1);
    assert_eq!(table.declarations[0].detail, "(x:int,/,y:int)->int");
}

#[test]
fn bind_formats_signature_with_keyword_only_params() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/funcs.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::FunctionDef(FunctionStatement {
            name: String::from("func"),
            type_params: Vec::new(),
            params: vec![
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
                    has_default: true,
                    positional_only: false,
                    keyword_only: true,
                    variadic: false,
                    keyword_variadic: false,
                },
            ],
            returns: None,
            returns_expr: None,
            is_async: false,
            is_override: false,
            is_deprecated: false,
            deprecation_message: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 1);
    assert_eq!(table.declarations[0].detail, "(x:int,*,y:str=)->");
}

#[test]
fn bind_formats_signature_with_variadic_and_keyword_variadic() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/funcs.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::FunctionDef(FunctionStatement {
            name: String::from("func"),
            type_params: Vec::new(),
            params: vec![
                FunctionParam {
                    name: String::from("args"),
                    annotation: Some(String::from("int")),
                    annotation_expr: None,
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: true,
                    keyword_variadic: false,
                },
                FunctionParam {
                    name: String::from("kwargs"),
                    annotation: Some(String::from("str")),
                    annotation_expr: None,
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: true,
                },
            ],
            returns: Some(String::from("None")),
            returns_expr: None,
            is_async: false,
            is_override: false,
            is_deprecated: false,
            deprecation_message: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 1);
    assert_eq!(table.declarations[0].detail, "(*args:int,**kwargs:str)->None");
}

#[test]
fn bind_collects_method_kinds_static_and_class() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/models.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::ClassDef(NamedBlockStatement {
            name: String::from("Util"),
            type_params: Vec::new(),
            header_suffix: String::new(),
            bases: Vec::new(),
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: vec![
                ClassMember {
                    name: String::from("create"),
                    kind: ClassMemberKind::Method,
                    method_kind: Some(MethodKind::Static),
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
                    line: 2,
                },
                ClassMember {
                    name: String::from("from_json"),
                    kind: ClassMemberKind::Method,
                    method_kind: Some(MethodKind::Class),
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
            ],
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 3);
    assert_eq!(table.declarations[1].name, "create");
    assert_eq!(table.declarations[1].method_kind, Some(MethodKind::Static));
    assert_eq!(table.declarations[2].name, "from_json");
    assert_eq!(table.declarations[2].method_kind, Some(MethodKind::Class));
}

#[test]
fn bind_collects_isinstance_guard_with_multiple_types() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/guards.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::If(typepython_syntax::IfStatement {
            owner_name: Some(String::from("check")),
            owner_type_name: None,
            guard: Some(typepython_syntax::GuardCondition::IsInstance {
                name: String::from("x"),
                types: vec![String::from("int"), String::from("str")],
            }),
            line: 1,
            true_start_line: 2,
            true_end_line: 2,
            false_start_line: None,
            false_end_line: None,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.if_guards,
        vec![IfGuardSite {
            owner_name: Some(String::from("check")),
            owner_type_name: None,
            guard: Some(GuardConditionSite::IsInstance {
                name: String::from("x"),
                types: vec![String::from("int"), String::from("str")],
            }),
            line: 1,
            true_start_line: 2,
            true_end_line: 2,
            false_start_line: None,
            false_end_line: None,
        }]
    );
}

#[test]
fn bind_collects_predicate_call_guard() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/guards.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::If(typepython_syntax::IfStatement {
            owner_name: Some(String::from("validate")),
            owner_type_name: None,
            guard: Some(typepython_syntax::GuardCondition::PredicateCall {
                name: String::from("x"),
                callee: String::from("is_valid"),
            }),
            line: 1,
            true_start_line: 2,
            true_end_line: 2,
            false_start_line: None,
            false_end_line: None,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.if_guards,
        vec![IfGuardSite {
            owner_name: Some(String::from("validate")),
            owner_type_name: None,
            guard: Some(GuardConditionSite::PredicateCall {
                name: String::from("x"),
                callee: String::from("is_valid"),
            }),
            line: 1,
            true_start_line: 2,
            true_end_line: 2,
            false_start_line: None,
            false_end_line: None,
        }]
    );
}

#[test]
fn bind_collects_composite_and_or_guards() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/guards.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::If(typepython_syntax::IfStatement {
            owner_name: Some(String::from("check")),
            owner_type_name: None,
            guard: Some(typepython_syntax::GuardCondition::And(vec![
                typepython_syntax::GuardCondition::IsNone {
                    name: String::from("a"),
                    negated: true,
                },
                typepython_syntax::GuardCondition::TruthyName { name: String::from("b") },
            ])),
            line: 1,
            true_start_line: 2,
            true_end_line: 2,
            false_start_line: None,
            false_end_line: None,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.if_guards,
        vec![IfGuardSite {
            owner_name: Some(String::from("check")),
            owner_type_name: None,
            guard: Some(GuardConditionSite::And(vec![
                GuardConditionSite::IsNone { name: String::from("a"), negated: true },
                GuardConditionSite::TruthyName { name: String::from("b") },
            ])),
            line: 1,
            true_start_line: 2,
            true_end_line: 2,
            false_start_line: None,
            false_end_line: None,
        }]
    );
}

#[test]
fn bind_collects_invalidation_rebind_like() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/invalidate.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::Invalidate(typepython_syntax::InvalidationStatement {
            kind: typepython_syntax::InvalidationKind::RebindLike,
            owner_name: Some(String::from("update")),
            owner_type_name: None,
            names: vec![String::from("count")],
            line: 2,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.invalidations,
        vec![InvalidationSite {
            kind: InvalidationKind::RebindLike,
            owner_name: Some(String::from("update")),
            owner_type_name: None,
            names: vec![String::from("count")],
            line: 2,
        }]
    );
}

#[test]
fn bind_collects_invalidation_scope_change() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/invalidate.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::Invalidate(typepython_syntax::InvalidationStatement {
            kind: typepython_syntax::InvalidationKind::ScopeChange,
            owner_name: Some(String::from("handler")),
            owner_type_name: None,
            names: vec![String::from("state"), String::from("flag")],
            line: 5,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(
        table.invalidations,
        vec![InvalidationSite {
            kind: InvalidationKind::ScopeChange,
            owner_name: Some(String::from("handler")),
            owner_type_name: None,
            names: vec![String::from("state"), String::from("flag")],
            line: 5,
        }]
    );
}

#[test]
fn bind_excludes_rebind_like_value_from_declarations() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/helpers.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::Value(ValueStatement {
            names: vec![String::from("count")],
            destructuring_target_names: None,
            annotation: None,
            annotation_expr: None,
            value_type_expr: None,
            value_type: None,
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
            value_binop_left: Some(Box::new(DirectExprMetadata {
                value_type_expr: None,
                value_type: None,
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("count")),
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
            value_binop_right: None,
            value_binop_operator: Some(String::from("+")),
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
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(table.declarations.is_empty(), "rebind-like update should not appear in declarations");
    assert_eq!(table.assignments.len(), 1);
    assert_eq!(table.assignments[0].name, "count");
    assert_eq!(table.assignments[0].value_binop_operator, Some(String::from("+")));
}

#[test]
fn bind_collects_match_literal_and_unsupported_patterns() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/match.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::Match(typepython_syntax::MatchStatement {
            owner_name: Some(String::from("route")),
            owner_type_name: None,
            subject_type: None,
            subject_is_awaited: false,
            subject_callee: None,
            subject_name: Some(String::from("code")),
            subject_member_owner_name: None,
            subject_member_name: None,
            subject_member_through_instance: false,
            subject_method_owner_name: None,
            subject_method_name: None,
            subject_method_through_instance: false,
            cases: vec![
                typepython_syntax::MatchCaseStatement {
                    patterns: vec![typepython_syntax::MatchPattern::Literal(String::from("1"))],
                    has_guard: false,
                    line: 2,
                },
                typepython_syntax::MatchCaseStatement {
                    patterns: vec![typepython_syntax::MatchPattern::Unsupported],
                    has_guard: false,
                    line: 3,
                },
            ],
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.matches.len(), 1);
    assert_eq!(
        table.matches[0].cases,
        vec![
            MatchCaseSite {
                patterns: vec![MatchPatternSite::Literal(String::from("1"))],
                has_guard: false,
                line: 2,
            },
            MatchCaseSite {
                patterns: vec![MatchPatternSite::Unsupported],
                has_guard: false,
                line: 3,
            },
        ]
    );
}

#[test]
fn bind_collects_class_with_multiple_bases() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/models.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::ClassDef(NamedBlockStatement {
            name: String::from("Widget"),
            type_params: Vec::new(),
            header_suffix: String::from("(Base1, Base2, Mixin)"),
            bases: vec![String::from("Base1"), String::from("Base2"), String::from("Mixin")],
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: Vec::new(),
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.declarations.len(), 1);
    assert_eq!(
        table.declarations[0].bases,
        vec![String::from("Base1"), String::from("Base2"), String::from("Mixin"),]
    );
    assert_eq!(table.declarations[0].detail, "Base1,Base2,Mixin");
}

#[test]
fn bind_collects_yield_from_sites() {
    let table = bind(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("src/app/gen.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::new(),
        },
        statements: vec![SyntaxStatement::Yield(typepython_syntax::YieldStatement {
            owner_name: String::from("delegate"),
            owner_type_name: None,
            value_type: Some(String::from("list[int]")),
            value_callee: None,
            value_name: Some(String::from("items")),
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
            line: 2,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert_eq!(table.yields.len(), 1);
    assert!(table.yields[0].is_yield_from);
    assert_eq!(table.yields[0].owner_name, "delegate");
    assert_eq!(table.yields[0].value_name, Some(String::from("items")));
}

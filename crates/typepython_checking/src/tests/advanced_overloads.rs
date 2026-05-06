use super::*;

#[test]
fn check_accepts_overload_sets_with_one_implementation() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    metadata: Default::default(),
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
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    metadata: Default::default(),
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
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    metadata: Default::default(),
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
            ],
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_overloads_without_concrete_implementation() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    metadata: Default::default(),
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
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    metadata: Default::default(),
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
            ],
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4004"));
    assert!(rendered.contains("without a concrete implementation"));
}

#[test]
fn check_reports_overloads_with_multiple_concrete_implementations() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    metadata: Default::default(),
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
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    metadata: Default::default(),
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
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    metadata: Default::default(),
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
            ],
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4004"));
    assert!(rendered.contains("more than one concrete implementation"));
}

#[test]
fn check_reports_ambiguous_overload_resolution() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    metadata: callable_metadata("(value:int)->int"),
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
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    metadata: callable_metadata("(value:int)->str"),
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
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    metadata: callable_metadata("(value:int)->int"),
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
            ],
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: vec![typepython_binding::CallSite {
                callee: String::from("parse"),
                arg_count: 1,
                arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
                    String::from("int"),
                ]),
                starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                    Vec::new(),
                ),
                keyword_names: Vec::new(),
                keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                    Vec::new(),
                ),
                keyword_expansion_values:
                    typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                line: 1,
            }],
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4012"));
    assert!(rendered.contains("ambiguous across 2 overloads"));
}

#[test]
fn check_accepts_direct_overloaded_call_assignment_type_match() {
    let result = check_temp_typepython_source(
        "overload def parse(value: int) -> str: ...\noverload def parse(value: str) -> int: ...\ndef parse(value):\n    return value\n\nresult: str = parse(1)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_direct_overloaded_none_argument_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        concat!(
            "overload def parse(value: int) -> str: ...\n",
            "def parse(value):\n",
            "    return 1\n\n",
            "result: str = parse(None)\n",
        ),
        ParseOptions::default(),
        crate::CheckerOptions { strict_nulls: false, ..crate::CheckerOptions::default() },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn method_overload_selection_honors_strict_nulls_option() {
    let mut node = type_relation_node_with_base_child();
    node.declarations.push(declaration! {
        name: String::from("parse"),
        kind: DeclarationKind::Overload,
        metadata: callable_metadata("(self,value:int)->str"),
        value_type_expr: None,
        method_kind: Some(typepython_syntax::MethodKind::Instance),
        class_kind: None,
        owner: Some(DeclarationOwner {
            kind: DeclarationOwnerKind::Class,
            name: String::from("Base"),
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
    });
    let graph = normalize_test_graph(&ModuleGraph { nodes: vec![node] });
    let node = &graph.nodes[0];
    let overloads = node
        .declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Overload)
        .map(|declaration| (declaration, None))
        .collect::<Vec<_>>();
    let call = typepython_binding::CallSite {
        callee: String::from("Base.parse"),
        arg_count: 1,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("None"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };
    let selection = crate::resolve_method_overload_selection(
        node,
        &graph.nodes,
        &call,
        &crate::SemanticType::Name(String::from("Base")),
        &overloads,
        crate::AssignabilityOptions {
            strict_nulls: false,
            ..crate::AssignabilityOptions::default()
        },
    );

    match selection {
        crate::ResolvedOverloadSelection::Selected(candidate) => {
            assert_eq!(
                candidate.return_type.as_ref().map(crate::diagnostic_type_text),
                Some(String::from("str"))
            );
        }
        other => panic!("expected strict-null-disabled method overload selection: {other:?}"),
    }
}

#[test]
fn check_rejects_tainted_overloaded_call_argument_when_taint_is_enabled() {
    let result = check_temp_typepython_source_with_checker_options(
        concat!(
            "overload def render(value: str) -> int: ...\n",
            "def render(value) -> str:\n",
            "    return \"unsafe\"\n\n",
            "raw: Tainted[str, \"html\"]\n",
            "result: int = render(raw)\n",
        ),
        ParseOptions::default(),
        crate::CheckerOptions { experimental_taint: true, ..crate::CheckerOptions::default() },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("assigns `str` where `result` expects `int`"), "{rendered}");
}

#[test]
fn check_accepts_direct_overloaded_call_return_type_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    metadata: callable_metadata("(value:int)->str"),
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
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    metadata: callable_metadata("(value:str)->int"),
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
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    metadata: callable_metadata("(value:int)->int"),
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
                declaration! {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    metadata: callable_metadata("()->str"),
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
            ],
            calls: vec![typepython_binding::CallSite {
                callee: String::from("parse"),
                arg_count: 1,
                arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
                    String::from("int"),
                ]),
                starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                    Vec::new(),
                ),
                keyword_names: Vec::new(),
                keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                    Vec::new(),
                ),
                keyword_expansion_values:
                    typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                line: 1,
            }],
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value: None,
                is_awaited: false,
                value_callee: Some(String::from("parse")),
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
                line: 1,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
        }],
    });

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_imported_overloaded_call_assignment_type_match() {
    let result =
        check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("/tmp/pkg/util.pyi"),
                    module_key: String::from("pkg.util"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![
                        declaration! {
                            name: String::from("parse"),
                            kind: DeclarationKind::Overload,
                            metadata: callable_metadata("(value:int)->str"),
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
                        declaration! {
                            name: String::from("parse"),
                            kind: DeclarationKind::Overload,
                            metadata: callable_metadata("(value:str)->int"),
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
                    ],
                    member_accesses: Vec::new(),
                    returns: Vec::new(),
                    yields: Vec::new(),
                    if_guards: Vec::new(),
                    asserts: Vec::new(),
                    invalidations: Vec::new(),
                    matches: Vec::new(),
                    for_loops: Vec::new(),
                    with_statements: Vec::new(),
                    except_handlers: Vec::new(),
                    assignments: Vec::new(),
                    summary_fingerprint: 1,
                    calls: Vec::new(),
                    method_calls: Vec::new(),
                },
                ModuleNode {
                    module_path: PathBuf::from("/tmp/app.tpy"),
                    module_key: String::from("app"),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![
                        declaration! {
                            name: String::from("parse"),
                            kind: DeclarationKind::Import,
                            metadata: import_metadata("pkg.util.parse"),
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
                        declaration! {
                            name: String::from("result"),
                            kind: DeclarationKind::Value,
                            metadata: value_metadata("str"),
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
                    ],
                    member_accesses: Vec::new(),
                    returns: Vec::new(),
                    yields: Vec::new(),
                    if_guards: Vec::new(),
                    asserts: Vec::new(),
                    invalidations: Vec::new(),
                    matches: Vec::new(),
                    for_loops: Vec::new(),
                    with_statements: Vec::new(),
                    except_handlers: Vec::new(),
                    assignments: vec![typepython_binding::AssignmentSite {
                        name: String::from("result"),
                        destructuring_target_names: None,
                        destructuring_index: None,
                        annotation: Some(String::from("str")),
                        annotation_expr: None,
                        is_awaited: false,
                        value_callee: Some(String::from("parse")),
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
                        value: None,
                        owner_name: None,
                        owner_type_name: None,
                        line: 1,
                    }],
                    summary_fingerprint: 1,
                    calls: vec![typepython_binding::CallSite {
                        callee: String::from("parse"),
                        arg_count: 1,
                        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                            vec![String::from("int")],
                        ),
                        starred_arg_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        keyword_names: Vec::new(),
                        keyword_arg_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        keyword_expansion_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        line: 1,
                    }],
                    method_calls: Vec::new(),
                },
            ],
        });

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_imported_generic_callable_resolution_through_module_member() {
    let result = check_temp_project_sources(&[
        (
            "helpers.tpy",
            "helpers",
            SourceKind::TypePython,
            "def box_value[T](value: T) -> list[T]:\n    return [value]\n",
        ),
        (
            "app.tpy",
            "app",
            SourceKind::TypePython,
            "import helpers\n\nvalue: list[int] = helpers.box_value(1)\n",
        ),
    ]);

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn imported_module_method_return_semantic_type_stays_semantic() {
    let graph =
        ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("/tmp/helpers.pyi"),
                    module_key: String::from("helpers"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![declaration! {
                        name: String::from("box_value"),
                        kind: DeclarationKind::Function,
                        metadata: typepython_binding::DeclarationMetadata::Callable {
                            signature: typepython_binding::BoundCallableSignature {
                                params: vec![typepython_syntax::DirectFunctionParamSite {
                                    name: String::from("value"),
                                    annotation: Some(String::from("int")),
                                    annotation_expr: Some(typepython_syntax::TypeExpr::Name(
                                        String::from("int"),
                                    )),
                                    has_default: false,
                                    positional_only: false,
                                    keyword_only: false,
                                    variadic: false,
                                    keyword_variadic: false,
                                }],
                                returns: Some(typepython_binding::BoundTypeExpr::new("list[int]")),
                            },
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
                    }],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
                    member_accesses: Vec::new(),
                    returns: Vec::new(),
                    yields: Vec::new(),
                    if_guards: Vec::new(),
                    asserts: Vec::new(),
                    invalidations: Vec::new(),
                    matches: Vec::new(),
                    for_loops: Vec::new(),
                    with_statements: Vec::new(),
                    except_handlers: Vec::new(),
                    assignments: Vec::new(),
                    summary_fingerprint: 1,
                },
                ModuleNode {
                    module_path: PathBuf::from("/tmp/app.tpy"),
                    module_key: String::from("app"),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![declaration! {
                        name: String::from("helpers"),
                        kind: DeclarationKind::Import,
                        metadata: typepython_binding::DeclarationMetadata::Import {
                            target: typepython_binding::BoundImportTarget::new("helpers"),
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
                    }],
                    calls: Vec::new(),
                    method_calls: vec![typepython_binding::MethodCallSite {
                        current_owner_name: None,
                        current_owner_type_name: None,
                        owner_name: String::from("helpers"),
                        method: String::from("box_value"),
                        through_instance: false,
                        arg_count: 1,
                        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                            vec![String::from("int")],
                        ),
                        starred_arg_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        keyword_names: Vec::new(),
                        keyword_arg_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        keyword_expansion_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        line: 1,
                    }],
                    member_accesses: Vec::new(),
                    returns: Vec::new(),
                    yields: Vec::new(),
                    if_guards: Vec::new(),
                    asserts: Vec::new(),
                    invalidations: Vec::new(),
                    matches: Vec::new(),
                    for_loops: Vec::new(),
                    with_statements: Vec::new(),
                    except_handlers: Vec::new(),
                    assignments: Vec::new(),
                    summary_fingerprint: 1,
                },
            ],
        };

    assert_eq!(
        crate::resolve_imported_module_method_return_semantic_type(
            &graph.nodes[1],
            &graph.nodes,
            1,
            "helpers",
            "box_value",
        )
        .as_ref()
        .map(crate::diagnostic_type_text),
        Some(String::from("list[int]")),
    );
}

#[test]
fn check_reports_non_applicable_overload_as_call_incompatibility() {
    let result = check_temp_typepython_source(
        "overload def parse(value: int) -> str: ...\noverload def parse(value: str) -> int: ...\ndef parse(value: int) -> str:\n    return \"x\"\n\nparse(None)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(!rendered.contains("TPY4012"));
}

#[test]
fn overload_applicability_accepts_keyword_default_and_semantic_match() {
    let call = typepython_binding::CallSite {
        callee: String::from("parse"),
        arg_count: 0,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: vec![String::from("value")],
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("None"),
        ]),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };
    let declaration = declaration! {
        metadata: callable_metadata("(value:Optional[int]=)->int"),
        name: String::from("parse"),
        kind: DeclarationKind::Overload,
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
    let declaration = normalize_test_declaration(&declaration);

    assert!(crate::overload_is_applicable(&call, &declaration));
}

#[test]
fn overload_applicability_rejects_positional_only_keyword() {
    let call = typepython_binding::CallSite {
        callee: String::from("parse"),
        arg_count: 0,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: vec![String::from("value")],
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("int"),
        ]),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };
    let declaration = declaration! {
        metadata: callable_metadata("(value:int,/)->int"),
        name: String::from("parse"),
        kind: DeclarationKind::Overload,
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

    assert!(!crate::overload_is_applicable(&call, &declaration));
}

#[test]
fn overload_applicability_accepts_variadic_arguments() {
    let call = typepython_binding::CallSite {
        callee: String::from("parse"),
        arg_count: 3,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("int"),
            String::from("int"),
            String::from("int"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };
    let declaration = declaration! {
        metadata: callable_metadata("(*args:int)->int"),
        name: String::from("parse"),
        kind: DeclarationKind::Overload,
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
    let declaration = normalize_test_declaration(&declaration);

    assert!(crate::overload_is_applicable(&call, &declaration));
}

#[test]
fn overload_applicability_accepts_nominal_subclass_arguments() {
    let call = typepython_binding::CallSite {
        callee: String::from("parse"),
        arg_count: 1,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("Child"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };
    let declaration = declaration! {
        metadata: callable_metadata("(value:Base)->int"),
        name: String::from("parse"),
        kind: DeclarationKind::Overload,
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
    let declaration = normalize_test_declaration(&declaration);

    let node = typepython_graph::ModuleNode {
        module_path: PathBuf::from("src/app/module.tpy"),
        module_key: String::new(),
        module_kind: SourceKind::TypePython,
        declarations: vec![
            declaration! {
                name: String::from("Base"),
                kind: DeclarationKind::Class,
                metadata: Default::default(),
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
            declaration! {
                name: String::from("Child"),
                kind: DeclarationKind::Class,
                metadata: Default::default(),
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
        ],
        member_accesses: Vec::new(),
        returns: Vec::new(),
        yields: Vec::new(),
        if_guards: Vec::new(),
        asserts: Vec::new(),
        invalidations: Vec::new(),
        matches: Vec::new(),
        for_loops: Vec::new(),
        with_statements: Vec::new(),
        except_handlers: Vec::new(),
        assignments: Vec::new(),
        summary_fingerprint: 1,
        calls: Vec::new(),
        method_calls: Vec::new(),
    };

    assert!(crate::overload_is_applicable_with_context(
        &node,
        std::slice::from_ref(&node),
        &call,
        &declaration
    ));
}

#[test]
fn overload_applicability_accepts_list_for_sequence_parameter() {
    let call = typepython_binding::CallSite {
        callee: String::from("parse"),
        arg_count: 1,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("list[int]"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };
    let declaration = declaration! {
        metadata: callable_metadata("(value:Sequence[int])->int"),
        name: String::from("parse"),
        kind: DeclarationKind::Overload,
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
    let declaration = normalize_test_declaration(&declaration);

    let node = typepython_graph::ModuleNode {
        module_path: PathBuf::from("src/app/module.tpy"),
        module_key: String::new(),
        module_kind: SourceKind::TypePython,
        declarations: Vec::new(),
        member_accesses: Vec::new(),
        returns: Vec::new(),
        yields: Vec::new(),
        if_guards: Vec::new(),
        asserts: Vec::new(),
        invalidations: Vec::new(),
        matches: Vec::new(),
        for_loops: Vec::new(),
        with_statements: Vec::new(),
        except_handlers: Vec::new(),
        assignments: Vec::new(),
        summary_fingerprint: 1,
        calls: Vec::new(),
        method_calls: Vec::new(),
    };

    assert!(crate::overload_is_applicable_with_context(
        &node,
        std::slice::from_ref(&node),
        &call,
        &declaration
    ));
}

#[test]
fn overload_applicability_uses_contextual_lambda_callable_types() {
    fn direct_expr(value_type: &str) -> typepython_syntax::DirectExprMetadata {
        typepython_syntax::DirectExprMetadata::from_type_text(value_type)
    }

    let lambda_arg = typepython_syntax::DirectExprMetadata {
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
        value_lambda: Some(Box::new(typepython_syntax::LambdaMetadata {
            params: vec![typepython_syntax::FunctionParam {
                name: String::from("x"),
                annotation: None,
                annotation_expr: None,
                has_default: false,
                positional_only: false,
                keyword_only: false,
                variadic: false,
                keyword_variadic: false,
            }],
            body: Box::new(direct_expr("str")),
        })),
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None,
    };
    let call = typepython_binding::CallSite {
        callee: String::from("choose"),
        arg_count: 1,
        arg_values: vec![lambda_arg],
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };
    let str_declaration = declaration! {
        metadata: callable_metadata("(fn:Callable[[int],str])->str"),
        name: String::from("choose"),
        kind: DeclarationKind::Overload,
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
    let str_declaration = normalize_test_declaration(&str_declaration);
    let int_declaration = declaration! {
        metadata: callable_metadata("(fn:Callable[[int],int])->int"),
        ..str_declaration.clone()
    };
    let int_declaration = normalize_test_declaration(&int_declaration);

    assert!(crate::overload_is_applicable(&call, &str_declaration));
    assert!(!crate::overload_is_applicable(&call, &int_declaration));
}

#[test]
fn overload_specificity_uses_instantiated_generic_candidate() {
    let generic_overload = declaration! {
        metadata: callable_metadata("(value:T)->tuple[T]"),
        name: String::from("wrap"),
        kind: DeclarationKind::Overload,
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
        type_params: vec![typepython_binding::GenericTypeParam {
            kind: typepython_binding::GenericTypeParamKind::TypeVar,
            name: String::from("T"),
            bound_expr: None,
            constraint_exprs: Vec::new(),
            default_expr: None,
        }],
    };
    let generic_overload = normalize_test_declaration(&generic_overload);
    let object_overload = declaration! {
        metadata: callable_metadata("(value:object)->tuple[object]"),
        name: String::from("wrap"),
        kind: DeclarationKind::Overload,
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
    let object_overload = normalize_test_declaration(&object_overload);
    let node = ModuleNode {
        module_path: PathBuf::from("<generic-overload>"),
        module_key: String::new(),
        module_kind: SourceKind::TypePython,
        declarations: vec![generic_overload.clone(), object_overload.clone()],
        member_accesses: Vec::new(),
        returns: Vec::new(),
        yields: Vec::new(),
        if_guards: Vec::new(),
        asserts: Vec::new(),
        invalidations: Vec::new(),
        matches: Vec::new(),
        for_loops: Vec::new(),
        with_statements: Vec::new(),
        except_handlers: Vec::new(),
        assignments: Vec::new(),
        summary_fingerprint: 0,
        calls: Vec::new(),
        method_calls: Vec::new(),
    };
    let call = typepython_binding::CallSite {
        callee: String::from("wrap"),
        arg_count: 1,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("int"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };
    let overloads = vec![&generic_overload, &object_overload];

    let selected = match crate::resolve_direct_overload_selection(
        &node,
        &[],
        &call,
        &overloads,
        crate::AssignabilityOptions::default(),
    ) {
        crate::ResolvedOverloadSelection::Selected(candidate) => candidate,
        _ => panic!("selected overload"),
    };

    assert!(std::ptr::eq(selected.declaration, overloads[0]));
    assert_eq!(
        selected
            .signature_sites
            .iter()
            .map(|param| param.annotation.as_deref().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["int"],
    );
    assert_eq!(
        selected.return_type.as_ref().map(crate::render_semantic_type),
        Some(String::from("tuple[int]")),
    );
}

#[test]
fn declaration_semantic_facts_use_shared_cache_and_typestore_ids() {
    let function = declaration! {
        metadata: callable_metadata("(value:int)->tuple[int, str]"),
        name: String::from("build_pair"),
        kind: DeclarationKind::Function,
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
    let function = normalize_test_declaration(&function);
    let alias = declaration! {
        metadata: type_alias_metadata("tuple[int, str]"),
        name: String::from("Pair"),
        kind: DeclarationKind::TypeAlias,
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
    let alias = normalize_test_declaration(&alias);
    let value = declaration! {
        metadata: value_metadata("tuple[int, str]"),
        name: String::from("pair"),
        kind: DeclarationKind::Value,
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
    let value = normalize_test_declaration(&value);

    let function_ids = crate::declaration_semantic_type_ids(&function);
    let function_clone_ids = crate::declaration_semantic_type_ids(&function.clone());
    let alias_ids = crate::declaration_semantic_type_ids(&alias);
    let value_ids = crate::declaration_semantic_type_ids(&value);

    assert_eq!(function_ids, function_clone_ids);
    assert_eq!(function_ids.callable_return, alias_ids.type_alias_body);
    assert_eq!(function_ids.callable_return, value_ids.value_annotation);
    assert_eq!(
        crate::declaration_callable_semantics(&function)
            .and_then(|callable| callable.return_type)
            .as_ref()
            .map(crate::render_semantic_type),
        Some(String::from("tuple[int, str]"))
    );
}

#[test]
fn declaration_semantics_prefer_structured_metadata_over_legacy_detail_text() {
    let list_of_int = typepython_syntax::TypeExpr::Generic {
        head: String::from("list"),
        args: vec![typepython_syntax::TypeExpr::Name(String::from("int"))],
    };
    let tuple_of_int = typepython_syntax::TypeExpr::Generic {
        head: String::from("tuple"),
        args: vec![typepython_syntax::TypeExpr::Name(String::from("int"))],
    };

    let value = declaration! {
        metadata: typepython_binding::DeclarationMetadata::Value {
            annotation: Some(typepython_binding::BoundTypeExpr::from_expr(list_of_int.clone())),
        },
        name: String::from("items"),
        kind: DeclarationKind::Value,
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
        crate::declaration_value_annotation_semantic_type(&value)
            .map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("list[int]")),
    );

    let alias = declaration! {
        metadata: typepython_binding::DeclarationMetadata::TypeAlias {
            value: typepython_binding::BoundTypeExpr::from_expr(
                typepython_syntax::TypeExpr::Union {
                    branches: vec![
                        typepython_syntax::TypeExpr::Name(String::from("int")),
                        typepython_syntax::TypeExpr::Name(String::from("None")),
                    ],
                    style: typepython_syntax::UnionStyle::Shorthand,
                },
            ),
        },
        name: String::from("MaybeInt"),
        kind: DeclarationKind::TypeAlias,
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
        crate::declaration_type_alias_semantics(&alias)
            .map(|facts| crate::render_semantic_type(&facts.body)),
        Some(String::from("Union[int, None]")),
    );

    let function = declaration! {
        metadata: typepython_binding::DeclarationMetadata::Callable {
            signature: typepython_binding::BoundCallableSignature {
                params: vec![typepython_syntax::DirectFunctionParamSite {
                    name: String::from("value"),
                    annotation: Some(String::from("list[int]")),
                    annotation_expr: Some(list_of_int),
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                }],
                returns: Some(typepython_binding::BoundTypeExpr::from_expr(tuple_of_int)),
            },
        },
        name: String::from("build"),
        kind: DeclarationKind::Function,
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
        crate::declaration_signature_param_types(&function),
        Some(vec![String::from("list[int]")]),
    );
    assert_eq!(
        crate::declaration_signature_return_semantic_type(&function)
            .map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("tuple[int]")),
    );

    let import = declaration! {
        metadata: typepython_binding::DeclarationMetadata::Import {
            target: typepython_binding::BoundImportTarget::new("pkg.sub.Symbol"),
        },
        name: String::from("Symbol"),
        kind: DeclarationKind::Import,
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
        crate::declaration_import_target_ref(&import).map(|target| target.raw_target),
        Some(String::from("pkg.sub.Symbol")),
    );
}

#[test]
fn resolved_direct_call_candidate_carries_signature_return_and_substitutions() {
    let node = ModuleNode {
        module_path: PathBuf::from("<resolved-call>"),
        module_key: String::new(),
        module_kind: SourceKind::TypePython,
        declarations: Vec::new(),
        member_accesses: Vec::new(),
        returns: Vec::new(),
        yields: Vec::new(),
        if_guards: Vec::new(),
        asserts: Vec::new(),
        invalidations: Vec::new(),
        matches: Vec::new(),
        for_loops: Vec::new(),
        with_statements: Vec::new(),
        except_handlers: Vec::new(),
        assignments: Vec::new(),
        summary_fingerprint: 0,
        calls: Vec::new(),
        method_calls: Vec::new(),
    };
    let function = declaration! {
        metadata: callable_metadata("(value:T)->list[T]"),
        name: String::from("box_value"),
        kind: DeclarationKind::Function,
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
        type_params: vec![typepython_binding::GenericTypeParam {
            kind: typepython_binding::GenericTypeParamKind::TypeVar,
            name: String::from("T"),
            bound_expr: None,
            constraint_exprs: Vec::new(),
            default_expr: None,
        }],
    };
    let function = normalize_test_declaration(&function);
    let call = typepython_binding::CallSite {
        callee: String::from("box_value"),
        arg_count: 1,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("int"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };

    let resolved =
        crate::resolve_direct_call_candidate(&node, &[], &function, &call).expect("resolved call");

    assert_eq!(
        resolved
            .signature_sites
            .iter()
            .map(|param| param.annotation.as_deref().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["int"],
    );
    assert_eq!(
        resolved.return_type.as_ref().map(crate::diagnostic_type_text),
        Some(String::from("list[int]")),
    );
    assert_eq!(
        resolved.substitutions.types.get("T").map(crate::diagnostic_type_text).as_deref(),
        Some("int"),
    );
}

#[test]
fn diagnostic_type_text_renders_callable_types_stably() {
    let ty = crate::SemanticType::Callable {
        params: crate::SemanticCallableParams::ParamList(vec![crate::SemanticType::Name(
            String::from("int"),
        )]),
        return_type: Box::new(crate::SemanticType::Name(String::from("str"))),
    };

    assert_eq!(crate::diagnostic_type_text(&ty), "Callable[[int], str]");
}

#[test]
fn check_reports_ambiguous_generic_overload_resolution() {
    let result = check_temp_typepython_source(
        "overload def echo[T](value: T) -> T: ...\noverload def echo[U](value: U) -> U: ...\ndef echo(value: object) -> object:\n    return value\n\necho(1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4012"), "{rendered}");
    assert!(rendered.contains("ambiguous"), "{rendered}");
}

#[test]
fn check_accepts_variadic_direct_calls() {
    let result = check_temp_typepython_source(
        "def takes(*args: int):\n    return 0\n\ndef kw(**kwargs: int):\n    return 0\n\ntakes(1, 2, 3)\nkw(x=1, y=2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_duplicate_function_parameter_binding() {
    let result =
        check_temp_typepython_source("def takes(x: int):\n    return x\n\ntakes(1, x=2)\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("binds parameter `x` both positionally and by keyword"));
}

#[test]
fn check_reports_duplicate_constructor_parameter_binding() {
    let result = check_temp_typepython_source(
        "class User:\n    def __init__(self, age: int):\n        self.age = age\n\nUser(1, age=2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("binds parameter `age` both positionally and by keyword"));
}

#[test]
fn check_reports_duplicate_method_parameter_binding() {
    let result = check_temp_typepython_source(
        "class User:\n    def set_age(self, age: int):\n        self.age = age\n\nuser = User()\nuser.set_age(1, age=2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("binds parameter `age` both positionally and by keyword"));
}

#[test]
fn check_accepts_stub_overloaded_method_keyword_calls() {
    let graph = ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("/tmp/pkg/util.pyi"),
                module_key: String::from("pkg.util"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    declaration! {
                        name: String::from("User"),
                        kind: DeclarationKind::Class,
                        metadata: Default::default(),
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
                    declaration! {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        metadata: callable_metadata("(self,value:int)->int"),
                        value_type_expr: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            kind: DeclarationOwnerKind::Class,
                            name: String::from("User"),
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
                    declaration! {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        metadata: callable_metadata("(self,value:str)->str"),
                        value_type_expr: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            kind: DeclarationOwnerKind::Class,
                            name: String::from("User"),
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
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 0,
            },
            ModuleNode {
                module_path: PathBuf::from("/tmp/app.tpy"),
                module_key: String::from("app"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    declaration! {
                        name: String::from("User"),
                        kind: DeclarationKind::Import,
                        metadata: import_metadata("pkg.util.User"),
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
                    declaration! {
                        name: String::from("user"),
                        kind: DeclarationKind::Value,
                        metadata: value_metadata("User"),
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
                ],
                calls: Vec::new(),
                method_calls: vec![typepython_binding::MethodCallSite {
                    current_owner_name: None,
                    current_owner_type_name: None,
                    owner_name: String::from("user"),
                    method: String::from("parse"),
                    through_instance: false,
                    arg_count: 0,
                    arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                        Vec::new(),
                    ),
                    starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                        Vec::new(),
                    ),
                    keyword_names: vec![String::from("value")],
                    keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                        vec![String::from("str")],
                    ),
                    keyword_expansion_values:
                        typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                    line: 1,
                }],
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 0,
            },
        ],
    };
    let graph = normalize_test_graph(&graph);

    let result = check(&graph);
    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_stub_overloaded_method_return_type() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("/tmp/pkg/util.pyi"),
                module_key: String::from("pkg.util"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    declaration! {
                        name: String::from("User"),
                        kind: DeclarationKind::Class,
                        metadata: Default::default(),
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
                    declaration! {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        metadata: callable_metadata("(self,value:int)->int"),
                        value_type_expr: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("User"),
                            kind: DeclarationOwnerKind::Class,
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
                    declaration! {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        metadata: callable_metadata("(self,value:str)->str"),
                        value_type_expr: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("User"),
                            kind: DeclarationOwnerKind::Class,
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
                ],
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            },
            ModuleNode {
                module_path: PathBuf::from("/tmp/app.tpy"),
                module_key: String::from("app"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    declaration! {
                        name: String::from("User"),
                        kind: DeclarationKind::Import,
                        metadata: import_metadata("pkg.util.User"),
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
                    declaration! {
                        name: String::from("user"),
                        kind: DeclarationKind::Value,
                        metadata: value_metadata("User"),
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
                    declaration! {
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        metadata: value_metadata("str"),
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
                ],
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("value"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("str")),
                    annotation_expr: None,
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("user")),
                    value_method_name: Some(String::from("parse")),
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
                    value: None,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: vec![typepython_binding::MethodCallSite {
                    current_owner_name: None,
                    current_owner_type_name: None,
                    owner_name: String::from("user"),
                    method: String::from("parse"),
                    through_instance: false,
                    arg_count: 0,
                    arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                        Vec::new(),
                    ),
                    starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                        Vec::new(),
                    ),
                    keyword_names: vec![String::from("value")],
                    keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                        vec![String::from("str")],
                    ),
                    keyword_expansion_values:
                        typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                    line: 1,
                }],
            },
        ],
    });

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("assigns `int` where `value` expects `str`"), "{rendered}");
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_generic_method_overload_specificity() {
    let result =
        check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("/tmp/pkg/util.pyi"),
                    module_key: String::from("pkg.util"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![
                        declaration! {
                            name: String::from("User"),
                            kind: DeclarationKind::Class,
                            metadata: Default::default(),
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
                        declaration! {
                            name: String::from("parse"),
                            kind: DeclarationKind::Overload,
                            metadata: callable_metadata("(self,value:T)->tuple[T]"),
                            value_type_expr: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("User"),
                                kind: DeclarationOwnerKind::Class,
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
                            type_params: vec![typepython_binding::GenericTypeParam {
                                kind: typepython_binding::GenericTypeParamKind::TypeVar,
                                name: String::from("T"),
                                bound_expr: None,
                                constraint_exprs: Vec::new(),
                                default_expr: None,
                            }],
                        },
                        declaration! {
                            name: String::from("parse"),
                            kind: DeclarationKind::Overload,
                            metadata: callable_metadata("(self,value:object)->tuple[object]"),
                            value_type_expr: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("User"),
                                kind: DeclarationOwnerKind::Class,
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
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
                    member_accesses: Vec::new(),
                    returns: Vec::new(),
                    yields: Vec::new(),
                    if_guards: Vec::new(),
                    asserts: Vec::new(),
                    invalidations: Vec::new(),
                    matches: Vec::new(),
                    for_loops: Vec::new(),
                    with_statements: Vec::new(),
                    except_handlers: Vec::new(),
                    assignments: Vec::new(),
                    summary_fingerprint: 1,
                },
                ModuleNode {
                    module_path: PathBuf::from("/tmp/app.tpy"),
                    module_key: String::from("app"),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![
                        declaration! {
                            name: String::from("User"),
                            kind: DeclarationKind::Import,
                            metadata: import_metadata("pkg.util.User"),
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
                        declaration! {
                            name: String::from("user"),
                            kind: DeclarationKind::Value,
                            metadata: value_metadata("User"),
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
                        declaration! {
                            name: String::from("result"),
                            kind: DeclarationKind::Value,
                            metadata: value_metadata("tuple[int]"),
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
                    ],
                    calls: Vec::new(),
                    method_calls: vec![typepython_binding::MethodCallSite {
                        current_owner_name: None,
                        current_owner_type_name: None,
                        owner_name: String::from("user"),
                        method: String::from("parse"),
                        through_instance: false,
                        arg_count: 1,
                        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                            vec![String::from("int")],
                        ),
                        starred_arg_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        keyword_names: Vec::new(),
                        keyword_arg_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        keyword_expansion_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        line: 1,
                    }],
                    member_accesses: Vec::new(),
                    returns: Vec::new(),
                    yields: Vec::new(),
                    if_guards: Vec::new(),
                    asserts: Vec::new(),
                    invalidations: Vec::new(),
                    matches: Vec::new(),
                    for_loops: Vec::new(),
                    with_statements: Vec::new(),
                    except_handlers: Vec::new(),
                    assignments: vec![typepython_binding::AssignmentSite {
                        name: String::from("result"),
                        destructuring_target_names: None,
                        destructuring_index: None,
                        annotation: Some(String::from("tuple[int]")),
                        annotation_expr: None,
                        is_awaited: false,
                        value_callee: None,
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: Some(String::from("user")),
                        value_method_name: Some(String::from("parse")),
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
                        value: None,
                        owner_name: None,
                        owner_type_name: None,
                        line: 1,
                    }],
                    summary_fingerprint: 1,
                },
            ],
        });

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_imported_defaulted_function_call() {
    let result =
        check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("/tmp/lib.pyi"),
                    module_key: String::from("lib"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![declaration! {
                        name: String::from("f"),
                        kind: DeclarationKind::Function,
                        metadata: callable_metadata("(x:int,y:int=)->None"),
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
                    }],
                    member_accesses: Vec::new(),
                    returns: Vec::new(),
                    yields: Vec::new(),
                    if_guards: Vec::new(),
                    asserts: Vec::new(),
                    invalidations: Vec::new(),
                    matches: Vec::new(),
                    for_loops: Vec::new(),
                    with_statements: Vec::new(),
                    except_handlers: Vec::new(),
                    assignments: Vec::new(),
                    summary_fingerprint: 1,
                    calls: Vec::new(),
                    method_calls: Vec::new(),
                },
                ModuleNode {
                    module_path: PathBuf::from("/tmp/app.tpy"),
                    module_key: String::from("app"),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![declaration! {
                        name: String::from("f"),
                        kind: DeclarationKind::Import,
                        metadata: import_metadata("lib.f"),
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
                    }],
                    member_accesses: Vec::new(),
                    returns: Vec::new(),
                    yields: Vec::new(),
                    if_guards: Vec::new(),
                    asserts: Vec::new(),
                    invalidations: Vec::new(),
                    matches: Vec::new(),
                    for_loops: Vec::new(),
                    with_statements: Vec::new(),
                    except_handlers: Vec::new(),
                    assignments: Vec::new(),
                    summary_fingerprint: 1,
                    calls: vec![typepython_binding::CallSite {
                        callee: String::from("f"),
                        arg_count: 1,
                        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                            vec![String::from("int")],
                        ),
                        starred_arg_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        keyword_names: Vec::new(),
                        keyword_arg_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        keyword_expansion_values:
                            typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                        line: 1,
                    }],
                    method_calls: Vec::new(),
                },
            ],
        });

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_stub_only_overload_sets_in_pyi_modules() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("types/module.pyi"),
            module_key: String::new(),
            module_kind: SourceKind::Stub,
            declarations: vec![
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    metadata: Default::default(),
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
                declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    metadata: Default::default(),
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
            ],
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

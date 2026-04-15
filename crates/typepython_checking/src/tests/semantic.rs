use super::*;

#[test]
fn check_accepts_empty_tail_paramspec_call() {
    let result = check_temp_typepython_source(
        "from typing import Callable, ParamSpec\n\nP = ParamSpec(\"P\")\n\ndef invoke(cb: Callable[P, int]) -> int:\n    return cb()\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_empty_tail_concatenate_call() {
    let result = check_temp_typepython_source(
        "from typing import Callable, Concatenate, ParamSpec\n\nP = ParamSpec(\"P\")\n\ndef invoke(cb: Callable[Concatenate[int, P], int]) -> int:\n    return cb(1)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_source_authored_paramspec_forwarding_call() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def invoke[**P, R](cb: Callable[P, R], *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "    return cb(*args, **kwargs)\n\n",
        "def greet(name: str, *, times: int) -> str:\n",
        "    return name\n\n",
        "result: str = invoke(greet, \"Ada\", times=1)\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_source_authored_paramspec_keyword_mismatch() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def invoke[**P, R](cb: Callable[P, R], *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "    return cb(*args, **kwargs)\n\n",
        "def greet(name: str, *, times: int) -> str:\n",
        "    return name\n\n",
        "result: str = invoke(greet, \"Ada\", times=\"oops\")\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("keyword `times`"));
    assert!(rendered.contains("expects `int`"));
}

#[test]
fn check_reports_unsafe_boundary_with_source_overrides_without_backing_file() {
    let result = check_virtual_source_with_overrides(
        "def run(expr: str) -> None:\n    eval(expr)\n",
        ParseOptions::default(),
        true,
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4019"), "{rendered}");
    assert!(rendered.contains("must appear inside `unsafe:`"), "{rendered}");
}

#[test]
fn check_reports_conditional_return_with_source_overrides_without_backing_file() {
    let result = check_virtual_source_with_overrides(
        "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n",
        ParseOptions { enable_conditional_returns: true, ..ParseOptions::default() },
        false,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4018"), "{rendered}");
    assert!(rendered.contains("missing: None"), "{rendered}");
}

#[test]
fn check_accepts_source_authored_concatenate_forwarding_call() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def bind_first[**P, R](cb: Callable[Concatenate[int, P], R], *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "    return cb(1, *args, **kwargs)\n\n",
        "def greet(prefix: int, name: str, *, times: int) -> str:\n",
        "    return name\n\n",
        "result: str = bind_first(greet, \"Ada\", times=1)\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_generic_callable_decorator_transform() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def identity[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "    return fn\n\n",
        "@identity\n",
        "def greet(name: str) -> str:\n",
        "    return name\n\n",
        "value: str = greet(\"Ada\")\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn decorated_function_transform_rewrites_effective_callable_annotation() {
    let source_text = concat!(
        "from typing import Callable, cast\n\n",
        "def stringify[**P](fn: Callable[P, int]) -> Callable[P, str]:\n",
        "    return cast(Callable[P, str], fn)\n\n",
        "@stringify\n",
        "def count(value: int) -> int:\n",
        "    return value\n",
    );
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    fs::write(&path, source_text).expect("temp source should be written");
    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let node = &graph.nodes[0];
    let decorator_info = typepython_syntax::collect_decorator_transform_module_info(source_text);

    assert_eq!(decorator_info.callables.len(), 1);
    assert_eq!(decorator_info.callables[0].name, "count");
    assert_eq!(decorator_info.callables[0].decorators, vec![String::from("stringify")]);
    let (decorator_node, decorator) =
        crate::resolve_function_provider_with_node(&graph.nodes, node, "stringify")
            .expect("decorator provider");
    let base_callable = String::from("Callable[[int], int]");
    let fake_call = crate::synthetic_decorator_application_call(&decorator.name, &base_callable);
    let instantiated_signature = crate::resolve_instantiated_direct_function_signature(
        decorator_node,
        &graph.nodes,
        decorator,
        &fake_call,
    );
    assert!(instantiated_signature.is_some(), "instantiated signature");
    let instantiated_return = crate::resolve_instantiated_callable_return_type_from_declaration(
        decorator_node,
        &graph.nodes,
        decorator,
        &fake_call,
    );
    let instantiated_semantic_return =
        crate::resolve_instantiated_callable_return_semantic_type_from_declaration(
            decorator_node,
            &graph.nodes,
            decorator,
            &fake_call,
        );
    assert_eq!(instantiated_return, Some(String::from("Callable[[int], str]")));
    assert_eq!(
        instantiated_semantic_return.map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("Callable[[int], str]"))
    );
    let base_semantic_callable = crate::lower_type_text_or_name("Callable[[int], int]");
    assert_eq!(
        crate::apply_named_callable_decorator_transform_semantic(
            decorator_node,
            &graph.nodes,
            &decorator.name,
            &base_semantic_callable,
        )
        .as_ref()
        .map(crate::diagnostic_type_text),
        Some(String::from("Callable[[int], str]"))
    );
    assert_eq!(
        crate::apply_named_callable_decorator_transform(
            decorator_node,
            &graph.nodes,
            &decorator.name,
            &base_callable,
        ),
        Some(String::from("Callable[[int], str]"))
    );

    let context = crate::CheckerContext::new(&graph.nodes, ImportFallback::Unknown, None);
    assert_eq!(
        crate::resolve_decorated_function_callable_semantic_type_with_context(
            &context,
            node,
            &graph.nodes,
            "count",
        )
        .as_ref()
        .map(crate::diagnostic_type_text),
        Some(String::from("Callable[[int], str]"))
    );
    assert_eq!(
        crate::resolve_decorated_function_callable_annotation(node, &graph.nodes, "count"),
        Some(String::from("Callable[[int], str]"))
    );
}

#[test]
fn semantic_callable_assignability_handles_concatenate_structurally() {
    let node = ModuleNode {
        module_path: PathBuf::from("<callable-assignability>"),
        module_key: String::from("callable.assignability"),
        module_kind: SourceKind::TypePython,
        declarations: Vec::new(),
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
    };
    let expected = crate::SemanticType::Callable {
        params: crate::SemanticCallableParams::Concatenate(vec![
            crate::SemanticType::Name(String::from("int")),
            crate::SemanticType::Name(String::from("P")),
        ]),
        return_type: Box::new(crate::SemanticType::Name(String::from("str"))),
    };
    let assignable = crate::SemanticType::Callable {
        params: crate::SemanticCallableParams::Concatenate(vec![
            crate::SemanticType::Name(String::from("Any")),
            crate::SemanticType::Name(String::from("P")),
        ]),
        return_type: Box::new(crate::SemanticType::Name(String::from("str"))),
    };
    let incompatible = crate::SemanticType::Callable {
        params: crate::SemanticCallableParams::Concatenate(vec![
            crate::SemanticType::Name(String::from("str")),
            crate::SemanticType::Name(String::from("P")),
        ]),
        return_type: Box::new(crate::SemanticType::Name(String::from("str"))),
    };

    assert!(crate::semantic_type_is_assignable(&node, &[], &expected, &assignable));
    assert!(!crate::semantic_type_is_assignable(&node, &[], &expected, &incompatible));
}

#[test]
fn imported_symbol_semantic_target_resolves_module_and_symbol_imports() {
    let graph = ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("/tmp/pkg/util.pyi"),
                module_key: String::from("pkg.util"),
                module_kind: SourceKind::Stub,
                declarations: vec![declaration! {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
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
                declarations: vec![
                    declaration! {
                        name: String::from("util"),
                        kind: DeclarationKind::Import,
                        metadata: import_metadata("pkg.util"),
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
        ],
    };
    let graph = normalize_test_graph(&graph);
    let node = &graph.nodes[1];

    let module_target = crate::resolve_imported_symbol_semantic_target(node, &graph.nodes, "util")
        .expect("module import target");
    assert_eq!(
        module_target.module_target().map(|module| module.module_key.as_str()),
        Some("pkg.util")
    );

    let symbol_target = crate::resolve_imported_symbol_semantic_target(node, &graph.nodes, "parse")
        .expect("symbol import target");
    assert_eq!(
        symbol_target.function_provider().map(|(provider, declaration)| {
            (provider.module_key.clone(), declaration.name.clone())
        }),
        Some((String::from("pkg.util"), String::from("parse"))),
    );
}

#[test]
fn direct_expression_semantic_type_unwraps_awaited_call_results() {
    let source_text = "async def fetch() -> int:\n    return 1\n";
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    fs::write(&path, source_text).expect("temp source should be written");
    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let node = &graph.nodes[0];

    assert_eq!(
        crate::resolve_direct_callable_return_semantic_type(node, &graph.nodes, "fetch")
            .map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("Awaitable[int]"))
    );
    assert_eq!(
        crate::resolve_direct_expression_semantic_type(
            node,
            &graph.nodes,
            None,
            None,
            None,
            None,
            1,
            None,
            true,
            Some("fetch"),
            None,
            None,
            None,
            false,
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("int"))
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn semantic_member_method_and_subscript_resolution_preserve_structured_types() {
    let source_text = concat!(
        "class Box:\n",
        "    value: list[int]\n",
        "    def get(self) -> tuple[int, str]:\n",
        "        return (1, \"x\")\n",
    );
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    fs::write(&path, source_text).expect("temp source should be written");
    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let node = &graph.nodes[0];

    assert_eq!(
        crate::resolve_direct_member_reference_semantic_type(
            node,
            &graph.nodes,
            None,
            None,
            None,
            None,
            1,
            "Box",
            "value",
            false,
        )
        .map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("list[int]"))
    );
    assert_eq!(
        crate::resolve_direct_method_return_semantic_type(
            node,
            &graph.nodes,
            None,
            None,
            None,
            None,
            1,
            "Box",
            "get",
            false,
        )
        .map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("tuple[int, str]"))
    );
    assert_eq!(
        crate::resolve_subscript_type_from_target_semantic_type(
            node,
            &graph.nodes,
            &crate::lower_type_text_or_name("tuple[int, str]"),
            None,
            Some("1"),
        )
        .map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("str"))
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn semantic_name_resolution_preserves_callable_shapes() {
    let source_text = "def greet(name: str) -> int:\n    return 1\n";
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    fs::write(&path, source_text).expect("temp source should be written");
    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let node = &graph.nodes[0];

    assert_eq!(
        crate::resolve_direct_name_reference_semantic_type(
            node,
            &graph.nodes,
            None,
            None,
            None,
            None,
            1,
            "greet",
        )
        .map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("Callable[[str], int]"))
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn semantic_name_resolution_uses_decorated_callable_semantic_path() {
    let source_text = concat!(
        "def stringify(func: Callable[[int], int]) -> Callable[[int], str]:\n",
        "    return func\n\n",
        "@stringify\n",
        "def count(value: int) -> int:\n",
        "    return value\n",
    );
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    fs::write(&path, source_text).expect("temp source should be written");
    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let node = &graph.nodes[0];
    let context = crate::CheckerContext::new(&graph.nodes, ImportFallback::Unknown, None);

    assert_eq!(
        crate::resolve_direct_name_reference_semantic_type_with_context(
            &context,
            node,
            &graph.nodes,
            None,
            None,
            None,
            None,
            1,
            "count",
        )
        .map(|ty| crate::diagnostic_type_text(&ty)),
        Some(String::from("Callable[[int], str]"))
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn semantic_guard_bindings_apply_flow_narrowing() {
    let source_text = "value = 1\n";
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    fs::write(&path, source_text).expect("temp source should be written");
    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let node = &graph.nodes[0];
    let mut bindings = BTreeMap::new();
    bindings.insert(String::from("value"), crate::lower_type_text_or_name("Optional[int]"));

    let narrowed = crate::apply_guard_to_local_semantic_bindings(
        node,
        &graph.nodes,
        &bindings,
        &typepython_binding::GuardConditionSite::IsNone {
            name: String::from("value"),
            negated: true,
        },
        true,
    );

    assert_eq!(narrowed.get("value").map(crate::render_semantic_type), Some(String::from("int")));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn semantic_metadata_resolution_reuses_expression_semantic_path() {
    let source_text = "async def fetch() -> int:\n    return 1\n";
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    fs::write(&path, source_text).expect("temp source should be written");
    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let node = &graph.nodes[0];
    let metadata = typepython_syntax::DirectExprMetadata {
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
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None,
    };

    assert_eq!(
        crate::resolve_direct_expression_semantic_type_from_metadata(
            node,
            &graph.nodes,
            None,
            None,
            None,
            1,
            &metadata,
        )
        .map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("int"))
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn semantic_contextual_lambda_resolution_builds_callable_types() {
    let source_text = "value = 1\n";
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    fs::write(&path, source_text).expect("temp source should be written");
    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let node = &graph.nodes[0];
    let lambda = typepython_syntax::LambdaMetadata {
        params: vec![typepython_syntax::FunctionParam {
            name: String::from("item"),
            annotation: None,
            annotation_expr: None,
            has_default: false,
            positional_only: false,
            keyword_only: false,
            variadic: false,
            keyword_variadic: false,
        }],
        body: Box::new(typepython_syntax::DirectExprMetadata {
            value_type_expr: Some(typepython_syntax::TypeExpr::Name(String::from("str"))),
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
    };

    assert_eq!(
        crate::resolve_contextual_lambda_callable_semantic_type(
            node,
            &graph.nodes,
            None,
            None,
            1,
            &lambda,
            Some("Callable[[int], str]"),
            None,
        )
        .map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("Callable[[int], str]"))
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn call_diagnostics_resolve_argument_types_through_semantic_path() {
    let source_text = "async def fetch() -> int:\n    return 1\n";
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    fs::write(&path, source_text).expect("temp source should be written");
    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let node = &graph.nodes[0];
    let call = typepython_binding::CallSite {
        callee: String::from("consume"),
        arg_count: 1,
        arg_values: vec![typepython_syntax::DirectExprMetadata {
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
            value_list_comprehension: None,
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        }],
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };

    assert_eq!(
        crate::resolved_call_arg_semantic_types(
            node,
            &graph.nodes,
            &call,
            &[Some(String::from("int"))],
        )
        .into_iter()
        .map(|ty| crate::render_semantic_type(&ty))
        .collect::<Vec<_>>(),
        vec![String::from("int")]
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_reports_callable_decorator_transform_return_rewrite() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable, cast\n\n",
        "def stringify[**P](fn: Callable[P, int]) -> Callable[P, str]:\n",
        "    return cast(Callable[P, str], fn)\n\n",
        "@stringify\n",
        "def count(value: int) -> int:\n",
        "    return value\n\n",
        "bad: int = count(1)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `str`"));
    assert!(rendered.contains("expects `int`"));
}

#[test]
fn check_accepts_method_callable_decorator_transform() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def identity[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "    return fn\n\n",
        "class Box:\n",
        "    @identity\n",
        "    def render(self, value: int) -> str:\n",
        "        return str(value)\n\n",
        "box = Box()\n",
        "text: str = box.render(1)\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_non_callable_decorator_transform_in_strict_mode() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "class Route:\n    pass\n\n",
            "def route(fn: Callable[[int], int]) -> Route:\n",
            "    return Route()\n\n",
            "@route\n",
            "def count(value: int) -> int:\n",
            "    return value\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("resolves to non-callable type `Route`"));
    assert!(rendered.contains("decorator `route`"));
}

#[test]
fn check_allows_non_callable_decorator_transform_in_non_strict_mode() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "class Route:\n    pass\n\n",
            "def route(fn: Callable[[int], int]) -> Route:\n",
            "    return Route()\n\n",
            "@route\n",
            "def count(value: int) -> int:\n",
            "    return value\n\n",
            "text: str = count(1)\n",
            "number: int = count(1)\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

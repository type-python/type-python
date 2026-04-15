use super::*;

#[test]
fn check_accepts_direct_generic_function_call_inference() {
    let result = check_temp_typepython_source(
        "def first[T](value: T) -> T:\n    return value\n\nresult: int = first(1)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_generic_function_call_inference_through_optional_annotation() {
    let result = check_temp_typepython_source(
        "def maybe[T](x: T | None) -> T | None:\n    return x\n\nvalue: int | None = maybe(1)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_generic_function_call_inference_from_type_param_default() {
    let result = check_temp_typepython_source(
        "def build[T = int](value: T = 1) -> T:\n    return value\n\nresult: int = build()\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_infers_generic_function_call_through_union_actual() {
    let node = ModuleNode {
        module_path: PathBuf::from("<generic-inference>"),
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
    let function = Declaration {
        metadata: Default::default(),
        name: String::from("maybe"),
        kind: DeclarationKind::Function,
        legacy_detail: String::from("(x:T | None)->T | None"),
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
            bound: None,
            constraints: Vec::new(),
            default: None,
            bound_expr: None,
            constraint_exprs: Vec::new(),
            default_expr: None,
        }],
    };
    let signature = vec![typepython_syntax::DirectFunctionParamSite {
        name: String::from("x"),
        annotation: Some(String::from("T | None")),
        annotation_expr: None,
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    }];
    let call = typepython_binding::CallSite {
        callee: String::from("maybe"),
        arg_count: 1,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("int | None"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };

    let substitutions =
        crate::infer_generic_type_param_substitutions(&node, &[], &function, &signature, &call)
            .expect("union actual should infer through Optional-like annotation");

    assert_eq!(
        substitutions.types.get("T").map(crate::render_semantic_type).as_deref(),
        Some("int")
    );
}

#[test]
fn check_generic_inference_prefers_arg_metadata_over_arg_type_text() {
    let node = ModuleNode {
        module_path: PathBuf::from("<generic-inference>"),
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
    let function = Declaration {
        metadata: Default::default(),
        name: String::from("wrap"),
        kind: DeclarationKind::Function,
        legacy_detail: String::from("(value:T)->T"),
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
            bound: None,
            constraints: Vec::new(),
            default: None,
            bound_expr: None,
            constraint_exprs: Vec::new(),
            default_expr: None,
        }],
    };
    let signature = vec![typepython_syntax::DirectFunctionParamSite {
        name: String::from("value"),
        annotation: Some(String::from("T")),
        annotation_expr: None,
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    }];
    let call = typepython_binding::CallSite {
        callee: String::from("wrap"),
        arg_count: 1,
        arg_values: vec![typepython_syntax::DirectExprMetadata {
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
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };

    let substitutions =
        crate::infer_generic_type_param_substitutions(&node, &[], &function, &signature, &call)
            .expect("generic inference should use structured arg metadata");

    assert_eq!(
        substitutions.types.get("T").map(crate::render_semantic_type).as_deref(),
        Some("list[int]")
    );
}

#[test]
fn check_infers_typevartuple_from_variadic_call_arguments() {
    let node = ModuleNode {
        module_path: PathBuf::from("<generic-inference>"),
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
    let function = Declaration {
        metadata: Default::default(),
        name: String::from("collect"),
        kind: DeclarationKind::Function,
        legacy_detail: String::from("(*args:Unpack[Ts])->tuple[Unpack[Ts]]"),
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
            kind: typepython_binding::GenericTypeParamKind::TypeVarTuple,
            name: String::from("Ts"),
            bound: None,
            constraints: Vec::new(),
            default: None,
            bound_expr: None,
            constraint_exprs: Vec::new(),
            default_expr: None,
        }],
    };
    let function = normalize_test_declaration(&function);
    let signature = crate::declaration_signature_sites(&function);
    let call = typepython_binding::CallSite {
        callee: String::from("collect"),
        arg_count: 2,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("int"),
            String::from("str"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };

    let substitutions =
        crate::infer_generic_type_param_substitutions(&node, &[], &function, &signature, &call)
            .expect("variadic pack should be inferred from positional arguments");

    assert_eq!(
        substitutions.type_packs.get("Ts").map(|binding| {
            binding.types.iter().map(crate::render_semantic_type).collect::<Vec<_>>()
        }),
        Some(vec![String::from("int"), String::from("str")]),
    );
}

#[test]
fn check_infers_paramspec_from_callable_argument() {
    let node = ModuleNode {
        module_path: PathBuf::from("<generic-inference>"),
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
    let function = Declaration {
        metadata: Default::default(),
        name: String::from("wrap"),
        kind: DeclarationKind::Function,
        legacy_detail: String::from("(cb:Callable[P, int])->Callable[P, int]"),
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
            kind: typepython_binding::GenericTypeParamKind::ParamSpec,
            name: String::from("P"),
            bound: None,
            constraints: Vec::new(),
            default: None,
            bound_expr: None,
            constraint_exprs: Vec::new(),
            default_expr: None,
        }],
    };
    let function = normalize_test_declaration(&function);
    let signature = crate::declaration_signature_sites(&function);
    let call = typepython_binding::CallSite {
        callee: String::from("wrap"),
        arg_count: 1,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("Callable[[str], int]"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };

    let substitutions =
        crate::infer_generic_type_param_substitutions(&node, &[], &function, &signature, &call)
            .expect("callable argument should infer ParamSpec bindings");

    assert_eq!(
        substitutions.param_lists.get("P").map(|binding| {
            binding
                .params
                .iter()
                .map(|param| param.annotation.as_deref().unwrap_or_default().to_owned())
                .collect::<Vec<_>>()
        }),
        Some(vec![String::from("str")]),
    );
}

#[test]
fn check_instantiates_variadic_typevartuple_signature_and_return() {
    let node = ModuleNode {
        module_path: PathBuf::from("<generic-inference>"),
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
    let function = Declaration {
        metadata: Default::default(),
        name: String::from("collect"),
        kind: DeclarationKind::Function,
        legacy_detail: String::from("(*args:Unpack[Ts])->tuple[Unpack[Ts]]"),
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
            kind: typepython_binding::GenericTypeParamKind::TypeVarTuple,
            name: String::from("Ts"),
            bound: None,
            constraints: Vec::new(),
            default: None,
            bound_expr: None,
            constraint_exprs: Vec::new(),
            default_expr: None,
        }],
    };
    let function = normalize_test_declaration(&function);
    let call = typepython_binding::CallSite {
        callee: String::from("collect"),
        arg_count: 2,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("int"),
            String::from("str"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };

    let instantiated_signature =
        crate::resolve_instantiated_direct_function_signature(&node, &[], &function, &call)
            .expect("instantiated signature");
    let instantiated_return = crate::resolve_instantiated_callable_return_type_from_declaration(
        &node,
        &[],
        &function,
        &call,
    )
    .expect("instantiated return type");

    assert_eq!(instantiated_signature.len(), 2);
    assert!(instantiated_signature.iter().all(|param| param.positional_only));
    assert_eq!(
        instantiated_signature
            .iter()
            .map(|param| param.annotation.as_deref().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["int", "str"],
    );
    assert_eq!(instantiated_return, "tuple[int, str]");
}

#[test]
fn check_infers_typevartuple_inside_tuple_annotation() {
    let node = ModuleNode {
        module_path: PathBuf::from("<generic-inference>"),
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
    let function = Declaration {
        metadata: Default::default(),
        name: String::from("collect"),
        kind: DeclarationKind::Function,
        legacy_detail: String::from("(value:tuple[Unpack[Ts]])->tuple[Unpack[Ts]]"),
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
            kind: typepython_binding::GenericTypeParamKind::TypeVarTuple,
            name: String::from("Ts"),
            bound: None,
            constraints: Vec::new(),
            default: None,
            bound_expr: None,
            constraint_exprs: Vec::new(),
            default_expr: None,
        }],
    };
    let function = normalize_test_declaration(&function);
    let signature = crate::declaration_signature_sites(&function);
    let call = typepython_binding::CallSite {
        callee: String::from("collect"),
        arg_count: 1,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("tuple[int, str]"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };

    let substitutions =
        crate::infer_generic_type_param_substitutions(&node, &[], &function, &signature, &call)
            .expect("tuple unpack should bind type pack");

    assert_eq!(
        substitutions.type_packs.get("Ts").map(|binding| {
            binding.types.iter().map(crate::render_semantic_type).collect::<Vec<_>>()
        }),
        Some(vec![String::from("int"), String::from("str")]),
    );
}

#[test]
fn check_accepts_source_authored_typevartuple_call_inference() {
    let result = check_temp_typepython_source(
        "def collect[*Ts](*args: *Ts) -> tuple[*Ts]:\n    return args\n\nvalue: tuple[int, str] = collect(1, \"x\")\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_source_authored_typevartuple_call_from_starred_iterable() {
    let result = check_temp_typepython_source(
        "def collect[*Ts](*args: *Ts) -> tuple[*Ts]:\n    return args\n\nitems: list[int] = [1, 2]\nvalue: tuple[int, ...] = collect(*items)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_source_authored_typevartuple_method_call_from_starred_iterable() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    metadata: Default::default(),
                    legacy_detail: String::new(),
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
                    name: String::from("box"),
                    kind: DeclarationKind::Value,
                    metadata: Default::default(),
                    legacy_detail: String::from("Box"),
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
                    name: String::from("collect"),
                    kind: DeclarationKind::Function,
                    metadata: Default::default(),
                    legacy_detail: String::from("(self,*args:Unpack[Ts])->tuple[Unpack[Ts]]"),
                    value_type_expr: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
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
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: vec![typepython_binding::GenericTypeParam {
                        kind: typepython_binding::GenericTypeParamKind::TypeVarTuple,
                        name: String::from("Ts"),
                        bound: None,
                        constraints: Vec::new(),
                        default: None,
                        bound_expr: None,
                        constraint_exprs: Vec::new(),
                        default_expr: None,
                    }],
                },
            ],
            calls: Vec::new(),
            method_calls: vec![typepython_binding::MethodCallSite {
                current_owner_name: None,
                current_owner_type_name: None,
                owner_name: String::from("box"),
                method: String::from("collect"),
                through_instance: false,
                arg_count: 0,
                arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                    vec![String::from("list[int]")],
                ),
                keyword_names: Vec::new(),
                keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
                    Vec::new(),
                ),
                keyword_expansion_values:
                    typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
                line: 1,
            }],
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("run"),
                owner_type_name: None,
                value: None,
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: None,
                value_member_name: None,
                value_member_through_instance: false,
                value_method_owner_name: Some(String::from("box")),
                value_method_name: Some(String::from("collect")),
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
fn resolve_scope_param_semantic_type_uses_declaration_signature_sites() {
    let node = ModuleNode {
        module_path: PathBuf::from("<scope-params>"),
        module_key: String::from("scope.params"),
        module_kind: SourceKind::TypePython,
        declarations: vec![Declaration {
            name: String::from("build"),
            kind: DeclarationKind::Function,
            metadata: Default::default(),
            legacy_detail: String::from("(x:int,*args:str,**kwargs:bool)->None"),
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
    };
    let node = normalize_test_graph(&ModuleGraph { nodes: vec![node] })
        .nodes
        .into_iter()
        .next()
        .expect("normalized node");

    assert_eq!(
        crate::resolve_scope_param_semantic_type(&node, Some("build"), None, "x")
            .as_ref()
            .map(crate::diagnostic_type_text),
        Some(String::from("int")),
    );
    assert_eq!(
        crate::resolve_scope_param_semantic_type(&node, Some("build"), None, "args")
            .as_ref()
            .map(crate::diagnostic_type_text),
        Some(String::from("tuple[str, ...]")),
    );
    assert_eq!(
        crate::resolve_scope_param_semantic_type(&node, Some("build"), None, "kwargs")
            .as_ref()
            .map(crate::diagnostic_type_text),
        Some(String::from("dict[str, bool]")),
    );
}

#[test]
fn check_accepts_source_authored_typevartuple_alias_roundtrip() {
    let result = check_temp_typepython_source(
        "typealias Pack[*Ts] = tuple[*Ts]\n\nvalue: Pack[int, str] = (1, \"x\")\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_imported_semantic_type_alias_expansion() {
    let result = check_temp_project_sources(&[
        ("base.tpy", "base", SourceKind::TypePython, "typealias Seq[T] = list[T]\n"),
        (
            "aliases.tpy",
            "aliases",
            SourceKind::TypePython,
            "from base import Seq\n\ntypealias Items[T] = Seq[T]\n",
        ),
        (
            "app.tpy",
            "app",
            SourceKind::TypePython,
            "from aliases import Items\n\nvalue: Items[int] = [1, 2, 3]\n",
        ),
    ]);

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn alias_type_param_substitutions_semantic_uses_semantic_args_directly() {
    let alias = Declaration {
        metadata: Default::default(),
        name: String::from("Items"),
        kind: DeclarationKind::TypeAlias,
        legacy_detail: String::from("list[T]"),
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
            bound: None,
            constraints: Vec::new(),
            default: None,
            bound_expr: None,
            constraint_exprs: Vec::new(),
            default_expr: None,
        }],
    };

    let substitutions = crate::alias_type_param_substitutions_semantic(
        &alias,
        &[crate::SemanticType::Generic {
            head: String::from("tuple"),
            args: vec![
                crate::SemanticType::Name(String::from("int")),
                crate::SemanticType::Name(String::from("str")),
            ],
        }],
    )
    .expect("semantic alias substitution should succeed");

    assert_eq!(
        substitutions.types.get("T").map(crate::diagnostic_type_text),
        Some(String::from("tuple[int, str]")),
    );
}

#[test]
fn check_rejects_source_authored_typevartuple_alias_shape_mismatch() {
    let result = check_temp_typepython_source(
        "typealias Pack[*Ts] = tuple[*Ts]\n\nvalue: Pack[int, str] = (1, 2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
}

#[test]
fn check_accepts_source_authored_typevartuple_alias_through_call() {
    let result = check_temp_typepython_source(
        "typealias Pack[*Ts] = tuple[*Ts]\n\ndef collect[*Ts](*args: *Ts) -> Pack[*Ts]:\n    return args\n\nvalue: Pack[int, str] = collect(1, \"x\")\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_rejects_generic_function_call_outside_constraint_list() {
    let result = check_temp_typepython_source(
        "def choose[T: (str, bytes)](value: T) -> T:\n    return value\n\nbad: int = choose(1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(
        rendered.contains("call to `choose`")
            || rendered.contains("assigns `T` where `bad` expects `int`")
            || rendered.contains("returns `T` where `bad` expects `int`"),
        "{rendered}"
    );
}

#[test]
fn check_reports_conflicting_union_aware_generic_inference() {
    let result = check_temp_typepython_source(
        "value: str | None = \"x\"\n\ndef choose[T](x: T | None, y: T) -> T:\n    return y\n\nout: int = choose(value, 1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(
        rendered.contains("returns `T` where `out` expects `int`")
            || rendered.contains("assigns `T` where `out` expects `int`")
            || rendered.contains("assigns `Union[str, int]` where `out` expects `int`")
            || rendered.contains("call to `choose`"),
        "{rendered}"
    );
}

#[test]
fn check_accepts_generic_function_call_inference_from_multiple_scalar_candidates() {
    let result = check_temp_typepython_source(
        "def pick[T](x: T, y: T) -> T:\n    return x\n\nvalue: int | str = pick(1, \"a\")\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_rejects_generic_function_call_inference_from_result_context_only() {
    let result =
        check_temp_typepython_source("def make[T]() -> T:\n    ...\n\nvalue: int = make()\n");

    let rendered = result.diagnostics.as_text();
    assert!(result.diagnostics.has_errors(), "{rendered}");
    assert!(
        rendered.contains("assigns `T` where `value` expects `int`")
            || rendered.contains("returns `T` where `value` expects `int`"),
        "{rendered}"
    );
}

#[test]
fn check_rejects_generic_function_call_inference_through_invariant_container_candidates() {
    let result = check_temp_typepython_source(
        "def pair[T](x: list[T], y: list[T]) -> list[T]:\n    return x\n\nvalue = pair([1], [\"a\"])\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("call to `pair`"));
}

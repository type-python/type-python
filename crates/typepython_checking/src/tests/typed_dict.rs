use super::*;

#[test]
fn check_reports_duplicate_module_symbols() {
    let graph = ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                declaration! {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
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
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
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
    };

    let result = check(&graph);

    let rendered = result.diagnostics.as_text();
    assert!(result.diagnostics.has_errors());
    assert!(rendered.contains("TPY4004"));
    assert!(rendered.contains("`User`"));
}

#[test]
fn check_reports_missing_required_typed_dict_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\npayload: User = {\"id\": 1}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_conformance_baseline_common_library_stub_surface() {
    let result = check_temp_project_sources(&[
        (
            "numpy/__init__.pyi",
            "numpy",
            SourceKind::Stub,
            "from typing import Any\n\nclass ndarray:\n    def reshape(self, shape: Any) -> ndarray: ...\n\ndef array(value: Any) -> ndarray: ...\n",
        ),
        (
            "torch/__init__.pyi",
            "torch",
            SourceKind::Stub,
            "from typing import Any\n\nclass Tensor:\n    def to(self, device: Any) -> Tensor: ...\n\ndef tensor(value: Any) -> Tensor: ...\n",
        ),
        (
            "app.tpy",
            "app",
            SourceKind::TypePython,
            "from numpy import array, ndarray\nfrom torch import Tensor, tensor\n\nmatrix: ndarray = array([1])\nreshaped: ndarray = matrix.reshape((1,))\nvalue: Tensor = tensor(1).to(\"cpu\")\n",
        ),
    ]);

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_conformance_baseline_higher_order_typing_surface() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable, Protocol\n\n",
        "class Named(Protocol):\n",
        "    @property\n",
        "    def name(self) -> str:\n",
        "        return \"\"\n\n",
        "class IntDescriptor:\n",
        "    def __get__(self, instance, owner) -> int:\n",
        "        return 1\n\n",
        "class FactoryMeta:\n",
        "    def __call__(cls) -> str:\n",
        "        return \"factory\"\n\n",
        "class User:\n",
        "    name: str\n\n",
        "class Box:\n",
        "    value: IntDescriptor\n\n",
        "class Factory(metaclass=FactoryMeta):\n",
        "    pass\n\n",
        "def invoke[**P, R](cb: Callable[P, R], *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "    return cb(*args, **kwargs)\n\n",
        "def greet(name: str, *, times: int) -> str:\n",
        "    return name\n\n",
        "def collect[*Ts](*args: *Ts) -> tuple[*Ts]:\n",
        "    return args\n\n",
        "user = User()\n",
        "box = Box()\n",
        "name: str = invoke(greet, user.name, times=1)\n",
        "count: int = box.value\n",
        "factory_value: str = Factory()\n",
        "items: list[int] = [1, 2]\n",
        "collected: tuple[int, ...] = collect(*items)\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_conformance_baseline_literal_match_exhaustiveness_diagnostic() {
    let result = check_temp_typepython_source(
        "from typing import Literal\n\ndef render(color: Literal[\"red\", \"blue\"]) -> int:\n    match color:\n        case \"red\":\n            return 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4009"), "{rendered}");
    assert!(rendered.contains("missing cases: \"blue\""), "{rendered}");
}

#[test]
fn check_accepts_structural_protocol_property_implementation() {
    let result = check_temp_typepython_source(
        "from typing import Protocol\n\nclass Named(Protocol):\n    @property\n    def name(self) -> str:\n        return \"\"\n\nclass User:\n    name: str\n\ndef greet(value: Named) -> str:\n    return value.name\n\nuser = User()\nmessage: str = greet(user)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_descriptor_backed_attribute_access() {
    let result = check_temp_typepython_source(
        "class IntDescriptor:\n    def __get__(self, instance, owner) -> int:\n        return 1\n\nclass Box:\n    value: IntDescriptor\n\ndef read(box: Box) -> int:\n    return box.value\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_general_metaclass_call_return_type() {
    let result = check_temp_typepython_source(
        "class FactoryMeta:\n    def __call__(cls) -> str:\n        return \"value\"\n\nclass Factory(metaclass=FactoryMeta):\n    pass\n\nvalue: str = Factory()\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_paramspec_overload_forwarding_call() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "overload def invoke[**P](cb: Callable[P, int], *args: P.args, **kwargs: P.kwargs) -> int: ...\n",
        "def invoke(cb, *args, **kwargs):\n",
        "    return cb(*args, **kwargs)\n\n",
        "def greet(name: str, *, times: int) -> int:\n",
        "    return times\n\n",
        "result: int = invoke(greet, \"Ada\", times=1)\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_typevartuple_overload_roundtrip() {
    let result = check_temp_typepython_source(
        "overload def collect[*Ts](*args: *Ts) -> tuple[*Ts]: ...\ndef collect(*args):\n    return args\n\nvalue: tuple[int, str] = collect(1, \"x\")\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_non_exhaustive_literal_match() {
    let result = check_temp_typepython_source(
        "from typing import Literal\n\ndef render(color: Literal[\"red\", \"blue\"]) -> int:\n    match color:\n        case \"red\":\n            return 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4009"), "{rendered}");
    assert!(rendered.contains("literal set"), "{rendered}");
    assert!(rendered.contains("\"blue\""), "{rendered}");
}

#[test]
fn check_accepts_total_false_typed_dict_missing_keys() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict, total=False):\n    id: int\n    name: str\n\npayload: User = {}\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_unknown_typed_dict_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n\npayload: User = {\"id\": 1, \"name\": \"Ada\"}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("unknown key `name`"));
}

#[test]
fn check_accepts_typed_dict_extra_items_literal_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict, extra_items=int):\n    id: int\n\npayload: User = {\"id\": 1, \"age\": 2}\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_incompatible_typed_dict_value() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n\npayload: User = {\"id\": \"oops\"}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("assigns `str` to key `id`"));
}

#[test]
fn check_reports_incompatible_typed_dict_extra_items_value() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict, extra_items=int):\n    id: int\n\npayload: User = {\"id\": 1, \"age\": \"old\"}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("assigns `str` to key `age`"));
}

#[test]
fn check_reports_typed_dict_collector_diagnostics_with_source_overrides_without_backing_file() {
    let result = check_virtual_source_with_overrides(
        "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict):\n    name: ReadOnly[str]\n\npayload: User = {\"name\": 1}\n\ndef mutate(user: User) -> None:\n    user[\"name\"] = \"Grace\"\n",
        ParseOptions::default(),
        false,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"), "{rendered}");
    assert!(rendered.contains("assigns `int` to key `name`"), "{rendered}");
    assert!(rendered.contains("TPY4016"), "{rendered}");
    assert!(rendered.contains("read-only and cannot be assigned"), "{rendered}");
}

#[test]
fn check_reports_invalid_typed_dict_expansion() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n\nclass Extra(TypedDict):\n    name: str\n\nextra: Extra = {\"name\": \"Ada\"}\npayload: User = {**extra}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("invalid `**` expansion"));
}

#[test]
fn check_reports_open_typed_dict_expansion() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n\nclass Extra(TypedDict):\n    id: int\n\ndef make_extra() -> Extra:\n    return {\"id\": 1}\n\npayload: User = {**make_extra()}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("cannot expand open TypedDict `Extra`"));
}

#[test]
fn check_accepts_closed_typed_dict_expansion() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n\nclass Extra(TypedDict, closed=True):\n    id: int\n\ndef make_extra() -> Extra:\n    return {\"id\": 1}\n\npayload: User = {**make_extra()}\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_contextual_typed_dict_literal_call_argument() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef takes_user(user: User) -> None:\n    return None\n\ntakes_user({\"id\": 1, \"name\": \"Ada\"})\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_typed_dict_literal_missing_key_in_call_argument() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef takes_user(user: User) -> None:\n    return None\n\ntakes_user({\"id\": 1})\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_reports_contextual_typed_dict_literal_keyword_value_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef takes_user(*, user: User) -> None:\n    return None\n\ntakes_user(user={\"id\": \"oops\", \"name\": \"Ada\"})\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("assigns `str` to key `id`"));
}

#[test]
fn check_accepts_contextual_typed_dict_literal_method_keyword_argument() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\nclass Service:\n    def takes(self, *, user: User) -> None:\n        return None\n\nService().takes(user={\"id\": 1, \"name\": \"Ada\"})\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_overload_with_contextual_typed_dict_literal_argument() {
    fn direct_expr(value_type: &str) -> typepython_syntax::DirectExprMetadata {
        typepython_syntax::DirectExprMetadata::from_type_text(value_type)
    }

    let call = typepython_binding::CallSite {
        callee: String::from("choose"),
        arg_count: 1,
        arg_values: vec![typepython_syntax::DirectExprMetadata {
            value_type_expr: Some(typepython_syntax::TypeExpr::Generic {
                head: String::from("dict"),
                args: vec![
                    typepython_syntax::TypeExpr::Name(String::from("str")),
                    typepython_syntax::TypeExpr::Name(String::from("object")),
                ],
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
            value_dict_entries: Some(vec![
                typepython_syntax::TypedDictLiteralEntry {
                    key: Some(String::from("id")),
                    key_value: Some(Box::new(direct_expr("str"))),
                    is_expansion: false,
                    value: direct_expr("int"),
                },
                typepython_syntax::TypedDictLiteralEntry {
                    key: Some(String::from("name")),
                    key_value: Some(Box::new(direct_expr("str"))),
                    is_expansion: false,
                    value: direct_expr("str"),
                },
            ]),
        }],
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };
    let typed_dict_overload = declaration! {
        metadata: callable_metadata("(user:User)->int"),
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
    let typed_dict_overload = normalize_test_declaration(&typed_dict_overload);
    let string_overload = declaration! {
        metadata: callable_metadata("(user:str)->str"),
        ..typed_dict_overload.clone()
    };
    let string_overload = normalize_test_declaration(&string_overload);
    let typed_dict_class = declaration! {
        metadata: class_metadata(&["TypedDict"]),
        name: String::from("User"),
        kind: DeclarationKind::Class,
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
        bases: vec![String::from("TypedDict")],
        type_params: Vec::new(),
    };
    let id_field = declaration! {
        metadata: value_metadata("int"),
        name: String::from("id"),
        kind: DeclarationKind::Value,
        value_type_expr: None,
        method_kind: None,
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
    };
    let name_field = declaration! {
        metadata: value_metadata("str"),
        name: String::from("name"),
        ..id_field.clone()
    };
    let node = typepython_graph::ModuleNode {
        module_path: PathBuf::from("src/app/module.tpy"),
        module_key: String::new(),
        module_kind: SourceKind::TypePython,
        declarations: vec![typed_dict_class, id_field, name_field],
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
    let node = normalize_test_graph(&typepython_graph::ModuleGraph { nodes: vec![node] })
        .nodes
        .into_iter()
        .next()
        .expect("normalized node");

    assert!(crate::overload_is_applicable_with_context(
        &node,
        std::slice::from_ref(&node),
        &call,
        &typed_dict_overload
    ));
    assert!(!crate::overload_is_applicable_with_context(
        &node,
        std::slice::from_ref(&node),
        &call,
        &string_overload
    ));
}

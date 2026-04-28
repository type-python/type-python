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
fn check_reports_tpy4014_for_unresolved_paramspec_call() {
    let result = check_temp_typepython_source(
        "from typing import Callable, ParamSpec\n\nP = ParamSpec(\"P\")\n\ndef invoke(cb: Callable[P, int]) -> int:\n    return cb(1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4014"), "{rendered}");
}

#[test]
fn check_accepts_source_authored_paramspec_forwarding_call() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable, cast\n\n",
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
        "from typing import Callable, cast\n\n",
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
fn check_warns_for_ignored_must_use_result() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "def must_use[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@must_use\n",
            "def make_task() -> int:\n",
            "    return 1\n\n",
            "def run() -> None:\n",
            "    make_task()\n",
            "    value: int = make_task()\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4022"), "{rendered}");
    assert!(rendered.contains("@must_use"), "{rendered}");
    assert!(rendered.contains("make_task"), "{rendered}");
    assert!(!rendered.contains("TPY4001"), "{rendered}");
}

#[test]
fn check_reports_unsupported_framework_transform_provider_in_strict_mode() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "@framework_transform(kind=\"function_to_object_decorator\")\n",
            "def celery_task(fn):\n",
            "    return fn\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4020"), "{rendered}");
    assert!(rendered.contains("celery_task"), "{rendered}");
    assert!(
        rendered.contains("does not advertise a supported static capability set"),
        "{rendered}"
    );
}

#[test]
fn check_accepts_supported_framework_class_shape_provider_in_strict_mode() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "def framework_transform(*args, **kwargs):\n",
            "    def wrap(obj):\n",
            "        return obj\n",
            "    return wrap\n\n",
            "@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\"))\n",
            "def model(cls):\n",
            "    return cls\n\n",
            "@model\n",
            "class User:\n",
            "    name: str\n\n",
            "user: User = User(\"Ada\")\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4020"), "{rendered}");
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn framework_class_shape_provider_emits_synthetic_init_stub() {
    let source_text = concat!(
        "def framework_transform(*args, **kwargs):\n",
        "    def wrap(obj):\n",
        "        return obj\n",
        "    return wrap\n\n",
        "@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\"))\n",
        "def model(cls):\n",
        "    return cls\n\n",
        "@model\n",
        "class User:\n",
        "    name: str\n",
        "    age: int = 1\n",
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
    let methods = crate::collect_synthetic_method_stubs(&graph);

    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].owner_type_name, "User");
    assert_eq!(methods[0].name, "__init__");
    assert_eq!(methods[0].params[1].name, "name");
    assert_eq!(methods[0].params[1].annotation.as_deref(), Some("str"));
    assert!(!methods[0].params[1].has_default);
    assert_eq!(methods[0].params[2].name, "age");
    assert_eq!(methods[0].params[2].annotation.as_deref(), Some("int"));
    assert!(methods[0].params[2].has_default);
}

#[test]
fn pydantic_like_base_model_provider_emits_field_alias_and_default_init_stub() {
    let source_text = concat!(
        "def framework_transform(*args, **kwargs):\n",
        "    def wrap(obj):\n",
        "        return obj\n",
        "    return wrap\n\n",
        "def Field(*, default=None, default_factory=None, alias=None):\n",
        "    return default\n\n",
        "@framework_transform(kind=\"base_class\", capabilities=(\"field_collection\", \"constructor_generation\", \"alias_handling\", \"required_optional_fields\"))\n",
        "class BaseModel:\n",
        "    pass\n\n",
        "class User(BaseModel):\n",
        "    id: int = Field(alias=\"user_id\")\n",
        "    name: str = Field(default=\"Ada\")\n",
        "    tags: object = Field(default_factory=list)\n",
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
    let methods = crate::collect_synthetic_method_stubs(&graph);
    let user_init = methods
        .iter()
        .find(|method| method.owner_type_name == "User" && method.name == "__init__")
        .expect("expected synthetic User.__init__ stub");

    assert_eq!(user_init.params[1].name, "user_id");
    assert_eq!(user_init.params[1].annotation.as_deref(), Some("int"));
    assert!(!user_init.params[1].has_default);
    assert_eq!(user_init.params[2].name, "name");
    assert_eq!(user_init.params[2].annotation.as_deref(), Some("str"));
    assert!(user_init.params[2].has_default);
    assert_eq!(user_init.params[3].name, "tags");
    assert_eq!(user_init.params[3].annotation.as_deref(), Some("object"));
    assert!(user_init.params[3].has_default);
}

#[test]
fn pydantic_like_base_model_provider_emits_model_construct_stub() {
    let source_text = concat!(
        "def framework_transform(*args, **kwargs):\n",
        "    def wrap(obj):\n",
        "        return obj\n",
        "    return wrap\n\n",
        "@framework_transform(kind=\"base_class\", capabilities=(\"field_collection\", \"constructor_generation\", \"method_synthesis\"))\n",
        "class BaseModel:\n",
        "    pass\n\n",
        "class User(BaseModel):\n",
        "    name: str\n",
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
    let methods = crate::collect_synthetic_method_stubs(&graph);
    let construct = methods
        .iter()
        .find(|method| method.owner_type_name == "User" && method.name == "model_construct")
        .expect("expected synthetic User.model_construct stub");

    assert_eq!(construct.method_kind, typepython_syntax::MethodKind::Class);
    assert_eq!(construct.returns.as_deref(), Some("User"));
    assert_eq!(construct.params[0].name, "cls");
    assert_eq!(construct.params[1].name, "_fields_set");
    assert_eq!(construct.params[1].annotation.as_deref(), Some("set[str] | None"));
    assert!(construct.params[1].has_default);
    assert_eq!(construct.params[2].name, "values");
    assert_eq!(construct.params[2].annotation.as_deref(), Some("object"));
    assert!(construct.params[2].keyword_variadic);
}

#[test]
fn pydantic_like_computed_field_emits_value_stub_override() {
    let source_text = concat!(
        "def framework_transform(*args, **kwargs):\n",
        "    def wrap(obj):\n",
        "        return obj\n",
        "    return wrap\n\n",
        "def computed_field(fn):\n",
        "    return fn\n\n",
        "@framework_transform(kind=\"base_class\", capabilities=(\"field_collection\", \"constructor_generation\", \"method_synthesis\"))\n",
        "class BaseModel:\n",
        "    pass\n\n",
        "class User(BaseModel):\n",
        "    name: str\n\n",
        "    @computed_field\n",
        "    def display_name(self) -> str:\n",
        "        return self.name\n",
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
    let overrides = crate::collect_effective_value_stub_overrides(&graph);
    let display_name = overrides
        .iter()
        .find(|override_value| override_value.annotation == "str")
        .expect("computed_field should be emitted as a value stub override");

    assert_eq!(display_name.module_key, "app");

    let result = check_temp_typepython_source_with_check_options(
        source_text,
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );
    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4001"), "{rendered}");
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_pydantic_like_validator_and_serializer_decorators() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "def framework_transform(*args, **kwargs):\n",
            "    def wrap(obj):\n",
            "        return obj\n",
            "    return wrap\n\n",
            "def field_validator(*fields, **kwargs):\n",
            "    def wrap(fn):\n",
            "        return fn\n",
            "    return wrap\n\n",
            "def model_validator(*args, **kwargs):\n",
            "    def wrap(fn):\n",
            "        return fn\n",
            "    return wrap\n\n",
            "def field_serializer(*fields, **kwargs):\n",
            "    def wrap(fn):\n",
            "        return fn\n",
            "    return wrap\n\n",
            "def model_serializer(*args, **kwargs):\n",
            "    def wrap(fn):\n",
            "        return fn\n",
            "    return wrap\n\n",
            "@framework_transform(kind=\"base_class\", capabilities=(\"field_collection\", \"constructor_generation\", \"method_synthesis\"))\n",
            "class BaseModel:\n",
            "    pass\n\n",
            "class User(BaseModel):\n",
            "    name: str\n\n",
            "    @field_validator(\"name\")\n",
            "    def validate_name(cls, value: str) -> str:\n",
            "        return value\n\n",
            "    @model_validator(mode=\"after\")\n",
            "    def validate_model(self) -> User:\n",
            "        return self\n\n",
            "    @field_serializer(\"name\")\n",
            "    def serialize_name(self, value: str) -> str:\n",
            "        return value\n\n",
            "    @model_serializer(mode=\"plain\")\n",
            "    def serialize_model(self) -> dict[str, object]:\n",
            "        return {}\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4001"), "{rendered}");
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_malformed_pydantic_like_validator_decorator_signature() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "def framework_transform(*args, **kwargs):\n",
            "    def wrap(obj):\n",
            "        return obj\n",
            "    return wrap\n\n",
            "def field_validator(*fields, **kwargs):\n",
            "    def wrap(fn):\n",
            "        return fn\n",
            "    return wrap\n\n",
            "@framework_transform(kind=\"base_class\", capabilities=(\"field_collection\", \"constructor_generation\", \"method_synthesis\"))\n",
            "class BaseModel:\n",
            "    pass\n\n",
            "class User(BaseModel):\n",
            "    name: str\n\n",
            "    @field_validator(\"name\")\n",
            "    def validate_name(cls):\n",
            "        return cls\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4020"), "{rendered}");
    assert!(rendered.contains("field_validator"), "{rendered}");
    assert!(rendered.contains("expected at least 2 explicit parameters"), "{rendered}");
    assert!(rendered.contains("expected an explicit return annotation"), "{rendered}");
}

#[test]
fn check_reports_pydantic_like_dynamic_field_alias() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "def framework_transform(*args, **kwargs):\n",
            "    def wrap(obj):\n",
            "        return obj\n",
            "    return wrap\n\n",
            "def Field(*, default=None, default_factory=None, alias=None):\n",
            "    return default\n\n",
            "ALIAS: str = \"user_id\"\n\n",
            "@framework_transform(kind=\"base_class\", capabilities=(\"field_collection\", \"constructor_generation\", \"alias_handling\", \"required_optional_fields\"))\n",
            "class BaseModel:\n",
            "    pass\n\n",
            "class User(BaseModel):\n",
            "    id: int = Field(alias=ALIAS)\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4021"), "{rendered}");
    assert!(rendered.contains("dynamic alias"), "{rendered}");
    assert!(rendered.contains("id"), "{rendered}");
}

#[test]
fn framework_method_synthesis_provider_emits_synthetic_value_stubs() {
    let source_text = concat!(
        "def framework_transform(*args, **kwargs):\n",
        "    def wrap(obj):\n",
        "        return obj\n",
        "    return wrap\n\n",
        "@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\", \"method_synthesis\"))\n",
        "def model(cls):\n",
        "    return cls\n\n",
        "@model\n",
        "class User:\n",
        "    name: str\n",
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
    let values = crate::collect_synthetic_value_stubs(&graph);

    assert_eq!(values.len(), 3);
    assert_eq!(values[0].owner_type_name, "User");
    assert_eq!(values[0].name, "metadata");
    assert_eq!(values[0].annotation, "dict[str, object]");
    assert_eq!(values[1].name, "objects");
    assert_eq!(values[1].annotation, "object");
    assert_eq!(values[2].name, "validators");
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
fn decorated_function_transform_can_resolve_non_callable_object_surface() {
    let source_text = concat!(
        "from typing import Callable, cast\n\n",
        "class Task[**P, R]:\n",
        "    def delay(self, *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "        ...\n\n",
        "def task[**P, R](fn: Callable[P, R]) -> Task[P, R]:\n",
        "    return cast(Task[P, R], Task())\n\n",
        "@task\n",
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
    let context = crate::CheckerContext::new_with_bound_surface_facts_and_strict(
        &graph.nodes,
        ImportFallback::Unknown,
        None,
        None,
        true,
    );

    assert_eq!(
        crate::resolve_decorated_function_callable_semantic_type_with_context(
            &context,
            node,
            &graph.nodes,
            "count",
        )
        .as_ref()
        .map(crate::diagnostic_type_text),
        Some(String::from("Task[[int], int]"))
    );
    let overrides = crate::collect_effective_value_stub_overrides(&graph);
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].annotation, "Task[[int], int]");
}

#[test]
fn framework_marked_function_to_object_decorator_uses_existing_stub_transform() {
    let source_text = concat!(
        "from typing import Callable, cast\n\n",
        "class Task[**P, R]:\n",
        "    def delay(self, *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "        ...\n\n",
        "@framework_transform(kind=\"function_to_object_decorator\", capabilities=(\"function_to_object_replacement\", \"generic_preservation\"))\n",
        "def task[**P, R](fn: Callable[P, R]) -> Task[P, R]:\n",
        "    return cast(Task[P, R], Task())\n\n",
        "@task\n",
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
    let result = check_with_options(
        &graph,
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
        ImportFallback::Unknown,
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
    let overrides = crate::collect_effective_value_stub_overrides(&graph);
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].annotation, "Task[[int], int]");
}

#[test]
fn decorated_function_transform_substitutes_multi_arg_paramspec_in_object_surface() {
    let source_text = concat!(
        "from typing import Callable, cast\n\n",
        "class Task[**P, R]:\n",
        "    def delay(self, *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "        ...\n\n",
        "def task[**P, R](fn: Callable[P, R]) -> Task[P, R]:\n",
        "    return cast(Task[P, R], Task())\n\n",
        "@task\n",
        "def combine(left: int, right: str) -> int:\n",
        "    return left\n",
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
    let overrides = crate::collect_effective_value_stub_overrides(&graph);

    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].annotation, "Task[[int, str], int]");
}

#[test]
fn decorated_method_transform_emits_value_stub_override() {
    let source_text = concat!(
        "from typing import Callable, cast\n\n",
        "class Task[**P, R]:\n",
        "    def delay(self, *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "        ...\n\n",
        "def task[**P, R](fn: Callable[P, R]) -> Task[P, R]:\n",
        "    return cast(Task[P, R], Task())\n\n",
        "class Worker:\n",
        "    @task\n",
        "    def run(self, name: str) -> int:\n",
        "        return len(name)\n",
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
    let overrides = crate::collect_effective_value_stub_overrides(&graph);

    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].annotation, "Task[[dynamic, str], int]");
}

#[test]
fn decorated_async_function_transform_preserves_awaitable_result_surface() {
    let source_text = concat!(
        "from typing import Callable, cast\n\n",
        "class Task[**P, R]:\n",
        "    def delay(self, *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "        ...\n\n",
        "def task[**P, R](fn: Callable[P, R]) -> Task[P, R]:\n",
        "    return cast(Task[P, R], Task())\n\n",
        "@task\n",
        "async def fetch(user_id: int) -> str:\n",
        "    return \"ok\"\n",
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
    let overrides = crate::collect_effective_value_stub_overrides(&graph);

    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].annotation, "Task[[int], Awaitable[str]]");
}

#[test]
fn decorated_generic_function_transform_preserves_typevar_result_surface() {
    let source_text = concat!(
        "from typing import Callable, cast\n\n",
        "class Task[**P, R]:\n",
        "    def delay(self, *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "        ...\n\n",
        "def task[**P, R](fn: Callable[P, R]) -> Task[P, R]:\n",
        "    return cast(Task[P, R], Task())\n\n",
        "@task\n",
        "def echo[T](value: T) -> T:\n",
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
    let overrides = crate::collect_effective_value_stub_overrides(&graph);

    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].annotation, "Task[[T], T]");
}

#[test]
fn check_accepts_function_to_object_decorator_transform_member_calls() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable, cast\n\n",
        "class Task[**P, R]:\n",
        "    def delay(self, *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "        ...\n\n",
        "def task[**P, R](fn: Callable[P, R]) -> Task[P, R]:\n",
        "    return cast(Task[P, R], Task())\n\n",
        "@task\n",
        "def count(value: int) -> int:\n",
        "    return value\n\n",
        "result: int = count.delay(1)\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
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
fn check_accepts_non_callable_decorator_transform_in_strict_mode_when_static_type_is_known() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable, cast\n\n",
            "class Route:\n    pass\n\n",
            "def route(fn: Callable[[int], int]) -> Route:\n",
            "    return cast(Route, Route())\n\n",
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

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
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

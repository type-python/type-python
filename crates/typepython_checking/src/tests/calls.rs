use super::*;

fn check_temp_typepython_framework_source(source_text: &str) -> crate::CheckResult {
    check_temp_typepython_source_with_checker_options(
        source_text,
        ParseOptions::default(),
        crate::CheckerOptions::permissive_test_default().with_framework_adapters(true),
    )
}

#[test]
fn check_substitutes_source_authored_paramspec_in_return_type() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def identity[**P, R](cb: Callable[P, R]) -> Callable[P, R]:\n",
        "    return cb\n\n",
        "def greet(name: str) -> str:\n",
        "    return name\n\n",
        "handler: Callable[[str], str] = identity(greet)\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_keyword_and_default_arguments_in_direct_calls() {
    let result = check_temp_typepython_source(
        "def field(default=None, init=True, kw_only=False):\n    return default\n\nfield(default=\"Ada\", init=False)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_mismatched_numeric_and_bytes_literals() {
    let result = check_temp_typepython_source(concat!(
        "def takes_int(value: int) -> int:\n",
        "    return value\n\n",
        "integer: int = 1.5\n",
        "text: str = b\"bytes\"\n",
        "takes_int(-3.5)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(result.diagnostics.has_errors(), "{rendered}");
    assert!(rendered.contains("assigns `float` where `integer` expects `int`"), "{rendered}");
    assert!(rendered.contains("assigns `bytes` where `text` expects `str`"), "{rendered}");
    assert!(
        rendered.contains("call to `takes_int`")
            && rendered.contains("passes `float` where parameter expects `int`"),
        "{rendered}"
    );
}

#[test]
fn check_decorated_callable_transform_honors_strict_nulls_option() {
    let result = check_temp_typepython_source_with_checker_options(
        concat!(
            "from typing import Callable\n\n",
            "def keep(fn: Callable[[int], int]) -> Callable[[int], int]:\n",
            "    return fn\n\n",
            "@keep\n",
            "def maybe(value: int) -> None:\n",
            "    return None\n",
        ),
        ParseOptions::default(),
        crate::CheckerOptions {
            strict: true,
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_callable_assignment_uses_decorated_function_params() {
    let result = check_virtual_binding_metadata_source(concat!(
        "from typing import Callable, cast\n\n",
        "def widen(fn: Callable[[str], str]) -> Callable[[object], str]:\n",
        "    return cast(Callable[[object], str], fn)\n\n",
        "@widen\n",
        "def takes_str(value: str) -> str:\n",
        "    return value\n\n",
        "handler: Callable[[object], str] = takes_str\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_callable_assignment_uses_decorated_member_params() {
    let source_text = concat!(
        "from typing import Callable, cast\n\n",
        "def widen(fn: Callable[[object, str], str]) -> Callable[[object, object], str]:\n",
        "    return cast(Callable[[object, object], str], fn)\n\n",
        "class Worker:\n",
        "    @widen\n",
        "    def takes_str(self, value: str) -> str:\n",
        "        return value\n\n",
        "handler: Callable[[object], str] = Worker.takes_str\n",
    );
    let source = SourceFile {
        path: PathBuf::from("virtual/app.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::from("app"),
        text: source_text.to_owned(),
    };
    let tree = parse_with_options(source, ParseOptions::default());
    let binding = bind(&tree);
    let bound_surface_facts =
        BTreeMap::from([(binding.module_key.clone(), binding.surface_facts.clone())]);
    let graph = build(std::slice::from_ref(&binding));
    let node = &graph.nodes[0];
    let context = crate::CheckerContext::new_with_bound_surface_facts_and_options(
        &graph.nodes,
        None,
        Some(&bound_surface_facts),
        crate::CheckerOptions::permissive_test_default(),
    );
    let assignment = typepython_binding::AssignmentSite {
        call_source_range: None,
        name: String::from("handler"),
        destructuring_target_names: None,
        destructuring_index: None,
        annotation: Some(String::from("Callable[[object], str]")),
        annotation_expr: None,
        is_awaited: false,
        value_callee: None,
        value_name: None,
        value_member_owner_name: Some(String::from("Worker")),
        value_member_name: Some(String::from("takes_str")),
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
    };

    assert!(
        matches!(
            crate::callable_assignment_result(
                &context,
                node,
                &graph.nodes,
                &assignment,
                "Callable[[object], str]",
            ),
            Some(None)
        ),
        "decorated member callable signature should satisfy the assignment"
    );
}

#[test]
fn check_reports_positional_only_parameter_passed_as_keyword() {
    let result =
        check_temp_typepython_source("def takes(x: int, /):\n    return x\n\ntakes(x=1)\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("positional-only parameter `x`"));
}

#[test]
fn check_accepts_keyword_constructor_calls_for_explicit_init() {
    let result = check_temp_typepython_source(
        "class User:\n    def __init__(self, age: int, name: str = \"Ada\"):\n        self.age = age\n        self.name = name\n\nUser(age=1)\nUser(age=1, name=\"Grace\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_positional_only_constructor_parameter_passed_as_keyword() {
    let result = check_temp_typepython_source(
        "class User:\n    def __init__(self, age: int, /):\n        self.age = age\n\nUser(age=1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("positional-only parameter `age`"));
}

#[test]
fn check_reports_incomplete_conditional_return_coverage() {
    let result = check_temp_typepython_source_with_options(
        "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n",
        ParseOptions { enable_conditional_returns: true, ..ParseOptions::default() },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4018"));
    assert!(rendered.contains("missing: None"));
}

#[test]
fn check_accepts_complete_conditional_return_coverage() {
    let result = check_temp_typepython_source_with_options(
        "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n",
        ParseOptions { enable_conditional_returns: true, ..ParseOptions::default() },
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4018"));
    assert!(!result.diagnostics.has_errors());
}

#[test]
fn check_accepts_dataclass_transform_decorator_constructor_call() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n    age: int\n\nuser: User = User(\"Ada\", 1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_dataclass_transform_base_class_constructor_call() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\nclass ModelBase:\n    pass\n\nclass User(ModelBase):\n    name: str\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_dataclass_transform_metaclass_constructor_call() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\nclass ModelMeta:\n    pass\n\nclass User(metaclass=ModelMeta):\n    name: str\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_framework_class_decorator_constructor_call() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\"))\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n    age: int = 1\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_requires_framework_adapters_gate_for_framework_constructor_shape() {
    let result = check_temp_typepython_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\"))\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
}

#[test]
fn check_accepts_framework_base_class_constructor_call() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@framework_transform(kind=\"base_class\", capabilities=(\"field_collection\", \"constructor_generation\"))\nclass ModelBase:\n    pass\n\nclass User(ModelBase):\n    name: str\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_pydantic_like_base_model_field_constructor_call() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\ndef Field(*, default=None, default_factory=None, alias=None):\n    return default\n\n@framework_transform(kind=\"base_class\", capabilities=(\"field_collection\", \"constructor_generation\", \"alias_handling\", \"required_optional_fields\"))\nclass BaseModel:\n    pass\n\nclass User(BaseModel):\n    id: int = Field(alias=\"user_id\")\n    name: str = Field(default=\"Ada\")\n    tags: object = Field(default_factory=list)\n\nuser: User = User(user_id=1)\nuser_with_defaults: User = User(user_id=1, name=\"Grace\", tags=object())\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_pydantic_like_base_model_missing_required_alias_field() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\ndef Field(*, default=None, default_factory=None, alias=None):\n    return default\n\n@framework_transform(kind=\"base_class\", capabilities=(\"field_collection\", \"constructor_generation\", \"alias_handling\", \"required_optional_fields\"))\nclass BaseModel:\n    pass\n\nclass User(BaseModel):\n    id: int = Field(alias=\"user_id\")\n    name: str = Field(default=\"Ada\")\n\nuser: User = User()\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(
        rendered.contains("missing required synthesized dataclass-transform field(s): user_id"),
        "{rendered}"
    );
}

#[test]
fn check_accepts_framework_metaclass_constructor_call() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@framework_transform(kind=\"metaclass\", capabilities=(\"field_collection\", \"constructor_generation\"))\nclass ModelMeta:\n    pass\n\nclass User(metaclass=ModelMeta):\n    name: str\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_framework_class_decorator_constructor_type_mismatch() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\"))\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    age: int\n\nuser: User = User(\"oops\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("synthesized dataclass-transform field `age` expects `int`"));
    let diagnostic = result
        .diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.message.contains("synthesized dataclass-transform field `age` expects `int`")
        })
        .expect("expected synthesized framework field diagnostic");
    assert_eq!(diagnostic.span.as_ref().map(|span| span.line), Some(10));
}

#[test]
fn check_accepts_framework_field_alias_kw_only_and_init_exclusion() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\ndef field(*, default=None, default_factory=None, init=True, kw_only=False, alias=None):\n    return default\n\n@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\", \"alias_handling\", \"required_optional_fields\"))\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    id: int = field(alias=\"user_id\")\n    name: str = field(kw_only=True)\n    cache: str = field(init=False)\n\nuser: User = User(1, name=\"Ada\")\nuser_alias: User = User(user_id=1, name=\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_framework_keyword_only_field_passed_positionally() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\ndef field(*, kw_only=False):\n    return None\n\n@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\", \"required_optional_fields\"))\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    id: int\n    name: str = field(kw_only=True)\n\nuser: User = User(1, \"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("expects at most 1 positional argument(s)"), "{rendered}");
}

#[test]
fn check_reports_framework_readonly_field_assignment_after_init() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\", \"readonly_fields\"), frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\nuser: User = User(\"Ada\")\nuser.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("frozen dataclass-transform field `name`"), "{rendered}");
}

#[test]
fn check_reports_pydantic_like_field_level_frozen_assignment_after_init() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\ndef Field(*, default=None, default_factory=None, alias=None, frozen=False):\n    return default\n\n@framework_transform(kind=\"base_class\", capabilities=(\"field_collection\", \"constructor_generation\", \"alias_handling\", \"required_optional_fields\", \"readonly_fields\"))\nclass BaseModel:\n    pass\n\nclass User(BaseModel):\n    id: int = Field(alias=\"user_id\", frozen=True)\n    name: str\n\nuser: User = User(user_id=1, name=\"Ada\")\nuser.id = 2\nuser.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("frozen dataclass-transform field `id`"), "{rendered}");
    assert!(!rendered.contains("frozen dataclass-transform field `name`"), "{rendered}");
}

#[test]
fn check_excludes_descriptor_defaults_from_framework_fields_when_advertised() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\nclass Descriptor:\n    def __get__(self, instance, owner):\n        return 0\n\n@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\", \"descriptor_backed_attributes\"))\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: int = Descriptor()\n\nuser: User = User()\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_framework_generated_class_attributes_when_advertised() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\", \"method_synthesis\"))\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\nmanager: object = User.objects\nmetadata: dict[str, object] = User.metadata\nvalidators: dict[str, object] = User.validators\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_framework_generated_class_attribute_type_mismatch() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\", \"method_synthesis\"))\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\nmetadata: int = User.metadata\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("dict[str, object]"), "{rendered}");
}

#[test]
fn check_reports_framework_generated_class_attribute_without_capability() {
    let result = check_temp_typepython_framework_source(
        "def framework_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@framework_transform(kind=\"class_decorator\", capabilities=(\"field_collection\", \"constructor_generation\"))\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\nmanager = User.objects\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `objects`"), "{rendered}");
    let diagnostic = result
        .diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("has no member `objects`"))
        .expect("expected framework generated member diagnostic");
    assert_eq!(diagnostic.span.as_ref().map(|span| span.line), Some(10));
}

#[test]
fn check_reports_dataclass_transform_constructor_arity_mismatch() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n    age: int\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("missing required synthesized dataclass-transform field(s): age"));
}

#[test]
fn check_reports_dataclass_transform_constructor_type_mismatch() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    age: int\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("synthesized dataclass-transform field `age` expects `int`"));
}

#[test]
fn check_accepts_dataclass_transform_constructor_none_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    age: int\n\nuser: User = User(None)\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_rejects_dataclass_transform_constructor_tainted_keyword_when_taint_is_enabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\nraw: Tainted[str, \"html\"]\nuser: User = User(name=raw)\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            experimental_taint: true,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("Tainted[str, \"html\"]"), "{rendered}");
    assert!(rendered.contains("synthesized keyword `name`"), "{rendered}");
}

#[test]
fn check_reports_dataclass_transform_constructor_keyword_type_mismatch() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    age: int\n\nuser: User = User(age=\"oops\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("synthesized keyword `age`"));
    assert!(rendered.contains("expects `int`"));
}

#[test]
fn check_reports_dataclass_transform_constructor_duplicate_binding() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    age: int\n\nuser: User = User(1, age=2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("binds synthesized field `age` both positionally and by keyword"));
}

#[test]
fn check_accepts_dataclass_transform_default_and_classvar_fields() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    role: ClassVar[str]\n    name: str\n    age: int = 1\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_plain_dataclass_constructor_arguments() {
    let result = check_temp_typepython_source(
        "@dataclass\nclass User:\n    name: str\n    age: int = 1\n\nUser(\"Ada\")\nUser(\"Ada\", 2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_plain_frozen_dataclass_field_assignment_after_init() {
    let result = check_temp_typepython_source(
        "@dataclass(frozen=True)\nclass User:\n    name: str\n\nuser = User(\"Ada\")\nuser.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("frozen dataclass field `name`"));
}

#[test]
fn check_reports_plain_kw_only_dataclass_positional_call() {
    let result = check_temp_typepython_source(
        "@dataclass(kw_only=True)\nclass User:\n    name: str\n\nUser(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("expects at most 0 positional argument(s) but received 1"));
}

#[test]
fn check_accepts_dataclass_transform_inherited_fields() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass Base:\n    name: str\n\nclass User(Base):\n    age: int\n\nuser: User = User(\"Ada\", 1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_inherited_dataclass_transform_kw_only_defaults() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\ndef field(*, default=None, kw_only=False, init=True):\n    return default\n\n@dataclass_transform(field_specifiers=(field,), kw_only_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass Base:\n    age: int\n\nclass User(Base):\n    name: str\n\nUser(name=\"Ada\", age=1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_excludes_descriptor_defaults_from_dataclass_transform_fields() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\nclass Descriptor:\n    def __get__(self, instance, owner):\n        return 0\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: int = Descriptor()\n\nuser: User = User()\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_frozen_dataclass_transform_assignment_in_init() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\n    def __init__(self, name: str):\n        self.name = name\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("frozen dataclass-transform field"), "{rendered}");
}

#[test]
fn check_reports_frozen_dataclass_transform_field_assignment_after_init() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\nuser: User = User(\"Ada\")\nuser.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("frozen dataclass-transform field `name`"));
}

#[test]
fn check_reports_frozen_dataclass_transform_field_assignment_after_init_with_explicit_init() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\n    def __init__(self, name: str):\n        self.name = name\n\nuser: User = User(\"Ada\")\nuser.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("frozen dataclass-transform field `name`"));
}

#[test]
fn check_reports_frozen_field_diagnostics_with_source_overrides_without_backing_file() {
    let result = check_virtual_source_with_overrides(
        "from dataclasses import dataclass\n\n@dataclass(frozen=True)\nclass User:\n    name: str\n\nuser = User(\"Ada\")\nuser.name = \"Grace\"\n",
        ParseOptions::default(),
        false,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("frozen dataclass field `name`"), "{rendered}");
}

#[test]
fn check_reports_frozen_dataclass_transform_augmented_assignment_after_init() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    count: int\n\nuser: User = User(1)\nuser.count += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("augmented assignment after initialization"));
}

#[test]
fn check_reports_readonly_typed_dict_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    user[\"name\"] = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"));
    assert!(rendered.contains("cannot be assigned"));
}

#[test]
fn check_reports_qualified_readonly_typed_dict_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nimport typing_extensions\n\nclass User(TypedDict):\n    name: typing_extensions.ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    user[\"name\"] = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"), "{rendered}");
    assert!(rendered.contains("cannot be assigned"), "{rendered}");
}

#[test]
fn check_accepts_writable_typed_dict_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"name\"] = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_writable_typed_dict_extra_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict, extra_items=int):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"age\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_writable_typed_dict_item_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"name\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where `name` expects `str`"));
}

#[test]
fn check_suggests_nearest_typed_dict_key_for_item_typo() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"nmae\"] = \"Grace\"\n",
    );

    let diagnostic = result
        .diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("item `nmae`"))
        .expect("unknown item diagnostic should be emitted");
    assert_eq!(diagnostic.suggestions.len(), 1);
    assert_eq!(diagnostic.suggestions[0].replacement, "name");
}

#[test]
fn check_reports_readonly_typed_dict_extra_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict, extra_items=ReadOnly[int]):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"age\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"));
    assert!(rendered.contains("cannot be assigned"));
}

#[test]
fn check_reports_qualified_readonly_typed_dict_extra_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nimport typing_extensions\n\nclass User(TypedDict, extra_items=typing_extensions.ReadOnly[int]):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"age\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"), "{rendered}");
    assert!(rendered.contains("cannot be assigned"), "{rendered}");
}

#[test]
fn check_accepts_qualified_notrequired_typed_dict_field() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nimport typing_extensions\n\nclass User(TypedDict):\n    name: str\n    age: typing_extensions.NotRequired[int]\n\nuser: User = {\"name\": \"Ada\"}\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_treats_qualified_never_extra_items_as_closed_typed_dict() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nimport typing_extensions\n\nclass User(TypedDict, extra_items=typing_extensions.Never):\n    name: str\n\nuser: User = {\"name\": \"Ada\", \"age\": 1}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("unknown key `age`"), "{rendered}");
}

#[test]
fn check_accepts_contextual_writable_typed_dict_item_assignment_lambda() {
    let result = check_temp_typepython_source(
        "from typing import Callable, TypedDict\n\nclass User(TypedDict):\n    formatter: Callable[[int], str]\n\ndef mutate(user: User) -> None:\n    user[\"formatter\"] = lambda x: str(x)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_contextual_writable_typed_dict_item_assignment_nested_typed_dict_missing_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass Child(TypedDict):\n    name: str\n\nclass User(TypedDict):\n    child: Child\n\ndef mutate(user: User) -> None:\n    user[\"child\"] = {}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_accepts_writable_typed_dict_item_augmented_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"name\"] += \"!\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_writable_typed_dict_item_augmented_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    age: int\n\ndef mutate(user: User) -> None:\n    user[\"age\"] += \"!\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("produces `str` where `age` expects `int`"));
}

#[test]
fn check_reports_readonly_typed_dict_item_augmented_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    user[\"name\"] += \"!\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"));
    assert!(rendered.contains("augmented assignment"));
}

#[test]
fn check_reports_readonly_typed_dict_item_delete() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    del user[\"name\"]\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"));
    assert!(rendered.contains("cannot be deleted"));
}

#[test]
fn check_reports_readonly_typed_dict_item_delete_for_qualified_base() {
    let result = check_temp_typepython_source(
        "import typing\nfrom typing_extensions import ReadOnly\n\nclass User(typing.TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    del user[\"name\"]\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"));
    assert!(rendered.contains("cannot be deleted"));
}

#[test]
fn check_accepts_nominal_setitem_subscript_assignment() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_nominal_setitem_subscript_value_mismatch() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] = \"bad\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("passes value `str` where `__setitem__` expects `int`"));
}

#[test]
fn check_reports_nominal_setitem_subscript_key_mismatch() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[1] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("passes key `int` where `__setitem__` expects `str`"));
}

#[test]
fn check_accepts_contextual_nominal_setitem_subscript_assignment_lambda() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\nclass Cache:\n    def __setitem__(self, key: str, value: Callable[[int], str]) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"fmt\"] = lambda x: str(x)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_contextual_nominal_setitem_subscript_assignment_typed_dict_missing_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    name: str\n\nclass Cache:\n    def __setitem__(self, key: str, value: User) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"user\"] = {}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_accepts_nominal_setitem_subscript_augmented_assignment() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __getitem__(self, key: str) -> int:\n        return 0\n\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_nominal_setitem_subscript_augmented_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __getitem__(self, key: str) -> int:\n        return 0\n\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] += \"bad\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("produces `str` where `__setitem__` expects `int`"));
}

#[test]
fn check_reports_unreadable_nominal_setitem_subscript_augmented_assignment() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("not readable via `__getitem__`"));
}

#[test]
fn check_accepts_inherited_setitem_subscript_assignment() {
    let result = check_temp_typepython_source(
        "class Base:\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\nclass Cache(Base):\n    pass\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_readonly_nominal_subscript_assignment_without_setitem() {
    let result = check_temp_typepython_source(
        "class View:\n    def __getitem__(self, key: str) -> int:\n        return 1\n\ndef mutate(view: View) -> None:\n    view[\"x\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("is not writable via `__setitem__`"));
}

#[test]
fn check_accepts_unique_module_symbols() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                declaration! {
                    name: String::from("UserId"),
                    kind: DeclarationKind::TypeAlias,
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
    });

    assert!(result.diagnostics.is_empty(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_bare_method_reference_member_access() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "class Client:\n",
        "    name: str\n",
        "    def ping(self) -> str:\n",
        "        return self.name\n\n",
        "def takes_callback(callback: Callable[[], str]) -> None:\n",
        "    ...\n\n",
        "def run(client: Client) -> None:\n",
        "    client.ping\n",
        "    takes_callback(client.ping)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_reports_union_member_access_for_method_reference_on_optional_owner() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    name: str\n",
        "    def ping(self) -> str:\n",
        "        return self.name\n\n",
        "def run(client: Client | None) -> None:\n",
        "    client.ping\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `ping` on every union branch"), "{rendered}");
}

#[test]
fn check_allows_any_and_dynamic_union_member_branches() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Any\n\n",
        "class Known:\n",
        "    value: int\n\n",
        "def read_any(owner: Known | Any) -> object:\n",
        "    return owner.whatever\n\n",
        "def call_dynamic(owner: Known | dynamic) -> None:\n",
        "    owner.whatever()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_allows_any_optional_member_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        concat!(
            "from typing import Any\n\n",
            "def read(owner: Any | None) -> object:\n",
            "    return owner.value\n",
        ),
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_reports_missing_member_after_method_reference_support() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    name: str\n",
        "    def ping(self) -> str:\n",
        "        return self.name\n\n",
        "def run(client: Client) -> None:\n",
        "    client.nonexistent\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `nonexistent`"), "{rendered}");
}

#[test]
fn check_accepts_standard_object_members_on_user_classes() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    name: str\n\n",
        "def run(client: Client) -> None:\n",
        "    client.__class__\n",
        "    client.__doc__\n",
        "    client.__str__\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_accepts_standard_object_members_on_optional_owners() {
    let result = check_temp_typepython_source(concat!(
        "def run(x: int | None) -> None:\n",
        "    x.__str__\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_reports_method_call_on_optional_owner_without_narrowing() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    name: str\n",
        "    def ping(self) -> str:\n",
        "        return self.name\n\n",
        "def run(client: Client | None) -> None:\n",
        "    client.ping()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `ping` on every union branch"), "{rendered}");
}

#[test]
fn check_reports_builtin_method_call_on_optional_owner_without_narrowing() {
    let result = check_temp_typepython_source(concat!(
        "def run(x: int | None) -> int:\n",
        "    return x.bit_length()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `bit_length` on every union branch"), "{rendered}");
}

#[test]
fn check_accepts_method_call_on_optional_owner_after_narrowing() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    name: str\n",
        "    def ping(self) -> str:\n",
        "        return self.name\n\n",
        "def run(client: Client | None, x: int | None) -> str:\n",
        "    if client is not None:\n",
        "        client.ping()\n",
        "    if isinstance(x, int):\n",
        "        x.bit_length()\n",
        "    return \"\"\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_accepts_object_member_method_call_on_optional_owner() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    name: str\n\n",
        "def run(client: Client | None) -> None:\n",
        "    client.__str__()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_accepts_inherited_members_through_unresolved_base() {
    let result = check_temp_typepython_source(concat!(
        "from external_pkg import BaseModel\n\n",
        "class User(BaseModel):\n",
        "    name: str\n\n",
        "def run(user: User) -> None:\n",
        "    user.inherited_attr\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_reports_missing_member_when_hierarchy_is_fully_resolved() {
    let result = check_temp_typepython_source(concat!(
        "class Base:\n",
        "    name: str\n\n",
        "class Child(Base):\n",
        "    extra: int\n\n",
        "def run(child: Child) -> None:\n",
        "    child.nonexistent\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `nonexistent`"), "{rendered}");
}

#[test]
fn check_reports_method_call_on_missing_member() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    name: str\n",
        "    def ping(self) -> str:\n",
        "        return self.name\n\n",
        "def run(client: Client) -> None:\n",
        "    client.nonexistent()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `nonexistent`"), "{rendered}");
}

#[test]
fn check_accepts_method_calls_through_fields_dunders_and_open_bases() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "from external_pkg import BaseModel\n\n",
        "class Client:\n",
        "    name: str\n",
        "    handler: Callable[[], str]\n",
        "    def ping(self) -> str:\n",
        "        return self.name\n\n",
        "class User(BaseModel):\n",
        "    name: str\n\n",
        "def run(client: Client, user: User) -> None:\n",
        "    client.ping()\n",
        "    client.handler()\n",
        "    client.__str__()\n",
        "    user.inherited_method()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_accepts_method_calls_through_instance_attributes_initialized_in_init() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "class Task[**P, R]:\n",
        "    def __init__(self, fn: Callable[P, R]) -> None:\n",
        "        self._fn = fn\n\n",
        "    def delay(self, *args: P.args, **kwargs: P.kwargs) -> R:\n",
        "        return self._fn(*args, **kwargs)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_reports_instance_attribute_assigned_only_outside_init_as_missing() {
    let result = check_temp_typepython_source(concat!(
        "class Task:\n",
        "    def install(self) -> None:\n",
        "        self.callback = lambda: None\n\n",
        "    def run(self) -> None:\n",
        "        self.callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `callback`"), "{rendered}");
}

#[test]
fn check_reports_conditionally_initialized_instance_attribute_as_missing() {
    let result = check_temp_typepython_source(concat!(
        "class Task:\n",
        "    def __init__(self, enabled: bool) -> None:\n",
        "        if enabled:\n",
        "            self.callback = lambda: None\n\n",
        "    def run(self) -> None:\n",
        "        self.callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `callback`"), "{rendered}");
}

#[test]
fn check_reports_instance_attribute_after_conditional_init_return_as_missing() {
    let result = check_temp_typepython_source(concat!(
        "class Task:\n",
        "    def __init__(self, disabled: bool) -> None:\n",
        "        if disabled:\n",
        "            return\n",
        "        self.callback = lambda: None\n\n",
        "    def run(self) -> None:\n",
        "        self.callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `callback`"), "{rendered}");
}

#[test]
fn check_reports_call_of_non_callable_instance_attribute_from_init_rhs_type() {
    let result = check_temp_typepython_source(concat!(
        "class Task:\n",
        "    def __init__(self) -> None:\n",
        "        self.callback = 0\n\n",
        "    def run(self) -> None:\n",
        "        self.callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("member `callback`"), "{rendered}");
    assert!(rendered.contains("non-callable type `int`"), "{rendered}");
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_reports_call_of_non_callable_instance_attribute_from_init_parameter_type() {
    let result = check_temp_typepython_source(concat!(
        "class Task:\n",
        "    def __init__(self, callback: int) -> None:\n",
        "        self.callback = callback\n\n",
        "    def run(self) -> None:\n",
        "        self.callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("non-callable type `int`"), "{rendered}");
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_reports_call_of_non_callable_annotated_init_assignment() {
    let result = check_temp_typepython_source(concat!(
        "class Task:\n",
        "    def __init__(self) -> None:\n",
        "        self.callback: int = 0\n\n",
        "    def run(self) -> None:\n",
        "        self.callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("non-callable type `int`"), "{rendered}");
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_reports_call_of_declared_non_callable_attribute() {
    let result = check_temp_typepython_source(concat!(
        "class Task:\n",
        "    callback: int\n\n",
        "    def run(self) -> None:\n",
        "        self.callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("non-callable type `int`"), "{rendered}");
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_accepts_callable_object_initialized_in_init() {
    let result = check_temp_typepython_source(concat!(
        "class Handler:\n",
        "    def __call__(self) -> None:\n",
        "        pass\n\n",
        "class Task:\n",
        "    def __init__(self) -> None:\n",
        "        self.callback = Handler()\n\n",
        "    def run(self) -> None:\n",
        "        self.callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_init_attribute_use_before_its_assignment() {
    let result = check_temp_typepython_source(concat!(
        "class Task:\n",
        "    def __init__(self) -> None:\n",
        "        self.callback()\n",
        "        self.callback = lambda: None\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `callback`"), "{rendered}");
}

#[test]
fn check_accepts_unconditional_init_assignment_after_non_returning_branch() {
    let result = check_temp_typepython_source(concat!(
        "class Task:\n",
        "    def __init__(self, enabled: bool) -> None:\n",
        "        if enabled:\n",
        "            pass\n",
        "        self.callback = lambda: None\n\n",
        "    def run(self) -> None:\n",
        "        self.callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_method_call_on_missing_interface_member() {
    let result = check_temp_typepython_source(concat!(
        "interface Closeable:\n",
        "    def close(self) -> None: ...\n\n",
        "def run(x: Closeable) -> None:\n",
        "    x.close()\n",
        "    x.missing()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `missing`"), "{rendered}");
    assert!(!rendered.contains("has no member `close`"), "{rendered}");
}

#[test]
fn check_accepts_parameter_forwarding_in_function_scope_call_args() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    name: str\n\n",
        "def takes_client(client: Client) -> None:\n",
        "    ...\n\n",
        "def takes_int(value: int) -> None:\n",
        "    ...\n\n",
        "def forward(c: Client, n: int) -> None:\n",
        "    takes_client(c)\n",
        "    takes_int(n)\n",
        "    takes_int(value=n)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4001"), "{rendered}");
}

#[test]
fn check_accepts_parameter_forwarding_in_method_call_args() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Protocol\n\n",
        "class SupportsRead(Protocol):\n",
        "    def read(self, size: int = -1) -> str: ...\n\n",
        "def read_prefix(reader: SupportsRead, size: int) -> str:\n",
        "    local: int = size\n",
        "    return reader.read(size=local)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_method_parameter_forwarding_type_mismatch_with_actual_type() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Protocol\n\n",
        "class SupportsRead(Protocol):\n",
        "    def read(self, size: int = -1) -> str: ...\n\n",
        "def read_prefix(reader: SupportsRead, text: str) -> str:\n",
        "    return reader.read(text)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("passes `str` where parameter expects `int`"), "{rendered}");
}

#[test]
fn check_infers_generic_method_type_from_parameter_forwarding() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    def identity[T](self, value: T) -> T:\n",
        "        return value\n\n",
        "def forward(client: Client, value: int) -> int:\n",
        "    return client.identity(value)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_infers_generic_method_type_from_keyword_parameter_forwarding() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    def identity[T](self, *, value: T) -> T:\n",
        "        return value\n\n",
        "def forward(client: Client, value: int) -> int:\n",
        "    return client.identity(value=value)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_infers_generic_method_type_from_scoped_collection_literal() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    def first[T](self, values: list[list[T]]) -> T:\n",
        "        return values[0][0]\n\n",
        "def forward(client: Client, value: int) -> int:\n",
        "    return client.first([[value]])\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_prefers_scoped_callable_parameter_in_method_call() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def callback(value: int) -> int:\n",
        "    return value\n\n",
        "class Runner:\n",
        "    def use(self, fn: Callable[[str], str]) -> None:\n",
        "        ...\n\n",
        "def forward(runner: Runner, callback: Callable[[str], str]) -> None:\n",
        "    runner.use(callback)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_infers_paramspec_method_from_scoped_callable_parameter() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def callback(value: int) -> int:\n",
        "    return value\n\n",
        "class Runner:\n",
        "    def preserve[**P, R](self, callback: Callable[P, R]) -> Callable[P, R]:\n",
        "        return callback\n\n",
        "def forward(\n",
        "    runner: Runner, callback: Callable[[str], str]\n",
        ") -> Callable[[str], str]:\n",
        "    return runner.preserve(callback)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_infers_imported_generic_dot_call_from_caller_scope() {
    let result = check_temp_project_sources(&[
        (
            "lib.tpy",
            "lib",
            SourceKind::TypePython,
            "def identity[T](value: T) -> T:\n    return value\n",
        ),
        (
            "app.tpy",
            "app",
            SourceKind::TypePython,
            concat!(
                "import lib\n\n",
                "def forward(value: int) -> int:\n",
                "    return lib.identity(value)\n",
            ),
        ),
    ]);

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_infers_cross_module_generic_method_with_provider_bound() {
    let result = check_temp_project_sources(&[
        (
            "lib.tpy",
            "lib",
            SourceKind::TypePython,
            concat!(
                "typealias Number = int\n\n",
                "class Client:\n",
                "    def identity[T: Number](self, value: T) -> T:\n",
                "        return value\n",
            ),
        ),
        (
            "app.tpy",
            "app",
            SourceKind::TypePython,
            concat!(
                "from lib import Client\n\n",
                "def forward(client: Client, value: int) -> int:\n",
                "    return client.identity(value)\n",
            ),
        ),
    ]);

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_resolves_method_result_stored_in_bare_local_assignment() {
    let result = check_temp_typepython_source(concat!(
        "class Item:\n",
        "    def to_json(self) -> str:\n",
        "        return \"{}\"\n\n",
        "class Repository:\n",
        "    def save(self, data: str) -> bool:\n",
        "        return True\n\n",
        "def save_item(repo: Repository, item: Item) -> bool:\n",
        "    data = item.to_json()\n",
        "    return repo.save(data)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_resolves_imported_bound_method_result_in_bare_local_assignment() {
    let result = check_temp_project_sources(&[
        (
            "services.tpy",
            "services",
            SourceKind::TypePython,
            concat!(
                "interface Serializable:\n",
                "    def to_json(self) -> str: ...\n\n",
                "class Repository[T: Serializable]:\n",
                "    def save(self, data: str) -> bool:\n",
                "        return True\n",
            ),
        ),
        (
            "app.tpy",
            "app",
            SourceKind::TypePython,
            concat!(
                "from services import Serializable, Repository\n\n",
                "def save_item[T: Serializable](repo: Repository[T], item: T) -> bool:\n",
                "    data = item.to_json()\n",
                "    return repo.save(data)\n",
            ),
        ),
    ]);

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_resolves_member_on_class_scoped_bounded_type_parameter() {
    let result = check_temp_typepython_source(concat!(
        "interface Named:\n",
        "    name: str\n\n",
        "class Labeler[T: Named]:\n",
        "    def label(self, item: T) -> str:\n",
        "        return item.name\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_argument_mismatch_on_bounded_type_parameter_method() {
    let result = check_temp_typepython_source(concat!(
        "interface Writer:\n",
        "    def write(self, value: str) -> None: ...\n\n",
        "def write_item[T: Writer](item: T) -> None:\n",
        "    item.write(1)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(result.diagnostics.has_errors(), "{rendered}");
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("expects `str`"), "{rendered}");
}

#[test]
fn check_reports_missing_method_on_bounded_type_parameter() {
    let result = check_temp_typepython_source(concat!(
        "interface Serializable:\n",
        "    def to_json(self) -> str: ...\n\n",
        "def serialize[T: Serializable](item: T) -> str:\n",
        "    return item.missing()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(result.diagnostics.has_errors(), "{rendered}");
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("has no member `missing`"), "{rendered}");
}

#[test]
fn check_reports_return_mismatch_from_bounded_type_parameter_method() {
    let result = check_temp_typepython_source(concat!(
        "interface Serializable:\n",
        "    def to_json(self) -> str: ...\n\n",
        "def serialize[T: Serializable](item: T) -> int:\n",
        "    return item.to_json()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(result.diagnostics.has_errors(), "{rendered}");
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("returns `str`"), "{rendered}");
}

#[test]
fn check_preserves_self_return_for_bounded_type_parameter_method() {
    let result = check_temp_typepython_source(concat!(
        "interface Cloneable:\n",
        "    def clone(self) -> Self: ...\n\n",
        "def clone_item[T: Cloneable](item: T) -> T:\n",
        "    return item.clone()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_preserves_self_parameter_for_bounded_type_parameter_method() {
    let result = check_temp_typepython_source(concat!(
        "interface Mergeable:\n",
        "    def merge(self, other: Self) -> Self: ...\n\n",
        "def merge_items[T: Mergeable](left: T, right: T) -> T:\n",
        "    return left.merge(right)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_preserves_self_member_for_bounded_type_parameter() {
    let result = check_temp_typepython_source(concat!(
        "interface Linked:\n",
        "    next: Self\n\n",
        "def next_item[T: Linked](item: T) -> T:\n",
        "    return item.next\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_preserves_self_when_bound_reuses_caller_type_parameter_name() {
    let result = check_temp_typepython_source(concat!(
        "interface Wrapper[T]:\n",
        "    next: Self\n",
        "    def clone(self) -> Self: ...\n",
        "    def replace(self, value: T) -> Self: ...\n\n",
        "def clone_item[T: Wrapper[int]](item: T) -> T:\n",
        "    return item.clone()\n\n",
        "def replace_item[T: Wrapper[int]](item: T) -> T:\n",
        "    return item.replace(1)\n\n",
        "def next_item[T: Wrapper[int]](item: T) -> T:\n",
        "    return item.next\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_validates_attribute_assignment_on_bounded_type_parameter() {
    let result = check_temp_typepython_source(concat!(
        "interface Named:\n",
        "    name: str\n\n",
        "def rename[T: Named](item: T) -> None:\n",
        "    item.name = 1\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(result.diagnostics.has_errors(), "{rendered}");
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("member `name` expects `str`"), "{rendered}");
}

#[test]
fn check_accepts_attribute_assignment_on_bounded_type_parameter() {
    let result = check_temp_typepython_source(concat!(
        "interface Named:\n",
        "    name: str\n\n",
        "def rename[T: Named](item: T) -> None:\n",
        "    item.name = \"updated\"\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_keeps_bounded_type_parameter_provenance_through_local_flows() {
    let result = check_temp_typepython_source(concat!(
        "interface HasOk:\n",
        "    def ok(self, value: str) -> None: ...\n",
        "    def clone(self) -> Self: ...\n\n",
        "def inspect[T: HasOk](item: T, items: list[T]) -> None:\n",
        "    alias: T = item\n",
        "    alias.ok(\"alias\")\n",
        "    copy = item.clone()\n",
        "    copy.ok(\"copy\")\n",
        "    for entry in items:\n",
        "        entry.ok(\"entry\")\n",
        "    selected = items[0]\n",
        "    selected.ok(\"item\")\n",
        "    conditional = item if True else item\n",
        "    conditional.ok(\"conditional\")\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_validates_bounded_type_parameter_methods_through_local_flows() {
    let result = check_temp_typepython_source(concat!(
        "interface HasOk:\n",
        "    def ok(self, value: str) -> None: ...\n",
        "    def clone(self) -> Self: ...\n\n",
        "def inspect[T: HasOk](item: T, items: list[T]) -> None:\n",
        "    alias: T = item\n",
        "    alias.ok(1)\n",
        "    copy = item.clone()\n",
        "    copy.ok(1)\n",
        "    for entry in items:\n",
        "        entry.ok(1)\n",
        "    selected = items[0]\n",
        "    selected.ok(1)\n",
        "    conditional = item if True else item\n",
        "    conditional.ok(1)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 5, "{rendered}");
}

#[test]
fn check_keeps_bounded_type_parameter_provenance_through_member_reads() {
    let result = check_temp_typepython_source(concat!(
        "interface Linked:\n",
        "    next: Self\n",
        "    def ok(self, value: str) -> None: ...\n\n",
        "class Holder[V]:\n",
        "    item: V\n\n",
        "def inspect[T: Linked](item: T, holder: Holder[T]) -> None:\n",
        "    linked = item.next\n",
        "    linked.ok(\"linked\")\n",
        "    held = holder.item\n",
        "    held.ok(\"held\")\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_validates_bounded_methods_after_member_reads() {
    let result = check_temp_typepython_source(concat!(
        "interface Linked:\n",
        "    next: Self\n",
        "    def ok(self, value: str) -> None: ...\n\n",
        "class Holder[V]:\n",
        "    item: V\n\n",
        "def inspect[T: Linked](item: T, holder: Holder[T]) -> None:\n",
        "    linked = item.next\n",
        "    linked.ok(1)\n",
        "    held = holder.item\n",
        "    held.ok(1)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 2, "{rendered}");
}

#[test]
fn check_does_not_confuse_same_named_class_with_scoped_type_parameter() {
    let result = check_temp_typepython_source(concat!(
        "class T:\n",
        "    pass\n\n",
        "def make() -> T:\n",
        "    return T()\n\n",
        "interface HasOk:\n",
        "    def ok(self) -> None: ...\n\n",
        "def inspect[T: HasOk]() -> None:\n",
        "    make().ok()\n",
        "    alias = make()\n",
        "    alias.ok()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(result.diagnostics.has_errors(), "{rendered}");
    assert_eq!(rendered.matches("error[TPY4002]").count(), 2, "{rendered}");
    assert!(rendered.contains("type `T`") && rendered.contains("has no member `ok`"), "{rendered}");
}

#[test]
fn check_does_not_propagate_type_parameter_through_non_self_method_return() {
    let result = check_temp_typepython_source(concat!(
        "class T:\n",
        "    pass\n\n",
        "interface Builder:\n",
        "    def make(self) -> T: ...\n",
        "    def ok(self) -> None: ...\n\n",
        "def inspect[T: Builder](builder: T) -> None:\n",
        "    result = builder.make()\n",
        "    result.ok()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(result.diagnostics.has_errors(), "{rendered}");
    assert!(rendered.contains("TPY4002"), "{rendered}");
    assert!(rendered.contains("type `T`") && rendered.contains("has no member `ok`"), "{rendered}");
}

#[test]
fn check_resolves_union_receiver_member_and_method_return_joins() {
    let result = check_temp_typepython_source(concat!(
        "class Left:\n",
        "    value: int\n",
        "    def parse(self, raw: str) -> int:\n",
        "        return 1\n\n",
        "class Right:\n",
        "    value: str\n",
        "    def parse(self, raw: str) -> str:\n",
        "        return raw\n\n",
        "def parse_value(owner: Left | Right) -> int | str:\n",
        "    return owner.parse(\"value\")\n\n",
        "def read_value(owner: Left | Right) -> int | str:\n",
        "    return owner.value\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_validates_each_union_receiver_method_signature() {
    let result = check_temp_typepython_source(concat!(
        "class Left:\n",
        "    def parse(self, raw: str) -> int:\n",
        "        return 1\n\n",
        "class Right:\n",
        "    def parse(self, raw: int) -> int:\n",
        "        return raw\n\n",
        "def parse_value(owner: Left | Right) -> int:\n",
        "    return owner.parse(\"value\")\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 1, "{rendered}");
    assert!(rendered.contains("Right.parse"), "{rendered}");
}

#[test]
fn check_preserves_self_return_across_union_receiver() {
    let result = check_temp_typepython_source(concat!(
        "class Left:\n",
        "    def clone(self) -> Self:\n",
        "        return self\n\n",
        "class Right:\n",
        "    def clone(self) -> Self:\n",
        "        return self\n\n",
        "def clone_value(owner: Left | Right) -> Left | Right:\n",
        "    return owner.clone()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_resolves_union_bound_member_method_and_self_returns() {
    let result = check_temp_typepython_source(concat!(
        "class Left:\n",
        "    value: int\n",
        "    def parse(self, raw: str) -> int:\n",
        "        return 1\n",
        "    def clone(self) -> Self:\n",
        "        return self\n\n",
        "class Right:\n",
        "    value: str\n",
        "    def parse(self, raw: str) -> str:\n",
        "        return raw\n",
        "    def clone(self) -> Self:\n",
        "        return self\n\n",
        "def parse_value[T: Left | Right](owner: T) -> int | str:\n",
        "    return owner.parse(\"value\")\n\n",
        "def read_value[T: Left | Right](owner: T) -> int | str:\n",
        "    return owner.value\n\n",
        "def clone_value[T: Left | Right](owner: T) -> T:\n",
        "    return owner.clone()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_resolves_constrained_typevar_members_and_self_returns() {
    let result = check_temp_typepython_source(concat!(
        "class Left:\n",
        "    value: int\n",
        "    def parse(self, raw: str) -> int:\n",
        "        return 1\n",
        "    def clone(self) -> Self:\n",
        "        return self\n\n",
        "class Right:\n",
        "    value: str\n",
        "    def parse(self, raw: str) -> str:\n",
        "        return raw\n",
        "    def clone(self) -> Self:\n",
        "        return self\n\n",
        "def parse_value[T: (Left, Right)](owner: T) -> int | str:\n",
        "    return owner.parse(\"value\")\n\n",
        "def read_value[T: (Left, Right)](owner: T) -> int | str:\n",
        "    return owner.value\n\n",
        "def clone_value[T: (Left, Right)](owner: T) -> T:\n",
        "    return owner.clone()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_validates_each_constrained_typevar_method_signature() {
    let result = check_temp_typepython_source(concat!(
        "class Left:\n",
        "    def parse(self, raw: str) -> int:\n",
        "        return 1\n\n",
        "class Right:\n",
        "    def parse(self, raw: str) -> int:\n",
        "        return 1\n\n",
        "def parse_value[T: (Left, Right)](owner: T) -> int:\n",
        "    return owner.parse(1)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 2, "{rendered}");
    assert!(rendered.contains("Left.parse") && rendered.contains("Right.parse"), "{rendered}");
}

#[test]
fn check_validates_each_union_bound_method_signature() {
    let result = check_temp_typepython_source(concat!(
        "class Left:\n",
        "    def parse(self, raw: str) -> int:\n",
        "        return 1\n\n",
        "class Right:\n",
        "    def parse(self, raw: str) -> int:\n",
        "        return 1\n\n",
        "def parse_value[T: Left | Right](owner: T) -> int:\n",
        "    return owner.parse(1)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 2, "{rendered}");
    assert!(rendered.contains("Left.parse") && rendered.contains("Right.parse"), "{rendered}");
}

#[test]
fn check_reports_union_receiver_joined_return_mismatches() {
    let result = check_temp_typepython_source(concat!(
        "class Left:\n",
        "    value: int\n",
        "    def parse(self) -> int:\n",
        "        return 1\n\n",
        "class Right:\n",
        "    value: str\n",
        "    def parse(self) -> str:\n",
        "        return \"value\"\n\n",
        "def parse_value(owner: Left | Right) -> int:\n",
        "    return owner.parse()\n\n",
        "def read_value(owner: Left | Right) -> int:\n",
        "    return owner.value\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 2, "{rendered}");
    assert!(rendered.contains("Union[int, str]"), "{rendered}");
}

#[test]
fn check_reports_union_bound_joined_return_mismatches() {
    let result = check_temp_typepython_source(concat!(
        "class Left:\n",
        "    value: int\n",
        "    def parse(self) -> int:\n",
        "        return 1\n\n",
        "class Right:\n",
        "    value: str\n",
        "    def parse(self) -> str:\n",
        "        return \"value\"\n\n",
        "def parse_value[T: Left | Right](owner: T) -> int:\n",
        "    return owner.parse()\n\n",
        "def read_value[T: Left | Right](owner: T) -> int:\n",
        "    return owner.value\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 2, "{rendered}");
    assert!(rendered.contains("Union[int, str]"), "{rendered}");
}

#[test]
fn check_ignores_none_union_branches_consistently_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        concat!(
            "class Box:\n",
            "    value: int\n",
            "    def parse(self) -> int:\n",
            "        return 1\n",
            "    def clone(self) -> Self:\n",
            "        return self\n\n",
            "def wrong_method(owner: Box | None) -> str:\n",
            "    return owner.parse()\n\n",
            "def wrong_member(owner: Box | None) -> str:\n",
            "    return owner.value\n\n",
            "def wrong_copy[T: Box | None](owner: T) -> str:\n",
            "    copy = owner.clone()\n",
            "    return copy.value\n",
        ),
        ParseOptions::default(),
        crate::CheckerOptions {
            strict: true,
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 3, "{rendered}");
    assert!(!rendered.contains("TPY4002"), "{rendered}");
}

#[test]
fn check_reports_method_missing_from_one_union_receiver_branch() {
    let result = check_temp_typepython_source(concat!(
        "class Left:\n",
        "    def parse(self) -> int:\n",
        "        return 1\n\n",
        "class Right:\n",
        "    pass\n\n",
        "def parse_value(owner: Left | Right) -> int:\n",
        "    return owner.parse()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4002]").count(), 1, "{rendered}");
    assert!(rendered.contains("has no member `parse` on every union branch"), "{rendered}");
}

#[test]
fn check_preserves_parameterized_self_across_union_receiver_branches() {
    let result = check_temp_typepython_source(concat!(
        "class Box[T]:\n",
        "    def clone(self) -> Self:\n",
        "        return self\n\n",
        "def clone_value(owner: Box[int] | Box[str]) -> Box[int]:\n",
        "    return owner.clone()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 1, "{rendered}");
    assert!(rendered.contains("Union[Box[int], Box[str]]"), "{rendered}");
}

#[test]
fn check_validates_self_parameters_per_union_receiver_branch() {
    let result = check_temp_typepython_source(concat!(
        "class Box[T]:\n",
        "    def merge(self, other: Self) -> Self:\n",
        "        return self\n\n",
        "def merge_values(\n",
        "    owner: Box[int] | Box[str],\n",
        "    other: Box[int] | Box[str],\n",
        ") -> None:\n",
        "    owner.merge(other)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 2, "{rendered}");
    assert!(rendered.contains("Box[int]") && rendered.contains("Box[str]"), "{rendered}");
}

#[test]
fn check_deduplicates_identical_union_branch_method_diagnostics() {
    let result = check_temp_typepython_source(concat!(
        "class Box[T]:\n",
        "    def parse(self, raw: str) -> int:\n",
        "        return 1\n\n",
        "def parse_value(owner: Box[int] | Box[str]) -> int:\n",
        "    return owner.parse(1)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 1, "{rendered}");
    assert!(rendered.contains("Box.parse"), "{rendered}");
}

#[test]
fn check_preserves_duplicate_argument_diagnostics_within_one_method_call() {
    let result = check_temp_typepython_source(concat!(
        "class Parser:\n",
        "    def parse(self, first: str, second: str) -> int:\n",
        "        return 1\n\n",
        "def parse_value(parser: Parser) -> int:\n",
        "    return parser.parse(1, 1)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 2, "{rendered}");
    assert_eq!(rendered.matches("passes `int` where parameter expects `str`").count(), 2);
}

#[test]
fn check_selects_overloads_on_each_union_receiver_branch() {
    let result = check_temp_typepython_source(concat!(
        "from typing import overload\n\n",
        "class Left:\n",
        "    @overload\n",
        "    def parse(self, raw: int) -> int: ...\n",
        "    @overload\n",
        "    def parse(self, raw: str) -> str: ...\n",
        "    def parse(self, raw: int | str) -> int | str:\n",
        "        return raw\n\n",
        "class Right:\n",
        "    @overload\n",
        "    def parse(self, raw: int) -> int: ...\n",
        "    @overload\n",
        "    def parse(self, raw: str) -> bytes: ...\n",
        "    def parse(self, raw: int | str) -> int | bytes:\n",
        "        return b\"value\"\n\n",
        "def parse_value(owner: Left | Right) -> str | bytes:\n",
        "    return owner.parse(\"value\")\n\n",
        "def wrong_value(owner: Left | Right) -> int:\n",
        "    return owner.parse(\"value\")\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("error[TPY4001]").count(), 1, "{rendered}");
    assert!(rendered.contains("Union[str, bytes]"), "{rendered}");
}

#[test]
fn check_resolves_method_argument_expansions_in_function_scope() {
    let result = check_temp_typepython_source(concat!(
        "from typing import TypedDict\n\n",
        "class Options(TypedDict, closed=True):\n",
        "    label: str\n\n",
        "class Client:\n",
        "    def combine(self, value: int, *, label: str) -> None:\n",
        "        ...\n\n",
        "def forward(client: Client, args: tuple[int], options: Options) -> None:\n",
        "    client.combine(*args, **options)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_resolves_direct_call_argument_expansions_in_function_scope() {
    let result = check_temp_typepython_source(concat!(
        "from typing import TypedDict\n\n",
        "class Options(TypedDict, closed=True):\n",
        "    label: str\n\n",
        "def combine(value: int, *, label: str) -> None:\n",
        "    ...\n\n",
        "def forward(args: tuple[int], options: Options) -> None:\n",
        "    combine(*args, **options)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_parameter_forwarding_type_mismatch_with_actual_type() {
    let result = check_temp_typepython_source(concat!(
        "def takes_int(value: int) -> None:\n",
        "    ...\n\n",
        "def forward(text: str) -> None:\n",
        "    takes_int(text)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("passes `str` where parameter expects `int`"), "{rendered}");
}

#[test]
fn check_reports_optional_parameter_forwarding_without_narrowing() {
    let result = check_temp_typepython_source(concat!(
        "def takes_int(value: int) -> None:\n",
        "    ...\n\n",
        "def forward(n: int | None) -> None:\n",
        "    takes_int(n)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(
        rendered.contains("passes `Union[int, None]` where parameter expects `int`"),
        "{rendered}"
    );
}

#[test]
fn check_accepts_newtype_value_where_base_type_is_expected() {
    let result = check_temp_typepython_source(concat!(
        "from typing import NewType\n\n",
        "UserId = NewType(\"UserId\", int)\n\n",
        "def takes_int(value: int) -> None:\n",
        "    ...\n\n",
        "def run(user_id: UserId) -> int:\n",
        "    takes_int(user_id)\n",
        "    return user_id\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4001"), "{rendered}");
}

#[test]
fn check_rejects_base_value_where_newtype_is_expected() {
    let result = check_temp_typepython_source(concat!(
        "from typing import NewType\n\n",
        "UserId = NewType(\"UserId\", int)\n\n",
        "def run(raw: int) -> UserId:\n",
        "    return raw\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("returns `int`"), "{rendered}");
}

#[test]
fn check_accepts_newtype_construction_from_base_value() {
    let result = check_temp_typepython_source(concat!(
        "from typing import NewType\n\n",
        "UserId = NewType(\"UserId\", int)\n\n",
        "def run(raw: int) -> UserId:\n",
        "    return UserId(raw)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4001"), "{rendered}");
}

#[test]
fn check_resolves_call_args_in_modules_with_typepython_surface_syntax() {
    let result = check_temp_typepython_source(concat!(
        "class Inner:\n",
        "    label: str\n\n",
        "class Outer:\n",
        "    inner: Inner\n\n",
        "sealed class Expr:\n",
        "    pass\n\n",
        "class Num(Expr):  value: int\n\n",
        "def takes_str(text: str) -> None:\n",
        "    ...\n\n",
        "def forward(outer: Outer) -> None:\n",
        "    takes_str(outer.inner.label)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4001"), "{rendered}");
    assert!(!rendered.contains("TPY4003"), "{rendered}");
}

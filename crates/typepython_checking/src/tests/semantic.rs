use super::*;

fn check_temp_typepython_source_with_effect_rows(
    source_text: &str,
    strict: bool,
) -> crate::CheckResult {
    check_temp_typepython_source_with_checker_options(
        source_text,
        ParseOptions::default(),
        crate::CheckerOptions {
            strict,
            experimental_effect_rows: true,
            experimental_framework_adapters: true,
            ..crate::CheckerOptions::permissive_test_default()
        },
    )
}

fn check_temp_typepython_source_with_taint(source_text: &str, strict: bool) -> crate::CheckResult {
    check_temp_typepython_source_with_checker_options(
        source_text,
        ParseOptions::default(),
        crate::CheckerOptions {
            strict,
            experimental_taint: true,
            experimental_framework_adapters: true,
            ..crate::CheckerOptions::permissive_test_default()
        },
    )
}

fn framework_adapters_check_options() -> crate::CheckerOptions {
    crate::CheckerOptions::permissive_test_default().with_framework_adapters(true)
}

#[test]
fn check_reports_implicit_dynamic_function_and_method_params_when_enabled() {
    let result = check_temp_typepython_source_with_checker_options(
        concat!(
            "def parse(value) -> int:\n",
            "    return value\n\n",
            "class Box:\n",
            "    def render(self, item) -> int:\n",
            "        return item\n",
        ),
        ParseOptions::default(),
        crate::CheckerOptions {
            no_implicit_dynamic: true,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4029"), "{rendered}");
    assert!(rendered.contains("parameter `value` on function `parse`"), "{rendered}");
    assert!(rendered.contains("parameter `item` on member `Box.render`"), "{rendered}");
    assert!(!rendered.contains("parameter `self`"), "{rendered}");
}

#[test]
fn check_accepts_explicit_dynamic_when_no_implicit_dynamic_is_enabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "def parse(value: dynamic) -> dynamic:\n    return value\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            no_implicit_dynamic: true,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_uncontextualized_lambda_param_when_no_implicit_dynamic_is_enabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "handler = lambda value: value\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            no_implicit_dynamic: true,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4029"), "{rendered}");
    assert!(rendered.contains("lambda assigned to `handler`"), "{rendered}");
    assert!(rendered.contains("parameter `value`"), "{rendered}");
}

#[test]
fn check_accepts_contextual_lambda_param_when_no_implicit_dynamic_is_enabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "from typing import Callable\n\nhandler: Callable[[int], int] = lambda value: value\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            no_implicit_dynamic: true,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_none_assignment_when_strict_nulls_is_enabled() {
    let result = check_temp_typepython_source("value: int = None\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("assigns `None`"), "{rendered}");
}

#[test]
fn check_reports_none_call_argument_when_strict_nulls_is_enabled() {
    let result =
        check_temp_typepython_source("def takes(value: int) -> None:\n    pass\n\ntakes(None)\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("passes `None`"), "{rendered}");
}

#[test]
fn check_reports_none_return_when_strict_nulls_is_enabled() {
    let result = check_temp_typepython_source("def build() -> int:\n    return None\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("returns `None`"), "{rendered}");
}

#[test]
fn check_accepts_none_assignment_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "value: int = None\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_none_call_argument_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "def takes(value: int) -> None:\n    pass\n\ntakes(None)\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_none_module_member_call_argument_when_strict_nulls_is_disabled() {
    let result = check_two_module_typepython_sources_with_checker_options(
        "def takes(value: int) -> None:\n    pass\n",
        "import lib\n\nlib.takes(None)\n",
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_none_return_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "def build() -> int:\n    return None\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_none_contextual_typed_dict_return_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        concat!(
            "from typing import TypedDict\n\n",
            "class Payload(TypedDict):\n",
            "    body: str\n\n",
            "def build() -> Payload:\n",
            "    return {\"body\": None}\n",
        ),
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_callable_none_return_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "from typing import Callable\n\nhandler: Callable[[int], int] = lambda value: None\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_override_none_return_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "class Base:\n    def value(self) -> int:\n        return 1\n\nclass Child(Base):\n    def value(self) -> None:\n        return None\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_protocol_none_value_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "from typing import Protocol\n\nclass HasValue(Protocol):\n    value: int\n\nclass Model(HasValue):\n    value: None = None\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_structural_protocol_none_value_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "from typing import Protocol\n\nclass HasValue(Protocol):\n    value: int\n\nclass Model:\n    value: None = None\n\nmodel: HasValue = Model()\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_bounded_generic_none_argument_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "def first[T: int](value: T) -> T:\n    return value\n\nresult: int = first(None)\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_bounded_generic_method_none_argument_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "class Service:\n    def first[T: int](self, value: T) -> T:\n        return value\n\nresult: int = Service().first(None)\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_rejects_none_literal_assignment_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "from typing import Literal\n\nvalue: Literal[1] = None\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
}

#[test]
fn check_rejects_none_literal_union_assignment_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "from typing import Literal\n\nvalue: Literal[1] | Literal[2] = None\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
}

#[test]
fn check_accepts_none_mixed_literal_union_assignment_when_strict_nulls_is_disabled() {
    let result = check_temp_typepython_source_with_checker_options(
        "from typing import Literal\n\nvalue: int | Literal[1] = None\n",
        ParseOptions::default(),
        crate::CheckerOptions {
            strict_nulls: false,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_assignment_into_unknown_boundary() {
    let result = check_temp_typepython_source("value: unknown = 1\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_unknown_assignment_to_concrete_type() {
    let result = check_temp_typepython_source(
        "def get_value() -> unknown:\n    ...\n\nvalue: int = get_value()\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("assigns `unknown`"), "{rendered}");
    assert!(rendered.contains("expects `int`"), "{rendered}");
}

#[test]
fn check_accepts_unknown_assignment_to_allowed_boundary_types() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "as_unknown: unknown = get_value()\n",
        "as_dynamic: dynamic = get_value()\n",
        "as_object: object = get_value()\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_unknown_assignment_to_object_alias_and_union_boundary() {
    let result = check_temp_typepython_source(concat!(
        "typealias ObjectSink = object\n\n",
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "as_alias: ObjectSink = get_value()\n",
        "as_union: object | int = get_value()\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_unknown_assignment_to_any() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Any\n\n",
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "value: Any = get_value()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("assigns `unknown`"), "{rendered}");
    assert!(rendered.contains("expects `Any`"), "{rendered}");
}

#[test]
fn check_reports_unknown_assignment_to_any_alias() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Any\n\n",
        "typealias Loose = Any\n\n",
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "value: Loose = get_value()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("assigns `unknown`"), "{rendered}");
    assert!(rendered.contains("expects `Loose`"), "{rendered}");
}

#[test]
fn check_reports_unknown_call_argument_to_concrete_parameter() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "def takes(value: int) -> None:\n",
        "    pass\n\n",
        "takes(get_value())\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("passes `unknown`"), "{rendered}");
    assert!(rendered.contains("expects `int`"), "{rendered}");
}

#[test]
fn check_reports_unknown_member_access_with_local_context() {
    let result = check_temp_typepython_source(concat!(
        "def run(value: unknown) -> None:\n",
        "    value.name\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_direct_call_with_local_context() {
    let result = check_temp_typepython_source(concat!(
        "def run(callback: unknown) -> None:\n",
        "    callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("call to `callback`"), "{rendered}");
    assert!(rendered.contains("`callback` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_direct_call_with_local_context_when_imports_are_dynamic() {
    let result = check_temp_typepython_source_with_checker_options(
        concat!("def run(callback: unknown) -> None:\n", "    callback()\n",),
        ParseOptions::default(),
        crate::CheckerOptions {
            import_fallback: ImportFallback::Dynamic,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("call to `callback`"), "{rendered}");
    assert!(rendered.contains("`callback` has type `unknown`"), "{rendered}");
}

#[test]
fn check_accepts_local_callable_direct_call() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def run(callback: Callable[[], None]) -> None:\n",
        "    callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4003"), "{rendered}");
}

#[test]
fn check_reports_unknown_direct_call_when_local_parameter_shadows_special_name() {
    let result = check_temp_typepython_source(concat!(
        "def run(isinstance: unknown, iter: unknown) -> None:\n",
        "    isinstance()\n",
        "    iter()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("call to `isinstance`"), "{rendered}");
    assert!(rendered.contains("call to `iter`"), "{rendered}");
}

#[test]
fn check_reports_unknown_direct_call_when_module_value_shadows_builtin() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "iter: unknown = get_value()\n",
        "iter()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("call to `iter`"), "{rendered}");
}

#[test]
fn check_reports_unknown_direct_call_when_module_value_shadows_builtin_in_function() {
    let result = check_temp_typepython_source_with_checker_options(
        concat!(
            "def get_value() -> unknown:\n",
            "    ...\n\n",
            "iter: unknown = get_value()\n\n",
            "def run() -> None:\n",
            "    iter()\n",
        ),
        ParseOptions::default(),
        crate::CheckerOptions {
            import_fallback: ImportFallback::Dynamic,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("call to `iter`"), "{rendered}");
}

#[test]
fn check_accepts_builtin_direct_call_before_module_value_shadows_builtin() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "items: list[int] = []\n",
        "first = iter(items)\n",
        "iter: unknown = get_value()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4003"), "{rendered}");
    assert!(!rendered.contains("call to `iter`"), "{rendered}");
}

#[test]
fn check_reports_unknown_direct_call_when_unresolved_import_shadows_builtin() {
    let result = check_temp_typepython_source(concat!(
        "from definitely_missing import iter\n\n",
        "iter()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("call to `iter`"), "{rendered}");
}

#[test]
fn check_reports_unknown_direct_call_when_unresolved_import_shadows_typing_name() {
    let result = check_temp_typepython_source(concat!(
        "from definitely_missing import TypeVar\n\n",
        "TypeVar(\"T\")\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("call to `TypeVar`"), "{rendered}");
}

#[test]
fn check_accepts_builtin_shadowed_by_unresolved_import_when_imports_are_dynamic() {
    let result = check_temp_typepython_source_with_checker_options(
        concat!("from definitely_missing import iter\n\n", "iter()\n",),
        ParseOptions::default(),
        crate::CheckerOptions {
            import_fallback: ImportFallback::Dynamic,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4003"), "{rendered}");
    assert!(!rendered.contains("call to `iter`"), "{rendered}");
}

#[test]
fn check_reports_unknown_return_to_concrete_type() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "def build() -> int:\n",
        "    return get_value()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("returns `unknown`"), "{rendered}");
    assert!(rendered.contains("expects `int`"), "{rendered}");
}

#[test]
fn check_accepts_unknown_after_explicit_cast() {
    let result = check_temp_typepython_source(concat!(
        "from typing import cast\n\n",
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "value: int = cast(int, get_value())\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_unknown_subscript_from_real_parse_pipeline() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "item = get_value()[0]\n\n",
        "def run() -> None:\n",
        "    value: unknown = get_value()\n",
        "    other = value[0]\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("subscript access"), "{rendered}");
    assert!(rendered.contains("`get_value()` has type `unknown`"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_arithmetic_from_real_parse_pipeline() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "left = get_value() + 1\n",
        "right = 1 + get_value()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("binary operation `+`"), "{rendered}");
    assert!(rendered.contains("left operand `get_value()` has type `unknown`"), "{rendered}");
    assert!(rendered.contains("right operand `get_value()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_extended_binary_operator_text() {
    let result = check_temp_typepython_source(concat!(
        "def run(value: unknown) -> None:\n",
        "    _pow = value ** 2\n",
        "    _bit = value | 1\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("binary operation `**`"), "{rendered}");
    assert!(rendered.contains("binary operation `|`"), "{rendered}");
    assert!(!rendered.contains("binary operation ``"), "{rendered}");
}

#[test]
fn check_reports_unknown_bare_expression_operations_from_real_parse_pipeline() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "get_value()[0]\n",
        "get_value() + 1\n",
        "1 + get_value()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("subscript access"), "{rendered}");
    assert!(rendered.contains("binary operation `+`"), "{rendered}");
    assert!(rendered.contains("left operand `get_value()` has type `unknown`"), "{rendered}");
    assert!(rendered.contains("right operand `get_value()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_bare_expression_operations_after_typepython_surface_syntax() {
    let result = check_temp_typepython_source(concat!(
        "interface SupportsClose:\n",
        "    def close(self) -> None: ...\n\n",
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "get_value()[0]\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("subscript access"), "{rendered}");
    assert!(rendered.contains("`get_value()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_accepts_typepython_typealias_when_collecting_expression_use_sites() {
    let result = check_temp_typepython_source(concat!(
        "typealias Pair = tuple[int, int]\n\n",
        "def run(value: Pair) -> None:\n",
        "    pass\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_unknown_guard_expression_operations_from_real_parse_pipeline() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "if get_value()[0]:\n",
        "    pass\n\n",
        "assert get_value() + 1\n\n",
        "while get_value()[0]:\n",
        "    break\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("subscript access"), "{rendered}");
    assert!(rendered.contains("binary operation `+`"), "{rendered}");
    assert!(rendered.contains("left operand `get_value()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_match_guard_expression_operations() {
    let result = check_temp_typepython_source(concat!(
        "def run(value: unknown, subject: int) -> None:\n",
        "    match subject:\n",
        "        case _ if value[0]:\n",
        "            pass\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("subscript access"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_definition_header_and_raise_expression_operations() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "def decorator(value: object):\n",
        "    def wrap(fn):\n",
        "        return fn\n",
        "    return wrap\n\n",
        "@decorator(get_value()[0])\n",
        "def decorated(default: object = get_value()[0]) -> None:\n",
        "    raise get_value()[0]\n\n",
        "class Derived(get_value()[0]):\n",
        "    pass\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.matches("subscript access").count() >= 4, "{rendered}");
    assert!(rendered.contains("`get_value()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_call_argument_operations_with_local_context() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "def takes(value: object) -> None:\n",
        "    ...\n\n",
        "def run() -> None:\n",
        "    value: unknown = get_value()\n",
        "    takes(value[0])\n",
        "    takes(value + 1)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("subscript access"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
    assert!(rendered.contains("binary operation `+`"), "{rendered}");
    assert!(rendered.contains("left operand `value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_deduplicates_unknown_multiline_expression_operations() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "def takes(value: object) -> None:\n",
        "    ...\n\n",
        "item = (\n",
        "    get_value()[0]\n",
        ")\n\n",
        "takes(\n",
        "    get_value()[0]\n",
        ")\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert_eq!(rendered.matches("subscript access").count(), 2, "{rendered}");
}

#[test]
fn check_reports_unknown_truthiness_and_boolean_operations() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "def run(value: unknown, other: bool) -> None:\n",
        "    if value:\n",
        "        pass\n",
        "    if get_value():\n",
        "        pass\n",
        "    if not value:\n",
        "        pass\n",
        "    if value and other:\n",
        "        pass\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("truthiness check"), "{rendered}");
    assert!(rendered.contains("unary operation `not`"), "{rendered}");
    assert!(rendered.contains("boolean operation `and`"), "{rendered}");
    assert!(rendered.contains("operand `value` has type `unknown`"), "{rendered}");
    assert!(rendered.contains("operand `get_value()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_expression_truthiness_use_sites() {
    let result = check_temp_typepython_source(concat!(
        "def run(value: unknown, other: unknown, values: list[int]) -> None:\n",
        "    _branch = 1 if value else 0\n",
        "    _items = [item for item in values if other]\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("truthiness check"), "{rendered}");
    assert!(rendered.contains("operand `value` has type `unknown`"), "{rendered}");
    assert!(rendered.contains("operand `other` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_expression_operations_with_source_overrides_without_backing_file() {
    let result = check_virtual_source_with_overrides(
        concat!(
            "def get_value() -> unknown:\n",
            "    ...\n\n",
            "def run(value: unknown) -> None:\n",
            "    value[0]\n",
            "    if value:\n",
            "        pass\n",
            "    _item = get_value()[0]\n",
        ),
        ParseOptions::default(),
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("subscript access"), "{rendered}");
    assert!(rendered.contains("truthiness check"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
    assert!(rendered.contains("`get_value()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_comparison_membership_and_unary_operations() {
    let result = check_temp_typepython_source(concat!(
        "def run(value: unknown, values: list[int]) -> None:\n",
        "    _eq = value == 1\n",
        "    _lt = value < 1\n",
        "    _in_left = value in values\n",
        "    _in_right = 1 in value\n",
        "    _neg = -value\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("comparison `==`"), "{rendered}");
    assert!(rendered.contains("comparison `<`"), "{rendered}");
    assert!(rendered.contains("comparison `in`"), "{rendered}");
    assert!(rendered.contains("unary operation `-`"), "{rendered}");
    assert!(rendered.contains("left operand `value` has type `unknown`"), "{rendered}");
    assert!(rendered.contains("right operand `value` has type `unknown`"), "{rendered}");
    assert!(rendered.contains("operand `value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_accepts_unknown_identity_and_isinstance_guards() {
    let result = check_temp_typepython_source(concat!(
        "def run(value: unknown) -> None:\n",
        "    if value is None:\n",
        "        return\n",
        "    if value is not None:\n",
        "        pass\n",
        "    if isinstance(value, int):\n",
        "        pass\n",
        "    if not isinstance(value, str):\n",
        "        pass\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_unknown_boolop_rhs_after_isinstance_narrowing() {
    let result = check_temp_typepython_source(concat!(
        "def run(value: unknown) -> None:\n",
        "    if isinstance(value, int) and value + 1:\n",
        "        pass\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_unknown_member_access_after_is_none_boolop_guard() {
    let result = check_temp_typepython_source(concat!(
        "def run(value: unknown) -> None:\n",
        "    if value is None and value.name:\n",
        "        pass\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_member_access_after_is_not_none_boolop_guard() {
    let result = check_temp_typepython_source(concat!(
        "def run(value: unknown) -> None:\n",
        "    if value is not None and value.name:\n",
        "        pass\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_accepts_known_member_access_after_isinstance_boolop_guard() {
    let result = check_temp_typepython_source(concat!(
        "class Box:\n",
        "    name: str\n\n",
        "def run(value: unknown) -> None:\n",
        "    if isinstance(value, Box) and value.name:\n",
        "        pass\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_unknown_member_access_after_isinstance_narrowing() {
    let result = check_temp_typepython_source(concat!(
        "class Box:\n",
        "    value: int\n\n",
        "def run(value: unknown) -> None:\n",
        "    if isinstance(value, Box):\n",
        "        value.value\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_unknown_member_access_when_local_parameter_shadows_module_value() {
    let result = check_temp_typepython_source(concat!(
        "value: int = 1\n\n",
        "def run(value: unknown) -> None:\n",
        "    value.name\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_member_access_to_module_value_in_function() {
    let result = check_temp_typepython_source_with_checker_options(
        concat!(
            "def get_value() -> unknown:\n",
            "    ...\n\n",
            "value: unknown = get_value()\n\n",
            "def run() -> None:\n",
            "    value.name\n",
        ),
        ParseOptions::default(),
        crate::CheckerOptions {
            import_fallback: ImportFallback::Dynamic,
            ..crate::CheckerOptions::permissive_test_default()
        },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_accepts_local_parameter_when_module_value_is_unknown() {
    let result = check_temp_typepython_source(concat!(
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "value: unknown = get_value()\n\n",
        "def run(value: int) -> None:\n",
        "    value + 1\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4003"), "{rendered}");
}

#[test]
fn check_accepts_imported_sys_runtime_inspection_members() {
    let result = check_temp_typepython_source(concat!(
        "import sys\n\n",
        "if sys.version_info >= (3, 11):\n",
        "    pass\n",
        "if sys.platform == \"darwin\":\n",
        "    pass\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_unknown_sys_runtime_member_when_local_parameter_shadows_import() {
    let result = check_temp_typepython_source(concat!(
        "import sys\n\n",
        "def run(sys: unknown) -> None:\n",
        "    sys.version_info\n",
        "    sys.platform\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `version_info`"), "{rendered}");
    assert!(rendered.contains("member access `platform`"), "{rendered}");
    assert!(rendered.contains("`sys` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_sys_runtime_member_when_module_value_shadows_import() {
    let result = check_temp_typepython_source(concat!(
        "import sys\n\n",
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "sys: unknown = get_value()\n",
        "sys.version_info\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `version_info`"), "{rendered}");
    assert!(rendered.contains("`sys` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_member_and_method_expression_positions() {
    let result = check_temp_typepython_source(concat!(
        "def takes(value: object) -> None:\n",
        "    ...\n\n",
        "def run(value: unknown) -> object:\n",
        "    takes(value.name)\n",
        "    takes(value.method())\n",
        "    return value.name\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("method call `value.method`"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_chained_member_and_method_expression_positions() {
    let result = check_temp_typepython_source(concat!(
        "def run(value: unknown) -> None:\n",
        "    value.name.method()\n",
        "    value.name()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_operations_on_callable_unknown_return() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def run(callback: Callable[[], unknown]) -> None:\n",
        "    callback().name\n",
        "    callback().method()\n",
        "    callback()[0]\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("method call `callback().method`"), "{rendered}");
    assert!(rendered.contains("subscript access"), "{rendered}");
    assert!(rendered.contains("`callback()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_member_and_method_on_attribute_owner_expression() {
    let result = check_temp_typepython_source(concat!(
        "class Box:\n",
        "    payload: unknown\n\n",
        "def run(box: Box) -> None:\n",
        "    box.payload.name\n",
        "    box.payload.method()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("method call `box.payload.method`"), "{rendered}");
    assert!(rendered.contains("`box.payload` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_member_and_method_on_subscript_owner_expression() {
    let result = check_temp_typepython_source(concat!(
        "def run(items: list[unknown]) -> None:\n",
        "    items[0].name\n",
        "    items[0].method()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("method call `items[0].method`"), "{rendered}");
    assert!(rendered.contains("`items[0]` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_member_and_method_on_nested_attribute_owner_expression() {
    let result = check_temp_typepython_source(concat!(
        "class Inner:\n",
        "    payload: unknown\n\n",
        "class Outer:\n",
        "    inner: Inner\n\n",
        "def run(outer: Outer) -> None:\n",
        "    outer.inner.payload.name\n",
        "    outer.inner.payload.method()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("method call `outer.inner.payload.method`"), "{rendered}");
    assert!(rendered.contains("`outer.inner.payload` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_operations_on_nested_attribute_owner_expression() {
    let result = check_temp_typepython_source(concat!(
        "class Inner:\n",
        "    payload: unknown\n\n",
        "class Outer:\n",
        "    inner: Inner\n\n",
        "def run(outer: Outer) -> None:\n",
        "    outer.inner.payload + 1\n",
        "    outer.inner.payload[0]\n",
        "    if outer.inner.payload:\n",
        "        pass\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("binary operation `+`"), "{rendered}");
    assert!(rendered.contains("subscript access"), "{rendered}");
    assert!(rendered.contains("truthiness check"), "{rendered}");
    assert!(rendered.contains("`outer.inner.payload` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_direct_call_on_attribute_callee() {
    let result = check_temp_typepython_source(concat!(
        "class Box:\n",
        "    payload: unknown\n\n",
        "def run(box: Box) -> None:\n",
        "    box.payload()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("call to `box.payload`"), "{rendered}");
    assert!(rendered.contains("`box.payload` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_direct_call_on_subscript_callee() {
    let result = check_temp_typepython_source(concat!(
        "def run(items: list[unknown]) -> None:\n",
        "    items[0]()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("call to `items[0]`"), "{rendered}");
    assert!(rendered.contains("`items[0]` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_direct_call_on_nested_attribute_callee() {
    let result = check_temp_typepython_source(concat!(
        "class Inner:\n",
        "    payload: unknown\n\n",
        "class Outer:\n",
        "    inner: Inner\n\n",
        "def run(outer: Outer) -> None:\n",
        "    outer.inner.payload()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("call to `outer.inner.payload`"), "{rendered}");
    assert!(rendered.contains("`outer.inner.payload` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_operation_on_subscript_callee_result() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def run(fns: list[Callable[[], unknown]]) -> None:\n",
        "    fns[0]() + 1\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("binary operation `+`"), "{rendered}");
    assert!(rendered.contains("`fns[0]()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_accepts_method_and_typed_callable_element_calls() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "class Box:\n",
        "    label: str\n",
        "    def ping(self) -> str:\n",
        "        return self.label\n\n",
        "def run(box: Box, fns: list[Callable[[], int]]) -> None:\n",
        "    box.ping()\n",
        "    fns[0]()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4003"), "{rendered}");
}

#[test]
fn check_accepts_typed_nested_attribute_chain_operations() {
    let result = check_temp_typepython_source(concat!(
        "class Profile:\n",
        "    name: str\n\n",
        "class Client:\n",
        "    profile: Profile\n\n",
        "def run(client: Client) -> None:\n",
        "    client.profile.name.upper()\n",
        "    client.profile.name + \"x\"\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4003"), "{rendered}");
}

#[test]
fn check_reports_unknown_member_on_callable_attribute_owner_expression() {
    let result = check_temp_typepython_source(concat!(
        "class Box:\n",
        "    payload: unknown\n\n",
        "def get_box() -> Box:\n",
        "    ...\n\n",
        "def run() -> None:\n",
        "    get_box().payload.name\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("`get_box().payload` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_operations_on_callable_shadowing_function_return() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def callback() -> int:\n",
        "    return 1\n\n",
        "def run(callback: Callable[[], unknown]) -> None:\n",
        "    callback().name\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("`callback()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_operations_on_module_callable_unknown_return() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def get_callback() -> Callable[[], unknown]:\n",
        "    ...\n\n",
        "callback: Callable[[], unknown] = get_callback()\n",
        "callback().name\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("`callback()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_reports_unknown_operations_on_module_callable_unknown_return_in_function() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def get_callback() -> Callable[[], unknown]:\n",
        "    ...\n\n",
        "callback: Callable[[], unknown] = get_callback()\n\n",
        "def run() -> None:\n",
        "    callback().name\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("member access `name`"), "{rendered}");
    assert!(rendered.contains("`callback()` has type `unknown`"), "{rendered}");
}

#[test]
fn check_accepts_callable_unknown_return_operation_before_module_value_binding() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def get_callback() -> Callable[[], unknown]:\n",
        "    ...\n\n",
        "callback().name\n",
        "callback: Callable[[], unknown] = get_callback()\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4003"), "{rendered}");
    assert!(!rendered.contains("member access `name`"), "{rendered}");
}

#[test]
fn check_accepts_typed_member_and_method_expression_positions() {
    let result = check_temp_typepython_source(concat!(
        "class Client:\n",
        "    name: str\n",
        "    def get(self) -> str:\n",
        "        return self.name\n\n",
        "def takes(value: object) -> None:\n",
        "    ...\n\n",
        "def run(client: Client) -> object:\n",
        "    takes(client.name)\n",
        "    takes(client.get())\n",
        "    return client.name\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4003"), "{rendered}");
}

#[test]
fn check_accepts_builtin_iter_direct_call() {
    let result = check_temp_typepython_source(concat!(
        "def run(items: list[int]) -> object:\n",
        "    return iter(items)\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4003"), "{rendered}");
}

#[test]
fn check_reports_unknown_attribute_assignment_and_deletion() {
    let result = check_temp_typepython_source(concat!(
        "def run(value: unknown) -> None:\n",
        "    value.name = 1\n",
        "    del value.name\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("attribute assignment `value.name`"), "{rendered}");
    assert!(rendered.contains("attribute deletion `value.name`"), "{rendered}");
    assert!(rendered.contains("`value` has type `unknown`"), "{rendered}");
}

#[test]
fn check_accepts_unknown_subscript_and_arithmetic_after_explicit_cast() {
    let result = check_temp_typepython_source(concat!(
        "from typing import cast\n\n",
        "def get_value() -> unknown:\n",
        "    ...\n\n",
        "item = cast(list, get_value())[0]\n",
        "left: int = cast(int, get_value()) + 1\n",
        "right: int = 1 + cast(int, get_value())\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

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
fn check_warns_when_pure_function_uses_effectful_result() {
    let result = check_temp_typepython_source_with_effect_rows(
        concat!(
            "from typing import Callable\n\n",
            "def effect_io_net[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@effect_io_net\n",
            "def fetch() -> str:\n",
            "    return \"payload\"\n\n",
            "@effect_pure\n",
            "def parse() -> str:\n",
            "    value = fetch()\n",
            "    return value\n",
        ),
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("pure function `parse`"), "{rendered}");
    assert!(rendered.contains("effect row `io.net`"), "{rendered}");
}

#[test]
fn check_warns_for_explicit_effect_decorator_surface() {
    let result = check_temp_typepython_source_with_effect_rows(
        concat!(
            "from typing import Callable\n\n",
            "def effect(label: str):\n",
            "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "        return fn\n",
            "    return wrap\n\n",
            "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@effect(\"io.net\")\n",
            "def fetch() -> str:\n",
            "    return \"payload\"\n\n",
            "@effect_pure\n",
            "def parse() -> str:\n",
            "    return fetch()\n",
        ),
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("effect row `io.net`"), "{rendered}");
}

#[test]
fn check_warns_for_qualified_explicit_effect_decorator_surface() {
    let result = check_temp_typepython_source_with_effect_rows(
        concat!(
            "from typing import Callable\n\n",
            "class tpy:\n",
            "    @staticmethod\n",
            "    def effect(label: str):\n",
            "        def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "            return fn\n",
            "        return wrap\n\n",
            "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@tpy.effect(\"io.net\")\n",
            "def fetch() -> str:\n",
            "    return \"payload\"\n\n",
            "@effect_pure\n",
            "def parse() -> str:\n",
            "    return fetch()\n",
        ),
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("effect row `io.net`"), "{rendered}");
}

#[test]
fn check_allows_unsafe_effect_inside_unsafe_capability_scope() {
    let result = check_temp_typepython_source_with_effect_rows(
        concat!(
            "from typing import Callable\n\n",
            "def effect_unsafe[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@effect_unsafe\n",
            "def inspect_dynamic() -> str:\n",
            "    return \"payload\"\n\n",
            "@effect_pure\n",
            "def parse() -> str:\n",
            "    unsafe:\n",
            "        value = inspect_dynamic()\n",
            "        return value\n",
        ),
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4026"), "{rendered}");
}

#[test]
fn check_rejects_assigning_tainted_value_to_plain_type() {
    let result = check_temp_typepython_source_with_taint(
        concat!("raw: Tainted[str, \"html\"]\n", "safe: str = raw\n",),
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("Tainted[str, \"html\"]"), "{rendered}");
}

#[test]
fn check_accepts_explicit_taint_sanitizer_result() {
    let result = check_temp_typepython_source_with_taint(
        concat!(
            "def escape_html(value: Tainted[str, \"html\"]) -> str:\n",
            "    return \"safe\"\n\n",
            "raw: Tainted[str, \"html\"]\n",
            "safe: str = escape_html(raw)\n",
        ),
        false,
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_rejects_tainted_value_at_plain_sink() {
    let result = check_temp_typepython_source_with_taint(
        concat!(
            "def request_body() -> Tainted[str, \"html\"]:\n",
            "    ...\n\n",
            "def render_html(value: str) -> None:\n",
            "    ...\n\n",
            "render_html(request_body())\n",
        ),
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("Tainted[str, \"html\"]"), "{rendered}");
}

#[test]
fn check_accepts_tainted_source_after_sanitizer_before_sink() {
    let result = check_temp_typepython_source_with_taint(
        concat!(
            "def request_body() -> Tainted[str, \"html\"]:\n",
            "    ...\n\n",
            "def escape_html(value: Tainted[str, \"html\"]) -> str:\n",
            "    ...\n\n",
            "def render_html(value: str) -> None:\n",
            "    ...\n\n",
            "render_html(escape_html(request_body()))\n",
        ),
        false,
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_uses_source_sink_and_sanitizer_decorators_for_taint_slice() {
    let result = check_temp_typepython_source_with_taint(
        concat!(
            "from typing import Callable\n\n",
            "def source[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "def sink[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "def sanitizer[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@source\n",
            "def request_body() -> Tainted[str, \"html\"]:\n",
            "    ...\n\n",
            "@sanitizer\n",
            "def escape_html(value: Tainted[str, \"html\"]) -> str:\n",
            "    ...\n\n",
            "@sink\n",
            "def render_html(value: str) -> None:\n",
            "    ...\n\n",
            "render_html(escape_html(request_body()))\n",
        ),
        false,
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_rejects_decorated_source_flowing_through_local_to_sink() {
    let result = check_temp_typepython_source_with_taint(
        concat!(
            "from typing import Callable\n\n",
            "def source[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "def sink[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@source\n",
            "def request_body() -> str:\n",
            "    ...\n\n",
            "@sink\n",
            "def render_html(value: str) -> None:\n",
            "    ...\n\n",
            "raw = request_body()\n",
            "render_html(raw)\n",
        ),
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4028"), "{rendered}");
    assert!(rendered.contains("tainted source result flows into sink"), "{rendered}");
}

#[test]
fn check_does_not_leak_decorated_source_taint_between_functions() {
    let result = check_temp_typepython_source_with_taint(
        concat!(
            "from typing import Callable\n\n",
            "def source[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "def sink[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@source\n",
            "def request_body() -> str:\n",
            "    ...\n\n",
            "@sink\n",
            "def render_html(value: str) -> None:\n",
            "    ...\n\n",
            "def collect() -> str:\n",
            "    raw = request_body()\n",
            "    return raw\n\n",
            "def show() -> None:\n",
            "    raw = \"safe\"\n",
            "    render_html(raw)\n",
        ),
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("tainted source result flows into sink"), "{rendered}");
}

#[test]
fn check_uses_framework_adapter_taint_source_and_sink_capabilities() {
    let result = check_temp_typepython_source_with_taint(
        concat!(
            "from typing import Callable\n\n",
            "def framework_transform(**kwargs):\n",
            "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "        return fn\n",
            "    return wrap\n\n",
            "@framework_transform(kind=\"function_decorator\", capabilities=(\"taint_source\",))\n",
            "def fastapi_body[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@framework_transform(kind=\"function_decorator\", capabilities=(\"taint_sink\",))\n",
            "def html_response[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@fastapi_body\n",
            "def request_body() -> str:\n",
            "    ...\n\n",
            "@html_response\n",
            "def render_html(value: str) -> None:\n",
            "    ...\n\n",
            "raw = request_body()\n",
            "render_html(raw)\n",
        ),
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4028"), "{rendered}");
    assert!(rendered.contains("tainted source result flows into sink"), "{rendered}");
}

#[test]
fn check_rejects_decorated_source_passed_directly_to_decorated_sink() {
    let result = check_temp_typepython_source_with_taint(
        concat!(
            "from typing import Callable\n\n",
            "def source[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "def sink[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@source\n",
            "def request_body() -> str:\n",
            "    ...\n\n",
            "@sink\n",
            "def render_html(value: str) -> None:\n",
            "    ...\n\n",
            "render_html(request_body())\n",
        ),
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4028"), "{rendered}");
    assert!(rendered.contains("tainted source result flows into sink"), "{rendered}");
}

#[test]
fn check_accepts_supported_restricted_type_level_shape_aliases() {
    let result = check_temp_typepython_source(concat!(
        "class User(TypedDict):\n",
        "    id: int\n",
        "    name: str\n\n",
        "typealias Names = RequiredKeys[User]\n",
        "typealias PublicUser = Pick[User, Literal[\"id\"]]\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4027"), "{rendered}");
}

#[test]
fn check_reports_unsupported_restricted_type_level_alias() {
    let result = check_temp_typepython_source(concat!(
        "class User(TypedDict):\n",
        "    name: str\n\n",
        "typealias Names = MapValues[User, Callable]\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4027"), "{rendered}");
    assert!(rendered.contains("unsupported form `MapValues[Callable]`"), "{rendered}");
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
fn check_accepts_dual_emit_decorator_as_typepython_lowering_marker() {
    let result = check_temp_typepython_source_with_experimental_check_options(
        concat!(
            "class User:\n",
            "    name: str\n\n",
            "class AsyncClient:\n",
            "    async def get(self, path: str) -> bytes:\n",
            "        return b\"\"\n\n",
            "from typing import Callable\n\n",
            "def dual_emit[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@dual_emit\n",
            "async def fetch_user(client: AsyncClient, user_id: str) -> User:\n",
            "    payload = await client.get(user_id)\n",
            "    return User()\n",
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
    assert!(!rendered.contains("TPY4003"), "{rendered}");
}

#[test]
fn check_reports_unsupported_dual_emit_async_constructs() {
    let result = check_temp_typepython_source_with_experimental_check_options(
        concat!(
            "from typing import AsyncIterator, Callable\n\n",
            "def dual_emit[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "class Stream:\n",
            "    async def __aenter__(self) -> Stream:\n",
            "        return self\n",
            "    async def __aexit__(self, exc_type, exc, tb) -> None:\n",
            "        pass\n",
            "    def __aiter__(self) -> AsyncIterator[int]:\n",
            "        return self\n",
            "    async def __anext__(self) -> int:\n",
            "        return 1\n\n",
            "@dual_emit\n",
            "async def collect(stream: Stream) -> int:\n",
            "    total = 0\n",
            "    async with stream:\n",
            "        async for item in stream:\n",
            "            total = total + item\n",
            "    return total\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4025"), "{rendered}");
    assert!(rendered.contains("collect"), "{rendered}");
    assert!(rendered.contains("async with"), "{rendered}");
    assert!(rendered.contains("async for"), "{rendered}");
}

#[test]
fn check_ignores_dual_emit_lowering_diagnostics_without_experimental_gate() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "def dual_emit[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@dual_emit\n",
            "async def collect(stream) -> int:\n",
            "    async with stream:\n",
            "        return 1\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4025"), "{rendered}");
}

#[test]
fn check_warns_for_unclosed_lifecycle_resource() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "def must_close[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "class Resource:\n",
            "    def close(self) -> None:\n",
            "        pass\n\n",
            "@must_close\n",
            "def open_resource() -> Resource:\n",
            "    return Resource()\n\n",
            "def leak() -> None:\n",
            "    resource = open_resource()\n\n",
            "def ok() -> None:\n",
            "    resource = open_resource()\n",
            "    resource.close()\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4023"), "{rendered}");
    assert!(rendered.contains("resource"), "{rendered}");
    assert!(rendered.contains("open_resource"), "{rendered}");
    assert!(!rendered.contains("TPY4001"), "{rendered}");
}

#[test]
fn check_reports_unsupported_framework_transform_provider_in_strict_mode() {
    let result = check_temp_typepython_source_with_experimental_check_options(
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
    let result = check_temp_typepython_source_with_experimental_check_options(
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
    assert!(crate::collect_synthetic_method_stubs(&graph).is_empty());
    let methods = crate::collect_synthetic_method_stubs_with_options(
        &graph,
        framework_adapters_check_options(),
    );

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
    let methods = crate::collect_synthetic_method_stubs_with_options(
        &graph,
        framework_adapters_check_options(),
    );
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
    let methods = crate::collect_synthetic_method_stubs_with_options(
        &graph,
        framework_adapters_check_options(),
    );
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
    let overrides = crate::collect_effective_value_stub_overrides_with_options(
        &graph,
        framework_adapters_check_options(),
    );
    let display_name = overrides
        .iter()
        .find(|override_value| override_value.annotation == "str")
        .expect("computed_field should be emitted as a value stub override");

    assert_eq!(display_name.module_key, "app");

    let result = check_temp_typepython_source_with_experimental_check_options(
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
    let result = check_temp_typepython_source_with_experimental_check_options(
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
    let result = check_temp_typepython_source_with_experimental_check_options(
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
    let result = check_temp_typepython_source_with_experimental_check_options(
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
fn check_warns_for_untyped_framework_model_field() {
    let result = check_temp_typepython_source_with_experimental_check_options(
        concat!(
            "def framework_transform(*args, **kwargs):\n",
            "    def wrap(obj):\n",
            "        return obj\n",
            "    return wrap\n\n",
            "@framework_transform(kind=\"base_class\", capabilities=(\"field_collection\", \"constructor_generation\"))\n",
            "class BaseModel:\n",
            "    pass\n\n",
            "class User(BaseModel):\n",
            "    name = \"Ada\"\n",
            "    age: int = 1\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4024"), "{rendered}");
    assert!(rendered.contains("name"), "{rendered}");
    assert!(rendered.contains("missing a type annotation"), "{rendered}");
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
    let values = crate::collect_synthetic_value_stubs_with_options(
        &graph,
        framework_adapters_check_options(),
    );

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
fn check_missing_override_suggestion_uses_source_overrides_without_backing_file() {
    let source_text = concat!(
        "class Base:\n",
        "    def run(self) -> None:\n",
        "        ...\n\n",
        "class Child(Base):\n",
        "    def run(self) -> None:\n",
        "        ...\n",
    );
    let path = PathBuf::from("virtual/app.tpy");
    let tree = parse_with_options(
        SourceFile {
            path: path.clone(),
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let source_overrides = BTreeMap::from([(path.display().to_string(), source_text.to_owned())]);
    let result = check_with_source_overrides(
        &graph,
        true,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
        Some(&source_overrides),
    );

    let diagnostic = result
        .diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "TPY4005")
        .expect("missing override diagnostic should be present");
    assert_eq!(diagnostic.suggestions.len(), 1);
    assert_eq!(diagnostic.suggestions[0].replacement, "@override\n");
}

#[test]
fn check_match_case_suggestion_uses_source_overrides_without_backing_file() {
    let result = check_virtual_source_with_overrides(
        concat!(
            "from typing import Literal\n\n",
            "def describe(value: Literal[\"a\", \"b\"]) -> int:\n",
            "    match value:\n",
            "        case \"a\":\n",
            "            return 1\n",
        ),
        ParseOptions::default(),
        false,
        false,
    );

    let diagnostic = result
        .diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "TPY4009")
        .expect("non-exhaustive match diagnostic should be present");
    assert_eq!(diagnostic.suggestions.len(), 1);
    assert!(diagnostic.suggestions[0].replacement.contains("case \"b\":"));
}

#[test]
fn check_missing_none_return_suggestion_uses_source_overrides_without_backing_file() {
    let result = check_virtual_source_with_overrides(
        concat!(
            "def maybe(flag: bool) -> int:\n",
            "    if flag:\n",
            "        return 1\n",
            "    return None\n",
        ),
        ParseOptions::default(),
        false,
        false,
    );

    let diagnostic = result
        .diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "TPY4001")
        .expect("return type diagnostic should be present");
    assert_eq!(diagnostic.suggestions.len(), 1);
    assert_eq!(diagnostic.suggestions[0].replacement, "int | None");
}

#[test]
fn check_union_member_guard_suggestion_uses_source_overrides_without_backing_file() {
    let source_text = concat!("value = None\n", "value.name\n");
    let path = PathBuf::from("virtual/app.tpy");
    let source_overrides = BTreeMap::from([(path.display().to_string(), source_text.to_owned())]);
    let result = check_with_source_overrides(
        &ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: path,
                module_key: String::from("app"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    declaration! {
                        name: String::from("A"),
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
                        name: String::from("name"),
                        kind: DeclarationKind::Value,
                        metadata: Default::default(),
                        value_type_expr: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("A"),
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
                        name: String::from("B"),
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
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        metadata: value_metadata("A | B"),
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
                returns: Vec::new(),
                member_accesses: vec![typepython_binding::MemberAccessSite {
                    current_owner_name: None,
                    current_owner_type_name: None,
                    owner_name: String::from("value"),
                    member: String::from("name"),
                    through_instance: false,
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
        },
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
        Some(&source_overrides),
    );

    let diagnostic = result
        .diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "TPY4002")
        .expect("union member diagnostic should be present");
    assert_eq!(diagnostic.suggestions.len(), 1);
    assert!(diagnostic.suggestions[0].replacement.contains("assert isinstance(value, A)"));
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
    let result = crate::check_with_checker_options(
        &normalize_test_graph(&graph),
        crate::CheckerOptions { strict: true, ..framework_adapters_check_options() },
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
    let overrides = crate::collect_effective_value_stub_overrides_with_options(
        &graph,
        framework_adapters_check_options(),
    );
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
fn semantic_assignability_treats_unknown_as_checked_boundary_not_any() {
    let node = type_relation_node_with_base_child();
    let nodes = vec![node.clone()];
    let named = |name: &str| crate::SemanticType::Name(String::from(name));

    let int = named("int");
    let unknown = named("unknown");
    let dynamic = named("dynamic");
    let any = named("Any");
    let object = named("object");
    let object_or_int = crate::SemanticType::parse("object | int").expect("union should parse");

    assert!(crate::semantic_type_is_assignable(&node, &nodes, &unknown, &int));
    assert!(crate::semantic_type_is_assignable(&node, &nodes, &unknown, &dynamic));
    assert!(crate::semantic_type_is_assignable(&node, &nodes, &dynamic, &unknown));
    assert!(crate::semantic_type_is_assignable(&node, &nodes, &object, &unknown));
    assert!(crate::semantic_type_is_assignable(&node, &nodes, &object_or_int, &unknown));

    assert!(!crate::semantic_type_is_assignable(&node, &nodes, &int, &unknown));
    assert!(!crate::semantic_type_is_assignable(&node, &nodes, &any, &unknown));
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
            crate::AssignabilityOptions::default(),
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

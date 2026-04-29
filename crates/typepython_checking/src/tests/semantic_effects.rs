use super::*;

#[test]
fn check_warns_when_pure_function_uses_effectful_result() {
    let result = check_temp_typepython_source_with_check_options(
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
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("pure function `parse`"), "{rendered}");
    assert!(rendered.contains("effect row `io.net`"), "{rendered}");
}

#[test]
fn check_warns_for_explicit_effect_decorator_surface() {
    let result = check_temp_typepython_source_with_check_options(
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
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("effect row `io.net`"), "{rendered}");
}

#[test]
fn check_warns_when_caller_effect_row_does_not_cover_callee() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "def effect(label: str):\n",
            "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "        return fn\n",
            "    return wrap\n\n",
            "@effect(\"io.net\")\n",
            "def fetch() -> str:\n",
            "    return \"payload\"\n\n",
            "@effect(\"io.fs\")\n",
            "def parse() -> str:\n",
            "    return fetch()\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("function `parse`"), "{rendered}");
    assert!(rendered.contains("uncovered effect row `io.net`"), "{rendered}");
}

#[test]
fn check_accepts_when_caller_effect_row_covers_callee() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "def effect(label: str):\n",
            "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "        return fn\n",
            "    return wrap\n\n",
            "@effect(\"io.net\")\n",
            "def fetch() -> str:\n",
            "    return \"payload\"\n\n",
            "@effect(\"io.net\")\n",
            "def parse() -> str:\n",
            "    return fetch()\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4026"), "{rendered}");
}

#[test]
fn check_warns_when_pure_function_uses_bare_effectful_call_statement() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "def effect(label: str):\n",
            "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "        return fn\n",
            "    return wrap\n\n",
            "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@effect(\"io.net\")\n",
            "def send_metric() -> None:\n",
            "    ...\n\n",
            "@effect_pure\n",
            "def render() -> None:\n",
            "    send_metric()\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("pure function `render`"), "{rendered}");
    assert!(rendered.contains("effect row `io.net`"), "{rendered}");
}

#[test]
fn check_warns_for_qualified_explicit_effect_decorator_surface() {
    let result = check_temp_typepython_source_with_check_options(
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
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("effect row `io.net`"), "{rendered}");
}

#[test]
fn check_allows_unsafe_effect_inside_unsafe_capability_scope() {
    let result = check_temp_typepython_source_with_check_options(
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
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4026"), "{rendered}");
}

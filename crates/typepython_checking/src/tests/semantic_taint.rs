use super::*;

#[test]
fn check_rejects_assigning_tainted_value_to_plain_type() {
    let result = check_temp_typepython_source(concat!(
        "raw: Tainted[str, \"html\"]\n",
        "safe: str = raw\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("Tainted[str, \"html\"]"), "{rendered}");
}

#[test]
fn check_accepts_explicit_taint_sanitizer_result() {
    let result = check_temp_typepython_source(concat!(
        "def escape_html(value: Tainted[str, \"html\"]) -> str:\n",
        "    return \"safe\"\n\n",
        "raw: Tainted[str, \"html\"]\n",
        "safe: str = escape_html(raw)\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_rejects_tainted_value_at_plain_sink() {
    let result = check_temp_typepython_source(concat!(
        "def request_body() -> Tainted[str, \"html\"]:\n",
        "    ...\n\n",
        "def render_html(value: str) -> None:\n",
        "    ...\n\n",
        "render_html(request_body())\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"), "{rendered}");
    assert!(rendered.contains("Tainted[str, \"html\"]"), "{rendered}");
}

#[test]
fn check_accepts_tainted_source_after_sanitizer_before_sink() {
    let result = check_temp_typepython_source(concat!(
        "def request_body() -> Tainted[str, \"html\"]:\n",
        "    ...\n\n",
        "def escape_html(value: Tainted[str, \"html\"]) -> str:\n",
        "    ...\n\n",
        "def render_html(value: str) -> None:\n",
        "    ...\n\n",
        "render_html(escape_html(request_body()))\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_uses_source_sink_and_sanitizer_decorators_for_taint_slice() {
    let result = check_temp_typepython_source(concat!(
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
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_rejects_decorated_source_flowing_through_local_to_sink() {
    let result = check_temp_typepython_source_with_check_options(
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
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4028"), "{rendered}");
    assert!(rendered.contains("tainted source result flows into sink"), "{rendered}");
}

#[test]
fn check_does_not_leak_decorated_source_taint_between_functions() {
    let result = check_temp_typepython_source_with_check_options(
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
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("tainted source result flows into sink"), "{rendered}");
}

#[test]
fn check_uses_framework_adapter_taint_source_and_sink_capabilities() {
    let result = check_temp_typepython_source_with_check_options(
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
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4028"), "{rendered}");
    assert!(rendered.contains("tainted source result flows into sink"), "{rendered}");
}

#[test]
fn check_rejects_decorated_source_passed_directly_to_decorated_sink() {
    let result = check_temp_typepython_source_with_check_options(
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
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4028"), "{rendered}");
    assert!(rendered.contains("tainted source result flows into sink"), "{rendered}");
}

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
fn check_uses_explicit_taint_effect_labels_for_local_source_sink_flow() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "def effect(label: str):\n",
            "    def wrap(fn):\n",
            "        return fn\n",
            "    return wrap\n\n",
            "@effect(\"taint.source\")\n",
            "def request_body() -> str:\n",
            "    ...\n\n",
            "@effect(\"taint.sanitize\")\n",
            "def escape_html(value: str) -> str:\n",
            "    ...\n\n",
            "@effect(\"taint.sink\")\n",
            "def render_html(value: str) -> None:\n",
            "    ...\n\n",
            "def unsafe() -> None:\n",
            "    raw = request_body()\n",
            "    render_html(raw)\n\n",
            "def safe() -> None:\n",
            "    raw = request_body()\n",
            "    render_html(escape_html(raw))\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("TPY4028").count(), 1, "{rendered}");
    assert!(rendered.contains("tainted source result flows into sink `render_html`"), "{rendered}");
}

#[test]
fn check_uses_imported_taint_source_sink_and_sanitizer_facts() {
    let root = create_temp_typepython_root();
    let lib_path = root.join("lib.tpy");
    let app_path = root.join("app.tpy");
    let lib_source = concat!(
        "from typing import Callable\n\n",
        "def source[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "    return fn\n\n",
        "def sink[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "    return fn\n\n",
        "def sanitizer[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "    return fn\n\n",
        "@source\n",
        "def request_body() -> str:\n",
        "    ...\n\n",
        "@sanitizer\n",
        "def escape_html(value: str) -> str:\n",
        "    ...\n\n",
        "@sink\n",
        "def render_html(value: str) -> None:\n",
        "    ...\n",
    );
    let app_source = concat!(
        "from lib import escape_html, render_html, request_body\n\n",
        "def unsafe() -> None:\n",
        "    raw = request_body()\n",
        "    render_html(raw)\n\n",
        "def safe() -> None:\n",
        "    raw = request_body()\n",
        "    render_html(escape_html(raw))\n",
    );
    fs::write(&lib_path, lib_source).expect("temp source should be written");
    fs::write(&app_path, app_source).expect("temp source should be written");

    let trees = [
        parse_with_options(
            SourceFile {
                path: lib_path,
                kind: SourceKind::TypePython,
                logical_module: String::from("lib"),
                text: lib_source.to_owned(),
            },
            ParseOptions::default(),
        ),
        parse_with_options(
            SourceFile {
                path: app_path,
                kind: SourceKind::TypePython,
                logical_module: String::from("app"),
                text: app_source.to_owned(),
            },
            ParseOptions::default(),
        ),
    ];
    let bindings = trees.iter().map(bind).collect::<Vec<_>>();
    let graph = build(&bindings);
    let result = check_with_binding_metadata(
        &graph,
        &bindings,
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
        ImportFallback::Unknown,
        None,
    );

    let rendered = result.diagnostics.as_text();
    assert_eq!(rendered.matches("TPY4028").count(), 1, "{rendered}");
    assert!(rendered.contains("tainted source result flows into sink `render_html`"), "{rendered}");

    let _ = fs::remove_dir_all(&root);
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

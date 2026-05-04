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
fn check_warns_when_pure_function_uses_nested_effectful_argument() {
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
            "def render(value: str) -> str:\n",
            "    return value\n\n",
            "@effect_pure\n",
            "def parse() -> str:\n",
            "    return render(fetch())\n",
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
fn check_allows_metadata_only_effect_decorators_without_transform_resolution() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "def effect(label: str):\n",
            "    def wrap(fn):\n",
            "        return fn\n",
            "    return wrap\n\n",
            "def source(fn):\n",
            "    return fn\n\n",
            "typealias UserKeys = Literal[\"id\"]\n\n",
            "@effect(\"io.net\")\n",
            "@source\n",
            "def fetch() -> str:\n",
            "    return \"payload\"\n\n",
            "@effect(\"io.net\")\n",
            "@effect(\"taint.source\")\n",
            "def handle() -> str:\n",
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
    assert!(!rendered.contains("TPY4001"), "{rendered}");
    assert!(!rendered.contains("TPY4026"), "{rendered}");
}

#[test]
fn check_allows_metadata_effect_decorator_mixed_with_callable_transform() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "def effect(label: str):\n",
            "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "        return fn\n",
            "    return wrap\n\n",
            "def passthrough[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@effect(\"io.net\")\n",
            "@passthrough\n",
            "def fetch() -> str:\n",
            "    return \"payload\"\n",
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
fn check_warns_for_effectful_bare_call_statement_with_source_overrides_without_backing_file() {
    let result = check_virtual_source_with_overrides(
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
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("pure function `render`"), "{rendered}");
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

#[test]
fn semantic_incremental_summary_records_effect_rows() {
    let root = create_temp_typepython_root();
    let path = root.join("lib.tpy");
    let source_text = concat!(
        "from typing import Callable\n\n",
        "def effect(label: str):\n",
        "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "        return fn\n",
        "    return wrap\n\n",
        "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "    return fn\n\n",
        "@effect(\"io.net\")\n",
        "@effect(\"time\")\n",
        "def fetch() -> str:\n",
        "    return \"payload\"\n\n",
        "@effect_pure\n",
        "def parse(text: str) -> dict[str, object]:\n",
        "    return {}\n",
    );
    fs::write(&path, source_text).expect("temp source should be written");

    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("lib"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let bindings = vec![bind(&tree)];
    let graph = build(&bindings);
    let summary = semantic_incremental_state_with_binding_metadata(
        &graph,
        &bindings,
        ImportFallback::Unknown,
        None,
        None,
        typepython_incremental::SnapshotMetadata::default(),
    )
    .summaries
    .into_iter()
    .find(|summary| summary.module == "lib")
    .expect("summary should exist");

    let fetch = summary
        .solver_facts
        .effect_summaries
        .iter()
        .find(|fact| fact.name == "fetch")
        .expect("fetch effect summary should exist");
    let parse = summary
        .solver_facts
        .effect_summaries
        .iter()
        .find(|fact| fact.name == "parse")
        .expect("parse effect summary should exist");

    assert_eq!(fetch.effects, vec![String::from("io.net"), String::from("time")]);
    assert!(!fetch.pure);
    assert!(parse.effects.is_empty());
    assert!(parse.pure);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn semantic_incremental_summary_records_inferred_effect_rows() {
    let root = create_temp_typepython_root();
    let path = root.join("lib.tpy");
    let source_text = concat!(
        "from typing import Callable\n\n",
        "def effect(label: str):\n",
        "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "        return fn\n",
        "    return wrap\n\n",
        "@effect(\"io.net\")\n",
        "def fetch() -> str:\n",
        "    return \"payload\"\n\n",
        "def load() -> str:\n",
        "    return fetch()\n",
    );
    fs::write(&path, source_text).expect("temp source should be written");

    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("lib"),
            text: source_text.to_owned(),
        },
        ParseOptions::default(),
    );
    let bindings = vec![bind(&tree)];
    let graph = build(&bindings);
    let summary = semantic_incremental_state_with_binding_metadata(
        &graph,
        &bindings,
        ImportFallback::Unknown,
        None,
        None,
        typepython_incremental::SnapshotMetadata::default(),
    )
    .summaries
    .into_iter()
    .find(|summary| summary.module == "lib")
    .expect("summary should exist");

    let load = summary
        .solver_facts
        .effect_summaries
        .iter()
        .find(|fact| fact.name == "load")
        .expect("load inferred effect summary should exist");

    assert_eq!(load.effects, vec![String::from("io.net")]);
    assert!(load.sources.contains(&String::from("inferred:fetch")));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_warns_when_pure_function_calls_imported_inferred_effectful_function() {
    let root = create_temp_typepython_root();
    let lib_path = root.join("lib.tpy");
    let app_path = root.join("app.tpy");
    let lib_source = concat!(
        "from typing import Callable\n\n",
        "def effect(label: str):\n",
        "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "        return fn\n",
        "    return wrap\n\n",
        "@effect(\"io.net\")\n",
        "def fetch() -> str:\n",
        "    return \"payload\"\n\n",
        "def load() -> str:\n",
        "    return fetch()\n",
    );
    let app_source = concat!(
        "from typing import Callable\n",
        "from lib import load\n\n",
        "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "    return fn\n\n",
        "@effect_pure\n",
        "def parse() -> str:\n",
        "    return load()\n",
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
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("pure function `parse`"), "{rendered}");
    assert!(rendered.contains("effectful function `load`"), "{rendered}");
    assert!(rendered.contains("effect row `io.net`"), "{rendered}");
    assert!(rendered.contains("effect inferred from `fetch`"), "{rendered}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_warns_when_pure_function_calls_stdlib_effect_function_import() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n",
            "from time import time\n\n",
            "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@effect_pure\n",
            "def now() -> float:\n",
            "    return time()\n",
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
    assert!(rendered.contains("pure function `now`"), "{rendered}");
    assert!(rendered.contains("effectful function `time`"), "{rendered}");
    assert!(rendered.contains("effect row `time`"), "{rendered}");
    assert!(rendered.contains("stdlib `time.time`"), "{rendered}");
}

#[test]
fn check_warns_when_pure_function_calls_stdlib_effect_module_method() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n",
            "import random\n\n",
            "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@effect_pure\n",
            "def pick() -> float:\n",
            "    return random.random()\n",
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
    assert!(rendered.contains("pure function `pick`"), "{rendered}");
    assert!(rendered.contains("effectful method `random.random`"), "{rendered}");
    assert!(rendered.contains("effect row `random`"), "{rendered}");
    assert!(rendered.contains("stdlib `random.random`"), "{rendered}");
}

#[test]
fn check_warns_when_pure_function_calls_imported_effectful_function() {
    let root = create_temp_typepython_root();
    let lib_path = root.join("lib.tpy");
    let app_path = root.join("app.tpy");
    let lib_source = concat!(
        "from typing import Callable\n\n",
        "def effect(label: str):\n",
        "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "        return fn\n",
        "    return wrap\n\n",
        "@effect(\"io.net\")\n",
        "def fetch() -> str:\n",
        "    return \"payload\"\n",
    );
    let app_source = concat!(
        "from typing import Callable\n",
        "from lib import fetch\n\n",
        "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "    return fn\n\n",
        "@effect_pure\n",
        "def parse() -> str:\n",
        "    return fetch()\n",
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
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("pure function `parse`"), "{rendered}");
    assert!(rendered.contains("effect row `io.net`"), "{rendered}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_warns_when_pure_function_calls_imported_effectful_method() {
    let root = create_temp_typepython_root();
    let lib_path = root.join("lib.tpy");
    let app_path = root.join("app.tpy");
    let lib_source = concat!(
        "from typing import Callable\n\n",
        "def effect(label: str):\n",
        "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "        return fn\n",
        "    return wrap\n\n",
        "class Client:\n",
        "    @effect(\"io.net\")\n",
        "    def fetch(self) -> str:\n",
        "        return \"payload\"\n",
    );
    let app_source = concat!(
        "from typing import Callable\n",
        "from lib import Client as ApiClient\n\n",
        "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "    return fn\n\n",
        "@effect_pure\n",
        "def parse() -> str:\n",
        "    return ApiClient().fetch()\n",
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
    assert!(rendered.contains("TPY4026"), "{rendered}");
    assert!(rendered.contains("pure function `parse`"), "{rendered}");
    assert!(rendered.contains("effectful method `ApiClient.fetch`"), "{rendered}");
    assert!(rendered.contains("effect row `io.net`"), "{rendered}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_warns_for_imported_ignored_must_use_result() {
    let root = create_temp_typepython_root();
    let lib_path = root.join("lib.tpy");
    let app_path = root.join("app.tpy");
    let lib_source = concat!(
        "from typing import Callable\n\n",
        "def must_use[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "    return fn\n\n",
        "@must_use\n",
        "def start_job() -> str:\n",
        "    return \"job\"\n",
    );
    let app_source =
        concat!("from lib import start_job\n\n", "def run() -> None:\n", "    start_job()\n",);
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
    assert!(rendered.contains("TPY4022"), "{rendered}");
    assert!(rendered.contains("result of `start_job` call is ignored"), "{rendered}");
    assert!(rendered.contains("@must_use"), "{rendered}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_warns_for_imported_unclosed_lifecycle_resource() {
    let root = create_temp_typepython_root();
    let lib_path = root.join("lib.tpy");
    let app_path = root.join("app.tpy");
    let lib_source = concat!(
        "from typing import Callable\n\n",
        "def must_close[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
        "    return fn\n\n",
        "@must_close\n",
        "def open_resource() -> object:\n",
        "    return object()\n",
    );
    let app_source = concat!(
        "from lib import open_resource\n\n",
        "def run() -> None:\n",
        "    resource = open_resource()\n",
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
    assert!(rendered.contains("TPY4023"), "{rendered}");
    assert!(rendered.contains("resource `resource` created by `open_resource`"), "{rendered}");
    assert!(rendered.contains("not closed before scope exit"), "{rendered}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_uses_framework_adapter_effect_capabilities() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "def framework_transform(**kwargs):\n",
            "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "        return fn\n",
            "    return wrap\n\n",
            "@framework_transform(kind=\"function_decorator\", capabilities=(\"effect_io_net\", \"effect_time\"))\n",
            "def remote_call[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "@remote_call\n",
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
    assert!(rendered.contains("effect row `io.net, time`"), "{rendered}");
    assert!(rendered.contains("framework_transform capability on `remote_call`"), "{rendered}");
}

#[test]
fn check_warns_when_pure_function_calls_effectful_method_result() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "def effect(label: str):\n",
            "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "        return fn\n",
            "    return wrap\n\n",
            "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "class Client:\n",
            "    @effect(\"io.net\")\n",
            "    def fetch(self) -> str:\n",
            "        return \"payload\"\n\n",
            "@effect_pure\n",
            "def parse() -> str:\n",
            "    return Client().fetch()\n",
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
    assert!(rendered.contains("effectful method `Client.fetch`"), "{rendered}");
    assert!(rendered.contains("effect row `io.net`"), "{rendered}");
}

#[test]
fn check_warns_when_pure_function_uses_bare_effectful_method_statement() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "def effect(label: str):\n",
            "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "        return fn\n",
            "    return wrap\n\n",
            "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "class Client:\n",
            "    @effect(\"io.net\")\n",
            "    def send(self) -> None:\n",
            "        ...\n\n",
            "@effect_pure\n",
            "def render() -> None:\n",
            "    Client().send()\n",
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
    assert!(rendered.contains("effectful method `Client.send`"), "{rendered}");
}

#[test]
fn check_does_not_confuse_effectful_method_with_same_named_function() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "from typing import Callable\n\n",
            "def effect(label: str):\n",
            "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "        return fn\n",
            "    return wrap\n\n",
            "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
            "    return fn\n\n",
            "class Client:\n",
            "    @effect(\"io.net\")\n",
            "    def fetch(self) -> str:\n",
            "        return \"payload\"\n\n",
            "def fetch() -> str:\n",
            "    return \"local\"\n\n",
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
    assert!(!rendered.contains("TPY4026"), "{rendered}");
}

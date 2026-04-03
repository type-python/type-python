use super::{check, check_with_binding_metadata, check_with_options};
use std::{
    fs,
    io::ErrorKind,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use typepython_binding::{
    Declaration, DeclarationKind, DeclarationOwner, DeclarationOwnerKind, bind,
};
use typepython_config::{DiagnosticLevel, ImportFallback};
use typepython_graph::{ModuleGraph, ModuleNode, build};
use typepython_syntax::{ParseOptions, SourceFile, SourceKind, parse_with_options};

static TEMP_SOURCE_ROOT_ID: AtomicU64 = AtomicU64::new(0);

fn create_temp_typepython_root() -> PathBuf {
    let temp_dir = std::env::temp_dir();
    loop {
        let unique = TEMP_SOURCE_ROOT_ID.fetch_add(1, Ordering::Relaxed);
        let root = temp_dir.join(format!("typepython-checking-{}-{unique}", std::process::id()));
        match fs::create_dir(&root) {
            Ok(()) => return root,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("temp directory should be created: {error}"),
        }
    }
}

fn check_temp_typepython_source(source_text: &str) -> super::CheckResult {
    check_temp_typepython_source_with_options(source_text, ParseOptions::default())
}

fn check_temp_typepython_source_with_options(
    source_text: &str,
    options: ParseOptions,
) -> super::CheckResult {
    check_temp_typepython_source_with_check_options(
        source_text,
        options,
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
    )
}

fn check_temp_typepython_source_with_check_options(
    source_text: &str,
    options: ParseOptions,
    require_explicit_overrides: bool,
    enable_sealed_exhaustiveness: bool,
    report_deprecated: DiagnosticLevel,
    strict: bool,
    warn_unsafe: bool,
) -> super::CheckResult {
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    fs::write(&path, source_text).expect("temp source should be written");

    let source = SourceFile {
        path: path.clone(),
        kind: SourceKind::TypePython,
        logical_module: String::from("app"),
        text: source_text.to_owned(),
    };
    let tree = parse_with_options(source, options);
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let result = check_with_options(
        &graph,
        require_explicit_overrides,
        enable_sealed_exhaustiveness,
        report_deprecated,
        strict,
        warn_unsafe,
        ImportFallback::Unknown,
    );

    let _ = fs::remove_dir_all(&root);
    result
}

fn check_temp_project_sources(sources: &[(&str, &str, SourceKind, &str)]) -> super::CheckResult {
    let root = create_temp_typepython_root();
    let bindings = sources
        .iter()
        .map(|(relative_path, logical_module, kind, source_text)| {
            let path = root.join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("temp source parent should be created");
            }
            fs::write(&path, source_text).expect("temp source should be written");

            let source = SourceFile {
                path,
                kind: *kind,
                logical_module: (*logical_module).to_owned(),
                text: (*source_text).to_owned(),
            };
            bind(&parse_with_options(source, ParseOptions::default()))
        })
        .collect::<Vec<_>>();
    let graph = build(&bindings);
    let result = check(&graph);

    let _ = fs::remove_dir_all(&root);
    result
}

fn check_virtual_binding_metadata_source(source_text: &str) -> super::CheckResult {
    let source = SourceFile {
        path: PathBuf::from("virtual/app.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::from("app"),
        text: source_text.to_owned(),
    };
    let tree = parse_with_options(source, ParseOptions::default());
    let binding = bind(&tree);
    let graph = build(std::slice::from_ref(&binding));

    check_with_binding_metadata(
        &graph,
        &[binding],
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
        None,
    )
}

#[test]
fn check_with_binding_metadata_uses_bound_typed_dict_facts_without_reading_source_file() {
    let source_text = "from typing import TypedDict\nclass Config(TypedDict, total=False):\n    name: str\nconfig: Config = {}\n";
    let source = SourceFile {
        path: PathBuf::from("virtual/app.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::from("app"),
        text: source_text.to_owned(),
    };
    let tree = parse_with_options(source, ParseOptions::default());
    let binding = bind(&tree);
    assert_eq!(
        binding
            .surface_facts
            .typed_dict_class_metadata
            .get("Config")
            .and_then(|metadata| metadata.total),
        Some(false),
        "binding should preserve TypedDict(total=False) metadata"
    );
    let graph = build(std::slice::from_ref(&binding));

    let result = check_with_binding_metadata(
        &graph,
        &[binding],
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
        None,
    );

    assert!(
        !result.diagnostics.has_errors(),
        "bound surface metadata should preserve TypedDict(total=False) behavior without a backing file: {:?}",
        result.diagnostics.diagnostics
    );
}

#[test]
fn check_with_binding_metadata_uses_bound_typed_dict_facts_in_contextual_collections() {
    let result = check_virtual_binding_metadata_source(
        "from typing import TypedDict\nclass Config(TypedDict, total=False):\n    name: str\nitems: list[Config] = [{}]\n",
    );

    assert!(
        !result.diagnostics.has_errors(),
        "bound surface metadata should preserve nested TypedDict(total=False) behavior without a backing file: {:?}",
        result.diagnostics.diagnostics
    );
}

#[test]
fn check_with_binding_metadata_uses_bound_dataclass_transform_facts_without_reading_source_file() {
    let result = check_virtual_binding_metadata_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\nuser: User = User(\"Ada\")\n",
    );

    assert!(
        !result.diagnostics.has_errors(),
        "bound surface metadata should preserve dataclass-transform constructor facts without a backing file: {:?}",
        result.diagnostics.diagnostics
    );
}

fn type_relation_node_with_base_child() -> ModuleNode {
    ModuleNode {
        module_path: PathBuf::from("<type-relations>"),
        module_key: String::new(),
        module_kind: SourceKind::TypePython,
        declarations: vec![
            Declaration {
                name: String::from("Base"),
                kind: DeclarationKind::Class,
                detail: String::new(),
                value_type: None,
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
                name: String::from("Child"),
                kind: DeclarationKind::Class,
                detail: String::from("Base"),
                value_type: None,
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
                bases: vec![String::from("Base")],
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
        summary_fingerprint: 0,
        calls: Vec::new(),
        method_calls: Vec::new(),
    }
}

#[test]
fn check_reports_duplicate_module_symbols() {
    let graph = ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
        typepython_syntax::DirectExprMetadata {
            value_type: Some(String::from(value_type)),
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
        }
    }

    let call = typepython_binding::CallSite {
        callee: String::from("choose"),
        arg_count: 1,
        arg_types: vec![String::from("dict[str, object]")],
        arg_values: vec![typepython_syntax::DirectExprMetadata {
            value_type: Some(String::from("dict[str, object]")),
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
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };
    let typed_dict_overload = Declaration {
        name: String::from("choose"),
        kind: DeclarationKind::Overload,
        detail: String::from("(user:User)->int"),
        value_type: None,
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
    let string_overload =
        Declaration { detail: String::from("(user:str)->str"), ..typed_dict_overload.clone() };
    let typed_dict_class = Declaration {
        name: String::from("User"),
        kind: DeclarationKind::Class,
        detail: String::new(),
        value_type: None,
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
    let id_field = Declaration {
        name: String::from("id"),
        kind: DeclarationKind::Value,
        detail: String::from("int"),
        value_type: None,
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
    let name_field =
        Declaration { name: String::from("name"), detail: String::from("str"), ..id_field.clone() };
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

#[test]
fn check_reports_unresolved_paramspec_call() {
    let result = check_temp_typepython_source(
        "from typing import Callable, ParamSpec\n\nP = ParamSpec(\"P\")\n\ndef invoke(cb: Callable[P, int]) -> int:\n    return cb()\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4014"));
    assert!(rendered.contains("Callable[P, int]"));
}

#[test]
fn check_reports_unresolved_concatenate_call() {
    let result = check_temp_typepython_source(
        "from typing import Callable, Concatenate, ParamSpec\n\nP = ParamSpec(\"P\")\n\ndef invoke(cb: Callable[Concatenate[int, P], int]) -> int:\n    return cb(1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4014"));
    assert!(rendered.contains("Concatenate[int, P]"));
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
        super::resolve_function_provider_with_node(&graph.nodes, node, "stringify")
            .expect("decorator provider");
    let base_callable = String::from("Callable[[int], int]");
    let fake_call = super::synthetic_decorator_application_call(&decorator.name, &base_callable);
    let instantiated_signature = super::resolve_instantiated_direct_function_signature(
        decorator_node,
        &graph.nodes,
        decorator,
        &fake_call,
    );
    assert!(instantiated_signature.is_some(), "instantiated signature");
    let instantiated_return = super::resolve_instantiated_callable_return_type_from_declaration(
        decorator_node,
        &graph.nodes,
        decorator,
        &fake_call,
    );
    assert_eq!(instantiated_return, Some(String::from("Callable[[int], str]")));
    assert_eq!(
        super::apply_named_callable_decorator_transform(
            decorator_node,
            &graph.nodes,
            &decorator.name,
            &base_callable,
        ),
        Some(String::from("Callable[[int], str]"))
    );

    assert_eq!(
        super::resolve_decorated_function_callable_annotation(node, &graph.nodes, "count"),
        Some(String::from("Callable[[int], str]"))
    );
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
        ParseOptions { enable_conditional_returns: true },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4018"));
    assert!(rendered.contains("missing: None"));
}

#[test]
fn check_accepts_complete_conditional_return_coverage() {
    let result = check_temp_typepython_source_with_options(
        "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n",
        ParseOptions { enable_conditional_returns: true },
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
fn check_reports_readonly_typed_dict_extra_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict, extra_items=ReadOnly[int]):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"age\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"));
    assert!(rendered.contains("cannot be assigned"));
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
                Declaration {
                    name: String::from("UserId"),
                    kind: DeclarationKind::TypeAlias,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_overload_sets_with_one_implementation() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_overloads_without_concrete_implementation() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::new(),
                    value_type: None,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4004"));
    assert!(rendered.contains("without a concrete implementation"));
}

#[test]
fn check_reports_overloads_with_multiple_concrete_implementations() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4004"));
    assert!(rendered.contains("more than one concrete implementation"));
}

#[test]
fn check_reports_ambiguous_overload_resolution() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::from("(value:int)->int"),
                    value_type: None,
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
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::from("(value:int)->str"),
                    value_type: None,
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
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:int)->int"),
                    value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("parse"),
                arg_count: 1,
                arg_types: vec![String::from("int")],
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4012"));
    assert!(rendered.contains("ambiguous across 2 overloads"));
}

#[test]
fn check_accepts_direct_overloaded_call_assignment_type_match() {
    let result = check_temp_typepython_source(
        "overload def parse(value: int) -> str: ...\noverload def parse(value: str) -> int: ...\ndef parse(value):\n    return value\n\nresult: str = parse(1)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_direct_overloaded_call_return_type_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::from("(value:int)->str"),
                    value_type: None,
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
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::from("(value:str)->int"),
                    value_type: None,
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
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:int)->int"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->str"),
                    value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("parse"),
                arg_count: 1,
                arg_types: vec![String::from("int")],
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("parse")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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
fn check_accepts_imported_overloaded_call_assignment_type_match() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("/tmp/pkg/util.pyi"),
                module_key: String::from("pkg.util"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::from("(value:int)->str"),
                        value_type: None,
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
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::from("(value:str)->int"),
                        value_type: None,
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
            },
            ModuleNode {
                module_path: PathBuf::from("/tmp/app.tpy"),
                module_key: String::from("app"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Import,
                        detail: String::from("pkg.util.parse"),
                        value_type: None,
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
                        name: String::from("result"),
                        kind: DeclarationKind::Value,
                        detail: String::from("str"),
                        value_type: None,
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("parse")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("parse"),
                    arg_count: 1,
                    arg_types: vec![String::from("int")],
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }],
                method_calls: Vec::new(),
            },
        ],
    });

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_non_applicable_overload_as_call_incompatibility() {
    let result = check_temp_typepython_source(
        "overload def parse(value: int) -> str: ...\noverload def parse(value: str) -> int: ...\ndef parse(value: int) -> str:\n    return \"x\"\n\nparse(None)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(!rendered.contains("TPY4012"));
}

#[test]
fn overload_applicability_accepts_keyword_default_and_semantic_match() {
    let call = typepython_binding::CallSite {
        callee: String::from("parse"),
        arg_count: 0,
        arg_types: Vec::new(),
        arg_values: Vec::new(),
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: vec![String::from("value")],
        keyword_arg_types: vec![String::from("None")],
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };
    let declaration = Declaration {
        name: String::from("parse"),
        kind: DeclarationKind::Overload,
        detail: String::from("(value:Optional[int]=)->int"),
        value_type: None,
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

    assert!(crate::overload_is_applicable(&call, &declaration));
}

#[test]
fn overload_applicability_rejects_positional_only_keyword() {
    let call = typepython_binding::CallSite {
        callee: String::from("parse"),
        arg_count: 0,
        arg_types: Vec::new(),
        arg_values: Vec::new(),
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: vec![String::from("value")],
        keyword_arg_types: vec![String::from("int")],
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };
    let declaration = Declaration {
        name: String::from("parse"),
        kind: DeclarationKind::Overload,
        detail: String::from("(value:int,/)->int"),
        value_type: None,
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

    assert!(!crate::overload_is_applicable(&call, &declaration));
}

#[test]
fn overload_applicability_accepts_variadic_arguments() {
    let call = typepython_binding::CallSite {
        callee: String::from("parse"),
        arg_count: 3,
        arg_types: vec![String::from("int"), String::from("int"), String::from("int")],
        arg_values: Vec::new(),
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };
    let declaration = Declaration {
        name: String::from("parse"),
        kind: DeclarationKind::Overload,
        detail: String::from("(*args:int)->int"),
        value_type: None,
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

    assert!(crate::overload_is_applicable(&call, &declaration));
}

#[test]
fn overload_applicability_accepts_nominal_subclass_arguments() {
    let call = typepython_binding::CallSite {
        callee: String::from("parse"),
        arg_count: 1,
        arg_types: vec![String::from("Child")],
        arg_values: Vec::new(),
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };
    let declaration = Declaration {
        name: String::from("parse"),
        kind: DeclarationKind::Overload,
        detail: String::from("(value:Base)->int"),
        value_type: None,
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

    let node = typepython_graph::ModuleNode {
        module_path: PathBuf::from("src/app/module.tpy"),
        module_key: String::new(),
        module_kind: SourceKind::TypePython,
        declarations: vec![
            Declaration {
                name: String::from("Base"),
                kind: DeclarationKind::Class,
                detail: String::new(),
                value_type: None,
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
                name: String::from("Child"),
                kind: DeclarationKind::Class,
                detail: String::new(),
                value_type: None,
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
                bases: vec![String::from("Base")],
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
    };

    assert!(crate::overload_is_applicable_with_context(
        &node,
        std::slice::from_ref(&node),
        &call,
        &declaration
    ));
}

#[test]
fn overload_applicability_accepts_list_for_sequence_parameter() {
    let call = typepython_binding::CallSite {
        callee: String::from("parse"),
        arg_count: 1,
        arg_types: vec![String::from("list[int]")],
        arg_values: Vec::new(),
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };
    let declaration = Declaration {
        name: String::from("parse"),
        kind: DeclarationKind::Overload,
        detail: String::from("(value:Sequence[int])->int"),
        value_type: None,
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

    let node = typepython_graph::ModuleNode {
        module_path: PathBuf::from("src/app/module.tpy"),
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
        summary_fingerprint: 1,
        calls: Vec::new(),
        method_calls: Vec::new(),
    };

    assert!(crate::overload_is_applicable_with_context(
        &node,
        std::slice::from_ref(&node),
        &call,
        &declaration
    ));
}

#[test]
fn overload_applicability_uses_contextual_lambda_callable_types() {
    fn direct_expr(value_type: &str) -> typepython_syntax::DirectExprMetadata {
        typepython_syntax::DirectExprMetadata {
            value_type: Some(String::from(value_type)),
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
        }
    }

    let lambda_arg = typepython_syntax::DirectExprMetadata {
        value_type: Some(String::new()),
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
        value_lambda: Some(Box::new(typepython_syntax::LambdaMetadata {
            params: vec![typepython_syntax::FunctionParam {
                name: String::from("x"),
                annotation: None,
                has_default: false,
                positional_only: false,
                keyword_only: false,
                variadic: false,
                keyword_variadic: false,
            }],
            body: Box::new(direct_expr("str")),
        })),
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None,
    };
    let call = typepython_binding::CallSite {
        callee: String::from("choose"),
        arg_count: 1,
        arg_types: vec![String::new()],
        arg_values: vec![lambda_arg],
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };
    let str_declaration = Declaration {
        name: String::from("choose"),
        kind: DeclarationKind::Overload,
        detail: String::from("(fn:Callable[[int],str])->str"),
        value_type: None,
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
    let int_declaration = Declaration {
        detail: String::from("(fn:Callable[[int],int])->int"),
        ..str_declaration.clone()
    };

    assert!(crate::overload_is_applicable(&call, &str_declaration));
    assert!(!crate::overload_is_applicable(&call, &int_declaration));
}

#[test]
fn check_accepts_variadic_direct_calls() {
    let result = check_temp_typepython_source(
        "def takes(*args: int):\n    return 0\n\ndef kw(**kwargs: int):\n    return 0\n\ntakes(1, 2, 3)\nkw(x=1, y=2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_duplicate_function_parameter_binding() {
    let result =
        check_temp_typepython_source("def takes(x: int):\n    return x\n\ntakes(1, x=2)\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("binds parameter `x` both positionally and by keyword"));
}

#[test]
fn check_reports_duplicate_constructor_parameter_binding() {
    let result = check_temp_typepython_source(
        "class User:\n    def __init__(self, age: int):\n        self.age = age\n\nUser(1, age=2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("binds parameter `age` both positionally and by keyword"));
}

#[test]
fn check_reports_duplicate_method_parameter_binding() {
    let result = check_temp_typepython_source(
        "class User:\n    def set_age(self, age: int):\n        self.age = age\n\nuser = User()\nuser.set_age(1, age=2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("binds parameter `age` both positionally and by keyword"));
}

#[test]
fn check_accepts_stub_overloaded_method_keyword_calls() {
    let graph = ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("/tmp/pkg/util.pyi"),
                module_key: String::from("pkg.util"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    Declaration {
                        name: String::from("User"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
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
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::from("(self,value:int)->int"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            kind: DeclarationOwnerKind::Class,
                            name: String::from("User"),
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
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::from("(self,value:str)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            kind: DeclarationOwnerKind::Class,
                            name: String::from("User"),
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
                summary_fingerprint: 0,
            },
            ModuleNode {
                module_path: PathBuf::from("/tmp/app.tpy"),
                module_key: String::from("app"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("User"),
                        kind: DeclarationKind::Import,
                        detail: String::from("pkg.util.User"),
                        value_type: None,
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
                        name: String::from("user"),
                        kind: DeclarationKind::Value,
                        detail: String::from("User"),
                        value_type: Some(String::from("User")),
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
                method_calls: vec![typepython_binding::MethodCallSite {
                    owner_name: String::from("user"),
                    method: String::from("parse"),
                    through_instance: false,
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: vec![String::from("value")],
                    keyword_arg_types: vec![String::from("str")],
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }],
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
            },
        ],
    };

    let result = check(&graph);
    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_stub_overloaded_method_return_type() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("/tmp/pkg/util.pyi"),
                module_key: String::from("pkg.util"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    Declaration {
                        name: String::from("User"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
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
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::from("(self,value:int)->int"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
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
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::from("(self,value:str)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
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
            },
            ModuleNode {
                module_path: PathBuf::from("/tmp/app.tpy"),
                module_key: String::from("app"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("User"),
                        kind: DeclarationKind::Import,
                        detail: String::from("pkg.util.User"),
                        value_type: None,
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
                        name: String::from("user"),
                        kind: DeclarationKind::Value,
                        detail: String::from("User"),
                        value_type: None,
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
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        detail: String::from("str"),
                        value_type: None,
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("value"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("user")),
                    value_method_name: Some(String::from("parse")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: vec![typepython_binding::MethodCallSite {
                    owner_name: String::from("user"),
                    method: String::from("parse"),
                    through_instance: false,
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: vec![String::from("value")],
                    keyword_arg_types: vec![String::from("str")],
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }],
            },
        ],
    });

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("assigns `int` where `value` expects `str`"), "{rendered}");
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_imported_defaulted_function_call() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("/tmp/lib.pyi"),
                module_key: String::from("lib"),
                module_kind: SourceKind::Stub,
                declarations: vec![Declaration {
                    name: String::from("f"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(x:int,y:int=)->None"),
                    value_type: None,
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
            },
            ModuleNode {
                module_path: PathBuf::from("/tmp/app.tpy"),
                module_key: String::from("app"),
                module_kind: SourceKind::TypePython,
                declarations: vec![Declaration {
                    name: String::from("f"),
                    kind: DeclarationKind::Import,
                    detail: String::from("lib.f"),
                    value_type: None,
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
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("f"),
                    arg_count: 1,
                    arg_types: vec![String::from("int")],
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }],
                method_calls: Vec::new(),
            },
        ],
    });

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_stub_only_overload_sets_in_pyi_modules() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("types/module.pyi"),
            module_key: String::new(),
            module_kind: SourceKind::Stub,
            declarations: vec![
                Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::new(),
                    value_type: None,
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_duplicate_interface_members() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("SupportsClose"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("close"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
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
                Declaration {
                    name: String::from("close"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4004"));
    assert!(rendered.contains("SupportsClose"));
    assert!(rendered.contains("member `close` more than once"));
}

#[test]
fn check_accepts_class_method_overload_group() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("Parser"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Parser"),
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
                Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Parser"),
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_final_reassignment_in_module_scope() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("MAX_SIZE"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: true,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("MAX_SIZE"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4006"));
    assert!(rendered.contains("Final binding `MAX_SIZE`"));
}

#[test]
fn check_reports_final_augmented_reassignment_in_module_scope() {
    let result = check_temp_typepython_source(
        "from typing import Final\n\nMAX_SIZE: Final[int] = 1\nMAX_SIZE += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4006"));
    assert!(rendered.contains("Final binding `MAX_SIZE`"));
}

#[test]
fn check_reports_final_reassignment_in_class_scope() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("limit"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
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
                    is_final: true,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("limit"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4006"));
    assert!(rendered.contains("type `Box`"));
    assert!(rendered.contains("Final binding `limit`"));
}

#[test]
fn check_reports_final_augmented_reassignment_in_local_scope() {
    let result = check_temp_typepython_source(
        "from typing import Final\n\ndef build() -> None:\n    limit: Final[int] = 1\n    limit += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4006"));
    assert!(rendered.contains("Final binding `limit`"));
}

#[test]
fn check_reports_final_attribute_assignment() {
    let result = check_temp_typepython_source(
        "from typing import Final\n\nclass Box:\n    limit: Final[int] = 1\n\ndef mutate(box: Box) -> None:\n    box.limit = 2\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4006"));
    assert!(rendered.contains("Final binding `limit`"));
}

#[test]
fn check_reports_final_attribute_augmented_assignment() {
    let result = check_temp_typepython_source(
        "from typing import Final\n\nclass Box:\n    limit: Final[int] = 1\n\ndef mutate(box: Box) -> None:\n    box.limit += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4006"));
    assert!(rendered.contains("Final binding `limit`"));
}

#[test]
fn check_reports_overriding_base_final_member() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("limit"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Base"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: true,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("Derived"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Base")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("limit"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Derived"),
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4006"));
    assert!(rendered.contains("overrides Final member `limit` from base `Base`"));
}

#[test]
fn check_reports_subclassing_final_class() {
    let graph = ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: true,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("Child"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Base")],
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("flag"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("bool")),
                value_type: Some(String::from("int")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    };

    let result = check(&graph);
    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4005"));
    assert!(rendered.contains("subclasses final class `Base`"));
}

#[test]
fn check_reports_subclassing_imported_final_class() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/base.py"),
                module_key: String::from("app.base"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: true,
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
                module_path: PathBuf::from("src/app/child.py"),
                module_key: String::from("app.child"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Import,
                        detail: String::from("app.base.Base"),
                        value_type: None,
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
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Base"),
                        value_type: None,
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
                        bases: vec![String::from("Base")],
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
                summary_fingerprint: 2,
            },
        ],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4005"));
    assert!(rendered.contains("subclasses final class `Base`"));
}

#[test]
fn check_reports_overriding_final_method() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Base"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: true,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("Child"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Base")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Child"),
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("missing"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("None")),
                value_type: Some(String::from("int")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4005"));
    assert!(rendered.contains("overrides final member `run` from base `Base`"));
}

#[test]
fn check_reports_missing_interface_members() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("SupportsClose"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Interface),
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
                    name: String::from("close"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
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
                Declaration {
                    name: String::from("Widget"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("SupportsClose")],
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4008"));
    assert!(rendered.contains("does not implement interface member `close`"));
}

#[test]
fn check_reports_incompatible_interface_member_signature() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("SupportsClose"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Interface),
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
                    name: String::from("close"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
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
                Declaration {
                    name: String::from("Widget"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("SupportsClose")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("close"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Widget"),
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("flag"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("bool")),
                value_type: Some(String::from("int")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4008"));
    assert!(rendered.contains("incompatible signature or annotation"));
}

#[test]
fn check_reports_incompatible_imported_interface_member_signature() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/protocols.tpy"),
                module_key: String::from("app.protocols"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("SupportsClose"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
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
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->int"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("SupportsClose"),
                            kind: DeclarationOwnerKind::Interface,
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
            ModuleNode {
                module_path: PathBuf::from("src/app/impl.tpy"),
                module_key: String::from("app.impl"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("SupportsClose"),
                        kind: DeclarationKind::Import,
                        detail: String::from("app.protocols.SupportsClose"),
                        value_type: None,
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
                        name: String::from("Widget"),
                        kind: DeclarationKind::Class,
                        detail: String::from("SupportsClose"),
                        value_type: None,
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
                        bases: vec![String::from("SupportsClose")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Widget"),
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
                summary_fingerprint: 2,
            },
        ],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4008"));
    assert!(rendered.contains("incompatible signature or annotation"));
}

#[test]
fn check_reports_missing_abstract_base_members() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Base"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_async: false,
                    is_override: false,
                    is_abstract_method: true,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("Child"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Base")],
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4008"));
    assert!(rendered.contains("does not implement abstract member `run`"));
}

#[test]
fn check_reports_direct_instantiation_of_abstract_class() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Base"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_async: false,
                    is_override: false,
                    is_abstract_method: true,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
            ],
            calls: vec![typepython_binding::CallSite {
                callee: String::from("Base"),
                arg_count: 0,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4007"));
    assert!(rendered.contains("directly instantiates abstract class `Base`"));
}

#[test]
fn check_reports_direct_instantiation_of_imported_abstract_class() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/base.py"),
                module_key: String::from("app.base"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
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
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->None"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: true,
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
            ModuleNode {
                module_path: PathBuf::from("src/app/use.py"),
                module_key: String::from("app.use"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Import,
                    detail: String::from("app.base.Base"),
                    value_type: None,
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
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("Base"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }],
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
                summary_fingerprint: 2,
            },
        ],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4007"));
    assert!(rendered.contains("directly instantiates abstract class `Base`"));
}

#[test]
fn check_reports_unresolved_same_project_imports() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/use.py"),
            module_key: String::from("app.use"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("Missing"),
                kind: DeclarationKind::Import,
                detail: String::from("app.missing.Missing"),
                value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("missing"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("None")),
                value_type: Some(String::from("int")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY3001"));
    assert!(rendered.contains("app.missing.Missing"));
}

#[test]
fn check_accepts_import_of_same_project_namespace_package_root() {
    let result = check_temp_project_sources(&[
        (
            "pkg/util.py",
            "pkg.util",
            SourceKind::Python,
            "def greet(name: str) -> str:\n    return name\n",
        ),
        ("app.py", "app", SourceKind::Python, "import pkg\n"),
    ]);

    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got:\n{}",
        result.diagnostics.as_text()
    );
}

#[test]
fn check_accepts_namespace_submodule_method_call_through_from_import() {
    let result = check_temp_project_sources(&[
        (
            "pkg/util.py",
            "pkg.util",
            SourceKind::Python,
            "def greet(name: str) -> str:\n    return name\n",
        ),
        (
            "app.py",
            "app",
            SourceKind::Python,
            "from pkg import util\nvalue: str = util.greet(\"Ada\")\n",
        ),
    ]);

    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got:\n{}",
        result.diagnostics.as_text()
    );
}

#[test]
fn check_reports_namespace_submodule_method_call_result_assignment_mismatch() {
    let result = check_temp_project_sources(&[
        (
            "pkg/util.py",
            "pkg.util",
            SourceKind::Python,
            "def greet(name: str) -> str:\n    return name\n",
        ),
        (
            "app.py",
            "app",
            SourceKind::Python,
            "from pkg import util\nvalue: int = util.greet(\"Ada\")\n",
        ),
    ]);

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `str` where `value` expects `int`"));
}

#[test]
fn check_reports_direct_call_arity_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(x:int,y:int)->None"),
                value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("build"),
                arg_count: 1,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("missing required argument(s): y"));
}

#[test]
fn check_reports_direct_call_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(x:int,y:str)->None"),
                value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("build"),
                arg_count: 2,
                arg_types: vec![String::from("str"), String::from("int")],
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("passes `str` where parameter expects `int`"));
}

#[test]
fn check_reports_direct_return_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("()->int"),
                value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::from("str")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `str` where `build` expects `int`"));
}

#[test]
fn check_reports_direct_bool_return_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("()->bool"),
                value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::from("str")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `str` where `build` expects `bool`"));
}

#[test]
fn check_reports_direct_none_return_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("()->None"),
                value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::from("int")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `int` where `build` expects `None`"));
}

#[test]
fn check_accepts_direct_returned_call_result_type_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("helper"),
                arg_count: 0,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("helper")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_direct_returned_call_result_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->str"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("helper"),
                arg_count: 0,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("helper")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `str` where `build` expects `int`"));
}

#[test]
fn check_accepts_direct_returned_constructor_type_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Box"),
                    value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("Box"),
                arg_count: 0,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("Box")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_direct_returned_constructor_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->str"),
                    value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("Box"),
                arg_count: 0,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("Box")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `Box` where `build` expects `str`"));
}

#[test]
fn check_accepts_direct_returned_parameter_type_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(value:int)->int"),
                value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_direct_returned_parameter_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(value:str)->int"),
                value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `str` where `build` expects `int`"));
}

#[test]
fn check_accepts_direct_returned_member_type_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
                    method_kind: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(box:Box)->str"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("box")),
                value_member_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_direct_returned_member_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
                    method_kind: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(box:Box)->int"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("box")),
                value_member_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `str` where `build` expects `int`"));
}

#[test]
fn check_accepts_direct_returned_constructor_member_type_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
                    method_kind: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->str"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("Box")),
                value_member_name: Some(String::from("value")),
                value_member_through_instance: true,
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_direct_returned_constructor_member_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
                    method_kind: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("Box")),
                value_member_name: Some(String::from("value")),
                value_member_through_instance: true,
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 1,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `str` where `build` expects `int`"));
}

#[test]
fn check_reports_bool_annotated_assignment_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("flag"),
                kind: DeclarationKind::Value,
                detail: String::from("bool"),
                value_type: Some(String::from("int")),
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("flag"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("bool")),
                value_type: Some(String::from("int")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where `flag` expects `bool`"));
}

#[test]
fn check_reports_none_annotated_assignment_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("missing"),
                kind: DeclarationKind::Value,
                detail: String::from("None"),
                value_type: Some(String::from("int")),
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("missing"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("None")),
                value_type: Some(String::from("int")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where `missing` expects `None`"));
}

#[test]
fn check_accepts_direct_call_annotated_assignment_type_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::from("int"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("value"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("int")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("helper")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_direct_call_annotated_assignment_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->str"),
                    value_type: None,
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
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::from("int"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("value"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("int")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("helper")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `str` where `value` expects `int`"));
}

#[test]
fn check_accepts_direct_name_annotated_assignment_type_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("source"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
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
                    name: String::from("target"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("target"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("str")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("source")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_direct_member_annotated_assignment_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
                    method_kind: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("box"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Box"),
                    value_type: None,
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
                    name: String::from("target"),
                    kind: DeclarationKind::Value,
                    detail: String::from("int"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("target"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("int")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("box")),
                value_member_name: Some(String::from("value")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `str` where `target` expects `int`"));
}

#[test]
fn check_reports_local_annotated_assignment_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(value:str)->None"),
                value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("result"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("int")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("function `build` in module `src/app/module.py` assigns `str` where local `result` expects `int`"));
}

#[test]
fn check_accepts_return_from_local_bare_assignment() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 3,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("value"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("helper")),
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
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 2,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_local_name_augmented_assignment() {
    let result =
        check_temp_typepython_source("def build() -> None:\n    value: int = 1\n    value += 2\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_local_name_augmented_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "def build() -> None:\n    value: int = 1\n    value += \"bad\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("augmented-assigns `str` where local `value` expects `int`"));
}

#[test]
fn check_does_not_reuse_deleted_local_assignment_type() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("()->None"),
                value_type: None,
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
            invalidations: vec![typepython_binding::InvalidationSite {
                kind: typepython_binding::InvalidationKind::Delete,
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                names: vec![String::from("value")],
                line: 3,
            }],
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: vec![
                typepython_binding::AssignmentSite {
                    name: String::from("value"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::from("int")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 2,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                },
            ],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("assigns `int` where local `result` expects `str`"), "{rendered}");
}

#[test]
fn check_reports_local_annotated_assignment_from_bare_assignment_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->str"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->None"),
                    value_type: None,
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
            assignments: vec![
                typepython_binding::AssignmentSite {
                    name: String::from("value"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 2,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 3,
                },
            ],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("function `build` in module `src/app/module.py` assigns `str` where local `result` expects `int`"));
}

#[test]
fn check_accepts_module_level_bare_assignment_name_reference() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("result"),
                    kind: DeclarationKind::Value,
                    detail: String::from("int"),
                    value_type: None,
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
            assignments: vec![
                typepython_binding::AssignmentSite {
                    name: String::from("value"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 2,
                },
            ],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_module_level_name_augmented_assignment() {
    let result = check_temp_typepython_source("value: int = 1\nvalue += 2\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_module_level_name_augmented_assignment_type_mismatch() {
    let result = check_temp_typepython_source("value: int = 1\nvalue += \"bad\"\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("augmented-assigns `str` where `value` expects `int`"));
}

#[test]
fn check_does_not_reuse_deleted_module_assignment_type() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("result"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
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
            invalidations: vec![typepython_binding::InvalidationSite {
                kind: typepython_binding::InvalidationKind::Delete,
                owner_name: None,
                owner_type_name: None,
                names: vec![String::from("value")],
                line: 2,
            }],
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: vec![
                typepython_binding::AssignmentSite {
                    name: String::from("value"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::from("int")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 3,
                },
            ],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("assigns `int` where `result` expects `str`"), "{rendered}");
}

#[test]
fn check_reports_module_level_bare_assignment_name_reference_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->str"),
                    value_type: None,
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
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("result"),
                    kind: DeclarationKind::Value,
                    detail: String::from("int"),
                    value_type: None,
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
            assignments: vec![
                typepython_binding::AssignmentSite {
                    name: String::from("value"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 2,
                },
            ],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `str` where `result` expects `int`"));
}

#[test]
fn check_accepts_local_chained_bare_assignments_for_annotated_target() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->None"),
                    value_type: None,
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
            assignments: vec![
                typepython_binding::AssignmentSite {
                    name: String::from("x"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 1,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("y"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("x")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 2,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("y")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 3,
                },
            ],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_local_chained_bare_assignments_for_return() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("y")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 3,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: vec![
                typepython_binding::AssignmentSite {
                    name: String::from("x"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 1,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("y"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("x")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 2,
                },
            ],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_module_level_chained_bare_assignment_name_reference_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->str"),
                    value_type: None,
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
                    name: String::from("x"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("y"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("result"),
                    kind: DeclarationKind::Value,
                    detail: String::from("int"),
                    value_type: None,
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
            assignments: vec![
                typepython_binding::AssignmentSite {
                    name: String::from("x"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("y"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("x")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 2,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("y")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 3,
                },
            ],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `str` where `result` expects `int`"));
}

#[test]
fn check_accepts_builtin_return_types_in_assignments_and_returns() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("count"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
                    name: String::from("size"),
                    kind: DeclarationKind::Value,
                    detail: String::from("int"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("count"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("len")),
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("size"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("int")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("len")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

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
        name: String::from("maybe"),
        kind: DeclarationKind::Function,
        detail: String::from("(x:T | None)->T | None"),
        value_type: None,
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
        }],
    };
    let signature = vec![typepython_syntax::DirectFunctionParamSite {
        name: String::from("x"),
        annotation: Some(String::from("T | None")),
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    }];
    let call = typepython_binding::CallSite {
        callee: String::from("maybe"),
        arg_count: 1,
        arg_types: vec![String::from("int | None")],
        arg_values: Vec::new(),
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
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
        name: String::from("collect"),
        kind: DeclarationKind::Function,
        detail: String::from("(*args:Unpack[Ts])->tuple[Unpack[Ts]]"),
        value_type: None,
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
        }],
    };
    let signature = super::direct_signature_sites_from_detail(&function.detail);
    let call = typepython_binding::CallSite {
        callee: String::from("collect"),
        arg_count: 2,
        arg_types: vec![String::from("int"), String::from("str")],
        arg_values: Vec::new(),
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
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
        name: String::from("collect"),
        kind: DeclarationKind::Function,
        detail: String::from("(*args:Unpack[Ts])->tuple[Unpack[Ts]]"),
        value_type: None,
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
        }],
    };
    let call = typepython_binding::CallSite {
        callee: String::from("collect"),
        arg_count: 2,
        arg_types: vec![String::from("int"), String::from("str")],
        arg_values: Vec::new(),
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    };

    let instantiated_signature =
        super::resolve_instantiated_direct_function_signature(&node, &[], &function, &call)
            .expect("instantiated signature");
    let instantiated_return = super::resolve_instantiated_callable_return_type_from_declaration(
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
        name: String::from("collect"),
        kind: DeclarationKind::Function,
        detail: String::from("(value:tuple[Unpack[Ts]])->tuple[Unpack[Ts]]"),
        value_type: None,
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
        }],
    };
    let signature = super::direct_signature_sites_from_detail(&function.detail);
    let call = typepython_binding::CallSite {
        callee: String::from("collect"),
        arg_count: 1,
        arg_types: vec![String::from("tuple[int, str]")],
        arg_values: Vec::new(),
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
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
fn check_reports_unresolved_source_authored_typevartuple_call() {
    let result = check_temp_typepython_source(
        "def collect[*Ts](*args: *Ts) -> tuple[*Ts]:\n    return args\n\nitems: list[int] = [1, 2]\nvalue = collect(*items)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4014"), "{rendered}");
    assert!(rendered.contains("generic parameter list of `collect` could not be resolved"));
}

#[test]
fn check_accepts_source_authored_typevartuple_alias_roundtrip() {
    let result = check_temp_typepython_source(
        "typealias Pack[*Ts] = tuple[*Ts]\n\nvalue: Pack[int, str] = (1, \"x\")\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
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

#[test]
fn check_accepts_list_literal_assignment_type_match() {
    let result = check_temp_typepython_source("values: list[int] = [1, 2]\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_list_comprehension_assignment_type_match() {
    let result = check_temp_typepython_source("values: list[int] = [x + 1 for x in [1, 2]]\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_generator_comprehension_assignment_type_match() {
    let result = check_temp_typepython_source(
        "values: Generator[int, None, None] = (x + 1 for x in [1, 2])\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_set_comprehension_assignment_type_match() {
    let result = check_temp_typepython_source("values: set[int] = {x + 1 for x in [1, 2]}\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_dict_comprehension_assignment_type_match() {
    let result =
        check_temp_typepython_source("values: dict[int, int] = {x: x + 1 for x in [1, 2]}\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_generator_comprehension_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "values: Generator[str, None, None] = (x + 1 for x in [1, 2])\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains(
        "assigns `Generator[int, None, None]` where `values` expects `Generator[str, None, None]`"
    ));
}

#[test]
fn check_reports_set_comprehension_assignment_type_mismatch() {
    let result = check_temp_typepython_source("values: set[str] = {x + 1 for x in [1, 2]}\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `set[int]` where `values` expects `set[str]`"));
}

#[test]
fn check_reports_dict_comprehension_assignment_type_mismatch() {
    let result =
        check_temp_typepython_source("values: dict[str, int] = {x: x + 1 for x in [1, 2]}\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `dict[int, int]` where `values` expects `dict[str, int]`"));
}

#[test]
fn check_reports_list_comprehension_assignment_type_mismatch() {
    let result = check_temp_typepython_source("values: list[str] = [x + 1 for x in [1, 2]]\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `list[int]` where `values` expects `list[str]`"));
}

#[test]
fn check_does_not_leak_list_comprehension_target_name() {
    let result = check_temp_typepython_source(
        "x: str = \"outer\"\nvalues: list[str] = [x for x in [\"inner\"]]\nvalue: str = x\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_compare_annotated_assignment_type_mismatch() {
    let result =
        check_temp_typepython_source("left: int = 1\nright: int = 2\nvalue: int = left < right\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `bool` where `value` expects `int`"));
}

#[test]
fn check_reports_unary_not_return_type_mismatch() {
    let result =
        check_temp_typepython_source("def build(flag: bool) -> int:\n    return not flag\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `bool` where `build` expects `int`"));
}

#[test]
fn check_reports_compare_call_arg_type_mismatch() {
    let result = check_temp_typepython_source(
        "left: int = 1\nright: int = 2\n\ndef takes(value: int) -> None:\n    return None\n\ntakes(left < right)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("passes `bool` where parameter expects `int`"));
}

#[test]
fn check_reports_list_literal_assignment_type_mismatch() {
    let result = check_temp_typepython_source("values: list[str] = [1, 2]\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `list[int]` where `values` expects `list[str]`"));
}

#[test]
fn check_accepts_boolop_assignment_union_type() {
    let result = check_temp_typepython_source(
        "x: int | None = 1\ny: int = 2\nvalue: int | None = x and y\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_boolop_assignment_type_mismatch() {
    let result =
        check_temp_typepython_source("x: int | None = 1\ny: int = 2\nvalue: str = x and y\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where `value` expects `str`"));
}

#[test]
fn check_accepts_binop_numeric_assignment_type_match() {
    let result = check_temp_typepython_source("value: int = 1 + 2\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_binop_assignment_type_mismatch() {
    let result = check_temp_typepython_source("value: str = 1 + 2\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where `value` expects `str`"));
}

#[test]
fn check_reports_binop_call_arg_type_mismatch() {
    let result = check_temp_typepython_source(
        "def takes(value: str) -> None:\n    return None\n\ntakes(1 + 2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("passes `int` where parameter expects `str`"));
}

#[test]
fn check_accepts_ifexp_assignment_type_match() {
    let result = check_temp_typepython_source("flag: bool = True\nvalue: int = 1 if flag else 2\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_ifexp_assignment_type_mismatch() {
    let result = check_temp_typepython_source("flag: bool = True\nvalue: str = 1 if flag else 2\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where `value` expects `str`"));
}

#[test]
fn check_accepts_ifexp_assignment_with_none_narrowing() {
    let result = check_temp_typepython_source(
        "def maybe() -> int | None:\n    return None\n\nx: int | None = maybe()\ny: int = x if x is not None else 0\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_ifexp_call_arg_with_none_narrowing() {
    let result = check_temp_typepython_source(
        "def takes_int(value: int) -> None:\n    return None\n\ndef maybe() -> int | None:\n    return None\n\nx: int | None = maybe()\ntakes_int(x if x is not None else 0)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_ifexp_return_with_isinstance_narrowing() {
    let result = check_temp_typepython_source(
        "def build(value: int | str) -> int:\n    return 0 if isinstance(value, str) else value\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_ifexp_return_without_guard_narrowing_fallback() {
    let result = check_temp_typepython_source(
        "def build(value: str | None, flag: bool) -> str:\n    return value if flag else \"\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("function `build`"));
    assert!(rendered.contains("expects `str`"));
}

#[test]
fn check_accepts_namedexpr_assignment_type_match() {
    let result = check_temp_typepython_source("value: int = (tmp := 1)\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_contextual_lambda_callable_assignment() {
    let result = check_temp_typepython_source("handler: Callable[[int], str] = lambda x: \"ok\"\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_authored_lambda_parameter_annotations() {
    let result =
        check_temp_typepython_source("handler: Callable[[int], str] = lambda (x: int): str(x)\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_lambda_callable_assignment_mismatch() {
    let result = check_temp_typepython_source("handler: Callable[[int], str] = lambda x: 1\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_reports_authored_lambda_parameter_annotation_mismatch() {
    let result =
        check_temp_typepython_source("handler: Callable[[int], str] = lambda (x: str): x\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("(str)->str"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_accepts_contextual_lambda_callable_return() {
    let result = check_temp_typepython_source(
        "def make() -> Callable[[int], str]:\n    return lambda x: str(x)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_lambda_callable_return_mismatch() {
    let result = check_temp_typepython_source(
        "def make() -> Callable[[int], str]:\n    return lambda x: x + 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_accepts_contextual_typed_dict_literal_return() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef make_user() -> User:\n    return {\"id\": 1, \"name\": \"Ada\"}\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_typed_dict_literal_return_missing_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef make_user() -> User:\n    return {\"id\": 1}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_accepts_contextual_list_literal_return() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\ndef make() -> list[Callable[[int], str]]:\n    return [lambda x: str(x)]\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_set_literal_return_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\ndef make() -> set[Callable[[int], str]]:\n    return {lambda x: x + 1}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], int]"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_accepts_contextual_dict_literal_return_nested_typed_dict() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef make() -> dict[str, User]:\n    return {\"owner\": {\"id\": 1, \"name\": \"Ada\"}}\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_dict_literal_return_nested_typed_dict_missing_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef make() -> dict[str, User]:\n    return {\"owner\": {\"id\": 1}}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_accepts_contextual_empty_list_assignment() {
    let result = check_temp_typepython_source("values: list[int] = []\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_contextual_empty_dict_assignment() {
    let result = check_temp_typepython_source("values: dict[str, int] = {}\n");

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_contextual_list_assignment_with_nested_lambda() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\nhandlers: list[Callable[[int], str]] = [lambda x: str(x)]\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_dict_assignment_with_nested_typed_dict_missing_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\nowners: dict[str, User] = {\"owner\": {\"id\": 1}}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_accepts_contextual_typed_dict_literal_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\nuser: User = {\"id\": 1, \"name\": \"Ada\"}\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_typed_dict_literal_assignment_missing_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\nuser: User = {\"id\": 1}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_reports_contextual_typed_dict_literal_assignment_value_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\nuser: User = {\"id\": \"oops\", \"name\": \"Ada\"}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("assigns `str` to key `id`"));
}

#[test]
fn check_reports_contextual_typed_dict_literal_assignment_unknown_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\nuser: User = {\"id\": 1, \"name\": \"Ada\", \"extra\": 1}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("unknown key `extra`"));
}

#[test]
fn check_accepts_contextual_empty_list_call_argument() {
    let result = check_temp_typepython_source(
        "def takes(values: list[int]) -> None:\n    return None\n\ntakes([])\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_contextual_empty_dict_call_argument() {
    let result = check_temp_typepython_source(
        "def takes(values: dict[str, int]) -> None:\n    return None\n\ntakes({})\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_list_literal_call_argument_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\ndef takes(values: list[Callable[[int], str]]) -> None:\n    return None\n\ntakes([lambda x: x + 1])\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], int]"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_reports_contextual_set_literal_call_argument_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\ndef takes(values: set[Callable[[int], str]]) -> None:\n    return None\n\ntakes({lambda x: x + 1})\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], int]"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_accepts_contextual_dict_literal_call_argument_nested_typed_dict() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef takes(*, owners: dict[str, User]) -> None:\n    return None\n\ntakes(owners={\"owner\": {\"id\": 1, \"name\": \"Ada\"}})\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_dict_literal_call_argument_nested_typed_dict_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef takes(*, owners: dict[str, User]) -> None:\n    return None\n\ntakes(owners={\"owner\": {\"id\": 1}})\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_reports_contextual_lambda_callable_argument_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\ndef apply(fn: Callable[[int], str]) -> None:\n    return None\n\napply(lambda x: x + 1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], int]"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_reports_contextual_lambda_callable_argument_arity_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\ndef apply(fn: Callable[[int], str]) -> None:\n    return None\n\napply(lambda x, y: \"ok\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_reports_contextual_lambda_callable_keyword_argument_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\ndef apply(*, fn: Callable[[int], str]) -> None:\n    return None\n\napply(fn=lambda x: x + 1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], int]"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_reports_contextual_lambda_method_argument_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\nclass Runner:\n    def use(self, fn: Callable[[int], str]) -> None:\n        return None\n\nrunner = Runner()\nrunner.use(lambda x: x + 1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], int]"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_reports_contextual_lambda_argument_mismatch_after_generic_instantiation() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\ndef use[T](value: T, fn: Callable[[T], str]) -> None:\n    return None\n\nuse(1, lambda x: x + 1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], int]"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_reports_namedexpr_assignment_type_mismatch() {
    let result = check_temp_typepython_source("value: str = (tmp := 1)\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where `value` expects `str`"));
}

#[test]
fn check_accepts_namedexpr_binding_for_later_flow() {
    let result = check_temp_typepython_source(
        "def build() -> int:\n    if (tmp := 1):\n        return tmp\n    return tmp\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_fixed_tuple_destructuring_for_later_flow() {
    let result = check_temp_typepython_source(
        "pair: tuple[int, str] = (1, \"x\")\nleft, right = pair\nvalue: str = right\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_fixed_tuple_destructuring_arity_mismatch() {
    let result = check_temp_typepython_source("pair: tuple[int] = (1,)\nleft, right = pair\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("destructures assignment target `(left, right)` with 2 name(s) from tuple type `tuple[int]` with 1 element(s)"));
}

#[test]
fn check_reports_namedexpr_binding_mismatch_for_later_assignment() {
    let result = check_temp_typepython_source(
        "def build() -> None:\n    if (tmp := 1):\n        pass\n    value: str = tmp\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where local `value` expects `str`"));
}

#[test]
fn check_accepts_starred_tuple_call_expansion() {
    let result = check_temp_typepython_source(
        "def takes(x: int, y: int) -> None:\n    return None\n\nxs: tuple[int, int] = (1, 2)\ntakes(*xs)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_starred_tuple_call_type_mismatch() {
    let result = check_temp_typepython_source(
        "def takes(x: int) -> None:\n    return None\n\nxs: tuple[str] = (\"oops\",)\ntakes(*xs)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("passes `str` where parameter expects `int`"));
}

#[test]
fn check_accepts_dict_keyword_expansion_into_kwargs() {
    let result = check_temp_typepython_source(
        "def build(**kwargs: int) -> None:\n    return None\n\nvalues: dict[str, int] = {\"x\": 1}\nbuild(**values)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_dict_keyword_expansion_without_kwargs() {
    let result = check_temp_typepython_source(
        "def build(x: int) -> None:\n    return None\n\nvalues: dict[str, int] = {\"x\": 1}\nbuild(**values)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("cannot expand `**dict[str, int]` without `**kwargs`"));
}

#[test]
fn check_accepts_closed_typed_dict_keyword_expansion_callsite() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass UserKw(TypedDict, closed=True):\n    name: str\n\ndef build(*, name: str) -> None:\n    return None\n\ndef payload() -> UserKw:\n    return {\"name\": \"Ada\"}\n\nbuild(**payload())\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_open_typed_dict_keyword_expansion_callsite() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass UserKw(TypedDict):\n    name: str\n\ndef build(*, name: str) -> None:\n    return None\n\ndef payload() -> UserKw:\n    return {\"name\": \"Ada\"}\n\nbuild(**payload())\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("cannot expand open TypedDict `UserKw`"));
}

#[test]
fn check_accepts_typed_dict_unpack_extra_items_callsite() {
    let result = check_temp_typepython_source(
        "class UserKw(TypedDict, extra_items=int):\n    name: str\n\ndef build(**kwargs: Unpack[UserKw]) -> None:\n    return None\n\nbuild(name=\"Ada\", age=1)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_typed_dict_unpack_extra_items_callsite_type_mismatch() {
    let result = check_temp_typepython_source(
        "class UserKw(TypedDict, extra_items=int):\n    name: str\n\ndef build(**kwargs: Unpack[UserKw]) -> None:\n    return None\n\nbuild(name=\"Ada\", age=\"old\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("variadic keyword parameter expects `int`"));
}

#[test]
fn check_reports_direct_generic_function_call_return_mismatch() {
    let result = check_temp_typepython_source(
        "def first[T](value: T) -> T:\n    return value\n\nresult: str = first(1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where `result` expects `str`"));
}

#[test]
fn check_accepts_unpack_typeddict_keyword_calls() {
    let result = check_temp_typepython_source(
        "class UserKw(TypedDict):\n    name: str\n    age: NotRequired[int]\n\ndef build(**kwargs: Unpack[UserKw]) -> None:\n    return None\n\nbuild(name=\"Ada\")\nbuild(name=\"Ada\", age=1)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_unpack_typeddict_missing_required_keyword() {
    let result = check_temp_typepython_source(
        "class UserKw(TypedDict):\n    name: str\n    age: NotRequired[int]\n\ndef build(**kwargs: Unpack[UserKw]) -> None:\n    return None\n\nbuild(age=1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("missing required argument(s): name"));
}

#[test]
fn check_reports_unpack_typeddict_unknown_keyword() {
    let result = check_temp_typepython_source(
        "class UserKw(TypedDict):\n    name: str\n\ndef build(**kwargs: Unpack[UserKw]) -> None:\n    return None\n\nbuild(name=\"Ada\", extra=1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("unknown unpacked keyword `extra`"));
}

#[test]
fn check_reports_unpack_typeddict_keyword_type_mismatch() {
    let result = check_temp_typepython_source(
        "class UserKw(TypedDict):\n    name: str\n\ndef build(**kwargs: Unpack[UserKw]) -> None:\n    return None\n\nbuild(name=1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("passes `int` for unpacked keyword `name`"));
    assert!(rendered.contains("expects `str`"));
}

#[test]
fn check_reports_eval_outside_unsafe_block_when_strict() {
    let result = check_temp_typepython_source_with_check_options(
        "def run() -> None:\n    eval(\"1 + 1\")\n",
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4019"));
    assert!(rendered.contains("eval(...)"));
}

#[test]
fn check_accepts_eval_inside_unsafe_block_when_strict() {
    let result = check_temp_typepython_source_with_check_options(
        "def run() -> None:\n    unsafe:\n        eval(\"1 + 1\")\n",
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        true,
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_builtin_return_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("name"),
                kind: DeclarationKind::Value,
                detail: String::from("str"),
                value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("name"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("str")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("len")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where `name` expects `str`"));
}

#[test]
fn check_accepts_generic_alias_normalization() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("make_items"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->List[int]"),
                    value_type: None,
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
                    name: String::from("items"),
                    kind: DeclarationKind::Value,
                    detail: String::from("list[int]"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("items"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("list[int]")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("make_items")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_callable_assignment_compatibility() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("handler"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Callable[[int], str]"),
                    value_type: None,
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
                    name: String::from("my_func"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(x:int)->str"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("handler"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("Callable[[int], str]")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("my_func")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_annotated_type_equivalence() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(value:Annotated[str, tag])->str"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_callable_assignment_compatibility_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("handler"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Callable[[int], str]"),
                    value_type: None,
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
                    name: String::from("my_func"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(x:str)->str"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("handler"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("Callable[[int], str]")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("my_func")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(
        rendered.contains(
            "assigns callable `(str)->str` where `handler` expects `Callable[[int], str]`"
        )
    );
}

#[test]
fn check_accepts_callable_ellipsis_assignment_compatibility() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("handler"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Callable[..., str]"),
                    value_type: None,
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
                    name: String::from("my_func"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(x:str,y:int)->str"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("handler"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("Callable[..., str]")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("my_func")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_callable_ellipsis_return_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("handler"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Callable[..., int]"),
                    value_type: None,
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
                    name: String::from("my_func"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(x:str,y:int)->str"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("handler"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("Callable[..., int]")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("my_func")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains(
        "assigns callable `(str,int)->str` where `handler` expects `Callable[..., int]`"
    ));
}

#[test]
fn check_accepts_callable_assignment_from_bound_method() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("get"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:int)->str"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("box"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Box"),
                    value_type: None,
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
                    name: String::from("handler"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Callable[[int], str]"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("handler"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("Callable[[int], str]")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("box")),
                value_member_name: Some(String::from("get")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_callable_assignment_from_bound_method_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("get"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:str)->str"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("box"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Box"),
                    value_type: None,
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
                    name: String::from("handler"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Callable[[int], str]"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("handler"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("Callable[[int], str]")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("box")),
                value_member_name: Some(String::from("get")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(
        rendered.contains(
            "assigns callable `(str)->str` where `handler` expects `Callable[[int], str]`"
        )
    );
}

#[test]
fn check_accepts_callable_assignment_from_bound_method_through_instance() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("get"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:int)->str"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("make_box"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Box"),
                    value_type: None,
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
                    name: String::from("handler"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Callable[[int], str]"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("handler"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("Callable[[int], str]")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("make_box")),
                value_member_name: Some(String::from("get")),
                value_member_through_instance: true,
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_callable_assignment_from_bound_method_through_instance_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("get"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:str)->str"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("make_box"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Box"),
                    value_type: None,
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
                    name: String::from("handler"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Callable[[int], str]"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("handler"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("Callable[[int], str]")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("make_box")),
                value_member_name: Some(String::from("get")),
                value_member_through_instance: true,
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(
        rendered.contains(
            "assigns callable `(str)->str` where `handler` expects `Callable[[int], str]`"
        )
    );
}

#[test]
fn check_accepts_builtin_container_generic_any_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("items"),
                kind: DeclarationKind::Value,
                detail: String::from("list[str]"),
                value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("items"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("list[str]")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("list")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_any_optional_and_union_direct_matches() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("anything"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Any"),
                    value_type: None,
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
                    name: String::from("maybe"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Optional[int]"),
                    value_type: None,
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
                    name: String::from("choice"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Union[int, str]"),
                    value_type: None,
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
                    name: String::from("measure"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("measure"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("len")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 4,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: vec![
                typepython_binding::AssignmentSite {
                    name: String::from("anything"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("Any")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("len")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("maybe"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("Optional[int]")),
                    value_type: Some(String::from("None")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 2,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("choice"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("Union[int, str]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("len")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 3,
                },
            ],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_optional_direct_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("name"),
                kind: DeclarationKind::Value,
                detail: String::from("Optional[str]"),
                value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("name"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("Optional[str]")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("len")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where `name` expects `Optional[str]`"));
}

#[test]
fn check_accepts_cast_builtin_return_type() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("text"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Any"),
                    value_type: None,
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
                    name: String::from("cast"),
                    kind: DeclarationKind::Import,
                    detail: String::from("typing.cast"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("cast")),
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("text"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("str")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("cast")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_typing_typevar_assignment() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("TypeVar"),
                    kind: DeclarationKind::Import,
                    detail: String::from("typing.TypeVar"),
                    value_type: None,
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
                    name: String::from("T"),
                    kind: DeclarationKind::Value,
                    detail: String::from("TypeVar"),
                    value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("TypeVar"),
                arg_count: 1,
                arg_types: vec![String::from("str")],
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("T"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("TypeVar")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("TypeVar")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_typing_typevar_argument_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("TypeVar"),
                kind: DeclarationKind::Import,
                detail: String::from("typing.TypeVar"),
                value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("TypeVar"),
                arg_count: 1,
                arg_types: vec![String::from("int")],
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains(
        "call to `TypeVar` in module `src/app/module.py` passes `int` where parameter expects `str`"
    ));
}

#[test]
fn check_accepts_typing_extensions_typevar_assignment() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("TypeVar"),
                        kind: DeclarationKind::Import,
                        detail: String::from("typing_extensions.TypeVar"),
                        value_type: None,
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
                        name: String::from("T"),
                        kind: DeclarationKind::Value,
                        detail: String::from("TypeVar"),
                        value_type: None,
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
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("TypeVar"),
                    arg_count: 1,
                    arg_types: vec![String::from("str")],
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }],
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("T"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("TypeVar")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("TypeVar")),
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
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            },
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<typing-extensions-prelude>"),
                module_key: String::from("typing_extensions"),
                module_kind: SourceKind::Stub,
                declarations: vec![typepython_binding::Declaration {
                    name: String::from("TypeVar"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(name:str)->TypeVar"),
                    value_type: None,
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
        ],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_typing_extensions_protocol_missing_member() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Protocol"),
                        kind: DeclarationKind::Import,
                        detail: String::from("typing_extensions.Protocol"),
                        value_type: None,
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
                        name: String::from("Reader"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Protocol"),
                        value_type: None,
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
                        bases: vec![String::from("Protocol")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("read"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Reader"),
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
                    Declaration {
                        name: String::from("BadReader"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Reader"),
                        value_type: None,
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
                        bases: vec![String::from("Reader")],
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
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<typing-extensions-prelude>"),
                module_key: String::from("typing_extensions"),
                module_kind: SourceKind::Stub,
                declarations: vec![typepython_binding::Declaration {
                    name: String::from("Protocol"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Interface),
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
        ],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4008"));
    assert!(rendered.contains("does not implement interface member `read` from `Reader`"));
}

#[test]
fn check_accepts_collections_abc_async_iterator_base() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("AsyncIterator"),
                        kind: DeclarationKind::Import,
                        detail: String::from("collections.abc.AsyncIterator"),
                        value_type: None,
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
                        name: String::from("Stream"),
                        kind: DeclarationKind::Class,
                        detail: String::from("AsyncIterator"),
                        value_type: None,
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
                        bases: vec![String::from("AsyncIterator")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__aiter__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->AsyncIterator[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Stream"),
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
                    Declaration {
                        name: String::from("__anext__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Awaitable[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Stream"),
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
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                module_key: String::from("collections.abc"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    typepython_binding::Declaration {
                        name: String::from("AsyncIterable"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
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
                    typepython_binding::Declaration {
                        name: String::from("__aiter__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->AsyncIterator[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("AsyncIterable"),
                            kind: DeclarationOwnerKind::Interface,
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
                    typepython_binding::Declaration {
                        name: String::from("AsyncIterator"),
                        kind: DeclarationKind::Class,
                        detail: String::from("AsyncIterable"),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("AsyncIterable")],
                        type_params: Vec::new(),
                    },
                    typepython_binding::Declaration {
                        name: String::from("__anext__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Awaitable[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("AsyncIterator"),
                            kind: DeclarationOwnerKind::Interface,
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
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_collections_abc_async_iterator_missing_member() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("AsyncIterator"),
                        kind: DeclarationKind::Import,
                        detail: String::from("collections.abc.AsyncIterator"),
                        value_type: None,
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
                        name: String::from("BadStream"),
                        kind: DeclarationKind::Class,
                        detail: String::from("AsyncIterator"),
                        value_type: None,
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
                        bases: vec![String::from("AsyncIterator")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__aiter__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->AsyncIterator[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("BadStream"),
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
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                module_key: String::from("collections.abc"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    typepython_binding::Declaration {
                        name: String::from("AsyncIterable"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
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
                    typepython_binding::Declaration {
                        name: String::from("__aiter__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->AsyncIterator[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("AsyncIterable"),
                            kind: DeclarationOwnerKind::Interface,
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
                    typepython_binding::Declaration {
                        name: String::from("AsyncIterator"),
                        kind: DeclarationKind::Class,
                        detail: String::from("AsyncIterable"),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("AsyncIterable")],
                        type_params: Vec::new(),
                    },
                    typepython_binding::Declaration {
                        name: String::from("__anext__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Awaitable[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("AsyncIterator"),
                            kind: DeclarationOwnerKind::Interface,
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
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4008"));
    assert!(
        rendered.contains("does not implement interface member `__anext__` from `AsyncIterator`")
    );
}

#[test]
fn check_accepts_newtype_assignment() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("NewType"),
                    kind: DeclarationKind::Import,
                    detail: String::from("typing.NewType"),
                    value_type: None,
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
                    name: String::from("UserId"),
                    kind: DeclarationKind::Value,
                    detail: String::from("NewType"),
                    value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("NewType"),
                arg_count: 2,
                arg_types: vec![String::from("str"), String::new()],
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("UserId"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("NewType")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("NewType")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_newtype_argument_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("NewType"),
                kind: DeclarationKind::Import,
                detail: String::from("typing.NewType"),
                value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("NewType"),
                arg_count: 2,
                arg_types: vec![String::from("int"), String::new()],
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains(
        "call to `NewType` in module `src/app/module.py` passes `int` where parameter expects `str`"
    ));
}

#[test]
fn check_accepts_protocol_derived_base_implementation() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Protocol"),
                        kind: DeclarationKind::Import,
                        detail: String::from("typing.Protocol"),
                        value_type: None,
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
                        name: String::from("Reader"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Protocol"),
                        value_type: None,
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
                        bases: vec![String::from("Protocol")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("read"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Reader"),
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
                    Declaration {
                        name: String::from("FileReader"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Reader"),
                        value_type: None,
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
                        bases: vec![String::from("Reader")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("read"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("FileReader"),
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
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<typing-prelude>"),
                module_key: String::from("typing"),
                module_kind: SourceKind::Stub,
                declarations: vec![typepython_binding::Declaration {
                    name: String::from("Protocol"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Interface),
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
        ],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_protocol_derived_base_missing_member() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Protocol"),
                        kind: DeclarationKind::Import,
                        detail: String::from("typing.Protocol"),
                        value_type: None,
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
                        name: String::from("Reader"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Protocol"),
                        value_type: None,
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
                        bases: vec![String::from("Protocol")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("read"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Reader"),
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
                    Declaration {
                        name: String::from("BadReader"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Reader"),
                        value_type: None,
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
                        bases: vec![String::from("Reader")],
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
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<typing-prelude>"),
                module_key: String::from("typing"),
                module_kind: SourceKind::Stub,
                declarations: vec![typepython_binding::Declaration {
                    name: String::from("Protocol"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Interface),
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
        ],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4008"));
    assert!(rendered.contains("does not implement interface member `read` from `Reader`"));
}

#[test]
fn check_accepts_collections_abc_sized_base() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Sized"),
                        kind: DeclarationKind::Import,
                        detail: String::from("collections.abc.Sized"),
                        value_type: None,
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
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Sized"),
                        value_type: None,
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
                        bases: vec![String::from("Sized")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__len__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->int"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
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
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                module_key: String::from("collections.abc"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    typepython_binding::Declaration {
                        name: String::from("Sized"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
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
                    typepython_binding::Declaration {
                        name: String::from("__len__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->int"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Sized"),
                            kind: DeclarationOwnerKind::Interface,
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
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_collections_abc_sized_missing_member() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Sized"),
                        kind: DeclarationKind::Import,
                        detail: String::from("collections.abc.Sized"),
                        value_type: None,
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
                        name: String::from("BadBox"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Sized"),
                        value_type: None,
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
                        bases: vec![String::from("Sized")],
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
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                module_key: String::from("collections.abc"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    typepython_binding::Declaration {
                        name: String::from("Sized"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
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
                    typepython_binding::Declaration {
                        name: String::from("__len__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->int"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Sized"),
                            kind: DeclarationOwnerKind::Interface,
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
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4008"));
    assert!(rendered.contains("does not implement interface member `__len__` from `Sized`"));
}

#[test]
fn check_accepts_collections_abc_callable_base() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Callable"),
                        kind: DeclarationKind::Import,
                        detail: String::from("collections.abc.Callable"),
                        value_type: None,
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
                        name: String::from("Runner"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Callable"),
                        value_type: None,
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
                        bases: vec![String::from("Callable")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__call__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Any"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Runner"),
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
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                module_key: String::from("collections.abc"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    typepython_binding::Declaration {
                        name: String::from("Callable"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
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
                    typepython_binding::Declaration {
                        name: String::from("__call__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Any"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Callable"),
                            kind: DeclarationOwnerKind::Interface,
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
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_collections_abc_iterator_missing_member() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Iterator"),
                        kind: DeclarationKind::Import,
                        detail: String::from("collections.abc.Iterator"),
                        value_type: None,
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
                        name: String::from("Cursor"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Iterator"),
                        value_type: None,
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
                        bases: vec![String::from("Iterator")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__iter__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Iterator[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Cursor"),
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
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                module_key: String::from("collections.abc"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    typepython_binding::Declaration {
                        name: String::from("Sized"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
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
                    typepython_binding::Declaration {
                        name: String::from("__len__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->int"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Sized"),
                            kind: DeclarationOwnerKind::Interface,
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
                    typepython_binding::Declaration {
                        name: String::from("Iterable"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Sized"),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Sized")],
                        type_params: Vec::new(),
                    },
                    typepython_binding::Declaration {
                        name: String::from("__iter__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Iterator[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Iterable"),
                            kind: DeclarationOwnerKind::Interface,
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
                    typepython_binding::Declaration {
                        name: String::from("Iterator"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Iterable"),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Iterable")],
                        type_params: Vec::new(),
                    },
                    typepython_binding::Declaration {
                        name: String::from("__next__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Any"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Iterator"),
                            kind: DeclarationOwnerKind::Interface,
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
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4008"));
    assert!(rendered.contains("does not implement interface member `__next__` from `Iterator`"));
}

#[test]
fn check_accepts_typing_awaitable_base() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Awaitable"),
                        kind: DeclarationKind::Import,
                        detail: String::from("typing.Awaitable"),
                        value_type: None,
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
                        name: String::from("Job"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Awaitable"),
                        value_type: None,
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
                        bases: vec![String::from("Awaitable")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__await__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Iterator[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Job"),
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
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<typing-prelude>"),
                module_key: String::from("typing"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    typepython_binding::Declaration {
                        name: String::from("Awaitable"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
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
                    typepython_binding::Declaration {
                        name: String::from("__await__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Iterator[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Awaitable"),
                            kind: DeclarationOwnerKind::Interface,
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
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_typing_awaitable_missing_member() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Awaitable"),
                        kind: DeclarationKind::Import,
                        detail: String::from("typing.Awaitable"),
                        value_type: None,
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
                        name: String::from("BadJob"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Awaitable"),
                        value_type: None,
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
                        bases: vec![String::from("Awaitable")],
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
            typepython_graph::ModuleNode {
                module_path: std::path::PathBuf::from("<typing-prelude>"),
                module_key: String::from("typing"),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    typepython_binding::Declaration {
                        name: String::from("Awaitable"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
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
                    typepython_binding::Declaration {
                        name: String::from("__await__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Iterator[Any]"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Awaitable"),
                            kind: DeclarationOwnerKind::Interface,
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
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4008"));
    assert!(rendered.contains("does not implement interface member `__await__` from `Awaitable`"));
}

#[test]
fn check_accepts_async_function_call_as_awaitable() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("fetch"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: true,
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
                    name: String::from("task"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Awaitable[int]"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("task"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("Awaitable[int]")),
                value_type: Some(String::new()),
                is_awaited: false,
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_async_function_call_non_awaitable_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("fetch"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: true,
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
                    name: String::from("result"),
                    kind: DeclarationKind::Value,
                    detail: String::from("int"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("result"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("int")),
                value_type: Some(String::new()),
                is_awaited: false,
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `Awaitable[int]` where `result` expects `int`"));
}

#[test]
fn check_accepts_direct_await_of_async_function() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("fetch"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: true,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: true,
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
            returns: vec![
                typepython_binding::ReturnSite {
                    owner_name: String::from("fetch"),
                    owner_type_name: None,
                    value_type: Some(String::from("int")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                },
                typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 5,
                },
            ],
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_tpy_direct_await_of_async_function() {
    let result = check_temp_typepython_source(
        "async def fetch() -> int:\n    return 1\n\nasync def build() -> int:\n    return await fetch()\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_direct_await_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("fetch"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: true,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->str"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: true,
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
            returns: vec![
                typepython_binding::ReturnSite {
                    owner_name: String::from("fetch"),
                    owner_type_name: None,
                    value_type: Some(String::from("int")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                },
                typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 5,
                },
            ],
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains(
        "function `build` in module `src/app/module.py` returns `int` where `build` expects `str`"
    ));
}

#[test]
fn check_accepts_generator_yield_type() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("produce"),
                kind: DeclarationKind::Function,
                detail: String::from("()->Generator[int, None, None]"),
                value_type: None,
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
            yields: vec![typepython_binding::YieldSite {
                owner_name: String::from("produce"),
                owner_type_name: None,
                value_type: Some(String::from("int")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                is_yield_from: false,
                line: 2,
            }],
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_tpy_yield_and_yield_from() {
    let result = check_temp_typepython_source(
        "from typing import Generator\n\ndef produce() -> Generator[int, None, None]:\n    yield 1\n\ndef relay(values: list[int]) -> Generator[int, None, None]:\n    yield from values\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_generator_yield_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("produce"),
                kind: DeclarationKind::Function,
                detail: String::from("()->Generator[int, None, None]"),
                value_type: None,
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
            yields: vec![typepython_binding::YieldSite {
                owner_name: String::from("produce"),
                owner_type_name: None,
                value_type: Some(String::from("str")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                is_yield_from: false,
                line: 2,
            }],
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("function `produce` in module `src/app/module.py` yields `str` where `Generator[int, ...]` expects `int`"));
}

#[test]
fn check_accepts_contextual_list_generator_yield() {
    let result = check_temp_typepython_source(
        "from typing import Callable, Generator\n\ndef produce() -> Generator[list[Callable[[int], str]], None, None]:\n    yield [lambda x: str(x)]\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_set_generator_yield_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import Callable, Generator\n\ndef produce() -> Generator[set[Callable[[int], str]], None, None]:\n    yield {lambda x: x + 1}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], int]"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_accepts_contextual_dict_generator_yield_nested_typed_dict() {
    let result = check_temp_typepython_source(
        "from typing import Generator, TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef produce() -> Generator[dict[str, User], None, None]:\n    yield {\"owner\": {\"id\": 1, \"name\": \"Ada\"}}\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_dict_generator_yield_nested_typed_dict_missing_key() {
    let result = check_temp_typepython_source(
        "from typing import Generator, TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef produce() -> Generator[dict[str, User], None, None]:\n    yield {\"owner\": {\"id\": 1}}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_accepts_contextual_lambda_generator_yield() {
    let result = check_temp_typepython_source(
        "def produce() -> Generator[Callable[[int], str], None, None]:\n    yield lambda x: str(x)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_lambda_generator_yield_mismatch() {
    let result = check_temp_typepython_source(
        "def produce() -> Generator[Callable[[int], str], None, None]:\n    yield lambda x: x + 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_accepts_contextual_typed_dict_generator_yield() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef produce() -> Generator[User, None, None]:\n    yield {\"id\": 1, \"name\": \"Ada\"}\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_contextual_typed_dict_generator_yield_missing_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\ndef produce() -> Generator[User, None, None]:\n    yield {\"id\": 1}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_accepts_yield_from_iterable_type() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("values"),
                    kind: DeclarationKind::Value,
                    detail: String::from("list[int]"),
                    value_type: None,
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
                    name: String::from("relay"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Generator[int, None, None]"),
                    value_type: None,
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
            yields: vec![typepython_binding::YieldSite {
                owner_name: String::from("relay"),
                owner_type_name: None,
                value_type: Some(String::new()),
                value_callee: None,
                value_name: Some(String::from("values")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                is_yield_from: true,
                line: 2,
            }],
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_unknown_direct_call_keyword() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(x:int,y:int)->None"),
                value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("build"),
                arg_count: 0,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: vec![String::from("z")],
                keyword_arg_types: vec![String::from("int")],
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("unknown keyword `z`"));
}

#[test]
fn check_reports_unknown_member_access() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("value"),
                kind: DeclarationKind::Value,
                detail: String::from("unknown"),
                value_type: None,
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
            member_accesses: vec![typepython_binding::MemberAccessSite {
                current_owner_name: None,
                current_owner_type_name: None,
                owner_name: String::from("value"),
                member: String::from("name"),
                through_instance: false,
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"));
    assert!(rendered.contains("unsupported because `value` has type `unknown`"));
}

#[test]
fn check_reports_unknown_method_call() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("value"),
                kind: DeclarationKind::Value,
                detail: String::from("unknown"),
                value_type: None,
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
            method_calls: vec![typepython_binding::MethodCallSite {
                owner_name: String::from("value"),
                method: String::from("run"),
                through_instance: false,
                arg_count: 0,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"));
    assert!(rendered.contains("method call `value.run`"));
}

#[test]
fn check_reports_unknown_direct_call_on_import() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.tpy"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::TypePython,
            declarations: vec![Declaration {
                name: String::from("external"),
                kind: DeclarationKind::Import,
                detail: String::from("pkg.external"),
                value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("external"),
                arg_count: 0,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"));
    assert!(rendered.contains("call to `external`"));
}

#[test]
fn check_reports_unknown_dotted_call_on_unresolved_import_when_imports_unknown() {
    let result = check_with_options(
        &ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.tpy"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::TypePython,
                declarations: vec![Declaration {
                    name: String::from("external"),
                    kind: DeclarationKind::Import,
                    detail: String::from("pkg.external"),
                    value_type: None,
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
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("external.run"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }],
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
            }],
        },
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"));
    assert!(rendered.contains("call to `external.run`"));
}

#[test]
fn check_allows_dotted_call_on_unresolved_import_when_imports_dynamic() {
    let result = check_with_options(
        &ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.tpy"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::TypePython,
                declarations: vec![Declaration {
                    name: String::from("external"),
                    kind: DeclarationKind::Import,
                    detail: String::from("pkg.external"),
                    value_type: None,
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
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("external.run"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }],
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
            }],
        },
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Dynamic,
    );

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_unresolved_external_module_method_call_from_real_parse_pipeline() {
    let result = check_temp_typepython_source_with_check_options(
        concat!(
            "import definitely_missing_pkg\n",
            "\n",
            "def run() -> None:\n",
            "    definitely_missing_pkg.work()\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"));
    assert!(rendered.contains("definitely_missing_pkg"));
}

#[test]
fn check_reports_missing_direct_member_access() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("Box"),
                kind: DeclarationKind::Class,
                detail: String::new(),
                value_type: None,
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
            }],
            calls: Vec::new(),
            method_calls: Vec::new(),
            returns: Vec::new(),
            member_accesses: vec![typepython_binding::MemberAccessSite {
                current_owner_name: None,
                current_owner_type_name: None,
                owner_name: String::from("Box"),
                member: String::from("missing"),
                through_instance: false,
                line: 1,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4002"));
    assert!(rendered.contains("has no member `missing`"));
}

#[test]
fn check_reports_union_member_access_with_isinstance_guard_suggestion() {
    let root = create_temp_typepython_root();
    let path = root.join("src/app/module.tpy");
    fs::create_dir_all(path.parent().expect("temp source parent should exist"))
        .expect("temp source parent should be created");
    fs::write(&path, concat!("value = None\n", "value.name\n",))
        .expect("temp source should be written");

    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: path.clone(),
            module_key: String::from("app.module"),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("A"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("name"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("str")),
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
                Declaration {
                    name: String::from("B"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::from("A | B"),
                    value_type: None,
                    method_kind: None,
                    owner: None,
                    class_kind: None,
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
    });
    let _ = fs::remove_dir_all(&root);

    let diagnostic = result
        .diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "TPY4002")
        .expect("union member access diagnostic should be present");
    assert_eq!(diagnostic.suggestions.len(), 1);
    assert!(diagnostic.suggestions[0].replacement.contains("assert isinstance(value, A)"));
}

#[test]
fn check_reports_direct_method_call_arity_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:int,y:int)->None"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
            ],
            calls: Vec::new(),
            method_calls: vec![typepython_binding::MethodCallSite {
                owner_name: String::from("Box"),
                method: String::from("run"),
                through_instance: false,
                arg_count: 1,
                arg_types: vec![String::from("int")],
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("call to `Box.run`"));
}

#[test]
fn check_reports_direct_constructor_arity_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("__init__"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:int,y:int)->None"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
            ],
            calls: vec![typepython_binding::CallSite {
                callee: String::from("Box"),
                arg_count: 1,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("call to `Box`"));
}

#[test]
fn check_reports_direct_constructor_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("__init__"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:int,y:str)->None"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
            ],
            calls: vec![typepython_binding::CallSite {
                callee: String::from("Box"),
                arg_count: 2,
                arg_types: vec![String::from("str"), String::from("int")],
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("call to `Box`"));
    assert!(rendered.contains("parameter expects `int`"));
}

#[test]
fn check_reports_invalid_top_level_override_usage() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("top_level"),
                kind: DeclarationKind::Function,
                detail: String::new(),
                value_type: None,
                method_kind: None,
                class_kind: None,
                owner: None,
                is_async: false,
                is_override: true,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: false,
                bases: Vec::new(),
                type_params: Vec::new(),
            }],
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4005"));
    assert!(rendered.contains("top_level"));
}

#[test]
fn check_reports_member_override_without_base_member() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("Child"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Base")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Child"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_async: false,
                    is_override: true,
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4005"));
    assert!(rendered.contains("no direct base member was found"));
}

#[test]
fn check_reports_incompatible_direct_override_signature() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:int)->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Base"),
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
                Declaration {
                    name: String::from("Child"),
                    kind: DeclarationKind::Class,
                    detail: String::from("Base"),
                    value_type: None,
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
                    bases: vec![String::from("Base")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:str)->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Child"),
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4005"));
    assert!(rendered.contains("incompatible signature or annotation"));
}

#[test]
fn check_accepts_variance_compatible_override_signature() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:Child)->Base"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Base"),
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
                Declaration {
                    name: String::from("Child"),
                    kind: DeclarationKind::Class,
                    detail: String::from("Base"),
                    value_type: None,
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
                    bases: vec![String::from("Base")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:Base)->Child"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Child"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_async: false,
                    is_override: true,
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
        }],
    });

    assert!(result.diagnostics.is_empty(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_rejects_list_covariance_assignment() {
    let node = type_relation_node_with_base_child();

    assert!(!crate::direct_type_is_assignable(&node, &[], "list[Base]", "list[Child]"));
}

#[test]
fn check_rejects_list_union_widening_assignment() {
    let node = type_relation_node_with_base_child();

    assert!(!crate::direct_type_is_assignable(&node, &[], "list[int | str]", "list[int]"));
}

#[test]
fn check_rejects_dict_covariance_assignment() {
    let node = type_relation_node_with_base_child();

    assert!(!crate::direct_type_is_assignable(&node, &[], "dict[str, Base]", "dict[str, Child]",));
}

#[test]
fn check_rejects_user_generic_covariance_assignment() {
    let node = type_relation_node_with_base_child();

    assert!(!crate::direct_type_is_assignable(&node, &[], "Box[Base]", "Box[Child]"));
}

#[test]
fn check_accepts_sequence_covariance_assignment() {
    let node = type_relation_node_with_base_child();

    assert!(crate::direct_type_is_assignable(&node, &[], "Sequence[Base]", "Sequence[Child]",));
}

#[test]
fn check_accepts_mapping_value_covariance_assignment() {
    let node = type_relation_node_with_base_child();

    assert!(crate::direct_type_is_assignable(
        &node,
        &[],
        "Mapping[str, Base]",
        "Mapping[str, Child]",
    ));
}

#[test]
fn check_accepts_typevartuple_alias_expansion_assignment() {
    let node = ModuleNode {
        module_path: PathBuf::from("src/app/module.tpy"),
        module_key: String::from("app.module"),
        module_kind: SourceKind::TypePython,
        declarations: vec![Declaration {
            name: String::from("Pack"),
            kind: DeclarationKind::TypeAlias,
            detail: String::from("tuple[Unpack[Ts]]"),
            value_type: None,
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
            }],
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
    let graph = vec![node.clone()];

    assert!(super::direct_type_is_assignable(&node, &graph, "tuple[int, str]", "Pack[int, str]"));
    assert!(super::direct_type_is_assignable(&node, &graph, "Pack[int, str]", "tuple[int, str]"));
}

#[test]
fn check_accepts_explicit_unpacked_fixed_tuple_assignment() {
    let node = type_relation_node_with_base_child();

    assert!(crate::direct_type_is_assignable(
        &node,
        &[],
        "tuple[int, str]",
        "tuple[Unpack[tuple[int, str]]]",
    ));
}

#[test]
fn check_rejects_mapping_key_covariance_assignment() {
    let node = type_relation_node_with_base_child();

    assert!(!crate::direct_type_is_assignable(
        &node,
        &[],
        "Mapping[Base, int]",
        "Mapping[Child, int]",
    ));
}

#[test]
fn check_accepts_structural_protocol_argument_without_inheritance() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Protocol"),
                    kind: DeclarationKind::Import,
                    detail: String::from("typing.Protocol"),
                    value_type: None,
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
                    name: String::from("SupportsClose"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Interface),
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: vec![String::from("Protocol")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("close"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->None"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
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
                Declaration {
                    name: String::from("FileHandle"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("close"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->None"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("FileHandle"),
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
                Declaration {
                    name: String::from("consume"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:SupportsClose)->None"),
                    value_type: None,
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
            calls: vec![typepython_binding::CallSite {
                callee: String::from("consume"),
                arg_count: 1,
                arg_types: vec![String::from("FileHandle")],
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
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
        }],
    });

    assert!(result.diagnostics.is_empty(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_structural_interface_implementation_signature() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Protocol"),
                    kind: DeclarationKind::Import,
                    detail: String::from("typing.Protocol"),
                    value_type: None,
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
                    name: String::from("Runner"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Interface),
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: vec![String::from("Protocol")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:Child)->Base"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Runner"),
                        kind: DeclarationOwnerKind::Interface,
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
                Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("Child"),
                    kind: DeclarationKind::Class,
                    detail: String::from("Base"),
                    value_type: None,
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
                    bases: vec![String::from("Base")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("Impl"),
                    kind: DeclarationKind::Class,
                    detail: String::from("Runner"),
                    value_type: None,
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
                    bases: vec![String::from("Runner")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,x:Base)->Child"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Impl"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_async: false,
                    is_override: true,
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
        }],
    });

    assert!(result.diagnostics.is_empty(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_incompatible_imported_override_signature() {
    let result = check(&ModuleGraph {
        nodes: vec![
            ModuleNode {
                module_path: PathBuf::from("src/app/base.py"),
                module_key: String::from("app.base"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
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
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:int)->int"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
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
            ModuleNode {
                module_path: PathBuf::from("src/app/child.tpy"),
                module_key: String::from("app.child"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Import,
                        detail: String::from("app.base.Base"),
                        value_type: None,
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
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Base"),
                        value_type: None,
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
                        bases: vec![String::from("Base")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:str)->int"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Child"),
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
                summary_fingerprint: 2,
            },
        ],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4005"));
    assert!(rendered.contains("incompatible signature or annotation"));
}

#[test]
fn check_reports_incompatible_override_method_kind() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(cls)->None"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Class),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Base"),
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
                Declaration {
                    name: String::from("Child"),
                    kind: DeclarationKind::Class,
                    detail: String::from("Base"),
                    value_type: None,
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
                    bases: vec![String::from("Base")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self, exc_type, exc_val, exc_tb)->None"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Child"),
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
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4005"));
    assert!(rendered.contains("incompatible signature or annotation"));
}

#[test]
fn check_reports_missing_explicit_override_when_required() {
    let result = check_with_options(
        &ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
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
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
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
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
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
                        bases: vec![String::from("Base")],
                        type_params: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Child"),
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
            }],
        },
        true,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4005"));
    assert!(rendered.contains("missing @override"));
}

#[test]
fn check_reports_missing_explicit_override_when_required_for_imported_base() {
    let result = check_with_options(
        &ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/base.py"),
                    module_key: String::from("app.base"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Base"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
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
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->None"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Base"),
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
                ModuleNode {
                    module_path: PathBuf::from("src/app/child.tpy"),
                    module_key: String::from("app.child"),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![
                        Declaration {
                            name: String::from("Base"),
                            kind: DeclarationKind::Import,
                            detail: String::from("app.base.Base"),
                            value_type: None,
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
                            name: String::from("Child"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Base"),
                            value_type: None,
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
                            bases: vec![String::from("Base")],
                            type_params: Vec::new(),
                        },
                        Declaration {
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->None"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Child"),
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
                    summary_fingerprint: 2,
                },
            ],
        },
        true,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4005"));
    assert!(rendered.contains("missing @override"));
}

#[test]
fn check_reports_classvar_outside_class_scope() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("VALUE"),
                kind: DeclarationKind::Value,
                detail: String::new(),
                value_type: None,
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
                is_class_var: true,
                bases: Vec::new(),
                type_params: Vec::new(),
            }],
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("ClassVar binding `VALUE`"));
}

#[test]
fn check_accepts_classvar_inside_class_scope() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::new(),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("cache"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
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
                    is_class_var: true,
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_direct_method_call_result_return() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("get"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(box:Box)->str"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: None,
                value_member_name: None,
                value_member_through_instance: false,
                value_method_owner_name: Some(String::from("box")),
                value_method_name: Some(String::from("get")),
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
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_direct_method_call_result_assignment() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("get"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("box"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Box"),
                    value_type: None,
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
                    name: String::from("result"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("result"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("str")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: None,
                value_member_name: None,
                value_member_through_instance: false,
                value_method_owner_name: Some(String::from("box")),
                value_method_name: Some(String::from("get")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_direct_method_call_result_assignment_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("get"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("box"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Box"),
                    value_type: None,
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
                    name: String::from("result"),
                    kind: DeclarationKind::Value,
                    detail: String::from("int"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("result"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("int")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: None,
                value_member_name: None,
                value_member_through_instance: false,
                value_method_owner_name: Some(String::from("box")),
                value_method_name: Some(String::from("get")),
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
                owner_name: None,
                owner_type_name: None,
                line: 1,
            }],
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `str` where `result` expects `int`"));
}

#[test]
fn check_reports_direct_method_call_result_return_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("get"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(box:Box)->int"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: None,
                value_member_name: None,
                value_member_through_instance: false,
                value_method_owner_name: Some(String::from("box")),
                value_method_name: Some(String::from("get")),
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
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `str` where `build` expects `int`"));
}

#[test]
fn check_accepts_direct_method_call_result_through_instance() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("get"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("make_box"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Box"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->str"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: None,
                value_member_name: None,
                value_member_through_instance: false,
                value_method_owner_name: Some(String::from("make_box")),
                value_method_name: Some(String::from("get")),
                value_method_through_instance: true,
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
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_direct_method_call_result_through_instance_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("get"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("make_box"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Box"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: None,
                value_member_name: None,
                value_member_through_instance: false,
                value_method_owner_name: Some(String::from("make_box")),
                value_method_name: Some(String::from("get")),
                value_method_through_instance: true,
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
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `str` where `build` expects `int`"));
}

#[test]
fn check_reports_keyword_type_mismatch_in_method_calls() {
    let result = check_temp_typepython_source(
        "class User:\n    def set_age(self, age: int):\n        self.age = age\n\nuser = User()\nuser.set_age(age=\"oops\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("keyword `age`"));
    assert!(rendered.contains("parameter expects `int`"));
}

#[test]
fn check_reports_positional_only_method_parameter_passed_as_keyword() {
    let result = check_temp_typepython_source(
        "class User:\n    def set_age(self, age: int, /):\n        self.age = age\n\nuser = User()\nuser.set_age(age=1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("positional-only parameter `age`"));
}

#[test]
fn check_accepts_semantically_matching_keyword_type_in_direct_calls() {
    let result = check_temp_typepython_source(
        "from typing import Optional\n\ndef takes(x: Optional[int]) -> int:\n    return 0\n\ntakes(x=None)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_semantically_matching_keyword_type_in_method_calls() {
    let result = check_temp_typepython_source(
        "from typing import Optional\n\nclass User:\n    def set_age(self, age: Optional[int]):\n        self.age = age\n\nuser = User()\nuser.set_age(age=None)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_for_loop_target_type_in_local_assignment() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(values:list[int])->None"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: vec![typepython_binding::ForSite {
                target_name: String::from("item"),
                target_names: Vec::new(),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                iter_type: Some(String::new()),
                iter_is_awaited: false,
                iter_callee: None,
                iter_name: Some(String::from("values")),
                iter_member_owner_name: None,
                iter_member_name: None,
                iter_member_through_instance: false,
                iter_method_owner_name: None,
                iter_method_name: None,
                iter_method_through_instance: false,
                line: 2,
            }],
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("result"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("int")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("item")),
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
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 3,
            }],
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_for_loop_target_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(values:list[int])->None"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: vec![typepython_binding::ForSite {
                target_name: String::from("item"),
                target_names: Vec::new(),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                iter_type: Some(String::new()),
                iter_is_awaited: false,
                iter_callee: None,
                iter_name: Some(String::from("values")),
                iter_member_owner_name: None,
                iter_member_name: None,
                iter_member_through_instance: false,
                iter_method_owner_name: None,
                iter_method_name: None,
                iter_method_through_instance: false,
                line: 2,
            }],
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("result"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("str")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("item")),
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
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 3,
            }],
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where local `result` expects `str`"));
}

#[test]
fn check_accepts_tuple_for_loop_target_type_in_return() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(pairs:tuple[tuple[int, str]])->str"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("b")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 4,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: vec![typepython_binding::ForSite {
                target_name: String::new(),
                target_names: vec![String::from("a"), String::from("b")],
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                iter_type: Some(String::new()),
                iter_is_awaited: false,
                iter_callee: None,
                iter_name: Some(String::from("pairs")),
                iter_member_owner_name: None,
                iter_member_name: None,
                iter_member_through_instance: false,
                iter_method_owner_name: None,
                iter_method_name: None,
                iter_method_through_instance: false,
                line: 2,
            }],
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_sequence_for_loop_target_type_in_return() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(pairs:list[Sequence[int]])->int"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("b")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 4,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: vec![typepython_binding::ForSite {
                target_name: String::new(),
                target_names: vec![String::from("a"), String::from("b")],
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                iter_type: Some(String::new()),
                iter_is_awaited: false,
                iter_callee: None,
                iter_name: Some(String::from("pairs")),
                iter_member_owner_name: None,
                iter_member_name: None,
                iter_member_through_instance: false,
                iter_method_owner_name: None,
                iter_method_name: None,
                iter_method_through_instance: false,
                line: 2,
            }],
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_tuple_for_loop_target_type_mismatch() {
    let graph = ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(pairs:tuple[tuple[int, str]])->int"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("b")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 4,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: vec![typepython_binding::ForSite {
                target_name: String::new(),
                target_names: vec![String::from("a"), String::from("b")],
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                iter_type: Some(String::new()),
                iter_is_awaited: false,
                iter_callee: None,
                iter_name: Some(String::from("pairs")),
                iter_member_owner_name: None,
                iter_member_name: None,
                iter_member_through_instance: false,
                iter_method_owner_name: None,
                iter_method_name: None,
                iter_method_through_instance: false,
                line: 2,
            }],
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
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `str` where `build` expects `int`"));
}

#[test]
fn check_reports_tuple_for_loop_target_arity_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(pairs:tuple[tuple[int]])->None"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: vec![typepython_binding::ForSite {
                target_name: String::new(),
                target_names: vec![String::from("a"), String::from("b")],
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                iter_type: Some(String::new()),
                iter_is_awaited: false,
                iter_callee: None,
                iter_name: Some(String::from("pairs")),
                iter_member_owner_name: None,
                iter_member_name: None,
                iter_member_through_instance: false,
                iter_method_owner_name: None,
                iter_method_name: None,
                iter_method_through_instance: false,
                line: 2,
            }],
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("destructures `for` target `(a, b)` with 2 name(s) from tuple element type `tuple[int]` with 1 element(s)"));
}

#[test]
fn check_accepts_with_target_type_in_return() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Manager"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("__enter__"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Manager"),
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
                Declaration {
                    name: String::from("__exit__"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self, exc_type, exc_val, exc_tb)->None"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Manager"),
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
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(manager:Manager)->str"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 4,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: vec![typepython_binding::WithSite {
                target_name: Some(String::from("value")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                context_type: Some(String::new()),
                context_is_awaited: false,
                context_callee: None,
                context_name: Some(String::from("manager")),
                context_member_owner_name: None,
                context_member_name: None,
                context_member_through_instance: false,
                context_method_owner_name: None,
                context_method_name: None,
                context_method_through_instance: false,
                line: 2,
            }],
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_with_target_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Manager"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("__enter__"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->int"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Manager"),
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
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(manager:Manager)->None"),
                    value_type: None,
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
            with_statements: vec![typepython_binding::WithSite {
                target_name: Some(String::from("value")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                context_type: Some(String::new()),
                context_is_awaited: false,
                context_callee: None,
                context_name: Some(String::from("manager")),
                context_member_owner_name: None,
                context_member_name: None,
                context_member_through_instance: false,
                context_method_owner_name: None,
                context_method_name: None,
                context_method_through_instance: false,
                line: 2,
            }],
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("lacks compatible `__enter__`/`__exit__` members"));
}

#[test]
fn check_accepts_except_handler_binding_type() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("()->ValueError"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("e")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 5,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: vec![typepython_binding::ExceptHandlerSite {
                exception_type: String::from("ValueError"),
                binding_name: Some(String::from("e")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 4,
                end_line: 5,
            }],
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_except_handler_binding_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("()->TypeError"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("e")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 5,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: vec![typepython_binding::ExceptHandlerSite {
                exception_type: String::from("ValueError"),
                binding_name: Some(String::from("e")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 4,
                end_line: 5,
            }],
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `ValueError` where `build` expects `TypeError`"));
}

#[test]
fn check_does_not_keep_except_binding_after_handler() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("()->ValueError"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("e")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 6,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: vec![typepython_binding::ExceptHandlerSite {
                exception_type: String::from("ValueError"),
                binding_name: Some(String::from("e")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 4,
                end_line: 5,
            }],
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_tuple_except_handler_binding_type() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("()->Union[ValueError, TypeError]"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("e")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 5,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: vec![typepython_binding::ExceptHandlerSite {
                exception_type: String::from("(ValueError, TypeError)"),
                binding_name: Some(String::from("e")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 4,
                end_line: 5,
            }],
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_tuple_except_handler_binding_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("()->ValueError"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("e")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 5,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: vec![typepython_binding::ExceptHandlerSite {
                exception_type: String::from("(ValueError, TypeError)"),
                binding_name: Some(String::from("e")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 4,
                end_line: 5,
            }],
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(
        rendered
            .contains("returns `Union[ValueError, TypeError]` where `build` expects `ValueError`")
    );
}

#[test]
fn check_accepts_bare_except_handler_binding_type() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("()->BaseException"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("e")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 5,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: vec![typepython_binding::ExceptHandlerSite {
                exception_type: String::from("BaseException"),
                binding_name: Some(String::from("e")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 4,
                end_line: 5,
            }],
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_bare_except_handler_binding_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("()->ValueError"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("e")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 5,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: vec![typepython_binding::ExceptHandlerSite {
                exception_type: String::from("BaseException"),
                binding_name: Some(String::from("e")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 4,
                end_line: 5,
            }],
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `BaseException` where `build` expects `ValueError`"));
}

#[test]
fn check_reports_non_exhaustive_sealed_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.tpy"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("Expr"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::SealedClass),
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
                    name: String::from("Add"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Expr")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("Mul"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Expr")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(expr:Expr)->None"),
                    value_type: None,
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
            matches: vec![typepython_binding::MatchSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                subject_type: Some(String::new()),
                subject_is_awaited: false,
                subject_callee: None,
                subject_name: Some(String::from("expr")),
                subject_member_owner_name: None,
                subject_member_name: None,
                subject_member_through_instance: false,
                subject_method_owner_name: None,
                subject_method_name: None,
                subject_method_through_instance: false,
                cases: vec![typepython_binding::MatchCaseSite {
                    patterns: vec![typepython_binding::MatchPatternSite::Class(String::from(
                        "Add",
                    ))],
                    has_guard: false,
                    line: 3,
                }],
                line: 2,
            }],
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4009"));
    assert!(rendered.contains("missing subclasses: Mul"));
}

#[test]
fn check_accepts_exhaustive_sealed_match_with_wildcard() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.tpy"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("Expr"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::SealedClass),
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
                    name: String::from("Add"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Expr")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("Mul"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Expr")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(expr:Expr)->None"),
                    value_type: None,
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
            matches: vec![typepython_binding::MatchSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                subject_type: Some(String::new()),
                subject_is_awaited: false,
                subject_callee: None,
                subject_name: Some(String::from("expr")),
                subject_member_owner_name: None,
                subject_member_name: None,
                subject_member_through_instance: false,
                subject_method_owner_name: None,
                subject_method_name: None,
                subject_method_through_instance: false,
                cases: vec![
                    typepython_binding::MatchCaseSite {
                        patterns: vec![typepython_binding::MatchPatternSite::Class(String::from(
                            "Add",
                        ))],
                        has_guard: false,
                        line: 3,
                    },
                    typepython_binding::MatchCaseSite {
                        patterns: vec![typepython_binding::MatchPatternSite::Wildcard],
                        has_guard: false,
                        line: 5,
                    },
                ],
                line: 2,
            }],
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_if_is_not_none_narrowing_for_return() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(value:Optional[str])->str"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 3,
            }],
            yields: Vec::new(),
            if_guards: vec![typepython_binding::IfGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_binding::GuardConditionSite::IsNone {
                    name: String::from("value"),
                    negated: true,
                }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: None,
                false_end_line: None,
            }],
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_assert_is_not_none_narrowing_for_return() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(value:Optional[str])->str"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 3,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: vec![typepython_binding::AssertGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_binding::GuardConditionSite::IsNone {
                    name: String::from("value"),
                    negated: true,
                }),
                line: 2,
            }],
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_isinstance_tuple_narrowing_for_return() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(value:Union[str, bytes, int])->Union[str, bytes]"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 3,
            }],
            yields: Vec::new(),
            if_guards: vec![typepython_binding::IfGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_binding::GuardConditionSite::IsInstance {
                    name: String::from("value"),
                    types: vec![String::from("str"), String::from("bytes")],
                }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: None,
                false_end_line: None,
            }],
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_typeguard_true_branch_narrowing() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("is_text"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Union[str, int])->TypeGuard[str]"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Union[str, int])->str"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 3,
            }],
            yields: Vec::new(),
            if_guards: vec![typepython_binding::IfGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_binding::GuardConditionSite::PredicateCall {
                    name: String::from("value"),
                    callee: String::from("is_text"),
                }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: None,
                false_end_line: None,
            }],
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_typeis_false_branch_narrowing() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("is_text"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Union[str, int])->TypeIs[str]"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Union[str, int])->int"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 5,
            }],
            yields: Vec::new(),
            if_guards: vec![typepython_binding::IfGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_binding::GuardConditionSite::PredicateCall {
                    name: String::from("value"),
                    callee: String::from("is_text"),
                }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: Some(5),
                false_end_line: Some(5),
            }],
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_typeis_post_if_fallthrough_narrowing() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("is_text"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Union[str, int])->TypeIs[str]"),
                    value_type: None,
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
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Union[str, int])->int"),
                    value_type: None,
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
            returns: vec![
                typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::from("int")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 3,
                },
                typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 4,
                },
            ],
            yields: Vec::new(),
            if_guards: vec![typepython_binding::IfGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_binding::GuardConditionSite::PredicateCall {
                    name: String::from("value"),
                    callee: String::from("is_text"),
                }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: None,
                false_end_line: None,
            }],
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_boolean_composition_narrowing() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(value:str | None)->str"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 3,
            }],
            yields: Vec::new(),
            if_guards: vec![typepython_binding::IfGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_binding::GuardConditionSite::And(vec![
                    typepython_binding::GuardConditionSite::Not(Box::new(
                        typepython_binding::GuardConditionSite::IsNone {
                            name: String::from("value"),
                            negated: false,
                        },
                    )),
                    typepython_binding::GuardConditionSite::IsInstance {
                        name: String::from("value"),
                        types: vec![String::from("str")],
                    },
                ])),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: None,
                false_end_line: None,
            }],
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_boolop_rhs_narrowing_for_is_not_none_and() {
    let result = check_temp_typepython_source(
        "def takes_int(value: int) -> bool:\n    return True\n\ndef build(value: int | None) -> bool:\n    return value is not None and takes_int(value)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_boolop_rhs_narrowing_for_is_none_or() {
    let result = check_temp_typepython_source(
        "def takes_int(value: int) -> bool:\n    return True\n\ndef build(value: int | None) -> bool:\n    return value is None or takes_int(value)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_boolop_rhs_narrowing_for_isinstance_call() {
    let result = check_temp_typepython_source(
        "def takes_str(value: str) -> bool:\n    return True\n\ndef build(value: str | int) -> bool:\n    return isinstance(value, str) and takes_str(value)\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_keeps_non_guard_boolop_rhs_unnarrowed() {
    let result = check_temp_typepython_source(
        "def truthy(value: int | None) -> bool:\n    return True\n\ndef build(value: int | None) -> None:\n    result: int = truthy(value) and value\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(
        rendered.contains("assigns `Union[bool, int, None]` where local `result` expects `int`")
    );
}

#[test]
fn check_accepts_truthiness_narrowing_for_bool_optional() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(flag:Optional[Literal[True]])->Literal[True]"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("flag")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 3,
            }],
            yields: Vec::new(),
            if_guards: vec![typepython_binding::IfGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_binding::GuardConditionSite::TruthyName {
                    name: String::from("flag"),
                }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: None,
                false_end_line: None,
            }],
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

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_does_not_over_narrow_truthiness_for_int_optional() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(value:Optional[int])->int"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 3,
            }],
            yields: Vec::new(),
            if_guards: vec![typepython_binding::IfGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_binding::GuardConditionSite::TruthyName {
                    name: String::from("value"),
                }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: None,
                false_end_line: None,
            }],
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `Optional[int]` where `build` expects `int`"));
}

#[test]
fn check_invalidates_narrowing_after_augassign() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(value:Optional[int])->int"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 4,
            }],
            yields: Vec::new(),
            if_guards: vec![typepython_binding::IfGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_binding::GuardConditionSite::IsNone {
                    name: String::from("value"),
                    negated: true,
                }),
                line: 2,
                true_start_line: 3,
                true_end_line: 4,
                false_start_line: None,
                false_end_line: None,
            }],
            asserts: Vec::new(),
            invalidations: vec![typepython_binding::InvalidationSite {
                kind: typepython_binding::InvalidationKind::RebindLike,
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                names: vec![String::from("value")],
                line: 3,
            }],
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `Optional[int]` where `build` expects `int`"));
}

#[test]
fn check_joins_branch_local_assignments_after_if() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(flag:bool)->Union[str, int]"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("result")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 5,
            }],
            yields: Vec::new(),
            if_guards: vec![typepython_binding::IfGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(typepython_binding::GuardConditionSite::TruthyName {
                    name: String::from("flag"),
                }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: Some(4),
                false_end_line: Some(4),
            }],
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: vec![
                typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::from("str")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 3,
                },
                typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: None,
                    value_type: Some(String::from("int")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                },
            ],
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_direct_recursive_type_alias_assignment() {
    let result = check_temp_typepython_source(
        "typealias Tree = int | list[Tree]\n\nvalue: Tree = [1, [2, 3]]\n",
    );

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_generic_recursive_type_alias_assignment() {
    let result = check_temp_typepython_source(
        "typealias Nested[T] = T | list[Nested[T]]\n\nvalue: Nested[int] = [1, [2, 3]]\n",
    );

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_deprecated_import_and_call_when_enabled() {
    let result = check_with_options(
        &ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/lib/deps.py"),
                    module_key: String::from("lib.deps"),
                    module_kind: SourceKind::Python,
                    declarations: vec![Declaration {
                        name: String::from("old"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: true,
                        deprecation_message: Some(String::from("use new instead")),
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
                    module_path: PathBuf::from("src/app/module.tpy"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![Declaration {
                        name: String::from("old"),
                        kind: DeclarationKind::Import,
                        detail: String::from("lib.deps.old"),
                        value_type: None,
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
                    calls: vec![typepython_binding::CallSite {
                        callee: String::from("old"),
                        arg_count: 0,
                        arg_types: Vec::new(),
                        arg_values: Vec::new(),
                        starred_arg_types: Vec::new(),
                        starred_arg_values: Vec::new(),
                        keyword_names: Vec::new(),
                        keyword_arg_types: Vec::new(),
                        keyword_arg_values: Vec::new(),
                        keyword_expansion_types: Vec::new(),
                        keyword_expansion_values: Vec::new(),
                        line: 1,
                    }],
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
                    summary_fingerprint: 2,
                },
            ],
        },
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4101"));
    assert!(rendered.contains("imports deprecated declaration `old`"));
    assert!(rendered.contains("calls deprecated declaration `old`"));
    assert!(rendered.contains("use new instead"));
}

#[test]
fn check_ignores_deprecated_uses_when_configured() {
    let result = check_with_options(
        &ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/lib/deps.py"),
                    module_key: String::from("lib.deps"),
                    module_kind: SourceKind::Python,
                    declarations: vec![Declaration {
                        name: String::from("old"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: true,
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
                    module_path: PathBuf::from("src/app/module.tpy"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![Declaration {
                        name: String::from("old"),
                        kind: DeclarationKind::Import,
                        detail: String::from("lib.deps.old"),
                        value_type: None,
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
                    calls: vec![typepython_binding::CallSite {
                        callee: String::from("old"),
                        arg_count: 0,
                        arg_types: Vec::new(),
                        arg_values: Vec::new(),
                        starred_arg_types: Vec::new(),
                        starred_arg_values: Vec::new(),
                        keyword_names: Vec::new(),
                        keyword_arg_types: Vec::new(),
                        keyword_arg_values: Vec::new(),
                        keyword_expansion_types: Vec::new(),
                        keyword_expansion_values: Vec::new(),
                        line: 1,
                    }],
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
                    summary_fingerprint: 2,
                },
            ],
        },
        false,
        true,
        DiagnosticLevel::Ignore,
        false,
        false,
        ImportFallback::Unknown,
    );

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_mutual_recursive_type_aliases() {
    let result = check_temp_typepython_source(concat!(
        "typealias JsonObject = dict[str, JsonValue]\n",
        "typealias JsonArray = list[JsonValue]\n",
        "typealias JsonValue = None | bool | int | str | JsonObject | JsonArray\n\n",
        "payload: JsonValue = {\"items\": [1, True, None, {\"name\": \"Ada\"}]}\n",
    ));

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_recursive_type_alias_value_mismatch() {
    let result = check_temp_typepython_source(concat!(
        "typealias JsonObject = dict[str, JsonValue]\n",
        "typealias JsonArray = list[JsonValue]\n",
        "typealias JsonValue = None | bool | int | str | JsonObject | JsonArray\n\n",
        "payload: JsonValue = {\"items\": {1}}\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("JsonValue"));
    assert!(rendered.contains("set[int]"));
}

#[test]
fn direct_type_is_assignable_accepts_mutual_recursive_alias_value() {
    let node = ModuleNode {
        module_path: PathBuf::from("src/app/module.tpy"),
        module_key: String::from("app.module"),
        module_kind: SourceKind::TypePython,
        declarations: vec![
            Declaration {
                name: String::from("JsonObject"),
                kind: DeclarationKind::TypeAlias,
                detail: String::from("dict[str, JsonValue]"),
                value_type: None,
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
                name: String::from("JsonArray"),
                kind: DeclarationKind::TypeAlias,
                detail: String::from("list[JsonValue]"),
                value_type: None,
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
                name: String::from("JsonValue"),
                kind: DeclarationKind::TypeAlias,
                detail: String::from("None | bool | int | str | JsonObject | JsonArray"),
                value_type: None,
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
    };
    let graph = vec![node.clone()];

    assert!(!super::direct_type_is_assignable(&node, &graph, "bool", "int"));
    assert!(super::direct_type_is_assignable(&node, &graph, "JsonValue", "str"));
    assert!(super::direct_type_is_assignable(&node, &graph, "JsonValue", "dict[str, str]"));
    assert!(super::direct_type_is_assignable(
        &node,
        &graph,
        "JsonValue",
        "int | bool | None | dict[str, str]",
    ));
    assert!(super::direct_type_is_assignable(
        &node,
        &graph,
        "JsonValue",
        "list[int | bool | None | dict[str, str]]",
    ));
    assert!(super::direct_type_is_assignable(
        &node,
        &graph,
        "JsonValue",
        "dict[str, list[int | bool | None | dict[str, str]]]",
    ));
}

#[test]
fn check_accepts_self_return_through_inherited_method_call() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("clone"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->Self"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
                    class_kind: None,
                    owner: Some(typepython_binding::DeclarationOwner {
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("SubBox"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Box")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(box:SubBox)->SubBox"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: None,
                value_member_name: None,
                value_member_through_instance: false,
                value_method_owner_name: Some(String::from("box")),
                value_method_name: Some(String::from("clone")),
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
                line: 3,
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
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_self_parameter_annotation_in_method_call() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("merge"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self,other:Self)->Self"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
                    class_kind: None,
                    owner: Some(typepython_binding::DeclarationOwner {
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
            method_calls: vec![typepython_binding::MethodCallSite {
                owner_name: String::from("Box"),
                method: String::from("merge"),
                through_instance: true,
                arg_count: 1,
                arg_types: vec![String::from("Box")],
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            }],
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_self_typed_attribute_access() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Node"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("next"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Self"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(typepython_binding::DeclarationOwner {
                        name: String::from("Node"),
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
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(node:Node)->Node"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("node")),
                value_member_name: Some(String::from("next")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 3,
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
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_property_access_in_return() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("name"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Property),
                    class_kind: None,
                    owner: Some(typepython_binding::DeclarationOwner {
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(box:Box)->str"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("box")),
                value_member_name: Some(String::from("name")),
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
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_property_access_assignment_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("name"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Property),
                    class_kind: None,
                    owner: Some(typepython_binding::DeclarationOwner {
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
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(box:Box)->None"),
                    value_type: None,
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
            assignments: vec![typepython_binding::AssignmentSite {
                name: String::from("value"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("int")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("box")),
                value_member_name: Some(String::from("name")),
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
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 2,
            }],
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `str` where local `value` expects `int`"));
}

#[test]
fn check_accepts_declared_attribute_assignment() {
    let result = check_temp_typepython_source(
        "class Box:\n    name: str\n\ndef mutate(box: Box) -> None:\n    box.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_inherited_attribute_assignment() {
    let result = check_temp_typepython_source(
        "class Base:\n    name: str\n\nclass Box(Base):\n    pass\n\ndef mutate(box: Box) -> None:\n    box.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_attribute_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "class Box:\n    name: str\n\ndef mutate(box: Box) -> None:\n    box.name = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where member `name` expects `str`"));
}

#[test]
fn check_accepts_contextual_declared_attribute_assignment_lambda() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\nclass Box:\n    handler: Callable[[int], str]\n\ndef mutate(box: Box) -> None:\n    box.handler = lambda x: str(x)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_contextual_property_setter_assignment_typed_dict() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    name: str\n\nclass Box:\n    @property\n    def user(self) -> User:\n        return {\"name\": \"Ada\"}\n\n    @user.setter\n    def user(self, value: User) -> None:\n        pass\n\ndef mutate(box: Box) -> None:\n    box.user = {\"name\": \"Grace\"}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_contextual_property_setter_assignment_typed_dict_missing_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    name: str\n\nclass Box:\n    @property\n    def user(self) -> User:\n        return {\"name\": \"Ada\"}\n\n    @user.setter\n    def user(self, value: User) -> None:\n        pass\n\ndef mutate(box: Box) -> None:\n    box.user = {}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_reports_contextual_declared_attribute_assignment_lambda_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\nclass Box:\n    handler: Callable[[int], str]\n\ndef mutate(box: Box) -> None:\n    box.handler = lambda x: x + 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("Callable[[int], str]"));
}

#[test]
fn check_accepts_declared_attribute_augmented_assignment() {
    let result = check_temp_typepython_source(
        "class Box:\n    count: int\n\ndef mutate(box: Box) -> None:\n    box.count += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_declared_attribute_augmented_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "class Box:\n    count: int\n\ndef mutate(box: Box) -> None:\n    box.count += \"x\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("produces `str` where member `count` expects `int`"));
}

#[test]
fn check_ignores_undeclared_attribute_assignment_target() {
    let result = check_temp_typepython_source(
        "class Box:\n    name: str\n\ndef mutate(box: Box) -> None:\n    box.age = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_getter_only_property_assignment() {
    let result = check_temp_typepython_source(
        "class Box:\n    @property\n    def name(self) -> str:\n        return \"x\"\n\ndef mutate(box: Box) -> None:\n    box.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("property `name` on `Box`"));
    assert!(rendered.contains("is not writable"));
}

#[test]
fn check_reports_getter_only_property_augmented_assignment() {
    let result = check_temp_typepython_source(
        "class Box:\n    @property\n    def count(self) -> int:\n        return 0\n\ndef mutate(box: Box) -> None:\n    box.count += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("property `count` on `Box`"));
    assert!(rendered.contains("is not writable"));
}

#[test]
fn check_accepts_property_setter_assignment() {
    let result = check_temp_typepython_source(
        "class Box:\n    @property\n    def name(self) -> str:\n        return \"x\"\n\n    @name.setter\n    def name(self, value: str) -> None:\n        pass\n\ndef mutate(box: Box) -> None:\n    box.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_property_setter_augmented_assignment() {
    let result = check_temp_typepython_source(
        "class Box:\n    @property\n    def count(self) -> int:\n        return 0\n\n    @count.setter\n    def count(self, value: int) -> None:\n        pass\n\ndef mutate(box: Box) -> None:\n    box.count += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_property_setter_augmented_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "class Box:\n    @property\n    def count(self) -> int:\n        return 0\n\n    @count.setter\n    def count(self, value: int) -> None:\n        pass\n\ndef mutate(box: Box) -> None:\n    box.count += \"x\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("produces `str` where member `count` expects `int`"));
}

#[test]
fn check_reports_property_setter_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "class Box:\n    @property\n    def name(self) -> str:\n        return \"x\"\n\n    @name.setter\n    def name(self, value: str) -> None:\n        pass\n\ndef mutate(box: Box) -> None:\n    box.name = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where member `name` expects `str`"));
}

#[test]
fn check_accepts_inherited_property_setter_assignment() {
    let result = check_temp_typepython_source(
        "class Base:\n    @property\n    def name(self) -> str:\n        return \"x\"\n\n    @name.setter\n    def name(self, value: str) -> None:\n        pass\n\nclass Box(Base):\n    pass\n\ndef mutate(box: Box) -> None:\n    box.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_property_getter_setter_type_mismatch() {
    let result = check_temp_typepython_source(
        "class Box:\n    @property\n    def name(self) -> str:\n        return \"x\"\n\n    @name.setter\n    def name(self, value: int) -> None:\n        pass\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("getter type `str` but setter expects `int`"));
}

#[test]
fn check_accepts_inherited_property_access() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Base"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("name"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Property),
                    class_kind: None,
                    owner: Some(typepython_binding::DeclarationOwner {
                        name: String::from("Base"),
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
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Base")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(box:Box)->str"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("box")),
                value_member_name: Some(String::from("name")),
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
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_bare_property_member_access() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    detail: String::from("Box"),
                    value_type: None,
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
                    name: String::from("name"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Property),
                    class_kind: None,
                    owner: Some(typepython_binding::DeclarationOwner {
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
                    type_params: Vec::new(),
                },
            ],
            calls: Vec::new(),
            method_calls: Vec::new(),
            member_accesses: vec![typepython_binding::MemberAccessSite {
                current_owner_name: None,
                current_owner_type_name: None,
                owner_name: String::from("box"),
                member: String::from("name"),
                through_instance: false,
                line: 1,
            }],
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
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_dict_subscript_read_type() {
    let result = check_temp_typepython_source(
        "def build(values: dict[str, int]) -> int:\n    return values[\"x\"]\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_mapping_subscript_read_type() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("build"),
                kind: DeclarationKind::Function,
                detail: String::from("(values:Mapping[str, int])->int"),
                value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: None,
                value_member_name: None,
                value_member_through_instance: false,
                value_method_owner_name: None,
                value_method_name: None,
                value_method_through_instance: false,
                value_subscript_target: Some(Box::new(typepython_syntax::DirectExprMetadata {
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("values")),
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
                })),
                value_subscript_string_key: Some(String::from("x")),
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
fn check_accepts_nominal_getitem_subscript_read_type() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __getitem__(self, key: str) -> int:\n        return 1\n\ndef build(cache: Cache) -> int:\n    return cache[\"x\"]\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_inherited_getitem_subscript_read_type() {
    let result = check_temp_typepython_source(
        "class Base:\n    def __getitem__(self, key: str) -> int:\n        return 1\n\nclass Cache(Base):\n    pass\n\ndef build(cache: Cache) -> int:\n    return cache[\"x\"]\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_subscript_read_type_mismatch() {
    let result = check_temp_typepython_source(
        "def build(values: dict[str, int]) -> str:\n    return values[\"x\"]\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("returns `int` where `build` expects `str`"));
}

#[test]
fn check_accepts_enum_member_access_as_enum_type() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Enum"),
                    kind: DeclarationKind::Import,
                    detail: String::from("enum.Enum"),
                    value_type: None,
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
                    name: String::from("Color"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Enum")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("RED"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Color"),
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
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Color"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("Color")),
                value_member_name: Some(String::from("RED")),
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
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_strenum_member_access_as_enum_type() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("StrEnum"),
                    kind: DeclarationKind::Import,
                    detail: String::from("enum.StrEnum"),
                    value_type: None,
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
                    name: String::from("Color"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("StrEnum")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("RED"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("str")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Color"),
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
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Color"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: None,
                value_member_owner_name: Some(String::from("Color")),
                value_member_name: Some(String::from("RED")),
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
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_non_exhaustive_enum_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.tpy"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("Enum"),
                    kind: DeclarationKind::Import,
                    detail: String::from("enum.Enum"),
                    value_type: None,
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
                    name: String::from("Color"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Enum")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("RED"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Color"),
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
                Declaration {
                    name: String::from("BLUE"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Color"),
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
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(color:Color)->None"),
                    value_type: None,
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
            matches: vec![typepython_binding::MatchSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                subject_type: Some(String::new()),
                subject_is_awaited: false,
                subject_callee: None,
                subject_name: Some(String::from("color")),
                subject_member_owner_name: None,
                subject_member_name: None,
                subject_member_through_instance: false,
                subject_method_owner_name: None,
                subject_method_name: None,
                subject_method_through_instance: false,
                cases: vec![typepython_binding::MatchCaseSite {
                    patterns: vec![typepython_binding::MatchPatternSite::Literal(String::from(
                        "Color.RED",
                    ))],
                    has_guard: false,
                    line: 3,
                }],
                line: 2,
            }],
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4009"));
    assert!(rendered.contains("non-exhaustive `match` over enum `Color`"));
    assert!(rendered.contains("missing members: BLUE"));
}

#[test]
fn check_reports_non_exhaustive_match_with_case_suggestion() {
    let root = create_temp_typepython_root();
    let path = root.join("src/app/module.tpy");
    fs::create_dir_all(path.parent().expect("temp source parent should exist"))
        .expect("temp source parent should be created");
    fs::write(
        &path,
        concat!(
            "def render(expr):\n",
            "    match expr:\n",
            "        case Num:\n",
            "            return 1\n",
            "        case Add:\n",
            "            return 2\n",
        ),
    )
    .expect("temp source should be written");

    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: path.clone(),
            module_key: String::from("app.module"),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("Expr"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::SealedClass),
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
                    name: String::from("Num"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Expr")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("Add"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Expr")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("Mul"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Expr")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("render"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(expr:Expr)->int"),
                    value_type: None,
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
            matches: vec![typepython_binding::MatchSite {
                owner_name: Some(String::from("render")),
                owner_type_name: None,
                subject_type: Some(String::new()),
                subject_is_awaited: false,
                subject_callee: None,
                subject_name: Some(String::from("expr")),
                subject_member_owner_name: None,
                subject_member_name: None,
                subject_member_through_instance: false,
                subject_method_owner_name: None,
                subject_method_name: None,
                subject_method_through_instance: false,
                cases: vec![
                    typepython_binding::MatchCaseSite {
                        patterns: vec![typepython_binding::MatchPatternSite::Class(String::from(
                            "Num",
                        ))],
                        has_guard: false,
                        line: 3,
                    },
                    typepython_binding::MatchCaseSite {
                        patterns: vec![typepython_binding::MatchPatternSite::Class(String::from(
                            "Add",
                        ))],
                        has_guard: false,
                        line: 5,
                    },
                ],
                line: 2,
            }],
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });
    let _ = fs::remove_dir_all(&root);

    let diagnostic = result
        .diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "TPY4009")
        .expect("non-exhaustive match diagnostic should be present");
    assert_eq!(diagnostic.suggestions.len(), 1);
    assert!(diagnostic.suggestions[0].message.contains("Add missing `match` case arms"));
    assert!(diagnostic.suggestions[0].replacement.contains("case Mul:"));
}

#[test]
fn check_accepts_with_without_target() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("Manager"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("__enter__"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Manager"),
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
                Declaration {
                    name: String::from("__exit__"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self, exc_type, exc_val, exc_tb)->None"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Manager"),
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
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(manager:Manager)->None"),
                    value_type: None,
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
            with_statements: vec![typepython_binding::WithSite {
                target_name: None,
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                context_type: Some(String::new()),
                context_is_awaited: false,
                context_callee: None,
                context_name: Some(String::from("manager")),
                context_member_owner_name: None,
                context_member_name: None,
                context_member_through_instance: false,
                context_method_owner_name: None,
                context_method_name: None,
                context_method_through_instance: false,
                line: 2,
            }],
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_accepts_multiple_with_items() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("A"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("__enter__"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->int"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
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
                Declaration {
                    name: String::from("__exit__"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self, exc_type, exc_val, exc_tb)->None"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
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
                Declaration {
                    name: String::from("B"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    name: String::from("__enter__"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self)->str"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("B"),
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
                Declaration {
                    name: String::from("__exit__"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(self, exc_type, exc_val, exc_tb)->None"),
                    value_type: None,
                    method_kind: Some(typepython_syntax::MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("B"),
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
                Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(a:A,b:B)->str"),
                    value_type: None,
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
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("build"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("y")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 4,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: vec![
                typepython_binding::WithSite {
                    target_name: Some(String::from("x")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    context_type: Some(String::new()),
                    context_is_awaited: false,
                    context_callee: None,
                    context_name: Some(String::from("a")),
                    context_member_owner_name: None,
                    context_member_name: None,
                    context_member_through_instance: false,
                    context_method_owner_name: None,
                    context_method_name: None,
                    context_method_through_instance: false,
                    line: 2,
                },
                typepython_binding::WithSite {
                    target_name: Some(String::from("y")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    context_type: Some(String::new()),
                    context_is_awaited: false,
                    context_callee: None,
                    context_name: Some(String::from("b")),
                    context_member_owner_name: None,
                    context_member_name: None,
                    context_member_through_instance: false,
                    context_method_owner_name: None,
                    context_method_name: None,
                    context_method_through_instance: false,
                    line: 2,
                },
            ],
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty());
}

#[test]
fn check_reports_return_inference_trace_and_none_suggestion() {
    let result = check_temp_typepython_source(
        "def build(flag: bool) -> int:\n    if flag:\n        return 1\n    return None\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(
        rendered.contains("inferred return type: `Optional[int]`")
            || rendered.contains("inferred return type: `Union[int, None]`")
            || rendered.contains("inferred return type: `int | None`")
    );
    assert!(rendered.contains("declared return type: `int`"));
    assert!(rendered.contains("inference trace:"));
    assert!(
        rendered.contains("join: `Optional[int]`")
            || rendered.contains("join: `Union[int, None]`")
            || rendered.contains("join: `int | None`")
    );
    assert!(rendered.contains("Add `| None` to the declared return type"));
}

#[test]
fn check_reports_nested_call_type_mismatch_path() {
    let result = check_temp_typepython_source(
        "from typing import Sequence\n\ndef takes(values: Sequence[tuple[int, int]]) -> None:\n    return None\n\npayload: list[tuple[int]] = [(1,)]\ntakes(payload)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("source: `list[tuple[int]]`"));
    assert!(rendered.contains("target: `Sequence[tuple[int, int]]`"));
    assert!(rendered.contains("mismatch at:"));
    assert!(rendered.contains("tuple[int]"));
}

#[test]
fn check_reports_unsafe_setattr_outside_unsafe_block() {
    let result = check_temp_typepython_source_with_check_options(
        "class Obj:\n    pass\n\ndef run(attr: str) -> None:\n    obj = Obj()\n    setattr(obj, attr, 1)\n",
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4019"));
}

#[test]
fn check_reports_unsafe_exec_outside_unsafe_block() {
    let result = check_temp_typepython_source_with_check_options(
        "def run() -> None:\n    exec(\"pass\")\n",
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        true,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4019"));
}

#[test]
fn check_accepts_setattr_inside_unsafe_block() {
    let result = check_temp_typepython_source_with_check_options(
        "class Obj:\n    pass\n\ndef run(attr: str) -> None:\n    obj = Obj()\n    unsafe:\n        setattr(obj, attr, 1)\n",
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        true,
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_deprecated_function_call_as_error() {
    let result = check_with_options(
        &ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/lib/legacy.py"),
                    module_key: String::from("lib.legacy"),
                    module_kind: SourceKind::Python,
                    declarations: vec![Declaration {
                        name: String::from("old_func"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: true,
                        deprecation_message: Some(String::from("removed")),
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
                    module_path: PathBuf::from("src/app/module.tpy"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![Declaration {
                        name: String::from("old_func"),
                        kind: DeclarationKind::Import,
                        detail: String::from("lib.legacy.old_func"),
                        value_type: None,
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
                    calls: vec![typepython_binding::CallSite {
                        callee: String::from("old_func"),
                        arg_count: 0,
                        arg_types: Vec::new(),
                        arg_values: Vec::new(),
                        starred_arg_types: Vec::new(),
                        starred_arg_values: Vec::new(),
                        keyword_names: Vec::new(),
                        keyword_arg_types: Vec::new(),
                        keyword_arg_values: Vec::new(),
                        keyword_expansion_types: Vec::new(),
                        keyword_expansion_values: Vec::new(),
                        line: 3,
                    }],
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
                    summary_fingerprint: 2,
                },
            ],
        },
        false,
        true,
        DiagnosticLevel::Error,
        false,
        false,
        ImportFallback::Unknown,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4101"));
    assert!(result.diagnostics.has_errors());
}

#[test]
fn check_reports_incompatible_attribute_augmented_assignment_type() {
    let result = check_temp_typepython_source(
        "class Counter:\n    count: int\n\ndef bump(c: Counter) -> None:\n    c.count += \"x\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("produces `str` where member `count` expects `int`"));
}

#[test]
fn check_reports_classvar_outside_class_scope_typepython_module() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.tpy"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::TypePython,
            declarations: vec![Declaration {
                name: String::from("LIMIT"),
                kind: DeclarationKind::Value,
                detail: String::new(),
                value_type: None,
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
                is_class_var: true,
                bases: Vec::new(),
                type_params: Vec::new(),
            }],
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

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("ClassVar binding `LIMIT`"));
}

#[test]
fn check_reports_getter_only_property_augmented_assignment_not_writable() {
    let result = check_temp_typepython_source(
        "class Gauge:\n    @property\n    def level(self) -> int:\n        return 0\n\ndef adjust(g: Gauge) -> None:\n    g.level += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("property `level` on `Gauge`"));
    assert!(rendered.contains("is not writable"));
}

#[test]
fn check_accepts_enum_exhaustive_match() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.tpy"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("Enum"),
                    kind: DeclarationKind::Import,
                    detail: String::from("enum.Enum"),
                    value_type: None,
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
                    name: String::from("Status"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Enum")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("OPEN"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Status"),
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
                Declaration {
                    name: String::from("CLOSED"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Status"),
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
                Declaration {
                    name: String::from("PENDING"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Status"),
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
                Declaration {
                    name: String::from("handle"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(s:Status)->None"),
                    value_type: None,
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
            matches: vec![typepython_binding::MatchSite {
                owner_name: Some(String::from("handle")),
                owner_type_name: None,
                subject_type: Some(String::new()),
                subject_is_awaited: false,
                subject_callee: None,
                subject_name: Some(String::from("s")),
                subject_member_owner_name: None,
                subject_member_name: None,
                subject_member_through_instance: false,
                subject_method_owner_name: None,
                subject_method_name: None,
                subject_method_through_instance: false,
                cases: vec![
                    typepython_binding::MatchCaseSite {
                        patterns: vec![typepython_binding::MatchPatternSite::Literal(
                            String::from("Status.OPEN"),
                        )],
                        has_guard: false,
                        line: 3,
                    },
                    typepython_binding::MatchCaseSite {
                        patterns: vec![typepython_binding::MatchPatternSite::Literal(
                            String::from("Status.CLOSED"),
                        )],
                        has_guard: false,
                        line: 4,
                    },
                    typepython_binding::MatchCaseSite {
                        patterns: vec![typepython_binding::MatchPatternSite::Literal(
                            String::from("Status.PENDING"),
                        )],
                        has_guard: false,
                        line: 5,
                    },
                ],
                line: 2,
            }],
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_non_exhaustive_enum_match_missing_member() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.tpy"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("Enum"),
                    kind: DeclarationKind::Import,
                    detail: String::from("enum.Enum"),
                    value_type: None,
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
                    name: String::from("Priority"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    bases: vec![String::from("Enum")],
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("LOW"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Priority"),
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
                Declaration {
                    name: String::from("MEDIUM"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Priority"),
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
                Declaration {
                    name: String::from("HIGH"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Priority"),
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
                Declaration {
                    name: String::from("triage"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(p:Priority)->None"),
                    value_type: None,
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
            matches: vec![typepython_binding::MatchSite {
                owner_name: Some(String::from("triage")),
                owner_type_name: None,
                subject_type: Some(String::new()),
                subject_is_awaited: false,
                subject_callee: None,
                subject_name: Some(String::from("p")),
                subject_member_owner_name: None,
                subject_member_name: None,
                subject_member_through_instance: false,
                subject_method_owner_name: None,
                subject_method_name: None,
                subject_method_through_instance: false,
                cases: vec![
                    typepython_binding::MatchCaseSite {
                        patterns: vec![typepython_binding::MatchPatternSite::Literal(
                            String::from("Priority.LOW"),
                        )],
                        has_guard: false,
                        line: 3,
                    },
                    typepython_binding::MatchCaseSite {
                        patterns: vec![typepython_binding::MatchPatternSite::Literal(
                            String::from("Priority.MEDIUM"),
                        )],
                        has_guard: false,
                        line: 4,
                    },
                ],
                line: 2,
            }],
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4009"));
    assert!(rendered.contains("missing members: HIGH"));
}

#[test]
fn check_reports_list_covariance_assignment_mismatch() {
    let result = check_temp_typepython_source(
        "class Animal:\n    pass\n\nclass Cat(Animal):\n    pass\n\ndef take(cats: list[Cat]) -> None:\n    xs: list[Animal] = cats\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
}

#[test]
fn check_accepts_sequence_covariance_assignment_from_list_subtype() {
    let node = type_relation_node_with_base_child();

    assert!(crate::direct_type_is_assignable(&node, &[], "Sequence[Base]", "list[Child]",));
}

#[test]
fn check_reports_for_loop_tuple_target_arity_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.tpy"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("items"),
                    kind: DeclarationKind::Value,
                    detail: String::from("list[tuple[int, str, bool]]"),
                    value_type: None,
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
                    name: String::from("process"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->None"),
                    value_type: None,
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
            for_loops: vec![typepython_binding::ForSite {
                target_name: String::new(),
                target_names: vec![String::from("a"), String::from("b")],
                owner_name: Some(String::from("process")),
                owner_type_name: None,
                iter_type: Some(String::new()),
                iter_is_awaited: false,
                iter_callee: None,
                iter_name: Some(String::from("items")),
                iter_member_owner_name: None,
                iter_member_name: None,
                iter_member_through_instance: false,
                iter_method_owner_name: None,
                iter_method_name: None,
                iter_method_through_instance: false,
                line: 2,
            }],
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("2 name(s)"));
    assert!(rendered.contains("3 element(s)"));
}

#[test]
fn check_reports_except_handler_binding_return_type_mismatch() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/module.py"),
            module_key: String::from("app.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("run"),
                kind: DeclarationKind::Function,
                detail: String::from("()->str"),
                value_type: None,
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
            member_accesses: Vec::new(),
            returns: vec![typepython_binding::ReturnSite {
                owner_name: String::from("run"),
                owner_type_name: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("exc")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                line: 4,
            }],
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: vec![typepython_binding::ExceptHandlerSite {
                exception_type: String::from("ValueError"),
                binding_name: Some(String::from("exc")),
                owner_name: Some(String::from("run")),
                owner_type_name: None,
                line: 3,
                end_line: 5,
            }],
            assignments: Vec::new(),
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("ValueError"));
}

#[test]
fn check_reports_direct_constructor_keyword_type_mismatch() {
    let result = check_temp_typepython_source(
        "class User:\n    def __init__(self, name: str, age: int):\n        self.name = name\n        self.age = age\n\nUser(name=\"Ada\", age=\"oops\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("keyword `age`"));
    assert!(rendered.contains("expects `int`"));
}

#[test]
fn check_reports_unresolved_import_with_fallback_unknown() {
    let result = check_with_options(
        &ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.tpy"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::TypePython,
                declarations: vec![Declaration {
                    name: String::from("remote"),
                    kind: DeclarationKind::Import,
                    detail: String::from("pkg.missing.remote"),
                    value_type: None,
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
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("remote.execute"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 2,
                }],
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
            }],
        },
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"));
    assert!(rendered.contains("remote.execute"));
}

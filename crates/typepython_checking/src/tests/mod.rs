pub(super) use super::{
    check, check_with_binding_metadata, check_with_options, check_with_source_overrides,
    semantic_incremental_state_with_binding_metadata,
    semantic_incremental_state_with_reused_summaries,
};
pub(super) use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::ErrorKind,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
pub(super) use typepython_binding::{
    Declaration, DeclarationKind, DeclarationOwner, DeclarationOwnerKind, bind,
};
pub(super) use typepython_config::{DiagnosticLevel, ImportFallback};
pub(super) use typepython_graph::{ModuleGraph, ModuleNode, build};
pub(super) use typepython_syntax::{ParseOptions, SourceFile, SourceKind, parse_with_options};

pub(super) static TEMP_SOURCE_ROOT_ID: AtomicU64 = AtomicU64::new(0);

pub(super) fn create_temp_typepython_root() -> PathBuf {
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

pub(super) fn check_temp_typepython_source(source_text: &str) -> crate::CheckResult {
    check_temp_typepython_source_with_options(source_text, ParseOptions::default())
}

pub(super) fn check_temp_typepython_source_with_options(
    source_text: &str,
    options: ParseOptions,
) -> crate::CheckResult {
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

pub(super) fn check_temp_typepython_source_with_check_options(
    source_text: &str,
    options: ParseOptions,
    require_explicit_overrides: bool,
    enable_sealed_exhaustiveness: bool,
    report_deprecated: DiagnosticLevel,
    strict: bool,
    warn_unsafe: bool,
) -> crate::CheckResult {
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

#[test]
fn semantic_incremental_state_reuses_unchanged_public_summaries() {
    let root = create_temp_typepython_root();
    let a_path = root.join("a.tpy");
    let b_path = root.join("b.tpy");
    fs::write(&a_path, "def produce() -> int:\n    return 1\n")
        .expect("temp source should be written");
    fs::write(&b_path, "def helper() -> int:\n    return 2\n")
        .expect("temp source should be written");

    let trees = [
        parse_with_options(
            SourceFile {
                path: a_path,
                kind: SourceKind::TypePython,
                logical_module: String::from("app.a"),
                text: String::from("def produce() -> int:\n    return 1\n"),
            },
            ParseOptions::default(),
        ),
        parse_with_options(
            SourceFile {
                path: b_path,
                kind: SourceKind::TypePython,
                logical_module: String::from("app.b"),
                text: String::from("def helper() -> int:\n    return 2\n"),
            },
            ParseOptions::default(),
        ),
    ];
    let bindings = trees.iter().map(bind).collect::<Vec<_>>();
    let graph = build(&bindings);
    let baseline = semantic_incremental_state_with_binding_metadata(
        &graph,
        &bindings,
        ImportFallback::Unknown,
        None,
        None,
        typepython_incremental::SnapshotMetadata::default(),
    );
    let mut previous_summaries = baseline.summaries.clone();
    let sentinel_summary = {
        let summary = previous_summaries
            .iter_mut()
            .find(|summary| summary.module == "app.b")
            .expect("baseline should contain app.b summary");
        summary.exports[0].type_repr = String::from("sentinel");
        summary.clone()
    };
    let changed_sentinel = {
        let summary = previous_summaries
            .iter_mut()
            .find(|summary| summary.module == "app.a")
            .expect("baseline should contain app.a summary");
        summary.exports[0].type_repr = String::from("stale");
        summary.clone()
    };

    let rebuilt = semantic_incremental_state_with_reused_summaries(
        &graph,
        &bindings,
        ImportFallback::Unknown,
        None,
        &previous_summaries,
        &BTreeSet::from([String::from("app.a")]),
        None,
        typepython_incremental::SnapshotMetadata::default(),
    );

    let reused_b = rebuilt
        .summaries
        .iter()
        .find(|summary| summary.module == "app.b")
        .cloned()
        .expect("rebuilt summaries should contain app.b");
    let refreshed_a = rebuilt
        .summaries
        .iter()
        .find(|summary| summary.module == "app.a")
        .cloned()
        .expect("rebuilt summaries should contain app.a");

    assert_eq!(reused_b, sentinel_summary);
    assert_ne!(refreshed_a, changed_sentinel);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn semantic_incremental_summary_preserves_native_runtime_feature_markers() {
    let root = create_temp_typepython_root();
    let path = root.join("app.py");
    let source_text = "type Pair[T = int] = tuple[T, T]\n\nclass Box[T = int]:\n    value: T\n\ndef first[T = int](value: T = 1) -> T:\n    return value\n";
    fs::write(&path, source_text).expect("temp source should be written");

    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::Python,
            logical_module: String::from("app"),
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
    .find(|summary| summary.module == "app")
    .expect("summary should exist");

    let pair = summary
        .exports
        .iter()
        .find(|export| export.name == "Pair")
        .expect("Pair export should exist");
    let box_export = summary
        .exports
        .iter()
        .find(|export| export.name == "Box")
        .expect("Box export should exist");
    let first = summary
        .exports
        .iter()
        .find(|export| export.name == "first")
        .expect("first export should exist");

    assert!(pair.required_runtime_features.contains(&String::from("type_stmt")));
    assert!(pair.required_runtime_features.contains(&String::from("inline_type_params")));
    assert!(pair.required_runtime_features.contains(&String::from("generic_defaults")));
    assert_eq!(
        pair.runtime_semantics.as_ref().map(|semantics| semantics.form),
        Some(typepython_target::RuntimeTypingForm::TypeAliasType)
    );
    assert!(box_export.required_runtime_features.contains(&String::from("inline_type_params")));
    assert!(box_export.required_runtime_features.contains(&String::from("generic_defaults")));
    assert_eq!(
        box_export.runtime_semantics.as_ref().map(|semantics| semantics.form),
        Some(typepython_target::RuntimeTypingForm::NativeGenericClass)
    );
    assert!(first.required_runtime_features.contains(&String::from("inline_type_params")));
    assert!(first.required_runtime_features.contains(&String::from("generic_defaults")));
    assert_eq!(
        first.runtime_semantics.as_ref().map(|semantics| semantics.form),
        Some(typepython_target::RuntimeTypingForm::NativeGenericFunction)
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn semantic_incremental_summary_marks_runtime_sensitive_typing_surface() {
    let root = create_temp_typepython_root();
    let path = root.join("app.py");
    let source_text = "from typing import NoDefault, ReadOnly, TypeIs\n\ntype Wrapped[T = int] = ReadOnly[T]\nmarker: NoDefault\n\ndef accepts(value: object) -> TypeIs[int]:\n    return isinstance(value, int)\n";
    fs::write(&path, source_text).expect("temp source should be written");

    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::Python,
            logical_module: String::from("app"),
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
    .find(|summary| summary.module == "app")
    .expect("summary should exist");

    let wrapped = summary
        .exports
        .iter()
        .find(|export| export.name == "Wrapped")
        .expect("Wrapped export should exist");
    let marker = summary
        .exports
        .iter()
        .find(|export| export.name == "marker")
        .expect("marker export should exist");
    let accepts = summary
        .exports
        .iter()
        .find(|export| export.name == "accepts")
        .expect("accepts export should exist");

    assert!(wrapped.required_runtime_features.contains(&String::from("typing_readonly")));
    assert!(marker.required_runtime_features.contains(&String::from("typing_no_default")));
    assert!(accepts.required_runtime_features.contains(&String::from("typing_type_is")));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn semantic_incremental_summary_prefers_structured_export_and_fact_types() {
    let list_of_int = typepython_syntax::TypeExpr::Generic {
        head: String::from("list"),
        args: vec![typepython_syntax::TypeExpr::Name(String::from("int"))],
    };
    let tuple_of_int = typepython_syntax::TypeExpr::Generic {
        head: String::from("tuple"),
        args: vec![typepython_syntax::TypeExpr::Name(String::from("int"))],
    };
    let maybe_int = typepython_syntax::TypeExpr::Union {
        branches: vec![
            typepython_syntax::TypeExpr::Name(String::from("int")),
            typepython_syntax::TypeExpr::Name(String::from("None")),
        ],
        style: typepython_syntax::UnionStyle::Shorthand,
    };
    let graph = ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("<summary-app>"),
            module_key: String::from("app"),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    metadata: typepython_binding::DeclarationMetadata::Value {
                        annotation: Some(typepython_binding::BoundTypeExpr::from_expr(
                            list_of_int.clone(),
                        )),
                    },
                    name: String::from("items"),
                    kind: DeclarationKind::Value,
                    legacy_detail: String::from("str"),
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
                    metadata: typepython_binding::DeclarationMetadata::TypeAlias {
                        value: typepython_binding::BoundTypeExpr::from_expr(maybe_int.clone()),
                    },
                    name: String::from("MaybeInt"),
                    kind: DeclarationKind::TypeAlias,
                    legacy_detail: String::from("str"),
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
                    metadata: typepython_binding::DeclarationMetadata::Callable {
                        signature: typepython_binding::BoundCallableSignature {
                            params: vec![typepython_syntax::DirectFunctionParamSite {
                                name: String::from("value"),
                                annotation: Some(String::from("list[int]")),
                                annotation_expr: Some(list_of_int),
                                has_default: false,
                                positional_only: false,
                                keyword_only: false,
                                variadic: false,
                                keyword_variadic: false,
                            }],
                            returns: Some(typepython_binding::BoundTypeExpr::from_expr(
                                tuple_of_int,
                            )),
                        },
                    },
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    legacy_detail: String::from("(value:str)->str"),
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
            summary_fingerprint: 0,
        }],
    };

    let summary = semantic_incremental_state_with_binding_metadata(
        &graph,
        &[],
        ImportFallback::Unknown,
        None,
        None,
        typepython_incremental::SnapshotMetadata::default(),
    )
    .summaries
    .into_iter()
    .find(|summary| summary.module == "app")
    .expect("summary should exist");

    let items = summary
        .exports
        .iter()
        .find(|export| export.name == "items")
        .expect("items export should exist");
    let maybe_int_export = summary
        .exports
        .iter()
        .find(|export| export.name == "MaybeInt")
        .expect("MaybeInt export should exist");
    let build = summary
        .exports
        .iter()
        .find(|export| export.name == "build")
        .expect("build export should exist");

    assert_eq!(
        items.type_expr.as_ref().map(typepython_syntax::TypeExpr::render),
        Some(String::from("list[int]")),
    );
    assert_eq!(items.exported_type.as_deref(), Some("list[int]"));
    assert_eq!(
        maybe_int_export.type_expr.as_ref().map(typepython_syntax::TypeExpr::render),
        Some(String::from("Union[int, None]")),
    );
    assert_eq!(
        build.type_expr.as_ref().map(typepython_syntax::TypeExpr::render),
        Some(String::from("Callable[[list[int]], tuple[int]]")),
    );
    assert_eq!(build.exported_type.as_deref(), Some("Callable[[list[int]], tuple[int]]"));
    assert_eq!(
        build.exported_type_expr.as_ref().map(typepython_syntax::TypeExpr::render),
        Some(String::from("tuple[int]")),
    );

    let item_fact = summary
        .solver_facts
        .declaration_facts
        .iter()
        .find(|fact| fact.name == "items")
        .expect("items declaration fact should exist");
    let alias_fact = summary
        .solver_facts
        .declaration_facts
        .iter()
        .find(|fact| fact.name == "MaybeInt")
        .expect("MaybeInt declaration fact should exist");

    assert_eq!(item_fact.type_expr.as_deref(), Some("list[int]"));
    assert_eq!(
        item_fact.type_expr_structured.as_ref().map(typepython_syntax::TypeExpr::render),
        Some(String::from("list[int]")),
    );
    assert_eq!(alias_fact.type_expr.as_deref(), Some("Union[int, None]"));
    assert_eq!(
        alias_fact.type_expr_structured.as_ref().map(typepython_syntax::TypeExpr::render),
        Some(String::from("Union[int, None]")),
    );
}

#[test]
fn check_propagates_generic_owner_arguments_into_member_reads() {
    let diagnostics = check_temp_typepython_source(
        "class Box[T]:\n    value: T\n\ndef read(box: Box[int]) -> int:\n    return box.value\n",
    )
    .diagnostics;

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
}

#[test]
fn check_propagates_generic_owner_arguments_into_method_returns() {
    let diagnostics = check_temp_typepython_source(
        "class Box[T]:\n    value: T\n    def get(self) -> T:\n        return self.value\n\ndef read(box: Box[int]) -> int:\n    return box.get()\n",
    )
    .diagnostics;

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
}

#[test]
fn resolve_method_call_candidate_instantiates_owner_generic_arguments() {
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    let source_text = "class Box[T]:\n    value: T\n    def get(self) -> T:\n        return self.value\n\ndef read(box: Box[int]) -> int:\n    return box.get()\n";
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
    assert_eq!(node.method_calls.len(), 1, "expected a single nested method call site");
    assert_eq!(node.method_calls[0].current_owner_name.as_deref(), Some("read"));
    assert_eq!(node.method_calls[0].current_owner_type_name.as_deref(), None);
    let context = super::CheckerContext::new(&graph.nodes, ImportFallback::Unknown, None);
    let resolved_receiver = super::resolve_direct_name_reference_semantic_type_with_context(
        &context,
        node,
        &graph.nodes,
        None,
        None,
        node.method_calls[0].current_owner_name.as_deref(),
        node.method_calls[0].current_owner_type_name.as_deref(),
        node.method_calls[0].line,
        &node.method_calls[0].owner_name,
    );
    assert_eq!(
        resolved_receiver.as_ref().map(crate::render_semantic_type),
        Some(String::from("Box[int]")),
    );
    assert!(
        !super::name_is_unknown_boundary_with_context(
            &context,
            node,
            &graph.nodes,
            node.method_calls[0].current_owner_name.as_deref(),
            node.method_calls[0].current_owner_type_name.as_deref(),
            node.method_calls[0].line,
            &node.method_calls[0].owner_name,
        ),
        "method receiver should not be treated as an unknown boundary",
    );
    let class_decl = node
        .declarations
        .iter()
        .find(|declaration| declaration.name == "Box" && declaration.kind == DeclarationKind::Class)
        .expect("Box class should be present");
    let method = node
        .declarations
        .iter()
        .find(|declaration| {
            declaration.name == "get"
                && declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
        })
        .expect("get method should be present");
    let direct_call = typepython_binding::CallSite {
        callee: String::from("Box.get"),
        arg_count: 0,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };

    let resolved = super::resolve_method_call_candidate_detailed(
        node,
        &graph.nodes,
        method,
        &direct_call,
        &crate::lower_type_text_or_name("Box[int]"),
        super::declaration_callable_semantics(method).as_ref(),
    )
    .expect("generic method call should resolve");

    assert_eq!(
        resolved.return_type.map(|ty| crate::render_semantic_type(&ty)),
        Some(String::from("int"))
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn instantiated_generic_return_prefers_wider_assignable_type_over_union() {
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    let source_text = concat!(
        "class Animal:\n    pass\n\n",
        "class Cat(Animal):\n    pass\n\n",
        "def choose[T](first: T, second: T) -> T:\n    return first\n",
    );
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
    let function = node
        .declarations
        .iter()
        .find(|declaration| {
            declaration.name == "choose" && declaration.kind == DeclarationKind::Function
        })
        .expect("choose function should be present");
    let call = typepython_binding::CallSite {
        callee: String::from("choose"),
        arg_count: 2,
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(vec![
            String::from("Animal"),
            String::from("Cat"),
        ]),
        starred_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_names: Vec::new(),
        keyword_arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(Vec::new()),
        keyword_expansion_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(
            Vec::new(),
        ),
        line: 1,
    };

    let instantiated_return = super::resolve_instantiated_callable_return_type_from_declaration(
        node,
        &graph.nodes,
        function,
        &call,
    )
    .expect("instantiated return type");

    assert_eq!(instantiated_return, "Animal");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_resolves_imports_inside_type_checking_guards() {
    let root = create_temp_typepython_root();
    let main_path = root.join("main.tpy");
    let models_path = root.join("models.tpy");
    fs::write(
        &main_path,
        "import typing\nif typing.TYPE_CHECKING:\n    from app.models import User\n\ndef take(user: User) -> User:\n    return user\n",
    )
    .expect("temp source should be written");
    fs::write(&models_path, "class User:\n    pass\n").expect("temp source should be written");

    let trees = [
        parse_with_options(
            SourceFile {
                path: main_path,
                kind: SourceKind::TypePython,
                logical_module: String::from("app.main"),
                text: String::from(
                    "import typing\nif typing.TYPE_CHECKING:\n    from app.models import User\n\ndef take(user: User) -> User:\n    return user\n",
                ),
            },
            ParseOptions::default(),
        ),
        parse_with_options(
            SourceFile {
                path: models_path,
                kind: SourceKind::TypePython,
                logical_module: String::from("app.models"),
                text: String::from("class User:\n    pass\n"),
            },
            ParseOptions::default(),
        ),
    ];
    let bindings = trees.iter().map(bind).collect::<Vec<_>>();
    let graph = build(&bindings);
    let diagnostics = check_with_options(
        &graph,
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
    )
    .diagnostics;

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_accepts_direct_type_checking_import_from_typing() {
    let diagnostics = check_temp_typepython_source(
        "from typing import TYPE_CHECKING\nflag: bool = TYPE_CHECKING\n",
    )
    .diagnostics;

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
}

#[test]
fn check_resolves_imports_inside_version_guards() {
    let root = create_temp_typepython_root();
    let main_path = root.join("main.tpy");
    let models_path = root.join("models.tpy");
    fs::write(
        &main_path,
        "import sys\nif sys.version_info >= (3, 11):\n    from app.models import User\n\ndef take(user: User) -> User:\n    return user\n",
    )
    .expect("temp source should be written");
    fs::write(&models_path, "class User:\n    pass\n").expect("temp source should be written");

    let trees = [
        parse_with_options(
            SourceFile {
                path: main_path,
                kind: SourceKind::TypePython,
                logical_module: String::from("app.main"),
                text: String::from(
                    "import sys\nif sys.version_info >= (3, 11):\n    from app.models import User\n\ndef take(user: User) -> User:\n    return user\n",
                ),
            },
            ParseOptions {
                target_python: Some(typepython_syntax::ParsePythonVersion { major: 3, minor: 11 }),
                ..ParseOptions::default()
            },
        ),
        parse_with_options(
            SourceFile {
                path: models_path,
                kind: SourceKind::TypePython,
                logical_module: String::from("app.models"),
                text: String::from("class User:\n    pass\n"),
            },
            ParseOptions::default(),
        ),
    ];
    let bindings = trees.iter().map(bind).collect::<Vec<_>>();
    let graph = build(&bindings);
    let diagnostics = check_with_options(
        &graph,
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
    )
    .diagnostics;

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_resolves_class_declarations_inside_type_checking_guards() {
    let diagnostics = check_temp_typepython_source(
        "import typing\nif typing.TYPE_CHECKING:\n    class User:\n        pass\n\ndef take(user: User) -> User:\n    return user\n",
    )
    .diagnostics;

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
}

#[test]
fn check_resolves_typealiases_inside_type_checking_guards() {
    let diagnostics = check_temp_typepython_source(
        "import typing\nif typing.TYPE_CHECKING:\n    typealias UserId = int\n\ndef take(user: UserId) -> UserId:\n    return user\nvalue: int = take(1)\n",
    )
    .diagnostics;

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
}

pub(super) fn check_temp_project_sources(
    sources: &[(&str, &str, SourceKind, &str)],
) -> crate::CheckResult {
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

pub(super) fn check_virtual_binding_metadata_source(source_text: &str) -> crate::CheckResult {
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

pub(super) fn check_virtual_source_with_overrides(
    source_text: &str,
    options: ParseOptions,
    strict: bool,
    warn_unsafe: bool,
) -> crate::CheckResult {
    let path = PathBuf::from("virtual/app.tpy");
    let source = SourceFile {
        path: path.clone(),
        kind: SourceKind::TypePython,
        logical_module: String::from("app"),
        text: source_text.to_owned(),
    };
    let tree = parse_with_options(source, options);
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let source_overrides = BTreeMap::from([(path.display().to_string(), source_text.to_owned())]);

    check_with_source_overrides(
        &graph,
        false,
        true,
        DiagnosticLevel::Warning,
        strict,
        warn_unsafe,
        ImportFallback::Unknown,
        Some(&source_overrides),
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

pub(super) fn type_relation_node_with_base_child() -> ModuleNode {
    ModuleNode {
        module_path: PathBuf::from("<type-relations>"),
        module_key: String::new(),
        module_kind: SourceKind::TypePython,
        declarations: vec![
            Declaration {
                name: String::from("Base"),
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
                name: String::from("Child"),
                kind: DeclarationKind::Class,
                metadata: Default::default(),
                legacy_detail: String::from("Base"),
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
fn surface_direct_method_signatures_trim_implicit_parameters() {
    let metadata = typepython_syntax::ModuleSurfaceMetadata {
        direct_method_signatures: vec![
            typepython_syntax::DirectMethodSignatureSite {
                owner_type_name: String::from("Widget"),
                name: String::from("instance_method"),
                method_kind: typepython_syntax::MethodKind::Instance,
                params: vec![
                    typepython_syntax::DirectFunctionParamSite {
                        name: String::from("self"),
                        annotation: None,
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    },
                    typepython_syntax::DirectFunctionParamSite {
                        name: String::from("value"),
                        annotation: Some(String::from("int")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    },
                ],
                line: 1,
            },
            typepython_syntax::DirectMethodSignatureSite {
                owner_type_name: String::from("Widget"),
                name: String::from("factory"),
                method_kind: typepython_syntax::MethodKind::Static,
                params: vec![typepython_syntax::DirectFunctionParamSite {
                    name: String::from("value"),
                    annotation: Some(String::from("int")),
                    annotation_expr: None,
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                }],
                line: 2,
            },
        ],
        ..Default::default()
    };

    let signatures = super::source_facts::surface_direct_method_signatures(&metadata);

    assert_eq!(
        signatures
            .get(&(String::from("Widget"), String::from("instance_method")))
            .expect("instance method signature should be collected")
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        vec!["value"]
    );
    assert_eq!(
        signatures
            .get(&(String::from("Widget"), String::from("factory")))
            .expect("static method signature should be collected")
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        vec!["value"]
    );
}

fn collect_rs_files(root: &PathBuf, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(root).expect("source directory should be readable");
    for entry in entries {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("tests") {
                continue;
            }
            collect_rs_files(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

#[test]
fn production_semantic_paths_do_not_read_legacy_detail_directly() {
    let checking_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let incremental_root = checking_root
        .parent()
        .expect("checking crate should live under crates")
        .join("typepython_incremental");
    let mut files = Vec::new();
    collect_rs_files(&checking_root.join("src"), &mut files);
    collect_rs_files(&incremental_root.join("src"), &mut files);

    let offenders = files
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path).expect("source file should be readable");
            let matches = contents
                .lines()
                .enumerate()
                .filter_map(|(index, line)| {
                    line.contains(".legacy_detail")
                        .then(|| format!("{}:{}", path.display(), index + 1))
                })
                .collect::<Vec<_>>();
            (!matches.is_empty()).then_some(matches)
        })
        .flatten()
        .collect::<Vec<_>>();

    assert!(
        offenders.is_empty(),
        "checking/incremental production code should use structured accessors instead of \
         `.legacy_detail`: {offenders:?}"
    );
}

mod advanced;
mod advanced_generics;
mod calls;
mod property_based;
mod semantic;
mod typed_dict;

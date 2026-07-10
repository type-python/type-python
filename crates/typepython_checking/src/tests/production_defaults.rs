use super::*;

fn check_with_production_defaults(source_text: &str) -> crate::CheckResult {
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
    let result = crate::check(&normalize_test_graph(&graph));

    let _ = fs::remove_dir_all(root);
    result
}

#[test]
fn production_entrypoint_enforces_no_implicit_dynamic() {
    let source = "def parse(value) -> int:\n    return value\n";

    let production = check_with_production_defaults(source);
    let permissive = check_temp_typepython_source_with_checker_options(
        source,
        ParseOptions::default(),
        crate::CheckerOptions::permissive_test_default(),
    );

    assert!(production.diagnostics.as_text().contains("TPY4029"));
    assert!(!permissive.diagnostics.as_text().contains("TPY4029"));
}

#[test]
fn production_entrypoint_enforces_strict_unsafe_boundaries() {
    let source = "def evaluate(expression: str) -> object:\n    return eval(expression)\n";

    let production = check_with_production_defaults(source);
    let permissive = check_temp_typepython_source_with_checker_options(
        source,
        ParseOptions::default(),
        crate::CheckerOptions::permissive_test_default(),
    );

    assert!(production.diagnostics.as_text().contains("TPY4019"));
    assert!(!permissive.diagnostics.as_text().contains("TPY4019"));
}

#[test]
fn production_entrypoint_matches_all_structured_default_constructors() {
    let source = concat!(
        "def parse(value) -> int:\n",
        "    return value\n\n",
        "def evaluate(expression: str) -> object:\n",
        "    return eval(expression)\n",
    );
    let root = create_temp_typepython_root();
    let path = root.join("app.tpy");
    fs::write(&path, source).expect("temp source should be written");
    let tree = parse_with_options(
        SourceFile {
            path,
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source.to_owned(),
        },
        ParseOptions::default(),
    );
    let binding = bind(&tree);
    let graph = normalize_test_graph(&build(&[binding]));

    let public_default = crate::check(&graph).diagnostics;
    let explicit_default =
        crate::check_with_checker_options(&graph, crate::CheckerOptions::default()).diagnostics;
    let config_default = crate::check_with_checker_options(
        &graph,
        crate::CheckerOptions::from_typing_config(&typepython_config::TypingConfig::default()),
    )
    .diagnostics;

    let _ = fs::remove_dir_all(root);
    assert_eq!(public_default.diagnostics, explicit_default.diagnostics);
    assert_eq!(public_default.diagnostics, config_default.diagnostics);
    assert!(public_default.as_text().contains("TPY4029"));
    assert!(public_default.as_text().contains("TPY4019"));
}

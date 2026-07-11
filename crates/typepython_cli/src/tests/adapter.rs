use super::*;

const STRICT_ADAPTER_MANIFEST: &str = r#"[adapter]
name = "strict-adapter"
version = "0.1.0"
framework = "strict.framework"
typepython_min = "0.3.0"
python_targets = ["3.12"]
stability = "prototype"

[[transforms]]
provider = "strict.framework.Model"
kind = "base_class"
target = "class"
capabilities = ["field_collection", "constructor_generation", "alias_handling", "required_optional_fields", "readonly_fields"]
field_collector = "annotated_class_fields"
constructor = "fields"
alias = { source = "field_specifier", keyword = "alias", literal_only = true }
default = { keyword = "default" }
default_factory = { keyword = "default_factory" }
frozen = { model_keyword = "frozen_default", field_keyword = "frozen" }
fallback = "strict_diagnostic"

[[transforms]]
provider = "strict.framework.task"
kind = "function_to_object_decorator"
target = "function"
capabilities = ["function_to_object_replacement", "generic_preservation", "effect_time"]
replacement_type = "strict.framework.Task[P, R]"
preserve_paramspec = true
preserve_return_type = true
fallback = "strict_diagnostic"

[[golden_tests]]
name = "fixture"
input = "fixture.tpy"
expected_py = "expected.py"
expected_pyi = "expected.pyi"
checkers = ["mypy"]
"#;

#[test]
fn adapter_validate_command_parses_manifest_path() {
    let cli = Cli::parse_from([
        "typepython",
        "adapter",
        "validate",
        "typepython-framework.toml",
        "--format",
        "json",
    ]);

    let Command::Adapter(args) = cli.command else {
        panic!("expected adapter command");
    };
    let crate::cli::AdapterCommand::Validate(args) = args.command;

    assert_eq!(args.manifest, PathBuf::from("typepython-framework.toml"));
    assert_eq!(args.format, OutputFormat::Json);
}

#[test]
fn run_adapter_validate_accepts_safe_manifest() {
    let project_dir = temp_project_dir("run_adapter_validate_accepts_safe_manifest");
    let result = {
        let input = project_dir.join("fixture.tpy");
        fs::write(&input, "class User:\n    pass\n").expect("test fixture should be written");
        fs::create_dir_all(project_dir.join("expected/app")).expect("golden dir should exist");
        fs::write(project_dir.join("expected/app/__init__.py"), "class User:\n    pass\n")
            .expect("runtime golden should be written");
        fs::write(project_dir.join("expected/app/__init__.pyi"), "class User: ...\n")
            .expect("stub golden should be written");
        let manifest = project_dir.join("typepython-framework.toml");
        fs::write(
            &manifest,
            r#"[adapter]
name = "toy-pydantic"
version = "0.1.0"
framework = "toy.pydantic"
typepython_min = "0.3.0"
python_targets = ["3.12"]
stability = "prototype"

[[transforms]]
provider = "toy.pydantic.BaseModel"
kind = "base_class"
target = "class"
capabilities = ["field_collection", "constructor_generation", "alias_handling", "required_optional_fields", "readonly_fields"]
field_collector = "annotated_class_fields"
constructor = "fields"
alias = { source = "field_specifier", keyword = "validation_alias", literal_only = true }
default = { keyword = "validation_default" }
default_factory = { keyword = "validation_factory" }
frozen = { model_keyword = "model_frozen", field_keyword = "field_frozen" }
fallback = "strict_diagnostic"

[[transforms]]
provider = "toy.tasks.task"
kind = "function_to_object_decorator"
target = "function"
capabilities = ["function_to_object_replacement", "generic_preservation"]
replacement_type = "toy.tasks.Task[P, R]"
preserve_paramspec = true
preserve_return_type = true
fallback = "strict_diagnostic"

[[transforms]]
provider = "toy.pydantic.remote_call"
kind = "function_decorator"
target = "function"
capabilities = ["effect_io_net", "effect_time"]
fallback = "strict_diagnostic"

[[golden_tests]]
name = "fixture"
input = "fixture.tpy"
expected_py = "expected/app/__init__.py"
expected_pyi = "expected/app/__init__.pyi"
checkers = ["basedpyright", "mypy", "pyright", "ty"]
"#,
        )
        .expect("manifest should be written");
        run_adapter_validate(AdapterValidateArgs { manifest, format: OutputFormat::Json })
            .expect("manifest validation should run")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(result, ExitCode::SUCCESS);
}

#[test]
fn adapter_validation_rejects_invalid_compatibility_metadata() {
    let project_dir = temp_project_dir("adapter_validation_rejects_invalid_compatibility_metadata");
    fs::write(project_dir.join("fixture.tpy"), "class User:\n    pass\n")
        .expect("input fixture should be written");
    fs::create_dir_all(project_dir.join("expected"))
        .expect("expected fixture directory should be created");
    fs::write(project_dir.join("expected/runtime.py"), "class User:\n    pass\n")
        .expect("runtime fixture should be written");
    fs::write(project_dir.join("expected/runtime.pyi"), "class User: ...\n")
        .expect("stub fixture should be written");
    let manifest = "[adapter]\nname = \"broken\"\nversion = \"not-a-version\"\nframework = \"toy.framework\"\ntypepython_min = \">=0.3\"\npython_targets = [\"3.9\", \"3.12\", \"3.12\"]\nstability = \"stable\"\n\n[[transforms]]\nprovider = \"toy.framework.model\"\nkind = \"function_decorator\"\ntarget = \"function\"\ncapabilities = []\n\n[[golden_tests]]\nname = \"fixture\"\ninput = \"fixture.tpy\"\nexpected_py = \"expected/runtime.py\"\nexpected_pyi = \"expected/runtime.pyi\"\ncheckers = [\"basedpyright\"]\n";

    let rendered = adapter_validation_diagnostics(manifest, Some(&project_dir)).as_text();
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("adapter.version `not-a-version`"), "{rendered}");
    assert!(rendered.contains("adapter.typepython_min `>=0.3`"), "{rendered}");
    assert!(rendered.contains("python_targets entry `3.9` is unsupported"), "{rendered}");
    assert!(rendered.contains("duplicate target `3.12`"), "{rendered}");
    assert!(rendered.contains("adapter.stability `stable` is unsupported"), "{rendered}");
    assert!(!rendered.contains("unsupported checker `basedpyright`"), "{rendered}");
}

#[test]
fn run_adapter_validate_rejects_unsafe_manifest_capabilities() {
    let project_dir = temp_project_dir("run_adapter_validate_rejects_unsafe_manifest");
    let result = {
        let manifest = project_dir.join("typepython-framework.toml");
        fs::write(
            &manifest,
            "[adapter]\nname = \"unsafe\"\nversion = \"0.1.0\"\nframework = \"toy.unsafe\"\ntypepython_min = \"0.3.0\"\npython_targets = [\"3.12\"]\nstability = \"prototype\"\n\n[[transforms]]\nprovider = \"toy.unsafe.Provider\"\nkind = \"runtime_python\"\ntarget = \"class\"\ncapabilities = [\"exec_python\"]\n",
        )
        .expect("manifest should be written");
        run_adapter_validate(AdapterValidateArgs { manifest, format: OutputFormat::Text })
            .expect("manifest validation should run")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(
        result,
        ExitCode::FAILURE,
        "TPY7003 adapter manifest diagnostics should reject unsafe capabilities",
    );
}

#[test]
fn adapter_manifest_schema_rejects_unknown_fields_at_every_level() {
    let cases = [
        (
            "manifest root",
            STRICT_ADAPTER_MANIFEST.replacen("[adapter]", "root_typo = true\n\n[adapter]", 1),
            "root_typo",
        ),
        (
            "adapter metadata",
            STRICT_ADAPTER_MANIFEST.replacen(
                "stability = \"prototype\"",
                "stability = \"prototype\"\nframework_verison = \"1\"",
                1,
            ),
            "framework_verison",
        ),
        (
            "transform",
            STRICT_ADAPTER_MANIFEST.replacen(
                "provider = \"strict.framework.Model\"",
                "provider = \"strict.framework.Model\"\nprovidre = \"typo\"",
                1,
            ),
            "providre",
        ),
        (
            "alias mapping",
            STRICT_ADAPTER_MANIFEST.replacen(
                "literal_only = true }",
                "literal_only = true, litteral_only = true }",
                1,
            ),
            "litteral_only",
        ),
        (
            "keyword mapping",
            STRICT_ADAPTER_MANIFEST.replacen(
                "default = { keyword = \"default\" }",
                "default = { keyword = \"default\", source = \"runtime\" }",
                1,
            ),
            "source",
        ),
        (
            "frozen mapping",
            STRICT_ADAPTER_MANIFEST.replacen(
                "field_keyword = \"frozen\" }",
                "field_keyword = \"frozen\", dynamic = true }",
                1,
            ),
            "dynamic",
        ),
        (
            "golden test",
            STRICT_ADAPTER_MANIFEST.replacen(
                "checkers = [\"mypy\"]",
                "checkers = [\"mypy\"]\nchecker = \"pyright\"",
                1,
            ),
            "checker",
        ),
    ];

    for (label, manifest, unknown_field) in cases {
        let diagnostics = crate::adapter::adapter_manifest_schema_diagnostics(manifest.as_str());
        let rendered = diagnostics.as_text();
        assert!(diagnostics.has_errors(), "{label} should fail: {rendered}");
        assert!(rendered.contains("TPY7003"), "{label}: {rendered}");
        assert!(rendered.contains("invalid TOML or schema"), "{label}: {rendered}");
        assert!(rendered.contains(unknown_field), "{label}: {rendered}");
    }
}

#[test]
fn adapter_manifest_validates_documented_field_relationships() {
    let cases = [
        (
            "dynamic alias",
            STRICT_ADAPTER_MANIFEST.replacen("literal_only = true", "literal_only = false", 1),
            "alias.literal_only must be true",
        ),
        (
            "invalid keyword",
            STRICT_ADAPTER_MANIFEST.replacen(
                "keyword = \"default_factory\"",
                "keyword = \"not-a-keyword\"",
                1,
            ),
            "is not a valid Python identifier",
        ),
        (
            "missing alias capability",
            STRICT_ADAPTER_MANIFEST.replacen("\"alias_handling\", ", "", 1),
            "field `alias` requires capability `alias_handling`",
        ),
        (
            "missing replacement type",
            STRICT_ADAPTER_MANIFEST.replacen(
                "replacement_type = \"strict.framework.Task[P, R]\"\n",
                "",
                1,
            ),
            "must declare a replacement_type",
        ),
        (
            "invalid replacement type",
            STRICT_ADAPTER_MANIFEST.replacen(
                "strict.framework.Task[P, R]",
                "strict.framework.Task[P,",
                1,
            ),
            "is not a valid TypePython type expression",
        ),
        (
            "incomplete generic preservation",
            STRICT_ADAPTER_MANIFEST.replacen(
                "preserve_paramspec = true",
                "preserve_paramspec = false",
                1,
            ),
            "generic_preservation` requires preserve_paramspec = true",
        ),
        (
            "class target mismatch",
            STRICT_ADAPTER_MANIFEST.replacen("target = \"class\"", "target = \"function\"", 1),
            "requires target `class`",
        ),
        (
            "class capability on function transform",
            STRICT_ADAPTER_MANIFEST.replacen(
                "\"function_to_object_replacement\", \"generic_preservation\"",
                "\"function_to_object_replacement\", \"generic_preservation\", \"field_collection\"",
                1,
            ),
            "capability `field_collection` requires a class transform kind",
        ),
        (
            "missing class collector",
            STRICT_ADAPTER_MANIFEST.replacen(
                "field_collector = \"annotated_class_fields\"\n",
                "",
                1,
            ),
            "must declare field_collector `annotated_class_fields`",
        ),
    ];

    for (label, manifest, expected) in cases {
        let rendered = adapter_validation_diagnostics(&manifest, None).as_text();
        assert!(rendered.contains(expected), "{label}: {rendered}");
    }
}

#[test]
fn malformed_adapter_toml_is_a_structured_validation_failure() {
    let malformed = "[adapter\nname = \"broken\"\n";
    let diagnostics = crate::adapter::adapter_manifest_schema_diagnostics(malformed);
    let rendered = diagnostics.as_text();
    assert!(diagnostics.has_errors(), "{rendered}");
    assert!(rendered.contains("TPY7003"), "{rendered}");
    assert!(rendered.contains("invalid TOML or schema"), "{rendered}");

    let project_dir = temp_project_dir("malformed_adapter_toml_is_structured");
    let manifest = project_dir.join("typepython-framework.toml");
    fs::write(&manifest, malformed).expect("malformed manifest should be written");
    let result = run_adapter_validate(AdapterValidateArgs { manifest, format: OutputFormat::Json })
        .expect("schema errors should render as validation diagnostics");
    assert_eq!(result, ExitCode::FAILURE);

    let invalid_utf8 = project_dir.join("invalid-utf8.toml");
    fs::write(&invalid_utf8, [0xff, 0xfe]).expect("invalid UTF-8 manifest should be written");
    let invalid_utf8_result = run_adapter_validate(AdapterValidateArgs {
        manifest: invalid_utf8,
        format: OutputFormat::Text,
    })
    .expect("invalid UTF-8 should render as a validation diagnostic");
    remove_temp_project_dir(&project_dir);

    assert_eq!(invalid_utf8_result, ExitCode::FAILURE);
}

#[test]
fn documented_adapter_toml_examples_match_the_strict_schema() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for relative_path in [
        "docs/rfcs/framework-adapter-manifest.md",
        "docs/examples/framework-adapters.md",
        "docs/framework-adapters.md",
    ] {
        let contents = fs::read_to_string(repo_root.join(relative_path))
            .expect("adapter documentation should be readable");
        let snippets = contents
            .split("```toml")
            .skip(1)
            .filter_map(|tail| tail.split("```").next())
            .collect::<Vec<_>>();
        assert!(!snippets.is_empty(), "{relative_path} should contain a TOML example");
        for (index, snippet) in snippets.into_iter().enumerate() {
            let diagnostics = crate::adapter::adapter_manifest_schema_diagnostics(snippet);
            assert!(
                diagnostics.is_empty(),
                "{relative_path} TOML example {} violates the adapter schema: {}",
                index + 1,
                diagnostics.as_text()
            );
        }
    }
}

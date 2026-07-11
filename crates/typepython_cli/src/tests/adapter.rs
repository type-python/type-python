use super::*;

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
            "[adapter]\nname = \"toy-pydantic\"\nversion = \"0.1.0\"\nframework = \"toy.pydantic\"\ntypepython_min = \"0.3.0\"\npython_targets = [\"3.12\"]\nstability = \"prototype\"\n\n[[transforms]]\nprovider = \"toy.pydantic.BaseModel\"\nkind = \"base_class\"\ntarget = \"class\"\ncapabilities = [\"field_collection\", \"constructor_generation\", \"alias_handling\", \"required_optional_fields\"]\nfield_collector = \"annotated_class_fields\"\nconstructor = \"fields\"\nfallback = \"strict_diagnostic\"\n\n[[transforms]]\nprovider = \"toy.pydantic.remote_call\"\nkind = \"function_decorator\"\ntarget = \"function\"\ncapabilities = [\"effect_io_net\", \"effect_time\"]\nfallback = \"strict_diagnostic\"\n\n[[golden_tests]]\nname = \"fixture\"\ninput = \"fixture.tpy\"\nexpected_py = \"expected/app/__init__.py\"\nexpected_pyi = \"expected/app/__init__.pyi\"\ncheckers = [\"basedpyright\", \"mypy\", \"pyright\", \"ty\"]\n",
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

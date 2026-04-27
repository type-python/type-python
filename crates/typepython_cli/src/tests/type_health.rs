use super::*;

#[test]
fn type_health_command_parses_fail_under_and_lock_flags() {
    let cli = Cli::parse_from([
        "typepython",
        "type-health",
        "--project",
        "examples/hello-world",
        "--format",
        "json",
        "--fail-under",
        "90",
        "--write-lock",
    ]);

    let super::Command::TypeHealth(args) = cli.command else {
        panic!("expected type-health command");
    };

    assert_eq!(args.run.project, Some(PathBuf::from("examples/hello-world")));
    assert_eq!(args.run.format, super::OutputFormat::Json);
    assert_eq!(args.fail_under, Some(90));
    assert!(args.write_lock);
}

#[test]
fn build_type_health_report_detects_pep561_and_stub_packages() {
    let project_dir = temp_project_dir("build_type_health_report_detects_packages");
    let report = {
        fs::create_dir_all(project_dir.join("site/demo")).expect("runtime package should exist");
        fs::write(project_dir.join("site/demo/py.typed"), "").expect("marker should be written");
        fs::create_dir_all(project_dir.join("site/demo-1.2.3.dist-info"))
            .expect("runtime metadata should exist");
        fs::write(
            project_dir.join("site/demo-1.2.3.dist-info/METADATA"),
            "Name: demo\nVersion: 1.2.3\n",
        )
        .expect("runtime metadata should be written");
        fs::create_dir_all(project_dir.join("site/demo-stubs/demo"))
            .expect("stub package should exist");
        fs::write(
            project_dir.join("site/demo-stubs/demo/__init__.pyi"),
            "from typing import Any\n\ndef fetch() -> Any: ...\ndef _private() -> Any: ...\nasync def load() -> typing.Any: ...\n",
        )
        .expect("stub surface should be written");
        fs::write(project_dir.join("site/demo-stubs/py.typed"), "partial\n")
            .expect("partial marker should be written");
        fs::create_dir_all(project_dir.join("site/demo_stubs-1.2.0.dist-info"))
            .expect("stub metadata should exist");
        fs::write(
            project_dir.join("site/demo_stubs-1.2.0.dist-info/METADATA"),
            "Name: demo-stubs\nVersion: 1.2.0\n",
        )
        .expect("stub metadata should be written");
        fs::create_dir_all(project_dir.join("site/untyped")).expect("untyped package should exist");

        build_type_health_report(&project_dir, &[String::from("site")])
            .expect("report should build")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.packages.len(), 3);
    assert_eq!(report.score, 66);
    assert!(report.packages.iter().any(|package| package.name == "demo" && package.has_py_typed));
    assert!(report.packages.iter().any(|package| package.name == "demo"
        && package.is_stub_only
        && package.is_partial_stub
        && package.public_any_returns == 2
        && package.runtime_version.as_deref() == Some("1.2.3")
        && package.stub_version.as_deref() == Some("1.2.0")
        && package.stub_version_matches_runtime == Some(false)));
}

#[test]
fn run_type_health_writes_lock_and_enforces_threshold() {
    let project_dir = temp_project_dir("run_type_health_writes_lock_and_enforces_threshold");
    let (success, failure, lock) = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.12\"\n\n[resolution]\nanalysis_python = \"3.11\"\ntype_roots = [\"site\"]\n",
        )
        .expect("config should be written");
        fs::create_dir_all(project_dir.join("src")).expect("src should exist");
        fs::create_dir_all(project_dir.join("site/demo")).expect("package should exist");
        fs::write(project_dir.join("site/demo/py.typed"), "").expect("marker should be written");
        fs::create_dir_all(project_dir.join("site/typing_extensions-4.12.2.dist-info"))
            .expect("typing_extensions metadata should exist");
        fs::write(
            project_dir.join("site/typing_extensions-4.12.2.dist-info/METADATA"),
            "Name: typing_extensions\nVersion: 4.12.2\n",
        )
        .expect("typing_extensions metadata should be written");

        let success = run_type_health(TypeHealthArgs {
            run: RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            fail_under: Some(100),
            write_lock: true,
        })
        .expect("type-health should run");
        let failure = run_type_health(TypeHealthArgs {
            run: RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            fail_under: Some(101),
            write_lock: false,
        })
        .expect("type-health should run");
        let lock = fs::read_to_string(project_dir.join(".typepython/type-lock.toml"))
            .expect("lock should be written");
        (success, failure, lock)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(success, ExitCode::SUCCESS);
    assert_eq!(failure, ExitCode::FAILURE);
    assert!(lock.contains("[inputs]"));
    assert!(lock.contains("target_python = \"3.12\""));
    assert!(lock.contains("analysis_python = \"3.11\""));
    assert!(lock.contains("typing_extensions_version = \"4.12.2\""));
    assert!(lock.contains("typeshed_commit = \"68517355a3269be407bde20fea8fd66af2dc4241\""));
    assert!(lock.contains("[[checker]]"));
    assert!(lock.contains("name = \"mypy\""));
    assert!(lock.contains("name = \"pyright\""));
    assert!(lock.contains("name = \"ty\""));
    assert!(lock.contains("[[package]]"));
    assert!(lock.contains("has_py_typed = true"));
    assert!(lock.contains("public_any_returns = 0"));
}

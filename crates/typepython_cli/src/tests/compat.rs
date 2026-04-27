use super::*;

#[test]
fn compat_command_parses_checker_preset_flags() {
    let cli = Cli::parse_from([
        "typepython",
        "compat",
        "--project",
        "examples/hello-world",
        "--format",
        "json",
        "--checkers",
        "mypy,pyright",
        "--strict-portability",
    ]);

    let super::Command::Compat(args) = cli.command else {
        panic!("expected compat command");
    };

    assert_eq!(args.run.project, Some(PathBuf::from("examples/hello-world")));
    assert_eq!(args.run.format, super::OutputFormat::Json);
    assert_eq!(args.checkers, "mypy,pyright");
    assert!(args.strict_portability);
}

#[test]
fn expand_checker_list_expands_default_all_matrix() {
    assert_eq!(
        expand_checker_list("all").expect("checker list should expand"),
        vec![String::from("mypy"), String::from("pyright"), String::from("ty")],
    );
}

#[test]
fn expand_checker_list_deduplicates_named_checkers() {
    assert_eq!(
        expand_checker_list("pyright,mypy,pyright").expect("checker list should expand"),
        vec![String::from("mypy"), String::from("pyright")],
    );
}

#[test]
fn compat_checker_invocations_use_known_cli_conventions() {
    let out_root = PathBuf::from(".typepython/build");

    assert_eq!(
        external_checker_invocation("mypy", "3.12", &out_root).args,
        vec![
            String::from("--python-version"),
            String::from("3.12"),
            String::from(".typepython/build")
        ],
    );
    assert_eq!(
        external_checker_invocation("pyright", "3.12", &out_root).args,
        vec![
            String::from("--pythonversion"),
            String::from("3.12"),
            String::from(".typepython/build")
        ],
    );
    assert_eq!(
        external_checker_invocation("ty", "3.12", &out_root).args,
        vec![
            String::from("check"),
            String::from("--no-progress"),
            String::from("--python-version"),
            String::from("3.12"),
            String::from(".typepython/build"),
        ],
    );
}

#[cfg(unix)]
#[test]
fn run_compat_uses_verify_pipeline_and_configured_checker() {
    let project_dir = temp_project_dir("run_compat_uses_verify_pipeline_and_configured_checker");
    let checker_path = project_dir.join("fake-checker.sh");
    let invoked_path = project_dir.join("checker-args.txt");
    let result = {
        let init_result = init_project(super::InitArgs {
            dir: project_dir.clone(),
            force: false,
            embed_pyproject: false,
        })
        .expect("init should succeed");
        assert_eq!(init_result, ExitCode::SUCCESS);
        write_executable_script(
            &checker_path,
            &format!("#!/bin/sh\nprintf '%s' \"$1\" > \"{}\"\n", invoked_path.display()),
        );

        let compat_result = run_compat(CompatArgs {
            run: RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            checkers: checker_path.display().to_string(),
            strict_portability: false,
        })
        .expect("compat should succeed with a passing checker");

        (compat_result, fs::read_to_string(&invoked_path).expect("checker args should be recorded"))
    };
    remove_temp_project_dir(&project_dir);

    let (compat_result, invoked) = result;
    assert_eq!(compat_result, ExitCode::SUCCESS);
    assert!(invoked.ends_with(".typepython/build"));
}

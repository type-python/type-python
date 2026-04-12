use super::*;

#[test]
fn verify_build_artifacts_reports_missing_runtime_and_marker_files() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_reports_missing_runtime_and_marker_files");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("missing runtime artifact"));
    assert!(rendered.contains("missing package marker"));
}

#[test]
fn run_verify_bootstraps_outputs_after_clean_project() {
    let project_dir = temp_project_dir("run_verify_bootstraps_outputs_after_clean_project");
    let result = {
        let init_result = init_project(super::InitArgs {
            dir: project_dir.clone(),
            force: false,
            embed_pyproject: false,
        })
        .expect("init should succeed");
        assert_eq!(init_result, ExitCode::SUCCESS);

        let out_dir = project_dir.join(".typepython/build");
        let cache_dir = project_dir.join(".typepython/cache");
        let runtime_path = out_dir.join("app/__init__.py");
        let stub_path = out_dir.join("app/__init__.pyi");
        let marker_path = out_dir.join("app/py.typed");
        let snapshot_path = cache_dir.join("snapshot.json");

        let clean_result = clean_project(CleanArgs { project: Some(project_dir.clone()) })
            .expect("clean should succeed");
        assert_eq!(clean_result, ExitCode::SUCCESS);
        assert!(!out_dir.exists());
        assert!(!cache_dir.exists());

        let verify_result = run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            checkers: Vec::new(),
            unsafe_runtime_imports: false,
        })
        .expect("verify should succeed");

        (
            verify_result,
            runtime_path.exists(),
            stub_path.exists(),
            marker_path.exists(),
            snapshot_path.exists(),
        )
    };
    remove_temp_project_dir(&project_dir);

    let (verify_result, runtime_exists, stub_exists, marker_exists, snapshot_exists) = result;
    assert_eq!(verify_result, ExitCode::SUCCESS);
    assert!(runtime_exists);
    assert!(stub_exists);
    assert!(marker_exists);
    assert!(snapshot_exists);
}

#[test]
fn run_verify_bootstraps_bytecode_after_clean_when_emit_pyc_is_enabled() {
    let project_dir =
        temp_project_dir("run_verify_bootstraps_bytecode_after_clean_when_emit_pyc_is_enabled");
    let result = {
        let init_result = init_project(super::InitArgs {
            dir: project_dir.clone(),
            force: false,
            embed_pyproject: false,
        })
        .expect("init should succeed");
        assert_eq!(init_result, ExitCode::SUCCESS);
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nemit_pyc = true\n",
        )
        .expect("test setup should succeed");

        let runtime_path = project_dir.join(".typepython/build/app/__init__.py");
        let bytecode_path =
            bytecode_path_for(&runtime_path).expect("bytecode path should be computed");

        let clean_result = clean_project(CleanArgs { project: Some(project_dir.clone()) })
            .expect("clean should succeed");
        assert_eq!(clean_result, ExitCode::SUCCESS);
        assert!(!runtime_path.exists());
        assert!(!bytecode_path.exists());

        let verify_result = run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            checkers: Vec::new(),
            unsafe_runtime_imports: false,
        })
        .expect("verify should succeed");

        (verify_result, runtime_path.exists(), bytecode_path.exists())
    };
    remove_temp_project_dir(&project_dir);

    let (verify_result, runtime_exists, bytecode_exists) = result;
    assert_eq!(verify_result, ExitCode::SUCCESS);
    assert!(runtime_exists);
    assert!(bytecode_exists);
}

#[cfg(unix)]
#[test]
fn run_verify_invokes_external_checker_on_emitted_output() {
    let project_dir = temp_project_dir("run_verify_invokes_external_checker_on_emitted_output");
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

        let verify_result = run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            checkers: vec![checker_path.display().to_string()],
            unsafe_runtime_imports: false,
        })
        .expect("verify should succeed with a passing checker");

        (verify_result, fs::read_to_string(&invoked_path).expect("checker args should be recorded"))
    };
    remove_temp_project_dir(&project_dir);

    let (verify_result, invoked) = result;
    assert_eq!(verify_result, ExitCode::SUCCESS);
    assert!(invoked.ends_with(".typepython/build"));
}

#[cfg(unix)]
#[test]
fn run_verify_reports_external_checker_failure() {
    let project_dir = temp_project_dir("run_verify_reports_external_checker_failure");
    let checker_path = project_dir.join("fake-checker.sh");
    let verify_result = {
        let init_result = init_project(super::InitArgs {
            dir: project_dir.clone(),
            force: false,
            embed_pyproject: false,
        })
        .expect("init should succeed");
        assert_eq!(init_result, ExitCode::SUCCESS);
        write_executable_script(&checker_path, "#!/bin/sh\necho 'checker failed' >&2\nexit 1\n");

        run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            checkers: vec![checker_path.display().to_string()],
            unsafe_runtime_imports: false,
        })
        .expect("verify should complete with checker diagnostics")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(verify_result, ExitCode::from(1));
}

#[test]
fn run_verify_reports_python_companion_stub_signature_mismatch() {
    let project_dir =
        temp_project_dir("run_verify_reports_python_companion_stub_signature_mismatch");
    let verify_result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.py"), "def build_user() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.pyi"), "def build_user(name: str) -> str: ...\n")
            .expect("test setup should succeed");

        run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            checkers: Vec::new(),
            unsafe_runtime_imports: false,
        })
        .expect("verify should run")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(verify_result, ExitCode::from(1));
}

#[test]
fn run_verify_reports_python_companion_stub_signature_mismatch_in_wheel() {
    let project_dir =
        temp_project_dir("run_verify_reports_python_companion_stub_signature_mismatch_in_wheel");
    let verify_result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("dist")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.py"), "def build_user() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.pyi"), "def build_user(name: str) -> str: ...\n")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/demo_pkg-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app.py", "def build_user() -> int:\n    return 1\n"),
                ("app.pyi", "def build_user(name: str) -> str: ...\n"),
            ],
        );

        run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: vec![wheel_path],
            sdists: Vec::new(),
            checkers: Vec::new(),
            unsafe_runtime_imports: false,
        })
        .expect("verify should run")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(verify_result, ExitCode::from(1));
}

#[test]
fn run_verify_skips_runtime_import_probes_by_default() {
    let project_dir = temp_project_dir("run_verify_skips_runtime_import_probes_by_default");
    let verify_result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.py"),
            "def exports():\n    return [\"build_user\"]\n\n__all__ = exports()\n\nraise RuntimeError(\"boom\")\n\ndef build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.pyi"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");

        run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            checkers: Vec::new(),
            unsafe_runtime_imports: false,
        })
        .expect("verify should run")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(verify_result, ExitCode::SUCCESS);
}

#[test]
fn run_verify_reports_runtime_import_failure_when_unsafe_runtime_imports_enabled() {
    let project_dir = temp_project_dir(
        "run_verify_reports_runtime_import_failure_when_unsafe_runtime_imports_enabled",
    );
    let verify_result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.py"),
            "def exports():\n    return [\"build_user\"]\n\n__all__ = exports()\n\nraise RuntimeError(\"boom\")\n\ndef build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.pyi"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");

        run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            checkers: Vec::new(),
            unsafe_runtime_imports: true,
        })
        .expect("verify should run")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(verify_result, ExitCode::from(1));
}

#[cfg(unix)]
#[test]
fn run_verify_ignores_project_python_executable_by_default() {
    let project_dir = temp_project_dir("run_verify_ignores_project_python_executable_by_default");
    let marker_path = project_dir.join("python-executed.txt");
    let verify_result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("bin")).expect("test setup should succeed");
        write_executable_script(
            &project_dir.join("bin/fake-python.sh"),
            &format!("#!/bin/sh\nprintf 'executed' > '{}'\nexit 97\n", marker_path.display()),
        );
        fs::write(
            project_dir.join("typepython.toml"),
            format!(
                "[project]\nsrc = [\"src\"]\n\n[resolution]\npython_executable = \"bin{}fake-python.sh\"\n",
                MAIN_SEPARATOR
            ),
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.py"),
            "def exports():\n    return [\"build_user\"]\n\n__all__ = exports()\n\ndef build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.pyi"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");

        run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            checkers: Vec::new(),
            unsafe_runtime_imports: false,
        })
        .expect("verify should run")
    };
    let marker_exists = marker_path.exists();
    remove_temp_project_dir(&project_dir);

    assert_eq!(verify_result, ExitCode::SUCCESS);
    assert!(!marker_exists);
}

#[test]
fn verify_build_artifacts_accepts_present_runtime_stub_and_marker_files() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_accepts_present_runtime_stub_and_marker_files");
    let diagnostics = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nemit_pyc = true\n",
        )
        .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/helpers.pyi"),
            "def helper() -> int: ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app/__pycache__"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__pycache__/__init__.pyc"), "pyc")
            .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[
                EmitArtifact {
                    source_path: project_dir.join("src/app/__init__.tpy"),
                    runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                    stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
                },
                EmitArtifact {
                    source_path: project_dir.join("src/app/helpers.pyi"),
                    runtime_path: None,
                    stub_path: Some(project_dir.join(".typepython/build/app/helpers.pyi")),
                },
            ],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
}

#[test]
fn verify_build_artifacts_accepts_native_type_alias_surface() {
    let project_dir = temp_project_dir("verify_build_artifacts_accepts_native_type_alias_surface");
    let rendered = {
        fs::create_dir_all(project_dir.join("build/app")).expect("build dir should be created");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n",
        )
        .expect("typepython.toml should be written");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("build dir should be created");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("cache dir should be created");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "type Pair[T] = tuple[T, T]\n",
        )
            .expect("runtime artifact should be written");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "type Pair[T] = tuple[T, T]\n",
        )
            .expect("stub artifact should be written");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("marker should be written");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("snapshot should be written");

        let config = load(&project_dir).expect("config should load");
        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(!rendered.contains("TPY5003"), "{rendered}");
}

#[test]
fn verify_build_artifacts_accepts_native_generic_class_and_function_surface() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_accepts_native_generic_class_and_function_surface");
    let rendered = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n",
        )
        .expect("typepython.toml should be written");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("build dir should be created");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("cache dir should be created");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "class Box[T = int]:\n    value: T\n\ndef first[T = int](value: T = 1) -> T:\n    return value\n",
        )
        .expect("runtime artifact should be written");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "class Box[T = int]:\n    value: T\n\ndef first[T = int](value: T = 1) -> T: ...\n",
        )
        .expect("stub artifact should be written");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("marker should be written");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("snapshot should be written");

        let config = load(&project_dir).expect("config should load");
        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(!rendered.contains("TPY5003"), "{rendered}");
}

#[test]
fn verify_build_artifacts_warns_about_comment_only_stub_metadata() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_warns_about_comment_only_stub_metadata");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "class Expr:\n    pass\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "# tpy:sealed Expr -> {Num, Add}\nclass Expr:  # tpy:sealed\n    ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(!diagnostics.has_errors());
    let rendered = diagnostics.as_text();
    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("tpy:sealed"));
    assert!(rendered.contains("external type checkers ignore"));
}

#[test]
fn verify_build_artifacts_warns_about_unknown_boundary_metadata() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_warns_about_unknown_boundary_metadata");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "def take(value: object) -> object:\n    return value\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "# tpy:unknown take\ndef take(value: object) -> object: ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(!diagnostics.has_errors());
    let rendered = diagnostics.as_text();
    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("tpy:unknown"));
}

#[test]
fn verify_build_artifacts_reports_missing_bytecode_when_enabled() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_reports_missing_bytecode_when_enabled");
    let rendered = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nemit_pyc = true\n",
        )
        .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("missing bytecode artifact"));
}

#[test]
fn verify_build_artifacts_reports_missing_incremental_snapshot() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_reports_missing_incremental_snapshot");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("missing incremental snapshot"));
}

#[test]
fn verify_build_artifacts_reports_invalid_emitted_python_syntax() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_reports_invalid_emitted_python_syntax");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "def broken(:\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("is not valid Python syntax"));
}

#[test]
fn verify_build_artifacts_reports_runtime_stub_surface_mismatch() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_reports_runtime_stub_surface_mismatch");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "def build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("declaration surface differs"));
}

#[test]
fn verify_build_artifacts_reports_method_kind_surface_mismatch() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_reports_method_kind_surface_mismatch");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "class Box:\n    @classmethod\n    def build(cls) -> None:\n        pass\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "class Box:\n    def build(self) -> None: ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("declaration surface differs"));
}

#[test]
fn verify_build_artifacts_reports_function_signature_surface_mismatch() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_reports_function_signature_surface_mismatch");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "def build_user(name: str) -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "def build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("declaration surface differs"));
}

#[test]
fn verify_build_artifacts_accepts_dynamic_runtime_all_exports() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_accepts_dynamic_runtime_all_exports");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "def exports():\n    return [\"build_user\"]\n\n__all__ = exports()\n\ndef build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "__all__: list[str] = [\"build_user\"]\n\ndef build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
}

#[test]
fn verify_build_artifacts_accepts_dynamic_runtime_all_tuple_helper_exports() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_accepts_dynamic_runtime_all_tuple_helper_exports");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "def exports():\n    return (\"build_user\",)\n\n__all__ = exports()\n\ndef build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
}

#[test]
fn verify_build_artifacts_accepts_stub_all_without_annotation() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_accepts_stub_all_without_annotation");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
}

#[test]
fn verify_build_artifacts_reports_runtime_statements_inside_stub() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_reports_runtime_statements_inside_stub");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "def build() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("contains runtime statements"));
}

#[test]
fn verify_build_artifacts_reports_corrupt_incremental_snapshot() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_reports_corrupt_incremental_snapshot");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/cache/snapshot.json"), "{not-json\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY6001"));
    assert!(rendered.contains("incompatible or corrupt"));
}

#[test]
fn verify_build_artifacts_requires_py_typed_for_stub_only_package() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_requires_py_typed_for_stub_only_package");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "def helper() -> int: ...\n",
        )
        .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.pyi"),
                runtime_path: None,
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("missing package marker"));
}

#[test]
fn verify_build_artifacts_requires_py_typed_for_implicit_namespace_package() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_requires_py_typed_for_implicit_namespace_package");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/ns"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/ns/mod.py"),
            "def build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/ns/mod.pyi"),
            "def build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState::default(),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/ns/mod.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/ns/mod.py")),
                stub_path: Some(project_dir.join(".typepython/build/ns/mod.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("missing package marker"));
    assert!(rendered.contains("ns/py.typed"));
}

#[test]
fn verify_packaged_artifacts_accepts_matching_wheel_and_sdist() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_accepts_matching_wheel_and_sdist");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "def build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "def build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_zip_archive(
            &wheel_path,
            &[
                ("app/__init__.py", "def build_user() -> int:\n    return 1\n"),
                ("app/__init__.pyi", "def build_user() -> int: ...\n"),
                ("app/py.typed", ""),
                ("type_python-0.1.0.dist-info/METADATA", "Metadata-Version: 2.1\n"),
            ],
        );
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "def build_user() -> int:\n    return 1\n"),
                ("app/__init__.pyi", "def build_user() -> int: ...\n"),
                ("app/py.typed", ""),
                ("README.md", "type-python\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[
                SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path },
                SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path },
            ],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
}

#[test]
fn verify_publication_metadata_reports_requires_python_mismatch_for_native_output() {
    let project_dir =
        temp_project_dir("verify_publication_metadata_reports_requires_python_mismatch_for_native_output");
    let rendered = {
        fs::create_dir_all(project_dir.join("build/app")).expect("build dir should be created");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n",
        )
        .expect("typepython.toml should be written");
        fs::write(
            project_dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\nrequires-python = \">=3.12\"\n",
        )
        .expect("pyproject.toml should be written");
        fs::write(
            project_dir.join("build/app/__init__.py"),
            "type Pair[T = int] = tuple[T, T]\n",
        )
        .expect("runtime artifact should be written");
        fs::write(
            project_dir.join("build/app/__init__.pyi"),
            "type Pair[T = int] = tuple[T, T]\n",
        )
        .expect("stub artifact should be written");

        let config = load(&project_dir).expect("config should load");
        verify_publication_metadata(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join("build/app/__init__.py")),
                stub_path: Some(project_dir.join("build/app/__init__.pyi")),
            }],
            None,
            &[],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("Requires-Python"));
    assert!(rendered.contains("at least `3.13`"));
}

#[test]
fn verify_publication_metadata_reports_missing_typing_extensions_baseline_in_wheel_metadata() {
    let project_dir = temp_project_dir(
        "verify_publication_metadata_reports_missing_typing_extensions_baseline_in_wheel_metadata",
    );
    let rendered = {
        fs::create_dir_all(project_dir.join("build/app")).expect("build dir should be created");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("typepython.toml should be written");
        fs::write(
            project_dir.join("build/app/__init__.py"),
            "from typing_extensions import ReadOnly\n",
        )
        .expect("runtime artifact should be written");
        fs::write(
            project_dir.join("build/app/__init__.pyi"),
            "from typing_extensions import ReadOnly\n",
        )
        .expect("stub artifact should be written");
        let wheel_path = project_dir.join("dist/demo-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[(
                "demo-0.1.0.dist-info/METADATA",
                "Metadata-Version: 2.1\nRequires-Python: >=3.10\n",
            )],
        );

        let config = load(&project_dir).expect("config should load");
        verify_publication_metadata(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join("build/app/__init__.py")),
                stub_path: Some(project_dir.join("build/app/__init__.pyi")),
            }],
            None,
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("typing_extensions>=4.12"));
}

#[test]
fn verify_publication_metadata_accepts_matching_requires_python_for_native_output() {
    let project_dir =
        temp_project_dir("verify_publication_metadata_accepts_matching_requires_python_for_native_output");
    let rendered = {
        fs::create_dir_all(project_dir.join("build/app")).expect("build dir should be created");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n",
        )
        .expect("typepython.toml should be written");
        fs::write(
            project_dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\nrequires-python = \">=3.13\"\n",
        )
        .expect("pyproject.toml should be written");
        fs::write(
            project_dir.join("build/app/__init__.py"),
            "type Pair[T = int] = tuple[T, T]\n",
        )
        .expect("runtime artifact should be written");
        fs::write(
            project_dir.join("build/app/__init__.pyi"),
            "type Pair[T = int] = tuple[T, T]\n",
        )
        .expect("stub artifact should be written");

        let config = load(&project_dir).expect("config should load");
        verify_publication_metadata(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join("build/app/__init__.py")),
                stub_path: Some(project_dir.join("build/app/__init__.pyi")),
            }],
            None,
            &[],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(!rendered.contains("TPY5003"), "{rendered}");
}

#[test]
fn verify_publication_metadata_accepts_typing_extensions_baseline_when_declared() {
    let project_dir = temp_project_dir(
        "verify_publication_metadata_accepts_typing_extensions_baseline_when_declared",
    );
    let rendered = {
        fs::create_dir_all(project_dir.join("build/app")).expect("build dir should be created");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("typepython.toml should be written");
        fs::write(
            project_dir.join("pyproject.toml"),
            concat!(
                "[project]\n",
                "name = \"demo\"\n",
                "version = \"0.1.0\"\n",
                "requires-python = \">=3.10\"\n",
                "dependencies = [\"typing_extensions>=4.12\"]\n",
            ),
        )
        .expect("pyproject.toml should be written");
        fs::write(
            project_dir.join("build/app/__init__.py"),
            "from typing_extensions import ReadOnly\n",
        )
        .expect("runtime artifact should be written");
        fs::write(
            project_dir.join("build/app/__init__.pyi"),
            "from typing_extensions import ReadOnly\n",
        )
        .expect("stub artifact should be written");

        let config = load(&project_dir).expect("config should load");
        verify_publication_metadata(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join("build/app/__init__.py")),
                stub_path: Some(project_dir.join("build/app/__init__.pyi")),
            }],
            None,
            &[],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(!rendered.contains("TPY5003"), "{rendered}");
}

#[test]
fn verify_publication_metadata_prefers_lowered_module_requirements_when_available() {
    let project_dir = temp_project_dir(
        "verify_publication_metadata_prefers_lowered_module_requirements_when_available",
    );
    let rendered = {
        fs::create_dir_all(project_dir.join("build/app")).expect("build dir should be created");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("typepython.toml should be written");
        fs::write(
            project_dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\nrequires-python = \">=3.12\"\n",
        )
        .expect("pyproject.toml should be written");
        fs::write(project_dir.join("build/app/__init__.py"), "pass\n")
            .expect("runtime artifact should be written");
        fs::write(project_dir.join("build/app/__init__.pyi"), "pass\n")
            .expect("stub artifact should be written");

        let config = load(&project_dir).expect("config should load");
        verify_publication_metadata(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join("build/app/__init__.py")),
                stub_path: Some(project_dir.join("build/app/__init__.pyi")),
            }],
            Some(&[typepython_lowering::LoweredModule {
                source_path: project_dir.join("src/app/__init__.tpy"),
                source_kind: typepython_syntax::SourceKind::TypePython,
                python_source: String::from("pass\n"),
                source_map: Vec::new(),
                span_map: Vec::new(),
                required_imports: Vec::new(),
                metadata: typepython_lowering::LoweringMetadata {
                    has_generic_type_params: false,
                    has_typed_dict_transforms: false,
                    has_sealed_classes: false,
                    required_runtime_features: std::collections::BTreeSet::from([
                        typepython_target::RuntimeFeature::GenericDefaults,
                    ]),
                    required_backports: std::collections::BTreeSet::new(),
                },
            }]),
            &[],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("at least `3.13`"));
}

#[test]
fn verify_packaged_artifacts_reports_missing_stub_in_wheel() {
    let project_dir = temp_project_dir("verify_packaged_artifacts_reports_missing_stub_in_wheel");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(&wheel_path, &[("app/__init__.py", "pass\n"), ("app/py.typed", "")]);
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("missing published file `app/__init__.pyi`"));
}

#[test]
fn verify_packaged_artifacts_reports_missing_py_typed_in_wheel() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_missing_py_typed_in_wheel");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[("app/__init__.py", "pass\n"), ("app/__init__.pyi", "pass\n")],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("missing published file `app/py.typed`"));
}

#[test]
fn verify_packaged_artifacts_reports_missing_py_typed_for_implicit_namespace_package_in_wheel() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_reports_missing_py_typed_for_implicit_namespace_package_in_wheel",
    );
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/ns"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/ns/mod.py"),
            "def build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/ns/mod.pyi"),
            "def build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/ns/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("ns/mod.py", "def build_user() -> int:\n    return 1\n"),
                ("ns/mod.pyi", "def build_user() -> int: ...\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/ns/mod.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/ns/mod.py")),
                stub_path: Some(project_dir.join(".typepython/build/ns/mod.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("missing published file `ns/py.typed`"));
}

#[test]
fn verify_packaged_artifacts_reports_missing_py_typed_in_sdist() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_missing_py_typed_in_sdist");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[("app/__init__.py", "pass\n"), ("app/__init__.pyi", "pass\n")],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("missing published file `app/py.typed`"));
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_runtime_file_in_wheel() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_unexpected_runtime_file_in_wheel");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("app/extra.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("unexpected published file `app/extra.py`"));
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_importable_artifact_shapes_in_wheel() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_reports_unexpected_importable_artifact_shapes_in_wheel",
    );
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("app/__pycache__/evil.cpython-311.pyc", "x"),
                ("app/evil.pyo", "x"),
                ("evil.pth", "x"),
                ("type_python-0.1.0.data/purelib/evil.abi3.so", "x"),
                ("type_python-0.1.0.data/platlib/evil.cp311-win_amd64.pyd", "x"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("app/__pycache__/evil.cpython-311.pyc"));
    assert!(rendered.contains("app/evil.pyo"));
    assert!(rendered.contains("evil.pth"));
    assert!(rendered.contains("type_python-0.1.0.data/purelib/evil.abi3.so"));
    assert!(rendered.contains("type_python-0.1.0.data/platlib/evil.cp311-win_amd64.pyd"));
}

#[test]
fn verify_packaged_artifacts_allows_extra_python_files_outside_package_root_in_wheel() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_allows_extra_python_files_outside_package_root_in_wheel",
    );
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("type_python-0.1.0.data/scripts/tool.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_runtime_file_in_sdist() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_unexpected_runtime_file_in_sdist");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("app/extra.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("unexpected published file `app/extra.py`"));
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_importable_artifact_shapes_in_sdist() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_reports_unexpected_importable_artifact_shapes_in_sdist",
    );
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("app/__pycache__/evil.cpython-311.pyc", "x"),
                ("app/evil.pyo", "x"),
                ("evil.pth", "x"),
                ("app/evil.abi3.so", "x"),
                ("app/evil.pyd", "x"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("app/__pycache__/evil.cpython-311.pyc"));
    assert!(rendered.contains("app/evil.pyo"));
    assert!(rendered.contains("evil.pth"));
    assert!(rendered.contains("app/evil.abi3.so"));
    assert!(rendered.contains("app/evil.pyd"));
}

#[test]
fn verify_packaged_artifacts_allows_extra_python_files_outside_package_root_in_sdist() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_allows_extra_python_files_outside_package_root_in_sdist",
    );
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("setup.py", "pass\n"),
                ("conftest.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_top_level_runtime_file_in_wheel() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_reports_unexpected_top_level_runtime_file_in_wheel",
    );
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.pyi"), "pass\n")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[("app.py", "pass\n"), ("app.pyi", "pass\n"), ("extra.py", "pass\n")],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app.py")),
                stub_path: Some(project_dir.join(".typepython/build/app.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("unexpected published file `extra.py`"));
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_top_level_runtime_file_in_sdist() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_reports_unexpected_top_level_runtime_file_in_sdist",
    );
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.pyi"), "pass\n")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[("app.py", "pass\n"), ("app.pyi", "pass\n"), ("extra.py", "pass\n")],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app.py")),
                stub_path: Some(project_dir.join(".typepython/build/app.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("unexpected published file `extra.py`"));
}

#[test]
fn verify_packaged_artifacts_allows_top_level_backend_files_for_module_wheel() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_allows_top_level_backend_files_for_module_wheel",
    );
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.pyi"), "pass\n")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app.py", "pass\n"),
                ("app.pyi", "pass\n"),
                ("type_python-0.1.0.data/scripts/tool.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app.py")),
                stub_path: Some(project_dir.join(".typepython/build/app.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
}

#[test]
fn verify_packaged_artifacts_reports_tests_python_file_in_wheel() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_tests_python_file_in_wheel");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("tests/test_api.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("tests/test_api.py"));
}

#[test]
fn verify_packaged_artifacts_reports_docs_python_file_in_wheel() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_docs_python_file_in_wheel");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("docs/conf.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("docs/conf.py"));
}

#[test]
fn verify_packaged_artifacts_reports_top_level_scripts_python_file() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_top_level_scripts_python_file");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.pyi"), "pass\n")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[("app.py", "pass\n"), ("app.pyi", "pass\n"), ("scripts/tool.py", "pass\n")],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app.py")),
                stub_path: Some(project_dir.join(".typepython/build/app.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("scripts/tool.py"));
}

#[test]
fn verify_packaged_artifacts_reports_tests_python_file_in_sdist() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_tests_python_file_in_sdist");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("tests/test_api.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("tests/test_api.py"));
}

#[test]
fn verify_packaged_artifacts_reports_docs_python_file_in_sdist() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_docs_python_file_in_sdist");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("docs/conf.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("docs/conf.py"));
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_purelib_surface_in_wheel() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_unexpected_purelib_surface_in_wheel");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("type_python-0.1.0.data/purelib/evil.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("purelib/evil.py"));
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_platlib_surface_in_wheel() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_unexpected_platlib_surface_in_wheel");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("type_python-0.1.0.data/platlib/evil/mod.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("platlib/evil/mod.py"));
}

#[test]
fn verify_packaged_artifacts_allows_top_level_backend_files_for_module_sdist() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_allows_top_level_backend_files_for_module_sdist",
    );
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.pyi"), "pass\n")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app.py", "pass\n"),
                ("app.pyi", "pass\n"),
                ("setup.py", "pass\n"),
                ("conftest.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app.py")),
                stub_path: Some(project_dir.join(".typepython/build/app.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_package_surface_for_module_wheel() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_reports_unexpected_package_surface_for_module_wheel",
    );
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.pyi"), "pass\n")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app.py", "pass\n"),
                ("app.pyi", "pass\n"),
                ("evil/__init__.py", "pass\n"),
                ("evil/mod.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app.py")),
                stub_path: Some(project_dir.join(".typepython/build/app.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("evil/__init__.py") || rendered.contains("evil/mod.py"));
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_package_surface_for_module_sdist() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_reports_unexpected_package_surface_for_module_sdist",
    );
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app.pyi"), "pass\n")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app.py", "pass\n"),
                ("app.pyi", "pass\n"),
                ("evil/__init__.py", "pass\n"),
                ("evil/mod.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app.py")),
                stub_path: Some(project_dir.join(".typepython/build/app.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("evil/__init__.py") || rendered.contains("evil/mod.py"));
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_outside_root_surface_for_package_wheel() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_reports_unexpected_outside_root_surface_for_package_wheel",
    );
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("extra.py", "pass\n"),
                ("evil/__init__.py", "pass\n"),
                ("evil/mod.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(
        rendered.contains("extra.py")
            || rendered.contains("evil/__init__.py")
            || rendered.contains("evil/mod.py")
    );
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_outside_root_surface_for_package_sdist() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_reports_unexpected_outside_root_surface_for_package_sdist",
    );
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("extra.py", "pass\n"),
                ("evil/__init__.py", "pass\n"),
                ("evil/mod.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(
        rendered.contains("extra.py")
            || rendered.contains("evil/__init__.py")
            || rendered.contains("evil/mod.py")
    );
}

#[test]
fn verify_packaged_artifacts_reports_unexpected_surface_for_scripts_package_root() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_reports_unexpected_surface_for_scripts_package_root",
    );
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/scripts"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/scripts/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/scripts/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/scripts/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("scripts/__init__.py", "pass\n"),
                ("scripts/__init__.pyi", "pass\n"),
                ("scripts/py.typed", ""),
                ("scripts/extra.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/scripts/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/scripts/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/scripts/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("scripts/extra.py"));
}

#[test]
fn verify_packaged_artifacts_reports_package_shaped_scripts_allowlist_entry() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_reports_package_shaped_scripts_allowlist_entry",
    );
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("scripts/__init__.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("scripts/__init__.py"));
}

#[test]
fn verify_packaged_artifacts_reports_package_shaped_tests_allowlist_entry() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_package_shaped_tests_allowlist_entry");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("tests/__init__.py", "pass\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("tests/__init__.py"));
}

#[test]
fn verify_packaged_artifacts_reports_divergent_runtime_in_sdist() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_reports_divergent_runtime_in_sdist");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "def build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "def build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/py.typed"), "")
            .expect("test setup should succeed");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "def build_user() -> int:\n    return 2\n"),
                ("app/__init__.pyi", "def build_user() -> int: ...\n"),
                ("app/py.typed", ""),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");

        verify_packaged_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("contains `app/__init__.py` that diverges"));
}

#[test]
fn verify_runtime_public_name_parity_accepts_matching_all_exports() {
    let project_dir =
        temp_project_dir("verify_runtime_public_name_parity_accepts_matching_all_exports");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int:\n    return 1\n\ndef _hidden() -> int:\n    return 0\n",
        ).expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int: ...\n\ndef _hidden() -> int: ...\n",
        ).expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_runtime_public_name_parity(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
}

#[test]
fn verify_runtime_public_name_parity_reports_runtime_missing_stub_export() {
    let project_dir =
        temp_project_dir("verify_runtime_public_name_parity_reports_runtime_missing_stub_export");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "def build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "__all__ = [\"build_user\", \"extra\"]\n\ndef build_user() -> int: ...\nextra: int\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_runtime_public_name_parity(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("runtime module `app` is missing public names"));
    assert!(rendered.contains("extra"));
}

#[test]
fn verify_runtime_public_name_parity_reports_stub_missing_runtime_export() {
    let project_dir =
        temp_project_dir("verify_runtime_public_name_parity_reports_stub_missing_runtime_export");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(project_dir.join(".typepython/build/app/__init__.py"), "__all__ = [\"build_user\", \"extra\"]\n\ndef build_user() -> int:\n    return 1\nextra = 1\n").expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "def build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_runtime_public_name_parity(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(
        rendered.contains("authoritative type surface for `app` is missing runtime public names")
    );
    assert!(rendered.contains("extra"));
}

#[test]
fn verify_runtime_public_name_parity_isolates_top_level_runtime_side_effects() {
    let project_dir = temp_project_dir(
        "verify_runtime_public_name_parity_isolates_top_level_runtime_side_effects",
    );
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        let side_effect_path = project_dir.join("import_side_effect.txt");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "from pathlib import Path\nPath(\"import_side_effect.txt\").write_text(\"verify imported me\", encoding=\"utf-8\")\n__all__ = [\"build_user\"]\n\ndef build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        let diagnostics = verify_runtime_public_name_parity(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        );
        (diagnostics, side_effect_path.exists())
    };
    remove_temp_project_dir(&project_dir);

    let (diagnostics, side_effect_exists) = diagnostics;
    assert!(diagnostics.is_empty());
    assert!(!side_effect_exists);
}

#[test]
fn verify_runtime_public_name_parity_uses_top_level_non_underscore_names_when_all_is_absent() {
    let project_dir = temp_project_dir(
        "verify_runtime_public_name_parity_uses_top_level_non_underscore_names_when_all_is_absent",
    );
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "def build_user() -> int:\n    return 1\n\n_hidden = 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "def build_user() -> int: ...\n_hidden: int\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_runtime_public_name_parity(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
}

#[test]
fn verify_runtime_public_name_parity_accepts_dynamic_runtime_all_exports() {
    let project_dir =
        temp_project_dir("verify_runtime_public_name_parity_accepts_dynamic_runtime_all_exports");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "def exports():\n    return [\"build_user\"]\n\n__all__ = exports()\n\ndef build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "__all__: list[str] = [\"build_user\"]\n\ndef build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_runtime_public_name_parity(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
}

#[test]
fn verify_runtime_public_name_parity_reports_invalid_runtime_all_members() {
    let project_dir =
        temp_project_dir("verify_runtime_public_name_parity_reports_invalid_runtime_all_members");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "__all__ = [\"build_user\", 1]\n\ndef build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_runtime_public_name_parity(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("__all__ must contain only strings"));
}

#[test]
fn verify_runtime_public_name_parity_reports_runtime_import_failure() {
    let project_dir =
        temp_project_dir("verify_runtime_public_name_parity_reports_runtime_import_failure");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "raise RuntimeError(\"boom\")\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_runtime_public_name_parity(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"));
    assert!(rendered.contains("runtime module `app`"));
    assert!(rendered.contains("not importable"));
    assert!(rendered.contains("RuntimeError: boom"));
}

#[test]
fn verify_runtime_public_name_parity_uses_configured_interpreter_environment() {
    let project_dir = temp_project_dir(
        "verify_runtime_public_name_parity_uses_configured_interpreter_environment",
    );
    let diagnostics = {
        fs::create_dir_all(project_dir.join("bin")).expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        let fake_python = project_dir.join("bin/fake-python.sh");
        fs::write(
            project_dir.join("typepython.toml"),
            format!(
                "[project]\nsrc = [\"src\"]\n\n[resolution]\npython_executable = \"bin{}fake-python.sh\"\n",
                MAIN_SEPARATOR
            ),
        )
        .expect("test setup should succeed");
        write_executable_script(
            &fake_python,
            r#"#!/bin/sh
if [ "$1" = "-c" ] && printf '%s' "$2" | grep -q 'version_info'; then
  printf '3.10\n'
  exit 0
fi
if printf '%s' "$*" | grep -q 'importlib.import_module'; then
  if printf ' %s ' "$*" | grep -Eq ' -(I|S) '; then
    printf '{"importable": false, "error": "ModuleNotFoundError: No module named demo_dep"}\n'
  else
    printf '{"importable": true, "public_names": ["build_user"]}\n'
  fi
  exit 0
fi
exec python3 "$@"
"#,
        );
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "import demo_dep\n__all__ = [\"build_user\"]\n\ndef build_user() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "__all__ = [\"build_user\"]\n\ndef build_user() -> int: ...\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_runtime_public_name_parity(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            }],
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty(), "{}", diagnostics.as_text());
}

#[test]
fn verify_command_parses_supplied_artifact_flags() {
    let cli = Cli::parse_from([
        "typepython",
        "verify",
        "--project",
        "examples/hello-world",
        "--unsafe-runtime-imports",
        "--wheel",
        "dist/pkg.whl",
        "--sdist",
        "dist/pkg.tar.gz",
        "--checker",
        "pyright",
    ]);

    let super::Command::Verify(args) = cli.command else {
        panic!("expected verify command");
    };
    let supplied = supplied_verify_artifacts(&args);
    assert_eq!(args.checkers, vec![String::from("pyright")]);
    assert!(args.unsafe_runtime_imports);
    assert_eq!(supplied.len(), 2);
    assert!(supplied.iter().any(|artifact| {
        matches!(artifact.kind, SuppliedArtifactKind::Wheel)
            && artifact.path == Path::new("dist/pkg.whl")
    }));
    assert!(supplied.iter().any(|artifact| {
        matches!(artifact.kind, SuppliedArtifactKind::Sdist)
            && artifact.path == Path::new("dist/pkg.tar.gz")
    }));
}

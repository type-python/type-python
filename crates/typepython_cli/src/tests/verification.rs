use super::*;

#[test]
fn supplied_archive_reader_rejects_unsafe_member_paths() {
    let project_dir = temp_project_dir("supplied_archive_reader_rejects_unsafe_member_paths");
    let errors = {
        let wheel = project_dir.join("demo-0.1.0-py3-none-any.whl");
        let sdist = project_dir.join("demo-0.1.0.zip");
        write_zip_archive(&wheel, &[("../outside.py", "pass\n")]);
        write_zip_archive(&sdist, &[("demo-0.1.0/../../outside.py", "pass\n")]);

        [
            SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel },
            SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist },
        ]
        .iter()
        .map(|artifact| {
            inspect_supplied_archive_paths(artifact)
                .expect_err("unsafe archive member path should be rejected")
        })
        .collect::<Vec<_>>()
    };
    remove_temp_project_dir(&project_dir);

    assert!(errors.iter().all(|error| error.contains("forbidden component `..`")), "{errors:?}");
}

#[test]
fn supplied_archive_reader_rejects_absolute_sdist_member_paths() {
    let project_dir =
        temp_project_dir("supplied_archive_reader_rejects_absolute_sdist_member_paths");
    let error = {
        let sdist = project_dir.join("demo-0.1.0.zip");
        write_zip_archive(&sdist, &[("/demo-0.1.0/app/__init__.py", "pass\n")]);

        inspect_supplied_archive_paths(&SuppliedVerifyArtifact {
            kind: SuppliedArtifactKind::Sdist,
            path: sdist,
        })
        .expect_err("absolute sdist member path should be rejected")
    };
    remove_temp_project_dir(&project_dir);

    assert!(error.contains("must be relative"), "{error}");
}

#[test]
fn supplied_archive_reader_rejects_tar_link_members() {
    let project_dir = temp_project_dir("supplied_archive_reader_rejects_tar_link_members");
    let error = {
        let sdist = project_dir.join("demo-0.1.0.tar.gz");
        let file = fs::File::create(&sdist).expect("sdist should be created");
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o777);
        header.set_entry_type(tar::EntryType::symlink());
        header.set_size(0);
        header.set_link_name("/etc/passwd").expect("link target should be valid");
        header.set_cksum();
        builder
            .append_data(&mut header, "demo-0.1.0/leak", std::io::empty())
            .expect("symlink entry should be written");
        let encoder = builder.into_inner().expect("tar stream should finish");
        encoder.finish().expect("gzip stream should finish");

        inspect_supplied_archive_paths(&SuppliedVerifyArtifact {
            kind: SuppliedArtifactKind::Sdist,
            path: sdist,
        })
        .expect_err("tar link member should be rejected")
    };
    remove_temp_project_dir(&project_dir);

    assert!(error.contains("unsupported tar entry type"), "{error}");
    assert!(error.contains("demo-0.1.0/leak"), "{error}");
}

#[test]
fn supplied_archive_reader_rejects_duplicate_member_paths() {
    let project_dir = temp_project_dir("supplied_archive_reader_rejects_duplicate_member_paths");
    let errors = {
        let wheel = project_dir.join("demo-0.1.0-py3-none-any.whl");
        let sdist = project_dir.join("demo-0.1.0.zip");
        let duplicate_files = [("app/__init__.py", "first\n"), ("app/__init__.py", "second\n")];
        write_zip_archive(&wheel, &duplicate_files);
        write_zip_archive(&sdist, &duplicate_files);

        [
            SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel },
            SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist },
        ]
        .iter()
        .map(|artifact| {
            inspect_supplied_archive_paths(artifact)
                .expect_err("duplicate archive member path should be rejected")
        })
        .collect::<Vec<_>>()
    };
    remove_temp_project_dir(&project_dir);

    assert!(errors.iter().all(|error| error.contains("duplicate member path")), "{errors:?}");
}

#[test]
fn supplied_archive_reader_rejects_excessive_compression_ratio() {
    let project_dir =
        temp_project_dir("supplied_archive_reader_rejects_excessive_compression_ratio");
    let error = {
        let wheel = project_dir.join("demo-0.1.0-py3-none-any.whl");
        let payload = "0".repeat(20 * 1024 * 1024);
        write_deflated_zip_archive(&wheel, &[("payload.bin", payload.as_str())]);

        inspect_supplied_archive_paths(&SuppliedVerifyArtifact {
            kind: SuppliedArtifactKind::Wheel,
            path: wheel,
        })
        .expect_err("excessive compression ratio should be rejected")
    };
    remove_temp_project_dir(&project_dir);

    assert!(error.contains("compression ratio"), "{error}");
}

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
fn verify_build_artifacts_rejects_non_file_package_marker() {
    let project_dir = temp_project_dir("verify_build_artifacts_rejects_non_file_package_marker");
    let rendered = {
        let package_root = project_dir.join(".typepython/build/app");
        fs::create_dir_all(package_root.join("py.typed"))
            .expect("directory marker should be created");
        fs::write(package_root.join("__init__.py"), "pass\n")
            .expect("runtime artifact should be written");
        fs::write(package_root.join("__init__.pyi"), "pass\n")
            .expect("stub artifact should be written");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        verify_build_artifacts(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(package_root.join("__init__.py")),
                stub_path: Some(package_root.join("__init__.pyi")),
            }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("package marker"), "{rendered}");
    assert!(rendered.contains("not a regular file"), "{rendered}");
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
            api_diff_old: None,
            checkers: Vec::new(),
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: false,
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
fn run_verify_ignores_runtime_feature_examples_in_docstrings() {
    let project_dir = temp_project_dir("run_verify_ignores_runtime_feature_examples_in_docstrings");
    let result = {
        init_project(super::InitArgs {
            dir: project_dir.clone(),
            force: false,
            embed_pyproject: false,
        })
        .expect("init should succeed");
        fs::write(
            project_dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\nrequires-python = \">=3.10\"\n",
        )
        .expect("pyproject.toml should be written");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            concat!(
                "\"\"\"Examples:\n",
                "type Pair[T = int] = tuple[T, T]\n",
                "\"\"\"\n",
                "def stable() -> int:\n",
                "    return 1\n",
            ),
        )
        .expect("source should be written");

        run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            api_diff_old: None,
            checkers: Vec::new(),
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: false,
        })
        .expect("verify should run")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(result, ExitCode::SUCCESS);
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
            api_diff_old: None,
            checkers: Vec::new(),
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: false,
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
            api_diff_old: None,
            checkers: vec![checker_path.display().to_string()],
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: false,
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
            api_diff_old: None,
            checkers: vec![checker_path.display().to_string()],
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: false,
        })
        .expect("verify should complete with checker diagnostics")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(verify_result, ExitCode::from(1));
}

#[test]
fn checker_allowlist_cannot_hide_checker_spawn_failure() {
    let project_dir = temp_project_dir("checker_allowlist_cannot_hide_checker_spawn_failure");
    let verify_result = {
        init_project(super::InitArgs {
            dir: project_dir.clone(),
            force: false,
            embed_pyproject: false,
        })
        .expect("init should succeed");
        fs::write(
            project_dir.join("checker-allowlist.toml"),
            concat!(
                "[[disagreements]]\n",
                "checker = \"typepython-checker-that-does-not-exist\"\n",
                "contains = \"unable to run external checker\"\n",
                "reason = \"must never mask infrastructure failures\"\n",
                "expires = \"2099-12-31\"\n",
            ),
        )
        .expect("allowlist should be written");

        run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            api_diff_old: None,
            checkers: vec![String::from("typepython-checker-that-does-not-exist")],
            checker_preset: None,
            checker_allowlist: Some(PathBuf::from("checker-allowlist.toml")),
            unsafe_runtime_imports: false,
            publication_type_health: false,
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
            api_diff_old: None,
            checkers: Vec::new(),
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: false,
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
            api_diff_old: None,
            checkers: Vec::new(),
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: false,
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
            api_diff_old: None,
            checkers: Vec::new(),
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: false,
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
            api_diff_old: None,
            checkers: Vec::new(),
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: true,
            publication_type_health: false,
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
            api_diff_old: None,
            checkers: Vec::new(),
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: false,
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
    let project_dir = temp_project_dir(
        "verify_build_artifacts_accepts_native_generic_class_and_function_surface",
    );
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
fn verify_build_artifacts_accepts_typevar_factory_assignments_in_stubs() {
    let project_dir =
        temp_project_dir("verify_build_artifacts_accepts_typevar_factory_assignments_in_stubs");
    let rendered = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("typepython.toml should be written");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("build dir should be created");
        fs::create_dir_all(project_dir.join(".typepython/cache"))
            .expect("cache dir should be created");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.py"),
            "from typing import TypeVar\nT = TypeVar(\"T\")\n\ndef first(value: T) -> T:\n    return value\n",
        )
        .expect("runtime artifact should be written");
        fs::write(
            project_dir.join(".typepython/build/app/__init__.pyi"),
            "from typing import TypeVar\nT = TypeVar(\"T\")\n\ndef first(value: T) -> T: ...\n",
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
    assert!(rendered.contains("runtime only: build_user"));
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
    assert!(rendered.contains("changed members: Box.build"));
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
    assert!(rendered.contains("changed members: build_user"));
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
        fs::write(
            project_dir.join("pyproject.toml"),
            "[project]\nname = \"type-python\"\nversion = \"0.1\"\n",
        )
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
        let wheel_path = project_dir.join("dist/type_python-0.1.0-1-py2.py3-none-any.whl");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_wheel_archive_with_record(
            &wheel_path,
            &[
                ("app/__init__.py", "def build_user() -> int:\n    return 1\n"),
                ("app/__init__.pyi", "def build_user() -> int: ...\n"),
                ("app/py.typed", ""),
                (
                    "type_python-0.1.0.dist-info/METADATA",
                    "Metadata-Version: 2.1\nName: type-python\nVersion: 0.1.0\n",
                ),
                (
                    "type_python-0.1.0.dist-info/WHEEL",
                    "Wheel-Version: 1.0\nGenerator: typepython-test\nRoot-Is-Purelib: true\nTag: py2-none-any\nTag: py3-none-any\nBuild: 1\n",
                ),
            ],
            "type_python-0.1.0.dist-info/RECORD",
        );
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "def build_user() -> int:\n    return 1\n"),
                ("app/__init__.pyi", "def build_user() -> int: ...\n"),
                ("app/py.typed", ""),
                ("PKG-INFO", "Metadata-Version: 2.1\nName: type-python\nVersion: 0.1.0\n"),
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
fn verify_packaged_artifacts_rejects_missing_standard_archive_metadata() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_rejects_missing_standard_archive_metadata");
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
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_zip_archive(
            &wheel_path,
            &[
                ("app/__init__.py", "def build_user() -> int:\n    return 1\n"),
                ("app/__init__.pyi", "def build_user() -> int: ...\n"),
                ("app/py.typed", ""),
                (
                    "type_python-0.1.0.dist-info/METADATA",
                    "Metadata-Version: 2.1\nName: type-python\nVersion: 0.1.0\n",
                ),
            ],
        );
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "def build_user() -> int:\n    return 1\n"),
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
            &[
                SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path },
                SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path },
            ],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("dist-info/WHEEL"), "{rendered}");
    assert!(rendered.contains("dist-info/RECORD"), "{rendered}");
    assert!(rendered.contains("PKG-INFO"), "{rendered}");
}

#[test]
fn verify_packaged_artifacts_rejects_invalid_metadata_values() {
    let project_dir = temp_project_dir("verify_packaged_artifacts_rejects_invalid_metadata_values");
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
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_wheel_archive_with_record(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                (
                    "type_python-0.1.0.dist-info/METADATA",
                    "Metadata-Version: totally-invalid\nName: Kelvin\nVersion: 1.0+K\n",
                ),
                (
                    "type_python-0.1.0.dist-info/WHEEL",
                    "Wheel-Version: 1.\nRoot-Is-Purelib: perhaps\nTag: py2.py3-none-any\n",
                ),
            ],
            "type_python-0.1.0.dist-info/RECORD",
        );
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("PKG-INFO", "Metadata-Version: totally-invalid\nName: Kelvin\nVersion: 1.0+K\n"),
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
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("invalid Metadata-Version"), "{rendered}");
    assert!(rendered.contains("invalid distribution Name"), "{rendered}");
    assert!(rendered.contains("invalid PEP 440 Version"), "{rendered}");
    assert!(rendered.contains("unsupported Wheel-Version"), "{rendered}");
    assert!(rendered.contains("invalid Root-Is-Purelib"), "{rendered}");
    assert!(rendered.contains("invalid expanded compatibility Tag"), "{rendered}");
}

#[test]
fn verify_packaged_artifacts_rejects_archive_identity_mismatches() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_rejects_archive_identity_mismatches");
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
        let sdist_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_wheel_archive_with_record(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                (
                    "archive_distribution-8.8.8.dist-info/METADATA",
                    "Metadata-Version: 2.5\nName: wrong-distribution\nVersion: 9.9.9\n",
                ),
                (
                    "archive_distribution-8.8.8.dist-info/WHEEL",
                    "Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
                ),
            ],
            "archive_distribution-8.8.8.dist-info/RECORD",
        );
        write_tar_gz_archive(
            &sdist_path,
            "type-python-0.1.0",
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("PKG-INFO", "Metadata-Version: 2.5\nName: wrong-distribution\nVersion: 9.9.9\n"),
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
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("wheel artifact filename"), "{rendered}");
    assert!(rendered.contains("sdist artifact filename"), "{rendered}");
    assert!(rendered.contains("identity mismatch"), "{rendered}");
    assert!(rendered.contains("does not match"), "{rendered}");
}

fn prepare_packaged_artifact_test_project(project_dir: &Path) -> EmitArtifact {
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
    EmitArtifact {
        source_path: project_dir.join("src/app/__init__.tpy"),
        runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
        stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
    }
}

#[test]
fn verify_packaged_artifacts_rejects_wheel_tag_and_build_mismatches() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_rejects_wheel_tag_and_build_mismatches");
    let rendered = {
        let artifact = prepare_packaged_artifact_test_project(&project_dir);
        let wheel_path = project_dir.join("dist/type_python-0.1.0-2-py3-none-any.whl");
        write_wheel_archive_with_record(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                (
                    "type_python-0.1.0.dist-info/METADATA",
                    "Metadata-Version: 2.1\nName: type-python\nVersion: 0.1.0\n",
                ),
                (
                    "type_python-0.1.0.dist-info/WHEEL",
                    "Wheel-Version: 1.0\nRoot-Is-Purelib: true\nBuild: 1\nTag: cp313-cp313-manylinux_2_17_x86_64\n",
                ),
            ],
            "type_python-0.1.0.dist-info/RECORD",
        );
        let config = load(&project_dir).expect("test setup should succeed");
        verify_packaged_artifacts(
            &config,
            &[artifact],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("WHEEL Build does not match"), "{rendered}");
    assert!(rendered.contains("WHEEL Tag fields do not match"), "{rendered}");
}

#[test]
fn verify_packaged_artifacts_rejects_sdist_root_and_zip_filename_mismatches() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_rejects_sdist_root_and_zip_filename_mismatches",
    );
    let rendered = {
        let artifact = prepare_packaged_artifact_test_project(&project_dir);
        let tar_path = project_dir.join("dist/type-python-0.1.0.tar.gz");
        write_tar_gz_archive(
            &tar_path,
            "wrong-9.9.9",
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("PKG-INFO", "Metadata-Version: 2.1\nName: type-python\nVersion: 0.1.0\n"),
            ],
        );
        let zip_path = project_dir.join("dist/wrong-9.9.9.zip");
        write_zip_archive(
            &zip_path,
            &[
                ("type-python-0.1.0/app/__init__.py", "pass\n"),
                ("type-python-0.1.0/app/__init__.pyi", "pass\n"),
                ("type-python-0.1.0/app/py.typed", ""),
                (
                    "type-python-0.1.0/PKG-INFO",
                    "Metadata-Version: 2.1\nName: type-python\nVersion: 0.1.0\n",
                ),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");
        verify_packaged_artifacts(
            &config,
            &[artifact],
            &[
                SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: tar_path },
                SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: zip_path },
            ],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("root directory `wrong-9.9.9`"), "{rendered}");
    assert!(rendered.contains("sdist artifact filename"), "{rendered}");
}

#[test]
fn verify_packaged_artifacts_unfolds_metadata_headers_before_validation() {
    let project_dir =
        temp_project_dir("verify_packaged_artifacts_unfolds_metadata_headers_before_validation");
    let rendered = {
        let artifact = prepare_packaged_artifact_test_project(&project_dir);
        let wheel_path = project_dir.join("dist/type_python-0.1.0-py3-none-any.whl");
        write_wheel_archive_with_record(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                (
                    "type_python-0.1.0.dist-info/METADATA",
                    "Metadata-Version: 1.0\nName: type-python\n injected\nVersion:\n 0.1.0\n",
                ),
                (
                    "type_python-0.1.0.dist-info/WHEEL",
                    "Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
                ),
            ],
            "type_python-0.1.0.dist-info/RECORD",
        );
        let config = load(&project_dir).expect("test setup should succeed");
        verify_packaged_artifacts(
            &config,
            &[artifact],
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("invalid distribution Name `type-python injected`"), "{rendered}");
    assert!(rendered.contains("wheels require version 1.1 or newer"), "{rendered}");
    assert!(!rendered.contains("non-empty `Version`"), "{rendered}");
}

#[test]
fn verify_packaged_artifacts_rejects_project_and_cross_archive_identity_mismatches() {
    let project_dir = temp_project_dir(
        "verify_packaged_artifacts_rejects_project_and_cross_archive_identity_mismatches",
    );
    let rendered = {
        let artifact = prepare_packaged_artifact_test_project(&project_dir);
        fs::write(
            project_dir.join("pyproject.toml"),
            "[project]\nname = \"type-python\"\nversion = \"0.1.0\"\n",
        )
        .expect("test setup should succeed");
        let wheel_path = project_dir.join("dist/other-9.9.9-py3-none-any.whl");
        write_wheel_archive_with_record(
            &wheel_path,
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                (
                    "other-9.9.9.dist-info/METADATA",
                    "Metadata-Version: 2.1\nName: other\nVersion: 9.9.9\n",
                ),
                (
                    "other-9.9.9.dist-info/WHEEL",
                    "Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
                ),
            ],
            "other-9.9.9.dist-info/RECORD",
        );
        let sdist_path = project_dir.join("dist/third-8.8.8.tar.gz");
        write_tar_gz_archive(
            &sdist_path,
            "third-8.8.8",
            &[
                ("app/__init__.py", "pass\n"),
                ("app/__init__.pyi", "pass\n"),
                ("app/py.typed", ""),
                ("PKG-INFO", "Metadata-Version: 2.1\nName: third\nVersion: 8.8.8\n"),
            ],
        );
        let config = load(&project_dir).expect("test setup should succeed");
        verify_packaged_artifacts(
            &config,
            &[artifact],
            &[
                SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path },
                SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path: sdist_path },
            ],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("does not match project metadata"), "{rendered}");
    assert!(rendered.contains("does not match wheel artifact"), "{rendered}");
}

#[test]
fn verify_publication_metadata_reports_requires_python_mismatch_for_native_output() {
    let project_dir = temp_project_dir(
        "verify_publication_metadata_reports_requires_python_mismatch_for_native_output",
    );
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
        fs::write(project_dir.join("build/app/__init__.py"), "type Pair[T = int] = tuple[T, T]\n")
            .expect("runtime artifact should be written");
        fs::write(project_dir.join("build/app/__init__.pyi"), "type Pair[T = int] = tuple[T, T]\n")
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
fn verify_publication_metadata_ignores_feature_text_in_strings_and_comments() {
    let project_dir =
        temp_project_dir("verify_publication_metadata_ignores_feature_text_in_strings");
    let rendered = {
        fs::create_dir_all(project_dir.join("build/app")).expect("build dir should be created");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("typepython.toml should be written");
        fs::write(
            project_dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\nrequires-python = \">=3.10\"\n",
        )
        .expect("pyproject.toml should be written");
        let source = concat!(
            "\"\"\"type Pair[T = int] = tuple[T, T]\n",
            "from typing import ReadOnly\n",
            "typing_extensions.TypeIs\n",
            "\"\"\"\n",
            "# class Box[T]: ...\n",
            "def stable() -> int:\n",
            "    return 1\n",
        );
        fs::write(project_dir.join("build/app/__init__.py"), source)
            .expect("runtime artifact should be written");
        fs::write(project_dir.join("build/app/__init__.pyi"), source)
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
fn verify_publication_metadata_ignores_description_headers() {
    let project_dir = temp_project_dir("verify_publication_metadata_ignores_description_headers");
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
        fs::write(project_dir.join("build/app/__init__.py"), "type Pair[T = int] = tuple[T, T]\n")
            .expect("runtime artifact should be written");
        fs::write(project_dir.join("build/app/__init__.pyi"), "type Pair[T = int] = tuple[T, T]\n")
            .expect("stub artifact should be written");
        let wheel_path = project_dir.join("dist/demo-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[(
                "demo-0.1.0.dist-info/METADATA",
                concat!(
                    "Metadata-Version: 2.1\n",
                    "Requires-Python: >=3.12\n",
                    "\n",
                    "The description may contain examples.\n",
                    "Requires-Python: >=3.13\n",
                ),
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

    assert!(rendered.contains("TPY5003"), "{rendered}");
    assert!(rendered.contains("declares Requires-Python `>=3.12`"), "{rendered}");
    assert!(rendered.contains("at least `3.13`"), "{rendered}");
}

#[test]
fn verify_publication_metadata_rejects_duplicate_requires_python_headers() {
    let project_dir =
        temp_project_dir("verify_publication_metadata_rejects_duplicate_requires_python_headers");
    let rendered = {
        fs::create_dir_all(project_dir.join("build/app")).expect("build dir should be created");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("typepython.toml should be written");
        fs::write(
            project_dir.join("build/app/__init__.py"),
            "from typing_extensions import ReadOnly\n",
        )
        .expect("runtime artifact should be written");
        let wheel_path = project_dir.join("dist/demo-0.1.0-py3-none-any.whl");
        write_zip_archive(
            &wheel_path,
            &[(
                "demo-0.1.0.dist-info/METADATA",
                concat!(
                    "Metadata-Version: 2.1\n",
                    "Requires-Python: >=3.12\n",
                    "Requires-Python: >=3.13\n",
                ),
            )],
        );

        let config = load(&project_dir).expect("config should load");
        verify_publication_metadata(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join("build/app/__init__.py")),
                stub_path: None,
            }],
            None,
            &[SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path: wheel_path }],
        )
        .as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY5003"), "{rendered}");
    assert!(rendered.contains("2 `Requires-Python` headers"), "{rendered}");
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
    let project_dir = temp_project_dir(
        "verify_publication_metadata_accepts_matching_requires_python_for_native_output",
    );
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
        fs::write(project_dir.join("build/app/__init__.py"), "type Pair[T = int] = tuple[T, T]\n")
            .expect("runtime artifact should be written");
        fs::write(project_dir.join("build/app/__init__.pyi"), "type Pair[T = int] = tuple[T, T]\n")
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
                    export_runtime_semantics: std::collections::BTreeMap::new(),
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
        write_valid_wheel_archive(
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
        write_valid_sdist_archive(
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
        write_valid_wheel_archive(
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
        write_valid_sdist_archive(
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
fn declaration_surface_accepts_function_to_object_transform_surface() {
    let project_dir =
        temp_project_dir("declaration_surface_accepts_function_to_object_transform_surface");
    let diagnostic = {
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        let runtime_path = project_dir.join(".typepython/build/app/__init__.py");
        let stub_path = project_dir.join(".typepython/build/app/__init__.pyi");
        fs::write(
            &runtime_path,
            "__all__ = [\"build\"]\n\ndef task(fn):\n    return fn\n\n@task\ndef build(value: int) -> str:\n    return str(value)\n",
        )
        .expect("test setup should succeed");
        fs::write(
            &stub_path,
            "__all__ = [\"build\"]\n\nclass Task[P, R]: ...\n\nbuild: Task[[int], str]\n",
        )
        .expect("test setup should succeed");

        verify_emitted_declaration_surface(&runtime_path, &stub_path)
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostic.is_none(), "{diagnostic:?}");
}

#[test]
fn declaration_surface_ignores_function_local_value_bindings() {
    let project_dir = temp_project_dir("declaration_surface_ignores_function_local_value_bindings");
    let diagnostic = {
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
            .expect("test setup should succeed");
        let runtime_path = project_dir.join(".typepython/build/app/__init__.py");
        let stub_path = project_dir.join(".typepython/build/app/__init__.pyi");
        fs::write(
            &runtime_path,
            "def summary(items: list[int]) -> int:\n    total = 0\n    for item in items:\n        total = total + item\n    return total\n",
        )
        .expect("test setup should succeed");
        fs::write(&stub_path, "def summary(items: list[int]) -> int: ...\n")
            .expect("test setup should succeed");

        verify_emitted_declaration_surface(&runtime_path, &stub_path)
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostic.is_none(), "{diagnostic:?}");
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
fn annotation_runtime_pythonpath_uses_platform_path_encoding() {
    let root = PathBuf::from("annotation-runtime-root");
    let first = PathBuf::from("existing-one");
    let second = PathBuf::from("existing-two");
    let existing = env::join_paths([&first, &second]).expect("test paths should be joinable");

    let joined = prepend_pythonpath(&root, Some(&existing)).expect("paths should be joinable");

    assert_eq!(env::split_paths(&joined).collect::<Vec<_>>(), vec![root, first, second]);
}

#[test]
fn runtime_annotation_compatibility_diagnostics_warns_for_py314_consumers() {
    let project_dir =
        temp_project_dir("runtime_annotation_compatibility_diagnostics_warns_for_py314_consumers");
    let diagnostics = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.14\"\n",
        )
        .expect("test setup should succeed");
        let runtime_path = project_dir.join("app.py");
        fs::write(
            &runtime_path,
            "import typing\n\nclass User:\n    name: str\n\ndef inspect_user() -> None:\n    typing.get_type_hints(User)\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        runtime_annotation_compatibility_diagnostics(
            &config,
            &runtime_path,
            PythonTarget::PYTHON_3_14,
        )
    };
    remove_temp_project_dir(&project_dir);

    let rendered = DiagnosticReport { diagnostics: diagnostics.clone() }.as_text();
    assert!(rendered.contains("TPY5004"), "{rendered}");
    assert!(rendered.contains("deferred annotations"), "{rendered}");
    assert!(rendered.contains("typing.get_type_hints"), "{rendered}");
}

#[test]
fn runtime_annotation_compatibility_diagnostics_warns_for_local_scope_annotations() {
    let project_dir = temp_project_dir(
        "runtime_annotation_compatibility_diagnostics_warns_for_local_scope_annotations",
    );
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let runtime_path = project_dir.join("app.py");
        fs::write(
            &runtime_path,
            "def outer():\n    class Local:\n        pass\n    def build(value: Local) -> 'Local':\n        return value\n    return build\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        runtime_annotation_compatibility_diagnostics(
            &config,
            &runtime_path,
            PythonTarget::PYTHON_3_10,
        )
    };
    remove_temp_project_dir(&project_dir);

    let rendered = DiagnosticReport { diagnostics: diagnostics.clone() }.as_text();
    assert!(rendered.contains("TPY5004"), "{rendered}");
    assert!(rendered.contains("local scope"), "{rendered}");
    assert!(rendered.contains("TPY-A001"), "{rendered}");
}

#[test]
fn runtime_annotation_compatibility_diagnostics_ignores_safe_nested_annotations() {
    let project_dir = temp_project_dir(
        "runtime_annotation_compatibility_diagnostics_ignores_safe_nested_annotations",
    );
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let runtime_path = project_dir.join("app.py");
        fs::write(
            &runtime_path,
            "class Box:\n    value: int\n    def render(self, value: int) -> str:\n        return str(value)\n\ndef outer():\n    class Local:\n        pass\n    def build(value: Local) -> Local:\n        return value\n    return build\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        runtime_annotation_compatibility_diagnostics(
            &config,
            &runtime_path,
            PythonTarget::PYTHON_3_10,
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn runtime_annotation_compatibility_diagnostics_enforces_target_syntax() {
    let project_dir =
        temp_project_dir("runtime_annotation_compatibility_diagnostics_enforces_target_syntax");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let runtime_path = project_dir.join("app.py");
        fs::write(&runtime_path, "type Alias = int\n").expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        runtime_annotation_compatibility_diagnostics(
            &config,
            &runtime_path,
            PythonTarget::new(3, 9),
        )
    };
    remove_temp_project_dir(&project_dir);

    let report = DiagnosticReport { diagnostics };
    assert!(report.has_errors(), "confirmed target syntax incompatibility must block verify");
    let rendered = report.as_text();
    assert!(rendered.contains("TPY5004"), "{rendered}");
    assert!(rendered.contains("not valid Python 3.9 syntax"), "{rendered}");
    assert!(!rendered.contains("audit failed"), "{rendered}");
}

#[test]
fn runtime_annotation_compatibility_diagnostics_warns_for_framework_consumers() {
    let project_dir = temp_project_dir(
        "runtime_annotation_compatibility_diagnostics_warns_for_framework_consumers",
    );
    let diagnostics = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.14\"\n",
        )
        .expect("test setup should succeed");
        let runtime_path = project_dir.join("app.py");
        fs::write(
            &runtime_path,
            "from fastapi import Depends, FastAPI\nfrom pydantic import BaseModel, Field\n\napp = FastAPI()\n\nclass User(BaseModel):\n    name: str = Field(alias='user_name')\n\n@app.get('/users/{name}')\ndef read_user(name: str, current: str = Depends()) -> User:\n    return User(name=name)\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        runtime_annotation_compatibility_diagnostics(
            &config,
            &runtime_path,
            PythonTarget::PYTHON_3_14,
        )
    };
    remove_temp_project_dir(&project_dir);

    let rendered = DiagnosticReport { diagnostics: diagnostics.clone() }.as_text();
    assert!(rendered.contains("TPY5004"), "{rendered}");
    assert!(rendered.contains("fastapi.route_decorator"), "{rendered}");
    assert!(rendered.contains("fastapi.Depends"), "{rendered}");
    assert!(rendered.contains("pydantic.BaseModel"), "{rendered}");
    assert!(rendered.contains("pydantic.Field"), "{rendered}");
}

#[test]
fn pep561_readiness_report_marks_ready_project() {
    let project_dir = temp_project_dir("pep561_readiness_report_marks_ready_project");
    let report = {
        fs::create_dir_all(project_dir.join("build/app")).expect("build dir should be created");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("config should be written");
        fs::write(project_dir.join("build/app/__init__.py"), "pass\n")
            .expect("runtime artifact should be written");
        fs::write(project_dir.join("build/app/__init__.pyi"), "def build() -> int: ...\n")
            .expect("stub artifact should be written");
        fs::write(project_dir.join("build/app/py.typed"), "").expect("marker should be written");
        let config = load(&project_dir).expect("config should load");

        pep561_readiness_report(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join("build/app/__init__.py")),
                stub_path: Some(project_dir.join("build/app/__init__.pyi")),
            }],
            &DiagnosticReport::default(),
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(report.ready);
    assert!(report.blocking_issues.is_empty());
}

#[test]
fn pep561_readiness_report_explains_blocking_issues() {
    let project_dir = temp_project_dir("pep561_readiness_report_explains_blocking_issues");
    let report = {
        fs::create_dir_all(project_dir.join("build/app")).expect("build dir should be created");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nwrite_py_typed = false\n",
        )
        .expect("config should be written");
        fs::write(project_dir.join("build/app/__init__.py"), "pass\n")
            .expect("runtime artifact should be written");
        let config = load(&project_dir).expect("config should load");
        let diagnostics = DiagnosticReport {
            diagnostics: vec![Diagnostic::error(
                "TPY5003",
                "wheel artifact `dist/demo.whl` is missing published file `app/py.typed`",
            )],
        };

        pep561_readiness_report(
            &config,
            &[EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join("build/app/__init__.py")),
                stub_path: None,
            }],
            &diagnostics,
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(!report.ready);
    assert!(
        report.blocking_issues.iter().any(|issue| issue.contains("does not include `.pyi` files"))
    );
    assert!(report.blocking_issues.iter().any(|issue| issue.contains("emit.write_py_typed")));
    assert!(
        report
            .blocking_issues
            .iter()
            .any(|issue| issue.contains("missing published file `app/py.typed`"))
    );
}

#[test]
fn verify_runtime_module_importability_accepts_relative_output_root() {
    let project_dir =
        temp_project_dir("verify_runtime_module_importability_accepts_relative_output_root");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join(".typepython/build/app"))
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
        let config = load(&project_dir).expect("test setup should succeed");

        verify_runtime_public_name_parity_for_artifact(
            &config,
            Path::new(".typepython/build"),
            &EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
            },
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn verify_runtime_public_name_parity_imports_each_module_once() {
    let project_dir =
        temp_project_dir("verify_runtime_public_name_parity_imports_each_module_once");
    let (diagnostics, import_count) = {
        let package_root = project_dir.join(".typepython/build/app");
        fs::create_dir_all(&package_root).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let import_marker = project_dir.join("import-count.txt");
        let marker_literal = serde_json::to_string(&import_marker.display().to_string())
            .expect("marker path should serialize");
        fs::write(
            package_root.join("__init__.py"),
            format!(
                "with open({marker_literal}, 'a', encoding='utf-8') as marker:\n    marker.write('x')\n__all__ = ['build_user']\n\ndef build_user() -> int:\n    return 1\n"
            ),
        )
        .expect("runtime artifact should be written");
        fs::write(
            package_root.join("__init__.pyi"),
            "__all__ = ['build_user']\n\ndef build_user() -> int: ...\n",
        )
        .expect("stub artifact should be written");
        let config = load(&project_dir).expect("test setup should succeed");

        let diagnostics = verify_runtime_public_name_parity_for_artifact(
            &config,
            &project_dir.join(".typepython/build"),
            &EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(package_root.join("__init__.py")),
                stub_path: Some(package_root.join("__init__.pyi")),
            },
        );
        let import_count = fs::read_to_string(&import_marker).expect("module should be imported");
        (diagnostics, import_count)
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_eq!(import_count, "x");
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
        "--api-diff",
        "dist/old-stubs",
        "--checker",
        "pyright",
        "--checker-preset",
        "all",
        "--checker-allowlist",
        "checker-allowlist.toml",
        "--publication-type-health",
    ]);

    let super::Command::Verify(args) = cli.command else {
        panic!("expected verify command");
    };
    let supplied = supplied_verify_artifacts(&args);
    assert_eq!(args.checkers, vec![String::from("pyright")]);
    assert_eq!(args.checker_preset, Some(String::from("all")));
    assert_eq!(args.checker_allowlist, Some(PathBuf::from("checker-allowlist.toml")));
    assert_eq!(args.api_diff_old, Some(PathBuf::from("dist/old-stubs")));
    assert!(args.unsafe_runtime_imports);
    assert!(args.publication_type_health);
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

#[test]
fn run_verify_api_diff_fails_on_public_surface_regression() {
    let project_dir = temp_project_dir("run_verify_api_diff_fails_on_public_surface_regression");
    let verify_result = {
        let old_surface = project_dir.join("old");
        fs::create_dir_all(old_surface.join("app")).expect("old surface should exist");
        fs::write(old_surface.join("app/__init__.pyi"), "def parse(value: str) -> int: ...\n")
            .expect("old stub should be written");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("config should be written");
        fs::create_dir_all(project_dir.join("src/app")).expect("source package should exist");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "def parse(value: str) -> str:\n    return value\n",
        )
        .expect("source file should be written");

        run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            api_diff_old: Some(old_surface),
            checkers: Vec::new(),
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: false,
        })
        .expect("verify should run")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(verify_result, ExitCode::from(1));
}

#[test]
fn publication_type_health_diagnostics_fail_for_untyped_packages() {
    let project_dir = temp_project_dir("publication_type_health_diagnostics_fail");
    let diagnostics = {
        fs::create_dir_all(project_dir.join("site/untyped")).expect("untyped package should exist");
        let report = build_type_health_report_for_target(
            &project_dir,
            &[String::from("site")],
            PythonTarget::default(),
        )
        .expect("type-health report should build");
        publication_type_health_diagnostics(&report)
    };
    remove_temp_project_dir(&project_dir);

    let rendered = diagnostics.as_text();
    assert!(rendered.contains("TPY7002"));
    assert!(rendered.contains("publication type-health score 0"));
    assert!(rendered.contains("does not expose PEP 561 typing metadata"));
}

#[test]
fn run_verify_publication_type_health_fails_on_untyped_package_metadata() {
    let project_dir = temp_project_dir("run_verify_publication_type_health_fails");
    let verify_result = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[resolution]\ntype_roots = [\"site\"]\n",
        )
        .expect("config should be written");
        fs::create_dir_all(project_dir.join("src/app")).expect("source package should exist");
        fs::write(project_dir.join("src/app/__init__.tpy"), "pass\n")
            .expect("source file should be written");
        fs::create_dir_all(project_dir.join("site/untyped")).expect("untyped package should exist");

        run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            api_diff_old: None,
            checkers: Vec::new(),
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: true,
        })
        .expect("verify should run")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(verify_result, ExitCode::from(1));
}

#[test]
fn checker_allowlist_downgrades_matching_checker_rejection_to_warning() {
    let diagnostic = Diagnostic::error(
        "TPY5003",
        "external checker `pyright` rejected emitted build output under `.typepython/build`: unsupported transform",
    );
    let allowed = allowlisted_checker_diagnostic(
        "pyright",
        diagnostic,
        &[CheckerAllowlistEntry {
            checker: String::from("pyright"),
            contains: String::from("unsupported transform"),
            reason: String::from("tracked checker limitation"),
            issue: Some(String::from("https://example.invalid/issue/1")),
            expires: Some(String::from("2026-12-31")),
        }],
    );

    assert_eq!(allowed.severity.to_string(), "warning");
    assert!(allowed.message.contains("known checker disagreement allowed"));
    assert!(allowed.notes.iter().any(|note| note.contains("unsupported transform")));
    assert!(allowed.notes.iter().any(|note| note.contains("allowlist expires")));
}

#[test]
fn checker_allowlist_rejects_empty_match_substrings() {
    let error = validate_checker_allowlist_entry(
        &CheckerAllowlistEntry {
            checker: String::from("pyright"),
            contains: String::new(),
            reason: String::from("tracked checker limitation"),
            issue: None,
            expires: Some(String::from("2026-12-31")),
        },
        0,
    )
    .expect_err("an empty substring would match every checker diagnostic");

    assert!(error.to_string().contains("`contains` must not be empty"));
}

#[test]
fn checker_allowlist_rejects_missing_malformed_and_expired_dates() {
    let base = CheckerAllowlistEntry {
        checker: String::from("pyright"),
        contains: String::from("unsupported transform"),
        reason: String::from("tracked checker limitation"),
        issue: None,
        expires: None,
    };
    assert!(
        validate_checker_allowlist_entry(&base, 0)
            .expect_err("expiration should be required")
            .to_string()
            .contains("`expires` is required")
    );

    let malformed =
        CheckerAllowlistEntry { expires: Some(String::from("2026-02-30")), ..base.clone() };
    assert!(
        validate_checker_allowlist_entry(&malformed, 0)
            .expect_err("invalid calendar dates should be rejected")
            .to_string()
            .contains("invalid expiration day")
    );

    let expired = CheckerAllowlistEntry { expires: Some(String::from("1970-01-01")), ..base };
    assert!(
        validate_checker_allowlist_entry(&expired, 1)
            .expect_err("past dates should be rejected")
            .to_string()
            .contains("expired on `1970-01-01`")
    );
}

#[test]
fn type_portability_score_counts_only_blocking_checker_failures() {
    let mut diagnostics = DiagnosticReport::default();
    diagnostics.push(Diagnostic::error(
        "TPY5003",
        "external checker `mypy` rejected emitted build output under `.typepython/build`",
    ));
    diagnostics.push(Diagnostic::warning(
        "TPY5003",
        "known checker disagreement allowed for `pyright`: tracked limitation",
    ));

    assert_eq!(type_portability_score(&diagnostics, 3), "66/100 (2/3 checker(s) passing)");
}

#[test]
fn type_portability_report_serializes_machine_readable_counts() {
    let mut diagnostics = DiagnosticReport::default();
    diagnostics.push(Diagnostic::error(
        "TPY5003",
        "external checker `mypy` rejected emitted build output under `.typepython/build`",
    ));

    let report: TypePortabilityReport = type_portability_report(&diagnostics, 4);
    let payload = serde_json::to_value(&report).expect("report should serialize as JSON");

    assert_eq!(payload["score"], 75);
    assert_eq!(payload["passing_checkers"], 3);
    assert_eq!(payload["total_checkers"], 4);
}

#[test]
fn stub_portability_diagnostics_warns_for_native_forms_before_target_support() {
    let project_dir = temp_project_dir(
        "stub_portability_diagnostics_warns_for_native_forms_before_target_support",
    );
    let diagnostics = {
        let stub_path = project_dir.join("app.pyi");
        fs::write(
            &stub_path,
            "from typing import ReadOnly\n\ntype Box[T] = list[T]\ndef identity[T = int](value: T) -> T: ...\n",
        )
        .expect("test stub should be written");

        stub_portability_diagnostics(&stub_path, PythonTarget::PYTHON_3_10)
    };
    remove_temp_project_dir(&project_dir);

    let rendered = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("Python 3.12 generic syntax"));
    assert!(rendered.contains("Python 3.13 generic default syntax"));
    assert!(rendered.contains("ReadOnly"));
}

#[test]
fn stub_portability_diagnostics_accepts_supported_target_forms() {
    let project_dir =
        temp_project_dir("stub_portability_diagnostics_accepts_supported_target_forms");
    let diagnostics = {
        let stub_path = project_dir.join("app.pyi");
        fs::write(
            &stub_path,
            "from typing import ReadOnly\n\ntype Box[T] = list[T]\ndef identity[T = int](value: T) -> T: ...\n",
        )
        .expect("test stub should be written");

        stub_portability_diagnostics(&stub_path, PythonTarget::PYTHON_3_13)
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
}

#[test]
fn verify_checker_preset_expands_and_deduplicates_with_explicit_checkers() {
    let args = VerifyArgs {
        run: RunArgs { project: None, format: OutputFormat::Json },
        wheels: Vec::new(),
        sdists: Vec::new(),
        api_diff_old: None,
        checkers: vec![String::from("pyright")],
        checker_preset: Some(String::from("all")),
        checker_allowlist: None,
        unsafe_runtime_imports: false,
        publication_type_health: false,
    };

    assert_eq!(
        verify_checker_invocations(&args).expect("checker preset should expand"),
        vec![
            String::from("basedpyright"),
            String::from("mypy"),
            String::from("pyright"),
            String::from("ty"),
        ],
    );
}

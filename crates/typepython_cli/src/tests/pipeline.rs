use super::*;

#[test]
fn run_pipeline_reports_incomplete_public_surface_when_required() {
    let project_dir =
        temp_project_dir("run_pipeline_reports_incomplete_public_surface_when_required");
    let rendered = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[typing]\nrequire_known_public_types = true\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "def leak(value: dynamic) -> int:\n    return 0\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        run_pipeline(&config).expect("test setup should succeed").diagnostics.as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY4015"));
    assert!(rendered.contains("exports incomplete type surface for `leak`"));
}

#[test]
fn run_pipeline_ignores_private_incomplete_surface_when_required() {
    let project_dir =
        temp_project_dir("run_pipeline_ignores_private_incomplete_surface_when_required");
    let diagnostics = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[typing]\nrequire_known_public_types = true\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "def _leak(value: dynamic) -> int:\n    return 0\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        run_pipeline(&config).expect("test setup should succeed").diagnostics
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
}

#[test]
fn run_pipeline_keeps_lowering_when_checker_fails() {
    let project_dir = temp_project_dir("run_pipeline_keeps_lowering_when_checker_fails");
    let snapshot = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return \"oops\"\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        run_pipeline(&config).expect("test setup should succeed")
    };
    remove_temp_project_dir(&project_dir);

    assert!(snapshot.diagnostics.has_errors());
    assert!(!snapshot.emit_blocked_by_pipeline);
    assert_eq!(snapshot.lowered_modules.len(), 1);
    assert_eq!(snapshot.emit_plan.len(), 1);
    assert!(snapshot.tracked_modules > 0);
}

#[test]
fn run_pipeline_blocks_emit_when_lowering_fails_even_if_emit_is_allowed() {
    let project_dir =
        temp_project_dir("run_pipeline_blocks_emit_when_lowering_fails_even_if_emit_is_allowed");
    let snapshot = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nno_emit_on_error = false\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.tpy"),
            "class User(TypedDict):\n    id: int\n\ntypealias UserPublic = Pick[User, \"name\"]\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        run_pipeline(&config).expect("test setup should succeed")
    };
    remove_temp_project_dir(&project_dir);

    assert!(snapshot.diagnostics.has_errors());
    assert!(snapshot.emit_blocked_by_pipeline);
    assert!(snapshot.lowered_modules.is_empty());
    assert!(snapshot.emit_plan.is_empty());
}

#[test]
fn run_build_like_command_emits_outputs_when_checker_fails_and_emit_is_allowed() {
    let project_dir = temp_project_dir(
        "run_build_like_command_emits_outputs_when_checker_fails_and_emit_is_allowed",
    );
    let result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nno_emit_on_error = false\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return \"oops\"\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let runtime_path = project_dir.join(".typepython/build/app.py");
        let stub_path = project_dir.join(".typepython/build/app.pyi");

        let exit_code =
            run_build_like_command(&config, super::OutputFormat::Json, "build", Vec::new())
                .expect("build should run to completion");

        (exit_code, runtime_path.exists(), stub_path.exists())
    };
    remove_temp_project_dir(&project_dir);

    let (exit_code, runtime_exists, stub_exists) = result;
    assert_eq!(exit_code, ExitCode::FAILURE);
    assert!(runtime_exists);
    assert!(stub_exists);
}

#[test]
fn run_build_like_command_skips_py_typed_when_disabled() {
    let project_dir = temp_project_dir("run_build_like_command_skips_py_typed_when_disabled");
    let result = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nwrite_py_typed = false\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("src/app/__init__.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        let exit_code =
            run_build_like_command(&config, super::OutputFormat::Json, "build", Vec::new())
                .expect("build should run to completion");

        (
            exit_code,
            project_dir.join(".typepython/build/app/__init__.py").exists(),
            project_dir.join(".typepython/build/app/__init__.pyi").exists(),
            project_dir.join(".typepython/build/app/py.typed").exists(),
        )
    };
    remove_temp_project_dir(&project_dir);

    let (exit_code, runtime_exists, stub_exists, py_typed_exists) = result;
    assert_eq!(exit_code, ExitCode::SUCCESS);
    assert!(runtime_exists);
    assert!(stub_exists);
    assert!(!py_typed_exists);
}

#[test]
fn run_verify_emits_outputs_when_checker_fails_and_emit_is_allowed() {
    let project_dir =
        temp_project_dir("run_verify_emits_outputs_when_checker_fails_and_emit_is_allowed");
    let result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nno_emit_on_error = false\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return \"oops\"\n")
            .expect("test setup should succeed");

        let verify_result = run_verify(VerifyArgs {
            run: super::RunArgs {
                project: Some(project_dir.clone()),
                format: super::OutputFormat::Json,
            },
            wheels: Vec::new(),
            sdists: Vec::new(),
            checkers: Vec::new(),
        })
        .expect("verify should run to completion");

        (
            verify_result,
            project_dir.join(".typepython/build/app.py").exists(),
            project_dir.join(".typepython/build/app.pyi").exists(),
        )
    };
    remove_temp_project_dir(&project_dir);

    let (verify_result, runtime_exists, stub_exists) = result;
    assert_eq!(verify_result, ExitCode::FAILURE);
    assert!(runtime_exists);
    assert!(stub_exists);
}

#[test]
fn run_pipeline_rejects_conditional_returns_by_default() {
    let project_dir = temp_project_dir("run_pipeline_rejects_conditional_returns_by_default");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.tpy"),
            "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        run_pipeline(&config).expect("test setup should succeed").diagnostics
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.has_errors(), "{}", diagnostics.as_text());
}

#[test]
fn run_pipeline_accepts_conditional_returns_when_enabled() {
    let project_dir = temp_project_dir("run_pipeline_accepts_conditional_returns_when_enabled");
    let diagnostics = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[typing]\nconditional_returns = true\n",
        )
        .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.tpy"),
            "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        run_pipeline(&config).expect("test setup should succeed").diagnostics
    };
    remove_temp_project_dir(&project_dir);

    assert!(!diagnostics.has_errors(), "{}", diagnostics.as_text());
}

#[test]
fn run_pipeline_uses_shadow_stubs_for_local_python_when_infer_passthrough_is_enabled() {
    let project_dir = temp_project_dir(
        "run_pipeline_uses_shadow_stubs_for_local_python_when_infer_passthrough_is_enabled",
    );
    let (with_inference, shadow_stub) = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[typing]\ninfer_passthrough = true\n",
        )
        .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/helpers.py"),
            "class User:\n    def __init__(self):\n        self.age = 3\n\ndef build():\n    return User()\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "from app.helpers import build\n\nuser = build()\nage: int = user.age\n",
        )
        .expect("test setup should succeed");

        let with = {
            let config = load(&project_dir).expect("test setup should succeed");
            run_pipeline(&config).expect("test setup should succeed").diagnostics
        };
        let shadow =
            fs::read_to_string(project_dir.join(".typepython/cache/shadow-stubs/app/helpers.pyi"))
                .expect("shadow stub should be written");

        (with, shadow)
    };
    remove_temp_project_dir(&project_dir);

    assert!(!with_inference.has_errors(), "{}", with_inference.as_text());
    assert!(shadow_stub.contains("def build() -> User: ..."));
    assert!(shadow_stub.contains("age: int"));
}

#[test]
fn run_pipeline_reuses_cached_outputs_when_snapshot_is_unchanged() {
    let project_dir =
        temp_project_dir("run_pipeline_reuses_cached_outputs_when_snapshot_is_unchanged");
    let second = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        let first = run_pipeline(&config).expect("test setup should succeed");
        write_runtime_outputs(
            &first.emit_plan,
            &first.lowered_modules,
            config.config.emit.write_py_typed,
            false,
            Some(&first.stub_contexts),
        )
        .expect("test setup should succeed");
        write_incremental_snapshot(
            &config.resolve_relative_path(&config.config.project.cache_dir),
            &first.incremental,
        )
        .expect("test setup should succeed");

        run_pipeline(&config).expect("test setup should succeed")
    };
    remove_temp_project_dir(&project_dir);

    assert!(second.diagnostics.is_empty());
    assert!(second.lowered_modules.is_empty());
    assert_eq!(second.emit_plan.len(), 1);
}

#[test]
fn run_pipeline_invalidates_cache_when_public_summary_changes() {
    let project_dir =
        temp_project_dir("run_pipeline_invalidates_cache_when_public_summary_changes");
    let second = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        let first = run_pipeline(&config).expect("test setup should succeed");
        write_runtime_outputs(
            &first.emit_plan,
            &first.lowered_modules,
            config.config.emit.write_py_typed,
            false,
            Some(&first.stub_contexts),
        )
        .expect("test setup should succeed");
        write_incremental_snapshot(
            &config.resolve_relative_path(&config.config.project.cache_dir),
            &first.incremental,
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> str:\n    return \"one\"\n")
            .expect("test setup should succeed");

        run_pipeline(&config).expect("test setup should succeed")
    };
    remove_temp_project_dir(&project_dir);

    assert!(second.diagnostics.is_empty());
    assert_eq!(second.lowered_modules.len(), 1);
    assert_eq!(second.emit_plan.len(), 1);
}

#[test]
fn build_diagnostics_adds_emit_blocked_error_when_configured() {
    let project_dir = temp_project_dir("build_diagnostics_adds_emit_blocked_error_when_configured");
    let rendered = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return \"oops\"\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let snapshot = run_pipeline(&config).expect("test setup should succeed");

        build_diagnostics(&config, &snapshot).as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("build"));
    assert!(rendered.contains("TPY5002"));
}

#[test]
fn should_emit_build_outputs_respects_no_emit_on_error() {
    let project_dir = temp_project_dir("should_emit_build_outputs_respects_no_emit_on_error");
    let result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nno_emit_on_error = false\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return \"oops\"\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let snapshot = run_pipeline(&config).expect("test setup should succeed");

        should_emit_build_outputs(&config, &snapshot)
    };
    remove_temp_project_dir(&project_dir);

    assert!(result);
}

#[test]
fn write_incremental_snapshot_persists_fingerprint_json() {
    let project_dir = temp_project_dir("write_incremental_snapshot_persists_fingerprint_json");
    let result = {
        let snapshot_path = write_incremental_snapshot(
            &project_dir.join(".typepython/cache"),
            &IncrementalState {
                fingerprints: std::collections::BTreeMap::from([
                    (String::from("pkg.a"), 10),
                    (String::from("pkg.b"), 20),
                ]),
                summaries: vec![typepython_incremental::PublicSummary {
                    module: String::from("pkg.a"),
                    is_package_entry: false,
                    exports: vec![typepython_incremental::SummaryExport {
                        name: String::from("Foo"),
                        kind: String::from("class"),
                        type_repr: String::from("Foo"),
                        type_expr: None,
                        declaration_signature: None,
                        exported_type: None,
                        exported_type_expr: None,
                        type_params: Vec::new(),
                        public: true,
                    }],
                    imports: vec![String::from("pkg.base")],
                    import_targets: Vec::new(),
                    sealed_roots: vec![typepython_incremental::SealedRootSummary {
                        root: String::from("Expr"),
                        members: vec![String::from("Add"), String::from("Num")],
                    }],
                    solver_facts: typepython_incremental::ModuleSolverFacts::default(),
                }],
                stdlib_snapshot: Some(String::from("fnv1a64:demo")),
            },
        )
        .expect("test setup should succeed");

        (
            snapshot_path,
            fs::read_to_string(project_dir.join(".typepython/cache/snapshot.json"))
                .expect("test setup should succeed"),
        )
    };
    remove_temp_project_dir(&project_dir);

    let (snapshot_path, rendered) = result;
    assert!(snapshot_path.ends_with("snapshot.json"));
    assert!(rendered.contains("pkg.a"));
    assert!(rendered.contains("pkg.b"));
    assert!(rendered.contains("\"exports\""));
    assert!(rendered.contains("\"imports\""));
    assert!(rendered.contains("\"sealedRoots\""));
    assert!(rendered.contains("\"solverFacts\""));
    assert!(rendered.contains("\"declarationSignature\""));
    assert!(rendered.contains("\"exportedType\""));
    assert!(rendered.contains("fnv1a64:demo"));
}

#[test]
fn compile_runtime_bytecode_uses_configured_python_executable() {
    let project_dir =
        temp_project_dir("compile_runtime_bytecode_uses_configured_python_executable");
    let result = {
        fs::create_dir_all(project_dir.join("bin")).expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("out/app")).expect("test setup should succeed");
        let log_path = project_dir.join("compiler.log");
        let fake_python = project_dir.join("bin/fake-python.sh");
        fs::write(
            project_dir.join("typepython.toml"),
            format!(
                "[project]\nsrc = [\"src\"]\n\n[resolution]\npython_executable = \"bin{}fake-python.sh\"\n\n[emit]\nemit_pyc = true\n",
                MAIN_SEPARATOR
            ),
        ).expect("test setup should succeed");
        fs::write(
            &fake_python,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"-c\" ] && printf '%s' \"$2\" | grep -q 'version_info'; then\n  printf '3.10\\n'\n  exit 0\nfi\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
                log_path.display()
            ),
        ).expect("test setup should succeed");
        #[cfg(unix)]
        {
            let mut permissions =
                fs::metadata(&fake_python).expect("test setup should succeed").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&fake_python, permissions).expect("test setup should succeed");
        }
        let config = load(&project_dir).expect("test setup should succeed");
        let artifacts = vec![EmitArtifact {
            source_path: project_dir.join("src/app/__init__.tpy"),
            runtime_path: Some(project_dir.join("out/app/__init__.py")),
            stub_path: None,
        }];
        fs::write(project_dir.join("out/app/__init__.py"), "pass\n")
            .expect("test setup should succeed");

        let compiled =
            compile_runtime_bytecode(&config, &artifacts).expect("test setup should succeed");
        let log = fs::read_to_string(&log_path).expect("test setup should succeed");
        (compiled, log)
    };
    remove_temp_project_dir(&project_dir);

    let (compiled, log) = result;
    assert_eq!(compiled, 1);
    assert!(log.contains("py_compile.compile"));
    assert!(log.contains("__init__.py"));
    assert!(log.contains("__pycache__"));
}


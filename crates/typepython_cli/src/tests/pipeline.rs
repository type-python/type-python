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
fn run_pipeline_emits_native_syntax_for_target_python_313() {
    let runtime_source = {
        let project_dir =
            temp_project_dir("run_pipeline_emits_native_syntax_for_target_python_313");
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "typealias Pair[T = int] = tuple[T, T]\n\nclass Box[T = int]:\n    value: T\n\ndef first[T = int](value: T = 1) -> T:\n    return value\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        let snapshot = run_pipeline(&config).expect("test setup should succeed");
        let runtime_source = snapshot
            .lowered_modules
            .iter()
            .find(|module| module.source_path.ends_with("src/app/__init__.tpy"))
            .map(|module| module.python_source.clone())
            .expect("expected lowered runtime source");
        remove_temp_project_dir(&project_dir);
        runtime_source
    };

    assert!(runtime_source.contains("type Pair[T = int] = tuple[T, T]"));
    assert!(runtime_source.contains("class Box[T = int]:"));
    assert!(runtime_source.contains("def first[T = int](value: T = 1) -> T:"));
    assert!(!runtime_source.contains("TypeVar("));
}

#[test]
fn run_pipeline_respects_target_and_emit_style_matrix_for_generic_output() {
    struct Case<'a> {
        name: &'a str,
        config: &'a str,
        source: &'a str,
        expected_fragments: &'a [&'a str],
        forbidden_fragments: &'a [&'a str],
    }

    let cases = [
        Case {
            name: "default-313-native",
            config: "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n",
            source: "typealias Pair[T = int] = tuple[T, T]\n",
            expected_fragments: &["type Pair[T = int] = tuple[T, T]"],
            forbidden_fragments: &["TypeVar(", "Pair: TypeAlias = tuple[T, T]"],
        },
        Case {
            name: "forced-313-compat",
            config: "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n\n[emit]\nemit_style = \"compat\"\n",
            source: "typealias Pair[T = int] = tuple[T, T]\n",
            expected_fragments: &[
                "TypeVar(\"T\", default=\"int\")",
                "Pair: TypeAlias = tuple[T, T]",
            ],
            forbidden_fragments: &["type Pair[T = int] = tuple[T, T]"],
        },
        Case {
            name: "default-312-compat",
            config: "[project]\nsrc = [\"src\"]\ntarget_python = \"3.12\"\n",
            source: "typealias Pair[T] = tuple[T, T]\n",
            expected_fragments: &["TypeVar(\"T\")", "Pair: TypeAlias = tuple[T, T]"],
            forbidden_fragments: &["type Pair[T] = tuple[T, T]"],
        },
        Case {
            name: "forced-312-native",
            config: "[project]\nsrc = [\"src\"]\ntarget_python = \"3.12\"\n\n[emit]\nemit_style = \"native\"\n",
            source: "typealias Pair[T] = tuple[T, T]\n",
            expected_fragments: &["type Pair[T] = tuple[T, T]"],
            forbidden_fragments: &["TypeVar(\"T\")", "Pair: TypeAlias = tuple[T, T]"],
        },
    ];

    for case in cases {
        let runtime_source = {
            let project_dir = temp_project_dir(case.name);
            fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
            fs::write(project_dir.join("typepython.toml"), case.config)
                .expect("test setup should succeed");
            fs::write(project_dir.join("src/app.tpy"), case.source)
                .expect("test setup should succeed");
            let config = load(&project_dir).expect("test setup should succeed");
            let snapshot = run_pipeline(&config).expect("test setup should succeed");
            let runtime_source = snapshot
                .lowered_modules
                .iter()
                .find(|module| module.source_path.ends_with("src/app.tpy"))
                .map(|module| module.python_source.clone())
                .expect("expected lowered runtime source");
            remove_temp_project_dir(&project_dir);
            runtime_source
        };

        for expected in case.expected_fragments {
            assert!(
                runtime_source.contains(expected),
                "{}: missing expected fragment `{}` in\n{}",
                case.name,
                expected,
                runtime_source
            );
        }
        for forbidden in case.forbidden_fragments {
            assert!(
                !runtime_source.contains(forbidden),
                "{}: found forbidden fragment `{}` in\n{}",
                case.name,
                forbidden,
                runtime_source
            );
        }
    }
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
fn run_with_pipeline_check_persists_analysis_cache_without_materializing_outputs() {
    let project_dir = temp_project_dir(
        "run_with_pipeline_check_persists_analysis_cache_without_materializing_outputs",
    );
    let result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");

        let exit_code = run_with_pipeline(
            "check",
            RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            false,
            Vec::new(),
        )
        .expect("check should run to completion");

        (
            exit_code,
            project_dir.join(".typepython/cache/snapshot.json").exists(),
            project_dir.join(".typepython/cache/analysis-cache.json").exists(),
            project_dir.join(".typepython/cache/build-manifest.json").exists(),
        )
    };
    remove_temp_project_dir(&project_dir);

    let (exit_code, snapshot_exists, analysis_exists, manifest_exists) = result;
    assert_eq!(exit_code, ExitCode::SUCCESS);
    assert!(snapshot_exists);
    assert!(analysis_exists);
    assert!(!manifest_exists);
}

#[test]
fn run_build_like_command_rebuilds_outputs_after_check_updates_semantic_cache() {
    let project_dir = temp_project_dir(
        "run_build_like_command_rebuilds_outputs_after_check_updates_semantic_cache",
    );
    let runtime_source = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let runtime_path = project_dir.join(".typepython/build/app.py");

        run_build_like_command(&config, super::OutputFormat::Json, "build", Vec::new())
            .expect("initial build should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 2\n")
            .expect("test setup should succeed");
        run_with_pipeline(
            "check",
            RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            false,
            Vec::new(),
        )
        .expect("check should update semantic cache");
        run_build_like_command(&config, super::OutputFormat::Json, "build", Vec::new())
            .expect("follow-up build should succeed");

        let runtime = fs::read_to_string(runtime_path).expect("runtime output should exist");
        remove_temp_project_dir(&project_dir);
        runtime
    };

    assert!(runtime_source.contains("return 2"));
    assert!(!runtime_source.contains("return 1"));
}

#[test]
fn run_build_like_command_removes_stale_outputs_for_deleted_modules() {
    let project_dir =
        temp_project_dir("run_build_like_command_removes_stale_outputs_for_deleted_modules");
    let result = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app/__init__.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        run_build_like_command(&config, super::OutputFormat::Json, "build", Vec::new())
            .expect("initial build should succeed");
        fs::remove_file(project_dir.join("src/app/__init__.tpy"))
            .expect("source should be removed");
        fs::write(project_dir.join("src/other.tpy"), "def build() -> int:\n    return 2\n")
            .expect("replacement source should be written");
        run_build_like_command(&config, super::OutputFormat::Json, "build", Vec::new())
            .expect("follow-up build should succeed");

        (
            project_dir.join(".typepython/build/app/__init__.py").exists(),
            project_dir.join(".typepython/build/app/__init__.pyi").exists(),
            project_dir.join(".typepython/build/app/py.typed").exists(),
            project_dir.join(".typepython/build/other.py").exists(),
            project_dir.join(".typepython/build/other.pyi").exists(),
        )
    };
    remove_temp_project_dir(&project_dir);

    let (
        old_runtime_exists,
        old_stub_exists,
        old_marker_exists,
        new_runtime_exists,
        new_stub_exists,
    ) = result;
    assert!(!old_runtime_exists);
    assert!(!old_stub_exists);
    assert!(!old_marker_exists);
    assert!(new_runtime_exists);
    assert!(new_stub_exists);
}

#[test]
fn run_build_like_command_removes_stale_py_typed_when_disabled() {
    let project_dir =
        temp_project_dir("run_build_like_command_removes_stale_py_typed_when_disabled");
    let result = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app/__init__.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        run_build_like_command(&config, super::OutputFormat::Json, "build", Vec::new())
            .expect("initial build should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nwrite_py_typed = false\n",
        )
        .expect("updated config should be written");
        let updated_config = load(&project_dir).expect("updated config should load");
        run_build_like_command(&updated_config, super::OutputFormat::Json, "build", Vec::new())
            .expect("follow-up build should succeed");

        (
            project_dir.join(".typepython/build/app/__init__.py").exists(),
            project_dir.join(".typepython/build/app/__init__.pyi").exists(),
            project_dir.join(".typepython/build/app/py.typed").exists(),
        )
    };
    remove_temp_project_dir(&project_dir);

    let (runtime_exists, stub_exists, marker_exists) = result;
    assert!(runtime_exists);
    assert!(stub_exists);
    assert!(!marker_exists);
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
            unsafe_runtime_imports: false,
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

fn write_chain_workspace(project_dir: &Path, module_count: usize) {
    fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
    fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
        .expect("test setup should succeed");
    fs::write(project_dir.join("src/app/__init__.tpy"), "pass\n")
        .expect("test setup should succeed");

    for index in 0..module_count {
        let module_name = format!("mod_{index:02}");
        let contents = if index == 0 {
            String::from("def produce() -> int:\n    return 1\n")
        } else {
            let previous = format!("mod_{:02}", index - 1);
            format!(
                "from app.{previous} import produce\n\n\
                 def run_{index:02}() -> int:\n    return produce()\n"
            )
        };
        fs::write(project_dir.join(format!("src/app/{module_name}.tpy")), contents)
            .expect("test setup should succeed");
    }
}

fn persist_pipeline_caches(config: &typepython_config::ConfigHandle, snapshot: &PipelineSnapshot) {
    persist_pipeline_analysis_state(config, snapshot).expect("analysis cache should be written");
    materialize_build_outputs(config, snapshot).expect("materialized outputs should be written");
}

#[test]
fn run_pipeline_selectively_lowers_only_changed_module_for_implementation_edits() {
    let project_dir = temp_project_dir(
        "run_pipeline_selectively_lowers_only_changed_module_for_implementation_edits",
    );
    let lowered_modules = {
        write_chain_workspace(&project_dir, 4);
        let config = load(&project_dir).expect("test setup should succeed");
        let first = run_pipeline(&config).expect("test setup should succeed");
        persist_pipeline_caches(&config, &first);

        fs::write(
            project_dir.join("src/app/mod_00.tpy"),
            "def produce() -> int:\n    value = 1\n    return value\n",
        )
        .expect("test setup should succeed");
        let second = run_pipeline(&config).expect("test setup should succeed");
        let lowered = second
            .lowered_modules
            .iter()
            .map(|module| {
                module
                    .source_path
                    .strip_prefix(&project_dir)
                    .expect("lowered module should stay under the temporary project directory")
                    .to_path_buf()
            })
            .collect::<Vec<_>>();
        remove_temp_project_dir(&project_dir);
        lowered
    };

    assert_eq!(lowered_modules, vec![PathBuf::from("src/app/mod_00.tpy")]);
}

#[test]
fn run_pipeline_selectively_lowers_transitive_dependents_for_public_edits() {
    let project_dir =
        temp_project_dir("run_pipeline_selectively_lowers_transitive_dependents_for_public_edits");
    let lowered_modules = {
        write_chain_workspace(&project_dir, 4);
        let config = load(&project_dir).expect("test setup should succeed");
        let first = run_pipeline(&config).expect("test setup should succeed");
        persist_pipeline_caches(&config, &first);

        fs::write(
            project_dir.join("src/app/mod_00.tpy"),
            "def produce() -> str:\n    return \"value\"\n",
        )
        .expect("test setup should succeed");
        let second = run_pipeline(&config).expect("test setup should succeed");
        let mut lowered = second
            .lowered_modules
            .iter()
            .map(|module| {
                module
                    .source_path
                    .strip_prefix(&project_dir)
                    .expect("lowered module should stay under the temporary project directory")
                    .to_path_buf()
            })
            .collect::<Vec<_>>();
        lowered.sort();
        remove_temp_project_dir(&project_dir);
        lowered
    };

    assert_eq!(
        lowered_modules,
        vec![
            PathBuf::from("src/app/__init__.tpy"),
            PathBuf::from("src/app/mod_00.tpy"),
            PathBuf::from("src/app/mod_01.tpy"),
            PathBuf::from("src/app/mod_02.tpy"),
            PathBuf::from("src/app/mod_03.tpy"),
        ]
    );
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
        materialize_build_outputs(&config, &first).expect("test setup should succeed");

        run_pipeline(&config).expect("test setup should succeed")
    };
    remove_temp_project_dir(&project_dir);

    assert!(second.diagnostics.is_empty());
    assert!(second.lowered_modules.is_empty());
    assert_eq!(second.emit_plan.len(), 1);
}

#[test]
fn run_pipeline_invalidates_cache_when_emit_style_changes() {
    let (lowered_modules, runtime_source) = {
        let project_dir =
            temp_project_dir("run_pipeline_invalidates_cache_when_emit_style_changes");
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "typealias Pair[T = int] = tuple[T, T]\n")
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

        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n\n[emit]\nemit_style = \"compat\"\n",
        )
        .expect("test setup should succeed");
        let compat_config = load(&project_dir).expect("test setup should succeed");
        let second = run_pipeline(&compat_config).expect("test setup should succeed");
        let runtime_source = second
            .lowered_modules
            .iter()
            .find(|module| module.source_path.ends_with("src/app.tpy"))
            .map(|module| module.python_source.clone())
            .expect("expected rebuilt runtime source");
        let result = (second.lowered_modules.len(), runtime_source);
        remove_temp_project_dir(&project_dir);
        result
    };

    assert_eq!(lowered_modules, 1);
    assert!(runtime_source.contains("TypeVar(\"T\", default=\"int\")"));
    assert!(runtime_source.contains("Pair: TypeAlias = tuple[T, T]"));
}

#[test]
fn run_pipeline_invalidates_cache_when_analysis_python_changes() {
    let lowered_modules = {
        let project_dir =
            temp_project_dir("run_pipeline_invalidates_cache_when_analysis_python_changes");
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n\n[resolution]\nanalysis_python = \"3.13\"\n",
        )
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

        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n\n[resolution]\nanalysis_python = \"3.14\"\n",
        )
        .expect("test setup should succeed");
        let updated_config = load(&project_dir).expect("test setup should succeed");
        let second = run_pipeline(&updated_config).expect("test setup should succeed");
        let lowered_modules = second.lowered_modules.len();
        remove_temp_project_dir(&project_dir);
        lowered_modules
    };

    assert_eq!(lowered_modules, 1);
}

#[test]
fn run_pipeline_invalidates_cache_when_runtime_validators_change() {
    let (lowered_modules, runtime_source) = {
        let project_dir =
            temp_project_dir("run_pipeline_invalidates_cache_when_runtime_validators_change");
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.tpy"),
            "data class UserInput:\n    name: str\n    age: int\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let runtime_path = project_dir.join(".typepython/build/app.py");

        let first = run_pipeline(&config).expect("test setup should succeed");
        materialize_build_outputs(&config, &first).expect("test setup should succeed");

        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nruntime_validators = true\n",
        )
        .expect("test setup should succeed");
        let updated_config = load(&project_dir).expect("test setup should succeed");
        let second = run_pipeline(&updated_config).expect("test setup should succeed");
        materialize_build_outputs(&updated_config, &second).expect("test setup should succeed");
        let result = (
            second.lowered_modules.len(),
            fs::read_to_string(runtime_path).expect("runtime output should exist"),
        );
        remove_temp_project_dir(&project_dir);
        result
    };

    assert_eq!(lowered_modules, 1);
    assert!(runtime_source.contains("__tpy_validate__"));
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
fn run_pipeline_invalidates_cache_when_source_hash_changes_without_public_summary_change() {
    let project_dir = temp_project_dir(
        "run_pipeline_invalidates_cache_when_source_hash_changes_without_public_summary_change",
    );
    let (diagnostics, lowered_modules, planned_artifacts, runtime_source) = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let runtime_path = project_dir.join(".typepython/build/app.py");

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

        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 2\n")
            .expect("test setup should succeed");

        let second = run_pipeline(&config).expect("test setup should succeed");
        write_runtime_outputs(
            &second.emit_plan,
            &second.lowered_modules,
            config.config.emit.write_py_typed,
            false,
            Some(&second.stub_contexts),
        )
        .expect("test setup should succeed");

        (
            second.diagnostics,
            second.lowered_modules.len(),
            second.emit_plan.len(),
            fs::read_to_string(runtime_path).expect("runtime output should exist"),
        )
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
    assert_eq!(lowered_modules, 1);
    assert_eq!(planned_artifacts, 1);
    assert!(runtime_source.contains("return 2"));
    assert!(!runtime_source.contains("return 1"));
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
                source_hashes: std::collections::BTreeMap::new(),
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
                        runtime_semantics: None,
                        required_runtime_features: Vec::new(),
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
                metadata: typepython_incremental::SnapshotMetadata::default(),
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
    assert!(rendered.contains("\"source_hashes\""));
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

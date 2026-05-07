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
fn run_pipeline_honors_experimental_shape_transform_gate() {
    let project_dir = temp_project_dir("run_pipeline_honors_experimental_shape_transform_gate");
    let runtime_source = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[experimental]\naccepted_features = [\"shape_transforms\"]\nshape_transforms = true\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.tpy"),
            concat!(
                "data class User:\n",
                "    id: int\n",
                "    name: str\n\n",
                "typealias UserPatch = Partial[User]\n",
            ),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let snapshot = run_pipeline(&config).expect("pipeline should run");
        assert!(!snapshot.diagnostics.has_errors(), "{}", snapshot.diagnostics.as_text());
        let runtime_source = snapshot
            .lowered_modules
            .iter()
            .find(|module| module.source_path.ends_with("src/app.tpy"))
            .map(|module| module.python_source.clone())
            .expect("expected lowered runtime source");
        remove_temp_project_dir(&project_dir);
        runtime_source
    };

    assert!(runtime_source.contains("class UserPatch(TypedDict):"), "{runtime_source}");
    assert!(runtime_source.contains("id: NotRequired[int]"), "{runtime_source}");
    assert!(runtime_source.contains("name: NotRequired[str]"), "{runtime_source}");
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
            project_dir.join(".typepython/cache/effects.json").exists(),
            project_dir.join(".typepython/cache/build-manifest.json").exists(),
        )
    };
    remove_temp_project_dir(&project_dir);

    let (exit_code, snapshot_exists, analysis_exists, effects_exists, manifest_exists) = result;
    assert_eq!(exit_code, ExitCode::SUCCESS);
    assert!(snapshot_exists);
    assert!(analysis_exists);
    assert!(effects_exists);
    assert!(!manifest_exists);
}

#[test]
fn run_with_pipeline_check_treats_old_analysis_cache_as_miss() {
    let project_dir = temp_project_dir("run_with_pipeline_check_treats_old_analysis_cache_as_miss");
    let result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");

        let first_exit_code = run_with_pipeline(
            "check",
            RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            false,
            Vec::new(),
        )
        .expect("first check should run to completion");

        let cache_path = project_dir.join(".typepython/cache/analysis-cache.json");
        let mut cache: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cache_path).expect("cache should exist"))
                .expect("cache should be valid JSON");
        let metadata = cache
            .get_mut("metadata")
            .and_then(serde_json::Value::as_object_mut)
            .expect("cache metadata should be an object");
        metadata.remove("strict_nulls");
        metadata.remove("no_implicit_dynamic");
        fs::write(&cache_path, serde_json::to_string_pretty(&cache).expect("cache renders"))
            .expect("test setup should succeed");

        let second_exit_code = run_with_pipeline(
            "check",
            RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            false,
            Vec::new(),
        )
        .expect("second check should rebuild stale analysis cache");

        (first_exit_code, second_exit_code)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(result, (ExitCode::SUCCESS, ExitCode::SUCCESS));
}

#[test]
fn run_with_pipeline_check_treats_incompatible_incremental_snapshot_as_miss() {
    let project_dir = temp_project_dir(
        "run_with_pipeline_check_treats_incompatible_incremental_snapshot_as_miss",
    );
    let result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");

        let first_exit_code = run_with_pipeline(
            "check",
            RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            false,
            Vec::new(),
        )
        .expect("first check should run to completion");

        let snapshot_path = project_dir.join(".typepython/cache/snapshot.json");
        let mut snapshot: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&snapshot_path).expect("snapshot should exist"),
        )
        .expect("snapshot should be valid JSON");
        snapshot["schema_version"] = serde_json::json!(0);
        fs::write(
            &snapshot_path,
            serde_json::to_string_pretty(&snapshot).expect("snapshot renders"),
        )
        .expect("test setup should succeed");

        let second_exit_code = run_with_pipeline(
            "check",
            RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            false,
            Vec::new(),
        )
        .expect("second check should rebuild stale incremental snapshot");

        (first_exit_code, second_exit_code)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(result, (ExitCode::SUCCESS, ExitCode::SUCCESS));
}

#[test]
fn run_with_pipeline_check_persists_effect_metadata_sidecar() {
    let project_dir = temp_project_dir("run_with_pipeline_check_persists_effect_metadata_sidecar");
    let result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[experimental]\naccepted_features = [\"effect_rows\"]\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.tpy"),
            concat!(
                "from typing import Callable\n\n",
                "def effect_io_net[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
                "    return fn\n\n",
                "@effect_io_net\n",
                "def fetch() -> str:\n",
                "    return \"ok\"\n",
            ),
        )
        .expect("test setup should succeed");

        let exit_code = run_with_pipeline(
            "check",
            RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            false,
            Vec::new(),
        )
        .expect("check should run to completion");
        let rendered = fs::read_to_string(project_dir.join(".typepython/cache/effects.json"))
            .expect("effect sidecar should be written");

        (exit_code, rendered)
    };
    remove_temp_project_dir(&project_dir);

    let (exit_code, rendered) = result;
    assert_eq!(exit_code, ExitCode::SUCCESS);
    assert!(rendered.contains("\"schema_version\""), "{rendered}");
    assert!(rendered.contains("\"module\": \"app\""), "{rendered}");
    assert!(rendered.contains("\"name\": \"fetch\""), "{rendered}");
    assert!(rendered.contains("\"effects\""), "{rendered}");
    assert!(rendered.contains("io.net"), "{rendered}");
}

#[test]
fn run_pipeline_suppresses_effect_rows_without_experimental_acceptance() {
    let project_dir =
        temp_project_dir("run_pipeline_suppresses_effect_rows_without_experimental_acceptance");
    let rendered = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[typing]\nstrict = true\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.tpy"),
            concat!(
                "from typing import Callable\n\n",
                "def effect(label: str):\n",
                "    def wrap[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
                "        return fn\n",
                "    return wrap\n\n",
                "def effect_pure[**P, R](fn: Callable[P, R]) -> Callable[P, R]:\n",
                "    return fn\n\n",
                "@effect(\"io.net\")\n",
                "def fetch() -> str:\n",
                "    return \"ok\"\n\n",
                "@effect_pure\n",
                "def parse() -> str:\n",
                "    return fetch()\n",
            ),
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        run_pipeline(&config).expect("check should run to completion").diagnostics.as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(!rendered.contains("TPY4026"), "{rendered}");
    assert!(!rendered.contains("effect row"), "{rendered}");
}

#[test]
fn run_with_pipeline_check_accepts_research_roadmap_demo() {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let example_dir = workspace_root.join("examples/research-roadmap-demo");
    let project_dir = temp_project_dir("run_with_pipeline_check_accepts_research_roadmap_demo");
    let result = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            fs::read_to_string(example_dir.join("typepython.toml"))
                .expect("research roadmap demo config should exist"),
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            fs::read_to_string(example_dir.join("src/app/__init__.tpy"))
                .expect("research roadmap demo source should exist"),
        )
        .expect("test setup should succeed");

        let exit_code = run_with_pipeline(
            "check",
            RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            false,
            Vec::new(),
        )
        .expect("check should run to completion");
        let rendered = fs::read_to_string(project_dir.join(".typepython/cache/effects.json"))
            .expect("effect sidecar should be written");

        (exit_code, rendered)
    };
    remove_temp_project_dir(&project_dir);

    let (exit_code, rendered) = result;
    assert_eq!(exit_code, ExitCode::SUCCESS);
    assert!(rendered.contains("\"module\": \"app\""), "{rendered}");
    assert!(rendered.contains("\"name\": \"load_user\""), "{rendered}");
    assert!(rendered.contains("io.net"), "{rendered}");
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
            api_diff_old: None,
            checkers: Vec::new(),
            checker_preset: None,
            checker_allowlist: None,
            unsafe_runtime_imports: false,
            publication_type_health: false,
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
            "[project]\nsrc = [\"src\"]\n\n[typing]\nconditional_returns = true\n\n[experimental]\naccepted_features = [\"conditional_returns\"]\n",
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
            "[project]\nsrc = [\"src\"]\n\n[typing]\ninfer_passthrough = true\n\n[experimental]\naccepted_features = [\"infer_passthrough\"]\n",
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
fn materialized_build_manifest_records_output_affecting_config() {
    let manifest = {
        let project_dir =
            temp_project_dir("materialized_build_manifest_records_output_affecting_config");
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            concat!(
                "[project]\n",
                "src = [\"src\"]\n",
                "target_python = \"3.13\"\n\n",
                "[emit]\n",
                "emit_style = \"compat\"\n",
                "emit_pyi = false\n",
                "write_py_typed = false\n",
            ),
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        let snapshot = run_pipeline(&config).expect("test setup should succeed");
        materialize_build_outputs(&config, &snapshot).expect("test setup should succeed");
        let manifest =
            fs::read_to_string(project_dir.join(".typepython/cache/build-manifest.json"))
                .expect("manifest should exist");
        remove_temp_project_dir(&project_dir);
        manifest
    };
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest).expect("manifest should be JSON");
    let output_config = &manifest["output_config"];

    assert_eq!(manifest["schema_version"].as_u64(), Some(3));
    assert_eq!(output_config["target_python"].as_str(), Some("3.13"));
    assert_eq!(output_config["emit_style"].as_str(), Some("compat"));
    assert_eq!(output_config["emit_pyi"].as_bool(), Some(false));
    assert_eq!(output_config["write_py_typed"].as_bool(), Some(false));
    assert_eq!(output_config["runtime_validators"].as_bool(), Some(false));
}

#[test]
fn run_pipeline_invalidates_materialized_outputs_when_emit_pyi_changes() {
    let (lowered_modules, stub_exists_after_rebuild, manifest) = {
        let project_dir =
            temp_project_dir("run_pipeline_invalidates_materialized_outputs_when_emit_pyi_changes");
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.13\"\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let stub_path = project_dir.join(".typepython/build/app.pyi");

        let first = run_pipeline(&config).expect("test setup should succeed");
        persist_pipeline_analysis_state(&config, &first).expect("test setup should succeed");
        materialize_build_outputs(&config, &first).expect("test setup should succeed");
        assert!(stub_path.exists(), "initial build should write a stub");

        fs::write(
            project_dir.join("typepython.toml"),
            concat!(
                "[project]\n",
                "src = [\"src\"]\n",
                "target_python = \"3.13\"\n\n",
                "[emit]\n",
                "emit_pyi = false\n",
            ),
        )
        .expect("test setup should succeed");
        let updated_config = load(&project_dir).expect("test setup should succeed");
        let second = run_pipeline(&updated_config).expect("test setup should succeed");
        materialize_build_outputs(&updated_config, &second).expect("test setup should succeed");
        let manifest =
            fs::read_to_string(project_dir.join(".typepython/cache/build-manifest.json"))
                .expect("manifest should exist");
        let result = (second.lowered_modules.len(), stub_path.exists(), manifest);
        remove_temp_project_dir(&project_dir);
        result
    };
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest).expect("manifest should be JSON");

    assert_eq!(lowered_modules, 1);
    assert!(!stub_exists_after_rebuild);
    assert_eq!(manifest["output_config"]["emit_pyi"].as_bool(), Some(false));
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

#[cfg(unix)]
#[test]
fn run_pipeline_skips_support_probe_without_external_imports() {
    let project_dir = temp_project_dir("run_pipeline_skips_support_probe_without_external_imports");
    let (probe_log, support_snapshot) = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("site-packages/demo"))
            .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo/__init__.pyi"), "value: int\n")
            .expect("test setup should succeed");
        let probe = project_dir.join("python-probe");
        let probe_log = project_dir.join("probe.log");
        write_executable_script(
            &probe,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$2\" >> \"{}\"\nif [ \"$1\" = \"-c\" ] && printf '%s' \"$2\" | grep -q version_info; then\n  printf '3.10\\n'\nelse\n  printf '[]\\n'\nfi\n",
                probe_log.display()
            ),
        );
        fs::write(
            project_dir.join("typepython.toml"),
            format!(
                "[project]\nsrc = [\"src\"]\n\n[resolution]\ntype_roots = [\"{}\"]\npython_executable = \"{}\"\n",
                project_dir.join("site-packages").display(),
                probe.display()
            ),
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        fs::write(&probe_log, "").expect("probe log should reset after config validation");

        let snapshot = run_pipeline(&config).expect("test setup should succeed");
        let probe_log_contents = fs::read_to_string(&probe_log).unwrap_or_default();
        let support_snapshot = snapshot.incremental.metadata.support_snapshot;
        remove_temp_project_dir(&project_dir);
        (probe_log_contents, support_snapshot)
    };

    assert_eq!(support_snapshot, None);
    assert!(
        probe_log.trim().is_empty(),
        "pipeline should not probe external support roots without external imports; got {probe_log:?}"
    );
}

#[cfg(unix)]
#[test]
fn run_pipeline_skips_external_probe_for_stdlib_only_imports() {
    let project_dir = temp_project_dir("run_pipeline_skips_external_probe_for_stdlib_only_imports");
    let (probe_log, support_snapshot) = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("site-packages/demo"))
            .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo/__init__.pyi"), "value: int\n")
            .expect("test setup should succeed");
        let probe = project_dir.join("python-probe");
        let probe_log = project_dir.join("probe.log");
        write_executable_script(
            &probe,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$2\" >> \"{}\"\nif [ \"$1\" = \"-c\" ] && printf '%s' \"$2\" | grep -q version_info; then\n  printf '3.10\\n'\nelse\n  printf '[]\\n'\nfi\n",
                probe_log.display()
            ),
        );
        fs::write(
            project_dir.join("typepython.toml"),
            format!(
                "[project]\nsrc = [\"src\"]\n\n[resolution]\ntype_roots = [\"{}\"]\npython_executable = \"{}\"\n",
                project_dir.join("site-packages").display(),
                probe.display()
            ),
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.tpy"),
            "from typing import Callable\n\ndef build(callback: Callable[[], int]) -> int:\n    return callback()\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        fs::write(&probe_log, "").expect("probe log should reset after config validation");

        let snapshot = run_pipeline(&config).expect("test setup should succeed");
        let probe_log_contents = fs::read_to_string(&probe_log).unwrap_or_default();
        let support_snapshot = snapshot.incremental.metadata.support_snapshot;
        remove_temp_project_dir(&project_dir);
        (probe_log_contents, support_snapshot)
    };

    assert_eq!(support_snapshot, None);
    assert!(
        probe_log.trim().is_empty(),
        "stdlib-only support imports must not probe external support roots; got {probe_log:?}"
    );
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
            "[project]\nsrc = [\"src\"]\n\n[emit]\nruntime_validators = true\n\n[experimental]\naccepted_features = [\"runtime_validators\"]\n",
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

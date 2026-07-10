pub(super) use super::adapter::run_adapter_validate;
pub(super) use super::api_diff::{diff_api_surfaces, run_api_diff};
pub(super) use super::compat::{compat_checker_allowlist, expand_compat_profile, run_compat};
pub(super) use super::discovery::{
    ExternalSupportRoot, bundled_stdlib_snapshot_identity_for_root,
    bundled_stdlib_sources_for_root, collect_source_paths, external_resolution_sources,
    python_type_roots_from_interpreter,
};
pub(super) use super::migration::{
    MigrationBudgetBaseline, MigrationPublicTypeDebtEntry, build_migration_budget_baseline,
    build_migration_diagnostic_baseline, build_migration_report, compare_migration_budget_baseline,
    compare_migration_diagnostic_baseline, emit_migration_stubs, migration_ci_annotations,
    migration_sarif, migration_trend_entry, run_migrate,
};
pub(super) use super::pipeline::{
    PipelineSnapshot, build_diagnostics, clean_project, compile_runtime_bytecode,
    format_watch_rebuild_note, load_syntax_trees, materialize_build_outputs,
    persist_pipeline_analysis_state, run_build_like_command, run_pipeline, run_with_pipeline,
    should_emit_build_outputs, validated_clean_target, watch_targets, write_incremental_snapshot,
};
pub(super) use super::type_health::{build_type_health_report_for_target, run_type_health};
pub(super) use super::verification::inspect_supplied_archive_paths;
pub(super) use super::verification::prepend_pythonpath;
pub(super) use super::verification::{
    CheckerAllowlistEntry, SuppliedArtifactKind, SuppliedVerifyArtifact, TypePortabilityReport,
    allowlisted_checker_diagnostic, expand_checker_list, external_checker_invocation,
    pep561_readiness_report, publication_type_health_diagnostics, run_verify,
    runtime_annotation_compatibility_diagnostics, stub_portability_diagnostics,
    supplied_verify_artifacts, type_portability_report, type_portability_score,
    validate_checker_allowlist_entry, verify_build_artifacts, verify_checker_invocations,
    verify_emitted_declaration_surface, verify_packaged_artifacts, verify_publication_metadata,
    verify_runtime_public_name_parity, verify_runtime_public_name_parity_for_artifact,
};
pub(super) use super::{
    CLI_JSON_SCHEMA_VERSION, Cli, Command, InitArgs, OutputFormat, RunArgs, bytecode_path_for,
    embedded_config_template, exit_code_for_error, init_project, plan_watch_target_update,
    project_start_dir, render_error_json, requested_output_format, run_watch_rebuild,
};
pub(super) use crate::cli::{
    AdapterValidateArgs, ApiDiffArgs, CleanArgs, CompatArgs, MigrateArgs, TypeHealthArgs,
    VerifyArgs,
};
pub(super) use clap::Parser;
pub(super) use flate2::{Compression, write::GzEncoder};
pub(super) use notify::RecursiveMode;
#[cfg(unix)]
pub(super) use std::os::unix::fs::PermissionsExt;
pub(super) use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::MAIN_SEPARATOR,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};
pub(super) use typepython_binding::bind;
pub(super) use typepython_checking::check as check_graph;
pub(super) use typepython_config::load;
pub(super) use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span};
pub(super) use typepython_emit::{EmitArtifact, write_runtime_outputs};
pub(super) use typepython_graph::build as build_graph;
pub(super) use typepython_incremental::IncrementalState;
pub(super) use typepython_target::PythonTarget;
pub(super) use zip::{CompressionMethod, ZipWriter, write::FileOptions};

#[test]
fn early_cli_error_json_uses_the_versioned_envelope() {
    let rendered = render_error_json("command", "invalid project configuration", 1);
    let payload: serde_json::Value =
        serde_json::from_str(&rendered).expect("error output should be valid JSON");

    assert_eq!(payload["schema_version"], serde_json::json!(CLI_JSON_SCHEMA_VERSION));
    assert!(payload["summary"].is_null());
    assert_eq!(payload["diagnostics"], serde_json::json!({ "diagnostics": [] }));
    assert_eq!(payload["error"]["kind"], "command");
    assert_eq!(payload["error"]["message"], "invalid project configuration");
    assert_eq!(payload["error"]["exit_code"], 1);
}

#[test]
fn malformed_cli_still_detects_an_explicit_json_format() {
    assert_eq!(
        requested_output_format(["typepython", "check", "--format", "json", "--unknown"]),
        OutputFormat::Json
    );
    assert_eq!(
        requested_output_format(["typepython", "check", "--format=json", "--unknown"]),
        OutputFormat::Json
    );
    assert_eq!(requested_output_format(["typepython", "check", "--unknown"]), OutputFormat::Text);
}

#[test]
fn relative_project_paths_resolve_to_absolute_directories() {
    let current_dir = env::current_dir().expect("current directory should resolve");
    let project_dir =
        current_dir.join(format!(".typepython/relative-project-path-test-{}", std::process::id()));
    fs::create_dir_all(&project_dir).expect("test project directory should be created");
    let config_path = project_dir.join("typepython.toml");
    fs::write(&config_path, "[project]\nsrc = [\"src\"]\n")
        .expect("test configuration should be written");
    let relative_config = config_path
        .strip_prefix(&current_dir)
        .expect("test path should be under the current directory")
        .to_path_buf();
    let relative_project = project_dir
        .strip_prefix(&current_dir)
        .expect("test directory should be under the current directory")
        .to_path_buf();

    let resolved = project_start_dir(Some(&relative_config)).expect("project path should resolve");
    let resolved_directory =
        project_start_dir(Some(&relative_project)).expect("project directory should resolve");

    assert!(resolved.is_absolute());
    assert_eq!(resolved, project_dir);
    assert_eq!(resolved_directory, resolved);
    fs::remove_dir_all(&project_dir).expect("test project directory should be removed");
}

#[cfg(unix)]
#[test]
fn project_start_dir_preserves_symlink_workspace_identity() {
    let root = temp_project_dir("project_start_dir_preserves_symlink_workspace_identity");
    let real_project = root.join("real-project");
    let linked_project = root.join("linked-project");
    fs::create_dir_all(&real_project).expect("real project directory should be created");
    fs::write(real_project.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
        .expect("test configuration should be written");
    std::os::unix::fs::symlink(&real_project, &linked_project)
        .expect("project symlink should be created");

    let resolved =
        project_start_dir(Some(&linked_project)).expect("symlink project path should resolve");

    assert_eq!(resolved, linked_project);
    assert_ne!(
        resolved,
        fs::canonicalize(&real_project).expect("real project should canonicalize")
    );
    remove_temp_project_dir(&root);
}

#[test]
fn collect_source_paths_includes_implicit_namespace_packages() {
    let project_dir = temp_project_dir("includes_implicit_namespace_packages");
    let result = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("src/pkg/subpkg")).expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/__init__.tpy"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/subpkg/mod.tpy"), "pass\n")
            .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        collect_source_paths(&config)
    };
    remove_temp_project_dir(&project_dir);

    let discovery = result.expect("test setup should succeed");
    assert!(discovery.diagnostics.is_empty());
    let logical_modules: Vec<_> =
        discovery.sources.iter().map(|source| source.logical_module.as_str()).collect();
    assert_eq!(logical_modules, vec!["pkg", "pkg.subpkg.mod"]);
}

#[test]
fn exit_code_for_config_errors_returns_one() {
    let error = anyhow::Error::new(typepython_config::ConfigError::NotFound(PathBuf::from(
        "/tmp/typepython-missing-project",
    )));

    assert_eq!(exit_code_for_error(&error), ExitCode::from(1));
}

#[test]
fn exit_code_for_internal_errors_returns_two() {
    let error = anyhow::anyhow!("unexpected internal compiler failure");

    assert_eq!(exit_code_for_error(&error), ExitCode::from(2));
}

#[test]
fn embedded_config_template_rewrites_sections_under_tool_typepython() {
    let rendered = embedded_config_template();

    assert!(rendered.contains("[tool.typepython.project]"));
    assert!(rendered.contains("[tool.typepython.typing]"));
    assert!(rendered.contains("[tool.typepython.emit]"));
    assert!(!rendered.contains("\n[project]\n"));
}

#[test]
fn packaged_versions_stay_in_sync() {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pyproject =
        fs::read_to_string(workspace_root.join("pyproject.toml")).expect("pyproject should exist");
    let package_init = fs::read_to_string(workspace_root.join("typepython/__init__.py"))
        .expect("typepython package should exist");
    let cargo_lock =
        fs::read_to_string(workspace_root.join("Cargo.lock")).expect("Cargo.lock should exist");
    let version = env!("CARGO_PKG_VERSION");

    assert!(pyproject.contains(&format!("version = \"{version}\"")));
    assert!(package_init.contains(&format!("__version__ = \"{version}\"")));
    for package in [
        "typepython-binding",
        "typepython-checking",
        "typepython-cli",
        "typepython-config",
        "typepython-diagnostics",
        "typepython-emit",
        "typepython-graph",
        "typepython-incremental",
        "typepython-lowering",
        "typepython-lsp",
        "typepython-project",
        "typepython-syntax",
    ] {
        assert!(cargo_lock.contains(&format!("name = \"{package}\"\nversion = \"{version}\"")));
    }
}

#[test]
fn init_project_embeds_config_into_existing_pyproject() {
    let project_dir = temp_project_dir("init_project_embeds_config_into_existing_pyproject");
    fs::write(project_dir.join("pyproject.toml"), "[build-system]\nrequires = [\"setuptools\"]\n")
        .expect("pyproject.toml should be written");

    let init_result = init_project(super::InitArgs {
        dir: project_dir.clone(),
        force: false,
        embed_pyproject: true,
    });

    let pyproject =
        fs::read_to_string(project_dir.join("pyproject.toml")).expect("pyproject should exist");
    let typepython_toml_exists = project_dir.join("typepython.toml").exists();
    let source_exists = project_dir.join("src/app/__init__.tpy").exists();
    remove_temp_project_dir(&project_dir);

    assert_eq!(init_result.expect("init should succeed"), ExitCode::SUCCESS);
    assert!(pyproject.contains("[tool.typepython.project]"));
    assert!(pyproject.contains("[tool.typepython.typing]"));
    assert!(!typepython_toml_exists);
    assert!(source_exists);
}

#[test]
fn init_project_rejects_embed_without_existing_pyproject() {
    let project_dir = temp_project_dir("init_project_rejects_embed_without_existing_pyproject");

    let init_result = init_project(super::InitArgs {
        dir: project_dir.clone(),
        force: false,
        embed_pyproject: true,
    });

    remove_temp_project_dir(&project_dir);

    let error = init_result.expect_err("embed should require an existing pyproject");
    assert!(error.to_string().contains("--embed-pyproject requires an existing pyproject.toml"));
}

#[test]
fn init_project_rejects_embed_when_tool_typepython_already_exists() {
    let project_dir =
        temp_project_dir("init_project_rejects_embed_when_tool_typepython_already_exists");
    fs::write(
        project_dir.join("pyproject.toml"),
        "[tool.typepython.project]\ntarget_python = \"3.10\"\n",
    )
    .expect("pyproject.toml should be written");

    let init_result = init_project(super::InitArgs {
        dir: project_dir.clone(),
        force: false,
        embed_pyproject: true,
    });

    let source_exists = project_dir.join("src/app/__init__.tpy").exists();
    remove_temp_project_dir(&project_dir);

    let error = init_result.expect_err("duplicate embedded config should be rejected");
    assert!(error.to_string().contains("already defines [tool.typepython] configuration"));
    assert!(!source_exists);
}

#[test]
fn collect_source_paths_respects_include_and_exclude_patterns() {
    let project_dir = temp_project_dir("respects_include_and_exclude_patterns");
    let result = {
        fs::write(
            project_dir.join("typepython.toml"),
            concat!(
                "[project]\n",
                "src = [\"src\"]\n",
                "include = [\"src/**/*.tpy\"]\n",
                "exclude = [\"src/pkg/excluded/**\"]\n"
            ),
        )
        .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("src/pkg/excluded"))
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/__init__.tpy"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/kept.tpy"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/excluded/__init__.tpy"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/excluded/hidden.tpy"), "pass\n")
            .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        collect_source_paths(&config)
    };
    remove_temp_project_dir(&project_dir);

    let discovery = result.expect("test setup should succeed");
    let logical_modules: Vec<_> =
        discovery.sources.iter().map(|source| source.logical_module.as_str()).collect();
    assert_eq!(logical_modules, vec!["pkg", "pkg.kept"]);
}

#[test]
fn collect_source_paths_rejects_invalid_include_glob_patterns() {
    let project_dir = temp_project_dir("rejects_invalid_include_glob_patterns");
    let result = {
        fs::write(
            project_dir.join("typepython.toml"),
            concat!("[project]\n", "src = [\"src\"]\n", "include = [\"src/[*.tpy\"]\n"),
        )
        .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        collect_source_paths(&config)
    };
    remove_temp_project_dir(&project_dir);

    let error = result.expect_err("expected invalid include glob to fail");
    let message = error.to_string();
    assert!(message.contains("TPY1002"));
    assert!(message.contains("project.include"));
    assert!(message.contains("invalid glob pattern"));
}

#[cfg(unix)]
#[test]
fn external_resolution_merges_partial_stub_packages_with_runtime_fallback() {
    let project_dir =
        temp_project_dir("external_resolution_merges_partial_stub_packages_with_runtime_fallback");
    let modules = {
        let probe = project_dir.join("python-probe");
        write_executable_script(
            &probe,
            "#!/bin/sh\nif [ \"$1\" = \"-c\" ] && printf '%s' \"$2\" | grep -q version_info; then\n  printf '3.10\\n'\nelse\n  printf '[]\\n'\nfi\n",
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
        fs::create_dir_all(project_dir.join("site-packages/demo"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("site-packages/demo-stubs/demo"))
            .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo/runtime_only.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo-stubs/demo/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo-stubs/demo/typed_only.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo-stubs/py.typed"), "partial\n")
            .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        let sources = external_resolution_sources(&config).expect("test setup should succeed");
        let mut modules =
            sources.into_iter().map(|source| source.logical_module).collect::<Vec<_>>();
        modules.sort();
        modules
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(modules, vec!["demo", "demo.runtime_only", "demo.typed_only"]);
}

#[cfg(unix)]
#[test]
fn external_resolution_does_not_fallback_for_non_partial_stub_packages() {
    let project_dir =
        temp_project_dir("external_resolution_does_not_fallback_for_non_partial_stub_packages");
    let modules = {
        let probe = project_dir.join("python-probe");
        write_executable_script(
            &probe,
            "#!/bin/sh\nif [ \"$1\" = \"-c\" ] && printf '%s' \"$2\" | grep -q version_info; then\n  printf '3.10\\n'\nelse\n  printf '[]\\n'\nfi\n",
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
        fs::create_dir_all(project_dir.join("site-packages/demo"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("site-packages/demo-stubs/demo"))
            .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo/runtime_only.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo-stubs/demo/__init__.pyi"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo-stubs/py.typed"), "\n")
            .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        let sources = external_resolution_sources(&config).expect("test setup should succeed");
        let mut modules =
            sources.into_iter().map(|source| source.logical_module).collect::<Vec<_>>();
        modules.sort();
        modules
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(modules, vec!["demo"]);
}

#[cfg(unix)]
#[test]
fn run_pipeline_prefers_local_companion_stub_surfaces() {
    let project_dir = temp_project_dir("run_pipeline_prefers_local_companion_stub_surfaces");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("src/lib")).expect("test setup should succeed");
        fs::write(project_dir.join("src/lib/__init__.py"), "def make() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/lib/__init__.pyi"), "def make() -> str: ...\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "from lib import make\n\nname: str = make()\n")
            .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        let discovery = collect_source_paths(&config).expect("test setup should succeed");
        let syntax = load_syntax_trees(
            &discovery.sources,
            false,
            &config.config.project.target_python.to_string(),
        )
        .expect("test setup should succeed");
        let bindings = syntax.iter().map(bind).collect::<Vec<_>>();
        check_graph(&build_graph(&bindings)).diagnostics
    };
    remove_temp_project_dir(&project_dir);

    assert!(!diagnostics.has_errors(), "{}", diagnostics.as_text());
}

#[cfg(unix)]
#[test]
fn run_pipeline_prefers_stub_packages_over_typed_runtime_packages() {
    let project_dir =
        temp_project_dir("run_pipeline_prefers_stub_packages_over_typed_runtime_packages");
    let diagnostics = {
        let probe = project_dir.join("python-probe");
        write_executable_script(
            &probe,
            "#!/bin/sh\nif [ \"$1\" = \"-c\" ] && printf '%s' \"$2\" | grep -q version_info; then\n  printf '3.10\\n'\nelse\n  printf '[]\\n'\nfi\n",
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
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("site-packages/demo"))
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("site-packages/demo-stubs/demo"))
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("site-packages/demo/__init__.py"),
            "def make() -> int:\n    return 1\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo/py.typed"), "")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("site-packages/demo-stubs/demo/__init__.pyi"),
            "def make() -> str: ...\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("site-packages/demo-stubs/py.typed"), "")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "from demo import make\n\nname: str = make()\n")
            .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        run_pipeline(&config).expect("test setup should succeed").diagnostics
    };
    remove_temp_project_dir(&project_dir);

    assert!(!diagnostics.has_errors(), "{}", diagnostics.as_text());
}

#[cfg(unix)]
#[test]
fn python_type_roots_from_interpreter_reads_json_from_probe_script() {
    let project_dir =
        temp_project_dir("python_type_roots_from_interpreter_reads_json_from_probe_script");
    let interpreter_path = project_dir.join("python3");
    write_executable_script(
        &interpreter_path,
        "#!/bin/sh\nprintf '[{\"path\":\"/tmp/site-packages\",\"allow_untyped_runtime\":false},{\"path\":\"/tmp/user-site\",\"allow_untyped_runtime\":false}]\\n'\n",
    );

    let roots = python_type_roots_from_interpreter(&interpreter_path);
    remove_temp_project_dir(&project_dir);

    assert_eq!(
        roots,
        vec![
            ExternalSupportRoot {
                path: PathBuf::from("/tmp/site-packages"),
                allow_untyped_runtime: false,
            },
            ExternalSupportRoot {
                path: PathBuf::from("/tmp/user-site"),
                allow_untyped_runtime: false,
            },
        ]
    );
}

#[cfg(unix)]
#[test]
fn run_pipeline_accepts_json_from_bundled_stdlib() {
    let project_dir = temp_project_dir("run_pipeline_accepts_json_from_bundled_stdlib");
    let diagnostics = {
        let probe = project_dir.join("python-probe");
        write_executable_script(
            &probe,
            "#!/bin/sh\nif [ \"$1\" = \"-c\" ] && printf '%s' \"$2\" | grep -q version_info; then\n  printf '3.10\\n'\nelse\n  printf '[]\\n'\nfi\n",
        );
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            format!(
                "[project]\nsrc = [\"src\"]\n\n[resolution]\npython_executable = \"{}\"\n",
                probe.display()
            ),
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "import json\n\ndef encode() -> str:\n    return json.dumps(\"value\")\n",
        )
        .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        run_pipeline(&config).expect("pipeline should succeed").diagnostics
    };
    remove_temp_project_dir(&project_dir);

    assert!(!diagnostics.has_errors(), "{}", diagnostics.as_text());
}

#[test]
fn run_pipeline_accepts_bundled_common_library_stubs() {
    let project_dir = temp_project_dir("run_pipeline_accepts_bundled_common_library_stubs");
    let diagnostics = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app.tpy"),
            concat!(
                "from numpy import array, ndarray\n",
                "from numpy.linalg import norm\n",
                "from pandas import DataFrame, read_csv\n",
                "from requests import Response, get\n",
                "from requests.sessions import Session\n",
                "from torch import Tensor, tensor\n",
                "from torch.nn import Linear, Module\n\n",
                "matrix: ndarray = array([1])\n",
                "reshaped: ndarray = matrix.reshape((1,))\n",
                "length: float = norm(reshaped)\n",
                "frame: DataFrame = read_csv(\"demo.csv\").head()\n",
                "response: Response = get(\"https://example.com\")\n",
                "payload = response.json()\n",
                "session = Session()\n",
                "response2: Response = session.get(\"https://example.com\")\n",
                "layer: Module = Linear(2, 1)\n",
                "value: Tensor = tensor(1).to(\"cpu\")\n",
            ),
        )
        .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        run_pipeline(&config).expect("test setup should succeed").diagnostics
    };
    remove_temp_project_dir(&project_dir);

    assert!(!diagnostics.has_errors(), "{}", diagnostics.as_text());
}

#[test]
fn bundled_stdlib_sources_filter_version_marked_files_by_target_python() {
    let project_dir =
        temp_project_dir("bundled_stdlib_sources_filter_version_marked_files_by_target_python");
    let root = project_dir.join("stdlib");
    fs::create_dir_all(&root).expect("test setup should succeed");
    fs::write(root.join("shared.pyi"), "def shared() -> int: ...\n")
        .expect("test setup should succeed");
    fs::write(
        root.join("modern_only.pyi"),
        "# typepython: min-python=3.11\n\ndef modern_only() -> int: ...\n",
    )
    .expect("test setup should succeed");
    fs::write(
        root.join("legacy_only.pyi"),
        "# typepython: max-python=3.10\n\ndef legacy_only() -> int: ...\n",
    )
    .expect("test setup should succeed");

    let modules_310 = bundled_stdlib_sources_for_root(&root, "3.10")
        .expect("3.10 stdlib selection should succeed")
        .into_iter()
        .map(|source| source.logical_module)
        .collect::<BTreeSet<_>>();
    let modules_311 = bundled_stdlib_sources_for_root(&root, "3.11")
        .expect("3.11 stdlib selection should succeed")
        .into_iter()
        .map(|source| source.logical_module)
        .collect::<BTreeSet<_>>();
    remove_temp_project_dir(&project_dir);

    assert_eq!(modules_310, BTreeSet::from([String::from("legacy_only"), String::from("shared"),]));
    assert_eq!(modules_311, BTreeSet::from([String::from("modern_only"), String::from("shared"),]));
}

#[test]
fn bundled_stdlib_snapshot_identity_tracks_target_filtered_surface() {
    let project_dir =
        temp_project_dir("bundled_stdlib_snapshot_identity_tracks_target_filtered_surface");
    let root = project_dir.join("stdlib");
    fs::create_dir_all(&root).expect("test setup should succeed");
    fs::write(root.join("shared.pyi"), "def shared() -> int: ...\n")
        .expect("test setup should succeed");
    fs::write(
        root.join("modern_only.pyi"),
        "# typepython: min-python=3.11\n\ndef modern_only() -> int: ...\n",
    )
    .expect("test setup should succeed");

    let snapshot_310 = bundled_stdlib_snapshot_identity_for_root(&root, "3.10")
        .expect("3.10 snapshot should succeed");
    let snapshot_311 = bundled_stdlib_snapshot_identity_for_root(&root, "3.11")
        .expect("3.11 snapshot should succeed");
    remove_temp_project_dir(&project_dir);

    assert_ne!(snapshot_310, snapshot_311);
}

#[test]
fn collect_source_paths_reports_tpy_python_collisions() {
    let project_dir = temp_project_dir("reports_tpy_python_collisions");
    let result = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("src/pkg")).expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/__init__.tpy"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/value.tpy"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/value.py"), "pass\n")
            .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        collect_source_paths(&config)
    };
    remove_temp_project_dir(&project_dir);

    let discovery = result.expect("test setup should succeed");
    assert!(discovery.diagnostics.has_errors());
    let text = discovery.diagnostics.as_text();
    assert!(text.contains("TPY3002"));
    assert!(text.contains("pkg.value"));
}

#[test]
fn collect_source_paths_allows_python_with_companion_stub() {
    let project_dir = temp_project_dir("allows_python_with_companion_stub");
    let result = {
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("src/pkg")).expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/__init__.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/value.py"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/value.pyi"), "...\n")
            .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        collect_source_paths(&config)
    };
    remove_temp_project_dir(&project_dir);

    let discovery = result.expect("test setup should succeed");
    assert!(discovery.diagnostics.is_empty());
    assert_eq!(discovery.sources.len(), 3);
}

#[test]
fn collect_source_paths_reports_cross_root_collisions() {
    let project_dir = temp_project_dir("reports_cross_root_collisions");
    let result = {
        fs::write(
            project_dir.join("typepython.toml"),
            concat!(
                "[project]\n",
                "src = [\"src\", \"vendor\"]\n",
                "include = [\"src/**/*.tpy\", \"vendor/**/*.tpy\"]\n"
            ),
        )
        .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("src/pkg")).expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("vendor/pkg")).expect("test setup should succeed");
        fs::write(project_dir.join("src/pkg/__init__.tpy"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("vendor/pkg/__init__.tpy"), "pass\n")
            .expect("test setup should succeed");

        let config = load(&project_dir).expect("test setup should succeed");
        collect_source_paths(&config)
    };
    remove_temp_project_dir(&project_dir);

    let discovery = result.expect("test setup should succeed");
    assert!(discovery.diagnostics.has_errors());
    assert!(discovery.diagnostics.as_text().contains("TPY3002"));
}

mod adapter;
mod api_diff;
mod compat;
mod consistency;
mod migration;
mod pipeline;
mod type_health;
mod verification;

pub(super) fn temp_project_dir(test_name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let directory = env::temp_dir().join(format!("typepython-cli-{test_name}-{unique}"));
    fs::create_dir_all(&directory).expect("temp project directory should be created");
    directory
}

pub(super) fn remove_temp_project_dir(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).expect("temp project directory should be removed");
    }
}

#[cfg(unix)]
pub(super) fn write_executable_script(path: &Path, body: &str) {
    fs::write(path, body).expect("script should be written");
    let mut permissions =
        fs::metadata(path).expect("script metadata should be readable").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("script should be executable");
}

pub(super) fn write_zip_archive(path: &Path, files: &[(&str, &str)]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("archive parent should be created");
    }
    let file = fs::File::create(path).expect("zip archive should be created");
    let mut writer = ZipWriter::new(file);
    let options = FileOptions::default();
    for (relative_path, contents) in files {
        writer
            .start_file(relative_path.replace('\\', "/"), options)
            .expect("zip file entry should be created");
        std::io::Write::write_all(&mut writer, contents.as_bytes())
            .expect("zip file entry should be written");
    }
    writer.finish().expect("zip archive should finish");
}

pub(super) fn write_deflated_zip_archive(path: &Path, files: &[(&str, &str)]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("archive parent should be created");
    }
    let file = fs::File::create(path).expect("zip archive should be created");
    let mut writer = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
    for (relative_path, contents) in files {
        writer
            .start_file(relative_path.replace('\\', "/"), options)
            .expect("zip file entry should be created");
        std::io::Write::write_all(&mut writer, contents.as_bytes())
            .expect("zip file entry should be written");
    }
    writer.finish().expect("zip archive should finish");
}

pub(super) fn write_valid_wheel_archive(path: &Path, files: &[(&str, &str)]) {
    const DIST_INFO: &str = "type_python-0.1.0.dist-info";
    const METADATA: &str = "Metadata-Version: 2.1\nName: type-python\nVersion: 0.1.0\n";
    const WHEEL: &str = concat!(
        "Wheel-Version: 1.0\n",
        "Generator: typepython-test\n",
        "Root-Is-Purelib: true\n",
        "Tag: py3-none-any\n",
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("archive parent should be created");
    }
    let file = fs::File::create(path).expect("wheel archive should be created");
    let mut writer = ZipWriter::new(file);
    let options = FileOptions::default();
    let mut record_paths = Vec::new();
    for (relative_path, contents) in files {
        let relative_path = relative_path.replace('\\', "/");
        writer.start_file(&relative_path, options).expect("wheel file entry should be created");
        std::io::Write::write_all(&mut writer, contents.as_bytes())
            .expect("wheel file entry should be written");
        record_paths.push(relative_path);
    }
    for (relative_path, contents) in
        [(format!("{DIST_INFO}/METADATA"), METADATA), (format!("{DIST_INFO}/WHEEL"), WHEEL)]
    {
        writer.start_file(&relative_path, options).expect("wheel metadata entry should be created");
        std::io::Write::write_all(&mut writer, contents.as_bytes())
            .expect("wheel metadata entry should be written");
        record_paths.push(relative_path);
    }
    let record_path = format!("{DIST_INFO}/RECORD");
    record_paths.push(record_path.clone());
    let record =
        record_paths.into_iter().map(|path| format!("{path},,")).collect::<Vec<_>>().join("\n")
            + "\n";
    writer.start_file(record_path, options).expect("wheel RECORD entry should be created");
    std::io::Write::write_all(&mut writer, record.as_bytes())
        .expect("wheel RECORD entry should be written");
    writer.finish().expect("wheel archive should finish");
}

pub(super) fn write_tar_gz_archive(path: &Path, root: &str, files: &[(&str, &str)]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("archive parent should be created");
    }
    let file = fs::File::create(path).expect("tar.gz archive should be created");
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for (relative_path, contents) in files {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(contents.len() as u64);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                format!("{}/{}", root, relative_path.replace('\\', "/")),
                contents.as_bytes(),
            )
            .expect("tar.gz entry should be written");
    }
    builder.finish().expect("tar.gz archive should finish");
}

pub(super) fn write_valid_sdist_archive(path: &Path, root: &str, files: &[(&str, &str)]) {
    const PKG_INFO: &str = "Metadata-Version: 2.1\nName: type-python\nVersion: 0.1.0\n";
    let mut files_with_metadata = files.to_vec();
    files_with_metadata.push(("PKG-INFO", PKG_INFO));
    write_tar_gz_archive(path, root, &files_with_metadata);
}

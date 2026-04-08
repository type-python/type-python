use super::discovery::{
    ExternalSupportRoot, bundled_stdlib_snapshot_identity_for_root,
    bundled_stdlib_sources_for_root, collect_source_paths, external_resolution_sources,
    python_type_roots_from_interpreter,
};
use super::migration::{build_migration_report, emit_migration_stubs};
use super::pipeline::{
    build_diagnostics, clean_project, compile_runtime_bytecode, format_watch_rebuild_note,
    load_syntax_trees, run_pipeline, should_emit_build_outputs, watch_targets,
    write_incremental_snapshot,
};
use super::verification::{
    SuppliedArtifactKind, SuppliedVerifyArtifact, run_verify, supplied_verify_artifacts,
    verify_build_artifacts, verify_packaged_artifacts, verify_runtime_public_name_parity,
};
use super::{Cli, bytecode_path_for, embedded_config_template, exit_code_for_error, init_project};
use crate::cli::{CleanArgs, VerifyArgs};
use clap::Parser;
use flate2::{Compression, write::GzEncoder};
use notify::RecursiveMode;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    collections::BTreeSet,
    env, fs,
    path::MAIN_SEPARATOR,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};
use typepython_binding::bind;
use typepython_checking::check as check_graph;
use typepython_config::load;
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_emit::{EmitArtifact, write_runtime_outputs};
use typepython_graph::build as build_graph;
use typepython_incremental::IncrementalState;
use zip::{ZipWriter, write::FileOptions};

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
        let syntax =
            load_syntax_trees(&discovery.sources, false).expect("test setup should succeed");
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
            "import json\n\ndef encode(value: str) -> str:\n    return json.dumps(value)\n",
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
                ("setup.py", "pass\n"),
                ("tests/test_api.py", "pass\n"),
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
                ("tests/test_api.py", "pass\n"),
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
    };
    remove_temp_project_dir(&project_dir);

    assert!(diagnostics.is_empty());
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
fn verify_command_parses_supplied_artifact_flags() {
    let cli = Cli::parse_from([
        "typepython",
        "verify",
        "--project",
        "examples/hello-world",
        "--wheel",
        "dist/pkg.whl",
        "--sdist",
        "dist/pkg.tar.gz",
    ]);

    let super::Command::Verify(args) = cli.command else {
        panic!("expected verify command");
    };
    let supplied = supplied_verify_artifacts(&args);
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
fn run_pipeline_stops_before_lowering_when_checker_fails() {
    let project_dir = temp_project_dir("run_pipeline_stops_before_lowering_when_checker_fails");
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
    assert!(snapshot.lowered_modules.is_empty());
    assert!(snapshot.emit_plan.is_empty());
    assert_eq!(snapshot.tracked_modules, 0);
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
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let mut diagnostics = DiagnosticReport::default();
        diagnostics.push(Diagnostic::error("TPY4004", "duplicate declaration"));

        build_diagnostics(&config, &diagnostics).as_text()
    };
    remove_temp_project_dir(&project_dir);

    assert!(rendered.contains("TPY4004"));
    assert!(rendered.contains("TPY5002"));
}

#[test]
fn should_emit_build_outputs_respects_no_emit_on_error() {
    let project_dir = temp_project_dir("should_emit_build_outputs_respects_no_emit_on_error");
    let result = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\nno_emit_on_error = false\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let mut diagnostics = DiagnosticReport::default();
        diagnostics.push(Diagnostic::error("TPY4004", "duplicate declaration"));

        should_emit_build_outputs(&config, &diagnostics)
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

#[test]
fn watch_targets_include_config_and_existing_source_roots() {
    let project_dir = temp_project_dir("watch_targets_include_config_and_existing_source_roots");
    let targets = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        watch_targets(&config)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(targets.len(), 2);
    assert!(targets.iter().any(|(path, mode)| {
        path.ends_with("typepython.toml") && *mode == RecursiveMode::NonRecursive
    }));
    assert!(
        targets
            .iter()
            .any(|(path, mode)| path.ends_with("src") && *mode == RecursiveMode::Recursive)
    );
}

#[test]
fn format_watch_rebuild_note_summarizes_changed_paths() {
    let changed = BTreeSet::from([
        PathBuf::from("src/app/__init__.tpy"),
        PathBuf::from("src/app/models.tpy"),
        PathBuf::from("src/app/views.tpy"),
        PathBuf::from("src/app/more.tpy"),
    ]);

    let note = format_watch_rebuild_note(&changed);
    assert!(note.contains("rebuild triggered by"));
    assert!(note.contains("and 1 more path(s)"));
}

#[test]
fn build_migration_report_counts_file_coverage_and_boundaries() {
    let project_dir =
        temp_project_dir("build_migration_report_counts_file_coverage_and_boundaries");
    let report = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "def typed(value: int) -> int:\n    return value\n\ndef untyped(value) -> int:\n    return 0\n\nleak: dynamic = 1\n",
        ).expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let discovery = collect_source_paths(&config).expect("test setup should succeed");
        let syntax_trees =
            load_syntax_trees(&discovery.sources, false).expect("test setup should succeed");
        build_migration_report(&config, &syntax_trees)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.total_declarations, 3);
    assert_eq!(report.known_declarations, 1);
    assert_eq!(report.total_dynamic_boundaries, 1);
    assert_eq!(report.total_unknown_boundaries, 0);
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].known_declarations, 1);
}

#[test]
fn build_migration_report_ranks_high_impact_untyped_files() {
    let project_dir = temp_project_dir("build_migration_report_ranks_high_impact_untyped_files");
    let report = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app/__init__.tpy"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app/a.tpy"), "def untyped(value) -> int:\n    return 0\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/b.tpy"),
            "from app.a import untyped\n\ndef use(value: int) -> int:\n    return value\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/c.tpy"),
            "def clean(value: int) -> int:\n    return value\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let discovery = collect_source_paths(&config).expect("test setup should succeed");
        let syntax_trees =
            load_syntax_trees(&discovery.sources, false).expect("test setup should succeed");
        build_migration_report(&config, &syntax_trees)
    };
    remove_temp_project_dir(&project_dir);

    assert!(!report.high_impact_untyped_files.is_empty());
    assert!(report.high_impact_untyped_files[0].path.ends_with("src/app/a.tpy"));
    assert_eq!(report.high_impact_untyped_files[0].downstream_references, 1);
}

#[test]
fn migrate_command_parses_emit_stubs_flags() {
    let cli = Cli::parse_from([
        "typepython",
        "migrate",
        "--project",
        "examples/hello-world",
        "--report",
        "--emit-stubs",
        "src/app",
        "--emit-stubs",
        "src/lib.py",
        "--stub-out-dir",
        ".generated-stubs",
    ]);

    let super::Command::Migrate(args) = cli.command else {
        panic!("expected migrate command");
    };

    assert!(args.report);
    assert_eq!(args.emit_stubs, vec![PathBuf::from("src/app"), PathBuf::from("src/lib.py")]);
    assert_eq!(args.stub_out_dir, Some(PathBuf::from(".generated-stubs")));
}

#[test]
fn emit_migration_stubs_writes_generated_pyi_to_configured_output_dir() {
    let project_dir =
        temp_project_dir("emit_migration_stubs_writes_generated_pyi_to_configured_output_dir");
    let (written, stub) = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app/__init__.py"), "").expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/helpers.py"),
            "VALUE = 1\n\ndef parse(text):\n    return VALUE\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let discovery = collect_source_paths(&config).expect("test setup should succeed");
        let written = emit_migration_stubs(
            &config,
            &discovery.sources,
            &[PathBuf::from("src/app")],
            Some(Path::new(".generated-stubs")),
        )
        .expect("migration stub emission should succeed");
        let stub_path = project_dir.join(".generated-stubs/app/helpers.pyi");
        let stub =
            fs::read_to_string(&stub_path).expect("generated migration stub should be readable");

        (written, stub)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(
        written,
        vec![
            project_dir.join(".generated-stubs/app/__init__.pyi"),
            project_dir.join(".generated-stubs/app/helpers.pyi"),
        ]
    );
    assert!(stub.starts_with("# auto-generated by typepython migrate"));
    assert!(stub.contains("VALUE: int"));
    assert!(stub.contains("# TODO: add type annotation"));
    assert!(stub.contains("def parse(text: ...) -> int: ..."));
}

fn temp_project_dir(test_name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let directory = env::temp_dir().join(format!("typepython-cli-{test_name}-{unique}"));
    fs::create_dir_all(&directory).expect("temp project directory should be created");
    directory
}

fn remove_temp_project_dir(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).expect("temp project directory should be removed");
    }
}

#[cfg(unix)]
fn write_executable_script(path: &Path, body: &str) {
    fs::write(path, body).expect("script should be written");
    let mut permissions =
        fs::metadata(path).expect("script metadata should be readable").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("script should be executable");
}

fn write_zip_archive(path: &Path, files: &[(&str, &str)]) {
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

fn write_tar_gz_archive(path: &Path, root: &str, files: &[(&str, &str)]) {
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

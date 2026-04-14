use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode},
};

use anyhow::{Context, Result};
use notify::RecursiveMode;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use typepython_binding::bind;
use typepython_checking::{
    check_modules_with_binding_metadata, collect_effective_callable_stub_overrides,
    collect_synthetic_method_stubs, semantic_incremental_state_with_binding_metadata,
    semantic_incremental_state_with_reused_summaries,
};
use typepython_config::ConfigHandle;
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_emit::{
    EmitArtifact, InferredStubMode, PlannedModuleSource, RuntimeWriteError, StubCallableOverride,
    StubSealedClass, StubSyntheticMethod, StubValueOverride, TypePythonStubContext,
    generate_inferred_stub_source, plan_emits_for_sources, write_runtime_outputs,
};
use typepython_graph::build;
use typepython_incremental::{
    IncrementalState, SnapshotMetadata, affected_modules, decode_snapshot, dependency_index,
    dependency_index_from_summaries, diff, encode_snapshot, snapshot_diff_modules,
    source_change_modules,
};
use typepython_lowering::{LoweredModule, LoweringOptions, LoweringResult, lower_with_options};
use typepython_project::{
    collect_import_source_paths, import_resolves_within_modules, inferred_shadow_stub_syntax_trees,
    replace_local_python_surfaces_with_shadow_stubs, support_source_snapshot_identity,
    write_shadow_stub_cache,
};
use typepython_syntax::{SourceFile, SourceKind, apply_type_ignore_directives};

use crate::cli::{CleanArgs, OutputFormat, RunArgs};
use crate::discovery::{
    DiscoveredSource, bundled_stdlib_snapshot_identity, collect_source_paths, support_source_index,
};
use crate::verification::{public_surface_completeness_diagnostics, verify_build_artifacts};
use crate::{
    CommandSummary, bytecode_path_for, exit_code, load_project, print_summary,
    remove_dir_if_exists, resolve_python_executable,
};

mod loading;
mod stubs;

pub(crate) use self::loading::load_syntax_trees;
use self::loading::{PreparedPipelineSyntax, prepare_pipeline_syntax};
use self::stubs::build_typepython_stub_contexts;

#[derive(Debug)]
pub(crate) struct PipelineSnapshot {
    pub(crate) lowered_modules: Vec<LoweredModule>,
    pub(crate) emit_plan: Vec<EmitArtifact>,
    pub(crate) stub_contexts: BTreeMap<PathBuf, TypePythonStubContext>,
    pub(crate) incremental: IncrementalState,
    pub(crate) module_diagnostics: BTreeMap<String, Vec<Diagnostic>>,
    pub(crate) tracked_modules: usize,
    pub(crate) discovered_sources: usize,
    pub(crate) emit_blocked_by_pipeline: bool,
    pub(crate) diagnostics: DiagnosticReport,
}

#[derive(Debug)]
struct AnalyzedPipelineState {
    bindings: Vec<typepython_binding::BindingTable>,
    graph: typepython_graph::ModuleGraph,
    incremental: IncrementalState,
    dependency_index: typepython_incremental::ModuleDependencyIndex,
    direct_changes: BTreeSet<String>,
    summary_changed_modules: BTreeSet<String>,
    tracked_modules: usize,
    pre_lowering_emit_plan: Vec<EmitArtifact>,
}

const MATERIALIZED_BUILD_MANIFEST_SCHEMA_VERSION: u32 = 1;
const ANALYSIS_CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
struct CachedEmitArtifact {
    source_path: PathBuf,
    runtime_path: Option<PathBuf>,
    stub_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
struct MaterializedBuildManifest {
    schema_version: u32,
    incremental: IncrementalState,
    emit_plan: Vec<CachedEmitArtifact>,
    runtime_validators: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
struct AnalysisCacheMetadata {
    snapshot: SnapshotMetadata,
    require_explicit_overrides: bool,
    enable_sealed_exhaustiveness: bool,
    report_deprecated: String,
    strict: bool,
    warn_unsafe: bool,
    imports: String,
    require_known_public_types: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
struct AnalysisCache {
    schema_version: u32,
    incremental: IncrementalState,
    metadata: AnalysisCacheMetadata,
    module_diagnostics: BTreeMap<String, Vec<Diagnostic>>,
}

pub(crate) fn should_emit_build_outputs(
    config: &ConfigHandle,
    snapshot: &PipelineSnapshot,
) -> bool {
    !snapshot.emit_blocked_by_pipeline
        && (!snapshot.diagnostics.has_errors() || !config.config.emit.no_emit_on_error)
}

pub(crate) fn build_diagnostics(
    config: &ConfigHandle,
    snapshot: &PipelineSnapshot,
) -> DiagnosticReport {
    let mut build_diagnostics = snapshot.diagnostics.clone();

    if snapshot.diagnostics.has_errors()
        && !snapshot.emit_blocked_by_pipeline
        && config.config.emit.no_emit_on_error
    {
        build_diagnostics.push(Diagnostic::error(
            "TPY5002",
            format!("emit blocked by `emit.no_emit_on_error` for {}", config.config_dir.display()),
        ));
    }

    build_diagnostics
}

pub(crate) fn runtime_write_diagnostic(error: &anyhow::Error) -> Option<Diagnostic> {
    match error.downcast_ref::<RuntimeWriteError>() {
        Some(RuntimeWriteError::StubGeneration { .. }) => {
            Some(Diagnostic::error("TPY5001", error.to_string()))
        }
        _ => None,
    }
}

pub(crate) fn clean_project(args: CleanArgs) -> Result<ExitCode> {
    let config = load_project(args.project.as_ref())?;
    let out_dir = config.resolve_relative_path(&config.config.project.out_dir);
    let cache_dir = config.resolve_relative_path(&config.config.project.cache_dir);

    remove_dir_if_exists(&out_dir)?;
    remove_dir_if_exists(&cache_dir)?;

    println!("cleaned TypePython artifacts for {}", config.config_dir.display());
    println!("  removed: {}", out_dir.display());
    println!("  removed: {}", cache_dir.display());

    Ok(ExitCode::SUCCESS)
}

pub(crate) fn run_lsp(args: RunArgs) -> Result<ExitCode> {
    let config = load_project(args.project.as_ref())?;
    if args.format == OutputFormat::Json {
        return Err(anyhow::anyhow!(
            "`typepython lsp` speaks JSON-RPC over stdio and does not support `--format json`"
        ));
    }
    typepython_lsp::serve(&config)?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn run_build_like_command(
    config: &ConfigHandle,
    format: OutputFormat,
    command: &str,
    mut notes: Vec<String>,
) -> Result<ExitCode> {
    ensure_output_dirs(config)?;

    let snapshot = run_pipeline(config)?;
    let _ = persist_pipeline_analysis_state(config, &snapshot)?;
    let mut diagnostics = build_diagnostics(config, &snapshot);
    if should_emit_build_outputs(config, &snapshot) {
        let materialize_notes = match materialize_build_outputs(config, &snapshot) {
            Ok(runtime_summary) => runtime_summary,
            Err(error) => {
                if let Some(diagnostic) = runtime_write_diagnostic(&error) {
                    diagnostics.push(diagnostic);
                    let summary = CommandSummary {
                        command: String::from(command),
                        config_path: config.config_path.display().to_string(),
                        config_source: config.source,
                        discovered_sources: snapshot.discovered_sources,
                        lowered_modules: snapshot.lowered_modules.len(),
                        planned_artifacts: snapshot.emit_plan.len(),
                        tracked_modules: snapshot.tracked_modules,
                        notes,
                    };
                    print_summary(format, &summary, &diagnostics)?;
                    return Ok(exit_code(&diagnostics));
                }
                return Err(error).with_context(|| {
                    format!(
                        "unable to write runtime artifacts under {}",
                        config.resolve_relative_path(&config.config.project.out_dir).display()
                    )
                });
            }
        };
        notes.extend(materialize_notes);
    }

    let summary = CommandSummary {
        command: String::from(command),
        config_path: config.config_path.display().to_string(),
        config_source: config.source,
        discovered_sources: snapshot.discovered_sources,
        lowered_modules: snapshot.lowered_modules.len(),
        planned_artifacts: snapshot.emit_plan.len(),
        tracked_modules: snapshot.tracked_modules,
        notes,
    };

    print_summary(format, &summary, &diagnostics)?;
    Ok(exit_code(&diagnostics))
}

pub(crate) fn materialize_build_outputs(
    config: &ConfigHandle,
    snapshot: &PipelineSnapshot,
) -> Result<Vec<String>> {
    let out_root = config.resolve_relative_path(&config.config.project.out_dir);
    let stale_cleanup_note = cleanup_stale_materialized_outputs(config, &snapshot.emit_plan)?;
    let runtime_summary = write_runtime_outputs(
        &snapshot.emit_plan,
        &snapshot.lowered_modules,
        config.config.emit.write_py_typed,
        config.config.emit.runtime_validators,
        Some(&snapshot.stub_contexts),
    )?;
    let mut py_typed_written = runtime_summary.py_typed_written;
    if config.config.emit.write_py_typed {
        for package_root in py_typed_package_roots(&out_root, &snapshot.emit_plan) {
            let marker_path = package_root.join("py.typed");
            if !marker_path.exists() {
                fs::write(&marker_path, "").with_context(|| {
                    format!("unable to write package marker {}", marker_path.display())
                })?;
                py_typed_written += 1;
            }
        }
    }
    let mut notes = vec![format!(
        "wrote {} runtime artifact(s), {} stub artifact(s), {} `py.typed` marker(s)",
        runtime_summary.runtime_files_written, runtime_summary.stub_files_written, py_typed_written
    )];
    if let Some(note) = stale_cleanup_note {
        notes.push(note);
    }
    if config.config.emit.emit_pyc {
        let compiled_pyc = compile_runtime_bytecode(config, &snapshot.emit_plan)?;
        notes.push(format!("compiled {} runtime artifact(s) to bytecode", compiled_pyc));
    }
    let snapshot_path = write_incremental_snapshot(
        &config.resolve_relative_path(&config.config.project.cache_dir),
        &snapshot.incremental,
    )?;
    let manifest_path = write_materialized_build_manifest(config, snapshot)?;
    notes.push(format!(
        "cached {} module fingerprint(s) at {}",
        snapshot.incremental.fingerprints.len(),
        snapshot_path.display()
    ));
    notes.push(format!("recorded materialized build manifest at {}", manifest_path.display()));
    Ok(notes)
}

pub(crate) fn py_typed_package_roots(
    out_root: &Path,
    artifacts: &[EmitArtifact],
) -> BTreeSet<PathBuf> {
    let mut package_roots = BTreeSet::new();

    for artifact in artifacts {
        for path in
            [artifact.runtime_path.as_ref(), artifact.stub_path.as_ref()].into_iter().flatten()
        {
            let Some(parent) = path.parent() else {
                continue;
            };
            let is_package_init = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "__init__.py" || name == "__init__.pyi");
            if is_package_init || parent != out_root {
                package_roots.insert(parent.to_path_buf());
            }
        }
    }

    package_roots
}

fn cleanup_stale_materialized_outputs(
    config: &ConfigHandle,
    current_artifacts: &[EmitArtifact],
) -> Result<Option<String>> {
    let Some(previous_manifest) = load_previous_materialized_build_manifest(config)? else {
        return Ok(None);
    };
    let previous_artifacts = emit_artifacts_from_cached(&previous_manifest.emit_plan);
    let current_paths = current_artifacts
        .iter()
        .flat_map(|artifact| {
            [artifact.runtime_path.as_ref(), artifact.stub_path.as_ref()].into_iter().flatten()
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut removed_files = 0usize;

    for artifact in &previous_artifacts {
        if let Some(runtime_path) = &artifact.runtime_path {
            if !current_paths.contains(runtime_path) {
                if runtime_path.exists() {
                    fs::remove_file(runtime_path).with_context(|| {
                        format!(
                            "unable to remove stale runtime artifact {}",
                            runtime_path.display()
                        )
                    })?;
                    removed_files += 1;
                }
                if let Ok(bytecode_path) = bytecode_path_for(runtime_path) {
                    if bytecode_path.exists() {
                        fs::remove_file(&bytecode_path).with_context(|| {
                            format!(
                                "unable to remove stale bytecode artifact {}",
                                bytecode_path.display()
                            )
                        })?;
                        removed_files += 1;
                    }
                }
            }
        }
        if let Some(stub_path) = &artifact.stub_path {
            if !current_paths.contains(stub_path) && stub_path.exists() {
                fs::remove_file(stub_path).with_context(|| {
                    format!("unable to remove stale stub artifact {}", stub_path.display())
                })?;
                removed_files += 1;
            }
        }
    }

    let out_root = config.resolve_relative_path(&config.config.project.out_dir);
    let desired_package_roots = if config.config.emit.write_py_typed {
        py_typed_package_roots(&out_root, current_artifacts)
    } else {
        BTreeSet::new()
    };
    let previous_package_roots = py_typed_package_roots(&out_root, &previous_artifacts);
    for package_root in previous_package_roots {
        if desired_package_roots.contains(&package_root) {
            continue;
        }
        let marker_path = package_root.join("py.typed");
        if marker_path.exists() {
            fs::remove_file(&marker_path).with_context(|| {
                format!("unable to remove stale package marker {}", marker_path.display())
            })?;
            removed_files += 1;
        }
    }

    Ok((removed_files > 0)
        .then(|| format!("removed {} stale materialized artifact(s)", removed_files)))
}

pub(crate) fn ensure_output_dirs(config: &ConfigHandle) -> Result<()> {
    fs::create_dir_all(config.resolve_relative_path(&config.config.project.out_dir)).with_context(
        || {
            format!(
                "unable to create output directory {}",
                config.resolve_relative_path(&config.config.project.out_dir).display()
            )
        },
    )?;
    fs::create_dir_all(config.resolve_relative_path(&config.config.project.cache_dir))
        .with_context(|| {
            format!(
                "unable to create cache directory {}",
                config.resolve_relative_path(&config.config.project.cache_dir).display()
            )
        })?;
    Ok(())
}

pub(crate) fn watch_targets(config: &ConfigHandle) -> Vec<(PathBuf, RecursiveMode)> {
    let mut targets = BTreeMap::new();
    targets.insert(config.config_path.clone(), RecursiveMode::NonRecursive);
    for src in &config.config.project.src {
        let path = config.resolve_relative_path(src);
        if path.exists() {
            targets.insert(path, RecursiveMode::Recursive);
        }
    }
    targets.into_iter().collect()
}

pub(crate) fn collect_watch_event_paths(
    changed_paths: &mut BTreeSet<PathBuf>,
    paths: Vec<PathBuf>,
) {
    changed_paths.extend(paths);
}

pub(crate) fn format_watch_rebuild_note(changed_paths: &BTreeSet<PathBuf>) -> String {
    if changed_paths.is_empty() {
        return String::from("rebuild triggered by filesystem changes");
    }

    let preview = changed_paths
        .iter()
        .take(3)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if changed_paths.len() <= 3 {
        format!("rebuild triggered by {preview}")
    } else {
        format!("rebuild triggered by {preview} and {} more path(s)", changed_paths.len() - 3)
    }
}

fn blocked_pipeline_snapshot(
    discovered_sources: usize,
    diagnostics: DiagnosticReport,
) -> PipelineSnapshot {
    PipelineSnapshot {
        lowered_modules: Vec::new(),
        emit_plan: Vec::new(),
        stub_contexts: BTreeMap::new(),
        incremental: IncrementalState::default(),
        module_diagnostics: BTreeMap::new(),
        tracked_modules: 0,
        discovered_sources,
        emit_blocked_by_pipeline: true,
        diagnostics,
    }
}

fn blocked_pipeline_snapshot_with_incremental(
    discovered_sources: usize,
    diagnostics: DiagnosticReport,
    incremental: IncrementalState,
    module_diagnostics: BTreeMap<String, Vec<Diagnostic>>,
    tracked_modules: usize,
) -> PipelineSnapshot {
    PipelineSnapshot {
        lowered_modules: Vec::new(),
        emit_plan: Vec::new(),
        stub_contexts: BTreeMap::new(),
        incremental,
        module_diagnostics,
        tracked_modules,
        discovered_sources,
        emit_blocked_by_pipeline: true,
        diagnostics,
    }
}

fn analyze_pipeline_state(
    config: &ConfigHandle,
    prepared: &PreparedPipelineSyntax,
    previous: Option<&IncrementalState>,
) -> Result<AnalyzedPipelineState> {
    let bindings: Vec<_> = prepared.all_syntax_trees.par_iter().map(bind).collect();
    let graph = build(&bindings);
    let current_dependency_index = dependency_index(&graph);

    let target_python = config.config.project.target_python.to_string();
    let analysis_python = config.analysis_python().to_string();
    let stdlib_snapshot = Some(bundled_stdlib_snapshot_identity(&analysis_python)?);
    let snapshot_metadata = SnapshotMetadata {
        target_python: Some(target_python),
        analysis_python: Some(analysis_python.clone()),
        emit_style: Some(config.config.emit.emit_style.to_string()),
        support_snapshot: Some(support_source_snapshot_identity(config, &analysis_python)?),
    };
    let source_hashes = syntax_tree_source_hashes(&prepared.all_syntax_trees);
    let incremental = match previous {
        Some(previous)
            if previous.stdlib_snapshot == stdlib_snapshot
                && previous.metadata == snapshot_metadata =>
        {
            let current_sources = IncrementalState::default()
                .with_source_hashes(source_hashes.clone())
                .with_metadata(snapshot_metadata.clone());
            let direct_changes = source_change_modules(previous, &current_sources);
            semantic_incremental_state_with_reused_summaries(
                &graph,
                &bindings,
                config.config.typing.imports,
                None,
                &previous.summaries,
                &direct_changes,
                stdlib_snapshot.clone(),
                snapshot_metadata.clone(),
            )
        }
        Some(_) | None => semantic_incremental_state_with_binding_metadata(
            &graph,
            &bindings,
            config.config.typing.imports,
            None,
            stdlib_snapshot.clone(),
            snapshot_metadata.clone(),
        ),
    }
    .with_source_hashes(source_hashes);
    let direct_changes = previous
        .map(|previous| source_change_modules(previous, &incremental))
        .unwrap_or_else(|| {
            prepared
                .syntax_trees
                .iter()
                .map(|tree| tree.source.logical_module.clone())
                .collect::<BTreeSet<_>>()
        });
    let summary_changed_modules = previous
        .map(|previous| snapshot_diff_modules(&diff(previous, &incremental)))
        .unwrap_or_else(|| incremental.fingerprints.keys().cloned().collect());
    let tracked_modules = incremental.fingerprints.len();
    let planned_sources: Vec<_> = prepared
        .syntax_trees
        .iter()
        .map(|tree| PlannedModuleSource {
            source_path: tree.source.path.clone(),
            source_kind: tree.source.kind,
        })
        .collect();
    let pre_lowering_emit_plan = plan_emits_for_sources(config, &planned_sources);

    Ok(AnalyzedPipelineState {
        bindings,
        graph,
        incremental,
        dependency_index: current_dependency_index,
        direct_changes,
        summary_changed_modules,
        tracked_modules,
        pre_lowering_emit_plan,
    })
}

fn syntax_tree_source_hashes(
    syntax_trees: &[typepython_syntax::SyntaxTree],
) -> BTreeMap<String, u64> {
    syntax_trees
        .iter()
        .map(|tree| {
            let mut hash = 0xcbf29ce484222325_u64;
            for byte in tree
                .source
                .logical_module
                .as_bytes()
                .iter()
                .chain([0_u8].iter())
                .chain(tree.source.text.as_bytes().iter())
            {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x100000001b3_u64);
            }
            (tree.source.logical_module.clone(), hash)
        })
        .collect()
}

fn cached_emit_artifacts(artifacts: &[EmitArtifact]) -> Vec<CachedEmitArtifact> {
    artifacts
        .iter()
        .map(|artifact| CachedEmitArtifact {
            source_path: artifact.source_path.clone(),
            runtime_path: artifact.runtime_path.clone(),
            stub_path: artifact.stub_path.clone(),
        })
        .collect()
}

fn emit_artifacts_from_cached(artifacts: &[CachedEmitArtifact]) -> Vec<EmitArtifact> {
    artifacts
        .iter()
        .map(|artifact| EmitArtifact {
            source_path: artifact.source_path.clone(),
            runtime_path: artifact.runtime_path.clone(),
            stub_path: artifact.stub_path.clone(),
        })
        .collect()
}

fn analysis_cache_metadata(
    config: &ConfigHandle,
    snapshot: &SnapshotMetadata,
) -> AnalysisCacheMetadata {
    AnalysisCacheMetadata {
        snapshot: snapshot.clone(),
        require_explicit_overrides: config.config.typing.require_explicit_overrides,
        enable_sealed_exhaustiveness: config.config.typing.enable_sealed_exhaustiveness,
        report_deprecated: format!("{:?}", config.config.typing.report_deprecated),
        strict: config.config.typing.strict,
        warn_unsafe: config.config.typing.warn_unsafe,
        imports: format!("{:?}", config.config.typing.imports),
        require_known_public_types: config.config.typing.require_known_public_types,
    }
}

fn analysis_cache_path(config: &ConfigHandle) -> PathBuf {
    config.resolve_relative_path(&config.config.project.cache_dir).join("analysis-cache.json")
}

fn materialized_build_manifest_path(config: &ConfigHandle) -> PathBuf {
    config.resolve_relative_path(&config.config.project.cache_dir).join("build-manifest.json")
}

fn load_previous_analysis_cache(config: &ConfigHandle) -> Result<Option<AnalysisCache>> {
    let cache_path = analysis_cache_path(config);
    if !cache_path.is_file() {
        return Ok(None);
    }
    let rendered = fs::read_to_string(&cache_path)
        .with_context(|| format!("unable to read {}", cache_path.display()))?;
    let cache: AnalysisCache = serde_json::from_str(&rendered)
        .with_context(|| format!("unable to decode analysis cache {}", cache_path.display()))?;
    if cache.schema_version != ANALYSIS_CACHE_SCHEMA_VERSION {
        return Ok(None);
    }
    Ok(Some(cache))
}

fn load_previous_materialized_build_manifest(
    config: &ConfigHandle,
) -> Result<Option<MaterializedBuildManifest>> {
    let manifest_path = materialized_build_manifest_path(config);
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let rendered = fs::read_to_string(&manifest_path)
        .with_context(|| format!("unable to read {}", manifest_path.display()))?;
    let manifest: MaterializedBuildManifest =
        serde_json::from_str(&rendered).with_context(|| {
            format!("unable to decode materialized build manifest {}", manifest_path.display())
        })?;
    if manifest.schema_version != MATERIALIZED_BUILD_MANIFEST_SCHEMA_VERSION {
        return Ok(None);
    }
    Ok(Some(manifest))
}

fn write_analysis_cache(
    config: &ConfigHandle,
    metadata: &AnalysisCacheMetadata,
    snapshot: &PipelineSnapshot,
) -> Result<PathBuf> {
    let cache_dir = config.resolve_relative_path(&config.config.project.cache_dir);
    fs::create_dir_all(&cache_dir)
        .with_context(|| format!("unable to create cache directory {}", cache_dir.display()))?;
    let cache_path = analysis_cache_path(config);
    let payload = serde_json::to_string_pretty(&AnalysisCache {
        schema_version: ANALYSIS_CACHE_SCHEMA_VERSION,
        incremental: snapshot.incremental.clone(),
        metadata: metadata.clone(),
        module_diagnostics: snapshot.module_diagnostics.clone(),
    })
    .context("unable to serialize analysis cache")?;
    fs::write(&cache_path, payload)
        .with_context(|| format!("unable to write {}", cache_path.display()))?;
    Ok(cache_path)
}

fn current_project_module_keys(syntax_trees: &[typepython_syntax::SyntaxTree]) -> BTreeSet<String> {
    syntax_trees.iter().map(|tree| tree.source.logical_module.clone()).collect()
}

fn current_project_syntax_by_module(
    syntax_trees: &[typepython_syntax::SyntaxTree],
) -> BTreeMap<String, typepython_syntax::SyntaxTree> {
    syntax_trees.iter().map(|tree| (tree.source.logical_module.clone(), tree.clone())).collect()
}

fn apply_type_ignores_for_module(
    tree: &typepython_syntax::SyntaxTree,
    diagnostics: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let mut report = DiagnosticReport { diagnostics };
    apply_type_ignore_directives(std::slice::from_ref(tree), &mut report);
    report.diagnostics
}

fn diagnostics_report_from_modules(
    module_diagnostics: &BTreeMap<String, Vec<Diagnostic>>,
) -> DiagnosticReport {
    let diagnostics =
        module_diagnostics.values().flat_map(|diagnostics| diagnostics.iter().cloned()).collect();
    DiagnosticReport { diagnostics }
}

fn lowering_diagnostics_by_module(
    results: &[LoweringResult],
    syntax_by_module: &BTreeMap<String, typepython_syntax::SyntaxTree>,
) -> BTreeMap<String, Vec<Diagnostic>> {
    let module_by_path = syntax_by_module
        .iter()
        .map(|(module_key, tree)| (tree.source.path.clone(), module_key.clone()))
        .collect::<BTreeMap<_, _>>();
    results
        .iter()
        .filter_map(|result| {
            module_by_path
                .get(&result.module.source_path)
                .cloned()
                .map(|module_key| (module_key, result.diagnostics.diagnostics.clone()))
        })
        .collect()
}

fn can_reuse_cached_pipeline_outputs(
    config: &ConfigHandle,
    analyzed: &AnalyzedPipelineState,
    previous_manifest: Option<&MaterializedBuildManifest>,
) -> bool {
    previous_manifest.is_some_and(|manifest| {
        manifest.incremental == analyzed.incremental
            && manifest.emit_plan == cached_emit_artifacts(&analyzed.pre_lowering_emit_plan)
            && manifest.runtime_validators == config.config.emit.runtime_validators
            && !verify_build_artifacts(config, &analyzed.pre_lowering_emit_plan).has_errors()
    })
}

fn reusable_cached_pipeline_snapshot(
    discovered_sources: usize,
    analyzed: AnalyzedPipelineState,
    module_diagnostics: BTreeMap<String, Vec<Diagnostic>>,
    diagnostics: DiagnosticReport,
) -> PipelineSnapshot {
    PipelineSnapshot {
        lowered_modules: Vec::new(),
        emit_plan: analyzed.pre_lowering_emit_plan,
        stub_contexts: BTreeMap::new(),
        incremental: analyzed.incremental,
        module_diagnostics,
        tracked_modules: analyzed.tracked_modules,
        discovered_sources,
        emit_blocked_by_pipeline: false,
        diagnostics,
    }
}

pub(crate) fn run_with_pipeline(
    command: &str,
    args: RunArgs,
    create_dirs: bool,
    mut notes: Vec<String>,
) -> Result<ExitCode> {
    let config = load_project(args.project.as_ref())?;

    if create_dirs {
        fs::create_dir_all(config.resolve_relative_path(&config.config.project.out_dir))?;
        fs::create_dir_all(config.resolve_relative_path(&config.config.project.cache_dir))?;
    }

    notes.push(String::from(
        "compiler pipeline, artifact planning, and verification completed for the loaded project",
    ));

    let snapshot = run_pipeline(&config)?;
    let _ = persist_pipeline_analysis_state(&config, &snapshot)?;
    let summary = CommandSummary {
        command: String::from(command),
        config_path: config.config_path.display().to_string(),
        config_source: config.source,
        discovered_sources: snapshot.discovered_sources,
        lowered_modules: snapshot.lowered_modules.len(),
        planned_artifacts: snapshot.emit_plan.len(),
        tracked_modules: snapshot.tracked_modules,
        notes,
    };

    print_summary(args.format, &summary, &snapshot.diagnostics)?;
    Ok(exit_code(&snapshot.diagnostics))
}

pub(crate) fn run_pipeline(config: &ConfigHandle) -> Result<PipelineSnapshot> {
    let discovery = collect_source_paths(config)?;
    if discovery.diagnostics.has_errors() {
        return Ok(blocked_pipeline_snapshot(discovery.sources.len(), discovery.diagnostics));
    }

    let prepared = prepare_pipeline_syntax(config, &discovery.sources)?;
    let mut parse_diagnostics = collect_parse_diagnostics(&prepared.all_syntax_trees);
    apply_type_ignore_directives(&prepared.syntax_trees, &mut parse_diagnostics);
    if parse_diagnostics.has_errors() {
        return Ok(blocked_pipeline_snapshot(prepared.source_paths.len(), parse_diagnostics));
    }

    let previous = load_previous_incremental_state(config)?;
    let previous_analysis_cache = load_previous_analysis_cache(config)?;
    let previous_manifest = load_previous_materialized_build_manifest(config)?;
    let analyzed = analyze_pipeline_state(config, &prepared, previous.as_ref())?;
    let project_module_keys = current_project_module_keys(&prepared.syntax_trees);
    let project_syntax_by_module = current_project_syntax_by_module(&prepared.syntax_trees);
    let analysis_metadata = analysis_cache_metadata(config, &analyzed.incremental.metadata);
    let previous_dependency_index =
        previous.as_ref().map(|previous| dependency_index_from_summaries(&previous.summaries));
    let reusable_analysis_cache = previous
        .as_ref()
        .zip(previous_analysis_cache.as_ref())
        .filter(|(previous, cache)| {
            cache.incremental == **previous && cache.metadata == analysis_metadata
        })
        .map(|(_, cache)| cache);
    let current_module_keys =
        analyzed.graph.nodes.iter().map(|node| node.module_key.clone()).collect::<BTreeSet<_>>();
    let mut module_diagnostics =
        reusable_analysis_cache.map(|cache| cache.module_diagnostics.clone()).unwrap_or_default();
    module_diagnostics.retain(|module_key, _| project_module_keys.contains(module_key));
    let rechecked_modules = if reusable_analysis_cache.is_some() {
        affected_modules(
            previous_dependency_index.as_ref(),
            &analyzed.dependency_index,
            &analyzed.direct_changes,
            &analyzed.summary_changed_modules,
        )
        .into_iter()
        .filter(|module_key| current_module_keys.contains(module_key))
        .collect::<BTreeSet<_>>()
    } else {
        current_module_keys.clone()
    };
    if !rechecked_modules.is_empty() {
        let module_result = check_modules_with_binding_metadata(
            &analyzed.graph,
            &analyzed.bindings,
            &rechecked_modules,
            config.config.typing.require_explicit_overrides,
            config.config.typing.enable_sealed_exhaustiveness,
            config.config.typing.report_deprecated,
            config.config.typing.strict,
            config.config.typing.warn_unsafe,
            config.config.typing.imports,
            None,
        );
        for (module_key, diagnostics) in module_result.diagnostics_by_module {
            if let Some(tree) = project_syntax_by_module.get(&module_key) {
                module_diagnostics
                    .insert(module_key, apply_type_ignores_for_module(tree, diagnostics));
            }
        }
    }
    let diagnostics = diagnostics_report_from_modules(&module_diagnostics);
    if previous.is_some()
        && can_reuse_cached_pipeline_outputs(config, &analyzed, previous_manifest.as_ref())
    {
        return Ok(reusable_cached_pipeline_snapshot(
            prepared.source_paths.len(),
            analyzed,
            module_diagnostics,
            diagnostics,
        ));
    }

    let modules_requiring_materialization = match previous_manifest.as_ref() {
        Some(manifest)
            if manifest.incremental == analyzed.incremental
                && manifest.runtime_validators == config.config.emit.runtime_validators
                && verify_build_artifacts(config, &analyzed.pre_lowering_emit_plan)
                    .has_errors() =>
        {
            project_module_keys.clone()
        }
        Some(manifest)
            if manifest.incremental == analyzed.incremental
                && manifest.runtime_validators == config.config.emit.runtime_validators =>
        {
            BTreeSet::new()
        }
        Some(manifest)
            if manifest.incremental.metadata != analyzed.incremental.metadata
                || manifest.runtime_validators != config.config.emit.runtime_validators =>
        {
            project_module_keys.clone()
        }
        Some(manifest) => affected_modules(
            Some(&dependency_index_from_summaries(&manifest.incremental.summaries)),
            &analyzed.dependency_index,
            &source_change_modules(&manifest.incremental, &analyzed.incremental),
            &snapshot_diff_modules(&diff(&manifest.incremental, &analyzed.incremental)),
        )
        .into_iter()
        .filter(|module_key| project_module_keys.contains(module_key))
        .collect(),
        None => project_module_keys.clone(),
    };
    let mut modules_to_lower = rechecked_modules
        .iter()
        .filter(|module_key| project_module_keys.contains(*module_key))
        .cloned()
        .collect::<BTreeSet<_>>();
    modules_to_lower.extend(modules_requiring_materialization);
    let lowering_options = LoweringOptions {
        target_python: config.config.project.target_python,
        emit_style: config.config.emit.emit_style,
    };
    let lowering_results: Vec<_> = prepared
        .syntax_trees
        .par_iter()
        .filter(|tree| modules_to_lower.contains(&tree.source.logical_module))
        .map(|tree| lower_with_options(tree, &lowering_options))
        .collect();
    let lowering_report = collect_lowering_diagnostics(&lowering_results);
    let lowering_diagnostics =
        lowering_diagnostics_by_module(&lowering_results, &project_syntax_by_module);
    for (module_key, diagnostics) in &lowering_diagnostics {
        module_diagnostics.entry(module_key.clone()).or_default().extend(diagnostics.clone());
    }
    let diagnostics = diagnostics_report_from_modules(&module_diagnostics);
    if lowering_report.has_errors() {
        return Ok(blocked_pipeline_snapshot_with_incremental(
            prepared.source_paths.len(),
            diagnostics,
            analyzed.incremental,
            module_diagnostics,
            analyzed.tracked_modules,
        ));
    }

    let lowered_modules: Vec<_> =
        lowering_results.into_iter().map(|result| result.module).collect();
    let lowered_module_by_path = lowered_modules
        .iter()
        .map(|module| (module.source_path.clone(), module.clone()))
        .collect::<BTreeMap<_, _>>();
    let lowered_syntax_trees = prepared
        .syntax_trees
        .iter()
        .filter(|tree| modules_to_lower.contains(&tree.source.logical_module))
        .cloned()
        .collect::<Vec<_>>();
    let stub_contexts =
        build_typepython_stub_contexts(&lowered_syntax_trees, &lowered_modules, &analyzed.graph);
    for tree in &lowered_syntax_trees {
        let Some(module) = lowered_module_by_path.get(&tree.source.path) else {
            continue;
        };
        let module_stub_contexts = stub_contexts
            .get(&tree.source.path)
            .cloned()
            .map(|context| BTreeMap::from([(tree.source.path.clone(), context)]))
            .unwrap_or_default();
        let diagnostics = public_surface_completeness_diagnostics(
            config,
            std::slice::from_ref(tree),
            std::slice::from_ref(module),
            &module_stub_contexts,
        );
        module_diagnostics
            .entry(tree.source.logical_module.clone())
            .or_default()
            .extend(diagnostics.diagnostics);
    }
    let diagnostics = diagnostics_report_from_modules(&module_diagnostics);

    Ok(PipelineSnapshot {
        lowered_modules,
        emit_plan: analyzed.pre_lowering_emit_plan,
        stub_contexts,
        incremental: analyzed.incremental,
        module_diagnostics,
        tracked_modules: analyzed.tracked_modules,
        discovered_sources: prepared.source_paths.len(),
        emit_blocked_by_pipeline: false,
        diagnostics,
    })
}

fn load_previous_incremental_state(config: &ConfigHandle) -> Result<Option<IncrementalState>> {
    let snapshot_path =
        config.resolve_relative_path(&config.config.project.cache_dir).join("snapshot.json");
    if !snapshot_path.is_file() {
        return Ok(None);
    }
    let rendered = fs::read_to_string(&snapshot_path)
        .with_context(|| format!("unable to read {}", snapshot_path.display()))?;
    decode_snapshot(&rendered)
        .map(Some)
        .map_err(|error| anyhow::anyhow!("unable to decode {}: {}", snapshot_path.display(), error))
}

pub(crate) fn collect_parse_diagnostics(
    syntax_trees: &[typepython_syntax::SyntaxTree],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();

    for tree in syntax_trees {
        diagnostics.diagnostics.extend(tree.diagnostics.diagnostics.iter().cloned());
    }

    diagnostics
}

fn collect_lowering_diagnostics(lowering_results: &[LoweringResult]) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();

    for result in lowering_results {
        diagnostics.diagnostics.extend(result.diagnostics.diagnostics.iter().cloned());
    }

    diagnostics
}

pub(crate) fn write_incremental_snapshot(
    cache_dir: &Path,
    snapshot: &IncrementalState,
) -> Result<PathBuf> {
    fs::create_dir_all(cache_dir)
        .with_context(|| format!("unable to create cache directory {}", cache_dir.display()))?;
    let snapshot_path = cache_dir.join("snapshot.json");
    let payload = encode_snapshot(snapshot).context("unable to serialize incremental snapshot")?;
    fs::write(&snapshot_path, payload)
        .with_context(|| format!("unable to write {}", snapshot_path.display()))?;
    Ok(snapshot_path)
}

pub(crate) fn persist_pipeline_analysis_state(
    config: &ConfigHandle,
    snapshot: &PipelineSnapshot,
) -> Result<Option<(PathBuf, PathBuf)>> {
    if snapshot.incremental.fingerprints.is_empty() {
        return Ok(None);
    }
    let cache_dir = config.resolve_relative_path(&config.config.project.cache_dir);
    let snapshot_path = write_incremental_snapshot(&cache_dir, &snapshot.incremental)?;
    let analysis_path = write_analysis_cache(
        config,
        &analysis_cache_metadata(config, &snapshot.incremental.metadata),
        snapshot,
    )?;
    Ok(Some((snapshot_path, analysis_path)))
}

fn write_materialized_build_manifest(
    config: &ConfigHandle,
    snapshot: &PipelineSnapshot,
) -> Result<PathBuf> {
    let cache_dir = config.resolve_relative_path(&config.config.project.cache_dir);
    fs::create_dir_all(&cache_dir)
        .with_context(|| format!("unable to create cache directory {}", cache_dir.display()))?;
    let manifest_path = materialized_build_manifest_path(config);
    let payload = serde_json::to_string_pretty(&MaterializedBuildManifest {
        schema_version: MATERIALIZED_BUILD_MANIFEST_SCHEMA_VERSION,
        incremental: snapshot.incremental.clone(),
        emit_plan: cached_emit_artifacts(&snapshot.emit_plan),
        runtime_validators: config.config.emit.runtime_validators,
    })
    .context("unable to serialize materialized build manifest")?;
    fs::write(&manifest_path, payload)
        .with_context(|| format!("unable to write {}", manifest_path.display()))?;
    Ok(manifest_path)
}

pub(crate) fn compile_runtime_bytecode(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
) -> Result<usize> {
    let interpreter = resolve_python_executable(config);
    let runtime_paths =
        artifacts.iter().filter_map(|artifact| artifact.runtime_path.clone()).collect::<Vec<_>>();

    runtime_paths
        .par_iter()
        .try_for_each(|runtime_path| compile_single_runtime_bytecode(&interpreter, runtime_path))?;

    Ok(runtime_paths.len())
}

fn compile_single_runtime_bytecode(interpreter: &Path, runtime_path: &Path) -> Result<()> {
    let bytecode_path = bytecode_path_for(runtime_path)?;
    if let Some(parent) = bytecode_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("unable to create bytecode directory {}", parent.display()))?;
    }
    let status = ProcessCommand::new(interpreter)
        .args([
            "-c",
            "import py_compile, sys; py_compile.compile(sys.argv[1], cfile=sys.argv[2], doraise=True)",
        ])
        .arg(runtime_path)
        .arg(&bytecode_path)
        .status()
        .with_context(|| {
            format!(
                "unable to run Python bytecode compiler `{}` for {}",
                interpreter.display(),
                runtime_path.display()
            )
        })?;
    if !status.success() {
        anyhow::bail!(
            "Python bytecode compiler `{}` failed for {} with status {}",
            interpreter.display(),
            runtime_path.display(),
            status
        );
    }

    Ok(())
}

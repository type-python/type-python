use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode},
};

use anyhow::{Context, Result};
use notify::RecursiveMode;
use typepython_binding::bind;
use typepython_checking::{
    check_with_binding_metadata, collect_effective_callable_stub_overrides,
    collect_synthetic_method_stubs, semantic_incremental_state_with_binding_metadata,
};
use typepython_config::ConfigHandle;
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_emit::{
    EmitArtifact, InferredStubMode, PlannedModuleSource, StubCallableOverride, StubSyntheticMethod,
    StubValueOverride, TypePythonStubContext, generate_inferred_stub_source, plan_emits,
    plan_emits_for_sources, write_runtime_outputs,
};
use typepython_graph::build;
use typepython_incremental::{IncrementalState, decode_snapshot, diff, encode_snapshot};
use typepython_lowering::{LoweredModule, LoweringOptions, LoweringResult, lower_with_options};
use typepython_syntax::{SourceFile, SourceKind, apply_type_ignore_directives};

use crate::cli::{CleanArgs, OutputFormat, RunArgs};
use crate::discovery::{
    DiscoveredSource, bundled_stdlib_snapshot_identity, bundled_stdlib_sources,
    collect_source_paths, external_resolution_sources,
};
use crate::verification::{public_surface_completeness_diagnostics, verify_build_artifacts};
use crate::{
    CommandSummary, bytecode_path_for, exit_code, load_project, print_summary,
    remove_dir_if_exists, resolve_python_executable,
};

#[derive(Debug)]
pub(crate) struct PipelineSnapshot {
    pub(crate) lowered_modules: Vec<LoweredModule>,
    pub(crate) emit_plan: Vec<EmitArtifact>,
    pub(crate) stub_contexts: BTreeMap<PathBuf, TypePythonStubContext>,
    pub(crate) incremental: IncrementalState,
    pub(crate) tracked_modules: usize,
    pub(crate) discovered_sources: usize,
    pub(crate) diagnostics: DiagnosticReport,
}

pub(crate) fn should_emit_build_outputs(
    config: &ConfigHandle,
    diagnostics: &DiagnosticReport,
) -> bool {
    !diagnostics.has_errors() || !config.config.emit.no_emit_on_error
}

pub(crate) fn build_diagnostics(
    config: &ConfigHandle,
    diagnostics: &DiagnosticReport,
) -> DiagnosticReport {
    let mut build_diagnostics = diagnostics.clone();

    if diagnostics.has_errors() && config.config.emit.no_emit_on_error {
        build_diagnostics.push(Diagnostic::error(
            "TPY5002",
            format!("emit blocked by `emit.no_emit_on_error` for {}", config.config_dir.display()),
        ));
    }

    build_diagnostics
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
    let mut diagnostics = build_diagnostics(config, &snapshot.diagnostics);
    if should_emit_build_outputs(config, &snapshot.diagnostics) {
        let runtime_summary = match write_runtime_outputs(
            &snapshot.emit_plan,
            &snapshot.lowered_modules,
            config.config.emit.runtime_validators,
            Some(&snapshot.stub_contexts),
        ) {
            Ok(runtime_summary) => runtime_summary,
            Err(error) if error.to_string().contains("TPY5001") => {
                diagnostics.push(Diagnostic::error("TPY5001", error.to_string()));
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
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "unable to write runtime artifacts under {}",
                        config.resolve_relative_path(&config.config.project.out_dir).display()
                    )
                });
            }
        };
        notes.push(format!(
            "wrote {} runtime artifact(s), {} stub artifact(s), {} `py.typed` marker(s)",
            runtime_summary.runtime_files_written,
            runtime_summary.stub_files_written,
            runtime_summary.py_typed_written
        ));
        if config.config.emit.emit_pyc {
            let compiled_pyc = compile_runtime_bytecode(config, &snapshot.emit_plan)?;
            notes.push(format!("compiled {} runtime artifact(s) to bytecode", compiled_pyc));
        }
        let snapshot_path = write_incremental_snapshot(
            &config.resolve_relative_path(&config.config.project.cache_dir),
            &snapshot.incremental,
        )?;
        notes.push(format!(
            "cached {} module fingerprint(s) at {}",
            snapshot.incremental.fingerprints.len(),
            snapshot_path.display()
        ));
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

fn ensure_output_dirs(config: &ConfigHandle) -> Result<()> {
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

#[derive(Debug, Clone)]
struct ShadowStub {
    logical_module: String,
    stub_path: PathBuf,
}

pub(crate) fn load_syntax_trees(
    sources: &[DiscoveredSource],
    enable_conditional_returns: bool,
) -> Result<Vec<typepython_syntax::SyntaxTree>> {
    sources
        .iter()
        .map(|source| {
            let mut source_file = SourceFile::from_path(&source.path)
                .with_context(|| format!("unable to read {}", source.path.display()))?;
            source_file.logical_module = source.logical_module.clone();
            Ok(typepython_syntax::parse_with_options(
                source_file,
                typepython_syntax::ParseOptions { enable_conditional_returns },
            ))
        })
        .collect::<Result<Vec<_>>>()
}

fn load_support_syntax_trees(
    config: &ConfigHandle,
    surface_syntax_trees: &[typepython_syntax::SyntaxTree],
) -> Result<Vec<typepython_syntax::SyntaxTree>> {
    let project_modules = surface_syntax_trees
        .iter()
        .map(|tree| tree.source.logical_module.clone())
        .collect::<BTreeSet<_>>();
    let import_paths = collect_import_source_paths(surface_syntax_trees);
    let external_import_paths = import_paths
        .into_iter()
        .filter(|import_path| !import_resolves_within_modules(import_path, &project_modules))
        .collect::<Vec<_>>();
    if external_import_paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut support_sources = bundled_stdlib_sources(&config.config.project.target_python)?;
    support_sources.extend(external_resolution_sources(config)?);
    let mut sources_by_module = BTreeMap::<String, Vec<DiscoveredSource>>::new();
    for source in support_sources {
        sources_by_module.entry(source.logical_module.clone()).or_default().push(source);
    }

    let mut queued_modules = BTreeSet::new();
    let mut queue = VecDeque::new();
    for import_path in external_import_paths {
        for module_key in matching_support_module_keys(&import_path, &sources_by_module) {
            if queued_modules.insert(module_key.clone()) {
                queue.push_back(module_key);
            }
        }
    }

    let mut loaded_modules = BTreeSet::new();
    let mut loaded_paths = BTreeSet::new();
    let mut support_syntax_trees = Vec::new();

    while let Some(module_key) = queue.pop_front() {
        if !loaded_modules.insert(module_key.clone()) {
            continue;
        }
        let Some(module_sources) = sources_by_module.get(&module_key) else {
            continue;
        };

        for source in module_sources {
            if !loaded_paths.insert(source.path.clone()) {
                continue;
            }

            let tree = if source.load_as_inferred_stub {
                let runtime_source = fs::read_to_string(&source.path)
                    .with_context(|| format!("unable to read {}", source.path.display()))?;
                let stub_source =
                    generate_inferred_stub_source(&runtime_source, InferredStubMode::Shadow)
                        .with_context(|| {
                            format!(
                                "unable to synthesize shadow stub for {}",
                                source.path.display()
                            )
                        })?;
                typepython_syntax::parse_with_options(
                    SourceFile {
                        path: source.path.clone(),
                        kind: SourceKind::Stub,
                        logical_module: source.logical_module.clone(),
                        text: stub_source,
                    },
                    typepython_syntax::ParseOptions {
                        enable_conditional_returns: config.config.typing.conditional_returns,
                    },
                )
            } else {
                let mut source_file = SourceFile::from_path(&source.path)
                    .with_context(|| format!("unable to read {}", source.path.display()))?;
                source_file.logical_module = source.logical_module.clone();
                typepython_syntax::parse_with_options(
                    source_file,
                    typepython_syntax::ParseOptions {
                        enable_conditional_returns: config.config.typing.conditional_returns,
                    },
                )
            };
            for import_path in collect_import_source_paths(std::slice::from_ref(&tree)) {
                for nested_module_key in
                    matching_support_module_keys(&import_path, &sources_by_module)
                {
                    if queued_modules.insert(nested_module_key.clone()) {
                        queue.push_back(nested_module_key);
                    }
                }
            }
            support_syntax_trees.push(tree);
        }
    }

    support_syntax_trees.sort_by(|left, right| left.source.path.cmp(&right.source.path));
    Ok(support_syntax_trees)
}

fn collect_import_source_paths(syntax_trees: &[typepython_syntax::SyntaxTree]) -> Vec<String> {
    syntax_trees
        .iter()
        .flat_map(|tree| tree.statements.iter())
        .filter_map(|statement| match statement {
            typepython_syntax::SyntaxStatement::Import(statement) => Some(
                statement
                    .bindings
                    .iter()
                    .map(|binding| binding.source_path.clone())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect()
}

fn import_resolves_within_modules(import_path: &str, module_keys: &BTreeSet<String>) -> bool {
    module_path_prefixes(import_path).any(|module_key| module_keys.contains(module_key))
}

fn matching_support_module_keys(
    import_path: &str,
    sources_by_module: &BTreeMap<String, Vec<DiscoveredSource>>,
) -> Vec<String> {
    module_path_prefixes(import_path)
        .filter(|module_key| sources_by_module.contains_key(*module_key))
        .map(str::to_owned)
        .collect()
}

fn module_path_prefixes(import_path: &str) -> impl Iterator<Item = &str> {
    let mut candidates = Vec::new();
    let mut current = import_path.strip_suffix(".*").unwrap_or(import_path);
    loop {
        if !current.is_empty() {
            candidates.push(current);
        }
        let Some((parent, _)) = current.rsplit_once('.') else {
            break;
        };
        current = parent;
    }
    candidates.into_iter()
}

fn write_shadow_stubs(
    config: &ConfigHandle,
    syntax_trees: &[typepython_syntax::SyntaxTree],
) -> Result<Vec<ShadowStub>> {
    let cache_root =
        config.resolve_relative_path(&config.config.project.cache_dir).join("shadow-stubs");
    fs::create_dir_all(&cache_root)
        .with_context(|| format!("unable to create {}", cache_root.display()))?;

    let local_stub_modules: BTreeSet<_> = syntax_trees
        .iter()
        .filter(|tree| tree.source.kind == SourceKind::Stub)
        .map(|tree| tree.source.logical_module.clone())
        .collect();

    let mut written = Vec::new();
    for tree in syntax_trees {
        if tree.source.kind != SourceKind::Python
            || local_stub_modules.contains(&tree.source.logical_module)
        {
            continue;
        }

        let relative_path = shadow_stub_relative_path(&tree.source);
        let stub_path = cache_root.join(relative_path);
        if let Some(parent) = stub_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("unable to create {}", parent.display()))?;
        }
        let stub_source =
            generate_inferred_stub_source(&tree.source.text, InferredStubMode::Shadow)
                .with_context(|| {
                    format!(
                        "unable to generate inferred shadow stub for {}",
                        tree.source.path.display()
                    )
                })?;
        fs::write(&stub_path, stub_source)
            .with_context(|| format!("unable to write {}", stub_path.display()))?;
        written.push(ShadowStub { logical_module: tree.source.logical_module.clone(), stub_path });
    }

    Ok(written)
}

fn shadow_stub_relative_path(source: &SourceFile) -> PathBuf {
    let mut path = PathBuf::new();
    let mut parts = source.logical_module.split('.').collect::<Vec<_>>();
    if source.path.file_name().is_some_and(|name| name == "__init__.py") {
        for part in parts {
            path.push(part);
        }
        path.push("__init__.pyi");
    } else {
        let module_name = parts.pop().unwrap_or("module");
        for part in parts {
            path.push(part);
        }
        path.push(format!("{module_name}.pyi"));
    }
    path
}

fn load_shadow_stub_syntax_trees(
    shadow_stubs: &[ShadowStub],
    enable_conditional_returns: bool,
) -> Result<Vec<typepython_syntax::SyntaxTree>> {
    shadow_stubs
        .iter()
        .map(|shadow_stub| {
            let mut source_file = SourceFile::from_path(&shadow_stub.stub_path)
                .with_context(|| format!("unable to read {}", shadow_stub.stub_path.display()))?;
            source_file.logical_module = shadow_stub.logical_module.clone();
            Ok(typepython_syntax::parse_with_options(
                source_file,
                typepython_syntax::ParseOptions { enable_conditional_returns },
            ))
        })
        .collect()
}

fn replace_local_python_surfaces_with_shadow_stubs(
    syntax_trees: &[typepython_syntax::SyntaxTree],
    shadow_stub_syntax: Vec<typepython_syntax::SyntaxTree>,
) -> Vec<typepython_syntax::SyntaxTree> {
    let shadow_modules: BTreeSet<_> =
        shadow_stub_syntax.iter().map(|tree| tree.source.logical_module.clone()).collect();
    let mut surfaces = syntax_trees
        .iter()
        .filter(|tree| {
            !(tree.source.kind == SourceKind::Python
                && shadow_modules.contains(&tree.source.logical_module))
        })
        .cloned()
        .collect::<Vec<_>>();
    surfaces.extend(shadow_stub_syntax);
    surfaces
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
        return Ok(PipelineSnapshot {
            lowered_modules: Vec::new(),
            emit_plan: Vec::new(),
            stub_contexts: BTreeMap::new(),
            incremental: IncrementalState::default(),
            tracked_modules: 0,
            discovered_sources: discovery.sources.len(),
            diagnostics: discovery.diagnostics,
        });
    }

    let source_paths: Vec<_> = discovery.sources.iter().map(|source| source.path.clone()).collect();
    let syntax_trees =
        load_syntax_trees(&discovery.sources, config.config.typing.conditional_returns)?;
    let shadow_stubs = if config.config.typing.infer_passthrough {
        write_shadow_stubs(config, &syntax_trees)?
    } else {
        Vec::new()
    };
    let mut all_syntax_trees = if config.config.typing.infer_passthrough && !shadow_stubs.is_empty()
    {
        let shadow_stub_syntax =
            load_shadow_stub_syntax_trees(&shadow_stubs, config.config.typing.conditional_returns)?;
        replace_local_python_surfaces_with_shadow_stubs(&syntax_trees, shadow_stub_syntax)
    } else {
        syntax_trees.clone()
    };
    let checking_support_syntax = load_support_syntax_trees(config, &all_syntax_trees)?;
    all_syntax_trees.extend(checking_support_syntax);
    let mut parse_diagnostics = collect_parse_diagnostics(&all_syntax_trees);
    apply_type_ignore_directives(&syntax_trees, &mut parse_diagnostics);
    if parse_diagnostics.has_errors() {
        return Ok(PipelineSnapshot {
            lowered_modules: Vec::new(),
            emit_plan: Vec::new(),
            stub_contexts: BTreeMap::new(),
            incremental: IncrementalState::default(),
            tracked_modules: 0,
            discovered_sources: source_paths.len(),
            diagnostics: parse_diagnostics,
        });
    }

    let bindings: Vec<_> = all_syntax_trees.iter().map(bind).collect();
    let graph = build(&bindings);
    let mut diagnostics = check_with_binding_metadata(
        &graph,
        &bindings,
        config.config.typing.require_explicit_overrides,
        config.config.typing.enable_sealed_exhaustiveness,
        config.config.typing.report_deprecated,
        config.config.typing.strict,
        config.config.typing.warn_unsafe,
        config.config.typing.imports,
        None,
    )
    .diagnostics;
    diagnostics = filter_project_diagnostics(&diagnostics, &source_paths);
    apply_type_ignore_directives(&syntax_trees, &mut diagnostics);

    if diagnostics.has_errors() {
        return Ok(PipelineSnapshot {
            lowered_modules: Vec::new(),
            emit_plan: Vec::new(),
            stub_contexts: BTreeMap::new(),
            incremental: IncrementalState::default(),
            tracked_modules: 0,
            discovered_sources: source_paths.len(),
            diagnostics,
        });
    }

    let stdlib_snapshot =
        Some(bundled_stdlib_snapshot_identity(&config.config.project.target_python)?);
    let incremental = semantic_incremental_state_with_binding_metadata(
        &graph,
        &bindings,
        config.config.typing.imports,
        None,
        stdlib_snapshot,
    );
    let tracked_modules = incremental.fingerprints.len();
    let planned_sources: Vec<_> = syntax_trees
        .iter()
        .map(|tree| PlannedModuleSource {
            source_path: tree.source.path.clone(),
            source_kind: tree.source.kind,
        })
        .collect();
    let emit_plan = plan_emits_for_sources(config, &planned_sources);
    if let Some(previous) = load_previous_incremental_state(config)? {
        let snapshot_diff = diff(&previous, &incremental);
        if snapshot_diff.added.is_empty()
            && snapshot_diff.removed.is_empty()
            && snapshot_diff.changed.is_empty()
            && previous.summaries == incremental.summaries
            && previous.stdlib_snapshot == incremental.stdlib_snapshot
            && !verify_build_artifacts(config, &emit_plan).has_errors()
        {
            return Ok(PipelineSnapshot {
                lowered_modules: Vec::new(),
                emit_plan,
                stub_contexts: BTreeMap::new(),
                incremental,
                tracked_modules,
                discovered_sources: source_paths.len(),
                diagnostics,
            });
        }
    }

    let lowering_options =
        LoweringOptions { target_python: config.config.project.target_python.clone() };
    let lowering_results: Vec<_> =
        syntax_trees.iter().map(|tree| lower_with_options(tree, &lowering_options)).collect();
    let lowering_diagnostics = collect_lowering_diagnostics(&lowering_results);
    if lowering_diagnostics.has_errors() {
        return Ok(PipelineSnapshot {
            lowered_modules: Vec::new(),
            emit_plan: Vec::new(),
            stub_contexts: BTreeMap::new(),
            incremental: IncrementalState::default(),
            tracked_modules: 0,
            discovered_sources: source_paths.len(),
            diagnostics: lowering_diagnostics,
        });
    }

    let lowered_modules: Vec<_> =
        lowering_results.into_iter().map(|result| result.module).collect();
    let stub_contexts = build_typepython_stub_contexts(&syntax_trees, &lowered_modules, &graph);
    diagnostics.diagnostics.extend(
        public_surface_completeness_diagnostics(
            config,
            &syntax_trees,
            &lowered_modules,
            &stub_contexts,
        )
        .diagnostics,
    );
    if diagnostics.has_errors() {
        return Ok(PipelineSnapshot {
            lowered_modules: Vec::new(),
            emit_plan: Vec::new(),
            stub_contexts: BTreeMap::new(),
            incremental: IncrementalState::default(),
            tracked_modules: 0,
            discovered_sources: source_paths.len(),
            diagnostics,
        });
    }
    let emit_plan = plan_emits(config, &lowered_modules);

    Ok(PipelineSnapshot {
        lowered_modules,
        emit_plan,
        stub_contexts,
        incremental,
        tracked_modules,
        discovered_sources: source_paths.len(),
        diagnostics,
    })
}

fn build_typepython_stub_contexts(
    syntax_trees: &[typepython_syntax::SyntaxTree],
    _lowered_modules: &[LoweredModule],
    graph: &typepython_graph::ModuleGraph,
) -> BTreeMap<PathBuf, TypePythonStubContext> {
    let mut contexts = syntax_trees
        .iter()
        .filter(|tree| tree.source.kind == SourceKind::TypePython)
        .map(|tree| {
            let mut context = TypePythonStubContext::default();
            collect_value_stub_overrides(&tree.statements, &mut context.value_overrides);
            (tree.source.path.clone(), context)
        })
        .collect::<BTreeMap<_, _>>();
    let module_paths = syntax_trees
        .iter()
        .map(|tree| (tree.source.logical_module.clone(), tree.source.path.clone()))
        .collect::<BTreeMap<_, _>>();

    for override_signature in collect_effective_callable_stub_overrides(graph) {
        let Some(path) = module_paths.get(&override_signature.module_key) else {
            continue;
        };
        let Some(context) = contexts.get_mut(path) else {
            continue;
        };
        context.callable_overrides.push(StubCallableOverride {
            line: override_signature.line,
            params: override_signature.params,
            returns: Some(override_signature.returns),
            use_async_syntax: false,
            drop_non_builtin_decorators: true,
        });
    }

    for synthetic_method in collect_synthetic_method_stubs(graph) {
        let Some(path) = module_paths.get(&synthetic_method.module_key) else {
            continue;
        };
        let Some(context) = contexts.get_mut(path) else {
            continue;
        };
        context.synthetic_methods.push(StubSyntheticMethod {
            class_line: synthetic_method.class_line,
            name: synthetic_method.name,
            method_kind: synthetic_method.method_kind,
            params: synthetic_method.params,
            returns: synthetic_method.returns,
        });
    }

    contexts
}

fn collect_value_stub_overrides(
    statements: &[typepython_syntax::SyntaxStatement],
    overrides: &mut Vec<StubValueOverride>,
) {
    for statement in statements {
        match statement {
            typepython_syntax::SyntaxStatement::Value(statement)
                if statement.annotation.is_none()
                    && statement.owner_name.is_none()
                    && statement.value_type.as_deref().is_some_and(|value| !value.is_empty()) =>
            {
                overrides.push(StubValueOverride {
                    line: statement.line,
                    annotation: statement.value_type.clone().unwrap_or_default(),
                });
            }
            typepython_syntax::SyntaxStatement::Interface(statement)
            | typepython_syntax::SyntaxStatement::DataClass(statement)
            | typepython_syntax::SyntaxStatement::SealedClass(statement)
            | typepython_syntax::SyntaxStatement::ClassDef(statement) => {
                collect_class_member_value_stub_overrides(&statement.members, overrides);
            }
            _ => {}
        }
    }
}

fn collect_class_member_value_stub_overrides(
    members: &[typepython_syntax::ClassMember],
    overrides: &mut Vec<StubValueOverride>,
) {
    for member in members {
        if member.kind == typepython_syntax::ClassMemberKind::Field
            && member.annotation.is_none()
            && member.value_type.as_deref().is_some_and(|value| !value.is_empty())
        {
            overrides.push(StubValueOverride {
                line: member.line,
                annotation: member.value_type.clone().unwrap_or_default(),
            });
        }
    }
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

fn filter_project_diagnostics(
    diagnostics: &DiagnosticReport,
    project_paths: &[PathBuf],
) -> DiagnosticReport {
    let project_paths =
        project_paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>();
    let diagnostics = diagnostics
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            if let Some(span) = &diagnostic.span {
                return project_paths.iter().any(|path| path == &span.path);
            }
            project_paths
                .iter()
                .any(|path| diagnostic.message.contains(&format!("module `{path}`")))
        })
        .cloned()
        .collect();
    DiagnosticReport { diagnostics }
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

pub(crate) fn compile_runtime_bytecode(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
) -> Result<usize> {
    let interpreter = resolve_python_executable(config);
    let mut compiled = 0usize;

    for artifact in artifacts {
        let Some(runtime_path) = &artifact.runtime_path else {
            continue;
        };
        let bytecode_path = bytecode_path_for(runtime_path)?;
        if let Some(parent) = bytecode_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("unable to create bytecode directory {}", parent.display())
            })?;
        }
        let status = ProcessCommand::new(&interpreter)
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
        compiled += 1;
    }

    Ok(compiled)
}

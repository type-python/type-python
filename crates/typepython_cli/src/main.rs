//! `typepython` command-line entrypoint.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    env, fs,
    io::Read,
    num::Wrapping,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode},
    sync::mpsc::{self, RecvTimeoutError},
    time::Duration,
};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use flate2::read::GzDecoder;
use glob::Pattern;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tar::Archive as TarArchive;
use tracing_subscriber::EnvFilter;
use typepython_binding::bind;
use typepython_checking::{
    check_with_options, collect_effective_callable_stub_overrides, collect_synthetic_method_stubs,
};
use typepython_config::{ConfigError, ConfigHandle, ConfigSource, load};
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_emit::{
    EmitArtifact, InferredStubMode, PlannedModuleSource, StubCallableOverride, StubSyntheticMethod,
    StubValueOverride, TypePythonStubContext, generate_inferred_stub_source,
    generate_typepython_stub_source, plan_emits, plan_emits_for_sources, write_runtime_outputs,
};
use typepython_graph::build;
use typepython_incremental::{IncrementalState, decode_snapshot, diff, encode_snapshot, snapshot};
use typepython_lowering::{LoweredModule, LoweringOptions, LoweringResult, lower_with_options};
use typepython_syntax::{SourceFile, SourceKind, apply_type_ignore_directives};
use zip::ZipArchive;

const CONFIG_TEMPLATE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates/typepython.toml"));
const INIT_SOURCE_TEMPLATE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates/src/app/__init__.tpy"));
const RUNTIME_PUBLIC_NAMES_SCRIPT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/runtime_public_names.py"));
const STATIC_ALL_NAMES_SCRIPT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/static_all_names.py"));

#[derive(Debug, Parser)]
#[command(name = "typepython", version, about = "Rust compiler and tooling for TypePython")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a starter TypePython config and source tree.
    Init(InitArgs),
    /// Load the project and run the TypePython checking pipeline.
    Check(RunArgs),
    /// Build Python output, stubs, and cache artifacts for the project.
    Build(RunArgs),
    /// Watch project inputs and rebuild/check when files change.
    Watch(RunArgs),
    /// Remove configured build and cache directories.
    Clean(CleanArgs),
    /// Start the TypePython language server.
    Lsp(RunArgs),
    /// Verify emitted artifacts and incremental state.
    Verify(VerifyArgs),
    /// Analyze migration coverage and dynamic boundaries.
    Migrate(MigrateArgs),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    /// Human-readable output.
    Text,
    /// Machine-readable JSON output.
    Json,
}

#[derive(Debug, Args)]
struct RunArgs {
    /// Project directory to search from.
    #[arg(long, value_name = "PATH")]
    project: Option<PathBuf>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Target directory for generated files.
    #[arg(long, value_name = "PATH", default_value = ".")]
    dir: PathBuf,
    /// Overwrite existing generated files.
    #[arg(long)]
    force: bool,
    #[arg(long)]
    embed_pyproject: bool,
}

#[derive(Debug, Args)]
struct CleanArgs {
    /// Project directory to search from.
    #[arg(long, value_name = "PATH")]
    project: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct VerifyArgs {
    #[command(flatten)]
    run: RunArgs,
    #[arg(
        long = "wheel",
        value_name = "PATH",
        help = "Verify a published wheel artifact against the build output"
    )]
    wheels: Vec<PathBuf>,
    #[arg(
        long = "sdist",
        value_name = "PATH",
        help = "Verify a published source distribution against the build output"
    )]
    sdists: Vec<PathBuf>,
}

#[derive(Debug, Args)]
struct MigrateArgs {
    #[command(flatten)]
    run: RunArgs,
    /// Emit the migration coverage report.
    #[arg(long)]
    report: bool,
    /// Generate inferred `.pyi` stubs for the selected `.py` files or directories.
    #[arg(long = "emit-stubs", value_name = "PATH")]
    emit_stubs: Vec<PathBuf>,
    /// Output directory for generated stubs. Defaults to writing alongside the source `.py` files.
    #[arg(long = "stub-out-dir", value_name = "PATH")]
    stub_out_dir: Option<PathBuf>,
}

#[derive(Debug)]
struct PipelineSnapshot {
    lowered_modules: Vec<LoweredModule>,
    emit_plan: Vec<EmitArtifact>,
    stub_contexts: BTreeMap<PathBuf, TypePythonStubContext>,
    incremental: IncrementalState,
    tracked_modules: usize,
    discovered_sources: usize,
    diagnostics: DiagnosticReport,
}

#[derive(Debug, Clone)]
struct DiscoveredSource {
    path: PathBuf,
    root: PathBuf,
    kind: SourceKind,
    logical_module: String,
    load_as_inferred_stub: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct ExternalSupportRoot {
    path: PathBuf,
    allow_untyped_runtime: bool,
}

#[derive(Debug)]
struct SourceDiscovery {
    sources: Vec<DiscoveredSource>,
    diagnostics: DiagnosticReport,
}

#[derive(Debug, Serialize)]
struct CommandSummary {
    command: String,
    config_path: String,
    config_source: ConfigSource,
    discovered_sources: usize,
    lowered_modules: usize,
    planned_artifacts: usize,
    tracked_modules: usize,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct MigrationReport {
    total_declarations: usize,
    known_declarations: usize,
    total_dynamic_boundaries: usize,
    total_unknown_boundaries: usize,
    files: Vec<MigrationCoverageEntry>,
    directories: Vec<MigrationCoverageEntry>,
    high_impact_untyped_files: Vec<MigrationImpactEntry>,
}

#[derive(Debug, Serialize, Clone)]
struct MigrationCoverageEntry {
    path: String,
    declarations: usize,
    known_declarations: usize,
    coverage_percent: f64,
    dynamic_boundaries: usize,
    unknown_boundaries: usize,
    source_kind: Option<String>,
}

#[derive(Debug, Serialize)]
struct MigrationImpactEntry {
    path: String,
    downstream_references: usize,
    untyped_declarations: usize,
    dynamic_boundaries: usize,
    unknown_boundaries: usize,
}

#[derive(Debug, Clone)]
struct MigrationFileStats {
    module_key: String,
    entry: MigrationCoverageEntry,
}

#[derive(Debug, Default, Clone, Copy)]
struct CoverageTally {
    declarations: usize,
    known_declarations: usize,
    dynamic_boundaries: usize,
    unknown_boundaries: usize,
}

fn main() -> ExitCode {
    if let Err(error) = init_tracing() {
        eprintln!("failed to initialize tracing: {error:#}");
        return ExitCode::from(2);
    }

    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error:#}");
            exit_code_for_error(&error)
        }
    }
}

fn exit_code_for_error(error: &anyhow::Error) -> ExitCode {
    if error.chain().any(|cause| cause.downcast_ref::<ConfigError>().is_some()) {
        return ExitCode::from(1);
    }

    if error
        .chain()
        .map(ToString::to_string)
        .any(|message| message.contains("already exists; rerun with --force"))
    {
        return ExitCode::from(1);
    }

    ExitCode::from(2)
}

fn init_tracing() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("typepython_cli=info,typepython_config=info")),
        )
        .with_target(false)
        .without_time()
        .try_init()
        .map_err(|error| anyhow::anyhow!("unable to install tracing subscriber: {error}"))
}

fn run() -> Result<ExitCode> {
    match Cli::parse().command {
        Command::Init(args) => init_project(args),
        Command::Check(args) => run_with_pipeline("check", args, false, Vec::new()),
        Command::Build(args) => run_build(args),
        Command::Watch(args) => run_watch(args),
        Command::Clean(args) => clean_project(args),
        Command::Lsp(args) => run_lsp(args),
        Command::Verify(args) => run_verify(args),
        Command::Migrate(args) => run_migrate(args),
    }
}

fn init_project(args: InitArgs) -> Result<ExitCode> {
    let root = if args.dir.is_absolute() {
        args.dir
    } else {
        env::current_dir().context("unable to determine current directory")?.join(args.dir)
    };

    let config_path = root.join("typepython.toml");
    let pyproject_path = root.join("pyproject.toml");
    let source_path = root.join("src/app/__init__.tpy");

    if args.embed_pyproject {
        write_embedded_pyproject_config(&pyproject_path, &config_path)?;
    } else {
        write_file(&config_path, CONFIG_TEMPLATE, args.force)?;
    }
    write_file(&source_path, INIT_SOURCE_TEMPLATE, args.force)?;

    println!("initialized TypePython project at {}", root.display());
    if args.embed_pyproject {
        println!("  config: {} ([tool.typepython])", pyproject_path.display());
    } else {
        println!("  config: {}", config_path.display());
    }
    println!("  source: {}", source_path.display());

    if pyproject_path.is_file() && !args.embed_pyproject {
        println!("  note: existing pyproject.toml detected; typepython.toml remains authoritative");
    }

    Ok(ExitCode::SUCCESS)
}

fn write_embedded_pyproject_config(pyproject_path: &Path, config_path: &Path) -> Result<()> {
    if !pyproject_path.is_file() {
        anyhow::bail!(
            "--embed-pyproject requires an existing pyproject.toml at {}",
            pyproject_path.display()
        );
    }
    if config_path.exists() {
        anyhow::bail!(
            "{} already exists; remove it before using --embed-pyproject",
            config_path.display()
        );
    }

    let existing = fs::read_to_string(pyproject_path)
        .with_context(|| format!("unable to read {}", pyproject_path.display()))?;
    if existing.contains("[tool.typepython]") || existing.contains("[tool.typepython.") {
        anyhow::bail!(
            "{} already defines [tool.typepython] configuration",
            pyproject_path.display()
        );
    }

    let mut rewritten = existing;
    if !rewritten.is_empty() && !rewritten.ends_with('\n') {
        rewritten.push('\n');
    }
    if !rewritten.trim().is_empty() {
        rewritten.push('\n');
    }
    rewritten.push_str(&embedded_config_template());

    fs::write(pyproject_path, rewritten)
        .with_context(|| format!("unable to write {}", pyproject_path.display()))
}

fn embedded_config_template() -> String {
    let mut rendered = String::new();
    for line in CONFIG_TEMPLATE.lines() {
        if line.starts_with('[') && line.ends_with(']') {
            rendered.push_str("[tool.typepython.");
            rendered.push_str(&line[1..line.len() - 1]);
            rendered.push(']');
        } else {
            rendered.push_str(line);
        }
        rendered.push('\n');
    }
    rendered
}

fn run_build(args: RunArgs) -> Result<ExitCode> {
    let config = load_project(args.project.as_ref())?;
    run_build_like_command(&config, args.format, "build", Vec::new())
}

fn run_watch(args: RunArgs) -> Result<ExitCode> {
    let config = load_project(args.project.as_ref())?;
    let watch_targets = watch_targets(&config);
    let mut last_exit = run_build_like_command(
        &config,
        args.format,
        "watch",
        vec![format!(
            "watching {} path(s) with {}ms debounce",
            watch_targets.len(),
            config.config.watch.debounce_ms
        )],
    )?;

    let (sender, receiver) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |result| {
            let _ = sender.send(result);
        },
        NotifyConfig::default(),
    )
    .context("unable to start filesystem watcher")?;

    for (path, mode) in &watch_targets {
        watcher
            .watch(path, *mode)
            .with_context(|| format!("unable to watch {}", path.display()))?;
    }

    let debounce = Duration::from_millis(config.config.watch.debounce_ms);
    loop {
        let mut changed_paths = BTreeSet::new();
        match receiver.recv() {
            Ok(Ok(event)) => collect_watch_event_paths(&mut changed_paths, event.paths),
            Ok(Err(error)) => {
                eprintln!("watch error: {error}");
                continue;
            }
            Err(_) => return Ok(last_exit),
        }

        loop {
            match receiver.recv_timeout(debounce) {
                Ok(Ok(event)) => collect_watch_event_paths(&mut changed_paths, event.paths),
                Ok(Err(error)) => eprintln!("watch error: {error}"),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Ok(last_exit),
            }
        }

        last_exit = run_build_like_command(
            &config,
            args.format,
            "watch",
            vec![format_watch_rebuild_note(&changed_paths)],
        )?;
    }
}

fn should_emit_build_outputs(config: &ConfigHandle, diagnostics: &DiagnosticReport) -> bool {
    !diagnostics.has_errors() || !config.config.emit.no_emit_on_error
}

fn build_diagnostics(config: &ConfigHandle, diagnostics: &DiagnosticReport) -> DiagnosticReport {
    let mut build_diagnostics = diagnostics.clone();

    if diagnostics.has_errors() && config.config.emit.no_emit_on_error {
        build_diagnostics.push(Diagnostic::error(
            "TPY5002",
            format!("emit blocked by `emit.no_emit_on_error` for {}", config.config_dir.display()),
        ));
    }

    build_diagnostics
}

fn clean_project(args: CleanArgs) -> Result<ExitCode> {
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

fn run_lsp(args: RunArgs) -> Result<ExitCode> {
    let config = load_project(args.project.as_ref())?;
    if args.format == OutputFormat::Json {
        return Err(anyhow::anyhow!(
            "`typepython lsp` speaks JSON-RPC over stdio and does not support `--format json`"
        ));
    }
    typepython_lsp::serve(&config)?;
    Ok(ExitCode::SUCCESS)
}

fn run_verify(args: VerifyArgs) -> Result<ExitCode> {
    let config = load_project(args.run.project.as_ref())?;
    let snapshot = run_pipeline(&config)?;
    let diagnostics = if snapshot.diagnostics.has_errors() {
        snapshot.diagnostics.clone()
    } else {
        let mut diagnostics = verify_build_artifacts(&config, &snapshot.emit_plan);
        if !diagnostics.has_errors() {
            diagnostics.diagnostics.extend(
                verify_runtime_public_name_parity(&config, &snapshot.emit_plan).diagnostics,
            );
        }
        if !diagnostics.has_errors() {
            diagnostics.diagnostics.extend(
                verify_packaged_artifacts(
                    &config,
                    &snapshot.emit_plan,
                    &supplied_verify_artifacts(&args),
                )
                .diagnostics,
            );
        }
        diagnostics
    };

    let supplied_artifact_count = args.wheels.len() + args.sdists.len();
    let mut notes = vec![String::from(
        "verifies current runtime artifacts, emitted stubs, and `py.typed` in the build tree",
    )];
    if supplied_artifact_count > 0 {
        notes.push(format!(
            "verified {} supplied wheel/sdist artifact(s) against the authoritative build tree",
            supplied_artifact_count
        ));
    }

    let summary = CommandSummary {
        command: String::from("verify"),
        config_path: config.config_path.display().to_string(),
        config_source: config.source,
        discovered_sources: snapshot.discovered_sources,
        lowered_modules: snapshot.lowered_modules.len(),
        planned_artifacts: snapshot.emit_plan.len(),
        tracked_modules: snapshot.tracked_modules,
        notes,
    };

    print_summary(args.run.format, &summary, &diagnostics)?;
    Ok(exit_code(&diagnostics))
}

#[derive(Debug, Clone)]
struct SuppliedVerifyArtifact {
    kind: SuppliedArtifactKind,
    path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
enum SuppliedArtifactKind {
    Wheel,
    Sdist,
}

impl SuppliedArtifactKind {
    fn label(self) -> &'static str {
        match self {
            Self::Wheel => "wheel",
            Self::Sdist => "sdist",
        }
    }
}

fn supplied_verify_artifacts(args: &VerifyArgs) -> Vec<SuppliedVerifyArtifact> {
    let mut artifacts = args
        .wheels
        .iter()
        .cloned()
        .map(|path| SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path })
        .collect::<Vec<_>>();
    artifacts.extend(
        args.sdists
            .iter()
            .cloned()
            .map(|path| SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path }),
    );
    artifacts
}

fn run_migrate(args: MigrateArgs) -> Result<ExitCode> {
    let config = load_project(args.run.project.as_ref())?;
    let discovery = collect_source_paths(&config)?;
    let mut syntax_trees =
        load_syntax_trees(&discovery.sources, config.config.typing.conditional_returns)?;
    let bundled_sources = bundled_stdlib_sources(&config.config.project.target_python)?;
    syntax_trees
        .extend(load_syntax_trees(&bundled_sources, config.config.typing.conditional_returns)?);
    let mut diagnostics = discovery.diagnostics.clone();
    let mut parse_diagnostics = collect_parse_diagnostics(&syntax_trees);
    apply_type_ignore_directives(&syntax_trees, &mut parse_diagnostics);
    diagnostics.diagnostics.extend(parse_diagnostics.diagnostics);

    let report = build_migration_report(&config, &syntax_trees);
    let emitted_stubs = emit_migration_stubs(
        &config,
        &discovery.sources,
        &args.emit_stubs,
        args.stub_out_dir.as_deref(),
    )?;
    let mut notes = Vec::new();
    if args.report {
        notes.push(String::from(
            "migration report includes file coverage, directory coverage, and high-impact untyped files",
        ));
    }
    if !emitted_stubs.is_empty() {
        let destination = args
            .stub_out_dir
            .as_ref()
            .map(|path| {
                if path.is_absolute() {
                    path.display().to_string()
                } else {
                    config.config_dir.join(path).display().to_string()
                }
            })
            .unwrap_or_else(|| String::from("source-adjacent `.pyi` files"));
        notes.push(format!(
            "generated {} inferred migration stub(s) under {}",
            emitted_stubs.len(),
            destination
        ));
    }

    let summary = CommandSummary {
        command: String::from("migrate"),
        config_path: config.config_path.display().to_string(),
        config_source: config.source,
        discovered_sources: discovery.sources.len(),
        lowered_modules: 0,
        planned_artifacts: 0,
        tracked_modules: 0,
        notes,
    };

    print_migration_report(args.run.format, &summary, &report, &diagnostics)?;
    Ok(exit_code(&diagnostics))
}

fn run_build_like_command(
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

fn watch_targets(config: &ConfigHandle) -> Vec<(PathBuf, RecursiveMode)> {
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

fn collect_watch_event_paths(changed_paths: &mut BTreeSet<PathBuf>, paths: Vec<PathBuf>) {
    changed_paths.extend(paths);
}

fn format_watch_rebuild_note(changed_paths: &BTreeSet<PathBuf>) -> String {
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

fn build_migration_report(
    config: &ConfigHandle,
    syntax_trees: &[typepython_syntax::SyntaxTree],
) -> MigrationReport {
    let mut files = syntax_trees
        .iter()
        .map(|syntax| migration_file_stats(config, syntax))
        .filter(|stats| stats.entry.declarations > 0)
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.entry.path.cmp(&right.entry.path));

    let mut directories = BTreeMap::<String, CoverageTally>::new();
    let mut total = CoverageTally::default();
    for stats in &files {
        total.declarations += stats.entry.declarations;
        total.known_declarations += stats.entry.known_declarations;
        total.dynamic_boundaries += stats.entry.dynamic_boundaries;
        total.unknown_boundaries += stats.entry.unknown_boundaries;

        let directory = Path::new(&stats.entry.path)
            .parent()
            .map(normalize_glob_path)
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| String::from("."));
        let tally = directories.entry(directory).or_default();
        tally.declarations += stats.entry.declarations;
        tally.known_declarations += stats.entry.known_declarations;
        tally.dynamic_boundaries += stats.entry.dynamic_boundaries;
        tally.unknown_boundaries += stats.entry.unknown_boundaries;
    }

    let mut directory_entries = directories
        .into_iter()
        .map(|(path, tally)| MigrationCoverageEntry {
            path,
            declarations: tally.declarations,
            known_declarations: tally.known_declarations,
            coverage_percent: coverage_percent(tally.known_declarations, tally.declarations),
            dynamic_boundaries: tally.dynamic_boundaries,
            unknown_boundaries: tally.unknown_boundaries,
            source_kind: None,
        })
        .collect::<Vec<_>>();
    directory_entries.sort_by(|left, right| left.path.cmp(&right.path));

    let mut downstream_reference_counts = BTreeMap::<String, usize>::new();
    for syntax in syntax_trees {
        for statement in &syntax.statements {
            let typepython_syntax::SyntaxStatement::Import(statement) = statement else {
                continue;
            };
            for binding in &statement.bindings {
                let target = files
                    .iter()
                    .filter(|stats| syntax.source.logical_module != stats.module_key)
                    .filter(|stats| {
                        binding.source_path == stats.module_key
                            || binding.source_path.starts_with(&format!("{}.", stats.module_key))
                    })
                    .max_by_key(|stats| stats.module_key.len());
                if let Some(stats) = target {
                    *downstream_reference_counts.entry(stats.entry.path.clone()).or_default() += 1;
                }
            }
        }
    }
    let mut high_impact_untyped_files = files
        .iter()
        .filter(|stats| stats.entry.known_declarations < stats.entry.declarations)
        .map(|stats| MigrationImpactEntry {
            path: stats.entry.path.clone(),
            downstream_references: downstream_reference_counts
                .get(&stats.entry.path)
                .copied()
                .unwrap_or(0),
            untyped_declarations: stats.entry.declarations - stats.entry.known_declarations,
            dynamic_boundaries: stats.entry.dynamic_boundaries,
            unknown_boundaries: stats.entry.unknown_boundaries,
        })
        .collect::<Vec<_>>();
    high_impact_untyped_files.sort_by(|left, right| {
        right
            .downstream_references
            .cmp(&left.downstream_references)
            .then_with(|| right.untyped_declarations.cmp(&left.untyped_declarations))
            .then_with(|| left.path.cmp(&right.path))
    });

    MigrationReport {
        total_declarations: total.declarations,
        known_declarations: total.known_declarations,
        total_dynamic_boundaries: total.dynamic_boundaries,
        total_unknown_boundaries: total.unknown_boundaries,
        files: files.into_iter().map(|stats| stats.entry).collect(),
        directories: directory_entries,
        high_impact_untyped_files,
    }
}

fn migration_file_stats(
    config: &ConfigHandle,
    syntax: &typepython_syntax::SyntaxTree,
) -> MigrationFileStats {
    let mut tally = CoverageTally::default();
    for statement in &syntax.statements {
        accumulate_statement_coverage(statement, &mut tally);
    }

    MigrationFileStats {
        module_key: syntax.source.logical_module.clone(),
        entry: MigrationCoverageEntry {
            path: syntax
                .source
                .path
                .strip_prefix(&config.config_dir)
                .map(normalize_glob_path)
                .unwrap_or_else(|_| syntax.source.path.display().to_string()),
            declarations: tally.declarations,
            known_declarations: tally.known_declarations,
            coverage_percent: coverage_percent(tally.known_declarations, tally.declarations),
            dynamic_boundaries: tally.dynamic_boundaries,
            unknown_boundaries: tally.unknown_boundaries,
            source_kind: Some(source_kind_label(syntax.source.kind).to_owned()),
        },
    }
}

fn accumulate_statement_coverage(
    statement: &typepython_syntax::SyntaxStatement,
    tally: &mut CoverageTally,
) {
    match statement {
        typepython_syntax::SyntaxStatement::TypeAlias(statement) => {
            tally.declarations += 1;
            let (dynamic_count, unknown_count) = count_boundary_tokens(&statement.value);
            tally.dynamic_boundaries += dynamic_count;
            tally.unknown_boundaries += unknown_count;
            if !statement.value.is_empty() && dynamic_count == 0 && unknown_count == 0 {
                tally.known_declarations += 1;
            }
        }
        typepython_syntax::SyntaxStatement::Interface(statement)
        | typepython_syntax::SyntaxStatement::DataClass(statement)
        | typepython_syntax::SyntaxStatement::SealedClass(statement)
        | typepython_syntax::SyntaxStatement::ClassDef(statement) => {
            tally.declarations += 1;
            let mut class_known = true;
            for base in &statement.bases {
                let (dynamic_count, unknown_count) = count_boundary_tokens(base);
                tally.dynamic_boundaries += dynamic_count;
                tally.unknown_boundaries += unknown_count;
                if dynamic_count > 0 || unknown_count > 0 {
                    class_known = false;
                }
            }
            if class_known {
                tally.known_declarations += 1;
            }
            for member in &statement.members {
                tally.declarations += 1;
                let (member_known, dynamic_count, unknown_count) = class_member_coverage(member);
                tally.dynamic_boundaries += dynamic_count;
                tally.unknown_boundaries += unknown_count;
                if member_known {
                    tally.known_declarations += 1;
                }
            }
        }
        typepython_syntax::SyntaxStatement::OverloadDef(statement) => {
            tally.declarations += 1;
            let (known, dynamic_count, unknown_count) =
                function_signature_coverage(&statement.params, statement.returns.as_deref(), false);
            tally.dynamic_boundaries += dynamic_count;
            tally.unknown_boundaries += unknown_count;
            if known {
                tally.known_declarations += 1;
            }
        }
        typepython_syntax::SyntaxStatement::FunctionDef(statement) => {
            tally.declarations += 1;
            let (known, dynamic_count, unknown_count) =
                function_signature_coverage(&statement.params, statement.returns.as_deref(), false);
            tally.dynamic_boundaries += dynamic_count;
            tally.unknown_boundaries += unknown_count;
            if known {
                tally.known_declarations += 1;
            }
        }
        typepython_syntax::SyntaxStatement::Value(statement) => {
            let (annotation_known, dynamic_count, unknown_count) = known_type_slot(
                statement.annotation.as_deref().or(statement.value_type.as_deref()),
            );
            tally.dynamic_boundaries += dynamic_count;
            tally.unknown_boundaries += unknown_count;
            for _ in &statement.names {
                tally.declarations += 1;
                if annotation_known {
                    tally.known_declarations += 1;
                }
            }
        }
        typepython_syntax::SyntaxStatement::If(_) => {}
        typepython_syntax::SyntaxStatement::Assert(_) => {}
        typepython_syntax::SyntaxStatement::Invalidate(_) => {}
        typepython_syntax::SyntaxStatement::Match(_) => {}
        typepython_syntax::SyntaxStatement::Import(_)
        | typepython_syntax::SyntaxStatement::Call(_)
        | typepython_syntax::SyntaxStatement::MethodCall(_)
        | typepython_syntax::SyntaxStatement::MemberAccess(_)
        | typepython_syntax::SyntaxStatement::Return(_)
        | typepython_syntax::SyntaxStatement::Yield(_)
        | typepython_syntax::SyntaxStatement::For(_)
        | typepython_syntax::SyntaxStatement::With(_)
        | typepython_syntax::SyntaxStatement::ExceptHandler(_)
        | typepython_syntax::SyntaxStatement::Unsafe(_) => {}
    }
}

fn class_member_coverage(member: &typepython_syntax::ClassMember) -> (bool, usize, usize) {
    match member.kind {
        typepython_syntax::ClassMemberKind::Field => {
            known_type_slot(member.annotation.as_deref().or(member.value_type.as_deref()))
        }
        typepython_syntax::ClassMemberKind::Method
        | typepython_syntax::ClassMemberKind::Overload => function_signature_coverage(
            &member.params,
            member.returns.as_deref(),
            !matches!(member.method_kind, Some(typepython_syntax::MethodKind::Static)),
        ),
    }
}

fn function_signature_coverage(
    params: &[typepython_syntax::FunctionParam],
    returns: Option<&str>,
    allow_implicit_receiver: bool,
) -> (bool, usize, usize) {
    let mut known = true;
    let mut dynamic_boundaries = 0usize;
    let mut unknown_boundaries = 0usize;

    for (index, param) in params.iter().enumerate() {
        let is_implicit_receiver = allow_implicit_receiver
            && index == 0
            && param.annotation.is_none()
            && matches!(param.name.as_str(), "self" | "cls");
        if is_implicit_receiver {
            continue;
        }

        let (param_known, dynamic_count, unknown_count) =
            known_type_slot(param.annotation.as_deref());
        dynamic_boundaries += dynamic_count;
        unknown_boundaries += unknown_count;
        if !param_known {
            known = false;
        }
    }

    let (return_known, dynamic_count, unknown_count) = known_type_slot(returns);
    dynamic_boundaries += dynamic_count;
    unknown_boundaries += unknown_count;
    if !return_known {
        known = false;
    }

    (known, dynamic_boundaries, unknown_boundaries)
}

fn known_type_slot(text: Option<&str>) -> (bool, usize, usize) {
    let Some(text) = text else {
        return (false, 0, 0);
    };
    let (dynamic_count, unknown_count) = count_boundary_tokens(text);
    (!text.is_empty() && dynamic_count == 0 && unknown_count == 0, dynamic_count, unknown_count)
}

fn count_boundary_tokens(text: &str) -> (usize, usize) {
    let mut dynamic_count = 0usize;
    let mut unknown_count = 0usize;
    let mut token = String::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
            continue;
        }

        match token.as_str() {
            "dynamic" => dynamic_count += 1,
            "unknown" => unknown_count += 1,
            _ => {}
        }
        token.clear();
    }

    match token.as_str() {
        "dynamic" => dynamic_count += 1,
        "unknown" => unknown_count += 1,
        _ => {}
    }

    (dynamic_count, unknown_count)
}

fn coverage_percent(known: usize, total: usize) -> f64 {
    if total == 0 { 100.0 } else { ((known as f64 / total as f64) * 1000.0).round() / 10.0 }
}

fn source_kind_label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::TypePython => "tpy",
        SourceKind::Python => "py",
        SourceKind::Stub => "pyi",
    }
}

#[derive(Debug, Clone)]
struct ShadowStub {
    logical_module: String,
    stub_path: PathBuf,
}

fn load_syntax_trees(
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

fn emit_migration_stubs(
    config: &ConfigHandle,
    discovered_sources: &[DiscoveredSource],
    requested_paths: &[PathBuf],
    stub_out_dir: Option<&Path>,
) -> Result<Vec<PathBuf>> {
    let targets = select_migration_stub_sources(config, discovered_sources, requested_paths)?;
    let output_root = stub_out_dir.map(|path| {
        if path.is_absolute() { path.to_path_buf() } else { config.config_dir.join(path) }
    });

    let mut written = Vec::new();
    for source in targets {
        let source_file = SourceFile::from_path(&source.path)
            .with_context(|| format!("unable to read {}", source.path.display()))?;
        let stub_source =
            generate_inferred_stub_source(&source_file.text, InferredStubMode::Migration)
                .with_context(|| {
                    format!("unable to generate migration stub for {}", source.path.display())
                })?;
        let stub_path = migration_stub_output_path(source, output_root.as_deref())?;
        if let Some(parent) = stub_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("unable to create {}", parent.display()))?;
        }
        fs::write(&stub_path, stub_source)
            .with_context(|| format!("unable to write {}", stub_path.display()))?;
        written.push(stub_path);
    }

    Ok(written)
}

fn select_migration_stub_sources<'a>(
    config: &ConfigHandle,
    discovered_sources: &'a [DiscoveredSource],
    requested_paths: &[PathBuf],
) -> Result<Vec<&'a DiscoveredSource>> {
    if requested_paths.is_empty() {
        return Ok(Vec::new());
    }

    let python_sources = discovered_sources
        .iter()
        .filter(|source| source.kind == SourceKind::Python)
        .collect::<Vec<_>>();
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();

    for requested in requested_paths {
        let resolved = if requested.is_absolute() {
            requested.clone()
        } else {
            config.config_dir.join(requested)
        };
        let matches = if resolved.is_dir() {
            python_sources
                .iter()
                .copied()
                .filter(|source| source.path.starts_with(&resolved))
                .collect::<Vec<_>>()
        } else {
            python_sources
                .iter()
                .copied()
                .filter(|source| source.path == resolved)
                .collect::<Vec<_>>()
        };

        if matches.is_empty() {
            anyhow::bail!(
                "unable to find project `.py` source matching `{}` for `typepython migrate --emit-stubs`",
                resolved.display()
            );
        }

        for source in matches {
            if seen.insert(source.path.clone()) {
                selected.push(source);
            }
        }
    }

    selected.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(selected)
}

fn migration_stub_output_path(
    source: &DiscoveredSource,
    output_root: Option<&Path>,
) -> Result<PathBuf> {
    match output_root {
        Some(root) => {
            let relative = source.path.strip_prefix(&source.root).with_context(|| {
                format!(
                    "unable to compute relative stub path for {} from source root {}",
                    source.path.display(),
                    source.root.display()
                )
            })?;
            Ok(root.join(relative).with_extension("pyi"))
        }
        None => Ok(source.path.with_extension("pyi")),
    }
}

fn run_with_pipeline(
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

fn run_pipeline(config: &ConfigHandle) -> Result<PipelineSnapshot> {
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
    let mut diagnostics = check_with_options(
        &graph,
        config.config.typing.require_explicit_overrides,
        config.config.typing.enable_sealed_exhaustiveness,
        config.config.typing.report_deprecated,
        config.config.typing.strict,
        config.config.typing.warn_unsafe,
        config.config.typing.imports,
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

    let mut incremental = snapshot(&graph);
    incremental.stdlib_snapshot =
        Some(bundled_stdlib_snapshot_identity(&config.config.project.target_python)?);
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

fn collect_parse_diagnostics(syntax_trees: &[typepython_syntax::SyntaxTree]) -> DiagnosticReport {
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

fn verify_build_artifacts(config: &ConfigHandle, artifacts: &[EmitArtifact]) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    let mut package_roots = BTreeSet::new();

    for artifact in artifacts {
        if let Some(runtime_path) = &artifact.runtime_path {
            if !runtime_path.exists() {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!("missing runtime artifact `{}`", runtime_path.display()),
                ));
            } else if let Some(diagnostic) = verify_emitted_text_artifact(runtime_path) {
                diagnostics.push(diagnostic);
            }
            if config.config.emit.emit_pyc {
                let bytecode_path = match bytecode_path_for(runtime_path) {
                    Ok(path) => path,
                    Err(error) => {
                        diagnostics.push(Diagnostic::error(
                            "TPY5003",
                            format!(
                                "unable to determine bytecode path for `{}`: {error}",
                                runtime_path.display()
                            ),
                        ));
                        continue;
                    }
                };
                if !bytecode_path.exists() {
                    diagnostics.push(Diagnostic::error(
                        "TPY5003",
                        format!("missing bytecode artifact `{}`", bytecode_path.display()),
                    ));
                }
            }
            if is_package_init_path(runtime_path) {
                if let Some(parent) = runtime_path.parent() {
                    package_roots.insert(parent.to_path_buf());
                }
            }
        }

        if let Some(stub_path) = &artifact.stub_path {
            if !stub_path.exists() {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!("missing stub artifact `{}`", stub_path.display()),
                ));
            } else if let Some(diagnostic) = verify_emitted_text_artifact(stub_path) {
                diagnostics.push(diagnostic);
            }
            if is_package_init_path(stub_path) {
                if let Some(parent) = stub_path.parent() {
                    package_roots.insert(parent.to_path_buf());
                }
            }
        }

        if let (Some(runtime_path), Some(stub_path)) = (&artifact.runtime_path, &artifact.stub_path)
        {
            if runtime_path.exists() && stub_path.exists() {
                if let Some(diagnostic) =
                    verify_emitted_declaration_surface(runtime_path, stub_path)
                {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    if config.config.emit.write_py_typed {
        for package_root in package_roots {
            let marker_path = package_root.join("py.typed");
            if !marker_path.exists() {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!("missing package marker `{}`", marker_path.display()),
                ));
            }
        }
    }

    let snapshot_path =
        config.resolve_relative_path(&config.config.project.cache_dir).join("snapshot.json");
    if !snapshot_path.exists() {
        diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!("missing incremental snapshot `{}`", snapshot_path.display()),
        ));
    } else if let Err(error) = verify_incremental_snapshot(&snapshot_path) {
        diagnostics.push(Diagnostic::error(
            "TPY6001",
            format!(
                "incremental snapshot `{}` is incompatible or corrupt: {}",
                snapshot_path.display(),
                error
            ),
        ));
    }

    diagnostics
}

fn verify_packaged_artifacts(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
    supplied_artifacts: &[SuppliedVerifyArtifact],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    if supplied_artifacts.is_empty() {
        return diagnostics;
    }

    let expected_files = match expected_published_files(config, artifacts) {
        Ok(files) => files,
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!("unable to collect authoritative build artifacts for publication verification: {error}"),
            ));
            return diagnostics;
        }
    };

    for artifact in supplied_artifacts {
        match read_supplied_artifact_entries(artifact) {
            Ok(entries) => {
                for (relative_path, expected_bytes) in &expected_files {
                    match entries.get(relative_path) {
                        None => diagnostics.push(Diagnostic::error(
                            "TPY5003",
                            format!(
                                "{} artifact `{}` is missing published file `{relative_path}`",
                                artifact.kind.label(),
                                artifact.path.display(),
                            ),
                        )),
                        Some(actual_bytes) if actual_bytes != expected_bytes => {
                            diagnostics.push(Diagnostic::error(
                                "TPY5003",
                                format!(
                                    "{} artifact `{}` contains `{relative_path}` that diverges from the authoritative build output",
                                    artifact.kind.label(),
                                    artifact.path.display(),
                                ),
                            ));
                        }
                        Some(_) => {}
                    }
                }
                for relative_path in
                    entries.keys().filter(|path| is_authoritative_publication_file(path))
                {
                    if !expected_files.contains_key(relative_path) {
                        diagnostics.push(Diagnostic::error(
                            "TPY5003",
                            format!(
                                "{} artifact `{}` contains unexpected published file `{relative_path}`",
                                artifact.kind.label(),
                                artifact.path.display(),
                            ),
                        ));
                    }
                }
            }
            Err(error) => diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "unable to inspect {} artifact `{}`: {error}",
                    artifact.kind.label(),
                    artifact.path.display(),
                ),
            )),
        }
    }

    diagnostics
}

fn verify_runtime_public_name_parity(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    let out_root = config.resolve_relative_path(&config.config.project.out_dir);

    for artifact in artifacts {
        let (Some(runtime_path), Some(stub_path)) = (&artifact.runtime_path, &artifact.stub_path)
        else {
            continue;
        };
        if !(runtime_path.exists() && stub_path.exists()) {
            continue;
        }
        let Some(module_name) = logical_module_name_from_runtime_path(&out_root, runtime_path)
        else {
            continue;
        };
        let Some(runtime_names) = runtime_public_names(config, &out_root, &module_name) else {
            continue;
        };
        let authoritative_names = match authoritative_public_names(config, stub_path) {
            Ok(names) => names,
            Err(error) => {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "unable to determine authoritative public names for `{}` from `{}`: {error}",
                        module_name,
                        stub_path.display(),
                    ),
                ));
                continue;
            }
        };

        let missing_from_runtime =
            authoritative_names.difference(&runtime_names).cloned().collect::<Vec<_>>();
        let missing_from_type_surface =
            runtime_names.difference(&authoritative_names).cloned().collect::<Vec<_>>();

        if !missing_from_runtime.is_empty() {
            diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "runtime module `{}` is missing public names declared by the authoritative type surface: {}",
                    module_name,
                    missing_from_runtime.join(", "),
                ),
            ));
        }
        if !missing_from_type_surface.is_empty() {
            diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "authoritative type surface for `{}` is missing runtime public names: {}",
                    module_name,
                    missing_from_type_surface.join(", "),
                ),
            ));
        }
    }

    diagnostics
}

fn logical_module_name_from_runtime_path(out_root: &Path, runtime_path: &Path) -> Option<String> {
    let relative = runtime_path.strip_prefix(out_root).ok()?;
    let stem = runtime_path.file_stem()?.to_str()?;
    let mut components = relative
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .components()
        .map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;

    if stem == "__init__" {
        return (!components.is_empty()).then(|| components.join("."));
    }

    components.push(stem.to_owned());
    Some(components.join("."))
}

#[derive(Debug, Deserialize)]
struct RuntimePublicNameResult {
    importable: bool,
    names: Option<Vec<String>>,
}

fn runtime_public_names(
    config: &ConfigHandle,
    out_root: &Path,
    module_name: &str,
) -> Option<BTreeSet<String>> {
    let interpreter = resolve_python_executable(config);
    let output = ProcessCommand::new(&interpreter)
        .args(["-c", RUNTIME_PUBLIC_NAMES_SCRIPT])
        .arg(out_root)
        .arg(module_name)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let result = serde_json::from_slice::<RuntimePublicNameResult>(&output.stdout).ok()?;
    result.importable.then(|| result.names.unwrap_or_default().into_iter().collect::<BTreeSet<_>>())
}

fn authoritative_public_names(
    config: &ConfigHandle,
    path: &Path,
) -> std::result::Result<BTreeSet<String>, String> {
    if let Some(names) = static_all_names(config, path)? {
        return Ok(names);
    }

    let syntax = emitted_syntax(path)
        .ok_or_else(|| format!("`{}` could not be parsed as a Python module", path.display()))?;
    Ok(module_level_surface_names(&syntax)
        .into_iter()
        .filter(|name| !name.starts_with('_'))
        .collect())
}

fn static_all_names(
    config: &ConfigHandle,
    path: &Path,
) -> std::result::Result<Option<BTreeSet<String>>, String> {
    let interpreter = resolve_python_executable(config);
    let output = ProcessCommand::new(&interpreter)
        .args(["-c", STATIC_ALL_NAMES_SCRIPT])
        .arg(path)
        .output()
        .map_err(|error| {
            format!("unable to run Python parser `{}`: {error}", interpreter.display())
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr_suffix =
            if stderr.trim().is_empty() { String::new() } else { format!(": {}", stderr.trim()) };
        return Err(format!("Python parser exited with status {}{}", output.status, stderr_suffix));
    }
    let names = serde_json::from_slice::<Option<Vec<String>>>(&output.stdout)
        .map_err(|error| format!("unable to parse `__all__` output: {error}"))?;
    Ok(names.map(|names| names.into_iter().collect()))
}

fn expected_published_files(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
) -> Result<BTreeMap<String, Vec<u8>>> {
    let out_root = config.resolve_relative_path(&config.config.project.out_dir);
    let mut expected_files = BTreeMap::new();
    let mut package_roots = BTreeSet::new();

    for artifact in artifacts {
        if let Some(runtime_path) = &artifact.runtime_path {
            expected_files
                .insert(relative_publish_path(&out_root, runtime_path)?, fs::read(runtime_path)?);
            if is_package_init_path(runtime_path) {
                if let Some(parent) = runtime_path.parent() {
                    package_roots.insert(parent.to_path_buf());
                }
            }
        }
        if let Some(stub_path) = &artifact.stub_path {
            expected_files
                .insert(relative_publish_path(&out_root, stub_path)?, fs::read(stub_path)?);
            if is_package_init_path(stub_path) {
                if let Some(parent) = stub_path.parent() {
                    package_roots.insert(parent.to_path_buf());
                }
            }
        }
    }

    if config.config.emit.write_py_typed {
        for package_root in package_roots {
            let marker_path = package_root.join("py.typed");
            expected_files
                .insert(relative_publish_path(&out_root, &marker_path)?, fs::read(marker_path)?);
        }
    }

    Ok(expected_files)
}

fn relative_publish_path(out_root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(out_root)
        .with_context(|| format!("{} is not inside {}", path.display(), out_root.display()))?;
    Ok(normalize_glob_path(relative))
}

fn is_authoritative_publication_file(path: &str) -> bool {
    path.ends_with(".py") || path.ends_with(".pyi")
}

fn is_package_init_path(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "__init__.py" || name == "__init__.pyi")
}

fn read_supplied_artifact_entries(
    artifact: &SuppliedVerifyArtifact,
) -> std::result::Result<BTreeMap<String, Vec<u8>>, String> {
    let path_text = artifact.path.to_string_lossy().to_ascii_lowercase();
    match artifact.kind {
        SuppliedArtifactKind::Wheel => {
            if !(path_text.ends_with(".whl") || path_text.ends_with(".zip")) {
                return Err(String::from("expected a .whl or .zip file"));
            }
            Ok(read_zip_entries(&artifact.path)?.into_iter().collect())
        }
        SuppliedArtifactKind::Sdist => {
            let entries = if path_text.ends_with(".tar.gz") || path_text.ends_with(".tgz") {
                read_tar_gz_entries(&artifact.path)?
            } else if path_text.ends_with(".zip") {
                read_zip_entries(&artifact.path)?
            } else {
                return Err(String::from("expected a .tar.gz, .tgz, or .zip file"));
            };
            Ok(strip_common_archive_root(entries))
        }
    }
}

fn read_zip_entries(path: &Path) -> std::result::Result<Vec<(String, Vec<u8>)>, String> {
    let file = fs::File::open(path).map_err(|error| format!("unable to open archive: {error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("unable to read zip archive: {error}"))?;
    let mut entries = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| format!("unable to read zip entry {index}: {error}"))?;
        if file.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| format!("unable to read zip entry `{}`: {error}", file.name()))?;
        entries.push((normalize_archive_path(file.name()), bytes));
    }

    Ok(entries)
}

fn read_tar_gz_entries(path: &Path) -> std::result::Result<Vec<(String, Vec<u8>)>, String> {
    let file = fs::File::open(path).map_err(|error| format!("unable to open archive: {error}"))?;
    let decoder = GzDecoder::new(file);
    let mut archive = TarArchive::new(decoder);
    let mut entries = Vec::new();

    for entry in
        archive.entries().map_err(|error| format!("unable to read tar archive: {error}"))?
    {
        let mut entry = entry.map_err(|error| format!("unable to read tar entry: {error}"))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let entry_path = entry
            .path()
            .map_err(|error| format!("unable to read tar entry path: {error}"))?
            .display()
            .to_string();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| format!("unable to read tar entry `{entry_path}`: {error}"))?;
        entries.push((normalize_archive_path(&entry_path), bytes));
    }

    Ok(entries)
}

fn strip_common_archive_root(entries: Vec<(String, Vec<u8>)>) -> BTreeMap<String, Vec<u8>> {
    let Some(common_root) = common_archive_root(&entries) else {
        return entries.into_iter().collect();
    };

    entries
        .into_iter()
        .map(|(path, bytes)| {
            let normalized =
                path.strip_prefix(&format!("{common_root}/")).map(str::to_owned).unwrap_or(path);
            (normalized, bytes)
        })
        .collect()
}

fn common_archive_root(entries: &[(String, Vec<u8>)]) -> Option<String> {
    let mut root: Option<&str> = None;

    for (path, _) in entries {
        let mut components = path.split('/').filter(|component| !component.is_empty());
        let first = components.next()?;
        components.next()?;
        match root {
            Some(existing) if existing != first => return None,
            Some(_) => {}
            None => root = Some(first),
        }
    }

    root.map(str::to_owned)
}

fn normalize_archive_path(path: &str) -> String {
    path.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn verify_incremental_snapshot(path: &Path) -> Result<(), String> {
    let rendered = fs::read_to_string(path)
        .map_err(|error| format!("unable to read incremental snapshot: {error}"))?;
    decode_snapshot(&rendered).map(|_| ()).map_err(|error| error.to_string())
}

fn verify_emitted_text_artifact(path: &Path) -> Option<Diagnostic> {
    let source = match SourceFile::from_path(path) {
        Ok(source) => source,
        Err(error) => {
            return Some(Diagnostic::error(
                "TPY5003",
                format!("unable to read emitted artifact `{}`: {error}", path.display()),
            ));
        }
    };
    let syntax = typepython_syntax::parse(source);
    if syntax.diagnostics.has_errors() {
        Some(Diagnostic::error(
            "TPY5003",
            format!("emitted artifact `{}` is not valid Python syntax", path.display()),
        ))
    } else if path.extension().is_some_and(|extension| extension == "pyi") {
        verify_stub_syntax_rules(path, &syntax)
    } else {
        None
    }
}

fn verify_emitted_declaration_surface(runtime_path: &Path, stub_path: &Path) -> Option<Diagnostic> {
    let runtime_syntax = emitted_syntax(runtime_path)?;
    let stub_syntax = emitted_syntax(stub_path)?;

    if module_level_surface_names(&runtime_syntax) == module_level_surface_names(&stub_syntax) {
        None
    } else {
        Some(Diagnostic::error(
            "TPY5003",
            format!(
                "emitted runtime/stub public names differ between `{}` and `{}`",
                runtime_path.display(),
                stub_path.display()
            ),
        ))
    }
}

fn verify_stub_syntax_rules(
    path: &Path,
    syntax: &typepython_syntax::SyntaxTree,
) -> Option<Diagnostic> {
    if syntax.statements.iter().any(stub_statement_is_runtime) {
        return Some(Diagnostic::error(
            "TPY5003",
            format!("emitted stub artifact `{}` contains runtime statements", path.display()),
        ));
    }

    for statement in &syntax.statements {
        match statement {
            typepython_syntax::SyntaxStatement::Value(statement)
                if statement.owner_name.is_some() =>
            {
                return Some(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` contains executable assignments",
                        path.display()
                    ),
                ));
            }
            typepython_syntax::SyntaxStatement::Value(statement)
                if statement.owner_name.is_none() && statement.annotation.is_none() =>
            {
                return Some(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` contains value declarations without annotations",
                        path.display()
                    ),
                ));
            }
            typepython_syntax::SyntaxStatement::Interface(_)
            | typepython_syntax::SyntaxStatement::DataClass(_)
            | typepython_syntax::SyntaxStatement::SealedClass(_)
            | typepython_syntax::SyntaxStatement::Unsafe(_) => {
                return Some(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` contains TypePython-only syntax",
                        path.display()
                    ),
                ));
            }
            typepython_syntax::SyntaxStatement::ClassDef(statement) => {
                if statement.members.iter().any(|member| {
                    member.kind == typepython_syntax::ClassMemberKind::Field
                        && member.annotation.is_none()
                }) {
                    return Some(Diagnostic::error(
                        "TPY5003",
                        format!(
                            "emitted stub artifact `{}` contains class fields without annotations",
                            path.display()
                        ),
                    ));
                }
            }
            _ => {}
        }
    }

    None
}

fn stub_statement_is_runtime(statement: &typepython_syntax::SyntaxStatement) -> bool {
    matches!(
        statement,
        typepython_syntax::SyntaxStatement::Call(_)
            | typepython_syntax::SyntaxStatement::MemberAccess(_)
            | typepython_syntax::SyntaxStatement::MethodCall(_)
            | typepython_syntax::SyntaxStatement::Return(_)
            | typepython_syntax::SyntaxStatement::Yield(_)
            | typepython_syntax::SyntaxStatement::If(_)
            | typepython_syntax::SyntaxStatement::Assert(_)
            | typepython_syntax::SyntaxStatement::Invalidate(_)
            | typepython_syntax::SyntaxStatement::Match(_)
            | typepython_syntax::SyntaxStatement::For(_)
            | typepython_syntax::SyntaxStatement::With(_)
            | typepython_syntax::SyntaxStatement::ExceptHandler(_)
            | typepython_syntax::SyntaxStatement::Unsafe(_)
    )
}

fn emitted_syntax(path: &Path) -> Option<typepython_syntax::SyntaxTree> {
    let source = SourceFile::from_path(path).ok()?;
    let syntax = typepython_syntax::parse(source);
    if syntax.diagnostics.has_errors() { None } else { Some(syntax) }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct SurfaceEntry {
    owner: Option<String>,
    kind: &'static str,
    name: String,
    detail: String,
}

fn declaration_surface(
    syntax: &typepython_syntax::SyntaxTree,
) -> std::collections::BTreeSet<SurfaceEntry> {
    let mut surface = std::collections::BTreeSet::new();

    for statement in &syntax.statements {
        match statement {
            typepython_syntax::SyntaxStatement::TypeAlias(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "typealias",
                    name: statement.name.clone(),
                    detail: statement.value.clone(),
                });
            }
            typepython_syntax::SyntaxStatement::Interface(statement)
            | typepython_syntax::SyntaxStatement::DataClass(statement)
            | typepython_syntax::SyntaxStatement::SealedClass(statement)
            | typepython_syntax::SyntaxStatement::ClassDef(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "class",
                    name: statement.name.clone(),
                    detail: format!(
                        "bases=[{}];final={}",
                        statement.bases.join(","),
                        statement.is_final_decorator
                    ),
                });
                for member in &statement.members {
                    surface.insert(SurfaceEntry {
                        owner: Some(statement.name.clone()),
                        kind: match member.kind {
                            typepython_syntax::ClassMemberKind::Field => "field",
                            typepython_syntax::ClassMemberKind::Method => "method",
                            typepython_syntax::ClassMemberKind::Overload => "overload",
                        },
                        name: member.name.clone(),
                        detail: match member.kind {
                            typepython_syntax::ClassMemberKind::Field => format!(
                                "annotation={};final={};classvar={}",
                                member.annotation.clone().unwrap_or_default(),
                                member.is_final_decorator,
                                member.is_class_var
                            ),
                            typepython_syntax::ClassMemberKind::Method
                            | typepython_syntax::ClassMemberKind::Overload => format!(
                                "kind={:?};final={};sig={}",
                                member
                                    .method_kind
                                    .unwrap_or(typepython_syntax::MethodKind::Instance),
                                member.is_final_decorator,
                                format_signature(&member.params, member.returns.as_deref())
                            ),
                        },
                    });
                }
            }
            typepython_syntax::SyntaxStatement::OverloadDef(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "overload",
                    name: statement.name.clone(),
                    detail: format_signature(&statement.params, statement.returns.as_deref()),
                });
            }
            typepython_syntax::SyntaxStatement::FunctionDef(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "function",
                    name: statement.name.clone(),
                    detail: format_signature(&statement.params, statement.returns.as_deref()),
                });
            }
            typepython_syntax::SyntaxStatement::Import(statement) => {
                for binding in &statement.bindings {
                    surface.insert(SurfaceEntry {
                        owner: None,
                        kind: "import",
                        name: binding.local_name.clone(),
                        detail: binding.source_path.clone(),
                    });
                }
            }
            typepython_syntax::SyntaxStatement::Value(statement) => {
                for name in &statement.names {
                    surface.insert(SurfaceEntry {
                        owner: None,
                        kind: "value",
                        name: name.clone(),
                        detail: statement.annotation.clone().unwrap_or_default(),
                    });
                }
            }
            typepython_syntax::SyntaxStatement::Call(_) => {}
            typepython_syntax::SyntaxStatement::MethodCall(_) => {}
            typepython_syntax::SyntaxStatement::MemberAccess(_) => {}
            typepython_syntax::SyntaxStatement::Return(_) => {}
            typepython_syntax::SyntaxStatement::Yield(_) => {}
            typepython_syntax::SyntaxStatement::If(_) => {}
            typepython_syntax::SyntaxStatement::Assert(_) => {}
            typepython_syntax::SyntaxStatement::Invalidate(_) => {}
            typepython_syntax::SyntaxStatement::Match(_) => {}
            typepython_syntax::SyntaxStatement::For(_) => {}
            typepython_syntax::SyntaxStatement::With(_) => {}
            typepython_syntax::SyntaxStatement::ExceptHandler(_) => {}
            typepython_syntax::SyntaxStatement::Unsafe(_) => {}
        }
    }

    surface
}

fn module_level_surface_names(syntax: &typepython_syntax::SyntaxTree) -> BTreeSet<String> {
    declaration_surface(syntax)
        .into_iter()
        .filter(|entry| entry.owner.is_none())
        .map(|entry| entry.name)
        .collect()
}

fn public_surface_completeness_diagnostics(
    config: &ConfigHandle,
    syntax_trees: &[typepython_syntax::SyntaxTree],
    lowered_modules: &[LoweredModule],
    stub_contexts: &BTreeMap<PathBuf, TypePythonStubContext>,
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();

    if !config.config.typing.require_known_public_types {
        return diagnostics;
    }

    let lowered_by_source = lowered_modules
        .iter()
        .map(|module| (module.source_path.clone(), module))
        .collect::<BTreeMap<_, _>>();

    for syntax in syntax_trees {
        let surface_syntax = if syntax.source.kind == SourceKind::TypePython {
            let Some(module) = lowered_by_source.get(&syntax.source.path) else {
                continue;
            };
            let context = stub_contexts.get(&syntax.source.path).cloned().unwrap_or_default();
            let Ok(stub_source) = generate_typepython_stub_source(module, &context) else {
                continue;
            };
            let stub_file = SourceFile {
                path: syntax.source.path.with_extension("pyi"),
                kind: SourceKind::Stub,
                logical_module: syntax.source.logical_module.clone(),
                text: stub_source,
            };
            typepython_syntax::parse(stub_file)
        } else {
            syntax.clone()
        };

        for entry in declaration_surface(&surface_syntax)
            .into_iter()
            .filter(is_public_surface_entry)
            .filter(|entry| entry.kind != "import")
        {
            if !surface_detail_is_incomplete(&entry.detail) {
                continue;
            }

            diagnostics.push(Diagnostic::error(
                "TPY4015",
                format!(
                    "module `{}` exports incomplete type surface for `{}`",
                    syntax.source.path.display(),
                    display_surface_entry(&entry)
                ),
            ));
        }
    }

    diagnostics
}

fn is_public_surface_entry(entry: &SurfaceEntry) -> bool {
    if entry.name.starts_with('_') {
        return false;
    }

    match &entry.owner {
        Some(owner) => !owner.starts_with('_'),
        None => true,
    }
}

fn display_surface_entry(entry: &SurfaceEntry) -> String {
    match &entry.owner {
        Some(owner) => format!("{owner}.{}", entry.name),
        None => entry.name.clone(),
    }
}

fn surface_detail_is_incomplete(detail: &str) -> bool {
    let mut token = String::new();

    for ch in detail.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
            continue;
        }

        if matches!(token.as_str(), "dynamic" | "unknown") {
            return true;
        }
        token.clear();
    }

    matches!(token.as_str(), "dynamic" | "unknown")
}

fn format_signature(params: &[typepython_syntax::FunctionParam], returns: Option<&str>) -> String {
    format!(
        "({})->{}",
        params
            .iter()
            .map(|param| match &param.annotation {
                Some(annotation) => format!("{}:{}", param.name, annotation),
                None => param.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(","),
        returns.unwrap_or("")
    )
}

fn write_incremental_snapshot(cache_dir: &Path, snapshot: &IncrementalState) -> Result<PathBuf> {
    fs::create_dir_all(cache_dir)
        .with_context(|| format!("unable to create cache directory {}", cache_dir.display()))?;
    let snapshot_path = cache_dir.join("snapshot.json");
    let payload = encode_snapshot(snapshot).context("unable to serialize incremental snapshot")?;
    fs::write(&snapshot_path, payload)
        .with_context(|| format!("unable to write {}", snapshot_path.display()))?;
    Ok(snapshot_path)
}

fn compile_runtime_bytecode(config: &ConfigHandle, artifacts: &[EmitArtifact]) -> Result<usize> {
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

fn bytecode_path_for(runtime_path: &Path) -> Result<PathBuf> {
    let parent = runtime_path.parent().ok_or_else(|| {
        anyhow::anyhow!("runtime artifact {} has no parent directory", runtime_path.display())
    })?;
    let stem = runtime_path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| {
        anyhow::anyhow!("runtime artifact {} has no valid file stem", runtime_path.display())
    })?;
    Ok(parent.join("__pycache__").join(format!("{stem}.pyc")))
}

fn resolve_python_executable(config: &ConfigHandle) -> PathBuf {
    match config.config.resolution.python_executable.as_deref() {
        Some(executable) => {
            let path = Path::new(executable);
            if path.is_absolute() || !executable.contains(std::path::MAIN_SEPARATOR) {
                path.to_path_buf()
            } else {
                config.config_dir.join(path)
            }
        }
        None => PathBuf::from("python3"),
    }
}

fn load_project(project: Option<&PathBuf>) -> Result<ConfigHandle> {
    let start = match project {
        Some(path) if path.is_file() => {
            path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
        }
        Some(path) => path.clone(),
        None => env::current_dir().context("unable to determine current directory")?,
    };

    load(start).context("unable to load TypePython project configuration")
}

fn collect_source_paths(config: &ConfigHandle) -> Result<SourceDiscovery> {
    let include_patterns =
        compile_patterns(config, &config.config.project.include, "project.include")?;
    let exclude_patterns =
        compile_patterns(config, &config.config.project.exclude, "project.exclude")?;
    let source_roots: Vec<_> =
        config.config.project.src.iter().map(|root| config.resolve_relative_path(root)).collect();
    let mut sources = Vec::new();

    for root in &source_roots {
        walk_directory(config, root, &include_patterns, &exclude_patterns, &mut sources)?;
    }

    sort_sources_by_type_authority(&mut sources);
    let diagnostics = detect_module_collisions(&sources, &source_roots);

    Ok(SourceDiscovery { sources, diagnostics })
}

fn bundled_stdlib_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib")
}

fn walk_bundled_stdlib_directory(
    root: &Path,
    directory: &Path,
    target_python: &str,
    sources: &mut Vec<DiscoveredSource>,
) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(directory)
        .with_context(|| format!("unable to read directory {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            walk_bundled_stdlib_directory(root, &path, target_python, sources)?;
            continue;
        }

        let Some(kind) = SourceKind::from_path(&path) else {
            continue;
        };
        if kind != SourceKind::Stub {
            continue;
        }
        if !bundled_stdlib_file_matches_target(&path, target_python)? {
            continue;
        }

        let Some(logical_module) = logical_module_path(root, &path) else {
            continue;
        };
        sources.push(DiscoveredSource {
            path,
            root: root.to_path_buf(),
            kind,
            logical_module,
            load_as_inferred_stub: false,
        });
    }

    Ok(())
}

fn bundled_stdlib_sources(target_python: &str) -> Result<Vec<DiscoveredSource>> {
    bundled_stdlib_sources_for_root(&bundled_stdlib_root(), target_python)
}

fn bundled_stdlib_sources_for_root(
    root: &Path,
    target_python: &str,
) -> Result<Vec<DiscoveredSource>> {
    let mut sources = Vec::new();
    if root.exists() {
        walk_bundled_stdlib_directory(root, root, target_python, &mut sources)?;
    }
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(sources)
}

fn bundled_stdlib_snapshot_identity(target_python: &str) -> Result<String> {
    bundled_stdlib_snapshot_identity_for_root(&bundled_stdlib_root(), target_python)
}

fn bundled_stdlib_snapshot_identity_for_root(root: &Path, target_python: &str) -> Result<String> {
    let mut files = Vec::new();
    if root.exists() {
        collect_stdlib_files(root, root, target_python, &mut files)?;
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hash = Wrapping(0xcbf29ce484222325_u64);
    let prime = Wrapping(0x100000001b3_u64);
    for byte in target_python.as_bytes().iter().chain([0_u8].iter()) {
        hash ^= Wrapping(u64::from(*byte));
        hash *= prime;
    }
    for (relative, bytes) in files {
        for byte in relative.as_bytes().iter().chain([0_u8].iter()).chain(bytes.iter()) {
            hash ^= Wrapping(u64::from(*byte));
            hash *= prime;
        }
    }

    Ok(format!("fnv1a64:{:016x}", hash.0))
}

fn collect_stdlib_files(
    root: &Path,
    directory: &Path,
    target_python: &str,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<()> {
    for entry in fs::read_dir(directory).with_context(|| {
        format!("unable to read bundled stdlib directory {}", directory.display())
    })? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_stdlib_files(root, &path, target_python, files)?;
            continue;
        }
        if !bundled_stdlib_file_matches_target(&path, target_python)? {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("unable to relativize {}", path.display()))?;
        files.push((normalize_glob_path(relative), fs::read(&path)?));
    }

    Ok(())
}

fn bundled_stdlib_file_matches_target(path: &Path, target_python: &str) -> Result<bool> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("unable to read bundled stdlib file {}", path.display()))?;
    Ok(parse_bundled_stdlib_version_filter(&contents).allows(target_python))
}

#[derive(Debug, Default, Clone)]
struct BundledStdlibVersionFilter {
    min_python: Option<String>,
    max_python: Option<String>,
}

impl BundledStdlibVersionFilter {
    fn allows(&self, target_python: &str) -> bool {
        let target = parse_supported_python_version(target_python);
        let min_ok = self
            .min_python
            .as_deref()
            .and_then(parse_supported_python_version)
            .is_none_or(|minimum| target >= Some(minimum));
        let max_ok = self
            .max_python
            .as_deref()
            .and_then(parse_supported_python_version)
            .is_none_or(|maximum| target <= Some(maximum));
        min_ok && max_ok
    }
}

fn parse_bundled_stdlib_version_filter(source: &str) -> BundledStdlibVersionFilter {
    let mut filter = BundledStdlibVersionFilter::default();

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.starts_with('#') {
            break;
        }
        let metadata = trimmed.trim_start_matches('#').trim();
        let Some(metadata) = metadata.strip_prefix("typepython:") else {
            continue;
        };
        for field in metadata.split_whitespace() {
            if let Some(value) = field.strip_prefix("min-python=") {
                filter.min_python = Some(value.to_owned());
            } else if let Some(value) = field.strip_prefix("max-python=") {
                filter.max_python = Some(value.to_owned());
            }
        }
    }

    filter
}

fn parse_supported_python_version(version: &str) -> Option<(u8, u8)> {
    let (major, minor) = version.trim().split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

fn external_resolution_sources(config: &ConfigHandle) -> Result<Vec<DiscoveredSource>> {
    let mut sources = Vec::new();
    for root in configured_external_type_roots(config)? {
        walk_external_type_root(&root, &mut sources)?;
    }
    sort_sources_by_type_authority(&mut sources);
    sources.dedup_by(|left, right| left.path == right.path);
    Ok(sources)
}

fn configured_external_type_roots(config: &ConfigHandle) -> Result<Vec<ExternalSupportRoot>> {
    let mut roots = config
        .config
        .resolution
        .type_roots
        .iter()
        .map(|root| ExternalSupportRoot {
            path: config.resolve_relative_path(root),
            allow_untyped_runtime: false,
        })
        .collect::<Vec<_>>();
    roots.extend(discovered_python_type_roots(config));
    roots.retain(|root| root.path.exists());
    roots.sort_by(|left, right| left.path.cmp(&right.path));
    roots.dedup_by(|left, right| {
        if left.path == right.path {
            left.allow_untyped_runtime |= right.allow_untyped_runtime;
            true
        } else {
            false
        }
    });
    Ok(roots)
}

fn discovered_python_type_roots(config: &ConfigHandle) -> Vec<ExternalSupportRoot> {
    let interpreter = resolve_python_executable(config);
    python_type_roots_from_interpreter(&interpreter)
}

fn python_type_roots_from_interpreter(interpreter: &Path) -> Vec<ExternalSupportRoot> {
    let output = ProcessCommand::new(interpreter)
        .args([
            "-c",
            "import json, site, sysconfig; typed_roots=[]; typed_roots.extend(filter(None, [sysconfig.get_path('purelib'), sysconfig.get_path('platlib')])); typed_roots.extend(site.getsitepackages()); usersite = site.getusersitepackages(); typed_roots.extend(usersite if isinstance(usersite, list) else [usersite]); payload=[{'path': root, 'allow_untyped_runtime': False} for root in sorted({r for r in typed_roots if r})]; print(json.dumps(payload))",
        ])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(roots) = serde_json::from_slice::<Vec<ExternalSupportRootProbe>>(&output.stdout) else {
        return Vec::new();
    };
    roots
        .into_iter()
        .map(|root| ExternalSupportRoot {
            path: PathBuf::from(root.path),
            allow_untyped_runtime: root.allow_untyped_runtime,
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalSupportRootProbe {
    path: String,
    allow_untyped_runtime: bool,
}

fn walk_external_type_root(
    root: &ExternalSupportRoot,
    sources: &mut Vec<DiscoveredSource>,
) -> Result<()> {
    walk_external_type_root_directory(root, &root.path, sources)
}

fn walk_external_type_root_directory(
    root: &ExternalSupportRoot,
    directory: &Path,
    sources: &mut Vec<DiscoveredSource>,
) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)
        .with_context(|| format!("unable to read directory {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_external_type_root_directory(root, &path, sources)?;
            continue;
        }

        let Some(kind) = SourceKind::from_path(&path) else {
            continue;
        };
        if !external_source_allowed(root, &path, kind) {
            continue;
        }
        let Some(logical_module) = external_logical_module_path(&root.path, &path) else {
            continue;
        };
        sources.push(DiscoveredSource {
            path,
            root: root.path.clone(),
            kind,
            logical_module,
            load_as_inferred_stub: root.allow_untyped_runtime && kind == SourceKind::Python,
        });
    }
    Ok(())
}

fn external_source_allowed(root: &ExternalSupportRoot, path: &Path, kind: SourceKind) -> bool {
    match kind {
        SourceKind::Stub => true,
        SourceKind::Python => external_runtime_allowed(root, path),
        SourceKind::TypePython => false,
    }
}

fn external_runtime_allowed(root: &ExternalSupportRoot, path: &Path) -> bool {
    if root.allow_untyped_runtime {
        return true;
    }
    let Some(stub_root) = sibling_stub_distribution_root(&root.path, path) else {
        return external_runtime_is_typed(&root.path, path);
    };

    partial_stub_package_marker(&stub_root)
        && runtime_module_missing_from_stub_package(&root.path, path, &stub_root)
}

fn external_logical_module_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let first =
        relative.components().next().and_then(|component| component.as_os_str().to_str())?;
    if first.ends_with("-stubs") {
        let stub_distribution_root = root.join(first);
        let Ok(relative_inside_distribution) = relative.strip_prefix(first) else {
            return None;
        };
        return logical_module_path(
            &stub_distribution_root,
            &stub_distribution_root.join(relative_inside_distribution),
        );
    }

    logical_module_path(root, path)
}

fn external_runtime_is_typed(root: &Path, path: &Path) -> bool {
    let Ok(relative_parent) = path.parent().unwrap_or(root).strip_prefix(root) else {
        return false;
    };
    let mut current = PathBuf::new();
    for component in relative_parent.components() {
        current.push(component.as_os_str());
        if root.join(&current).join("py.typed").is_file() {
            return true;
        }
    }
    false
}

fn sibling_stub_distribution_root(root: &Path, path: &Path) -> Option<PathBuf> {
    let Ok(relative) = path.strip_prefix(root) else {
        return None;
    };
    let mut components = relative.components();
    let first = components.next().and_then(|component| component.as_os_str().to_str())?;
    if first.ends_with("-stubs") {
        return None;
    }

    let stub_root = root.join(format!("{first}-stubs"));
    stub_root.exists().then_some(stub_root)
}

fn runtime_module_missing_from_stub_package(root: &Path, path: &Path, stub_root: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let Some(first) =
        relative.components().next().and_then(|component| component.as_os_str().to_str())
    else {
        return false;
    };
    let Ok(relative_inside_package) = relative.strip_prefix(first) else {
        return false;
    };
    let nested_stub_root = stub_root.join(first);
    let stub_package_root =
        if nested_stub_root.exists() { nested_stub_root } else { stub_root.to_path_buf() };
    let stub_candidate = stub_package_root.join(relative_inside_package).with_extension("pyi");
    !stub_candidate.is_file()
}

fn sort_sources_by_type_authority(sources: &mut [DiscoveredSource]) {
    sources.sort_by(|left, right| {
        left.logical_module
            .cmp(&right.logical_module)
            .then_with(|| {
                source_kind_authority_rank(left.kind).cmp(&source_kind_authority_rank(right.kind))
            })
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn source_kind_authority_rank(kind: SourceKind) -> u8 {
    match kind {
        SourceKind::TypePython => 0,
        SourceKind::Stub => 1,
        SourceKind::Python => 2,
    }
}

fn partial_stub_package_marker(stub_root: &Path) -> bool {
    fs::read_to_string(stub_root.join("py.typed"))
        .ok()
        .is_some_and(|contents| contents.lines().any(|line| line.trim() == "partial"))
}

fn compile_patterns(
    config: &ConfigHandle,
    patterns: &[String],
    field_name: &str,
) -> Result<Vec<Pattern>> {
    patterns
        .iter()
        .map(|pattern| {
            Pattern::new(pattern).with_context(|| {
                format!(
                    "TPY1002: invalid configuration value in {}: {field_name} contains invalid glob pattern `{pattern}`",
                    config.config_path.display()
                )
            })
        })
        .collect()
}

fn walk_directory(
    config: &ConfigHandle,
    directory: &Path,
    include_patterns: &[Pattern],
    exclude_patterns: &[Pattern],
    sources: &mut Vec<DiscoveredSource>,
) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(directory)
        .with_context(|| format!("unable to read directory {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            walk_directory(config, &path, include_patterns, exclude_patterns, sources)?;
            continue;
        }

        let Some(kind) = SourceKind::from_path(&path) else {
            continue;
        };

        if !is_selected_source_path(config, &path, include_patterns, exclude_patterns)? {
            continue;
        }

        let Some(root) = source_root_for_path(config, &path) else {
            continue;
        };
        let Some(logical_module) = logical_module_path(&root, &path) else {
            continue;
        };

        sources.push(DiscoveredSource {
            path,
            root,
            kind,
            logical_module,
            load_as_inferred_stub: false,
        });
    }

    Ok(())
}

fn is_selected_source_path(
    config: &ConfigHandle,
    path: &Path,
    include_patterns: &[Pattern],
    exclude_patterns: &[Pattern],
) -> Result<bool> {
    let relative = path.strip_prefix(&config.config_dir).with_context(|| {
        format!("unable to relativize {} to {}", path.display(), config.config_dir.display())
    })?;
    let relative = normalize_glob_path(relative);

    let is_included = include_patterns.iter().any(|pattern| pattern.matches(&relative));
    let is_excluded = exclude_patterns.iter().any(|pattern| pattern.matches(&relative));

    Ok(is_included && !is_excluded)
}

fn source_root_for_path(config: &ConfigHandle, path: &Path) -> Option<PathBuf> {
    config
        .config
        .project
        .src
        .iter()
        .map(|root| config.resolve_relative_path(root))
        .find(|root| path.starts_with(root))
}

fn logical_module_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let package_components = package_components(parent)?;
    let stem = path.file_stem()?.to_str()?;

    if stem == "__init__" {
        return (!package_components.is_empty()).then(|| package_components.join("."));
    }

    let mut components = package_components;
    components.push(stem.to_owned());
    Some(components.join("."))
}

fn package_components(relative_parent: &Path) -> Option<Vec<String>> {
    let mut components = Vec::new();
    for component in relative_parent.components() {
        let name = component.as_os_str().to_str()?.to_owned();
        components.push(name);
    }

    Some(components)
}

fn normalize_glob_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn detect_module_collisions(
    sources: &[DiscoveredSource],
    source_roots: &[PathBuf],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    let mut by_module: BTreeMap<&str, Vec<&DiscoveredSource>> = BTreeMap::new();

    for source in sources {
        by_module.entry(&source.logical_module).or_default().push(source);
    }

    let normalized_roots: BTreeSet<_> =
        source_roots.iter().map(|root| normalize_glob_path(root)).collect();

    for (logical_module, module_sources) in by_module {
        if module_sources.len() < 2 {
            continue;
        }

        let distinct_roots: BTreeSet<_> =
            module_sources.iter().map(|source| normalize_glob_path(&source.root)).collect();
        let has_multiple_roots =
            distinct_roots.len() > 1 && distinct_roots.is_subset(&normalized_roots);
        let allows_runtime_with_stub = allows_runtime_with_stub_pair(&module_sources);

        if has_multiple_roots || !allows_runtime_with_stub {
            let mut diagnostic = Diagnostic::error(
                "TPY3002",
                format!("logical module `{logical_module}` has conflicting source files"),
            );

            for source in &module_sources {
                diagnostic = diagnostic.with_note(format!(
                    "{} ({})",
                    source.path.display(),
                    source_kind_name(source.kind)
                ));
            }

            diagnostics.push(diagnostic);
        }
    }

    diagnostics
}

fn allows_runtime_with_stub_pair(module_sources: &[&DiscoveredSource]) -> bool {
    if module_sources.len() != 2 {
        return false;
    }

    matches!(
        (module_sources[0].kind, module_sources[1].kind),
        (SourceKind::Python, SourceKind::Stub) | (SourceKind::Stub, SourceKind::Python)
    ) && module_sources[0].root == module_sources[1].root
}

fn source_kind_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::TypePython => ".tpy",
        SourceKind::Python => ".py",
        SourceKind::Stub => ".pyi",
    }
}

fn write_file(path: &Path, content: &str, force: bool) -> Result<()> {
    if path.exists() && !force {
        anyhow::bail!("{} already exists; rerun with --force to overwrite", path.display());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("unable to create directory {}", parent.display()))?;
    }

    fs::write(path, content).with_context(|| format!("unable to write {}", path.display()))
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("unable to remove {}", path.display()))?;
    }

    Ok(())
}

fn print_summary(
    format: OutputFormat,
    summary: &CommandSummary,
    diagnostics: &DiagnosticReport,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("{}:", summary.command);
            println!("  config: {} ({})", summary.config_path, summary.config_source);
            println!("  discovered sources: {}", summary.discovered_sources);
            println!("  lowered modules: {}", summary.lowered_modules);
            println!("  planned artifacts: {}", summary.planned_artifacts);
            println!("  tracked modules: {}", summary.tracked_modules);
            for note in &summary.notes {
                println!("  note: {note}");
            }

            if !diagnostics.is_empty() {
                print!("{}", diagnostics.as_text());
            }
        }
        OutputFormat::Json => {
            let payload = serde_json::json!({
                "summary": summary,
                "diagnostics": diagnostics,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload)
                    .context("unable to serialize command summary as JSON")?
            );
        }
    }

    Ok(())
}

fn print_migration_report(
    format: OutputFormat,
    summary: &CommandSummary,
    report: &MigrationReport,
    diagnostics: &DiagnosticReport,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            print_summary(OutputFormat::Text, summary, diagnostics)?;
            println!("  migration total declarations: {}", report.total_declarations);
            println!("  migration known declarations: {}", report.known_declarations);
            println!("  migration dynamic boundaries: {}", report.total_dynamic_boundaries);
            println!("  migration unknown boundaries: {}", report.total_unknown_boundaries);
            println!("  file coverage:");
            for entry in &report.files {
                println!(
                    "    {} [{}]: {}/{} known ({:.1}%), dynamic={}, unknown={}",
                    entry.path,
                    entry.source_kind.as_deref().unwrap_or("?"),
                    entry.known_declarations,
                    entry.declarations,
                    entry.coverage_percent,
                    entry.dynamic_boundaries,
                    entry.unknown_boundaries
                );
            }
            println!("  directory coverage:");
            for entry in &report.directories {
                println!(
                    "    {}: {}/{} known ({:.1}%), dynamic={}, unknown={}",
                    entry.path,
                    entry.known_declarations,
                    entry.declarations,
                    entry.coverage_percent,
                    entry.dynamic_boundaries,
                    entry.unknown_boundaries
                );
            }
            println!("  high-impact untyped files:");
            for entry in &report.high_impact_untyped_files {
                println!(
                    "    {}: downstream_refs={}, untyped={}, dynamic={}, unknown={}",
                    entry.path,
                    entry.downstream_references,
                    entry.untyped_declarations,
                    entry.dynamic_boundaries,
                    entry.unknown_boundaries
                );
            }
        }
        OutputFormat::Json => {
            let payload = serde_json::json!({
                "summary": summary,
                "report": report,
                "diagnostics": diagnostics,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload)
                    .context("unable to serialize migration report as JSON")?
            );
        }
    }

    Ok(())
}

fn exit_code(diagnostics: &DiagnosticReport) -> ExitCode {
    if diagnostics.has_errors() { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

#[cfg(test)]
mod tests;

//! `typepython` command-line entrypoint.

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode},
    sync::mpsc::{self, RecvTimeoutError},
    time::Duration,
};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use glob::Pattern;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tracing_subscriber::EnvFilter;
use typepython_binding::bind;
use typepython_checking::check_with_options;
use typepython_config::{ConfigHandle, ConfigSource, load};
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_emit::{EmitArtifact, plan_emits, write_runtime_outputs};
use typepython_graph::build;
use typepython_incremental::{IncrementalState, snapshot};
use typepython_lowering::{LoweredModule, LoweringResult, lower};
use typepython_syntax::{SourceFile, SourceKind, apply_type_ignore_directives, parse};

const CONFIG_TEMPLATE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates/typepython.toml"));
const INIT_SOURCE_TEMPLATE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates/src/app/__init__.tpy"));

#[derive(Debug, Parser)]
#[command(
    name = "typepython",
    version,
    about = "Rust workspace skeleton for the TypePython compiler"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a starter TypePython config and source tree.
    Init(InitArgs),
    /// Load the project and execute the placeholder compile pipeline.
    Check(RunArgs),
    /// Create output/cache directories and execute the placeholder pipeline.
    Build(RunArgs),
    /// Execute the placeholder pipeline and report watch-mode status.
    Watch(RunArgs),
    /// Remove configured build and cache directories.
    Clean(CleanArgs),
    /// Start the placeholder language server.
    Lsp(RunArgs),
    /// Execute the placeholder verification flow.
    Verify(RunArgs),
    /// Execute the placeholder migration flow.
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
}

#[derive(Debug, Args)]
struct CleanArgs {
    /// Project directory to search from.
    #[arg(long, value_name = "PATH")]
    project: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct MigrateArgs {
    #[command(flatten)]
    run: RunArgs,
    /// Emit the migration report placeholder.
    #[arg(long)]
    report: bool,
}

#[derive(Debug)]
struct PipelineSnapshot {
    lowered_modules: Vec<LoweredModule>,
    emit_plan: Vec<EmitArtifact>,
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
        return ExitCode::FAILURE;
    }

    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
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
    let source_path = root.join("src/app/__init__.tpy");

    write_file(&config_path, CONFIG_TEMPLATE, args.force)?;
    write_file(&source_path, INIT_SOURCE_TEMPLATE, args.force)?;

    println!("initialized TypePython project skeleton at {}", root.display());
    println!("  config: {}", config_path.display());
    println!("  source: {}", source_path.display());

    if root.join("pyproject.toml").is_file() {
        println!("  note: existing pyproject.toml detected; typepython.toml remains authoritative");
    }

    Ok(ExitCode::SUCCESS)
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
        watcher.watch(path, *mode).with_context(|| format!("unable to watch {}", path.display()))?;
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
            format!(
                "emit blocked by `emit.no_emit_on_error` for {}",
                config.config_dir.display()
            ),
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
        return Err(anyhow::anyhow!("`typepython lsp` speaks JSON-RPC over stdio and does not support `--format json`"));
    }
    typepython_lsp::serve(&config)?;
    Ok(ExitCode::SUCCESS)
}

fn run_verify(args: RunArgs) -> Result<ExitCode> {
    let config = load_project(args.project.as_ref())?;
    let snapshot = run_pipeline(&config)?;
    let diagnostics = if snapshot.diagnostics.has_errors() {
        snapshot.diagnostics.clone()
    } else {
        verify_build_artifacts(&config, &snapshot.emit_plan)
    };

    let summary = CommandSummary {
        command: String::from("verify"),
        config_path: config.config_path.display().to_string(),
        config_source: config.source,
        discovered_sources: snapshot.discovered_sources,
        lowered_modules: snapshot.lowered_modules.len(),
        planned_artifacts: snapshot.emit_plan.len(),
        tracked_modules: snapshot.tracked_modules,
        notes: vec![String::from(
            "verifies current runtime artifacts, emitted stubs, and `py.typed` in the build tree",
        )],
    };

    print_summary(args.format, &summary, &diagnostics)?;
    Ok(exit_code(&diagnostics))
}

fn run_migrate(args: MigrateArgs) -> Result<ExitCode> {
    let config = load_project(args.run.project.as_ref())?;
    let discovery = collect_source_paths(&config)?;
    let mut syntax_trees = load_syntax_trees(&discovery.sources)?;
    let bundled_sources = bundled_stdlib_sources()?;
    syntax_trees.extend(load_syntax_trees(&bundled_sources)?);
    let mut diagnostics = discovery.diagnostics.clone();
    let mut parse_diagnostics = collect_parse_diagnostics(&syntax_trees);
    apply_type_ignore_directives(&syntax_trees, &mut parse_diagnostics);
    diagnostics.diagnostics.extend(parse_diagnostics.diagnostics.into_iter());

    let report = build_migration_report(&config, &syntax_trees);
    let mut notes = vec![String::from(
        "pass-through inference and stub generation remain experimental and disabled",
    )];
    if args.report {
        notes.push(String::from(
            "migration report includes file coverage, directory coverage, and high-impact untyped files",
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
    let diagnostics = build_diagnostics(config, &snapshot.diagnostics);
    if should_emit_build_outputs(config, &snapshot.diagnostics) {
        let runtime_summary = write_runtime_outputs(
            &snapshot.emit_plan,
            &snapshot.lowered_modules,
            config.config.emit.runtime_validators,
        )
            .with_context(|| {
                format!(
                    "unable to write runtime artifacts under {}",
                    config.resolve_relative_path(&config.config.project.out_dir).display()
                )
            })?;
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
    fs::create_dir_all(config.resolve_relative_path(&config.config.project.out_dir)).with_context(|| {
        format!(
            "unable to create output directory {}",
            config.resolve_relative_path(&config.config.project.out_dir).display()
        )
    })?;
    fs::create_dir_all(config.resolve_relative_path(&config.config.project.cache_dir)).with_context(|| {
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
        format!(
            "rebuild triggered by {preview} and {} more path(s)",
            changed_paths.len() - 3
        )
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
                    *downstream_reference_counts
                        .entry(stats.entry.path.clone())
                        .or_default() += 1;
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

fn accumulate_statement_coverage(statement: &typepython_syntax::SyntaxStatement, tally: &mut CoverageTally) {
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
            let (annotation_known, dynamic_count, unknown_count) =
                known_type_slot(statement.annotation.as_deref().or(statement.value_type.as_deref()));
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
        typepython_syntax::ClassMemberKind::Method | typepython_syntax::ClassMemberKind::Overload => {
            function_signature_coverage(
                &member.params,
                member.returns.as_deref(),
                !matches!(member.method_kind, Some(typepython_syntax::MethodKind::Static)),
            )
        }
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

        let (param_known, dynamic_count, unknown_count) = known_type_slot(param.annotation.as_deref());
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
    (
        !text.is_empty() && dynamic_count == 0 && unknown_count == 0,
        dynamic_count,
        unknown_count,
    )
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
    if total == 0 {
        100.0
    } else {
        ((known as f64 / total as f64) * 1000.0).round() / 10.0
    }
}

fn source_kind_label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::TypePython => "tpy",
        SourceKind::Python => "py",
        SourceKind::Stub => "pyi",
    }
}

fn load_syntax_trees(sources: &[DiscoveredSource]) -> Result<Vec<typepython_syntax::SyntaxTree>> {
    sources
        .iter()
        .map(|source| {
            let mut source_file = SourceFile::from_path(&source.path)
                .with_context(|| format!("unable to read {}", source.path.display()))?;
            source_file.logical_module = source.logical_module.clone();
            Ok(parse(source_file))
        })
        .collect::<Result<Vec<_>>>()
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
        "parser extensions are still incomplete in this milestone",
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
            incremental: IncrementalState::default(),
            tracked_modules: 0,
            discovered_sources: discovery.sources.len(),
            diagnostics: discovery.diagnostics,
        });
    }

    let source_paths: Vec<_> = discovery.sources.iter().map(|source| source.path.clone()).collect();
    let syntax_trees = load_syntax_trees(&discovery.sources)?;
    let mut checking_sources = bundled_stdlib_sources()?;
    checking_sources.extend(external_resolution_sources(config)?);
    let checking_support_syntax = load_syntax_trees(&checking_sources)?;
    let mut all_syntax_trees = syntax_trees.clone();
    all_syntax_trees.extend(checking_support_syntax);
    let mut parse_diagnostics = collect_parse_diagnostics(&all_syntax_trees);
    apply_type_ignore_directives(&syntax_trees, &mut parse_diagnostics);
    if parse_diagnostics.has_errors() {
        return Ok(PipelineSnapshot {
            lowered_modules: Vec::new(),
            emit_plan: Vec::new(),
            incremental: IncrementalState::default(),
            tracked_modules: 0,
            discovered_sources: source_paths.len(),
            diagnostics: parse_diagnostics,
        });
    }

    let lowering_results: Vec<_> = syntax_trees.iter().map(lower).collect();
    let lowering_diagnostics = collect_lowering_diagnostics(&lowering_results);
    if lowering_diagnostics.has_errors() {
        return Ok(PipelineSnapshot {
            lowered_modules: Vec::new(),
            emit_plan: Vec::new(),
            incremental: IncrementalState::default(),
            tracked_modules: 0,
            discovered_sources: source_paths.len(),
            diagnostics: lowering_diagnostics,
        });
    }

    let lowered_modules: Vec<_> = lowering_results.into_iter().map(|result| result.module).collect();
    let bindings: Vec<_> = all_syntax_trees.iter().map(bind).collect();
    let graph = build(&bindings);
    let mut diagnostics = check_with_options(
        &graph,
        config.config.typing.require_explicit_overrides,
        config.config.typing.enable_sealed_exhaustiveness,
        config.config.typing.report_deprecated,
    )
    .diagnostics;
    apply_type_ignore_directives(&syntax_trees, &mut diagnostics);
    diagnostics.diagnostics.extend(
        public_surface_completeness_diagnostics(config, &syntax_trees)
            .diagnostics
            .into_iter(),
    );
    let emit_plan = plan_emits(config, &lowered_modules);
    let incremental = snapshot(&graph);
    let tracked_modules = incremental.fingerprints.len();

    Ok(PipelineSnapshot {
        lowered_modules,
        emit_plan,
        incremental,
        tracked_modules,
        discovered_sources: source_paths.len(),
        diagnostics,
    })
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
            if runtime_path.file_name().is_some_and(|name| name == "__init__.py") {
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
        }

        if let (Some(runtime_path), Some(stub_path)) = (&artifact.runtime_path, &artifact.stub_path) {
            if runtime_path.exists() && stub_path.exists() {
                if let Some(diagnostic) = verify_emitted_declaration_surface(runtime_path, stub_path) {
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

    let snapshot_path = config
        .resolve_relative_path(&config.config.project.cache_dir)
        .join("snapshot.json");
    if !snapshot_path.exists() {
        diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!("missing incremental snapshot `{}`", snapshot_path.display()),
        ));
    }

    diagnostics
}

fn verify_emitted_text_artifact(path: &Path) -> Option<Diagnostic> {
    let source = match SourceFile::from_path(path) {
        Ok(source) => source,
        Err(error) => {
            return Some(Diagnostic::error(
                "TPY5003",
                format!("unable to read emitted artifact `{}`: {error}", path.display()),
            ))
        }
    };
    let syntax = parse(source);
    if syntax.diagnostics.has_errors() {
        Some(Diagnostic::error(
            "TPY5003",
            format!("emitted artifact `{}` is not valid Python syntax", path.display()),
        ))
    } else {
        None
    }
}

fn verify_emitted_declaration_surface(runtime_path: &Path, stub_path: &Path) -> Option<Diagnostic> {
    let runtime_syntax = emitted_syntax(runtime_path)?;
    let stub_syntax = emitted_syntax(stub_path)?;

    if declaration_surface(&runtime_syntax) == declaration_surface(&stub_syntax) {
        None
    } else {
        Some(Diagnostic::error(
            "TPY5003",
            format!(
                "emitted runtime/stub declaration surfaces differ between `{}` and `{}`",
                runtime_path.display(),
                stub_path.display()
            ),
        ))
    }
}

fn emitted_syntax(path: &Path) -> Option<typepython_syntax::SyntaxTree> {
    let source = SourceFile::from_path(path).ok()?;
    let syntax = parse(source);
    if syntax.diagnostics.has_errors() {
        None
    } else {
        Some(syntax)
    }
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
                                member.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance),
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

fn public_surface_completeness_diagnostics(
    config: &ConfigHandle,
    syntax_trees: &[typepython_syntax::SyntaxTree],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();

    if !config.config.typing.require_known_public_types {
        return diagnostics;
    }

    for syntax in syntax_trees {
        for entry in declaration_surface(syntax)
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
    let payload = serde_json::to_string_pretty(&serde_json::json!({
        "fingerprints": snapshot.fingerprints,
    }))
    .context("unable to serialize incremental snapshot")?;
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

    sources.sort_by(|left, right| left.path.cmp(&right.path));
    let diagnostics = detect_module_collisions(&sources, &source_roots);

    Ok(SourceDiscovery { sources, diagnostics })
}

fn bundled_stdlib_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib")
}

fn walk_bundled_stdlib_directory(directory: &Path, sources: &mut Vec<DiscoveredSource>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(directory)
        .with_context(|| format!("unable to read directory {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            walk_bundled_stdlib_directory(&path, sources)?;
            continue;
        }

        let Some(kind) = SourceKind::from_path(&path) else {
            continue;
        };
        if kind != SourceKind::Stub {
            continue;
        }

        let root = bundled_stdlib_root();
        let Some(logical_module) = logical_module_path(&root, &path) else {
            continue;
        };
        sources.push(DiscoveredSource { path, root, kind, logical_module });
    }

    Ok(())
}

fn bundled_stdlib_sources() -> Result<Vec<DiscoveredSource>> {
    let root = bundled_stdlib_root();
    let mut sources = Vec::new();
    if root.exists() {
        walk_bundled_stdlib_directory(&root, &mut sources)?;
    }
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(sources)
}

fn external_resolution_sources(config: &ConfigHandle) -> Result<Vec<DiscoveredSource>> {
    let mut sources = Vec::new();
    for root in configured_external_type_roots(config)? {
        walk_external_type_root(&root, &mut sources)?;
    }
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    sources.dedup_by(|left, right| left.path == right.path);
    Ok(sources)
}

fn configured_external_type_roots(config: &ConfigHandle) -> Result<Vec<PathBuf>> {
    let mut roots = config
        .config
        .resolution
        .type_roots
        .iter()
        .map(|root| config.resolve_relative_path(root))
        .collect::<Vec<_>>();
    roots.extend(discovered_python_type_roots(config));
    roots.retain(|root| root.exists());
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn discovered_python_type_roots(config: &ConfigHandle) -> Vec<PathBuf> {
    if config.config.resolution.python_executable.is_none() {
        return Vec::new();
    }
    let interpreter = resolve_python_executable(config);
    let output = ProcessCommand::new(&interpreter)
        .args([
            "-c",
            "import json, site, sysconfig; roots=[]; roots.extend(filter(None, [sysconfig.get_path('purelib'), sysconfig.get_path('platlib')])); roots.extend(site.getsitepackages()); usersite = site.getusersitepackages(); roots.extend(usersite if isinstance(usersite, list) else [usersite]); print(json.dumps(sorted({r for r in roots if r})))",
        ])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(roots) = serde_json::from_slice::<Vec<String>>(&output.stdout) else {
        return Vec::new();
    };
    roots.into_iter().map(PathBuf::from).collect()
}

fn walk_external_type_root(root: &Path, sources: &mut Vec<DiscoveredSource>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)
        .with_context(|| format!("unable to read directory {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_external_type_root(&path, sources)?;
            continue;
        }

        let Some(kind) = SourceKind::from_path(&path) else {
            continue;
        };
        if !external_source_allowed(root, &path, kind) {
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
        });
    }
    Ok(())
}

fn external_source_allowed(root: &Path, path: &Path, kind: SourceKind) -> bool {
    match kind {
        SourceKind::Stub => true,
        SourceKind::Python => external_runtime_is_typed(root, path),
        SourceKind::TypePython => false,
    }
}

fn external_runtime_is_typed(root: &Path, path: &Path) -> bool {
    let Ok(relative_parent) = path
        .parent()
        .unwrap_or(root)
        .strip_prefix(root)
    else {
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

        sources.push(DiscoveredSource { path, root, kind, logical_module });
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
    let package_components = explicit_package_components(root, parent)?;
    let stem = path.file_stem()?.to_str()?;

    if stem == "__init__" {
        return (!package_components.is_empty()).then(|| package_components.join("."));
    }

    let mut components = package_components;
    components.push(stem.to_owned());
    Some(components.join("."))
}

fn explicit_package_components(root: &Path, relative_parent: &Path) -> Option<Vec<String>> {
    let mut components = Vec::new();
    let mut current = PathBuf::new();

    for component in relative_parent.components() {
        let name = component.as_os_str().to_str()?.to_owned();
        current.push(&name);
        if !is_explicit_package_dir(&root.join(&current)) {
            return None;
        }
        components.push(name);
    }

    Some(components)
}

fn is_explicit_package_dir(directory: &Path) -> bool {
    ["__init__.py", "__init__.tpy", "__init__.pyi"]
        .iter()
        .any(|entry| directory.join(entry).is_file())
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

#[cfg(test)]
mod tests {
    use super::{
        build_diagnostics, build_migration_report, collect_source_paths,
        compile_runtime_bytecode, format_watch_rebuild_note, load_syntax_trees, run_pipeline,
        should_emit_build_outputs, watch_targets, verify_build_artifacts,
        write_incremental_snapshot,
    };
    use notify::RecursiveMode;
    use std::{
        collections::BTreeSet,
        env, fs,
        path::MAIN_SEPARATOR,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use typepython_config::load;
    use typepython_diagnostics::{Diagnostic, DiagnosticReport};
    use typepython_emit::EmitArtifact;
    use typepython_incremental::IncrementalState;

    #[test]
    fn collect_source_paths_skips_implicit_namespace_packages() {
        let project_dir = temp_project_dir("skips_implicit_namespace_packages");
        let result = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            fs::create_dir_all(project_dir.join("src/pkg/subpkg")).unwrap();
            fs::write(project_dir.join("src/pkg/__init__.tpy"), "pass\n").unwrap();
            fs::write(project_dir.join("src/pkg/subpkg/mod.tpy"), "pass\n").unwrap();

            let config = load(&project_dir).unwrap();
            collect_source_paths(&config)
        })();
        remove_temp_project_dir(&project_dir);

        let discovery = result.unwrap();
        assert!(discovery.diagnostics.is_empty());
        assert_eq!(discovery.sources.len(), 1);
        assert_eq!(discovery.sources[0].logical_module, "pkg");
    }

    #[test]
    fn collect_source_paths_respects_include_and_exclude_patterns() {
        let project_dir = temp_project_dir("respects_include_and_exclude_patterns");
        let result = (|| {
            fs::write(
                project_dir.join("typepython.toml"),
                concat!(
                    "[project]\n",
                    "src = [\"src\"]\n",
                    "include = [\"src/**/*.tpy\"]\n",
                    "exclude = [\"src/pkg/excluded/**\"]\n"
                ),
            )
            .unwrap();
            fs::create_dir_all(project_dir.join("src/pkg/excluded")).unwrap();
            fs::write(project_dir.join("src/pkg/__init__.tpy"), "pass\n").unwrap();
            fs::write(project_dir.join("src/pkg/kept.tpy"), "pass\n").unwrap();
            fs::write(project_dir.join("src/pkg/excluded/__init__.tpy"), "pass\n").unwrap();
            fs::write(project_dir.join("src/pkg/excluded/hidden.tpy"), "pass\n").unwrap();

            let config = load(&project_dir).unwrap();
            collect_source_paths(&config)
        })();
        remove_temp_project_dir(&project_dir);

        let discovery = result.unwrap();
        let logical_modules: Vec<_> =
            discovery.sources.iter().map(|source| source.logical_module.as_str()).collect();
        assert_eq!(logical_modules, vec!["pkg", "pkg.kept"]);
    }

    #[test]
    fn collect_source_paths_reports_tpy_python_collisions() {
        let project_dir = temp_project_dir("reports_tpy_python_collisions");
        let result = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            fs::create_dir_all(project_dir.join("src/pkg")).unwrap();
            fs::write(project_dir.join("src/pkg/__init__.tpy"), "pass\n").unwrap();
            fs::write(project_dir.join("src/pkg/value.tpy"), "pass\n").unwrap();
            fs::write(project_dir.join("src/pkg/value.py"), "pass\n").unwrap();

            let config = load(&project_dir).unwrap();
            collect_source_paths(&config)
        })();
        remove_temp_project_dir(&project_dir);

        let discovery = result.unwrap();
        assert!(discovery.diagnostics.has_errors());
        let text = discovery.diagnostics.as_text();
        assert!(text.contains("TPY3002"));
        assert!(text.contains("pkg.value"));
    }

    #[test]
    fn collect_source_paths_allows_python_with_companion_stub() {
        let project_dir = temp_project_dir("allows_python_with_companion_stub");
        let result = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            fs::create_dir_all(project_dir.join("src/pkg")).unwrap();
            fs::write(project_dir.join("src/pkg/__init__.py"), "pass\n").unwrap();
            fs::write(project_dir.join("src/pkg/value.py"), "pass\n").unwrap();
            fs::write(project_dir.join("src/pkg/value.pyi"), "...\n").unwrap();

            let config = load(&project_dir).unwrap();
            collect_source_paths(&config)
        })();
        remove_temp_project_dir(&project_dir);

        let discovery = result.unwrap();
        assert!(discovery.diagnostics.is_empty());
        assert_eq!(discovery.sources.len(), 3);
    }

    #[test]
    fn collect_source_paths_reports_cross_root_collisions() {
        let project_dir = temp_project_dir("reports_cross_root_collisions");
        let result = (|| {
            fs::write(
                project_dir.join("typepython.toml"),
                concat!(
                    "[project]\n",
                    "src = [\"src\", \"vendor\"]\n",
                    "include = [\"src/**/*.tpy\", \"vendor/**/*.tpy\"]\n"
                ),
            )
            .unwrap();
            fs::create_dir_all(project_dir.join("src/pkg")).unwrap();
            fs::create_dir_all(project_dir.join("vendor/pkg")).unwrap();
            fs::write(project_dir.join("src/pkg/__init__.tpy"), "pass\n").unwrap();
            fs::write(project_dir.join("vendor/pkg/__init__.tpy"), "pass\n").unwrap();

            let config = load(&project_dir).unwrap();
            collect_source_paths(&config)
        })();
        remove_temp_project_dir(&project_dir);

        let discovery = result.unwrap();
        assert!(discovery.diagnostics.has_errors());
        assert!(discovery.diagnostics.as_text().contains("TPY3002"));
    }

    #[test]
    fn verify_build_artifacts_reports_missing_runtime_and_marker_files() {
        let project_dir = temp_project_dir("verify_build_artifacts_reports_missing_runtime_and_marker_files");
        let rendered = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            let config = load(&project_dir).unwrap();

            verify_build_artifacts(
                &config,
                &[EmitArtifact {
                    source_path: project_dir.join("src/app/__init__.tpy"),
                    runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                    stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
                }],
            )
            .as_text()
        })();
        remove_temp_project_dir(&project_dir);

        assert!(rendered.contains("TPY5003"));
        assert!(rendered.contains("missing runtime artifact"));
        assert!(rendered.contains("missing package marker"));
    }

    #[test]
    fn verify_build_artifacts_accepts_present_runtime_stub_and_marker_files() {
        let project_dir = temp_project_dir("verify_build_artifacts_accepts_present_runtime_stub_and_marker_files");
        let diagnostics = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n\n[emit]\nemit_pyc = true\n").unwrap();
            fs::create_dir_all(project_dir.join(".typepython/build/app")).unwrap();
            fs::create_dir_all(project_dir.join(".typepython/cache")).unwrap();
            fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n").unwrap();
            fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n").unwrap();
            fs::write(project_dir.join(".typepython/build/app/helpers.pyi"), "def helper() -> int: ...\n").unwrap();
            fs::write(project_dir.join(".typepython/build/app/py.typed"), "").unwrap();
            fs::create_dir_all(project_dir.join(".typepython/build/app/__pycache__")).unwrap();
            fs::write(project_dir.join(".typepython/build/app/__pycache__/__init__.pyc"), "pyc").unwrap();
            fs::write(project_dir.join(".typepython/cache/snapshot.json"), "{}\n").unwrap();
            let config = load(&project_dir).unwrap();

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
        })();
        remove_temp_project_dir(&project_dir);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn verify_build_artifacts_reports_missing_bytecode_when_enabled() {
        let project_dir = temp_project_dir("verify_build_artifacts_reports_missing_bytecode_when_enabled");
        let rendered = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n\n[emit]\nemit_pyc = true\n").unwrap();
            fs::create_dir_all(project_dir.join(".typepython/build/app")).unwrap();
            fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n").unwrap();
            fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n").unwrap();
            fs::write(project_dir.join(".typepython/build/app/py.typed"), "").unwrap();
            let config = load(&project_dir).unwrap();

            verify_build_artifacts(
                &config,
                &[EmitArtifact {
                    source_path: project_dir.join("src/app/__init__.tpy"),
                    runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                    stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
                }],
            )
            .as_text()
        })();
        remove_temp_project_dir(&project_dir);

        assert!(rendered.contains("TPY5003"));
        assert!(rendered.contains("missing bytecode artifact"));
    }

    #[test]
    fn verify_build_artifacts_reports_missing_incremental_snapshot() {
        let project_dir = temp_project_dir("verify_build_artifacts_reports_missing_incremental_snapshot");
        let rendered = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            fs::create_dir_all(project_dir.join(".typepython/build/app")).unwrap();
            fs::write(project_dir.join(".typepython/build/app/__init__.py"), "pass\n").unwrap();
            fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n").unwrap();
            fs::write(project_dir.join(".typepython/build/app/py.typed"), "").unwrap();
            let config = load(&project_dir).unwrap();

            verify_build_artifacts(
                &config,
                &[EmitArtifact {
                    source_path: project_dir.join("src/app/__init__.tpy"),
                    runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                    stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
                }],
            )
            .as_text()
        })();
        remove_temp_project_dir(&project_dir);

        assert!(rendered.contains("TPY5003"));
        assert!(rendered.contains("missing incremental snapshot"));
    }

    #[test]
    fn verify_build_artifacts_reports_invalid_emitted_python_syntax() {
        let project_dir = temp_project_dir("verify_build_artifacts_reports_invalid_emitted_python_syntax");
        let rendered = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            fs::create_dir_all(project_dir.join(".typepython/build/app")).unwrap();
            fs::create_dir_all(project_dir.join(".typepython/cache")).unwrap();
            fs::write(project_dir.join(".typepython/build/app/__init__.py"), "def broken(:\n").unwrap();
            fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n").unwrap();
            fs::write(project_dir.join(".typepython/build/app/py.typed"), "").unwrap();
            fs::write(project_dir.join(".typepython/cache/snapshot.json"), "{}\n").unwrap();
            let config = load(&project_dir).unwrap();

            verify_build_artifacts(
                &config,
                &[EmitArtifact {
                    source_path: project_dir.join("src/app/__init__.tpy"),
                    runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                    stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
                }],
            )
            .as_text()
        })();
        remove_temp_project_dir(&project_dir);

        assert!(rendered.contains("TPY5003"));
        assert!(rendered.contains("is not valid Python syntax"));
    }

    #[test]
    fn verify_build_artifacts_reports_runtime_stub_surface_mismatch() {
        let project_dir = temp_project_dir("verify_build_artifacts_reports_runtime_stub_surface_mismatch");
        let rendered = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            fs::create_dir_all(project_dir.join(".typepython/build/app")).unwrap();
            fs::create_dir_all(project_dir.join(".typepython/cache")).unwrap();
            fs::write(project_dir.join(".typepython/build/app/__init__.py"), "def build_user() -> int:\n    return 1\n").unwrap();
            fs::write(project_dir.join(".typepython/build/app/__init__.pyi"), "pass\n").unwrap();
            fs::write(project_dir.join(".typepython/build/app/py.typed"), "").unwrap();
            fs::write(project_dir.join(".typepython/cache/snapshot.json"), "{}\n").unwrap();
            let config = load(&project_dir).unwrap();

            verify_build_artifacts(
                &config,
                &[EmitArtifact {
                    source_path: project_dir.join("src/app/__init__.tpy"),
                    runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                    stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
                }],
            )
            .as_text()
        })();
        remove_temp_project_dir(&project_dir);

        assert!(rendered.contains("TPY5003"));
        assert!(rendered.contains("declaration surfaces differ"));
    }

    #[test]
    fn verify_build_artifacts_reports_method_kind_surface_mismatch() {
        let project_dir = temp_project_dir("verify_build_artifacts_reports_method_kind_surface_mismatch");
        let rendered = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            fs::create_dir_all(project_dir.join(".typepython/build/app")).unwrap();
            fs::create_dir_all(project_dir.join(".typepython/cache")).unwrap();
            fs::write(
                project_dir.join(".typepython/build/app/__init__.py"),
                "class Box:\n    @classmethod\n    def build(cls) -> None:\n        pass\n",
            )
            .unwrap();
            fs::write(
                project_dir.join(".typepython/build/app/__init__.pyi"),
                "class Box:\n    def build(self) -> None: ...\n",
            )
            .unwrap();
            fs::write(project_dir.join(".typepython/build/app/py.typed"), "").unwrap();
            fs::write(project_dir.join(".typepython/cache/snapshot.json"), "{}\n").unwrap();
            let config = load(&project_dir).unwrap();

            verify_build_artifacts(
                &config,
                &[EmitArtifact {
                    source_path: project_dir.join("src/app/__init__.tpy"),
                    runtime_path: Some(project_dir.join(".typepython/build/app/__init__.py")),
                    stub_path: Some(project_dir.join(".typepython/build/app/__init__.pyi")),
                }],
            )
            .as_text()
        })();
        remove_temp_project_dir(&project_dir);

        assert!(rendered.contains("TPY5003"));
        assert!(rendered.contains("declaration surfaces differ"));
    }

    #[test]
    fn run_pipeline_reports_incomplete_public_surface_when_required() {
        let project_dir = temp_project_dir("run_pipeline_reports_incomplete_public_surface_when_required");
        let rendered = (|| {
            fs::create_dir_all(project_dir.join("src/app")).unwrap();
            fs::write(
                project_dir.join("typepython.toml"),
                "[project]\nsrc = [\"src\"]\n\n[typing]\nrequire_known_public_types = true\n",
            )
            .unwrap();
            fs::write(
                project_dir.join("src/app/__init__.tpy"),
                "def leak(value: dynamic) -> int:\n    return 0\n",
            )
            .unwrap();
            let config = load(&project_dir).unwrap();

            run_pipeline(&config).unwrap().diagnostics.as_text()
        })();
        remove_temp_project_dir(&project_dir);

        assert!(rendered.contains("TPY4015"));
        assert!(rendered.contains("exports incomplete type surface for `leak`"));
    }

    #[test]
    fn run_pipeline_ignores_private_incomplete_surface_when_required() {
        let project_dir = temp_project_dir("run_pipeline_ignores_private_incomplete_surface_when_required");
        let diagnostics = (|| {
            fs::create_dir_all(project_dir.join("src/app")).unwrap();
            fs::write(
                project_dir.join("typepython.toml"),
                "[project]\nsrc = [\"src\"]\n\n[typing]\nrequire_known_public_types = true\n",
            )
            .unwrap();
            fs::write(
                project_dir.join("src/app/__init__.tpy"),
                "def _leak(value: dynamic) -> int:\n    return 0\n",
            )
            .unwrap();
            let config = load(&project_dir).unwrap();

            run_pipeline(&config).unwrap().diagnostics
        })();
        remove_temp_project_dir(&project_dir);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn build_diagnostics_adds_emit_blocked_error_when_configured() {
        let project_dir = temp_project_dir("build_diagnostics_adds_emit_blocked_error_when_configured");
        let rendered = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            let config = load(&project_dir).unwrap();
            let mut diagnostics = DiagnosticReport::default();
            diagnostics.push(Diagnostic::error("TPY4004", "duplicate declaration"));

            build_diagnostics(&config, &diagnostics).as_text()
        })();
        remove_temp_project_dir(&project_dir);

        assert!(rendered.contains("TPY4004"));
        assert!(rendered.contains("TPY5002"));
    }

    #[test]
    fn should_emit_build_outputs_respects_no_emit_on_error() {
        let project_dir = temp_project_dir("should_emit_build_outputs_respects_no_emit_on_error");
        let result = (|| {
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n\n[emit]\nno_emit_on_error = false\n").unwrap();
            let config = load(&project_dir).unwrap();
            let mut diagnostics = DiagnosticReport::default();
            diagnostics.push(Diagnostic::error("TPY4004", "duplicate declaration"));

            should_emit_build_outputs(&config, &diagnostics)
        })();
        remove_temp_project_dir(&project_dir);

        assert!(result);
    }

    #[test]
    fn write_incremental_snapshot_persists_fingerprint_json() {
        let project_dir = temp_project_dir("write_incremental_snapshot_persists_fingerprint_json");
        let result = (|| {
            let snapshot_path = write_incremental_snapshot(
                &project_dir.join(".typepython/cache"),
                &IncrementalState {
                    fingerprints: std::collections::BTreeMap::from([
                        (String::from("pkg.a"), 10),
                        (String::from("pkg.b"), 20),
                    ]),
                },
            )
            .unwrap();

            (snapshot_path, fs::read_to_string(project_dir.join(".typepython/cache/snapshot.json")).unwrap())
        })();
        remove_temp_project_dir(&project_dir);

        let (snapshot_path, rendered) = result;
        assert!(snapshot_path.ends_with("snapshot.json"));
        assert!(rendered.contains("pkg.a"));
        assert!(rendered.contains("pkg.b"));
    }

    #[test]
    fn compile_runtime_bytecode_uses_configured_python_executable() {
        let project_dir = temp_project_dir("compile_runtime_bytecode_uses_configured_python_executable");
        let result = (|| {
            fs::create_dir_all(project_dir.join("bin")).unwrap();
            fs::create_dir_all(project_dir.join("out/app")).unwrap();
            let log_path = project_dir.join("compiler.log");
            let fake_python = project_dir.join("bin/fake-python.sh");
            fs::write(
                project_dir.join("typepython.toml"),
                format!(
                    "[project]\nsrc = [\"src\"]\n\n[resolution]\npython_executable = \"bin{}fake-python.sh\"\n\n[emit]\nemit_pyc = true\n",
                    MAIN_SEPARATOR
                ),
            )
            .unwrap();
            fs::write(
                &fake_python,
                format!(
                    "#!/bin/sh\nif [ \"$1\" = \"-c\" ] && printf '%s' \"$2\" | grep -q 'version_info'; then\n  printf '3.10\\n'\n  exit 0\nfi\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
                    log_path.display()
                ),
            )
            .unwrap();
            #[cfg(unix)]
            {
                let mut permissions = fs::metadata(&fake_python).unwrap().permissions();
                permissions.set_mode(0o755);
                fs::set_permissions(&fake_python, permissions).unwrap();
            }
            let config = load(&project_dir).unwrap();
            let artifacts = vec![EmitArtifact {
                source_path: project_dir.join("src/app/__init__.tpy"),
                runtime_path: Some(project_dir.join("out/app/__init__.py")),
                stub_path: None,
            }];
            fs::write(project_dir.join("out/app/__init__.py"), "pass\n").unwrap();

            let compiled = compile_runtime_bytecode(&config, &artifacts).unwrap();
            let log = fs::read_to_string(&log_path).unwrap();
            (compiled, log)
        })();
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
        let targets = (|| {
            fs::create_dir_all(project_dir.join("src/app")).unwrap();
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            let config = load(&project_dir).unwrap();
            watch_targets(&config)
        })();
        remove_temp_project_dir(&project_dir);

        assert_eq!(targets.len(), 2);
        assert!(targets.iter().any(|(path, mode)| {
            path.ends_with("typepython.toml") && *mode == RecursiveMode::NonRecursive
        }));
        assert!(targets.iter().any(|(path, mode)| path.ends_with("src") && *mode == RecursiveMode::Recursive));
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
        let project_dir = temp_project_dir("build_migration_report_counts_file_coverage_and_boundaries");
        let report = (|| {
            fs::create_dir_all(project_dir.join("src/app")).unwrap();
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            fs::write(
                project_dir.join("src/app/__init__.tpy"),
                "def typed(value: int) -> int:\n    return value\n\ndef untyped(value) -> int:\n    return 0\n\nleak: dynamic = 1\n",
            )
            .unwrap();
            let config = load(&project_dir).unwrap();
            let discovery = collect_source_paths(&config).unwrap();
            let syntax_trees = load_syntax_trees(&discovery.sources).unwrap();
            build_migration_report(&config, &syntax_trees)
        })();
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
        let report = (|| {
            fs::create_dir_all(project_dir.join("src/app")).unwrap();
            fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
            fs::write(project_dir.join("src/app/__init__.tpy"), "pass\n").unwrap();
            fs::write(project_dir.join("src/app/a.tpy"), "def untyped(value) -> int:\n    return 0\n").unwrap();
            fs::write(
                project_dir.join("src/app/b.tpy"),
                "from app.a import untyped\n\ndef use(value: int) -> int:\n    return value\n",
            )
            .unwrap();
            fs::write(project_dir.join("src/app/c.tpy"), "def clean(value: int) -> int:\n    return value\n").unwrap();
            let config = load(&project_dir).unwrap();
            let discovery = collect_source_paths(&config).unwrap();
            let syntax_trees = load_syntax_trees(&discovery.sources).unwrap();
            build_migration_report(&config, &syntax_trees)
        })();
        remove_temp_project_dir(&project_dir);

        assert!(!report.high_impact_untyped_files.is_empty());
        assert!(report.high_impact_untyped_files[0].path.ends_with("src/app/a.tpy"));
        assert_eq!(report.high_impact_untyped_files[0].downstream_references, 1);
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
            println!(
                "  migration dynamic boundaries: {}",
                report.total_dynamic_boundaries
            );
            println!(
                "  migration unknown boundaries: {}",
                report.total_unknown_boundaries
            );
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

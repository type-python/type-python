//! `typepython` command-line entrypoint.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use tracing_subscriber::EnvFilter;
use typepython_binding::bind;
use typepython_checking::check;
use typepython_config::{ConfigHandle, ConfigSource, load};
use typepython_diagnostics::DiagnosticReport;
use typepython_emit::plan_emits;
use typepython_graph::build;
use typepython_incremental::snapshot;
use typepython_lowering::{LoweredModule, lower};
use typepython_syntax::{SourceFile, SourceKind, SyntaxTree, parse};

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
    syntax_trees: Vec<SyntaxTree>,
    lowered_modules: Vec<LoweredModule>,
    emit_plan_len: usize,
    tracked_modules: usize,
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
        Command::Watch(args) => run_with_pipeline(
            "watch",
            args,
            false,
            vec![String::from("watch invalidation and filesystem events are not implemented yet")],
        ),
        Command::Clean(args) => clean_project(args),
        Command::Lsp(args) => run_lsp(args),
        Command::Verify(args) => run_with_pipeline(
            "verify",
            args,
            false,
            vec![String::from(
                "typed publication checks and runtime/type surface verification are not implemented yet",
            )],
        ),
        Command::Migrate(args) => {
            let mut notes = vec![String::from(
                "migration analysis and pass-through inference are not implemented yet",
            )];
            if args.report {
                notes.push(String::from(
                    "--report requested: JSON/text migration reports will land in a later milestone",
                ));
            }
            run_with_pipeline("migrate", args.run, false, notes)
        }
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

    let snapshot = run_pipeline(&config)?;
    let summary = CommandSummary {
        command: String::from("build"),
        config_path: config.config_path.display().to_string(),
        config_source: config.source,
        discovered_sources: snapshot.syntax_trees.len(),
        lowered_modules: snapshot.lowered_modules.len(),
        planned_artifacts: snapshot.emit_plan_len,
        tracked_modules: snapshot.tracked_modules,
        notes: vec![String::from(
            "build directories are created, but real lowering/emit is still milestone work",
        )],
    };

    print_summary(args.format, &summary, &snapshot.diagnostics)?;
    Ok(exit_code(&snapshot.diagnostics))
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
    let mut notes = Vec::new();

    if let Err(error) = typepython_lsp::serve() {
        notes.push(error.to_string());
    }

    let snapshot = run_pipeline(&config)?;
    let summary = CommandSummary {
        command: String::from("lsp"),
        config_path: config.config_path.display().to_string(),
        config_source: config.source,
        discovered_sources: snapshot.syntax_trees.len(),
        lowered_modules: snapshot.lowered_modules.len(),
        planned_artifacts: snapshot.emit_plan_len,
        tracked_modules: snapshot.tracked_modules,
        notes,
    };

    print_summary(args.format, &summary, &snapshot.diagnostics)?;
    Ok(exit_code(&snapshot.diagnostics))
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
        "include/exclude globs, parser extensions, and semantic checking are still placeholders",
    ));

    let snapshot = run_pipeline(&config)?;
    let summary = CommandSummary {
        command: String::from(command),
        config_path: config.config_path.display().to_string(),
        config_source: config.source,
        discovered_sources: snapshot.syntax_trees.len(),
        lowered_modules: snapshot.lowered_modules.len(),
        planned_artifacts: snapshot.emit_plan_len,
        tracked_modules: snapshot.tracked_modules,
        notes,
    };

    print_summary(args.format, &summary, &snapshot.diagnostics)?;
    Ok(exit_code(&snapshot.diagnostics))
}

fn run_pipeline(config: &ConfigHandle) -> Result<PipelineSnapshot> {
    let source_paths = collect_source_paths(config)?;
    let syntax_trees: Vec<_> = source_paths
        .iter()
        .map(SourceFile::from_path)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("unable to read discovered source files")?
        .into_iter()
        .map(parse)
        .collect();

    let lowered_modules: Vec<_> = syntax_trees.iter().map(lower).collect();
    let bindings: Vec<_> = lowered_modules.iter().map(bind).collect();
    let graph = build(&bindings);
    let checking = check(&graph);
    let emit_plan = plan_emits(config, &lowered_modules);
    let incremental = snapshot(&graph);

    Ok(PipelineSnapshot {
        syntax_trees,
        lowered_modules,
        emit_plan_len: emit_plan.len(),
        tracked_modules: incremental.fingerprints.len(),
        diagnostics: checking.diagnostics,
    })
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

fn collect_source_paths(config: &ConfigHandle) -> Result<Vec<PathBuf>> {
    let mut sources = Vec::new();

    for root in &config.config.project.src {
        walk_directory(&config.resolve_relative_path(root), &mut sources)?;
    }

    sources.sort();
    Ok(sources)
}

fn walk_directory(directory: &Path, sources: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(directory)
        .with_context(|| format!("unable to read directory {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            walk_directory(&path, sources)?;
            continue;
        }

        if SourceKind::from_path(&path).is_some() {
            sources.push(path);
        }
    }

    Ok(())
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

fn exit_code(diagnostics: &DiagnosticReport) -> ExitCode {
    if diagnostics.has_errors() { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

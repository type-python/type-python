//! `typepython` command-line entrypoint.

mod cli;
mod discovery;
mod migration;
mod pipeline;
mod verification;

use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::mpsc::{self, RecvTimeoutError},
    time::Duration,
};

use anyhow::{Context, Result};
use clap::Parser;
use notify::{Config as NotifyConfig, RecommendedWatcher, Watcher};
use serde::Serialize;
use tracing_subscriber::EnvFilter;
use typepython_config::{ConfigError, ConfigHandle, ConfigSource, load};
use typepython_diagnostics::DiagnosticReport;

use crate::cli::{Cli, Command, InitArgs, OutputFormat, RunArgs};
use crate::migration::run_migrate;
use crate::pipeline::{
    clean_project, collect_watch_event_paths, format_watch_rebuild_note, run_build_like_command,
    run_lsp, run_with_pipeline, watch_targets,
};
use crate::verification::run_verify;

const CONFIG_TEMPLATE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates/typepython.toml"));
const INIT_SOURCE_TEMPLATE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates/src/app/__init__.tpy"));
const STATIC_ALL_NAMES_SCRIPT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/static_all_names.py"));

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

#[cfg(test)]
mod tests;

//! `typepython` command-line entrypoint.

mod adapter;
mod api_diff;
mod archive;
mod cli;
mod compat;
mod discovery;
mod migration;
mod pipeline;
mod type_health;
mod verification;

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Component, Path, PathBuf},
    process::ExitCode,
    sync::{
        Mutex,
        mpsc::{self, RecvTimeoutError},
    },
    time::Duration,
};

use anyhow::{Context, Result};
use clap::Parser;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tracing_subscriber::EnvFilter;
use typepython_config::{
    ConfigError, ConfigHandle, ConfigSource, load, load_without_python_executable_validation,
};
use typepython_diagnostics::DiagnosticReport;

use crate::adapter::run_adapter;
use crate::api_diff::run_api_diff;
use crate::cli::{Cli, Command, InitArgs, OutputFormat, RunArgs};
use crate::compat::run_compat;
use crate::migration::run_migrate;
use crate::pipeline::{
    clean_project, collect_watch_event_paths, format_watch_rebuild_note, run_build_like_command,
    run_lsp, run_with_pipeline, watch_targets,
};
use crate::type_health::run_type_health;
use crate::verification::run_verify;

const CONFIG_TEMPLATE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates/typepython.toml"));
const INIT_SOURCE_TEMPLATE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates/src/app/__init__.tpy"));
const RUNTIME_IMPORTABILITY_SCRIPT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/runtime_importability.py"));
pub(crate) const CLI_JSON_SCHEMA_VERSION: u32 = 1;

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
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = u8::try_from(error.exit_code()).unwrap_or(2);
            if exit_code == 0 || requested_output_format(env::args_os()) == OutputFormat::Text {
                let _ = error.print();
            } else {
                eprintln!("{}", render_error_json("cli", &error.to_string(), exit_code));
            }
            return ExitCode::from(exit_code);
        }
    };
    let format = cli.command.output_format();

    if let Err(error) = init_tracing() {
        render_command_error(
            format,
            "initialization",
            &anyhow::anyhow!("failed to initialize tracing: {error:#}"),
            2,
        );
        return ExitCode::from(2);
    }

    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            let numeric_exit_code = numeric_exit_code_for_error(&error);
            render_command_error(format, "command", &error, numeric_exit_code);
            exit_code_for_error(&error)
        }
    }
}

fn exit_code_for_error(error: &anyhow::Error) -> ExitCode {
    ExitCode::from(numeric_exit_code_for_error(error))
}

fn numeric_exit_code_for_error(error: &anyhow::Error) -> u8 {
    if error.chain().any(|cause| cause.downcast_ref::<ConfigError>().is_some()) {
        return 1;
    }

    if error
        .chain()
        .map(ToString::to_string)
        .any(|message| message.contains("already exists; rerun with --force"))
    {
        return 1;
    }

    2
}

fn requested_output_format(
    args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
) -> OutputFormat {
    let args =
        args.into_iter().map(|arg| arg.as_ref().to_string_lossy().into_owned()).collect::<Vec<_>>();
    if args.windows(2).any(|pair| pair[0] == "--format" && pair[1] == "json")
        || args.iter().any(|arg| arg == "--format=json")
    {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    }
}

fn render_command_error(format: OutputFormat, kind: &str, error: &anyhow::Error, exit_code: u8) {
    match format {
        OutputFormat::Text => eprintln!("{error:#}"),
        OutputFormat::Json => {
            eprintln!("{}", render_error_json(kind, &format!("{error:#}"), exit_code));
        }
    }
}

fn render_error_json(kind: &str, message: &str, exit_code: u8) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": CLI_JSON_SCHEMA_VERSION,
        "summary": null,
        "diagnostics": { "diagnostics": [] },
        "error": {
            "kind": kind,
            "message": message,
            "exit_code": exit_code,
        },
    }))
    .unwrap_or_else(|_| {
        String::from(
            r#"{"schema_version":1,"summary":null,"diagnostics":{"diagnostics":[]},"error":{"kind":"serialization","message":"unable to serialize CLI error","exit_code":2}}"#,
        )
    })
}

fn init_tracing() -> Result<()> {
    let filter = tracing_filter();
    let builder =
        tracing_subscriber::fmt().with_env_filter(filter).with_target(false).without_time();

    if let Some(log_file) = env::var_os("TYPEPYTHON_LOG_FILE") {
        let file = fs::OpenOptions::new().create(true).append(true).open(&log_file).with_context(
            || {
                format!(
                    "unable to open TYPEPYTHON_LOG_FILE at {}",
                    PathBuf::from(&log_file).display()
                )
            },
        )?;
        builder
            .with_writer(Mutex::new(file))
            .try_init()
            .map_err(|error| anyhow::anyhow!("unable to install tracing subscriber: {error}"))
    } else {
        builder
            .try_init()
            .map_err(|error| anyhow::anyhow!("unable to install tracing subscriber: {error}"))
    }
}

fn tracing_filter() -> EnvFilter {
    env::var("RUST_LOG")
        .or_else(|_| env::var("TYPEPYTHON_LOG"))
        .ok()
        .and_then(|value| EnvFilter::try_new(value).ok())
        .unwrap_or_else(|| EnvFilter::new("typepython_cli=info,typepython_config=info"))
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Init(args) => init_project(args),
        Command::Check(args) => run_with_pipeline("check", args, false, Vec::new()),
        Command::Build(args) => run_build(args),
        Command::Watch(args) => run_watch(args),
        Command::Clean(args) => clean_project(args),
        Command::Lsp(args) => run_lsp(args),
        Command::Verify(args) => run_verify(args),
        Command::Compat(args) => run_compat(args),
        Command::ApiDiff(args) => run_api_diff(args),
        Command::TypeHealth(args) => run_type_health(args),
        Command::Migrate(args) => run_migrate(args),
        Command::Adapter(args) => run_adapter(args),
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
    let initial_watch_targets = watch_targets(&config);
    let debounce_ms = config.config.watch.debounce_ms;
    let initial_rebuild = run_watch_rebuild_with_config(
        config,
        args.format,
        vec![format!(
            "watching {} path(s) with {}ms debounce",
            initial_watch_targets.len(),
            debounce_ms
        )],
    )?;
    let mut last_exit = initial_rebuild.exit_code;

    let (sender, receiver) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |result| {
            let _ = sender.send(result);
        },
        NotifyConfig::default(),
    )
    .context("unable to start filesystem watcher")?;

    let mut active_watch_targets = BTreeMap::new();
    apply_watch_targets(
        &mut watcher,
        &mut active_watch_targets,
        watch_targets(&initial_rebuild.config),
    )?;

    let mut debounce = Duration::from_millis(initial_rebuild.config.config.watch.debounce_ms);
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

        last_exit = match run_watch_rebuild(
            args.project.as_ref(),
            args.format,
            vec![format_watch_rebuild_note(&changed_paths)],
        ) {
            Ok(rebuild) => {
                let exit_code = rebuild.exit_code;
                if let Err(error) = apply_watch_targets(
                    &mut watcher,
                    &mut active_watch_targets,
                    watch_targets(&rebuild.config),
                ) {
                    eprintln!("watch reconfiguration failed: {error:#}");
                    ExitCode::from(2)
                } else {
                    debounce = Duration::from_millis(rebuild.config.config.watch.debounce_ms);
                    exit_code
                }
            }
            Err(error) => {
                eprintln!("watch rebuild failed: {error:#}");
                ExitCode::from(2)
            }
        };
    }
}

#[derive(Debug)]
struct WatchRebuildResult {
    exit_code: ExitCode,
    config: ConfigHandle,
}

fn run_watch_rebuild(
    project: Option<&PathBuf>,
    format: OutputFormat,
    notes: Vec<String>,
) -> Result<WatchRebuildResult> {
    let config = load_project(project)?;
    run_watch_rebuild_with_config(config, format, notes)
}

fn run_watch_rebuild_with_config(
    config: ConfigHandle,
    format: OutputFormat,
    notes: Vec<String>,
) -> Result<WatchRebuildResult> {
    let exit_code = run_build_like_command(&config, format, "watch", notes)?;
    Ok(WatchRebuildResult { exit_code, config })
}

#[derive(Debug, Default, Eq, PartialEq)]
struct WatchTargetUpdatePlan {
    unwatch: Vec<PathBuf>,
    watch: Vec<(PathBuf, RecursiveMode)>,
}

fn plan_watch_target_update(
    active_targets: &BTreeMap<PathBuf, RecursiveMode>,
    desired_targets: &[(PathBuf, RecursiveMode)],
) -> WatchTargetUpdatePlan {
    let desired_targets = desired_targets.iter().cloned().collect::<BTreeMap<_, _>>();
    let mut plan = WatchTargetUpdatePlan::default();

    for (path, active_mode) in active_targets {
        if !matches!(desired_targets.get(path), Some(desired_mode) if desired_mode == active_mode) {
            plan.unwatch.push(path.clone());
        }
    }

    for (path, desired_mode) in desired_targets {
        if !matches!(active_targets.get(&path), Some(active_mode) if active_mode == &desired_mode) {
            plan.watch.push((path, desired_mode));
        }
    }

    plan
}

fn apply_watch_targets(
    watcher: &mut RecommendedWatcher,
    active_targets: &mut BTreeMap<PathBuf, RecursiveMode>,
    desired_targets: Vec<(PathBuf, RecursiveMode)>,
) -> Result<()> {
    let plan = plan_watch_target_update(active_targets, &desired_targets);

    for path in plan.unwatch {
        watcher
            .unwatch(&path)
            .with_context(|| format!("unable to stop watching {}", path.display()))?;
        active_targets.remove(&path);
    }

    for (path, mode) in plan.watch {
        watcher
            .watch(&path, mode)
            .with_context(|| format!("unable to watch {}", path.display()))?;
        active_targets.insert(path, mode);
    }

    Ok(())
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
    typepython_project::resolve_python_executable(config)
}

fn load_project(project: Option<&PathBuf>) -> Result<ConfigHandle> {
    let start = project_start_dir(project)?;

    load(start).context("unable to load TypePython project configuration")
}

fn load_project_without_python_executable_validation(
    project: Option<&PathBuf>,
) -> Result<ConfigHandle> {
    let start = project_start_dir(project)?;

    load_without_python_executable_validation(start)
        .context("unable to load TypePython project configuration")
}

fn project_start_dir(project: Option<&PathBuf>) -> Result<PathBuf> {
    let candidate = match project {
        Some(path) if path.is_absolute() => path.clone(),
        Some(path) => {
            env::current_dir().context("unable to determine current directory")?.join(path)
        }
        None => env::current_dir().context("unable to determine current directory")?,
    };
    let start = if candidate.is_file() {
        candidate.parent().map(Path::to_path_buf).unwrap_or(candidate)
    } else {
        candidate
    };
    Ok(normalize_lexical_path(&start))
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    let mut has_root = false;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => {
                normalized.push(component.as_os_str());
                has_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some_and(|name| name != "..") {
                    normalized.pop();
                } else if !has_root {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
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
                "schema_version": CLI_JSON_SCHEMA_VERSION,
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

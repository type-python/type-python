use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Context, Result};
use serde::Serialize;
use typepython_config::ConfigHandle;
use typepython_diagnostics::DiagnosticReport;
use typepython_emit::{InferredStubMode, generate_inferred_stub_source};
use typepython_syntax::{SourceFile, SourceKind, apply_type_ignore_directives};

use crate::cli::{MigrateArgs, OutputFormat};
use crate::discovery::{
    DiscoveredSource, bundled_stdlib_sources, collect_source_paths, normalize_glob_path,
};
use crate::pipeline::{collect_parse_diagnostics, load_syntax_trees};
use crate::{CommandSummary, exit_code, load_project, print_summary};

#[derive(Debug, Serialize)]
pub(crate) struct MigrationReport {
    pub(crate) total_declarations: usize,
    pub(crate) known_declarations: usize,
    pub(crate) total_dynamic_boundaries: usize,
    pub(crate) total_unknown_boundaries: usize,
    pub(crate) files: Vec<MigrationCoverageEntry>,
    pub(crate) directories: Vec<MigrationCoverageEntry>,
    pub(crate) high_impact_untyped_files: Vec<MigrationImpactEntry>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct MigrationCoverageEntry {
    pub(crate) path: String,
    pub(crate) declarations: usize,
    pub(crate) known_declarations: usize,
    pub(crate) coverage_percent: f64,
    pub(crate) dynamic_boundaries: usize,
    pub(crate) unknown_boundaries: usize,
    pub(crate) source_kind: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MigrationImpactEntry {
    pub(crate) path: String,
    pub(crate) downstream_references: usize,
    pub(crate) untyped_declarations: usize,
    pub(crate) dynamic_boundaries: usize,
    pub(crate) unknown_boundaries: usize,
}

#[derive(Debug, Clone)]
struct MigrationFileStats {
    module_key: String,
    entry: MigrationCoverageEntry,
}

#[derive(Debug, Default, Clone, Copy)]
struct CoverageTally {
    pub(crate) declarations: usize,
    pub(crate) known_declarations: usize,
    pub(crate) dynamic_boundaries: usize,
    pub(crate) unknown_boundaries: usize,
}

pub(crate) fn run_migrate(args: MigrateArgs) -> Result<ExitCode> {
    let config = load_project(args.run.project.as_ref())?;
    let discovery = collect_source_paths(&config)?;
    let mut syntax_trees = load_syntax_trees(
        &discovery.sources,
        config.config.typing.conditional_returns,
        &config.analysis_python().to_string(),
    )?;
    let bundled_sources = bundled_stdlib_sources(&config.analysis_python().to_string())?;
    syntax_trees.extend(load_syntax_trees(
        &bundled_sources,
        config.config.typing.conditional_returns,
        &config.analysis_python().to_string(),
    )?);
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

pub(crate) fn build_migration_report(
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

pub(crate) fn emit_migration_stubs(
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

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use typepython_config::ConfigHandle;
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Severity};
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
    pub(crate) public_api_exports: usize,
    pub(crate) known_public_api_exports: usize,
    pub(crate) files: Vec<MigrationCoverageEntry>,
    pub(crate) directories: Vec<MigrationCoverageEntry>,
    pub(crate) public_api_files: Vec<MigrationPublicApiEntry>,
    pub(crate) untyped_import_files: Vec<MigrationUntypedImportEntry>,
    pub(crate) inline_suppression_files: Vec<MigrationInlineSuppressionEntry>,
    pub(crate) high_impact_untyped_files: Vec<MigrationImpactEntry>,
    pub(crate) framework_pattern_files: Vec<MigrationFrameworkPatternEntry>,
    pub(crate) diagnostic_baseline: Option<MigrationDiagnosticBaselineComparison>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub(crate) struct MigrationDiagnosticBaseline {
    pub(crate) version: u8,
    #[serde(default)]
    pub(crate) severity_overrides: BTreeMap<String, String>,
    pub(crate) diagnostics: Vec<MigrationDiagnosticBaselineEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct MigrationDiagnosticBaselineEntry {
    pub(crate) code: String,
    pub(crate) severity: String,
    pub(crate) path: Option<String>,
    pub(crate) line: Option<usize>,
    pub(crate) column: Option<usize>,
    pub(crate) message: String,
}

#[derive(Debug, Serialize, Clone, Eq, PartialEq)]
pub(crate) struct MigrationDiagnosticBaselineComparison {
    pub(crate) baseline_path: String,
    pub(crate) baseline_diagnostics: usize,
    pub(crate) current_diagnostics: usize,
    pub(crate) blocking_new_diagnostics: usize,
    pub(crate) new_diagnostics: Vec<MigrationDiagnosticBaselineEntry>,
    pub(crate) resolved_diagnostics: Vec<MigrationDiagnosticBaselineEntry>,
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
    pub(crate) downstream_public_importers: usize,
    pub(crate) untyped_declarations: usize,
    pub(crate) dynamic_boundaries: usize,
    pub(crate) unknown_boundaries: usize,
    pub(crate) impact_score: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct MigrationFrameworkPatternEntry {
    pub(crate) path: String,
    pub(crate) frameworks: Vec<String>,
    pub(crate) signals: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MigrationPublicApiEntry {
    pub(crate) path: String,
    pub(crate) public_exports: usize,
    pub(crate) known_public_exports: usize,
    pub(crate) completeness_percent: f64,
    pub(crate) incomplete_exports: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MigrationUntypedImportEntry {
    pub(crate) path: String,
    pub(crate) untyped_import_count: usize,
    pub(crate) imports: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MigrationInlineSuppressionEntry {
    pub(crate) path: String,
    pub(crate) suppression_count: usize,
    pub(crate) directives: Vec<MigrationInlineSuppressionDirective>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MigrationInlineSuppressionDirective {
    pub(crate) line: usize,
    pub(crate) codes: Option<Vec<String>>,
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

    let current_baseline = build_migration_diagnostic_baseline(&diagnostics);
    let baseline_comparison = migration_diagnostic_baseline_comparison(
        &config,
        args.baseline.as_deref(),
        &current_baseline,
    )?;
    if args.no_new_diagnostics {
        match &baseline_comparison {
            Some(comparison) if comparison.new_diagnostics.is_empty() => {}
            Some(comparison) if comparison.blocking_new_diagnostics == 0 => diagnostics.push(
                Diagnostic::warning(
                    "TPY6001",
                    format!(
                        "migration baseline gate found {} new diagnostic(s), all downgraded by severity overrides",
                        comparison.new_diagnostics.len()
                    ),
                )
                .with_note(format!(
                    "baseline {} demoted these diagnostics below error severity",
                    comparison.baseline_path
                )),
            ),
            Some(comparison) => diagnostics.push(
                Diagnostic::error(
                    "TPY6001",
                    format!(
                        "migration baseline gate found {} blocking new diagnostic(s)",
                        comparison.blocking_new_diagnostics
                    ),
                )
                .with_note(format!(
                    "compare against baseline {} or refresh it with `typepython migrate --write-baseline` after intentional debt changes",
                    comparison.baseline_path
                )),
            ),
            None => diagnostics.push(
                Diagnostic::error(
                    "TPY6002",
                    "--no-new-diagnostics requires --baseline PATH so current diagnostics can be compared",
                )
                .with_note("write the initial baseline with `typepython migrate --write-baseline PATH`"),
            ),
        }
    }

    if let Some(path) = args.write_baseline.as_deref() {
        write_migration_diagnostic_baseline(&config, path, &current_baseline)?;
    }

    let mut report = build_migration_report(&config, &syntax_trees);
    report.diagnostic_baseline = baseline_comparison;
    let emitted_stubs = emit_migration_stubs(
        &config,
        &discovery.sources,
        &args.emit_stubs,
        args.stub_out_dir.as_deref(),
    )?;
    let mut notes = Vec::new();
    if args.report {
        notes.push(String::from(
            "migration report includes file coverage, directory coverage, high-impact untyped files, and diagnostic baseline state",
        ));
    }
    if let Some(path) = args.write_baseline.as_deref() {
        notes.push(format!(
            "wrote migration diagnostic baseline to {}",
            resolve_migration_report_path(&config, path).display()
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
    let known_module_keys = syntax_trees
        .iter()
        .map(|syntax| syntax.source.logical_module.clone())
        .collect::<BTreeSet<_>>();
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

    let public_api_by_path = files
        .iter()
        .map(|stats| {
            (
                stats.entry.path.clone(),
                migration_public_api_entry_for_module(
                    stats.module_key.as_str(),
                    &stats.entry.path,
                    syntax_trees,
                )
                .map(|entry| entry.public_exports)
                .unwrap_or(0),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut downstream_reference_counts = BTreeMap::<String, usize>::new();
    let mut downstream_public_importers = BTreeMap::<String, usize>::new();
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
                    if public_api_by_path.get(&stats.entry.path).copied().unwrap_or(0) > 0 {
                        *downstream_public_importers
                            .entry(stats.entry.path.clone())
                            .or_default() += 1;
                    }
                }
            }
        }
    }
    let mut high_impact_untyped_files = files
        .iter()
        .filter(|stats| stats.entry.known_declarations < stats.entry.declarations)
        .map(|stats| {
            let downstream_references =
                downstream_reference_counts.get(&stats.entry.path).copied().unwrap_or(0);
            let downstream_public_importers =
                downstream_public_importers.get(&stats.entry.path).copied().unwrap_or(0);
            let untyped_declarations = stats.entry.declarations - stats.entry.known_declarations;
            let impact_score = downstream_references * 10
                + downstream_public_importers * 25
                + untyped_declarations * 3
                + stats.entry.dynamic_boundaries
                + stats.entry.unknown_boundaries;
            MigrationImpactEntry {
                path: stats.entry.path.clone(),
                downstream_references,
                downstream_public_importers,
                untyped_declarations,
                dynamic_boundaries: stats.entry.dynamic_boundaries,
                unknown_boundaries: stats.entry.unknown_boundaries,
                impact_score,
            }
        })
        .collect::<Vec<_>>();
    high_impact_untyped_files.sort_by(|left, right| {
        right
            .impact_score
            .cmp(&left.impact_score)
            .then_with(|| right.downstream_public_importers.cmp(&left.downstream_public_importers))
            .then_with(|| right.downstream_references.cmp(&left.downstream_references))
            .then_with(|| right.untyped_declarations.cmp(&left.untyped_declarations))
            .then_with(|| left.path.cmp(&right.path))
    });

    let mut public_api_files = syntax_trees
        .iter()
        .map(|syntax| migration_public_api_entry(config, syntax))
        .filter(|entry| entry.public_exports > 0)
        .collect::<Vec<_>>();
    public_api_files.sort_by(|left, right| left.path.cmp(&right.path));
    let public_api_exports = public_api_files.iter().map(|entry| entry.public_exports).sum();
    let known_public_api_exports =
        public_api_files.iter().map(|entry| entry.known_public_exports).sum();
    let mut untyped_import_files = syntax_trees
        .iter()
        .map(|syntax| migration_untyped_import_entry(config, syntax, &known_module_keys))
        .filter(|entry| entry.untyped_import_count > 0)
        .collect::<Vec<_>>();
    untyped_import_files.sort_by(|left, right| left.path.cmp(&right.path));
    let mut inline_suppression_files = syntax_trees
        .iter()
        .map(|syntax| migration_inline_suppression_entry(config, syntax))
        .filter(|entry| entry.suppression_count > 0)
        .collect::<Vec<_>>();
    inline_suppression_files.sort_by(|left, right| left.path.cmp(&right.path));

    MigrationReport {
        total_declarations: total.declarations,
        known_declarations: total.known_declarations,
        total_dynamic_boundaries: total.dynamic_boundaries,
        total_unknown_boundaries: total.unknown_boundaries,
        public_api_exports,
        known_public_api_exports,
        files: files.into_iter().map(|stats| stats.entry).collect(),
        directories: directory_entries,
        public_api_files,
        untyped_import_files,
        inline_suppression_files,
        high_impact_untyped_files,
        framework_pattern_files: framework_pattern_entries(config, syntax_trees),
        diagnostic_baseline: None,
    }
}

fn migration_public_api_entry(
    config: &ConfigHandle,
    syntax: &typepython_syntax::SyntaxTree,
) -> MigrationPublicApiEntry {
    migration_public_api_entry_inner(
        syntax
            .source
            .path
            .strip_prefix(&config.config_dir)
            .map(normalize_glob_path)
            .unwrap_or_else(|_| syntax.source.path.display().to_string()),
        syntax,
    )
}

fn migration_public_api_entry_for_module(
    module_key: &str,
    path: &str,
    syntax_trees: &[typepython_syntax::SyntaxTree],
) -> Option<MigrationPublicApiEntry> {
    syntax_trees
        .iter()
        .find(|syntax| syntax.source.logical_module == module_key)
        .map(|syntax| migration_public_api_entry_inner(path.to_owned(), syntax))
}

fn migration_public_api_entry_inner(
    path: String,
    syntax: &typepython_syntax::SyntaxTree,
) -> MigrationPublicApiEntry {
    let mut public_exports = 0usize;
    let mut known_public_exports = 0usize;
    let mut incomplete_exports = Vec::new();

    for statement in &syntax.statements {
        match statement {
            typepython_syntax::SyntaxStatement::TypeAlias(statement)
                if is_public_name(&statement.name) =>
            {
                public_exports += 1;
                let (dynamic_count, unknown_count) = count_boundary_tokens(&statement.value);
                if !statement.value.is_empty() && dynamic_count == 0 && unknown_count == 0 {
                    known_public_exports += 1;
                } else {
                    incomplete_exports.push(statement.name.clone());
                }
            }
            typepython_syntax::SyntaxStatement::Interface(statement)
            | typepython_syntax::SyntaxStatement::DataClass(statement)
            | typepython_syntax::SyntaxStatement::SealedClass(statement)
            | typepython_syntax::SyntaxStatement::ClassDef(statement)
                if is_public_name(&statement.name) =>
            {
                public_exports += 1;
                let class_known = statement.bases.iter().all(|base| {
                    let (dynamic_count, unknown_count) = count_boundary_tokens(base);
                    dynamic_count == 0 && unknown_count == 0
                });
                if class_known {
                    known_public_exports += 1;
                } else {
                    incomplete_exports.push(statement.name.clone());
                }
            }
            typepython_syntax::SyntaxStatement::OverloadDef(statement)
                if is_public_name(&statement.name) =>
            {
                public_exports += 1;
                let (known, _, _) = function_signature_coverage(
                    &statement.params,
                    statement.returns.as_deref(),
                    false,
                );
                if known {
                    known_public_exports += 1;
                } else {
                    incomplete_exports.push(statement.name.clone());
                }
            }
            typepython_syntax::SyntaxStatement::FunctionDef(statement)
                if is_public_name(&statement.name) =>
            {
                public_exports += 1;
                let (known, _, _) = function_signature_coverage(
                    &statement.params,
                    statement.returns.as_deref(),
                    false,
                );
                if known {
                    known_public_exports += 1;
                } else {
                    incomplete_exports.push(statement.name.clone());
                }
            }
            typepython_syntax::SyntaxStatement::Value(statement) => {
                let (annotation_known, _, _) = known_type_slot(
                    statement.annotation.as_deref().or(statement.rendered_value_type().as_deref()),
                );
                for name in &statement.names {
                    if !is_public_name(name) {
                        continue;
                    }
                    public_exports += 1;
                    if annotation_known {
                        known_public_exports += 1;
                    } else {
                        incomplete_exports.push(name.clone());
                    }
                }
            }
            _ => {}
        }
    }

    incomplete_exports.sort();
    MigrationPublicApiEntry {
        path,
        public_exports,
        known_public_exports,
        completeness_percent: coverage_percent(known_public_exports, public_exports),
        incomplete_exports,
    }
}

fn migration_untyped_import_entry(
    config: &ConfigHandle,
    syntax: &typepython_syntax::SyntaxTree,
    known_module_keys: &BTreeSet<String>,
) -> MigrationUntypedImportEntry {
    let path = syntax.source.path.strip_prefix(&config.config_dir).map(normalize_glob_path).ok();
    let Some(path) = path else {
        return MigrationUntypedImportEntry {
            path: syntax.source.path.display().to_string(),
            untyped_import_count: 0,
            imports: Vec::new(),
        };
    };
    let mut imports = syntax
        .statements
        .iter()
        .filter_map(|statement| match statement {
            typepython_syntax::SyntaxStatement::Import(statement) => Some(statement),
            _ => None,
        })
        .flat_map(|statement| statement.bindings.iter().map(|binding| binding.source_path.clone()))
        .filter(|source_path| !import_resolves_to_known_module(source_path, known_module_keys))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    imports.sort();

    MigrationUntypedImportEntry { path, untyped_import_count: imports.len(), imports }
}

fn migration_inline_suppression_entry(
    config: &ConfigHandle,
    syntax: &typepython_syntax::SyntaxTree,
) -> MigrationInlineSuppressionEntry {
    let directives = syntax
        .type_ignore_directives
        .iter()
        .map(|directive| MigrationInlineSuppressionDirective {
            line: directive.line,
            codes: directive.codes.clone(),
        })
        .collect::<Vec<_>>();
    MigrationInlineSuppressionEntry {
        path: syntax
            .source
            .path
            .strip_prefix(&config.config_dir)
            .map(normalize_glob_path)
            .unwrap_or_else(|_| syntax.source.path.display().to_string()),
        suppression_count: directives.len(),
        directives,
    }
}

fn import_resolves_to_known_module(
    source_path: &str,
    known_module_keys: &BTreeSet<String>,
) -> bool {
    known_module_keys.contains(source_path)
        || known_module_keys.iter().any(|module_key| {
            module_key.starts_with(&format!("{source_path}."))
                || source_path.starts_with(&format!("{module_key}."))
        })
}

fn is_public_name(name: &str) -> bool {
    !name.starts_with('_')
}

fn framework_pattern_entries(
    config: &ConfigHandle,
    syntax_trees: &[typepython_syntax::SyntaxTree],
) -> Vec<MigrationFrameworkPatternEntry> {
    let mut entries = syntax_trees
        .iter()
        .filter_map(|syntax| {
            let signals = framework_pattern_signals(&syntax.source.text);
            if signals.is_empty() {
                return None;
            }
            let frameworks = signals
                .iter()
                .filter_map(|signal| {
                    signal.split_once(':').map(|(framework, _)| framework.to_owned())
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            Some(MigrationFrameworkPatternEntry {
                path: syntax
                    .source
                    .path
                    .strip_prefix(&config.config_dir)
                    .map(normalize_glob_path)
                    .unwrap_or_else(|_| syntax.source.path.display().to_string()),
                frameworks,
                signals,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries
}

fn framework_pattern_signals(source: &str) -> Vec<String> {
    let patterns: &[(&str, &[&str])] = &[
        ("pydantic", &["pydantic", "BaseModel", "Field("]),
        ("fastapi", &["fastapi", "FastAPI", "APIRouter", "@app.", "@router."]),
        ("django", &["django", "models.Model"]),
        ("sqlalchemy", &["sqlalchemy", "Column(", "mapped_column", "DeclarativeBase"]),
        ("celery", &["celery", "@task", ".task("]),
        ("click", &["click", "@click."]),
        ("typer", &["typer", "Typer", "@app.command"]),
    ];
    let mut signals = BTreeSet::new();
    for (framework, needles) in patterns {
        for needle in *needles {
            if source.contains(needle) {
                signals.insert(format!("{framework}:{needle}"));
            }
        }
    }
    signals.into_iter().collect()
}

pub(crate) fn build_migration_diagnostic_baseline(
    diagnostics: &DiagnosticReport,
) -> MigrationDiagnosticBaseline {
    let mut entries =
        diagnostics.diagnostics.iter().map(migration_diagnostic_baseline_entry).collect::<Vec<_>>();
    entries.sort();
    entries.dedup();

    MigrationDiagnosticBaseline {
        version: 2,
        severity_overrides: BTreeMap::new(),
        diagnostics: entries,
    }
}

pub(crate) fn compare_migration_diagnostic_baseline(
    baseline_path: String,
    baseline: &MigrationDiagnosticBaseline,
    current: &MigrationDiagnosticBaseline,
) -> MigrationDiagnosticBaselineComparison {
    let baseline_entries = baseline
        .diagnostics
        .iter()
        .filter_map(|entry| {
            apply_baseline_severity_override(entry.clone(), &baseline.severity_overrides)
        })
        .collect::<BTreeSet<_>>();
    let current_entries = current
        .diagnostics
        .iter()
        .filter_map(|entry| {
            apply_baseline_severity_override(entry.clone(), &baseline.severity_overrides)
        })
        .collect::<BTreeSet<_>>();
    let new_diagnostics: Vec<_> = current_entries.difference(&baseline_entries).cloned().collect();
    let resolved_diagnostics: Vec<_> =
        baseline_entries.difference(&current_entries).cloned().collect();
    let blocking_new_diagnostics = new_diagnostics
        .iter()
        .filter(|entry| entry.severity == Severity::Error.to_string())
        .count();

    MigrationDiagnosticBaselineComparison {
        baseline_path,
        baseline_diagnostics: baseline_entries.len(),
        current_diagnostics: current_entries.len(),
        blocking_new_diagnostics,
        new_diagnostics,
        resolved_diagnostics,
    }
}

fn apply_baseline_severity_override(
    mut entry: MigrationDiagnosticBaselineEntry,
    severity_overrides: &BTreeMap<String, String>,
) -> Option<MigrationDiagnosticBaselineEntry> {
    let Some(override_value) = severity_overrides.get(&entry.code) else {
        return Some(entry);
    };
    match override_value.as_str() {
        "ignore" => None,
        "warning" => {
            entry.severity = Severity::Warning.to_string();
            Some(entry)
        }
        "error" => {
            entry.severity = Severity::Error.to_string();
            Some(entry)
        }
        _ => Some(entry),
    }
}

fn migration_diagnostic_baseline_entry(
    diagnostic: &Diagnostic,
) -> MigrationDiagnosticBaselineEntry {
    MigrationDiagnosticBaselineEntry {
        code: diagnostic.code.clone(),
        severity: diagnostic.severity.to_string(),
        path: diagnostic.span.as_ref().map(|span| span.path.clone()),
        line: diagnostic.span.as_ref().map(|span| span.line),
        column: diagnostic.span.as_ref().map(|span| span.column),
        message: diagnostic.message.clone(),
    }
}

fn migration_diagnostic_baseline_comparison(
    config: &ConfigHandle,
    baseline_path: Option<&Path>,
    current: &MigrationDiagnosticBaseline,
) -> Result<Option<MigrationDiagnosticBaselineComparison>> {
    let Some(path) = baseline_path else {
        return Ok(None);
    };
    let resolved_path = resolve_migration_report_path(config, path);
    let contents = fs::read_to_string(&resolved_path).with_context(|| {
        format!("unable to read migration baseline {}", resolved_path.display())
    })?;
    let baseline: MigrationDiagnosticBaseline =
        serde_json::from_str(&contents).with_context(|| {
            format!("unable to parse migration baseline {}", resolved_path.display())
        })?;
    Ok(Some(compare_migration_diagnostic_baseline(
        resolved_path.display().to_string(),
        &baseline,
        current,
    )))
}

fn write_migration_diagnostic_baseline(
    config: &ConfigHandle,
    path: &Path,
    baseline: &MigrationDiagnosticBaseline,
) -> Result<()> {
    let resolved_path = resolve_migration_report_path(config, path);
    if let Some(parent) = resolved_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("unable to create migration baseline directory {}", parent.display())
        })?;
    }
    let json = serde_json::to_string_pretty(baseline)
        .context("unable to serialize migration diagnostic baseline")?;
    fs::write(&resolved_path, format!("{json}\n"))
        .with_context(|| format!("unable to write migration baseline {}", resolved_path.display()))
}

fn resolve_migration_report_path(config: &ConfigHandle, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { config.config_dir.join(path) }
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
                statement.annotation.as_deref().or(statement.rendered_value_type().as_deref()),
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
        typepython_syntax::ClassMemberKind::Field => known_type_slot(
            member.annotation.as_deref().or(member.rendered_value_type().as_deref()),
        ),
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
            println!(
                "  migration public API completeness: {}/{} known ({:.1}%)",
                report.known_public_api_exports,
                report.public_api_exports,
                coverage_percent(report.known_public_api_exports, report.public_api_exports)
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
                    "    {}: score={}, downstream_refs={}, public_importers={}, untyped={}, dynamic={}, unknown={}",
                    entry.path,
                    entry.impact_score,
                    entry.downstream_references,
                    entry.downstream_public_importers,
                    entry.untyped_declarations,
                    entry.dynamic_boundaries,
                    entry.unknown_boundaries
                );
            }
            println!("  public API completeness by file:");
            for entry in &report.public_api_files {
                println!(
                    "    {}: {}/{} known ({:.1}%), incomplete={}",
                    entry.path,
                    entry.known_public_exports,
                    entry.public_exports,
                    entry.completeness_percent,
                    if entry.incomplete_exports.is_empty() {
                        String::from("-")
                    } else {
                        entry.incomplete_exports.join(",")
                    }
                );
            }
            println!("  untyped import candidates:");
            for entry in &report.untyped_import_files {
                println!(
                    "    {}: {} import(s) -> {}",
                    entry.path,
                    entry.untyped_import_count,
                    entry.imports.join(",")
                );
            }
            println!("  inline suppression directives:");
            for entry in &report.inline_suppression_files {
                let directives = entry
                    .directives
                    .iter()
                    .map(|directive| match &directive.codes {
                        Some(codes) => format!("line {} [{}]", directive.line, codes.join(",")),
                        None => format!("line {} [all]", directive.line),
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                println!(
                    "    {}: {} suppression(s) -> {}",
                    entry.path, entry.suppression_count, directives
                );
            }
            println!("  framework pattern candidates:");
            for entry in &report.framework_pattern_files {
                println!(
                    "    {}: frameworks={}, signals={}",
                    entry.path,
                    entry.frameworks.join(","),
                    entry.signals.join(",")
                );
            }
            if let Some(comparison) = &report.diagnostic_baseline {
                println!("  diagnostic baseline:");
                println!("    baseline: {}", comparison.baseline_path);
                println!("    current diagnostics: {}", comparison.current_diagnostics);
                println!("    baseline diagnostics: {}", comparison.baseline_diagnostics);
                println!("    new diagnostics: {}", comparison.new_diagnostics.len());
                println!("    resolved diagnostics: {}", comparison.resolved_diagnostics.len());
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

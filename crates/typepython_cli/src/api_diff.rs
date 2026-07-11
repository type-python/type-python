use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use ruff_python_ast::{
    Expr, Operator, Stmt,
    token::{Token, TokenKind, Tokens},
    visitor::{self, Visitor},
};
use ruff_python_parser::parse_module;
use serde::Serialize;
use tar::Archive as TarArchive;
use zip::ZipArchive;

use crate::archive::{
    ArchiveMemberPaths, ArchivePathKind, ArchiveReadBudget, BoundedArchiveReader,
    sdist_tar_entry_is_file,
};
use crate::{
    CLI_JSON_SCHEMA_VERSION, CommandSummary,
    cli::{ApiDiffArgs, OutputFormat},
    exit_code, print_summary,
};
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Severity};

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct ApiSurfaceDiffReport {
    pub(crate) old: String,
    pub(crate) new: String,
    pub(crate) added: Vec<ApiSurfaceChange>,
    pub(crate) removed: Vec<ApiSurfaceChange>,
    pub(crate) changed: Vec<ApiSurfaceChange>,
    pub(crate) release_notes: Vec<String>,
    pub(crate) semver_recommendation: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct ApiSurfaceChange {
    pub(crate) module: String,
    pub(crate) symbol: String,
    pub(crate) kind: String,
    pub(crate) old_signature: Option<String>,
    pub(crate) new_signature: Option<String>,
    pub(crate) classification: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct PublicSymbol {
    kind: String,
    signature: String,
    dynamic_positions: Option<CallableDynamicPositions>,
}

impl PublicSymbol {
    fn new(kind: impl Into<String>, signature: String) -> Self {
        Self { kind: kind.into(), signature, dynamic_positions: None }
    }

    fn callable(
        kind: impl Into<String>,
        signature: String,
        dynamic_positions: CallableDynamicPositions,
    ) -> Self {
        Self { kind: kind.into(), signature, dynamic_positions: Some(dynamic_positions) }
    }
}

const TYPING_METADATA_MODULE: &str = "__typing_metadata__";
const PY_TYPED_SYMBOL: &str = "py.typed";

pub(crate) fn run_api_diff(args: ApiDiffArgs) -> Result<ExitCode> {
    let report = diff_api_surfaces(&args.old, &args.new)?;
    let diagnostics = api_surface_diff_diagnostics(&report);

    if args.format == OutputFormat::Json {
        let payload = serde_json::json!({
            "schema_version": CLI_JSON_SCHEMA_VERSION,
            "summary": report,
            "diagnostics": diagnostics,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .context("unable to serialize api-diff summary as JSON")?
        );
    } else {
        print_api_diff_text(&report);
        let summary = CommandSummary {
            command: String::from("api-diff"),
            config_path: String::new(),
            config_source: typepython_config::ConfigSource::TypePythonToml,
            discovered_sources: report.old_symbol_count() + report.new_symbol_count(),
            lowered_modules: 0,
            planned_artifacts: report.added.len() + report.removed.len() + report.changed.len(),
            tracked_modules: report.module_count(),
            notes: vec![String::from("compared public symbols from typed source trees")],
        };
        print_summary(args.format, &summary, &diagnostics)?;
    }
    Ok(exit_code(&diagnostics))
}

pub(crate) fn api_surface_diff_diagnostics(report: &ApiSurfaceDiffReport) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    for change in report.removed.iter().chain(report.changed.iter()) {
        diagnostics.push(Diagnostic {
            code: String::from("TPY7001"),
            severity: if change.classification == "likely type-compatible" {
                Severity::Warning
            } else {
                Severity::Error
            },
            message: format!(
                "public API surface {}: {}.{} ({})",
                change.classification, change.module, change.symbol, change.kind
            ),
            span: None,
            notes: Vec::new(),
            suggestions: Vec::new(),
        });
    }
    for change in &report.added {
        diagnostics.push(Diagnostic {
            code: String::from("TPY7001"),
            severity: Severity::Note,
            message: format!(
                "public API surface added: {}.{} ({})",
                change.module, change.symbol, change.kind
            ),
            span: None,
            notes: Vec::new(),
            suggestions: Vec::new(),
        });
    }
    diagnostics
}

pub(crate) fn diff_api_surfaces(old: &Path, new: &Path) -> Result<ApiSurfaceDiffReport> {
    let old_surface = collect_surface(old)?;
    let new_surface = collect_surface(new)?;
    let mut modules = old_surface.keys().cloned().collect::<BTreeSet<_>>();
    modules.extend(new_surface.keys().cloned());

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for module in modules {
        let old_symbols = old_surface.get(&module).cloned().unwrap_or_default();
        let new_symbols = new_surface.get(&module).cloned().unwrap_or_default();
        let mut symbols = old_symbols.keys().cloned().collect::<BTreeSet<_>>();
        symbols.extend(new_symbols.keys().cloned());
        for symbol in symbols {
            match (old_symbols.get(&symbol), new_symbols.get(&symbol)) {
                (None, Some(new_symbol)) => added.push(ApiSurfaceChange {
                    module: module.clone(),
                    symbol,
                    kind: new_symbol.kind.clone(),
                    old_signature: None,
                    new_signature: Some(new_symbol.signature.clone()),
                    classification: String::from("source-compatible"),
                }),
                (Some(old_symbol), None) => removed.push(ApiSurfaceChange {
                    module: module.clone(),
                    symbol,
                    kind: old_symbol.kind.clone(),
                    old_signature: Some(old_symbol.signature.clone()),
                    new_signature: None,
                    classification: if old_symbol.kind == "metadata" {
                        String::from("runtime-breaking signal")
                    } else {
                        String::from("likely type-breaking")
                    },
                }),
                (Some(old_symbol), Some(new_symbol)) if old_symbol != new_symbol => {
                    changed.push(ApiSurfaceChange {
                        module: module.clone(),
                        symbol,
                        kind: new_symbol.kind.clone(),
                        old_signature: Some(old_symbol.signature.clone()),
                        new_signature: Some(new_symbol.signature.clone()),
                        classification: classify_changed_symbol(old_symbol, new_symbol),
                    });
                }
                _ => {}
            }
        }
    }
    let release_notes = release_note_snippets(&removed, &changed, &added);
    let semver_recommendation = semver_recommendation(&removed, &changed, &added);
    Ok(ApiSurfaceDiffReport {
        old: old.display().to_string(),
        new: new.display().to_string(),
        added,
        removed,
        changed,
        release_notes,
        semver_recommendation,
    })
}

fn classify_changed_symbol(old_symbol: &PublicSymbol, new_symbol: &PublicSymbol) -> String {
    if old_symbol.kind != new_symbol.kind {
        return String::from("unknown risk");
    }
    if old_symbol.kind == "metadata" {
        return String::from("runtime-breaking signal");
    }
    if matches!(old_symbol.kind.as_str(), "function" | "method" | "property")
        && let (Some(old), Some(new)) = (
            old_symbol
                .dynamic_positions
                .clone()
                .or_else(|| callable_dynamic_positions(&old_symbol.signature)),
            new_symbol
                .dynamic_positions
                .clone()
                .or_else(|| callable_dynamic_positions(&new_symbol.signature)),
        )
    {
        let likely_breaking = (old.parameters && !new.parameters) || (!old.returns && new.returns);
        let likely_compatible =
            (!old.parameters && new.parameters) || (old.returns && !new.returns);
        return match (likely_breaking, likely_compatible) {
            (true, false) => String::from("likely type-breaking"),
            (false, true) => String::from("likely type-compatible"),
            _ => String::from("unknown risk"),
        };
    }
    String::from("unknown risk")
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
struct CallableDynamicPositions {
    parameters: bool,
    returns: bool,
}

fn callable_dynamic_positions(signature: &str) -> Option<CallableDynamicPositions> {
    callable_dynamic_positions_by(signature, signature_mentions_dynamic_type)
}

fn callable_dynamic_positions_for_bindings(
    signature: &str,
    bindings: &OverloadBindings,
    fallback: Option<&OverloadBindings>,
) -> Option<CallableDynamicPositions> {
    callable_dynamic_positions_by(signature, |segment| {
        signature_mentions_bound_any(segment, bindings, fallback)
    })
}

fn callable_dynamic_positions_by(
    signature: &str,
    mentions_dynamic: impl Fn(&str) -> bool,
) -> Option<CallableDynamicPositions> {
    let mut positions = CallableDynamicPositions::default();
    let mut found = false;
    for line in signature.lines() {
        let Some(function) = line.find("def ") else {
            continue;
        };
        let open = line[function..].find('(')? + function;
        let close = matching_parenthesis(line, open)?;
        let colon = line.rfind(':')?;
        if colon <= close {
            return None;
        }
        positions.parameters |= mentions_dynamic(&line[open + 1..close]);
        if let Some(arrow) = line[close + 1..colon].find("->") {
            positions.returns |= mentions_dynamic(&line[close + 1 + arrow + 2..colon]);
        }
        found = true;
    }
    found.then_some(positions)
}

fn signature_mentions_bound_any(
    signature: &str,
    bindings: &OverloadBindings,
    fallback: Option<&OverloadBindings>,
) -> bool {
    signature
        .split(|character: char| {
            !(character == '_' || character == '.' || character.is_alphanumeric())
        })
        .filter(|token| !token.is_empty())
        .any(|token| {
            if let Some((root, member)) = token.split_once('.') {
                member == "Any"
                    && bindings.resolve(root, fallback) == Some(OverloadBinding::TypingModule)
            } else {
                bindings.resolve(token, fallback) == Some(OverloadBinding::AnyType)
            }
        })
}

fn matching_parenthesis(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (offset, character) in text[open..].char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            continue;
        }
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn signature_mentions_dynamic_type(signature: &str) -> bool {
    signature
        .split(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
        .any(|token| token == "Any")
}

fn semver_recommendation(
    removed: &[ApiSurfaceChange],
    changed: &[ApiSurfaceChange],
    added: &[ApiSurfaceChange],
) -> String {
    if !removed.is_empty()
        || changed.iter().any(|change| change.classification != "likely type-compatible")
    {
        String::from("major")
    } else if !changed.is_empty() || !added.is_empty() {
        String::from("minor")
    } else {
        String::from("patch")
    }
}

fn release_note_snippets(
    removed: &[ApiSurfaceChange],
    changed: &[ApiSurfaceChange],
    added: &[ApiSurfaceChange],
) -> Vec<String> {
    removed
        .iter()
        .map(release_note_for_removed)
        .chain(changed.iter().map(release_note_for_changed))
        .chain(added.iter().map(release_note_for_added))
        .collect()
}

fn release_note_for_removed(change: &ApiSurfaceChange) -> String {
    if change.kind == "metadata" && is_typing_metadata_module(&change.module) {
        let package = typing_metadata_package(&change.module);
        return format!(
            "Runtime typing metadata changed: `{}` was removed{}; downstream tools may no longer treat the package as typed.",
            change.symbol,
            package.map_or_else(String::new, |package| format!(" for package `{package}`"))
        );
    }
    format!(
        "Breaking type-surface change: removed {} `{}` from module `{}`.",
        change.kind, change.symbol, change.module
    )
}

fn release_note_for_changed(change: &ApiSurfaceChange) -> String {
    if change.kind == "metadata" && is_typing_metadata_module(&change.module) {
        let package = typing_metadata_package(&change.module);
        return format!(
            "Runtime typing metadata changed: `{}`{} changed from `{}` to `{}`.",
            change.symbol,
            package.map_or_else(String::new, |package| format!(" for package `{package}`")),
            change.old_signature.as_deref().unwrap_or("?"),
            change.new_signature.as_deref().unwrap_or("?"),
        );
    }
    format!(
        "Review required: changed {} `{}` in module `{}` from `{}` to `{}`.",
        change.kind,
        change.symbol,
        change.module,
        change.old_signature.as_deref().unwrap_or("?"),
        change.new_signature.as_deref().unwrap_or("?")
    )
}

fn is_typing_metadata_module(module: &str) -> bool {
    module == TYPING_METADATA_MODULE
        || module.strip_prefix(TYPING_METADATA_MODULE).is_some_and(|suffix| suffix.starts_with('.'))
}

fn typing_metadata_package(module: &str) -> Option<&str> {
    module.strip_prefix(TYPING_METADATA_MODULE)?.strip_prefix('.')
}

fn release_note_for_added(change: &ApiSurfaceChange) -> String {
    format!("Added public {} `{}` to module `{}`.", change.kind, change.symbol, change.module)
}

impl ApiSurfaceDiffReport {
    fn module_count(&self) -> usize {
        self.added
            .iter()
            .chain(&self.removed)
            .chain(&self.changed)
            .map(|change| change.module.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    }

    fn old_symbol_count(&self) -> usize {
        self.removed.len() + self.changed.len()
    }

    fn new_symbol_count(&self) -> usize {
        self.added.len() + self.changed.len()
    }
}

fn collect_surface(root: &Path) -> Result<BTreeMap<String, BTreeMap<String, PublicSymbol>>> {
    if is_zip_artifact(root) {
        return collect_zip_surface(root);
    }
    if is_tar_gz_artifact(root) {
        return collect_tar_gz_surface(root);
    }

    let mut files = Vec::new();
    collect_source_surface_files(root, &mut files)?;
    collect_pyi_files(root, &mut files)?;
    files.sort_by(|left, right| {
        surface_source_priority(
            SurfaceSourceKind::from_path(left).unwrap_or(SurfaceSourceKind::Python),
        )
        .cmp(&surface_source_priority(
            SurfaceSourceKind::from_path(right).unwrap_or(SurfaceSourceKind::Python),
        ))
        .then_with(|| left.cmp(right))
    });
    let mut modules = BTreeMap::new();
    let mut module_origins = BTreeMap::new();
    for path in files {
        let module = module_name(root, &path)?;
        let source = fs::read_to_string(&path)
            .with_context(|| format!("unable to read {}", path.display()))?;
        let kind = SurfaceSourceKind::from_path(&path)
            .context("unable to classify public API surface source")?;
        let symbols = public_symbols(&source, kind)
            .with_context(|| format!("unable to parse public API surface {}", path.display()))?;
        insert_surface_module(
            &mut modules,
            &mut module_origins,
            module,
            path.with_extension("").display().to_string(),
            path.display().to_string(),
            symbols,
        )?;
    }
    for (package, signature) in collect_directory_py_typed_markers(root)? {
        insert_py_typed_marker(&mut modules, &package, signature);
    }
    Ok(modules)
}

fn collect_zip_surface(path: &Path) -> Result<BTreeMap<String, BTreeMap<String, PublicSymbol>>> {
    let kind = if path.extension().and_then(|extension| extension.to_str()) == Some("whl") {
        ArchivePathKind::Wheel
    } else {
        ArchivePathKind::Sdist
    };
    let (mut file, mut budget) = ArchiveReadBudget::open(path, kind).map_err(anyhow::Error::msg)?;
    budget.validate_zip_directory(&mut file).map_err(anyhow::Error::msg)?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("unable to read zip artifact {}", path.display()))?;
    let mut modules = BTreeMap::new();
    let mut module_origins = BTreeMap::new();
    let mut typed_roots = Vec::new();
    let mut typed_markers = Vec::new();
    let mut sources = Vec::new();
    let mut member_paths = ArchiveMemberPaths::new(kind);
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).with_context(|| {
            format!("unable to read entry {index} from zip artifact {}", path.display())
        })?;
        let entry_name =
            member_paths.register(file.name_raw(), file.is_dir()).map_err(anyhow::Error::msg)?;
        let declared_bytes = file.size();
        budget.register_member().map_err(anyhow::Error::msg)?;
        if file.is_dir() {
            continue;
        }
        let zip_file_type = file.unix_mode().map(|mode| mode & 0o170000).unwrap_or(0);
        let is_regular_file = zip_file_type == 0 || zip_file_type == 0o100000;
        if is_regular_file && let Some(typed_root) = archive_typed_root(&entry_name) {
            budget
                .register_payload(&entry_name, declared_bytes, Some(file.compressed_size()))
                .map_err(anyhow::Error::msg)?;
            let bytes = budget
                .read_entry(&mut file, &entry_name, declared_bytes)
                .map_err(anyhow::Error::msg)?;
            typed_markers.push((typed_root.clone(), py_typed_signature(&bytes)?));
            typed_roots.push(typed_root);
            continue;
        }
        let Some(kind) = surface_source_kind_from_archive_entry(&entry_name) else {
            continue;
        };
        if entry_name.contains(".dist-info/") {
            continue;
        }
        budget
            .register_payload(&entry_name, declared_bytes, Some(file.compressed_size()))
            .map_err(anyhow::Error::msg)?;
        let bytes = budget
            .read_entry(&mut file, &entry_name, declared_bytes)
            .map_err(anyhow::Error::msg)?;
        let source = String::from_utf8(bytes).with_context(|| {
            format!("typed source entry {entry_name} in {} is not valid UTF-8", path.display())
        })?;
        sources.push((entry_name, source, kind));
    }
    budget.verify_file_unchanged(&archive.into_inner()).map_err(anyhow::Error::msg)?;
    sources.sort_by(|left, right| {
        surface_source_priority(left.2)
            .cmp(&surface_source_priority(right.2))
            .then_with(|| left.0.cmp(&right.0))
    });
    sources.retain(|(entry_name, _, source_kind)| {
        *source_kind == SurfaceSourceKind::Stub
            || archive_entry_is_under_typed_root(entry_name, &typed_roots)
    });
    let strip_sdist_src = kind == ArchivePathKind::Sdist && sdist_sources_use_src_layout(&sources);
    for (typed_root, signature) in typed_markers {
        let package = archive_package_name(&typed_root, kind, strip_sdist_src);
        insert_py_typed_marker(&mut modules, &package, signature);
    }
    for (entry_name, source, source_kind) in sources {
        let module = module_name_from_archive_entry(&entry_name, kind, strip_sdist_src);
        let symbols = public_symbols(&source, source_kind).with_context(|| {
            format!("unable to parse public API surface entry {entry_name} in {}", path.display())
        })?;
        insert_surface_module(
            &mut modules,
            &mut module_origins,
            module,
            Path::new(&entry_name).with_extension("").display().to_string(),
            entry_name,
            symbols,
        )?;
    }
    Ok(modules)
}

fn collect_tar_gz_surface(path: &Path) -> Result<BTreeMap<String, BTreeMap<String, PublicSymbol>>> {
    let kind = ArchivePathKind::Sdist;
    let (file, mut budget) = ArchiveReadBudget::open(path, kind).map_err(anyhow::Error::msg)?;
    let compressed = BoundedArchiveReader::compressed(file, kind);
    let decoder = GzDecoder::new(compressed);
    let decoded = BoundedArchiveReader::tar_stream(decoder, kind);
    let mut archive = TarArchive::new(decoded);
    let mut modules = BTreeMap::new();
    let mut module_origins = BTreeMap::new();
    let mut typed_roots = Vec::new();
    let mut typed_markers = Vec::new();
    let mut sources = Vec::new();
    let mut member_paths = ArchiveMemberPaths::new(ArchivePathKind::Sdist);
    for entry in archive
        .entries()
        .with_context(|| format!("unable to read tar artifact {}", path.display()))?
    {
        let mut entry =
            entry.with_context(|| format!("unable to read tar entry in {}", path.display()))?;
        let entry_type = entry.header().entry_type();
        let raw_path = entry.path_bytes();
        let entry_path = member_paths
            .register(raw_path.as_ref(), entry_type.is_dir())
            .map_err(anyhow::Error::msg)?;
        let is_file =
            sdist_tar_entry_is_file(entry_type, &entry_path).map_err(anyhow::Error::msg)?;
        let declared_bytes = entry.size();
        budget.register_member().map_err(anyhow::Error::msg)?;
        budget.register_payload(&entry_path, declared_bytes, None).map_err(anyhow::Error::msg)?;
        if !is_file {
            continue;
        }
        if let Some(typed_root) = archive_typed_root(&entry_path) {
            let bytes = budget
                .read_entry(&mut entry, &entry_path, declared_bytes)
                .map_err(anyhow::Error::msg)?;
            typed_markers.push((typed_root.clone(), py_typed_signature(&bytes)?));
            typed_roots.push(typed_root);
            continue;
        }
        let Some(kind) = surface_source_kind_from_archive_entry(&entry_path) else {
            continue;
        };
        let bytes = budget
            .read_entry(&mut entry, &entry_path, declared_bytes)
            .map_err(anyhow::Error::msg)?;
        let source = String::from_utf8(bytes).with_context(|| {
            format!("typed source entry {entry_path} in {} is not valid UTF-8", path.display())
        })?;
        sources.push((entry_path, source, kind));
    }
    let compressed = archive.into_inner().into_inner().into_inner();
    budget.verify_file_unchanged(&compressed.into_inner()).map_err(anyhow::Error::msg)?;
    sources.sort_by(|left, right| {
        surface_source_priority(left.2)
            .cmp(&surface_source_priority(right.2))
            .then_with(|| left.0.cmp(&right.0))
    });
    sources.retain(|(entry_path, _, source_kind)| {
        *source_kind == SurfaceSourceKind::Stub
            || archive_entry_is_under_typed_root(entry_path, &typed_roots)
    });
    let strip_sdist_src = sdist_sources_use_src_layout(&sources);
    for (typed_root, signature) in typed_markers {
        let package = archive_package_name(&typed_root, ArchivePathKind::Sdist, strip_sdist_src);
        insert_py_typed_marker(&mut modules, &package, signature);
    }
    for (entry_path, source, source_kind) in sources {
        let module =
            module_name_from_archive_entry(&entry_path, ArchivePathKind::Sdist, strip_sdist_src);
        let symbols = public_symbols(&source, source_kind).with_context(|| {
            format!("unable to parse public API surface entry {entry_path} in {}", path.display())
        })?;
        insert_surface_module(
            &mut modules,
            &mut module_origins,
            module,
            Path::new(&entry_path).with_extension("").display().to_string(),
            entry_path,
            symbols,
        )?;
    }
    Ok(modules)
}

fn insert_surface_module(
    modules: &mut BTreeMap<String, BTreeMap<String, PublicSymbol>>,
    origins: &mut BTreeMap<String, (String, String)>,
    module: String,
    physical_identity: String,
    source_label: String,
    symbols: BTreeMap<String, PublicSymbol>,
) -> Result<()> {
    if let Some((existing_identity, existing_label)) = origins.get(&module)
        && existing_identity != &physical_identity
    {
        anyhow::bail!(
            "conflicting API surface sources `{existing_label}` and `{source_label}` both map to module `{module}`"
        );
    }
    origins.insert(module.clone(), (physical_identity, source_label));
    modules.insert(module, symbols);
    Ok(())
}

fn surface_source_kind_from_archive_entry(entry: &str) -> Option<SurfaceSourceKind> {
    if entry.ends_with(".pyi") {
        Some(SurfaceSourceKind::Stub)
    } else if entry.ends_with(".tpy") {
        Some(SurfaceSourceKind::TypePython)
    } else if entry.ends_with(".py") {
        Some(SurfaceSourceKind::Python)
    } else {
        None
    }
}

fn archive_typed_root(marker: &str) -> Option<String> {
    let (parent, name) = marker.rsplit_once('/').unwrap_or(("", marker));
    if name != "py.typed" || parent.split('/').any(|component| component.ends_with(".dist-info")) {
        return None;
    }
    Some(parent.to_owned())
}

fn archive_entry_is_under_typed_root(entry: &str, roots: &[String]) -> bool {
    roots.iter().any(|root| {
        root.is_empty()
            || entry.strip_prefix(root).is_some_and(|relative| relative.starts_with('/'))
    })
}

fn collect_directory_py_typed_markers(root: &Path) -> Result<Vec<(String, String)>> {
    if root.is_file() {
        if root.file_name().and_then(|name| name.to_str()) != Some("py.typed") {
            return Ok(Vec::new());
        }
        let signature = py_typed_signature(
            &fs::read(root)
                .with_context(|| format!("unable to read package marker {}", root.display()))?,
        )?;
        return Ok(vec![(String::new(), signature)]);
    }
    let mut markers = Vec::new();
    collect_directory_py_typed_markers_from(root, root, &mut markers)?;
    Ok(markers)
}

fn collect_directory_py_typed_markers_from(
    root: &Path,
    directory: &Path,
    markers: &mut Vec<(String, String)>,
) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("unable to read {}", directory.display()))?
    {
        let entry = entry?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("unable to inspect {}", entry.path().display()))?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".dist-info"))
            {
                continue;
            }
            collect_directory_py_typed_markers_from(root, &path, markers)?;
        } else if file_type.is_file()
            && path.file_name().and_then(|name| name.to_str()) == Some("py.typed")
        {
            let parent = path.parent().unwrap_or(root);
            let relative = parent.strip_prefix(root).with_context(|| {
                format!("unable to identify package marker root for {}", path.display())
            })?;
            let package = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join(".");
            let signature =
                py_typed_signature(&fs::read(&path).with_context(|| {
                    format!("unable to read package marker {}", path.display())
                })?)?;
            markers.push((package, signature));
        }
    }
    Ok(())
}

fn insert_py_typed_marker(
    modules: &mut BTreeMap<String, BTreeMap<String, PublicSymbol>>,
    package: &str,
    signature: String,
) {
    let module = if package.is_empty() {
        String::from(TYPING_METADATA_MODULE)
    } else {
        format!("{TYPING_METADATA_MODULE}.{package}")
    };
    modules
        .entry(module)
        .or_default()
        .insert(String::from(PY_TYPED_SYMBOL), PublicSymbol::new("metadata", signature));
}

fn py_typed_signature(contents: &[u8]) -> Result<String> {
    let rendered = std::str::from_utf8(contents).context("py.typed marker is not valid UTF-8")?;
    let mode =
        if rendered.lines().any(|line| line.trim() == "partial") { "partial" } else { "complete" };
    Ok(format!("py.typed: {mode}"))
}

fn is_zip_artifact(path: &Path) -> bool {
    matches!(path.extension().and_then(|ext| ext.to_str()), Some("whl" | "zip"))
}

fn is_tar_gz_artifact(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".sdist")
}

fn sdist_sources_use_src_layout(sources: &[(String, String, SurfaceSourceKind)]) -> bool {
    !sources.is_empty()
        && sources.iter().all(|(entry, _, _)| {
            let mut components = entry.split('/');
            components.next().is_some_and(|root| root.contains('-'))
                && components.next() == Some("src")
                && components.next().is_some()
        })
}

fn archive_package_name(root: &str, kind: ArchivePathKind, strip_sdist_src: bool) -> String {
    let synthetic_init =
        if root.is_empty() { String::from("__init__.pyi") } else { format!("{root}/__init__.pyi") };
    let module = module_name_from_archive_entry(&synthetic_init, kind, strip_sdist_src);
    if module == "__init__" { String::new() } else { module }
}

fn module_name_from_archive_entry(
    entry_name: &str,
    kind: ArchivePathKind,
    strip_sdist_src: bool,
) -> String {
    let mut parts = Path::new(entry_name)
        .with_extension("")
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    match kind {
        ArchivePathKind::Sdist
            if parts.first().is_some_and(|part| part.contains('-')) && parts.len() > 1 =>
        {
            parts.remove(0);
            if strip_sdist_src && parts.first().map(String::as_str) == Some("src") {
                parts.remove(0);
            }
        }
        ArchivePathKind::Wheel => {
            if let Some(package) = parts.first_mut()
                && let Some(runtime_package) = package.strip_suffix("-stubs")
                && !runtime_package.is_empty()
            {
                *package = runtime_package.to_owned();
            }
        }
        ArchivePathKind::Sdist => {}
    }
    if parts.last().map(String::as_str) == Some("__init__") {
        parts.pop();
    }
    if parts.is_empty() { String::from("__init__") } else { parts.join(".") }
}

fn collect_pyi_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if root.is_file() {
        if root.extension().and_then(|ext| ext.to_str()) == Some("pyi") {
            files.push(root.to_owned());
        }
        return Ok(());
    }
    for entry in fs::read_dir(root).with_context(|| format!("unable to read {}", root.display()))? {
        let entry = entry?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("unable to inspect {}", entry.path().display()))?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_pyi_files(&path, files)?;
        } else if file_type.is_file()
            && path.extension().and_then(|ext| ext.to_str()) == Some("pyi")
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(())
}

fn collect_source_surface_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if root.is_file() {
        if matches!(root.extension().and_then(|ext| ext.to_str()), Some("py" | "tpy")) {
            files.push(root.to_owned());
        }
        return Ok(());
    }
    for entry in fs::read_dir(root).with_context(|| format!("unable to read {}", root.display()))? {
        let entry = entry?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("unable to inspect {}", entry.path().display()))?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_source_surface_files(&path, files)?;
        } else if file_type.is_file()
            && matches!(path.extension().and_then(|ext| ext.to_str()), Some("py" | "tpy"))
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(())
}

fn module_name(root: &Path, path: &Path) -> Result<String> {
    let relative = if root.is_file() {
        path.file_name().map(PathBuf::from)
    } else {
        Some(path.strip_prefix(root)?.to_owned())
    }
    .context("unable to compute module name")?;
    let mut parts = relative
        .with_extension("")
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if parts.last().map(String::as_str) == Some("__init__") {
        parts.pop();
    }
    if parts.is_empty() {
        return Ok(String::from("__init__"));
    }
    Ok(parts.join("."))
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SurfaceSourceKind {
    Python,
    Stub,
    TypePython,
}

impl SurfaceSourceKind {
    fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("py") => Some(Self::Python),
            Some("pyi") => Some(Self::Stub),
            Some("tpy") => Some(Self::TypePython),
            _ => None,
        }
    }
}

fn surface_source_priority(kind: SurfaceSourceKind) -> u8 {
    match kind {
        SurfaceSourceKind::Python => 0,
        SurfaceSourceKind::TypePython => 1,
        SurfaceSourceKind::Stub => 2,
    }
}

fn public_symbols(
    source: &str,
    source_kind: SurfaceSourceKind,
) -> Result<BTreeMap<String, PublicSymbol>> {
    if source_kind == SurfaceSourceKind::TypePython {
        return typepython_public_symbols(source);
    }

    let parsed = parse_module(source).context("invalid Python syntax")?;
    let explicit_exports = static_all_names(parsed.suite())?;
    let mut extractor = PythonSurfaceExtractor {
        source,
        tokens: parsed.tokens(),
        explicit_exports: explicit_exports.as_ref(),
        source_kind,
        symbols: BTreeMap::new(),
        overloads: BTreeSet::new(),
        overload_bindings: OverloadBindings::default(),
        grouped_signatures: BTreeMap::new(),
    };
    extractor.extract_module(parsed.suite());

    if let Some(exports) = explicit_exports.as_ref() {
        for name in exports {
            extractor
                .symbols
                .entry(name.clone())
                .or_insert_with(|| PublicSymbol::new("export", format!("__all__: {name}")));
        }
    }

    Ok(extractor.symbols)
}

struct PythonSurfaceExtractor<'a> {
    source: &'a str,
    tokens: &'a Tokens,
    explicit_exports: Option<&'a BTreeSet<String>>,
    source_kind: SurfaceSourceKind,
    symbols: BTreeMap<String, PublicSymbol>,
    overloads: BTreeSet<String>,
    overload_bindings: OverloadBindings,
    grouped_signatures: BTreeMap<String, Vec<String>>,
}

impl PythonSurfaceExtractor<'_> {
    fn extract_module(&mut self, suite: &[Stmt]) {
        for statement in suite {
            match statement {
                Stmt::FunctionDef(function)
                    if self.top_level_name_is_exported(function.name.as_str()) =>
                {
                    self.insert_function(function.name.as_str(), function, "function", None);
                }
                Stmt::ClassDef(class_def)
                    if self.top_level_name_is_exported(class_def.name.as_str()) =>
                {
                    self.insert_class(class_def.name.as_str(), class_def);
                }
                Stmt::Assign(assign) => {
                    let signature = canonical_tokens(
                        self.source,
                        self.tokens.in_range(assign.range),
                        TokenLimit::All,
                    );
                    for name in assign.targets.iter().flat_map(simple_target_names) {
                        if name != "__all__" && self.top_level_name_is_exported(name) {
                            self.insert_value(name, "value", signature.clone());
                        }
                    }
                }
                Stmt::AnnAssign(assign) => {
                    if let Expr::Name(name) = assign.target.as_ref()
                        && name.id.as_str() != "__all__"
                        && self.top_level_name_is_exported(name.id.as_str())
                    {
                        let kind = if annotation_is_type_alias(
                            assign.annotation.as_ref(),
                            &self.overload_bindings,
                            None,
                        ) {
                            "type alias"
                        } else {
                            "value"
                        };
                        self.insert_value(
                            name.id.as_str(),
                            kind,
                            canonical_tokens(
                                self.source,
                                self.tokens.in_range(assign.range),
                                TokenLimit::All,
                            ),
                        );
                    }
                }
                Stmt::TypeAlias(type_alias) => {
                    if let Expr::Name(name) = type_alias.name.as_ref()
                        && self.top_level_name_is_exported(name.id.as_str())
                    {
                        self.insert_value(
                            name.id.as_str(),
                            "type alias",
                            canonical_tokens(
                                self.source,
                                self.tokens.in_range(type_alias.range),
                                TokenLimit::All,
                            ),
                        );
                    }
                }
                Stmt::Import(import) => self.insert_imports(import),
                Stmt::ImportFrom(import) => self.insert_from_imports(import),
                Stmt::If(if_statement) => self.extract_if_statement(if_statement),
                Stmt::Try(try_statement) => self.extract_try_statement(try_statement),
                _ => {}
            }
            self.overload_bindings.record_statement(statement, None);
        }
    }

    fn extract_if_statement(&mut self, statement: &ruff_python_ast::StmtIf) {
        let mut candidates =
            vec![(Some(statement.test.as_ref()), statement.body.as_slice(), String::from("if"))];
        candidates.extend(statement.elif_else_clauses.iter().enumerate().map(|(index, clause)| {
            (
                clause.test.as_ref(),
                clause.body.as_slice(),
                if clause.test.is_some() {
                    format!("elif {}", index + 1)
                } else {
                    String::from("else")
                },
            )
        }));

        let mut branches = Vec::new();
        let mut exhaustive = false;
        for (test, body, label) in candidates {
            match test.and_then(literal_boolean_value) {
                Some(false) => continue,
                Some(true) => {
                    if branches.is_empty() {
                        self.extract_module(body);
                        return;
                    }
                    branches.push((label, self.extract_module_branch(&[body])));
                    exhaustive = true;
                    break;
                }
                None if test.is_none() => {
                    if branches.is_empty() {
                        self.extract_module(body);
                        return;
                    }
                    branches.push((label, self.extract_module_branch(&[body])));
                    exhaustive = true;
                    break;
                }
                None => branches.push((label, self.extract_module_branch(&[body]))),
            }
        }
        if !branches.is_empty() {
            self.merge_conditional_symbols(branches, !exhaustive);
        }
    }

    fn extract_try_statement(&mut self, statement: &ruff_python_ast::StmtTry) {
        if statement.handlers.is_empty() {
            self.extract_module(&statement.body);
            self.extract_module(&statement.orelse);
        } else {
            let mut branches = vec![(
                String::from("try"),
                self.extract_module_branch(&[
                    statement.body.as_slice(),
                    statement.orelse.as_slice(),
                ]),
            )];
            for (index, handler) in statement.handlers.iter().enumerate() {
                let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                branches.push((
                    format!("except {}", index + 1),
                    self.extract_module_branch(&[handler.body.as_slice()]),
                ));
            }
            self.merge_conditional_symbols(branches, false);
        }
        self.extract_module(&statement.finalbody);
    }

    fn extract_module_branch(&self, suites: &[&[Stmt]]) -> BTreeMap<String, PublicSymbol> {
        let mut branch = PythonSurfaceExtractor {
            source: self.source,
            tokens: self.tokens,
            explicit_exports: self.explicit_exports,
            source_kind: self.source_kind,
            symbols: BTreeMap::new(),
            overloads: BTreeSet::new(),
            overload_bindings: self.overload_bindings.clone(),
            grouped_signatures: BTreeMap::new(),
        };
        for suite in suites {
            branch.extract_module(suite);
        }
        branch.symbols
    }

    fn merge_conditional_symbols(
        &mut self,
        branches: Vec<(String, BTreeMap<String, PublicSymbol>)>,
        include_base_fallback: bool,
    ) {
        let base = self.symbols.clone();
        let keys = branches
            .iter()
            .flat_map(|(_, symbols)| symbols.keys().cloned())
            .collect::<BTreeSet<_>>();
        for key in keys {
            let mut variants = branches
                .iter()
                .map(|(label, symbols)| {
                    (label.clone(), symbols.get(&key).or_else(|| base.get(&key)).cloned())
                })
                .collect::<Vec<_>>();
            if include_base_fallback {
                variants.push((String::from("otherwise"), base.get(&key).cloned()));
            }
            let present =
                variants.iter().filter_map(|(_, symbol)| symbol.as_ref()).collect::<Vec<_>>();
            if !present.is_empty()
                && present.len() == variants.len()
                && present.windows(2).all(|pair| pair[0] == pair[1])
            {
                self.symbols.insert(key, (*present[0]).clone());
                continue;
            }
            let kind = present.first().map_or("conditional", |symbol| symbol.kind.as_str());
            let kind = if present.iter().all(|symbol| symbol.kind == kind) {
                kind.to_owned()
            } else {
                String::from("conditional")
            };
            let dynamic_positions = present.iter().try_fold(
                CallableDynamicPositions::default(),
                |mut combined, symbol| {
                    let positions = symbol.dynamic_positions.as_ref()?;
                    combined.parameters |= positions.parameters;
                    combined.returns |= positions.returns;
                    Some(combined)
                },
            );
            let signature = variants
                .into_iter()
                .map(|(label, symbol)| match symbol {
                    Some(symbol) => format!("[{label}]\n{}", symbol.signature),
                    None => format!("[{label}]\n<absent>"),
                })
                .collect::<Vec<_>>()
                .join("\n");
            self.symbols.insert(key, PublicSymbol { kind, signature, dynamic_positions });
        }
    }

    fn insert_class(&mut self, key: &str, class_def: &ruff_python_ast::StmtClassDef) {
        let signature = decorated_header_signature(
            self.source,
            self.tokens,
            &class_def.decorator_list,
            self.tokens.in_range(class_def.range),
        );
        self.symbols.insert(key.to_owned(), PublicSymbol::new("class", signature));

        let mut class_overload_bindings = OverloadBindings::default();
        self.extract_class_suite(key, &class_def.body, &mut class_overload_bindings);
    }

    fn extract_class_suite(
        &mut self,
        key: &str,
        suite: &[Stmt],
        class_overload_bindings: &mut OverloadBindings,
    ) {
        for statement in suite {
            match statement {
                Stmt::FunctionDef(function) if public_member_name(function.name.as_str()) => {
                    let member_key = format!("{key}.{}", function.name.as_str());
                    let kind = if is_property(function) { "property" } else { "method" };
                    self.insert_function(
                        &member_key,
                        function,
                        kind,
                        Some(class_overload_bindings),
                    );
                    let binds_class_receiver = function.decorator_list.iter().any(|decorator| {
                        matches!(
                            class_overload_bindings.expression_binding(
                                &decorator.expression,
                                Some(&self.overload_bindings),
                            ),
                            OverloadBinding::ClassMethodDecorator
                                | OverloadBinding::StaticMethodDecorator
                        )
                    });
                    self.insert_instance_attributes(key, function, binds_class_receiver);
                }
                Stmt::ClassDef(nested) if public_member_name(nested.name.as_str()) => {
                    self.insert_class(&format!("{key}.{}", nested.name.as_str()), nested);
                }
                Stmt::Assign(assign) => {
                    let signature = canonical_tokens(
                        self.source,
                        self.tokens.in_range(assign.range),
                        TokenLimit::All,
                    );
                    for name in assign.targets.iter().flat_map(simple_target_names) {
                        if public_name(name) {
                            self.insert_value(
                                &format!("{key}.{name}"),
                                "attribute",
                                signature.clone(),
                            );
                        }
                    }
                }
                Stmt::AnnAssign(assign) => {
                    if let Expr::Name(name) = assign.target.as_ref()
                        && public_name(name.id.as_str())
                    {
                        let kind = if annotation_is_type_alias(
                            assign.annotation.as_ref(),
                            class_overload_bindings,
                            Some(&self.overload_bindings),
                        ) {
                            "type alias"
                        } else {
                            "attribute"
                        };
                        self.insert_value(
                            &format!("{key}.{}", name.id.as_str()),
                            kind,
                            canonical_tokens(
                                self.source,
                                self.tokens.in_range(assign.range),
                                TokenLimit::All,
                            ),
                        );
                    }
                }
                Stmt::TypeAlias(type_alias) => {
                    if let Expr::Name(name) = type_alias.name.as_ref()
                        && public_member_name(name.id.as_str())
                    {
                        self.insert_value(
                            &format!("{key}.{}", name.id.as_str()),
                            "type alias",
                            canonical_tokens(
                                self.source,
                                self.tokens.in_range(type_alias.range),
                                TokenLimit::All,
                            ),
                        );
                    }
                }
                Stmt::If(if_statement) => {
                    self.extract_class_if_statement(key, if_statement, class_overload_bindings);
                }
                Stmt::Try(try_statement) => {
                    self.extract_class_try_statement(key, try_statement, class_overload_bindings);
                }
                _ => {}
            }
            class_overload_bindings.record_statement(statement, Some(&self.overload_bindings));
        }
    }

    fn extract_class_if_statement(
        &mut self,
        key: &str,
        statement: &ruff_python_ast::StmtIf,
        bindings: &OverloadBindings,
    ) {
        if literal_boolean_value(statement.test.as_ref()) == Some(true) {
            let mut bindings = bindings.clone();
            self.extract_class_suite(key, &statement.body, &mut bindings);
            return;
        }
        let mut branches = Vec::new();
        if literal_boolean_value(statement.test.as_ref()) != Some(false) {
            branches.push((
                String::from("if"),
                self.extract_class_branch(key, &[statement.body.as_slice()], bindings),
            ));
        }
        let mut exhaustive = false;
        for (index, clause) in statement.elif_else_clauses.iter().enumerate() {
            match clause.test.as_ref() {
                None => {
                    if branches.is_empty() {
                        let mut bindings = bindings.clone();
                        self.extract_class_suite(key, &clause.body, &mut bindings);
                        return;
                    }
                    branches.push((
                        String::from("else"),
                        self.extract_class_branch(key, &[clause.body.as_slice()], bindings),
                    ));
                    exhaustive = true;
                    break;
                }
                Some(test) => match literal_boolean_value(test) {
                    Some(false) => continue,
                    Some(true) => {
                        if branches.is_empty() {
                            let mut bindings = bindings.clone();
                            self.extract_class_suite(key, &clause.body, &mut bindings);
                            return;
                        }
                        branches.push((
                            format!("elif {}", index + 1),
                            self.extract_class_branch(key, &[clause.body.as_slice()], bindings),
                        ));
                        exhaustive = true;
                        break;
                    }
                    None => branches.push((
                        format!("elif {}", index + 1),
                        self.extract_class_branch(key, &[clause.body.as_slice()], bindings),
                    )),
                },
            }
        }
        if !branches.is_empty() {
            self.merge_conditional_symbols(branches, !exhaustive);
        }
    }

    fn extract_class_try_statement(
        &mut self,
        key: &str,
        statement: &ruff_python_ast::StmtTry,
        bindings: &OverloadBindings,
    ) {
        if statement.handlers.is_empty() {
            let mut branch_bindings = bindings.clone();
            self.extract_class_suite(key, &statement.body, &mut branch_bindings);
            self.extract_class_suite(key, &statement.orelse, &mut branch_bindings);
        } else {
            let mut branches = vec![(
                String::from("try"),
                self.extract_class_branch(
                    key,
                    &[statement.body.as_slice(), statement.orelse.as_slice()],
                    bindings,
                ),
            )];
            for (index, handler) in statement.handlers.iter().enumerate() {
                let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                branches.push((
                    format!("except {}", index + 1),
                    self.extract_class_branch(key, &[handler.body.as_slice()], bindings),
                ));
            }
            self.merge_conditional_symbols(branches, false);
        }
        let mut final_bindings = bindings.clone();
        self.extract_class_suite(key, &statement.finalbody, &mut final_bindings);
    }

    fn extract_class_branch(
        &self,
        key: &str,
        suites: &[&[Stmt]],
        bindings: &OverloadBindings,
    ) -> BTreeMap<String, PublicSymbol> {
        let mut branch = PythonSurfaceExtractor {
            source: self.source,
            tokens: self.tokens,
            explicit_exports: self.explicit_exports,
            source_kind: self.source_kind,
            symbols: BTreeMap::new(),
            overloads: BTreeSet::new(),
            overload_bindings: self.overload_bindings.clone(),
            grouped_signatures: BTreeMap::new(),
        };
        let mut bindings = bindings.clone();
        for suite in suites {
            branch.extract_class_suite(key, suite, &mut bindings);
        }
        branch.symbols
    }

    fn insert_function(
        &mut self,
        key: &str,
        function: &ruff_python_ast::StmtFunctionDef,
        kind: &str,
        local_overload_bindings: Option<&OverloadBindings>,
    ) {
        let signature = decorated_header_signature(
            self.source,
            self.tokens,
            &function.decorator_list,
            self.tokens.in_range(function.range),
        );
        let dynamic_positions = local_overload_bindings.map_or_else(
            || {
                callable_dynamic_positions_for_bindings(&signature, &self.overload_bindings, None)
                    .unwrap_or_default()
            },
            |local| {
                callable_dynamic_positions_for_bindings(
                    &signature,
                    local,
                    Some(&self.overload_bindings),
                )
                .unwrap_or_default()
            },
        );
        let is_overload = function.decorator_list.iter().any(|decorator| {
            local_overload_bindings.map_or_else(
                || is_overload_decorator(&decorator.expression, &self.overload_bindings, None),
                |local| {
                    is_overload_decorator(
                        &decorator.expression,
                        local,
                        Some(&self.overload_bindings),
                    )
                },
            )
        });
        let is_grouped_property = is_property(function);

        if is_overload || is_grouped_property {
            if is_overload && self.overloads.insert(key.to_owned()) {
                self.grouped_signatures.remove(key);
            }
            let signatures = self.grouped_signatures.entry(key.to_owned()).or_default();
            signatures.push(signature);
            let combined_signature = signatures.join("\n");
            let combined_dynamic_positions = local_overload_bindings.map_or_else(
                || {
                    callable_dynamic_positions_for_bindings(
                        &combined_signature,
                        &self.overload_bindings,
                        None,
                    )
                    .unwrap_or_default()
                },
                |local| {
                    callable_dynamic_positions_for_bindings(
                        &combined_signature,
                        local,
                        Some(&self.overload_bindings),
                    )
                    .unwrap_or_default()
                },
            );
            self.symbols.insert(
                key.to_owned(),
                PublicSymbol::callable(kind, combined_signature, combined_dynamic_positions),
            );
        } else if !self.overloads.contains(key) {
            self.symbols
                .insert(key.to_owned(), PublicSymbol::callable(kind, signature, dynamic_positions));
        }
    }

    fn insert_value(&mut self, key: &str, kind: &str, signature: String) {
        self.symbols.insert(key.to_owned(), PublicSymbol::new(kind, signature));
    }

    fn insert_instance_attributes(
        &mut self,
        class_key: &str,
        function: &ruff_python_ast::StmtFunctionDef,
        binds_class_receiver: bool,
    ) {
        if binds_class_receiver {
            return;
        }
        let Some(receiver) = function.parameters.iter().next().map(|parameter| parameter.name())
        else {
            return;
        };
        let mut collector =
            InstanceAttributeCollector { receiver: receiver.as_str(), assignments: Vec::new() };
        collector.visit_body(&function.body);
        for (name, assignment) in collector.assignments {
            if public_name(name) {
                self.insert_value(
                    &format!("{class_key}.{name}"),
                    "attribute",
                    canonical_tokens(
                        self.source,
                        self.tokens.in_range(assignment.range),
                        TokenLimit::All,
                    ),
                );
            }
        }
    }

    fn insert_imports(&mut self, import: &ruff_python_ast::StmtImport) {
        for alias in &import.names {
            let source_name = alias.name.as_str();
            let local_name = alias.asname.as_ref().map_or_else(
                || source_name.split('.').next().unwrap_or(source_name),
                |name| name.as_str(),
            );
            if self.import_is_exported(local_name, alias.asname.is_some()) {
                let signature = match &alias.asname {
                    Some(alias) => format!("import {source_name} as {}", alias.as_str()),
                    None => format!("import {source_name}"),
                };
                self.insert_value(local_name, "re-export", signature);
            }
        }
    }

    fn insert_from_imports(&mut self, import: &ruff_python_ast::StmtImportFrom) {
        let module = format!(
            "{}{}",
            ".".repeat(import.level as usize),
            import.module.as_ref().map_or("", |module| module.as_str())
        );
        for alias in &import.names {
            let source_name = alias.name.as_str();
            if source_name == "*" {
                continue;
            }
            let local_name =
                alias.asname.as_ref().map_or(source_name, ruff_python_ast::Identifier::as_str);
            if self.import_is_exported(local_name, alias.asname.is_some()) {
                let signature = match &alias.asname {
                    Some(alias) => {
                        format!("from {module} import {source_name} as {}", alias.as_str())
                    }
                    None => format!("from {module} import {source_name}"),
                };
                self.insert_value(local_name, "re-export", signature);
            }
        }
    }

    fn top_level_name_is_exported(&self, name: &str) -> bool {
        self.explicit_exports.map_or_else(|| public_name(name), |names| names.contains(name))
    }

    fn import_is_exported(&self, name: &str, has_explicit_alias: bool) -> bool {
        match self.explicit_exports {
            Some(names) => names.contains(name),
            None if self.source_kind == SurfaceSourceKind::Stub => {
                has_explicit_alias && public_name(name)
            }
            None => public_name(name),
        }
    }
}

struct InstanceAttributeCollector<'a> {
    receiver: &'a str,
    assignments: Vec<(&'a str, &'a ruff_python_ast::StmtAnnAssign)>,
}

impl<'a> Visitor<'a> for InstanceAttributeCollector<'a> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::AnnAssign(assignment) => {
                if let Expr::Attribute(attribute) = assignment.target.as_ref()
                    && matches!(
                        attribute.value.as_ref(),
                        Expr::Name(name) if name.id.as_str() == self.receiver
                    )
                {
                    self.assignments.push((attribute.attr.as_str(), assignment));
                }
                visitor::walk_stmt(self, statement);
            }
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            _ => visitor::walk_stmt(self, statement),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum OverloadBinding {
    Decorator,
    AnyType,
    BuiltinsModule,
    ClassMethodDecorator,
    StaticMethodDecorator,
    TypeAliasMarker,
    TypingModule,
    Other,
}

#[derive(Debug, Clone, Default)]
struct OverloadBindings {
    names: BTreeMap<String, OverloadBinding>,
}

impl OverloadBindings {
    fn resolve(&self, name: &str, fallback: Option<&OverloadBindings>) -> Option<OverloadBinding> {
        self.names
            .get(name)
            .copied()
            .or_else(|| fallback.and_then(|bindings| bindings.names.get(name).copied()))
            .or(match name {
                "classmethod" => Some(OverloadBinding::ClassMethodDecorator),
                "staticmethod" => Some(OverloadBinding::StaticMethodDecorator),
                _ => None,
            })
    }

    fn expression_binding(
        &self,
        expression: &Expr,
        fallback: Option<&OverloadBindings>,
    ) -> OverloadBinding {
        match expression {
            Expr::Name(name) => {
                self.resolve(name.id.as_str(), fallback).unwrap_or(OverloadBinding::Other)
            }
            Expr::Attribute(attribute)
                if attribute.attr.as_str() == "overload"
                    && matches!(
                        attribute.value.as_ref(),
                        Expr::Name(name)
                            if self.resolve(name.id.as_str(), fallback)
                                == Some(OverloadBinding::TypingModule)
                    ) =>
            {
                OverloadBinding::Decorator
            }
            Expr::Attribute(attribute)
                if attribute.attr.as_str() == "Any"
                    && matches!(
                        attribute.value.as_ref(),
                        Expr::Name(name)
                            if self.resolve(name.id.as_str(), fallback)
                                == Some(OverloadBinding::TypingModule)
                    ) =>
            {
                OverloadBinding::AnyType
            }
            Expr::Attribute(attribute)
                if attribute.attr.as_str() == "TypeAlias"
                    && matches!(
                        attribute.value.as_ref(),
                        Expr::Name(name)
                            if self.resolve(name.id.as_str(), fallback)
                                == Some(OverloadBinding::TypingModule)
                    ) =>
            {
                OverloadBinding::TypeAliasMarker
            }
            Expr::Attribute(attribute)
                if matches!(attribute.attr.as_str(), "classmethod" | "staticmethod")
                    && matches!(
                        attribute.value.as_ref(),
                        Expr::Name(name)
                            if self.resolve(name.id.as_str(), fallback)
                                == Some(OverloadBinding::BuiltinsModule)
                    ) =>
            {
                if attribute.attr.as_str() == "classmethod" {
                    OverloadBinding::ClassMethodDecorator
                } else {
                    OverloadBinding::StaticMethodDecorator
                }
            }
            _ => OverloadBinding::Other,
        }
    }

    fn record_statement(&mut self, statement: &Stmt, fallback: Option<&OverloadBindings>) {
        match statement {
            Stmt::Import(import) => {
                for alias in &import.names {
                    let source_name = alias.name.as_str();
                    let local_name = alias.asname.as_ref().map_or_else(
                        || source_name.split('.').next().unwrap_or(source_name),
                        ruff_python_ast::Identifier::as_str,
                    );
                    let binding = match source_name {
                        "builtins" => OverloadBinding::BuiltinsModule,
                        "typing" | "typing_extensions" => OverloadBinding::TypingModule,
                        _ => OverloadBinding::Other,
                    };
                    self.names.insert(local_name.to_owned(), binding);
                }
            }
            Stmt::ImportFrom(import) => {
                let imports_typing = import.level == 0
                    && import.module.as_ref().is_some_and(|module| {
                        matches!(module.as_str(), "typing" | "typing_extensions")
                    });
                let imports_builtins = import.level == 0
                    && import.module.as_ref().is_some_and(|module| module.as_str() == "builtins");
                for alias in &import.names {
                    let source_name = alias.name.as_str();
                    if source_name == "*" {
                        continue;
                    }
                    let local_name = alias
                        .asname
                        .as_ref()
                        .map_or(source_name, ruff_python_ast::Identifier::as_str);
                    let binding = match (imports_typing, source_name) {
                        (true, "overload") => OverloadBinding::Decorator,
                        (true, "Any") => OverloadBinding::AnyType,
                        (true, "TypeAlias") => OverloadBinding::TypeAliasMarker,
                        (false, "classmethod") if imports_builtins => {
                            OverloadBinding::ClassMethodDecorator
                        }
                        (false, "staticmethod") if imports_builtins => {
                            OverloadBinding::StaticMethodDecorator
                        }
                        _ => OverloadBinding::Other,
                    };
                    self.names.insert(local_name.to_owned(), binding);
                }
            }
            Stmt::FunctionDef(function) => {
                self.names.insert(function.name.as_str().to_owned(), OverloadBinding::Other);
            }
            Stmt::ClassDef(class_def) => {
                self.names.insert(class_def.name.as_str().to_owned(), OverloadBinding::Other);
            }
            Stmt::Assign(assign) => {
                let binding = self.expression_binding(assign.value.as_ref(), fallback);
                for target in &assign.targets {
                    if let Expr::Name(name) = target {
                        self.names.insert(name.id.as_str().to_owned(), binding);
                    } else {
                        for name in simple_target_names(target) {
                            self.names.insert(name.to_owned(), OverloadBinding::Other);
                        }
                    }
                }
            }
            Stmt::AnnAssign(assign) => {
                if let Expr::Name(name) = assign.target.as_ref() {
                    let binding = assign.value.as_ref().map_or(OverloadBinding::Other, |value| {
                        self.expression_binding(value, fallback)
                    });
                    self.names.insert(name.id.as_str().to_owned(), binding);
                }
            }
            Stmt::AugAssign(assign) => {
                if let Expr::Name(name) = assign.target.as_ref() {
                    self.names.insert(name.id.as_str().to_owned(), OverloadBinding::Other);
                }
            }
            Stmt::TypeAlias(type_alias) => {
                if let Expr::Name(name) = type_alias.name.as_ref() {
                    self.names.insert(name.id.as_str().to_owned(), OverloadBinding::Other);
                }
            }
            Stmt::Delete(delete) => {
                for target in &delete.targets {
                    for name in simple_target_names(target) {
                        self.names.remove(name);
                    }
                }
            }
            _ => {}
        }
    }
}

fn literal_boolean_value(expression: &Expr) -> Option<bool> {
    match expression {
        Expr::BooleanLiteral(boolean) => Some(boolean.value),
        _ => None,
    }
}

fn is_overload_decorator(
    decorator: &Expr,
    bindings: &OverloadBindings,
    fallback: Option<&OverloadBindings>,
) -> bool {
    match decorator {
        Expr::Name(name) => {
            bindings.resolve(name.id.as_str(), fallback) == Some(OverloadBinding::Decorator)
        }
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "overload"
                && matches!(
                    attribute.value.as_ref(),
                    Expr::Name(name)
                        if bindings.resolve(name.id.as_str(), fallback)
                            == Some(OverloadBinding::TypingModule)
                )
        }
        Expr::Call(call) => is_overload_decorator(call.func.as_ref(), bindings, fallback),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum TokenLimit {
    All,
    Header,
}

fn decorated_header_signature(
    source: &str,
    tokens: &Tokens,
    decorators: &[ruff_python_ast::Decorator],
    header_and_body_tokens: &[Token],
) -> String {
    decorators
        .iter()
        .map(|decorator| {
            canonical_tokens(source, tokens.in_range(decorator.range), TokenLimit::All)
        })
        .chain(std::iter::once(canonical_tokens(
            source,
            declaration_header_tokens(header_and_body_tokens),
            TokenLimit::Header,
        )))
        .collect::<Vec<_>>()
        .join("\n")
}

fn declaration_header_tokens(tokens: &[Token]) -> &[Token] {
    let Some(declaration) =
        tokens.iter().position(|token| matches!(token.kind(), TokenKind::Def | TokenKind::Class))
    else {
        return tokens;
    };
    let start = tokens[..declaration]
        .iter()
        .rposition(|token| !token_is_trivia(token.kind()))
        .filter(|index| tokens[*index].kind() == TokenKind::Async)
        .unwrap_or(declaration);
    &tokens[start..]
}

fn canonical_tokens(source: &str, tokens: &[Token], limit: TokenLimit) -> String {
    let end = if limit == TokenLimit::Header { header_token_end(tokens) } else { tokens.len() };
    let significant =
        tokens[..end].iter().filter(|token| !token_is_trivia(token.kind())).collect::<Vec<_>>();
    let mut output = String::new();
    let mut previous = None;
    let mut delimiter_stack = Vec::<bool>::new();
    let mut saw_declaration_keyword = false;
    let mut saw_declaration_name = false;
    let mut saw_parameter_list = false;

    for (index, token) in significant.iter().enumerate() {
        let kind = token.kind();
        if matches!(kind, TokenKind::Def | TokenKind::Class) {
            saw_declaration_keyword = true;
        } else if saw_declaration_keyword && !saw_declaration_name && kind == TokenKind::Name {
            saw_declaration_name = true;
        }
        if kind == TokenKind::Comma
            && significant
                .get(index + 1)
                .is_some_and(|next| token_is_closing_delimiter(next.kind()))
            && limit == TokenLimit::Header
            && delimiter_stack.last() == Some(&true)
        {
            continue;
        }
        let text = token_text(source, token);
        if !output.is_empty() && token_needs_leading_space(previous, kind) {
            output.push(' ');
        }
        output.push_str(text);
        previous = Some(kind);

        if matches!(kind, TokenKind::Lpar | TokenKind::Lsqb | TokenKind::Lbrace) {
            let is_declaration_list = limit == TokenLimit::Header
                && saw_declaration_name
                && delimiter_stack.is_empty()
                && !saw_parameter_list
                && matches!(kind, TokenKind::Lpar | TokenKind::Lsqb);
            if is_declaration_list && kind == TokenKind::Lpar {
                saw_parameter_list = true;
            }
            delimiter_stack.push(is_declaration_list);
        } else if token_is_closing_delimiter(kind) {
            delimiter_stack.pop();
        }
    }
    output
}

fn header_token_end(tokens: &[Token]) -> usize {
    let mut nesting = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind() {
            TokenKind::Lpar | TokenKind::Lsqb | TokenKind::Lbrace => nesting += 1,
            TokenKind::Rpar | TokenKind::Rsqb | TokenKind::Rbrace => {
                nesting = nesting.saturating_sub(1);
            }
            TokenKind::Colon if nesting == 0 => return index + 1,
            _ => {}
        }
    }
    tokens.len()
}

fn token_text<'a>(source: &'a str, token: &Token) -> &'a str {
    let (_, range) = token.as_tuple();
    &source[range.start().to_usize()..range.end().to_usize()]
}

fn token_is_trivia(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Comment
            | TokenKind::Newline
            | TokenKind::NonLogicalNewline
            | TokenKind::Indent
            | TokenKind::Dedent
            | TokenKind::EndOfFile
    )
}

fn token_is_closing_delimiter(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Rpar | TokenKind::Rsqb | TokenKind::Rbrace)
}

fn token_needs_leading_space(previous: Option<TokenKind>, current: TokenKind) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    if matches!(
        current,
        TokenKind::Rpar
            | TokenKind::Rsqb
            | TokenKind::Rbrace
            | TokenKind::Comma
            | TokenKind::Colon
            | TokenKind::Dot
            | TokenKind::Semi
    ) || matches!(
        previous,
        TokenKind::Lpar | TokenKind::Lsqb | TokenKind::Lbrace | TokenKind::Dot
    ) {
        return false;
    }
    if matches!(current, TokenKind::Lpar | TokenKind::Lsqb)
        && matches!(
            previous,
            TokenKind::Name
                | TokenKind::Rpar
                | TokenKind::Rsqb
                | TokenKind::Rbrace
                | TokenKind::String
        )
    {
        return false;
    }
    if matches!(previous, TokenKind::At | TokenKind::Star | TokenKind::DoubleStar) {
        return false;
    }
    if matches!(current, TokenKind::Star | TokenKind::DoubleStar)
        && matches!(previous, TokenKind::Lpar | TokenKind::Lsqb | TokenKind::Comma)
    {
        return false;
    }
    true
}

fn simple_target_names(target: &Expr) -> Vec<&str> {
    match target {
        Expr::Name(name) => vec![name.id.as_str()],
        Expr::Tuple(tuple) => tuple.elts.iter().flat_map(simple_target_names).collect(),
        Expr::List(list) => list.elts.iter().flat_map(simple_target_names).collect(),
        _ => Vec::new(),
    }
}

fn annotation_is_type_alias(
    annotation: &Expr,
    bindings: &OverloadBindings,
    fallback: Option<&OverloadBindings>,
) -> bool {
    bindings.expression_binding(annotation, fallback) == OverloadBinding::TypeAliasMarker
}

fn decorator_name(decorator: &Expr) -> Option<&str> {
    match decorator {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => Some(attribute.attr.as_str()),
        Expr::Call(call) => decorator_name(call.func.as_ref()),
        _ => None,
    }
}

fn is_property(function: &ruff_python_ast::StmtFunctionDef) -> bool {
    function.decorator_list.iter().any(|decorator| {
        decorator_name(&decorator.expression)
            .is_some_and(|name| matches!(name, "property" | "setter" | "deleter" | "getter"))
    })
}

fn static_all_names(suite: &[Stmt]) -> Result<Option<BTreeSet<String>>> {
    let mut known_sequences = BTreeMap::<String, Vec<String>>::new();
    let mut exports = None::<Vec<String>>;
    let mut has_all = false;

    for statement in suite {
        match statement {
            Stmt::Assign(assign) => {
                let resolved = resolve_string_sequence(assign.value.as_ref(), &known_sequences);
                for name in assign.targets.iter().flat_map(simple_target_names) {
                    if let Some(values) = &resolved {
                        known_sequences.insert(name.to_owned(), values.clone());
                    } else {
                        known_sequences.remove(name);
                    }
                    if name == "__all__" {
                        has_all = true;
                        exports = resolved.clone();
                    }
                }
            }
            Stmt::AnnAssign(assign) => {
                if let Expr::Name(name) = assign.target.as_ref() {
                    let resolved = assign
                        .value
                        .as_deref()
                        .and_then(|value| resolve_string_sequence(value, &known_sequences));
                    if let Some(values) = &resolved {
                        known_sequences.insert(name.id.as_str().to_owned(), values.clone());
                    } else {
                        known_sequences.remove(name.id.as_str());
                    }
                    if name.id.as_str() == "__all__" {
                        has_all = true;
                        exports = resolved;
                    }
                }
            }
            Stmt::AugAssign(assign)
                if matches!(assign.op, Operator::Add)
                    && matches!(assign.target.as_ref(), Expr::Name(name) if name.id.as_str() == "__all__") =>
            {
                has_all = true;
                if let (Some(current), Some(mut additional)) = (
                    exports.as_mut(),
                    resolve_string_sequence(assign.value.as_ref(), &known_sequences),
                ) {
                    current.append(&mut additional);
                    known_sequences.insert(String::from("__all__"), current.clone());
                } else {
                    exports = None;
                    known_sequences.remove("__all__");
                }
            }
            Stmt::Expr(expression) => {
                let Expr::Call(call) = expression.value.as_ref() else {
                    continue;
                };
                let Expr::Attribute(attribute) = call.func.as_ref() else {
                    continue;
                };
                let Expr::Name(receiver) = attribute.value.as_ref() else {
                    continue;
                };
                let receiver = receiver.id.as_str();
                if !matches!(attribute.attr.as_str(), "append" | "extend") {
                    continue;
                }
                let updated =
                    if call.arguments.args.len() == 1 && call.arguments.keywords.is_empty() {
                        known_sequences.get(receiver).cloned().and_then(|mut current| {
                            match attribute.attr.as_str() {
                                "append" => {
                                    let Expr::StringLiteral(value) = &call.arguments.args[0] else {
                                        return None;
                                    };
                                    current.push(value.value.to_str().to_owned());
                                }
                                "extend" => current.extend(resolve_string_sequence(
                                    &call.arguments.args[0],
                                    &known_sequences,
                                )?),
                                _ => return None,
                            }
                            Some(current)
                        })
                    } else {
                        None
                    };
                if let Some(values) = &updated {
                    known_sequences.insert(receiver.to_owned(), values.clone());
                } else {
                    known_sequences.remove(receiver);
                }
                if receiver == "__all__" {
                    has_all = true;
                    exports = updated;
                }
            }
            Stmt::Delete(delete) => {
                for name in delete.targets.iter().flat_map(simple_target_names) {
                    known_sequences.remove(name);
                    if name == "__all__" {
                        has_all = false;
                        exports = None;
                    }
                }
            }
            _ => {}
        }
    }

    if !has_all {
        return Ok(None);
    }
    exports
        .map(|names| Some(names.into_iter().collect()))
        .ok_or_else(|| anyhow::anyhow!("unable to statically resolve module `__all__`"))
}

fn resolve_string_sequence(
    expression: &Expr,
    known_sequences: &BTreeMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    match expression {
        Expr::List(list) => resolve_string_elements(&list.elts, known_sequences),
        Expr::Tuple(tuple) => resolve_string_elements(&tuple.elts, known_sequences),
        Expr::Set(set) => resolve_string_elements(&set.elts, known_sequences),
        Expr::Name(name) => known_sequences.get(name.id.as_str()).cloned(),
        Expr::BinOp(binary) if matches!(binary.op, Operator::Add) => {
            let mut left = resolve_string_sequence(binary.left.as_ref(), known_sequences)?;
            left.extend(resolve_string_sequence(binary.right.as_ref(), known_sequences)?);
            Some(left)
        }
        _ => None,
    }
}

fn resolve_string_elements(
    elements: &[Expr],
    known_sequences: &BTreeMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    let mut values = Vec::new();
    for element in elements {
        match element {
            Expr::StringLiteral(string) => values.push(string.value.to_str().to_owned()),
            Expr::Starred(starred) => {
                values.extend(resolve_string_sequence(starred.value.as_ref(), known_sequences)?);
            }
            _ => return None,
        }
    }
    Some(values)
}

fn typepython_public_symbols(source: &str) -> Result<BTreeMap<String, PublicSymbol>> {
    let syntax = typepython_syntax::parse(typepython_syntax::SourceFile {
        path: PathBuf::from("api-surface.tpy"),
        kind: typepython_syntax::SourceKind::TypePython,
        logical_module: String::new(),
        text: source.to_owned(),
    });
    if syntax.diagnostics.has_errors() {
        anyhow::bail!("invalid TypePython syntax");
    }

    let explicit_exports = typepython_static_all_names(source, &syntax.statements)?;
    let mut symbols = BTreeMap::new();
    let mut overloads = BTreeMap::<String, Vec<String>>::new();
    for statement in &syntax.statements {
        match statement {
            typepython_syntax::SyntaxStatement::TypeAlias(alias)
                if typepython_name_is_exported(&alias.name, explicit_exports.as_ref()) =>
            {
                symbols.insert(
                    alias.name.clone(),
                    PublicSymbol::new(
                        "type alias",
                        format!(
                            "typealias {}{} = {}",
                            alias.name,
                            render_typepython_type_params(&alias.type_params),
                            alias.value
                        ),
                    ),
                );
            }
            typepython_syntax::SyntaxStatement::FunctionDef(function)
                if typepython_name_is_exported(&function.name, explicit_exports.as_ref()) =>
            {
                if !overloads.contains_key(&function.name) {
                    symbols.insert(
                        function.name.clone(),
                        typepython_callable_symbol(
                            "function",
                            render_typepython_function(function),
                        ),
                    );
                }
            }
            typepython_syntax::SyntaxStatement::OverloadDef(function)
                if typepython_name_is_exported(&function.name, explicit_exports.as_ref()) =>
            {
                let signatures = overloads.entry(function.name.clone()).or_default();
                signatures.push(format!("@overload\n{}", render_typepython_function(function)));
                symbols.insert(
                    function.name.clone(),
                    typepython_callable_symbol("function", signatures.join("\n")),
                );
            }
            typepython_syntax::SyntaxStatement::ClassDef(class_def)
                if typepython_name_is_exported(&class_def.name, explicit_exports.as_ref()) =>
            {
                insert_typepython_class_symbols(&mut symbols, class_def, "class");
            }
            typepython_syntax::SyntaxStatement::DataClass(class_def)
                if typepython_name_is_exported(&class_def.name, explicit_exports.as_ref()) =>
            {
                insert_typepython_class_symbols(&mut symbols, class_def, "data class");
            }
            typepython_syntax::SyntaxStatement::Interface(class_def)
                if typepython_name_is_exported(&class_def.name, explicit_exports.as_ref()) =>
            {
                insert_typepython_class_symbols(&mut symbols, class_def, "interface");
            }
            typepython_syntax::SyntaxStatement::SealedClass(class_def)
                if typepython_name_is_exported(&class_def.name, explicit_exports.as_ref()) =>
            {
                insert_typepython_class_symbols(&mut symbols, class_def, "sealed class");
            }
            typepython_syntax::SyntaxStatement::Import(import) => {
                for binding in &import.bindings {
                    if typepython_name_is_exported(&binding.local_name, explicit_exports.as_ref()) {
                        symbols.insert(
                            binding.local_name.clone(),
                            PublicSymbol::new(
                                "re-export",
                                format!("import {} as {}", binding.source_path, binding.local_name),
                            ),
                        );
                    }
                }
            }
            typepython_syntax::SyntaxStatement::Value(value) if value.owner_name.is_none() => {
                for name in &value.names {
                    if name != "__all__"
                        && typepython_name_is_exported(name, explicit_exports.as_ref())
                    {
                        let detail = value
                            .annotation_expr
                            .as_ref()
                            .map(typepython_syntax::TypeExpr::render)
                            .or_else(|| value.annotation.clone())
                            .or_else(|| value.rendered_value_type())
                            .unwrap_or_else(|| String::from("unknown"));
                        symbols.insert(
                            name.clone(),
                            PublicSymbol::new("value", format!("{name}: {detail}")),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(exports) = explicit_exports {
        for name in exports {
            symbols
                .entry(name.clone())
                .or_insert_with(|| PublicSymbol::new("export", format!("__all__: {name}")));
        }
    }
    Ok(symbols)
}

fn typepython_callable_symbol(kind: &str, signature: String) -> PublicSymbol {
    let dynamic_positions = callable_dynamic_positions(&signature).unwrap_or_default();
    PublicSymbol::callable(kind, signature, dynamic_positions)
}

fn typepython_name_is_exported(name: &str, exports: Option<&BTreeSet<String>>) -> bool {
    exports.map_or_else(|| public_name(name), |exports| exports.contains(name))
}

fn typepython_static_all_names(
    source: &str,
    statements: &[typepython_syntax::SyntaxStatement],
) -> Result<Option<BTreeSet<String>>> {
    if let Ok(parsed) = parse_module(source) {
        return static_all_names(parsed.suite());
    }

    let lines = source.lines().collect::<Vec<_>>();
    let mut saw_all = false;
    let mut exports = None;
    for statement in statements {
        let typepython_syntax::SyntaxStatement::Value(value) = statement else {
            continue;
        };
        if value.owner_name.is_some() || !value.names.iter().any(|name| name == "__all__") {
            continue;
        }
        saw_all = true;
        let start = value.line.saturating_sub(1);
        let mut candidate = String::new();
        let mut resolved = None;
        for line in lines.iter().skip(start) {
            if !candidate.is_empty() {
                candidate.push('\n');
            }
            candidate.push_str(line);
            let Ok(parsed) = parse_module(&candidate) else {
                continue;
            };
            resolved = static_all_names(parsed.suite())?;
            break;
        }
        exports = resolved;
    }
    if saw_all {
        exports
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("unable to statically resolve TypePython `__all__`"))
    } else {
        Ok(None)
    }
}

fn insert_typepython_class_symbols(
    symbols: &mut BTreeMap<String, PublicSymbol>,
    class_def: &typepython_syntax::NamedBlockStatement,
    declaration_kind: &str,
) {
    symbols.insert(
        class_def.name.clone(),
        PublicSymbol::new(
            "class",
            format!(
                "{declaration_kind} {}{}{}:",
                class_def.name,
                render_typepython_type_params(&class_def.type_params),
                class_def.header_suffix
            ),
        ),
    );
    let mut overloads = BTreeMap::<String, Vec<String>>::new();
    let mut properties = BTreeMap::<String, Vec<String>>::new();
    for member in &class_def.members {
        if !public_member_name(&member.name) {
            continue;
        }
        let key = format!("{}.{}", class_def.name, member.name);
        match member.kind {
            typepython_syntax::ClassMemberKind::Field => {
                let detail = member
                    .rendered_annotation()
                    .or_else(|| member.rendered_value_type())
                    .unwrap_or_else(|| String::from("unknown"));
                symbols.insert(
                    key,
                    PublicSymbol::new("attribute", format!("{}: {detail}", member.name)),
                );
            }
            typepython_syntax::ClassMemberKind::Method => {
                if overloads.contains_key(&key) {
                    continue;
                }
                let signature = render_typepython_member(member);
                if matches!(
                    member.method_kind,
                    Some(
                        typepython_syntax::MethodKind::Property
                            | typepython_syntax::MethodKind::PropertySetter
                    )
                ) {
                    let signatures = properties.entry(key.clone()).or_default();
                    signatures.push(signature);
                    symbols
                        .insert(key, typepython_callable_symbol("property", signatures.join("\n")));
                } else {
                    symbols.insert(key, typepython_callable_symbol("method", signature));
                }
            }
            typepython_syntax::ClassMemberKind::Overload => {
                let signatures = overloads.entry(key.clone()).or_default();
                signatures.push(format!("@overload\n{}", render_typepython_member(member)));
                symbols.insert(key, typepython_callable_symbol("method", signatures.join("\n")));
            }
        }
    }
}

fn render_typepython_function(function: &typepython_syntax::FunctionStatement) -> String {
    format!(
        "{}def {}{}({}){}:",
        if function.is_async { "async " } else { "" },
        function.name,
        render_typepython_type_params(&function.type_params),
        render_typepython_params(&function.params),
        function
            .returns_expr
            .as_ref()
            .map(typepython_syntax::TypeExpr::render)
            .or_else(|| function.returns.clone())
            .map_or_else(String::new, |returns| format!(" -> {returns}")),
    )
}

fn render_typepython_member(member: &typepython_syntax::ClassMember) -> String {
    let decorator = match member.method_kind {
        Some(typepython_syntax::MethodKind::Class) => "@classmethod\n".to_owned(),
        Some(typepython_syntax::MethodKind::Static) => "@staticmethod\n".to_owned(),
        Some(typepython_syntax::MethodKind::Property) => "@property\n".to_owned(),
        Some(typepython_syntax::MethodKind::PropertySetter) => {
            format!("@{}.setter\n", member.name)
        }
        Some(typepython_syntax::MethodKind::Instance) | None => String::new(),
    };
    format!(
        "{decorator}{}def {}{}({}){}:",
        if member.is_async { "async " } else { "" },
        member.name,
        render_typepython_type_params(&member.type_params),
        render_typepython_params(&member.params),
        member.rendered_returns().map_or_else(String::new, |returns| format!(" -> {returns}")),
    )
}

fn render_typepython_params(params: &[typepython_syntax::FunctionParam]) -> String {
    params
        .iter()
        .map(|param| {
            let prefix = if param.keyword_variadic {
                "**"
            } else if param.variadic {
                "*"
            } else {
                ""
            };
            let annotation = param
                .rendered_annotation()
                .map_or_else(String::new, |annotation| format!(": {annotation}"));
            let default = if param.has_default { " = ..." } else { "" };
            format!("{prefix}{}{annotation}{default}", param.name)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_typepython_type_params(params: &[typepython_syntax::TypeParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    format!(
        "[{}]",
        params
            .iter()
            .map(|param| {
                let prefix = match param.kind {
                    typepython_syntax::TypeParamKind::TypeVar => "",
                    typepython_syntax::TypeParamKind::ParamSpec => "**",
                    typepython_syntax::TypeParamKind::TypeVarTuple => "*",
                };
                let constraint = if !param.rendered_constraints().is_empty() {
                    format!(": ({})", param.rendered_constraints().join(", "))
                } else {
                    param.rendered_bound().map_or_else(String::new, |bound| format!(": {bound}"))
                };
                let default = param
                    .rendered_default()
                    .map_or_else(String::new, |default| format!(" = {default}"));
                format!("{prefix}{}{constraint}{default}", param.name)
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn public_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(|first| unicode_ident::is_xid_start(first) && first != '_')
        && characters.all(unicode_ident::is_xid_continue)
}

fn public_member_name(name: &str) -> bool {
    public_name(name) || (name.starts_with("__") && name.ends_with("__") && name.len() > 4)
}

fn print_api_diff_text(report: &ApiSurfaceDiffReport) {
    println!("api-diff:");
    println!("  old: {}", report.old);
    println!("  new: {}", report.new);
    println!("  semver recommendation: {}", report.semver_recommendation);
    for change in &report.removed {
        println!("  removed: {}.{} ({})", change.module, change.symbol, change.kind);
    }
    for change in &report.changed {
        println!("  changed: {}.{} ({})", change.module, change.symbol, change.kind);
    }
    for change in &report.added {
        println!("  added: {}.{} ({})", change.module, change.symbol, change.kind);
    }
    if !report.release_notes.is_empty() {
        println!("  release notes:");
        for note in &report.release_notes {
            println!("    - {note}");
        }
    }
}

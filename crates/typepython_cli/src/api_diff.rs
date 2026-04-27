use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use serde::Serialize;
use tar::Archive as TarArchive;
use zip::ZipArchive;

use crate::{
    CommandSummary,
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
}

const TYPING_METADATA_MODULE: &str = "__typing_metadata__";
const PY_TYPED_SYMBOL: &str = "py.typed";

pub(crate) fn run_api_diff(args: ApiDiffArgs) -> Result<ExitCode> {
    let report = diff_api_surfaces(&args.old, &args.new)?;
    let mut diagnostics = DiagnosticReport::default();
    for change in report.removed.iter().chain(report.changed.iter()) {
        diagnostics.push(Diagnostic {
            code: String::from("TPY7001"),
            severity: Severity::Error,
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

    if args.format == OutputFormat::Json {
        let payload = serde_json::json!({
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
            notes: vec![String::from("compared public symbols from .pyi trees")],
        };
        print_summary(args.format, &summary, &diagnostics)?;
    }
    Ok(exit_code(&diagnostics))
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
                        classification: String::from("unknown risk"),
                    });
                }
                _ => {}
            }
        }
    }
    Ok(ApiSurfaceDiffReport {
        old: old.display().to_string(),
        new: new.display().to_string(),
        added,
        removed,
        changed,
    })
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
    collect_pyi_files(root, &mut files)?;
    let mut modules = BTreeMap::new();
    for path in files {
        let module = module_name(root, &path)?;
        let source = fs::read_to_string(&path)
            .with_context(|| format!("unable to read {}", path.display()))?;
        modules.insert(module, public_symbols(&source));
    }
    if contains_py_typed_marker(root)? {
        insert_py_typed_marker(&mut modules);
    }
    Ok(modules)
}

fn collect_zip_surface(path: &Path) -> Result<BTreeMap<String, BTreeMap<String, PublicSymbol>>> {
    let file =
        fs::File::open(path).with_context(|| format!("unable to open {}", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("unable to read zip artifact {}", path.display()))?;
    let mut modules = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).with_context(|| {
            format!("unable to read entry {index} from zip artifact {}", path.display())
        })?;
        let entry_name = file.name().to_owned();
        if entry_name.ends_with("py.typed") {
            insert_py_typed_marker(&mut modules);
            continue;
        }
        if !entry_name.ends_with(".pyi") || entry_name.contains(".dist-info/") {
            continue;
        }
        let mut source = String::new();
        file.read_to_string(&mut source).with_context(|| {
            format!("unable to read stub entry {entry_name} from {}", path.display())
        })?;
        modules.insert(module_name_from_archive_entry(&entry_name), public_symbols(&source));
    }
    Ok(modules)
}

fn collect_tar_gz_surface(path: &Path) -> Result<BTreeMap<String, BTreeMap<String, PublicSymbol>>> {
    let file =
        fs::File::open(path).with_context(|| format!("unable to open {}", path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = TarArchive::new(decoder);
    let mut modules = BTreeMap::new();
    for entry in archive
        .entries()
        .with_context(|| format!("unable to read tar artifact {}", path.display()))?
    {
        let mut entry =
            entry.with_context(|| format!("unable to read tar entry in {}", path.display()))?;
        let entry_path = entry
            .path()
            .with_context(|| format!("unable to read tar entry path in {}", path.display()))?
            .to_string_lossy()
            .into_owned();
        if entry_path.ends_with("py.typed") {
            insert_py_typed_marker(&mut modules);
            continue;
        }
        if !entry_path.ends_with(".pyi") {
            continue;
        }
        let mut source = String::new();
        entry.read_to_string(&mut source).with_context(|| {
            format!("unable to read stub entry {entry_path} from {}", path.display())
        })?;
        modules.insert(module_name_from_archive_entry(&entry_path), public_symbols(&source));
    }
    Ok(modules)
}

fn contains_py_typed_marker(root: &Path) -> Result<bool> {
    if root.is_file() {
        return Ok(root.file_name().and_then(|name| name.to_str()) == Some("py.typed"));
    }
    for entry in fs::read_dir(root).with_context(|| format!("unable to read {}", root.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            if contains_py_typed_marker(&path)? {
                return Ok(true);
            }
        } else if path.file_name().and_then(|name| name.to_str()) == Some("py.typed") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn insert_py_typed_marker(modules: &mut BTreeMap<String, BTreeMap<String, PublicSymbol>>) {
    modules.entry(String::from(TYPING_METADATA_MODULE)).or_default().insert(
        String::from(PY_TYPED_SYMBOL),
        PublicSymbol {
            kind: String::from("metadata"),
            signature: String::from("py.typed: present"),
        },
    );
}

fn is_zip_artifact(path: &Path) -> bool {
    matches!(path.extension().and_then(|ext| ext.to_str()), Some("whl" | "zip"))
}

fn is_tar_gz_artifact(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".sdist")
}

fn module_name_from_archive_entry(entry_name: &str) -> String {
    let mut parts = Path::new(entry_name)
        .with_extension("")
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if parts.first().is_some_and(|part| part.contains('-')) && parts.len() > 1 {
        parts.remove(0);
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
        let path = entry?.path();
        if path.is_dir() {
            collect_pyi_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("pyi") {
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

fn public_symbols(source: &str) -> BTreeMap<String, PublicSymbol> {
    let mut symbols = BTreeMap::new();
    for line in source.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('@') {
            continue;
        }
        if let Some((name, signature)) =
            public_def(trimmed, "def ").or_else(|| public_def(trimmed, "async def "))
        {
            symbols.insert(name, PublicSymbol { kind: String::from("function"), signature });
        } else if let Some((name, signature)) = public_class(trimmed) {
            symbols.insert(name, PublicSymbol { kind: String::from("class"), signature });
        } else if let Some((name, signature)) = public_value(trimmed) {
            symbols.insert(name, PublicSymbol { kind: String::from("value"), signature });
        }
    }
    symbols
}

fn public_def(line: &str, prefix: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix(prefix)?;
    let name = rest.split(['(', '[']).next()?.trim();
    public_name(name).then(|| (name.to_owned(), line.to_owned()))
}

fn public_class(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("class ")?;
    let name = rest.split(['(', ':', '[']).next()?.trim();
    public_name(name).then(|| (name.to_owned(), line.to_owned()))
}

fn public_value(line: &str) -> Option<(String, String)> {
    let (name, _) = line.split_once(':').or_else(|| line.split_once('='))?;
    let name = name.trim();
    public_name(name).then(|| (name.to_owned(), line.to_owned()))
}

fn public_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('_')
        && name.chars().all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn print_api_diff_text(report: &ApiSurfaceDiffReport) {
    println!("api-diff:");
    println!("  old: {}", report.old);
    println!("  new: {}", report.new);
    for change in &report.removed {
        println!("  removed: {}.{} ({})", change.module, change.symbol, change.kind);
    }
    for change in &report.changed {
        println!("  changed: {}.{} ({})", change.module, change.symbol, change.kind);
    }
    for change in &report.added {
        println!("  added: {}.{} ({})", change.module, change.symbol, change.kind);
    }
}

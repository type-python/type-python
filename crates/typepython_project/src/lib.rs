use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result};
use glob::Pattern;
use serde::{Deserialize, Serialize};
use typepython_config::ConfigHandle;
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_syntax::SourceKind;

#[derive(Debug, Clone)]
pub struct DiscoveredSource {
    pub path: PathBuf,
    pub root: PathBuf,
    pub kind: SourceKind,
    pub logical_module: String,
    pub load_as_inferred_stub: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ExternalSupportRoot {
    pub path: PathBuf,
    pub allow_untyped_runtime: bool,
}

#[derive(Debug, Clone)]
pub struct ModuleCollision {
    pub logical_module: String,
    pub sources: Vec<DiscoveredSource>,
}

#[derive(Debug, Clone, Default)]
pub struct SupportSourceIndex {
    sources_by_module: BTreeMap<String, Vec<DiscoveredSource>>,
}

impl SupportSourceIndex {
    pub fn from_sources(mut sources: Vec<DiscoveredSource>) -> Self {
        sort_sources_by_type_authority(&mut sources);
        sources.dedup_by(|left, right| left.path == right.path);
        let mut sources_by_module = BTreeMap::<String, Vec<DiscoveredSource>>::new();
        for source in sources {
            sources_by_module.entry(source.logical_module.clone()).or_default().push(source);
        }
        Self { sources_by_module }
    }

    pub fn all_sources(&self) -> Vec<DiscoveredSource> {
        self.sources_by_module.values().flatten().cloned().collect()
    }

    pub fn sources_by_module(&self) -> &BTreeMap<String, Vec<DiscoveredSource>> {
        &self.sources_by_module
    }

    pub fn into_sources_by_module(self) -> BTreeMap<String, Vec<DiscoveredSource>> {
        self.sources_by_module
    }
}

pub fn resolve_python_executable(config: &ConfigHandle) -> PathBuf {
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

pub fn compile_patterns(
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

pub fn source_roots(config: &ConfigHandle) -> Vec<PathBuf> {
    config.config.project.src.iter().map(|root| config.resolve_relative_path(root)).collect()
}

pub fn collect_project_sources(
    config: &ConfigHandle,
    source_roots: &[PathBuf],
    include_patterns: &[Pattern],
    exclude_patterns: &[Pattern],
) -> Result<Vec<DiscoveredSource>> {
    let mut sources = Vec::new();
    for root in source_roots {
        walk_directory(config, root, include_patterns, exclude_patterns, &mut sources)?;
    }
    Ok(sources)
}

pub fn discover_project_source_for_path(
    config: &ConfigHandle,
    source_roots: &[PathBuf],
    include_patterns: &[Pattern],
    exclude_patterns: &[Pattern],
    path: &Path,
) -> Result<Option<DiscoveredSource>> {
    let Some(kind) = SourceKind::from_path(path) else {
        return Ok(None);
    };
    if !is_selected_source_path(config, path, include_patterns, exclude_patterns)? {
        return Ok(None);
    }
    let Some(root) = source_root_for_path_from_roots(source_roots, path) else {
        return Ok(None);
    };
    let Some(logical_module) = logical_module_path(&root, path) else {
        return Ok(None);
    };
    Ok(Some(DiscoveredSource {
        path: path.to_path_buf(),
        root,
        kind,
        logical_module,
        load_as_inferred_stub: false,
    }))
}

pub fn walk_directory(
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

pub fn is_selected_source_path(
    config: &ConfigHandle,
    path: &Path,
    include_patterns: &[Pattern],
    exclude_patterns: &[Pattern],
) -> Result<bool> {
    let relative = path.strip_prefix(&config.config_dir).with_context(|| {
        format!("unable to relativize {} to {}", path.display(), config.config_dir.display())
    })?;
    let relative = normalize_glob_path(relative);

    Ok(include_patterns.iter().any(|pattern| pattern.matches(&relative))
        && !exclude_patterns.iter().any(|pattern| pattern.matches(&relative)))
}

pub fn source_root_for_path(config: &ConfigHandle, path: &Path) -> Option<PathBuf> {
    source_root_for_path_from_roots(&source_roots(config), path)
}

pub fn source_root_for_path_from_roots(source_roots: &[PathBuf], path: &Path) -> Option<PathBuf> {
    source_roots.iter().find(|root| path.starts_with(root)).cloned()
}

pub fn logical_module_path(root: &Path, path: &Path) -> Option<String> {
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

pub fn package_components(relative_parent: &Path) -> Option<Vec<String>> {
    let mut components = Vec::new();
    for component in relative_parent.components() {
        components.push(component.as_os_str().to_str()?.to_owned());
    }
    Some(components)
}

pub fn normalize_glob_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn sort_sources_by_type_authority(sources: &mut [DiscoveredSource]) {
    sources.sort_by(|left, right| {
        left.logical_module
            .cmp(&right.logical_module)
            .then_with(|| {
                source_kind_authority_rank(left.kind).cmp(&source_kind_authority_rank(right.kind))
            })
            .then_with(|| left.path.cmp(&right.path))
    });
}

pub fn source_kind_authority_rank(kind: SourceKind) -> u8 {
    match kind {
        SourceKind::TypePython => 0,
        SourceKind::Stub => 1,
        SourceKind::Python => 2,
    }
}

pub fn detect_module_collisions(
    sources: &[DiscoveredSource],
    source_roots: &[PathBuf],
) -> Vec<ModuleCollision> {
    let mut by_module = BTreeMap::<&str, Vec<&DiscoveredSource>>::new();
    for source in sources {
        by_module.entry(&source.logical_module).or_default().push(source);
    }

    let normalized_roots: BTreeSet<_> =
        source_roots.iter().map(|root| normalize_glob_path(root)).collect();
    let mut collisions = Vec::new();

    for (logical_module, module_sources) in by_module {
        if module_sources.len() < 2 {
            continue;
        }

        let distinct_roots: BTreeSet<_> =
            module_sources.iter().map(|source| normalize_glob_path(&source.root)).collect();
        let has_multiple_roots =
            distinct_roots.len() > 1 && distinct_roots.is_subset(&normalized_roots);

        if has_multiple_roots || !allows_runtime_with_stub_pair(&module_sources) {
            collisions.push(ModuleCollision {
                logical_module: logical_module.to_owned(),
                sources: module_sources.into_iter().cloned().collect(),
            });
        }
    }

    collisions
}

pub fn module_collision_diagnostics(collisions: &[ModuleCollision]) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();

    for collision in collisions {
        let mut diagnostic = Diagnostic::error(
            "TPY3002",
            format!("logical module `{}` has conflicting source files", collision.logical_module),
        );
        for source in &collision.sources {
            diagnostic = diagnostic.with_note(format!(
                "{} ({})",
                source.path.display(),
                source_kind_name(source.kind)
            ));
        }
        diagnostics.push(diagnostic);
    }

    diagnostics
}

pub fn source_kind_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::TypePython => ".tpy",
        SourceKind::Python => ".py",
        SourceKind::Stub => ".pyi",
    }
}

pub fn bundled_stdlib_root(manifest_dir: &str) -> PathBuf {
    PathBuf::from(manifest_dir).join("../../stdlib")
}

pub fn bundled_stdlib_sources_for_root(
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

pub fn walk_bundled_stdlib_directory(
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
        if kind != SourceKind::Stub || !bundled_stdlib_file_matches_target(&path, target_python)? {
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

pub fn bundled_stdlib_file_matches_target(path: &Path, target_python: &str) -> Result<bool> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("unable to read bundled stdlib file {}", path.display()))?;
    Ok(parse_bundled_stdlib_version_filter(&contents).allows(target_python))
}

pub fn support_source_index(
    config: &ConfigHandle,
    target_python: &str,
) -> Result<SupportSourceIndex> {
    let stdlib_root = bundled_stdlib_root(env!("CARGO_MANIFEST_DIR"));
    let external_roots = configured_external_type_roots(config)?;
    let cache_path = support_source_index_cache_path(config, target_python);

    if let Some(index) =
        load_cached_support_source_index(&cache_path, target_python, &stdlib_root, &external_roots)?
    {
        return Ok(index);
    }

    let mut sources = bundled_stdlib_sources_for_root(&stdlib_root, target_python)?;
    for root in &external_roots {
        walk_external_type_root(root, &mut sources)?;
    }
    let index = SupportSourceIndex::from_sources(sources);
    let _ = write_cached_support_source_index(
        &cache_path,
        target_python,
        &stdlib_root,
        &external_roots,
        &index,
    );
    Ok(index)
}

pub fn external_resolution_sources(config: &ConfigHandle) -> Result<Vec<DiscoveredSource>> {
    let mut sources = Vec::new();
    for root in configured_external_type_roots(config)? {
        walk_external_type_root(&root, &mut sources)?;
    }
    sort_sources_by_type_authority(&mut sources);
    sources.dedup_by(|left, right| left.path == right.path);
    Ok(sources)
}

pub fn configured_external_type_roots(config: &ConfigHandle) -> Result<Vec<ExternalSupportRoot>> {
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
    roots.extend(python_type_roots_from_interpreter(&resolve_python_executable(config)));
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

pub fn python_type_roots_from_interpreter(interpreter: &Path) -> Vec<ExternalSupportRoot> {
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

pub fn walk_external_type_root(
    root: &ExternalSupportRoot,
    sources: &mut Vec<DiscoveredSource>,
) -> Result<()> {
    walk_external_type_root_directory(root, &root.path, sources)
}

pub fn walk_external_type_root_directory(
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

pub fn external_source_allowed(root: &ExternalSupportRoot, path: &Path, kind: SourceKind) -> bool {
    match kind {
        SourceKind::Stub => true,
        SourceKind::Python => external_runtime_allowed(root, path),
        SourceKind::TypePython => false,
    }
}

pub fn external_runtime_allowed(root: &ExternalSupportRoot, path: &Path) -> bool {
    if root.allow_untyped_runtime {
        return true;
    }
    let Some(stub_root) = sibling_stub_distribution_root(&root.path, path) else {
        return external_runtime_is_typed(&root.path, path);
    };

    partial_stub_package_marker(&stub_root)
        && runtime_module_missing_from_stub_package(&root.path, path, &stub_root)
}

pub fn external_logical_module_path(root: &Path, path: &Path) -> Option<String> {
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

pub fn external_runtime_is_typed(root: &Path, path: &Path) -> bool {
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

pub fn sibling_stub_distribution_root(root: &Path, path: &Path) -> Option<PathBuf> {
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

pub fn runtime_module_missing_from_stub_package(
    root: &Path,
    path: &Path,
    stub_root: &Path,
) -> bool {
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

pub fn partial_stub_package_marker(stub_root: &Path) -> bool {
    fs::read_to_string(stub_root.join("py.typed"))
        .ok()
        .is_some_and(|contents| contents.lines().any(|line| line.trim() == "partial"))
}

const SUPPORT_SOURCE_INDEX_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
struct CachedSupportSourceIndex {
    version: u32,
    target_python: String,
    roots: Vec<CachedSupportRoot>,
    sources: Vec<CachedSupportSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
struct CachedSupportRoot {
    kind: String,
    path: String,
    allow_untyped_runtime: bool,
    modified_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
struct CachedSupportSource {
    path: String,
    root: String,
    logical_module: String,
    load_as_inferred_stub: bool,
}

fn support_source_index_cache_path(config: &ConfigHandle, target_python: &str) -> PathBuf {
    let sanitized_target = target_python.replace('.', "_");
    config
        .resolve_relative_path(&config.config.project.cache_dir)
        .join(format!("support-index-v{SUPPORT_SOURCE_INDEX_VERSION}-{sanitized_target}.json"))
}

fn load_cached_support_source_index(
    cache_path: &Path,
    target_python: &str,
    stdlib_root: &Path,
    external_roots: &[ExternalSupportRoot],
) -> Result<Option<SupportSourceIndex>> {
    if !cache_path.is_file() {
        return Ok(None);
    }

    let rendered = match fs::read_to_string(cache_path) {
        Ok(rendered) => rendered,
        Err(_) => return Ok(None),
    };
    let cached = match serde_json::from_str::<CachedSupportSourceIndex>(&rendered) {
        Ok(cached) => cached,
        Err(_) => return Ok(None),
    };
    let current_roots = support_root_signatures(stdlib_root, external_roots)?;
    if cached.version != SUPPORT_SOURCE_INDEX_VERSION
        || cached.target_python != target_python
        || cached.roots != current_roots
    {
        return Ok(None);
    }

    Ok(Some(SupportSourceIndex::from_sources(
        cached
            .sources
            .into_iter()
            .filter_map(|source| {
                let path = PathBuf::from(&source.path);
                Some(DiscoveredSource {
                    kind: SourceKind::from_path(&path)?,
                    path,
                    root: PathBuf::from(source.root),
                    logical_module: source.logical_module,
                    load_as_inferred_stub: source.load_as_inferred_stub,
                })
            })
            .collect(),
    )))
}

fn write_cached_support_source_index(
    cache_path: &Path,
    target_python: &str,
    stdlib_root: &Path,
    external_roots: &[ExternalSupportRoot],
    index: &SupportSourceIndex,
) -> Result<()> {
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("unable to create support cache directory {}", parent.display())
        })?;
    }

    let cached = CachedSupportSourceIndex {
        version: SUPPORT_SOURCE_INDEX_VERSION,
        target_python: target_python.to_owned(),
        roots: support_root_signatures(stdlib_root, external_roots)?,
        sources: index
            .all_sources()
            .into_iter()
            .map(|source| CachedSupportSource {
                path: source.path.display().to_string(),
                root: source.root.display().to_string(),
                logical_module: source.logical_module,
                load_as_inferred_stub: source.load_as_inferred_stub,
            })
            .collect(),
    };
    let payload =
        serde_json::to_string(&cached).context("unable to serialize support source index")?;
    fs::write(cache_path, payload)
        .with_context(|| format!("unable to write support source index {}", cache_path.display()))
}

fn support_root_signatures(
    stdlib_root: &Path,
    external_roots: &[ExternalSupportRoot],
) -> Result<Vec<CachedSupportRoot>> {
    let mut roots = vec![cached_support_root("bundled_stdlib", stdlib_root, false)?];
    for root in external_roots {
        roots.push(cached_support_root("external", &root.path, root.allow_untyped_runtime)?);
    }
    roots.sort();
    Ok(roots)
}

fn cached_support_root(
    kind: &str,
    path: &Path,
    allow_untyped_runtime: bool,
) -> Result<CachedSupportRoot> {
    let modified_unix_ms = path
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_millis()).ok());
    Ok(CachedSupportRoot {
        kind: kind.to_owned(),
        path: path.display().to_string(),
        allow_untyped_runtime,
        modified_unix_ms,
    })
}

#[derive(Debug, Default, Clone)]
pub struct BundledStdlibVersionFilter {
    pub min_python: Option<String>,
    pub max_python: Option<String>,
}

impl BundledStdlibVersionFilter {
    pub fn allows(&self, target_python: &str) -> bool {
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

pub fn parse_bundled_stdlib_version_filter(source: &str) -> BundledStdlibVersionFilter {
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

fn allows_runtime_with_stub_pair(module_sources: &[&DiscoveredSource]) -> bool {
    if module_sources.len() != 2 {
        return false;
    }

    matches!(
        (module_sources[0].kind, module_sources[1].kind),
        (SourceKind::Python, SourceKind::Stub) | (SourceKind::Stub, SourceKind::Python)
    ) && module_sources[0].root == module_sources[1].root
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalSupportRootProbe {
    path: String,
    allow_untyped_runtime: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, time::SystemTime};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    fn write_executable_script(path: &Path, script: &str) {
        fs::write(path, script).expect("script should be written");
        let mut permissions =
            fs::metadata(path).expect("script metadata should exist").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("script permissions should be updated");
    }

    fn temp_project_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("typepython-project-{test_name}-{unique}"));
        fs::create_dir_all(&root).expect("temp project directory should be created");
        root
    }

    fn remove_temp_project_dir(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    #[cfg(unix)]
    #[test]
    fn support_source_index_writes_cache_file() {
        let project_dir = temp_project_dir("support_source_index_writes_cache_file");
        let result = {
            let probe = project_dir.join("python-probe");
            write_executable_script(
                &probe,
                "#!/bin/sh\nif [ \"$1\" = \"-c\" ] && printf '%s' \"$2\" | grep -q version_info; then\n  printf '3.10\\n'\nelse\n  printf '[]\\n'\nfi\n",
            );
            fs::create_dir_all(project_dir.join("site-packages/demo"))
                .expect("support package directory should be created");
            fs::write(project_dir.join("site-packages/demo/__init__.pyi"), "pass\n")
                .expect("support package stub should be written");
            fs::write(
                project_dir.join("typepython.toml"),
                format!(
                    "[project]\nsrc = [\"src\"]\n\n[resolution]\ntype_roots = [\"{}\"]\npython_executable = \"{}\"\n",
                    project_dir.join("site-packages").display(),
                    probe.display()
                ),
            )
            .expect("typepython.toml should be written");
            let config = typepython_config::load(&project_dir).expect("config should load");
            let index = support_source_index(&config, &config.config.project.target_python)
                .expect("index should build");
            let cache_path =
                support_source_index_cache_path(&config, &config.config.project.target_python);

            (index.sources_by_module().contains_key("demo"), cache_path.is_file())
        };
        remove_temp_project_dir(&project_dir);

        let (has_demo, cache_exists) = result;
        assert!(has_demo);
        assert!(cache_exists);
    }

    #[cfg(unix)]
    #[test]
    fn load_cached_support_source_index_round_trips_cached_entries() {
        let project_dir =
            temp_project_dir("load_cached_support_source_index_round_trips_cached_entries");
        let result = {
            let probe = project_dir.join("python-probe");
            write_executable_script(
                &probe,
                "#!/bin/sh\nif [ \"$1\" = \"-c\" ] && printf '%s' \"$2\" | grep -q version_info; then\n  printf '3.10\\n'\nelse\n  printf '[]\\n'\nfi\n",
            );
            fs::create_dir_all(project_dir.join("site-packages/demo"))
                .expect("support package directory should be created");
            fs::write(project_dir.join("site-packages/demo/__init__.pyi"), "pass\n")
                .expect("support package stub should be written");
            fs::write(
                project_dir.join("typepython.toml"),
                format!(
                    "[project]\nsrc = [\"src\"]\n\n[resolution]\ntype_roots = [\"{}\"]\npython_executable = \"{}\"\n",
                    project_dir.join("site-packages").display(),
                    probe.display()
                ),
            )
            .expect("typepython.toml should be written");
            let config = typepython_config::load(&project_dir).expect("config should load");
            let stdlib_root = bundled_stdlib_root(env!("CARGO_MANIFEST_DIR"));
            let external_roots =
                configured_external_type_roots(&config).expect("external roots should resolve");
            let cache_path =
                support_source_index_cache_path(&config, &config.config.project.target_python);
            let index = support_source_index(&config, &config.config.project.target_python)
                .expect("index should build");
            let cached = load_cached_support_source_index(
                &cache_path,
                &config.config.project.target_python,
                &stdlib_root,
                &external_roots,
            )
            .expect("cache load should succeed")
            .expect("cache should be valid");

            (
                index.sources_by_module().get("demo").map(Vec::len),
                cached.sources_by_module().get("demo").map(Vec::len),
            )
        };
        remove_temp_project_dir(&project_dir);

        assert_eq!(result.0, Some(1));
        assert_eq!(result.0, result.1);
    }
}

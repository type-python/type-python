use super::*;

pub(crate) fn collect_project_source_paths(
    config: &ConfigHandle,
    overlays: &BTreeMap<PathBuf, OverlayDocument>,
) -> Result<Vec<DiscoveredSource>> {
    let include_patterns = compile_patterns(config, &config.config.project.include)?;
    let exclude_patterns = compile_patterns(config, &config.config.project.exclude)?;
    let source_roots: Vec<_> =
        config.config.project.src.iter().map(|root| config.resolve_relative_path(root)).collect();
    let mut local_sources = Vec::new();

    for root in &source_roots {
        walk_directory(config, root, &include_patterns, &exclude_patterns, &mut local_sources)?;
    }

    for path in overlays.keys() {
        let Some(kind) = SourceKind::from_path(path) else {
            continue;
        };
        if !is_selected_source_path(config, path, &include_patterns, &exclude_patterns)? {
            continue;
        }
        let Some(root) = source_root_for_path(config, path) else {
            continue;
        };
        let Some(logical_module) = logical_module_path(&root, path) else {
            continue;
        };
        if !local_sources.iter().any(|source| source.path == *path) {
            local_sources.push(DiscoveredSource { path: path.clone(), kind, logical_module });
        }
    }

    sort_sources_by_type_authority(&mut local_sources);
    local_sources.dedup_by(|left, right| left.path == right.path);
    Ok(local_sources)
}

pub(crate) fn collect_import_source_paths(syntax_trees: &[SyntaxTree]) -> Vec<String> {
    syntax_trees
        .iter()
        .flat_map(|tree| tree.statements.iter())
        .filter_map(|statement| match statement {
            SyntaxStatement::Import(statement) => Some(
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

pub(crate) fn import_resolves_within_modules(
    import_path: &str,
    module_keys: &BTreeSet<String>,
) -> bool {
    module_path_prefixes(import_path).any(|module_key| module_keys.contains(module_key))
}

pub(crate) fn matching_support_module_keys(
    import_path: &str,
    sources_by_module: &BTreeMap<String, Vec<DiscoveredSource>>,
) -> Vec<String> {
    module_path_prefixes(import_path)
        .filter(|module_key| sources_by_module.contains_key(*module_key))
        .map(str::to_owned)
        .collect()
}

pub(crate) fn module_path_prefixes(import_path: &str) -> impl Iterator<Item = &str> {
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

pub(crate) fn bundled_stdlib_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib")
}

pub(crate) fn bundled_stdlib_sources(target_python: &str) -> Result<Vec<DiscoveredSource>> {
    let root = bundled_stdlib_root();
    let mut sources = Vec::new();
    if root.exists() {
        walk_bundled_stdlib_directory(&root, &root, target_python, &mut sources)?;
    }
    Ok(sources)
}

pub(crate) fn walk_bundled_stdlib_directory(
    root: &Path,
    directory: &Path,
    target_python: &str,
    sources: &mut Vec<DiscoveredSource>,
) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)
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
        if !sources.iter().any(|source| source.path == path) {
            sources.push(DiscoveredSource { path, kind, logical_module });
        }
    }
    Ok(())
}

pub(crate) fn external_resolution_sources(config: &ConfigHandle) -> Result<Vec<DiscoveredSource>> {
    let mut sources = Vec::new();
    for root in configured_external_type_roots(config)? {
        walk_external_type_root(&root, &mut sources)?;
    }
    sort_sources_by_type_authority(&mut sources);
    sources.dedup_by(|left, right| left.path == right.path);
    Ok(sources)
}

pub(crate) fn configured_external_type_roots(config: &ConfigHandle) -> Result<Vec<PathBuf>> {
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

pub(crate) fn discovered_python_type_roots(config: &ConfigHandle) -> Vec<PathBuf> {
    let interpreter = resolve_python_executable(config);
    python_type_roots_from_interpreter(&interpreter)
}

pub(crate) fn python_type_roots_from_interpreter(interpreter: &Path) -> Vec<PathBuf> {
    let output = ProcessCommand::new(interpreter)
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

pub(crate) fn bundled_stdlib_file_matches_target(path: &Path, target_python: &str) -> Result<bool> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("unable to read bundled stdlib file {}", path.display()))?;
    Ok(parse_bundled_stdlib_version_filter(&contents).allows(target_python))
}

#[derive(Debug, Default, Clone)]
pub(crate) struct BundledStdlibVersionFilter {
    min_python: Option<String>,
    max_python: Option<String>,
}

impl BundledStdlibVersionFilter {
    pub(crate) fn allows(&self, target_python: &str) -> bool {
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

pub(crate) fn parse_bundled_stdlib_version_filter(source: &str) -> BundledStdlibVersionFilter {
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

pub(crate) fn parse_supported_python_version(version: &str) -> Option<(u8, u8)> {
    let (major, minor) = version.trim().split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

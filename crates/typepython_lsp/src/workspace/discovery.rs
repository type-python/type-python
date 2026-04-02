pub(super) fn resolve_python_executable(config: &ConfigHandle) -> PathBuf {
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

pub(super) fn walk_external_type_root(
    root: &Path,
    sources: &mut Vec<DiscoveredSource>,
) -> Result<()> {
    walk_external_type_root_directory(root, root, sources)
}

pub(super) fn walk_external_type_root_directory(
    root: &Path,
    directory: &Path,
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
            walk_external_type_root_directory(root, &path, sources)?;
            continue;
        }

        let Some(kind) = SourceKind::from_path(&path) else {
            continue;
        };
        if !external_source_allowed(root, &path, kind) {
            continue;
        }
        let Some(logical_module) = external_logical_module_path(root, &path) else {
            continue;
        };
        if !sources.iter().any(|source| source.path == path) {
            sources.push(DiscoveredSource { path, kind, logical_module });
        }
    }
    Ok(())
}

pub(super) fn external_source_allowed(root: &Path, path: &Path, kind: SourceKind) -> bool {
    match kind {
        SourceKind::Stub => true,
        SourceKind::Python => external_runtime_allowed(root, path),
        SourceKind::TypePython => false,
    }
}

pub(super) fn external_runtime_allowed(root: &Path, path: &Path) -> bool {
    let Some(stub_root) = sibling_stub_distribution_root(root, path) else {
        return external_runtime_is_typed(root, path);
    };

    partial_stub_package_marker(&stub_root)
        && runtime_module_missing_from_stub_package(root, path, &stub_root)
}

pub(super) fn external_logical_module_path(root: &Path, path: &Path) -> Option<String> {
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

pub(super) fn external_runtime_is_typed(root: &Path, path: &Path) -> bool {
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

pub(super) fn sibling_stub_distribution_root(root: &Path, path: &Path) -> Option<PathBuf> {
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

pub(super) fn runtime_module_missing_from_stub_package(
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

pub(super) fn sort_sources_by_type_authority(sources: &mut [DiscoveredSource]) {
    sources.sort_by(|left, right| {
        left.logical_module
            .cmp(&right.logical_module)
            .then_with(|| {
                source_kind_authority_rank(left.kind).cmp(&source_kind_authority_rank(right.kind))
            })
            .then_with(|| left.path.cmp(&right.path))
    });
}

pub(super) fn source_kind_authority_rank(kind: SourceKind) -> u8 {
    match kind {
        SourceKind::TypePython => 0,
        SourceKind::Stub => 1,
        SourceKind::Python => 2,
    }
}

pub(super) fn partial_stub_package_marker(stub_root: &Path) -> bool {
    std::fs::read_to_string(stub_root.join("py.typed"))
        .ok()
        .is_some_and(|contents| contents.lines().any(|line| line.trim() == "partial"))
}

pub(super) fn compile_patterns(config: &ConfigHandle, patterns: &[String]) -> Result<Vec<Pattern>> {
    patterns
        .iter()
        .map(|pattern| {
            Pattern::new(pattern).with_context(|| {
                format!("invalid glob pattern `{pattern}` in {}", config.config_path.display())
            })
        })
        .collect()
}

pub(super) fn walk_directory(
    config: &ConfigHandle,
    directory: &Path,
    include_patterns: &[Pattern],
    exclude_patterns: &[Pattern],
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
        sources.push(DiscoveredSource { path, kind, logical_module });
    }
    Ok(())
}

pub(super) fn is_selected_source_path(
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

pub(super) fn source_root_for_path(config: &ConfigHandle, path: &Path) -> Option<PathBuf> {
    config
        .config
        .project
        .src
        .iter()
        .map(|root| config.resolve_relative_path(root))
        .find(|root| path.starts_with(root))
}

pub(super) fn logical_module_path(root: &Path, path: &Path) -> Option<String> {
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

pub(super) fn package_components(relative_parent: &Path) -> Option<Vec<String>> {
    let mut components = Vec::new();
    for component in relative_parent.components() {
        let name = component.as_os_str().to_str()?.to_owned();
        components.push(name);
    }
    Some(components)
}

pub(super) fn collect_parse_diagnostics(syntax_trees: &[SyntaxTree]) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    for tree in syntax_trees {
        diagnostics.diagnostics.extend(tree.diagnostics.diagnostics.iter().cloned());
    }
    diagnostics
}

pub(super) fn normalize_glob_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(super) fn normalize_path_string(path: &Path) -> String {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
}

pub(super) fn path_to_uri(path: &Path) -> String {
    Url::from_file_path(path).expect("filesystem paths should convert to file:// URIs").into()
}

pub(super) fn uri_to_path(uri: &str) -> Result<PathBuf, LspError> {
    let parsed = Url::parse(uri)
        .map_err(|error| LspError::Other(format!("unsupported URI `{uri}`: {error}")))?;
    parsed.to_file_path().map_err(|()| LspError::Other(format!("unsupported URI `{uri}`")))
}

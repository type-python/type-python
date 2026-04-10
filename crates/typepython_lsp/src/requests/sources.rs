use super::*;
use typepython_project::{
    bundled_stdlib_root as shared_bundled_stdlib_root,
    bundled_stdlib_sources_for_root as shared_bundled_stdlib_sources_for_root,
    collect_project_sources, discover_project_source_for_path, source_roots,
};

pub(crate) fn collect_project_source_paths(
    config: &ConfigHandle,
    overlays: &BTreeMap<PathBuf, OverlayDocument>,
) -> Result<Vec<DiscoveredSource>> {
    let include_patterns =
        compile_patterns(config, &config.config.project.include, "project.include")?;
    let exclude_patterns =
        compile_patterns(config, &config.config.project.exclude, "project.exclude")?;
    let source_roots = source_roots(config);
    let mut local_sources =
        collect_project_sources(config, &source_roots, &include_patterns, &exclude_patterns)?;

    for path in overlays.keys() {
        let Some(source) = discover_project_source_for_path(
            config,
            &source_roots,
            &include_patterns,
            &exclude_patterns,
            path,
        )?
        else {
            continue;
        };
        if !local_sources.iter().any(|existing| existing.path == source.path) {
            local_sources.push(source);
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
    shared_bundled_stdlib_root(env!("CARGO_MANIFEST_DIR"))
}

pub(crate) fn bundled_stdlib_sources(target_python: &str) -> Result<Vec<DiscoveredSource>> {
    shared_bundled_stdlib_sources_for_root(&bundled_stdlib_root(), target_python)
}

pub(crate) fn external_resolution_sources(config: &ConfigHandle) -> Result<Vec<DiscoveredSource>> {
    typepython_project::external_resolution_sources(config)
}

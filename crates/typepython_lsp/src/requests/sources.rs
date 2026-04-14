use super::*;
use typepython_project::{
    collect_import_source_paths as shared_collect_import_source_paths, collect_project_sources,
    discover_project_source_for_path,
    import_resolves_within_modules as shared_import_resolves_within_modules, source_roots,
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
    shared_collect_import_source_paths(syntax_trees)
}

pub(crate) fn import_resolves_within_modules(
    import_path: &str,
    module_keys: &BTreeSet<String>,
) -> bool {
    shared_import_resolves_within_modules(import_path, module_keys)
}

use std::{
    fs,
    num::Wrapping,
    path::{Path, PathBuf},
};

use anyhow::Result;
use typepython_config::ConfigHandle;
use typepython_diagnostics::DiagnosticReport;
pub(crate) use typepython_project::{DiscoveredSource, ExternalSupportRoot, normalize_glob_path};
use typepython_project::{
    bundled_stdlib_file_matches_target, bundled_stdlib_root,
    bundled_stdlib_sources_for_root as shared_bundled_stdlib_sources_for_root,
    collect_project_sources, compile_patterns, configured_external_type_roots,
    detect_module_collisions, module_collision_diagnostics,
    python_type_roots_from_interpreter as shared_python_type_roots_from_interpreter,
    sort_sources_by_type_authority, source_roots,
    support_source_index as shared_support_source_index, walk_external_type_root,
};

#[derive(Debug)]
pub(crate) struct SourceDiscovery {
    pub(crate) sources: Vec<DiscoveredSource>,
    pub(crate) diagnostics: DiagnosticReport,
}

pub(crate) fn collect_source_paths(config: &ConfigHandle) -> Result<SourceDiscovery> {
    let include_patterns =
        compile_patterns(config, &config.config.project.include, "project.include")?;
    let exclude_patterns =
        compile_patterns(config, &config.config.project.exclude, "project.exclude")?;
    let source_roots = source_roots(config);
    let mut sources =
        collect_project_sources(config, &source_roots, &include_patterns, &exclude_patterns)?;

    sort_sources_by_type_authority(&mut sources);
    let diagnostics =
        module_collision_diagnostics(&detect_module_collisions(&sources, &source_roots));

    Ok(SourceDiscovery { sources, diagnostics })
}

fn cli_bundled_stdlib_root() -> PathBuf {
    bundled_stdlib_root(env!("CARGO_MANIFEST_DIR"))
}

pub(crate) fn bundled_stdlib_sources(target_python: &str) -> Result<Vec<DiscoveredSource>> {
    shared_bundled_stdlib_sources_for_root(&cli_bundled_stdlib_root(), target_python)
}

pub(crate) fn bundled_stdlib_sources_for_root(
    root: &Path,
    target_python: &str,
) -> Result<Vec<DiscoveredSource>> {
    shared_bundled_stdlib_sources_for_root(root, target_python)
}

pub(crate) fn bundled_stdlib_snapshot_identity(target_python: &str) -> Result<String> {
    bundled_stdlib_snapshot_identity_for_root(&cli_bundled_stdlib_root(), target_python)
}

pub(crate) fn bundled_stdlib_snapshot_identity_for_root(
    root: &Path,
    target_python: &str,
) -> Result<String> {
    let mut files = Vec::new();
    if root.exists() {
        collect_stdlib_files(root, root, target_python, &mut files)?;
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hash = Wrapping(0xcbf29ce484222325_u64);
    let prime = Wrapping(0x100000001b3_u64);
    for byte in target_python.as_bytes().iter().chain([0_u8].iter()) {
        hash ^= Wrapping(u64::from(*byte));
        hash *= prime;
    }
    for (relative, bytes) in files {
        for byte in relative.as_bytes().iter().chain([0_u8].iter()).chain(bytes.iter()) {
            hash ^= Wrapping(u64::from(*byte));
            hash *= prime;
        }
    }

    Ok(format!("fnv1a64:{:016x}", hash.0))
}

fn collect_stdlib_files(
    root: &Path,
    directory: &Path,
    target_python: &str,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_stdlib_files(root, &path, target_python, files)?;
            continue;
        }
        if !bundled_stdlib_file_matches_target(&path, target_python)? {
            continue;
        }

        let relative = path.strip_prefix(root)?;
        files.push((normalize_glob_path(relative), fs::read(&path)?));
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

pub(crate) fn python_type_roots_from_interpreter(interpreter: &Path) -> Vec<ExternalSupportRoot> {
    shared_python_type_roots_from_interpreter(interpreter)
}

pub(crate) fn support_source_index(
    config: &ConfigHandle,
    target_python: &str,
) -> Result<typepython_project::SupportSourceIndex> {
    shared_support_source_index(config, target_python)
}

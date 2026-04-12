pub(super) fn resolve_python_executable(config: &ConfigHandle) -> PathBuf {
    typepython_project::resolve_python_executable(config)
}

pub(super) fn sort_sources_by_type_authority(sources: &mut [DiscoveredSource]) {
    typepython_project::sort_sources_by_type_authority(sources);
}

pub(super) fn compile_patterns(
    config: &ConfigHandle,
    patterns: &[String],
    field_name: &str,
) -> Result<Vec<Pattern>> {
    typepython_project::compile_patterns(config, patterns, field_name)
}

pub(super) fn collect_parse_diagnostics(syntax_trees: &[SyntaxTree]) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    for tree in syntax_trees {
        diagnostics.diagnostics.extend(tree.diagnostics.diagnostics.iter().cloned());
    }
    diagnostics
}

pub(super) fn normalize_path_string(path: &Path) -> String {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
}

pub(super) fn path_to_uri(path: &Path) -> String {
    Url::from_file_path(path).expect("filesystem paths should convert to file:// URIs").into()
}

pub(super) fn uri_to_path(uri: &str) -> Result<PathBuf, LspError> {
    let parsed = Url::parse(uri)
        .map_err(|error| LspError::invalid_params(format!("unsupported `file://` URI `{uri}`: {error}")))?;
    parsed
        .to_file_path()
        .map_err(|()| LspError::invalid_params(format!("unsupported `file://` URI `{uri}`")))
}

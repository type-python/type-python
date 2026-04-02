use super::*;

impl SupportSourceCatalog {
    pub(super) fn new(config: &ConfigHandle) -> Result<Self, LspError> {
        let mut support_sources = bundled_stdlib_sources(&config.config.project.target_python)?;
        support_sources.extend(external_resolution_sources(config)?);
        let mut sources_by_module = BTreeMap::<String, Vec<DiscoveredSource>>::new();
        for source in support_sources {
            sources_by_module.entry(source.logical_module.clone()).or_default().push(source);
        }
        Ok(Self { sources_by_module })
    }
}

impl IncrementalWorkspace {
    pub(super) fn new(
        config: ConfigHandle,
        overlays: &BTreeMap<PathBuf, OverlayDocument>,
    ) -> Result<Self, LspError> {
        let include_patterns = compile_patterns(&config, &config.config.project.include)?;
        let exclude_patterns = compile_patterns(&config, &config.config.project.exclude)?;
        let source_roots = config
            .config
            .project
            .src
            .iter()
            .map(|root| config.resolve_relative_path(root))
            .collect();
        let support_catalog = SupportSourceCatalog::new(&config)?;

        let mut project_documents = BTreeMap::new();
        for source in collect_project_source_paths(&config, overlays)? {
            let syntax = parse_discovered_source(
                &source,
                overlays.get(&source.path).map(|overlay| overlay.text.as_str()),
                config.config.typing.conditional_returns,
            )?;
            let binding = bind(&syntax);
            let (document, declarations) = index_document_state(syntax);
            project_documents.insert(
                source.path.clone(),
                CachedDocument { source, binding, document, declarations, references: Vec::new() },
            );
        }

        let mut workspace = Self {
            config,
            include_patterns,
            exclude_patterns,
            source_roots,
            support_catalog,
            project_documents,
            support_documents: BTreeMap::new(),
            active_support_paths: BTreeSet::new(),
            check_diagnostics_by_module: BTreeMap::new(),
            incremental: IncrementalState::default(),
            dependency_index: ModuleDependencyIndex::default(),
            parse_blocked: false,
            state: empty_workspace_state(),
            last_state_refresh_was_full: false,
        };
        let direct_changes = workspace
            .project_documents
            .values()
            .map(|document| document.source.logical_module.clone())
            .collect();
        workspace.sync_support_documents()?;
        workspace.rebuild_state(direct_changes, true)?;
        Ok(workspace)
    }

    pub(super) fn workspace(&self) -> &WorkspaceState {
        &self.state
    }

    pub(super) fn apply_project_path_update(
        &mut self,
        path: &Path,
        overlay: Option<&OverlayDocument>,
    ) -> Result<(), LspError> {
        let direct_changes = self.update_project_document(path, overlay)?;
        self.sync_support_documents()?;
        self.rebuild_state(direct_changes, false)
    }

    pub(super) fn update_project_document(
        &mut self,
        path: &Path,
        overlay: Option<&OverlayDocument>,
    ) -> Result<BTreeSet<String>, LspError> {
        let mut direct_changes = BTreeSet::new();
        if let Some(existing) = self.project_documents.get(path) {
            direct_changes.insert(existing.source.logical_module.clone());
        }

        let next_source = self.project_source_for_path(path)?;
        match next_source {
            Some(source) => {
                let syntax = parse_discovered_source(
                    &source,
                    overlay.map(|document| document.text.as_str()),
                    self.config.config.typing.conditional_returns,
                )?;
                let binding = bind(&syntax);
                let (document, declarations) = index_document_state(syntax);
                direct_changes.insert(source.logical_module.clone());
                self.project_documents.insert(
                    source.path.clone(),
                    CachedDocument {
                        source,
                        binding,
                        document,
                        declarations,
                        references: Vec::new(),
                    },
                );
            }
            None => {
                self.project_documents.remove(path);
            }
        }

        Ok(direct_changes)
    }

    pub(super) fn project_source_for_path(
        &self,
        path: &Path,
    ) -> Result<Option<DiscoveredSource>, LspError> {
        let Some(kind) = SourceKind::from_path(path) else {
            return Ok(None);
        };
        if !is_selected_source_path(
            &self.config,
            path,
            &self.include_patterns,
            &self.exclude_patterns,
        )? {
            return Ok(None);
        }
        let Some(root) = source_root_for_path_from_roots(&self.source_roots, path) else {
            return Ok(None);
        };
        let Some(logical_module) = logical_module_path(&root, path) else {
            return Ok(None);
        };
        Ok(Some(DiscoveredSource { path: path.to_path_buf(), kind, logical_module }))
    }

    pub(super) fn sync_support_documents(&mut self) -> Result<(), LspError> {
        let project_syntax_trees = self
            .project_documents
            .values()
            .map(|document| document.document.syntax.clone())
            .collect::<Vec<_>>();
        let project_modules = project_syntax_trees
            .iter()
            .map(|tree| tree.source.logical_module.clone())
            .collect::<BTreeSet<_>>();
        let external_import_paths = collect_import_source_paths(&project_syntax_trees)
            .into_iter()
            .filter(|import_path| !import_resolves_within_modules(import_path, &project_modules))
            .collect::<Vec<_>>();

        let mut queued_modules = BTreeSet::new();
        let mut queue = VecDeque::new();
        for import_path in external_import_paths {
            for module_key in
                matching_support_module_keys(&import_path, &self.support_catalog.sources_by_module)
            {
                if queued_modules.insert(module_key.clone()) {
                    queue.push_back(module_key);
                }
            }
        }

        let mut active_support_paths = BTreeSet::new();
        while let Some(module_key) = queue.pop_front() {
            let Some(module_sources) =
                self.support_catalog.sources_by_module.get(&module_key).cloned()
            else {
                continue;
            };
            for source in module_sources {
                self.ensure_support_document(&source)?;
                active_support_paths.insert(source.path.clone());
                let document = self
                    .support_documents
                    .get(&source.path)
                    .expect("support document should be loaded");
                for import_path in
                    collect_import_source_paths(std::slice::from_ref(&document.document.syntax))
                {
                    for nested_module_key in matching_support_module_keys(
                        &import_path,
                        &self.support_catalog.sources_by_module,
                    ) {
                        if queued_modules.insert(nested_module_key.clone()) {
                            queue.push_back(nested_module_key);
                        }
                    }
                }
            }
        }

        self.active_support_paths = active_support_paths;
        Ok(())
    }

    pub(super) fn ensure_support_document(
        &mut self,
        source: &DiscoveredSource,
    ) -> Result<(), LspError> {
        if self.support_documents.contains_key(&source.path) {
            return Ok(());
        }

        let syntax =
            parse_discovered_source(source, None, self.config.config.typing.conditional_returns)?;
        let binding = bind(&syntax);
        let (document, declarations) = index_document_state(syntax);
        self.support_documents.insert(
            source.path.clone(),
            CachedDocument {
                source: source.clone(),
                binding,
                document,
                declarations,
                references: Vec::new(),
            },
        );
        Ok(())
    }

    pub(super) fn active_cached_documents(&self) -> Vec<&CachedDocument> {
        let mut documents = self.project_documents.values().collect::<Vec<_>>();
        documents.extend(
            self.support_documents
                .iter()
                .filter(|(path, _)| self.active_support_paths.contains(*path))
                .map(|(_, document)| document),
        );
        documents.sort_by(|left, right| left.source.path.cmp(&right.source.path));
        documents
    }

    pub(super) fn active_syntax_trees(&self) -> Vec<SyntaxTree> {
        self.active_cached_documents()
            .into_iter()
            .map(|document| document.document.syntax.clone())
            .collect()
    }

    pub(super) fn active_bindings(&self) -> Vec<BindingTable> {
        self.active_cached_documents()
            .into_iter()
            .map(|document| document.binding.clone())
            .collect()
    }

    pub(super) fn active_source_overrides(&self) -> BTreeMap<String, String> {
        self.active_cached_documents()
            .into_iter()
            .map(|document| {
                (
                    document.source.path.display().to_string(),
                    document.document.syntax.source.text.clone(),
                )
            })
            .collect()
    }

    pub(super) fn rebuild_state(
        &mut self,
        direct_changes: BTreeSet<String>,
        force_full_check: bool,
    ) -> Result<(), LspError> {
        let syntax_trees = self.active_syntax_trees();
        let bindings = self.active_bindings();
        let graph = build(&bindings);
        let current_module_keys =
            graph.nodes.iter().map(|node| node.module_key.clone()).collect::<BTreeSet<_>>();
        let current_incremental = snapshot(&graph);
        let current_dependency_index = dependency_index(&graph);
        let snapshot_diff = diff(&self.incremental, &current_incremental);
        let summary_changed_modules = snapshot_diff_modules(&snapshot_diff);
        let mut parse_diagnostics = collect_parse_diagnostics(&syntax_trees);
        apply_type_ignore_directives(&syntax_trees, &mut parse_diagnostics);
        let has_parse_errors = parse_diagnostics.has_errors();
        self.check_diagnostics_by_module
            .retain(|module_key, _| current_module_keys.contains(module_key));

        if !has_parse_errors {
            let source_overrides = self.active_source_overrides();
            if force_full_check || self.parse_blocked {
                self.check_diagnostics_by_module = check_modules_with_source_overrides(
                    &graph,
                    &current_module_keys,
                    self.config.config.typing.require_explicit_overrides,
                    self.config.config.typing.enable_sealed_exhaustiveness,
                    self.config.config.typing.report_deprecated,
                    self.config.config.typing.strict,
                    self.config.config.typing.warn_unsafe,
                    self.config.config.typing.imports,
                    Some(&source_overrides),
                )
                .diagnostics_by_module;
            } else {
                let affected = affected_modules(
                    Some(&self.dependency_index),
                    &current_dependency_index,
                    &direct_changes,
                    &summary_changed_modules,
                );
                let rechecked_modules = affected
                    .into_iter()
                    .filter(|module_key| current_module_keys.contains(module_key))
                    .collect::<BTreeSet<_>>();
                if !rechecked_modules.is_empty() {
                    let module_result = check_modules_with_source_overrides(
                        &graph,
                        &rechecked_modules,
                        self.config.config.typing.require_explicit_overrides,
                        self.config.config.typing.enable_sealed_exhaustiveness,
                        self.config.config.typing.report_deprecated,
                        self.config.config.typing.strict,
                        self.config.config.typing.warn_unsafe,
                        self.config.config.typing.imports,
                        Some(&source_overrides),
                    );
                    for (module_key, diagnostics) in module_result.diagnostics_by_module {
                        self.check_diagnostics_by_module.insert(module_key, diagnostics);
                    }
                }
                for removed in &snapshot_diff.removed {
                    self.check_diagnostics_by_module.remove(&removed.module_key);
                }
            }
        }

        let mut diagnostics = parse_diagnostics.clone();
        if !has_parse_errors {
            diagnostics.diagnostics.extend(
                self.check_diagnostics_by_module
                    .values()
                    .flat_map(|module_diagnostics| module_diagnostics.iter().cloned()),
            );
            apply_type_ignore_directives(&syntax_trees, &mut diagnostics);
        }

        let index_trigger_modules =
            direct_changes.union(&summary_changed_modules).cloned().collect::<BTreeSet<_>>();
        let reindexed_modules = if force_full_check || self.parse_blocked {
            current_module_keys.clone()
        } else {
            affected_modules(
                Some(&self.dependency_index),
                &current_dependency_index,
                &direct_changes,
                &index_trigger_modules,
            )
            .into_iter()
            .filter(|module_key| current_module_keys.contains(module_key))
            .collect()
        };
        let force_full_state_refresh = force_full_check || self.parse_blocked;
        self.rebuild_document_indexes(&reindexed_modules, force_full_check || self.parse_blocked);
        self.update_workspace_state(
            diagnostics,
            graph,
            &reindexed_modules,
            force_full_state_refresh,
        );
        self.incremental = current_incremental;
        self.dependency_index = current_dependency_index;
        self.parse_blocked = has_parse_errors;
        Ok(())
    }

    pub(super) fn rebuild_document_indexes(
        &mut self,
        module_keys: &BTreeSet<String>,
        force_full: bool,
    ) {
        let documents = self.active_cached_documents();
        let declarations_by_canonical = declarations_by_canonical_from_documents(&documents);
        let member_symbols = member_symbols_from_documents(&documents);

        for document in self.project_documents.values_mut() {
            if force_full || module_keys.contains(&document.source.logical_module) {
                document.references = collect_reference_occurrences(
                    &document.document,
                    &member_symbols,
                    &declarations_by_canonical,
                );
            }
        }

        let active_support_paths = self.active_support_paths.iter().cloned().collect::<Vec<_>>();
        for path in active_support_paths {
            if let Some(document) = self.support_documents.get_mut(&path)
                && (force_full || module_keys.contains(&document.source.logical_module))
            {
                document.references = collect_reference_occurrences(
                    &document.document,
                    &member_symbols,
                    &declarations_by_canonical,
                );
            }
        }
    }

    pub(super) fn update_workspace_state(
        &mut self,
        diagnostics: DiagnosticReport,
        graph: ModuleGraph,
        module_keys: &BTreeSet<String>,
        force_full: bool,
    ) {
        self.last_state_refresh_was_full = force_full;
        if force_full {
            self.state = self.assemble_workspace_state(diagnostics, graph);
            return;
        }

        let active_documents = self
            .active_cached_documents()
            .into_iter()
            .map(|document| {
                (
                    document.document.uri.clone(),
                    document.document.clone(),
                    document.declarations.clone(),
                    document.references.clone(),
                )
            })
            .collect::<Vec<QueryDocumentState>>();
        let active_uris =
            active_documents.iter().map(|(uri, _, _, _)| uri.clone()).collect::<BTreeSet<_>>();
        let removed_documents = self
            .state
            .documents
            .iter()
            .filter(|document| !active_uris.contains(&document.uri))
            .cloned()
            .collect::<Vec<_>>();
        if !removed_documents.is_empty() {
            let removed_uris = removed_documents
                .iter()
                .map(|document| document.uri.clone())
                .collect::<BTreeSet<_>>();
            self.state.documents.retain(|document| !removed_uris.contains(&document.uri));
            self.state
                .declarations_by_canonical
                .retain(|_, occurrence| !removed_uris.contains(&occurrence.uri));
            self.state.occurrences.retain(|occurrence| !removed_uris.contains(&occurrence.uri));
        }

        let known_uris = self
            .state
            .documents
            .iter()
            .map(|document| document.uri.clone())
            .collect::<BTreeSet<_>>();
        let changed_documents = active_documents
            .into_iter()
            .filter(|(uri, document, _, _)| {
                !known_uris.contains(uri)
                    || module_keys.contains(&document.syntax.source.logical_module)
            })
            .collect::<Vec<_>>();
        if changed_documents.is_empty() {
            self.state.diagnostics_by_uri = diagnostics_by_uri(&self.state.documents, &diagnostics);
            refresh_workspace_queries(
                &mut self.state.queries,
                &removed_documents,
                &changed_documents,
                &graph,
                module_keys,
            );
            self.state.graph = graph;
            return;
        }

        for (uri, updated_document, declarations, references) in &changed_documents {
            if let Some(document) =
                self.state.documents.iter_mut().find(|existing| existing.uri == *uri)
            {
                *document = updated_document.clone();
            } else {
                self.state.documents.push(updated_document.clone());
            }

            self.state.declarations_by_canonical.retain(|_, occurrence| occurrence.uri != *uri);
            for occurrence in declarations {
                self.state
                    .declarations_by_canonical
                    .entry(occurrence.canonical.clone())
                    .or_insert_with(|| occurrence.clone());
            }

            self.state.occurrences.retain(|occurrence| occurrence.uri != *uri);
            let mut occurrences =
                declarations.iter().chain(references.iter()).cloned().collect::<Vec<_>>();
            dedupe_occurrences(&mut occurrences);
            self.state.occurrences.extend(occurrences);
        }

        self.state.documents.sort_by(|left, right| left.path.cmp(&right.path));
        self.state.diagnostics_by_uri = diagnostics_by_uri(&self.state.documents, &diagnostics);
        refresh_workspace_queries(
            &mut self.state.queries,
            &removed_documents,
            &changed_documents,
            &graph,
            module_keys,
        );
        self.state.graph = graph;
    }

    pub(super) fn assemble_workspace_state(
        &self,
        diagnostics: DiagnosticReport,
        graph: ModuleGraph,
    ) -> WorkspaceState {
        let active_documents = self.active_cached_documents();
        let documents =
            active_documents.iter().map(|document| document.document.clone()).collect::<Vec<_>>();
        let declarations_by_canonical = declarations_by_canonical_from_documents(&active_documents);
        let mut occurrences = active_documents
            .iter()
            .flat_map(|document| {
                document
                    .declarations
                    .iter()
                    .chain(document.references.iter())
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        dedupe_occurrences(&mut occurrences);

        let diagnostics_by_uri = diagnostics_by_uri(&documents, &diagnostics);
        let queries = build_workspace_queries(&documents, &occurrences, &graph);
        WorkspaceState {
            documents,
            diagnostics_by_uri,
            occurrences,
            declarations_by_canonical,
            graph,
            queries,
        }
    }
}

pub(super) fn empty_workspace_state() -> WorkspaceState {
    WorkspaceState {
        documents: Vec::new(),
        diagnostics_by_uri: BTreeMap::new(),
        occurrences: Vec::new(),
        declarations_by_canonical: BTreeMap::new(),
        graph: ModuleGraph::default(),
        queries: WorkspaceQueries::default(),
    }
}

pub(super) fn parse_discovered_source(
    source: &DiscoveredSource,
    overlay_text: Option<&str>,
    enable_conditional_returns: bool,
) -> Result<SyntaxTree, LspError> {
    let mut source_file = if let Some(text) = overlay_text {
        SourceFile {
            path: source.path.clone(),
            kind: source.kind,
            logical_module: source.logical_module.clone(),
            text: text.to_owned(),
        }
    } else {
        let mut source_file = SourceFile::from_path(&source.path)
            .with_context(|| format!("unable to read {}", source.path.display()))?;
        source_file.logical_module = source.logical_module.clone();
        source_file
    };
    source_file.logical_module = source.logical_module.clone();
    Ok(parse_with_options(source_file, ParseOptions { enable_conditional_returns }))
}

pub(super) fn source_root_for_path_from_roots(
    source_roots: &[PathBuf],
    path: &Path,
) -> Option<PathBuf> {
    source_roots.iter().find(|root| path.starts_with(root)).cloned()
}

pub(super) fn index_document_state(syntax: SyntaxTree) -> (DocumentState, Vec<SymbolOccurrence>) {
    let text = syntax.source.text.clone();
    let uri = path_to_uri(&syntax.source.path);
    let mut document = DocumentState {
        uri,
        path: syntax.source.path.clone(),
        text,
        syntax,
        local_symbols: BTreeMap::new(),
        local_value_types: BTreeMap::new(),
    };
    let (local_symbols, declarations) = collect_declarations(&document);
    let local_value_types = collect_local_value_types(&document, &local_symbols);
    document.local_symbols = local_symbols;
    document.local_value_types = local_value_types;
    (document, declarations)
}

pub(super) fn declarations_by_canonical_from_documents(
    documents: &[&CachedDocument],
) -> BTreeMap<String, SymbolOccurrence> {
    let mut declarations_by_canonical = BTreeMap::new();
    for occurrence in documents.iter().flat_map(|document| document.declarations.iter()) {
        declarations_by_canonical
            .entry(occurrence.canonical.clone())
            .or_insert_with(|| occurrence.clone());
    }
    declarations_by_canonical
}

pub(super) fn member_symbols_from_documents(
    documents: &[&CachedDocument],
) -> BTreeMap<String, Vec<String>> {
    let mut member_symbols = BTreeMap::<String, Vec<String>>::new();
    for occurrence in documents.iter().flat_map(|document| document.declarations.iter()) {
        if occurrence.canonical.matches('.').count() >= 2 {
            member_symbols
                .entry(occurrence.name.clone())
                .or_default()
                .push(occurrence.canonical.clone());
        }
    }
    member_symbols
}

pub(super) fn build_workspace_queries(
    documents: &[DocumentState],
    occurrences: &[SymbolOccurrence],
    graph: &ModuleGraph,
) -> WorkspaceQueries {
    let mut queries = WorkspaceQueries::default();
    for document in documents {
        insert_workspace_query_document(&mut queries, document);
    }
    insert_workspace_query_occurrences(&mut queries, occurrences);
    for node in &graph.nodes {
        queries.nodes_by_module_key.insert(node.module_key.clone(), node.clone());
    }
    queries
}

pub(super) fn refresh_workspace_queries(
    queries: &mut WorkspaceQueries,
    removed_documents: &[DocumentState],
    changed_documents: &[QueryDocumentState],
    graph: &ModuleGraph,
    module_keys: &BTreeSet<String>,
) {
    for document in removed_documents {
        queries.documents_by_uri.remove(&document.uri);
        queries.documents_by_module_key.remove(&document.syntax.source.logical_module);
        remove_workspace_query_occurrences_for_uri(queries, &document.uri);
    }

    for (_, document, declarations, references) in changed_documents {
        insert_workspace_query_document(queries, document);
        remove_workspace_query_occurrences_for_uri(queries, &document.uri);
        let mut occurrences =
            declarations.iter().chain(references.iter()).cloned().collect::<Vec<_>>();
        dedupe_occurrences(&mut occurrences);
        insert_workspace_query_occurrences(queries, &occurrences);
    }

    let current_module_keys =
        graph.nodes.iter().map(|node| node.module_key.clone()).collect::<BTreeSet<_>>();
    queries.nodes_by_module_key.retain(|module_key, _| current_module_keys.contains(module_key));
    for node in &graph.nodes {
        if module_keys.contains(&node.module_key)
            || !queries.nodes_by_module_key.contains_key(&node.module_key)
        {
            queries.nodes_by_module_key.insert(node.module_key.clone(), node.clone());
        }
    }
}

pub(super) fn insert_workspace_query_document(
    queries: &mut WorkspaceQueries,
    document: &DocumentState,
) {
    queries.documents_by_uri.insert(document.uri.clone(), document.clone());
    queries
        .documents_by_module_key
        .insert(document.syntax.source.logical_module.clone(), document.clone());
}

pub(super) fn insert_workspace_query_occurrences(
    queries: &mut WorkspaceQueries,
    occurrences: &[SymbolOccurrence],
) {
    for occurrence in occurrences {
        queries
            .occurrences_by_uri
            .entry(occurrence.uri.clone())
            .or_default()
            .push(occurrence.clone());
        queries
            .occurrences_by_canonical
            .entry(occurrence.canonical.clone())
            .or_default()
            .push(occurrence.clone());
    }
}

pub(super) fn remove_workspace_query_occurrences_for_uri(
    queries: &mut WorkspaceQueries,
    uri: &str,
) {
    let Some(previous) = queries.occurrences_by_uri.remove(uri) else {
        return;
    };
    let mut emptied = Vec::new();
    for occurrence in previous {
        if let Some(entries) = queries.occurrences_by_canonical.get_mut(&occurrence.canonical) {
            entries
                .retain(|entry| !(entry.uri == occurrence.uri && entry.range == occurrence.range));
            if entries.is_empty() {
                emptied.push(occurrence.canonical.clone());
            }
        }
    }
    for canonical in emptied {
        queries.occurrences_by_canonical.remove(&canonical);
    }
}

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

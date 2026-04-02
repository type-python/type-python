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


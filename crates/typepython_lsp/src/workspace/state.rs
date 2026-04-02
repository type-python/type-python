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


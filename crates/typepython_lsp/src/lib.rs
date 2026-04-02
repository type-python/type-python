use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use anyhow::{Context, Result};
use glob::Pattern;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use typepython_binding::{BindingTable, bind};
use typepython_checking::check_modules_with_source_overrides;
use typepython_config::ConfigHandle;
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Severity};
use typepython_graph::{ModuleGraph, ModuleNode, build};
use typepython_incremental::{
    IncrementalState, ModuleDependencyIndex, affected_modules, dependency_index, diff, snapshot,
    snapshot_diff_modules,
};
use typepython_syntax::{
    NamedBlockStatement, ParseOptions, SourceFile, SourceKind, SyntaxStatement, SyntaxTree,
    apply_type_ignore_directives, parse_with_options,
};
use url::Url;

#[derive(Debug, Error)]
pub enum LspError {
    #[error("{0}")]
    Other(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<anyhow::Error> for LspError {
    fn from(value: anyhow::Error) -> Self {
        Self::Other(value.to_string())
    }
}

pub fn serve(config: &ConfigHandle) -> Result<(), LspError> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut server = Server::new(config.clone());
    server.serve(stdin.lock(), stdout.lock())
}

#[derive(Debug, Clone)]
struct OverlayDocument {
    uri: String,
    text: String,
    version: i64,
}

#[derive(Debug, Clone)]
struct DiscoveredSource {
    path: PathBuf,
    kind: SourceKind,
    logical_module: String,
}

#[derive(Debug, Clone)]
struct DocumentState {
    uri: String,
    path: PathBuf,
    text: String,
    syntax: SyntaxTree,
    local_symbols: BTreeMap<String, String>,
    local_value_types: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct SymbolOccurrence {
    canonical: String,
    name: String,
    uri: String,
    range: LspRange,
    detail: String,
    declaration: bool,
}

#[derive(Debug, Clone)]
struct WorkspaceState {
    documents: Vec<DocumentState>,
    diagnostics_by_uri: BTreeMap<String, Vec<LspDiagnostic>>,
    occurrences: Vec<SymbolOccurrence>,
    declarations_by_canonical: BTreeMap<String, SymbolOccurrence>,
    graph: ModuleGraph,
}

#[derive(Debug, Clone)]
struct CachedDocument {
    source: DiscoveredSource,
    binding: BindingTable,
    document: DocumentState,
    declarations: Vec<SymbolOccurrence>,
    references: Vec<SymbolOccurrence>,
}

#[derive(Debug, Clone, Default)]
struct SupportSourceCatalog {
    sources_by_module: BTreeMap<String, Vec<DiscoveredSource>>,
}

#[derive(Debug)]
struct IncrementalWorkspace {
    config: ConfigHandle,
    include_patterns: Vec<Pattern>,
    exclude_patterns: Vec<Pattern>,
    source_roots: Vec<PathBuf>,
    support_catalog: SupportSourceCatalog,
    project_documents: BTreeMap<PathBuf, CachedDocument>,
    support_documents: BTreeMap<PathBuf, CachedDocument>,
    active_support_paths: BTreeSet<PathBuf>,
    check_diagnostics_by_module: BTreeMap<String, Vec<Diagnostic>>,
    incremental: IncrementalState,
    dependency_index: ModuleDependencyIndex,
    parse_blocked: bool,
    state: WorkspaceState,
    last_state_refresh_was_full: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
struct LspPosition {
    line: u32,
    character: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
struct LspRange {
    start: LspPosition,
    end: LspPosition,
}

#[derive(Debug, Clone, Serialize)]
struct LspLocation {
    uri: String,
    range: LspRange,
}

#[derive(Debug, Clone, Serialize)]
struct LspDiagnostic {
    range: LspRange,
    severity: u8,
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LspTextEdit {
    range: LspRange,
    new_text: String,
}

#[derive(Debug, Clone, Serialize)]
struct LspParameterInformation {
    label: String,
}

#[derive(Debug, Clone, Serialize)]
struct LspSignatureInformation {
    label: String,
    parameters: Vec<LspParameterInformation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LspSignatureHelp {
    signatures: Vec<LspSignatureInformation>,
    active_signature: usize,
    active_parameter: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LspDocumentSymbol {
    name: String,
    kind: u32,
    range: LspRange,
    selection_range: LspRange,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<LspDocumentSymbol>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LspContentChangeEvent {
    #[serde(default)]
    range: Option<LspRange>,
    #[serde(default)]
    range_length: Option<u32>,
    text: String,
}

impl SupportSourceCatalog {
    fn new(config: &ConfigHandle) -> Result<Self, LspError> {
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
    fn new(
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

    fn workspace(&self) -> &WorkspaceState {
        &self.state
    }

    fn apply_project_path_update(
        &mut self,
        path: &Path,
        overlay: Option<&OverlayDocument>,
    ) -> Result<(), LspError> {
        let direct_changes = self.update_project_document(path, overlay)?;
        self.sync_support_documents()?;
        self.rebuild_state(direct_changes, false)
    }

    fn update_project_document(
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

    fn project_source_for_path(&self, path: &Path) -> Result<Option<DiscoveredSource>, LspError> {
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

    fn sync_support_documents(&mut self) -> Result<(), LspError> {
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

    fn ensure_support_document(&mut self, source: &DiscoveredSource) -> Result<(), LspError> {
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

    fn active_cached_documents(&self) -> Vec<&CachedDocument> {
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

    fn active_syntax_trees(&self) -> Vec<SyntaxTree> {
        self.active_cached_documents()
            .into_iter()
            .map(|document| document.document.syntax.clone())
            .collect()
    }

    fn active_bindings(&self) -> Vec<BindingTable> {
        self.active_cached_documents()
            .into_iter()
            .map(|document| document.binding.clone())
            .collect()
    }

    fn active_source_overrides(&self) -> BTreeMap<String, String> {
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

    fn rebuild_state(
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

    fn rebuild_document_indexes(&mut self, module_keys: &BTreeSet<String>, force_full: bool) {
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

    fn update_workspace_state(
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
            .collect::<Vec<_>>();
        let active_uris =
            active_documents.iter().map(|(uri, _, _, _)| uri.clone()).collect::<BTreeSet<_>>();
        let removed_uris = self
            .state
            .documents
            .iter()
            .map(|document| document.uri.clone())
            .filter(|uri| !active_uris.contains(uri))
            .collect::<Vec<_>>();
        if !removed_uris.is_empty() {
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
            self.state.graph = graph;
            return;
        }

        for (uri, updated_document, declarations, references) in changed_documents {
            if let Some(document) =
                self.state.documents.iter_mut().find(|existing| existing.uri == uri)
            {
                *document = updated_document.clone();
            } else {
                self.state.documents.push(updated_document.clone());
            }

            self.state.declarations_by_canonical.retain(|_, occurrence| occurrence.uri != uri);
            for occurrence in &declarations {
                self.state
                    .declarations_by_canonical
                    .entry(occurrence.canonical.clone())
                    .or_insert_with(|| occurrence.clone());
            }

            self.state.occurrences.retain(|occurrence| occurrence.uri != uri);
            let mut occurrences =
                declarations.iter().chain(references.iter()).cloned().collect::<Vec<_>>();
            dedupe_occurrences(&mut occurrences);
            self.state.occurrences.extend(occurrences);
        }

        self.state.documents.sort_by(|left, right| left.path.cmp(&right.path));
        self.state.diagnostics_by_uri = diagnostics_by_uri(&self.state.documents, &diagnostics);
        self.state.graph = graph;
    }

    fn assemble_workspace_state(
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
        WorkspaceState {
            documents,
            diagnostics_by_uri,
            occurrences,
            declarations_by_canonical,
            graph,
        }
    }
}

fn empty_workspace_state() -> WorkspaceState {
    WorkspaceState {
        documents: Vec::new(),
        diagnostics_by_uri: BTreeMap::new(),
        occurrences: Vec::new(),
        declarations_by_canonical: BTreeMap::new(),
        graph: ModuleGraph::default(),
    }
}

fn parse_discovered_source(
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

fn source_root_for_path_from_roots(source_roots: &[PathBuf], path: &Path) -> Option<PathBuf> {
    source_roots.iter().find(|root| path.starts_with(root)).cloned()
}

fn index_document_state(syntax: SyntaxTree) -> (DocumentState, Vec<SymbolOccurrence>) {
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

fn declarations_by_canonical_from_documents(
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

fn member_symbols_from_documents(documents: &[&CachedDocument]) -> BTreeMap<String, Vec<String>> {
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

struct Server {
    config: ConfigHandle,
    overlays: BTreeMap<PathBuf, OverlayDocument>,
    cached_workspace: Option<IncrementalWorkspace>,
    shutdown_requested: bool,
    exited: bool,
}

impl Server {
    fn new(config: ConfigHandle) -> Self {
        Self {
            config,
            overlays: BTreeMap::new(),
            cached_workspace: None,
            shutdown_requested: false,
            exited: false,
        }
    }

    fn serve<R: BufRead, W: Write>(
        &mut self,
        mut reader: R,
        mut writer: W,
    ) -> Result<(), LspError> {
        while let Some(message) = read_message(&mut reader)? {
            let responses = self.handle_message(message)?;
            for response in responses {
                write_message(&mut writer, &response)?;
            }
            writer.flush()?;
            if self.exited {
                break;
            }
        }

        Ok(())
    }

    fn handle_message(&mut self, message: Value) -> Result<Vec<Value>, LspError> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(Vec::new());
        };
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "capabilities": {
                        "textDocumentSync": {
                            "openClose": true,
                            "change": 2
                        },
                        "hoverProvider": true,
                        "definitionProvider": true,
                        "referencesProvider": true,
                        "signatureHelpProvider": {
                            "triggerCharacters": ["(", ","]
                        },
                        "documentSymbolProvider": true,
                        "renameProvider": true,
                        "codeActionProvider": true,
                        "completionProvider": {
                            "resolveProvider": false,
                            "triggerCharacters": ["."]
                        }
                    },
                    "serverInfo": {
                        "name": "typepython",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            })]),
            "initialized" => Ok(Vec::new()),
            "shutdown" => {
                self.shutdown_requested = true;
                Ok(vec![json!({"jsonrpc": "2.0", "id": id, "result": Value::Null})])
            }
            "exit" => {
                self.exited = true;
                Ok(Vec::new())
            }
            "textDocument/didOpen" => {
                self.apply_did_open(params)?;
                self.publish_diagnostics()
            }
            "textDocument/didChange" => {
                self.apply_did_change(params)?;
                self.publish_diagnostics()
            }
            "textDocument/didClose" => self.apply_did_close(params),
            "textDocument/hover" => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": self.handle_hover(params)?
            })]),
            "textDocument/definition" => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": self.handle_definition(params)?
            })]),
            "textDocument/references" => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": self.handle_references(params)?
            })]),
            "textDocument/signatureHelp" => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": self.handle_signature_help(params)?
            })]),
            "textDocument/documentSymbol" => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": self.handle_document_symbol(params)?
            })]),
            "textDocument/rename" => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": self.handle_rename(params)?
            })]),
            "textDocument/codeAction" => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": self.handle_code_action(params)?
            })]),
            "textDocument/completion" => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": self.handle_completion(params)?
            })]),
            _ if id.is_some() => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": Value::Null
            })]),
            _ => Ok(Vec::new()),
        }
    }

    fn apply_did_open(&mut self, params: Value) -> Result<(), LspError> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| LspError::Other(String::from("didOpen missing textDocument")))?;
        let uri = text_document
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Other(String::from("didOpen missing uri")))?;
        let text = text_document
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Other(String::from("didOpen missing text")))?;
        let version = text_document.get("version").and_then(Value::as_i64).ok_or_else(|| {
            LspError::Other(String::from("TPY6002: didOpen missing document version"))
        })?;
        let path = uri_to_path(uri)?;
        self.overlays.insert(
            path.clone(),
            OverlayDocument { uri: uri.to_owned(), text: text.to_owned(), version },
        );
        if let Some(workspace) = self.cached_workspace.as_mut() {
            let overlay =
                self.overlays.get(&path).expect("opened overlay should be available in cache");
            workspace.apply_project_path_update(&path, Some(overlay))?;
        }
        Ok(())
    }

    fn apply_did_change(&mut self, params: Value) -> Result<(), LspError> {
        let text_document = params
            .get("textDocument")
            .ok_or_else(|| LspError::Other(String::from("didChange missing textDocument")))?;
        let uri = text_document
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Other(String::from("didChange missing uri")))?;
        let version = text_document.get("version").and_then(Value::as_i64).ok_or_else(|| {
            LspError::Other(String::from("TPY6002: didChange missing document version"))
        })?;
        let path = uri_to_path(uri)?;
        let current = self.overlays.get(&path).ok_or_else(|| {
            LspError::Other(format!("TPY6002: didChange received for unopened overlay `{}`", uri))
        })?;
        if version <= current.version {
            return Err(LspError::Other(format!(
                "TPY6002: didChange version {} is out of sync with overlay version {} for `{}`",
                version, current.version, uri
            )));
        }
        let content_changes: Vec<LspContentChangeEvent> =
            serde_json::from_value(params.get("contentChanges").cloned().ok_or_else(|| {
                LspError::Other(String::from("didChange missing contentChanges"))
            })?)?;
        if content_changes.is_empty() {
            return Err(LspError::Other(format!(
                "TPY6002: didChange received no content changes for `{}`",
                uri
            )));
        }
        let text = apply_content_changes(&current.text, &content_changes, uri)?;
        self.overlays.insert(path.clone(), OverlayDocument { uri: uri.to_owned(), text, version });
        if let Some(workspace) = self.cached_workspace.as_mut() {
            let overlay =
                self.overlays.get(&path).expect("changed overlay should be available in cache");
            workspace.apply_project_path_update(&path, Some(overlay))?;
        }
        Ok(())
    }

    fn apply_did_close(&mut self, params: Value) -> Result<Vec<Value>, LspError> {
        let uri = params
            .get("textDocument")
            .and_then(|document| document.get("uri"))
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Other(String::from("didClose missing uri")))?;
        let path = uri_to_path(uri)?;
        if self.overlays.remove(&path).is_none() {
            return Err(LspError::Other(format!(
                "TPY6002: didClose received for unopened overlay `{}`",
                uri
            )));
        }
        if let Some(workspace) = self.cached_workspace.as_mut() {
            workspace.apply_project_path_update(&path, None)?;
        }
        Ok(vec![publish_diagnostics_notification(uri, Vec::new())])
    }

    fn publish_diagnostics(&mut self) -> Result<Vec<Value>, LspError> {
        let workspace = self.workspace()?;
        let mut notifications = workspace
            .diagnostics_by_uri
            .iter()
            .map(|(uri, diagnostics)| publish_diagnostics_notification(uri, diagnostics.clone()))
            .collect::<Vec<_>>();

        for overlay in self.overlays.values() {
            if !notifications.iter().any(|notification| {
                notification
                    .get("params")
                    .and_then(|params| params.get("uri"))
                    .and_then(Value::as_str)
                    == Some(overlay.uri.as_str())
            }) {
                notifications.push(publish_diagnostics_notification(&overlay.uri, Vec::new()));
            }
        }

        Ok(notifications)
    }

    fn handle_hover(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let Some(symbol) = resolve_symbol(workspace, &uri, position) else {
            return Ok(Value::Null);
        };
        let detail = workspace
            .declarations_by_canonical
            .get(&symbol.canonical)
            .map(|declaration| declaration.detail.clone())
            .unwrap_or_else(|| symbol.detail.clone());
        Ok(json!({
            "contents": {
                "kind": "markdown",
                "value": format!("```typepython\n{}\n```", detail)
            },
            "range": symbol.range
        }))
    }

    fn handle_definition(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let Some(symbol) = resolve_symbol(workspace, &uri, position) else {
            return Ok(Value::Null);
        };
        let Some(declaration) = workspace.declarations_by_canonical.get(&symbol.canonical) else {
            return Ok(Value::Null);
        };
        Ok(json!([LspLocation { uri: declaration.uri.clone(), range: declaration.range }]))
    }

    fn handle_references(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let include_declaration = params
            .get("context")
            .and_then(|context| context.get("includeDeclaration"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let Some(symbol) = resolve_symbol(workspace, &uri, position) else {
            return Ok(json!([]));
        };
        let references = workspace
            .occurrences
            .iter()
            .filter(|occurrence| occurrence.canonical == symbol.canonical)
            .filter(|occurrence| include_declaration || !occurrence.declaration)
            .map(|occurrence| LspLocation { uri: occurrence.uri.clone(), range: occurrence.range })
            .collect::<Vec<_>>();
        Ok(json!(references))
    }

    fn handle_signature_help(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let Some(document) = workspace.documents.iter().find(|document| document.uri == uri) else {
            return Ok(Value::Null);
        };
        let Some(active_call) = active_call(document, position, &uri)? else {
            return Ok(Value::Null);
        };
        let signatures =
            resolve_signature_information(workspace, document, position, &active_call.callee);
        if signatures.is_empty() {
            return Ok(Value::Null);
        }
        let active_signature = 0usize;
        let active_parameter = signatures[active_signature]
            .parameters
            .len()
            .saturating_sub(1)
            .min(active_call.active_parameter);
        Ok(json!(LspSignatureHelp { signatures, active_signature, active_parameter }))
    }

    fn handle_document_symbol(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let uri = params
            .get("textDocument")
            .and_then(|document| document.get("uri"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                LspError::Other(String::from("textDocument/documentSymbol request missing uri"))
            })?;
        let Some(document) = workspace.documents.iter().find(|document| document.uri == uri) else {
            return Ok(json!([]));
        };
        Ok(json!(collect_document_symbols(document)))
    }

    fn handle_rename(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let new_name = params
            .get("newName")
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Other(String::from("rename missing newName")))?;
        let Some(symbol) = resolve_symbol(workspace, &uri, position) else {
            return Ok(Value::Null);
        };
        let mut changes = BTreeMap::<String, Vec<LspTextEdit>>::new();
        for occurrence in workspace
            .occurrences
            .iter()
            .filter(|occurrence| occurrence.canonical == symbol.canonical)
        {
            changes
                .entry(occurrence.uri.clone())
                .or_default()
                .push(LspTextEdit { range: occurrence.range, new_text: new_name.to_owned() });
        }
        Ok(json!({"changes": changes}))
    }

    fn handle_code_action(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let (uri, range) = text_document_range(&params)?;
        let Some(document) = workspace.documents.iter().find(|document| document.uri == uri) else {
            return Ok(json!([]));
        };

        let mut actions = Vec::new();
        actions.extend(collect_diagnostic_suggestion_code_actions(document, range, &params));
        actions.extend(collect_missing_annotation_code_actions(workspace, document, range));
        actions.extend(collect_unsafe_code_actions(document, range, &params));
        actions.extend(collect_missing_import_code_actions(workspace, document, range));
        Ok(json!(actions))
    }

    fn handle_completion(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let Some(document) = workspace.documents.iter().find(|document| document.uri == uri) else {
            return Ok(json!([]));
        };
        let is_member_access = line_prefix(&document.text, position).trim_end().ends_with('.');

        let items = if is_member_access {
            collect_member_completion_items(workspace, document, position)
        } else {
            let mut keys = document.local_symbols.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            keys.into_iter()
                .map(|name| {
                    let canonical = &document.local_symbols[&name];
                    let detail = workspace
                        .declarations_by_canonical
                        .get(canonical)
                        .map(|occurrence| occurrence.detail.clone())
                        .unwrap_or_else(|| canonical.clone());
                    json!({"label": name, "detail": detail})
                })
                .collect::<Vec<_>>()
        };

        Ok(json!({"isIncomplete": false, "items": items}))
    }

    fn workspace(&mut self) -> Result<&WorkspaceState, LspError> {
        if self.cached_workspace.is_none() {
            self.cached_workspace =
                Some(IncrementalWorkspace::new(self.config.clone(), &self.overlays)?);
        }
        Ok(self.cached_workspace.as_ref().expect("workspace cache should be populated").workspace())
    }
}

#[derive(Debug, Clone)]
struct ActiveCall {
    callee: String,
    active_parameter: usize,
}

fn active_call(
    document: &DocumentState,
    position: LspPosition,
    uri: &str,
) -> Result<Option<ActiveCall>, LspError> {
    let offset = lsp_position_to_byte_offset(&document.text, position, uri)?;
    let prefix = &document.text[..offset];
    let Some((open_offset, active_parameter)) = active_call_open(prefix) else {
        return Ok(None);
    };
    let Some(callee) = call_callee_before_offset(prefix, open_offset) else {
        return Ok(None);
    };
    Ok(Some(ActiveCall { callee, active_parameter }))
}

fn active_call_open(prefix: &str) -> Option<(usize, usize)> {
    let mut paren_stack = Vec::<(usize, usize)>::new();
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;

    for (offset, ch) in prefix.char_indices() {
        match ch {
            '(' => paren_stack.push((offset, 0)),
            ')' => {
                paren_stack.pop();
            }
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if bracket_depth == 0 && brace_depth == 0 => {
                if let Some((_, active_parameter)) = paren_stack.last_mut() {
                    *active_parameter += 1;
                }
            }
            _ => {}
        }
    }

    paren_stack.pop()
}

fn call_callee_before_offset(prefix: &str, open_offset: usize) -> Option<String> {
    let before = prefix[..open_offset].trim_end();
    if before.is_empty() {
        return None;
    }

    let mut start = before.len();
    let mut generic_depth = 0usize;
    for (offset, ch) in before.char_indices().rev() {
        match ch {
            ']' => {
                generic_depth += 1;
                start = offset;
            }
            '[' => {
                if generic_depth == 0 {
                    break;
                }
                generic_depth -= 1;
                start = offset;
            }
            _ if generic_depth > 0 => {
                start = offset;
            }
            _ if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' => {
                start = offset;
            }
            _ => break,
        }
    }

    let callee = before[start..].trim();
    (!callee.is_empty()).then(|| callee.to_owned())
}

fn resolve_signature_information(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
    callee: &str,
) -> Vec<LspSignatureInformation> {
    let normalized = strip_generic_args(callee).trim();
    if normalized.is_empty() {
        return Vec::new();
    }

    if let Some((receiver, member_name)) = normalized.rsplit_once('.') {
        return resolve_member_signature_information(
            workspace,
            document,
            position,
            receiver.trim(),
            member_name.trim(),
        );
    }

    let mut signatures = if let Some(canonical) = document.local_symbols.get(normalized) {
        signature_information_for_canonical(workspace, canonical)
    } else {
        Vec::new()
    };
    if signatures.is_empty() {
        let (_, owner_type_name) = scope_context_at_position(document, position);
        if let Some(owner_type_name) = owner_type_name {
            signatures.extend(class_member_signature_information(
                document,
                &owner_type_name,
                normalized,
            ));
        }
    }
    signatures
}

fn resolve_member_signature_information(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
    receiver: &str,
    member_name: &str,
) -> Vec<LspSignatureInformation> {
    let mut owner_canonicals = Vec::new();
    if let Some(canonical) = document.local_symbols.get(receiver) {
        push_unique(&mut owner_canonicals, canonical.clone());
    }
    if let Some(type_text) =
        resolve_visible_name_type_text(workspace, document, position, receiver, 0)
    {
        for canonical in resolve_type_canonicals(workspace, document, &type_text) {
            push_unique(&mut owner_canonicals, canonical);
        }
    }

    owner_canonicals
        .into_iter()
        .flat_map(|owner_canonical| {
            signature_information_for_canonical(
                workspace,
                &format!("{owner_canonical}.{member_name}"),
            )
        })
        .collect()
}

fn signature_information_for_canonical(
    workspace: &WorkspaceState,
    canonical: &str,
) -> Vec<LspSignatureInformation> {
    let Some(declaration) = workspace.declarations_by_canonical.get(canonical) else {
        return Vec::new();
    };
    let Some(document) =
        workspace.documents.iter().find(|document| document.uri == declaration.uri)
    else {
        return Vec::new();
    };
    let Some((owner_canonical, member_name)) = canonical.rsplit_once('.') else {
        return Vec::new();
    };
    if workspace.declarations_by_canonical.contains_key(owner_canonical) {
        let owner_name =
            owner_canonical.rsplit_once('.').map(|(_, name)| name).unwrap_or(owner_canonical);
        return class_member_signature_information(document, owner_name, member_name);
    }
    top_level_signature_information(document, member_name)
}

fn top_level_signature_information(
    document: &DocumentState,
    name: &str,
) -> Vec<LspSignatureInformation> {
    let signatures = document
        .syntax
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::FunctionDef(function) | SyntaxStatement::OverloadDef(function)
                if function.name == name =>
            {
                Some(signature_information(
                    &function.name,
                    &function.params,
                    function.returns.as_deref(),
                    false,
                ))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if !signatures.is_empty() {
        return signatures;
    }

    document
        .syntax
        .statements
        .iter()
        .find_map(|statement| match statement {
            SyntaxStatement::Interface(class_like)
            | SyntaxStatement::DataClass(class_like)
            | SyntaxStatement::SealedClass(class_like)
            | SyntaxStatement::ClassDef(class_like)
                if class_like.name == name =>
            {
                Some(class_constructor_signature_information(class_like))
            }
            _ => None,
        })
        .into_iter()
        .collect()
}

fn class_member_signature_information(
    document: &DocumentState,
    owner_name: &str,
    member_name: &str,
) -> Vec<LspSignatureInformation> {
    document
        .syntax
        .statements
        .iter()
        .find_map(|statement| match statement {
            SyntaxStatement::Interface(class_like)
            | SyntaxStatement::DataClass(class_like)
            | SyntaxStatement::SealedClass(class_like)
            | SyntaxStatement::ClassDef(class_like)
                if class_like.name == owner_name =>
            {
                Some(
                    class_like
                        .members
                        .iter()
                        .filter(|member| member.name == member_name)
                        .filter(|member| member.kind != typepython_syntax::ClassMemberKind::Field)
                        .map(|member| {
                            let drop_first = member
                                .method_kind
                                .is_some_and(|kind| kind != typepython_syntax::MethodKind::Static);
                            signature_information(
                                &format!("{owner_name}.{}", member.name),
                                &member.params,
                                member.returns.as_deref(),
                                drop_first,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn class_constructor_signature_information(
    class_like: &NamedBlockStatement,
) -> LspSignatureInformation {
    let init_signatures = class_like
        .members
        .iter()
        .filter(|member| member.name == "__init__")
        .filter(|member| member.kind != typepython_syntax::ClassMemberKind::Field)
        .map(|member| {
            let drop_first = member
                .method_kind
                .is_some_and(|kind| kind != typepython_syntax::MethodKind::Static);
            signature_information(&class_like.name, &member.params, Some("None"), drop_first)
        })
        .collect::<Vec<_>>();
    if let Some(signature) = init_signatures.into_iter().next() {
        return signature;
    }

    let field_params = class_like
        .members
        .iter()
        .filter(|member| {
            member.kind == typepython_syntax::ClassMemberKind::Field && !member.is_class_var
        })
        .map(|member| typepython_syntax::FunctionParam {
            name: member.name.clone(),
            annotation: member.annotation.clone().or_else(|| member.value_type.clone()),
            has_default: false,
            positional_only: false,
            keyword_only: false,
            variadic: false,
            keyword_variadic: false,
        })
        .collect::<Vec<_>>();
    signature_information(&class_like.name, &field_params, Some(&class_like.name), false)
}

fn signature_information(
    name: &str,
    params: &[typepython_syntax::FunctionParam],
    returns: Option<&str>,
    drop_first: bool,
) -> LspSignatureInformation {
    let shown_params = if drop_first {
        params.iter().skip(1).collect::<Vec<_>>()
    } else {
        params.iter().collect::<Vec<_>>()
    };
    let parameter_labels =
        shown_params.iter().map(|param| render_parameter_label(param)).collect::<Vec<_>>();
    LspSignatureInformation {
        label: format!(
            "{}({}){}",
            name,
            parameter_labels.join(", "),
            returns.map(|returns| format!(" -> {returns}")).unwrap_or_default()
        ),
        parameters: parameter_labels
            .into_iter()
            .map(|label| LspParameterInformation { label })
            .collect(),
    }
}

fn render_parameter_label(param: &typepython_syntax::FunctionParam) -> String {
    let mut label = String::new();
    if param.keyword_variadic {
        label.push_str("**");
    } else if param.variadic {
        label.push('*');
    }
    label.push_str(&param.name);
    if let Some(annotation) = &param.annotation {
        label.push_str(": ");
        label.push_str(annotation);
    }
    if param.has_default {
        label.push_str(" = ...");
    }
    label
}

fn collect_document_symbols(document: &DocumentState) -> Vec<LspDocumentSymbol> {
    document
        .syntax
        .statements
        .iter()
        .flat_map(|statement| match statement {
            SyntaxStatement::TypeAlias(statement) => {
                find_name_range(&document.text, statement.line, &statement.name)
                    .map(|selection_range| {
                        vec![LspDocumentSymbol {
                            name: statement.name.clone(),
                            kind: 13,
                            range: single_line_range(&document.text, statement.line),
                            selection_range,
                            detail: Some(statement.value.clone()),
                            children: Vec::new(),
                        }]
                    })
                    .unwrap_or_default()
            }
            SyntaxStatement::Interface(statement) => {
                collect_type_block_symbols(document, statement, 11)
            }
            SyntaxStatement::DataClass(statement)
            | SyntaxStatement::SealedClass(statement)
            | SyntaxStatement::ClassDef(statement) => {
                collect_type_block_symbols(document, statement, 5)
            }
            SyntaxStatement::OverloadDef(statement) | SyntaxStatement::FunctionDef(statement) => {
                find_name_range(&document.text, statement.line, &statement.name)
                    .map(|selection_range| {
                        vec![LspDocumentSymbol {
                            name: statement.name.clone(),
                            kind: 12,
                            range: block_range(&document.text, statement.line),
                            selection_range,
                            detail: Some(format_signature(
                                &statement.params,
                                statement.returns.as_deref(),
                            )),
                            children: Vec::new(),
                        }]
                    })
                    .unwrap_or_default()
            }
            SyntaxStatement::Value(statement) => statement
                .names
                .iter()
                .filter_map(|name| {
                    find_name_range(&document.text, statement.line, name).map(|selection_range| {
                        LspDocumentSymbol {
                            name: name.clone(),
                            kind: value_symbol_kind(name, statement.is_final),
                            range: single_line_range(&document.text, statement.line),
                            selection_range,
                            detail: statement
                                .annotation
                                .clone()
                                .or_else(|| statement.value_type.clone()),
                            children: Vec::new(),
                        }
                    })
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn collect_type_block_symbols(
    document: &DocumentState,
    statement: &NamedBlockStatement,
    kind: u32,
) -> Vec<LspDocumentSymbol> {
    find_name_range(&document.text, statement.line, &statement.name)
        .map(|selection_range| {
            vec![LspDocumentSymbol {
                name: statement.name.clone(),
                kind,
                range: block_range(&document.text, statement.line),
                selection_range,
                detail: (!statement.bases.is_empty()).then(|| statement.bases.join(", ")),
                children: collect_class_member_symbols(document, statement),
            }]
        })
        .unwrap_or_default()
}

fn collect_class_member_symbols(
    document: &DocumentState,
    class_like: &NamedBlockStatement,
) -> Vec<LspDocumentSymbol> {
    class_like
        .members
        .iter()
        .filter_map(|member| {
            find_name_range(&document.text, member.line, &member.name).map(|selection_range| {
                let kind = match member.kind {
                    typepython_syntax::ClassMemberKind::Field => {
                        value_symbol_kind(&member.name, member.is_final)
                    }
                    typepython_syntax::ClassMemberKind::Method
                    | typepython_syntax::ClassMemberKind::Overload => match member.method_kind {
                        Some(typepython_syntax::MethodKind::Property)
                        | Some(typepython_syntax::MethodKind::PropertySetter) => 7,
                        _ => 6,
                    },
                };
                let detail = match member.kind {
                    typepython_syntax::ClassMemberKind::Field => {
                        member.annotation.clone().or_else(|| member.value_type.clone())
                    }
                    typepython_syntax::ClassMemberKind::Method
                    | typepython_syntax::ClassMemberKind::Overload => {
                        Some(format_signature(&member.params, member.returns.as_deref()))
                    }
                };
                LspDocumentSymbol {
                    name: member.name.clone(),
                    kind,
                    range: match member.kind {
                        typepython_syntax::ClassMemberKind::Field => {
                            single_line_range(&document.text, member.line)
                        }
                        typepython_syntax::ClassMemberKind::Method
                        | typepython_syntax::ClassMemberKind::Overload => {
                            block_range(&document.text, member.line)
                        }
                    },
                    selection_range,
                    detail,
                    children: Vec::new(),
                }
            })
        })
        .collect()
}

fn value_symbol_kind(name: &str, is_final: bool) -> u32 {
    if is_final || name.chars().all(|ch| !ch.is_ascii_lowercase()) { 14 } else { 13 }
}

fn single_line_range(text: &str, line: usize) -> LspRange {
    let line_text = text.lines().nth(line.saturating_sub(1)).unwrap_or_default();
    LspRange {
        start: LspPosition { line: line.saturating_sub(1) as u32, character: 0 },
        end: LspPosition {
            line: line.saturating_sub(1) as u32,
            character: line_text.chars().count() as u32,
        },
    }
}

fn block_range(text: &str, line: usize) -> LspRange {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return LspRange {
            start: LspPosition { line: line.saturating_sub(1) as u32, character: 0 },
            end: LspPosition { line: line.saturating_sub(1) as u32, character: 0 },
        };
    }

    let start_index = line.saturating_sub(1).min(lines.len().saturating_sub(1));
    let start_indent = line_indentation(lines[start_index]);
    let mut end_index = start_index;
    for (index, candidate) in lines.iter().enumerate().skip(start_index + 1) {
        if candidate.trim().is_empty() {
            end_index = index;
            continue;
        }
        if line_indentation(candidate) <= start_indent {
            break;
        }
        end_index = index;
    }

    LspRange {
        start: LspPosition { line: start_index as u32, character: 0 },
        end: LspPosition {
            line: end_index as u32,
            character: lines[end_index].chars().count() as u32,
        },
    }
}

fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>, LspError> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                content_length = Some(value.trim().parse::<usize>().map_err(|error| {
                    LspError::Other(format!("invalid Content-Length: {error}"))
                })?);
            }
        }
    }

    let Some(content_length) = content_length else {
        return Ok(None);
    };
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

fn write_message<W: Write>(writer: &mut W, value: &Value) -> Result<(), LspError> {
    let payload = serde_json::to_vec(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n", payload.len())?;
    writer.write_all(&payload)?;
    Ok(())
}

fn publish_diagnostics_notification(uri: &str, diagnostics: Vec<LspDiagnostic>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": uri,
            "diagnostics": diagnostics,
        }
    })
}

fn text_document_position(params: &Value) -> Result<(String, LspPosition), LspError> {
    let uri = params
        .get("textDocument")
        .and_then(|document| document.get("uri"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            LspError::Other(String::from("textDocument/position request missing uri"))
        })?;
    let position: LspPosition =
        serde_json::from_value(params.get("position").cloned().ok_or_else(|| {
            LspError::Other(String::from("textDocument/position request missing position"))
        })?)?;
    Ok((uri.to_owned(), position))
}

fn text_document_range(params: &Value) -> Result<(String, LspRange), LspError> {
    let uri = params
        .get("textDocument")
        .and_then(|document| document.get("uri"))
        .and_then(Value::as_str)
        .ok_or_else(|| LspError::Other(String::from("textDocument/range request missing uri")))?;
    let range: LspRange =
        serde_json::from_value(params.get("range").cloned().ok_or_else(|| {
            LspError::Other(String::from("textDocument/range request missing range"))
        })?)?;
    Ok((uri.to_owned(), range))
}

fn apply_content_changes(
    current_text: &str,
    content_changes: &[LspContentChangeEvent],
    uri: &str,
) -> Result<String, LspError> {
    let mut text = current_text.to_owned();
    for change in content_changes {
        match change.range {
            Some(range) => apply_ranged_change(&mut text, range, &change.text, uri)?,
            None => {
                if change.range_length.is_some() {
                    return Err(LspError::Other(format!(
                        "TPY6002: didChange for `{}` provided rangeLength without range",
                        uri
                    )));
                }
                text = change.text.clone();
            }
        }
    }
    Ok(text)
}

fn apply_ranged_change(
    text: &mut String,
    range: LspRange,
    replacement: &str,
    uri: &str,
) -> Result<(), LspError> {
    let start = lsp_position_to_byte_offset(text, range.start, uri)?;
    let end = lsp_position_to_byte_offset(text, range.end, uri)?;
    if start > end {
        return Err(LspError::Other(format!(
            "TPY6002: didChange for `{}` uses an invalid range with start after end",
            uri
        )));
    }
    text.replace_range(start..end, replacement);
    Ok(())
}

fn lsp_position_to_byte_offset(
    text: &str,
    position: LspPosition,
    uri: &str,
) -> Result<usize, LspError> {
    let mut line_start = 0usize;
    for (line_index, line) in text.split_inclusive('\n').enumerate() {
        let line_number = line_index as u32;
        let line_text = line.strip_suffix('\n').unwrap_or(line);
        if line_number == position.line {
            return utf16_column_to_byte_offset(line_text, line_start, position, uri);
        }
        line_start += line.len();
    }

    let total_lines = text.lines().count() as u32;
    if position.line == total_lines {
        return utf16_column_to_byte_offset(&text[line_start..], line_start, position, uri);
    }

    Err(LspError::Other(format!(
        "TPY6002: didChange for `{}` references line {} beyond the current document",
        uri, position.line
    )))
}

fn utf16_column_to_byte_offset(
    line_text: &str,
    line_start: usize,
    position: LspPosition,
    uri: &str,
) -> Result<usize, LspError> {
    let mut utf16_offset = 0u32;
    for (byte_offset, ch) in line_text.char_indices() {
        if utf16_offset == position.character {
            return Ok(line_start + byte_offset);
        }
        utf16_offset += ch.len_utf16() as u32;
        if utf16_offset > position.character {
            return Err(LspError::Other(format!(
                "TPY6002: didChange for `{}` splits a UTF-16 code point at line {}, character {}",
                uri, position.line, position.character
            )));
        }
    }

    if utf16_offset == position.character {
        Ok(line_start + line_text.len())
    } else {
        Err(LspError::Other(format!(
            "TPY6002: didChange for `{}` references character {} beyond line {}",
            uri, position.character, position.line
        )))
    }
}

fn resolve_symbol<'a>(
    workspace: &'a WorkspaceState,
    uri: &str,
    position: LspPosition,
) -> Option<&'a SymbolOccurrence> {
    workspace
        .occurrences
        .iter()
        .find(|occurrence| occurrence.uri == uri && range_contains(occurrence.range, position))
}

fn range_contains(range: LspRange, position: LspPosition) -> bool {
    (position.line > range.start.line
        || (position.line == range.start.line && position.character >= range.start.character))
        && (position.line < range.end.line
            || (position.line == range.end.line && position.character <= range.end.character))
}

fn range_intersects(left: LspRange, right: LspRange) -> bool {
    !(left.end.line < right.start.line
        || right.end.line < left.start.line
        || (left.end.line == right.start.line && left.end.character < right.start.character)
        || (right.end.line == left.start.line && right.end.character < left.start.character))
}

fn diagnostics_by_uri(
    documents: &[DocumentState],
    diagnostics: &DiagnosticReport,
) -> BTreeMap<String, Vec<LspDiagnostic>> {
    let mut by_uri = documents
        .iter()
        .map(|document| (document.uri.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    let path_to_uri = documents
        .iter()
        .map(|document| (normalize_path_string(&document.path), document.uri.clone()))
        .collect::<BTreeMap<_, _>>();

    for diagnostic in &diagnostics.diagnostics {
        let Some(span) = &diagnostic.span else {
            continue;
        };
        let normalized = normalize_path_string(Path::new(&span.path));
        let Some(uri) = path_to_uri.get(&normalized) else {
            continue;
        };
        by_uri.entry(uri.clone()).or_default().push(LspDiagnostic {
            range: LspRange {
                start: LspPosition {
                    line: span.line.saturating_sub(1) as u32,
                    character: span.column.saturating_sub(1) as u32,
                },
                end: LspPosition {
                    line: span.end_line.saturating_sub(1) as u32,
                    character: span.end_column.saturating_sub(1) as u32,
                },
            },
            severity: match diagnostic.severity {
                Severity::Error => 1,
                Severity::Warning => 2,
                Severity::Note => 3,
            },
            code: diagnostic.code.clone(),
            message: if diagnostic.notes.is_empty() {
                diagnostic.message.clone()
            } else {
                format!("{} ({})", diagnostic.message, diagnostic.notes.join("; "))
            },
            data: (!diagnostic.suggestions.is_empty()).then(|| {
                json!({
                    "suggestions": diagnostic.suggestions,
                })
            }),
        });
    }

    by_uri
}

fn collect_declarations(
    document: &DocumentState,
) -> (BTreeMap<String, String>, Vec<SymbolOccurrence>) {
    let mut local_symbols = BTreeMap::new();
    let mut declarations = Vec::new();
    let module_key = &document.syntax.source.logical_module;

    for statement in &document.syntax.statements {
        match statement {
            SyntaxStatement::TypeAlias(statement) => {
                let canonical = format!("{module_key}.{}", statement.name);
                local_symbols.insert(statement.name.clone(), canonical.clone());
                if let Some(range) =
                    find_name_range(&document.text, statement.line, &statement.name)
                {
                    declarations.push(SymbolOccurrence {
                        canonical,
                        name: statement.name.clone(),
                        uri: document.uri.clone(),
                        range,
                        detail: format!("typealias {} = {}", statement.name, statement.value),
                        declaration: true,
                    });
                }
            }
            SyntaxStatement::Interface(statement)
            | SyntaxStatement::DataClass(statement)
            | SyntaxStatement::SealedClass(statement)
            | SyntaxStatement::ClassDef(statement) => {
                let canonical = format!("{module_key}.{}", statement.name);
                local_symbols.insert(statement.name.clone(), canonical.clone());
                if let Some(range) =
                    find_name_range(&document.text, statement.line, &statement.name)
                {
                    declarations.push(SymbolOccurrence {
                        canonical: canonical.clone(),
                        name: statement.name.clone(),
                        uri: document.uri.clone(),
                        range,
                        detail: format!("class {}({})", statement.name, statement.bases.join(", ")),
                        declaration: true,
                    });
                }
                for member in &statement.members {
                    let member_canonical = format!("{canonical}.{}", member.name);
                    if let Some(range) = find_name_range(&document.text, member.line, &member.name)
                    {
                        declarations.push(SymbolOccurrence {
                            canonical: member_canonical,
                            name: member.name.clone(),
                            uri: document.uri.clone(),
                            range,
                            detail: match member.kind {
                                typepython_syntax::ClassMemberKind::Field => format!(
                                    "field {}: {}",
                                    member.name,
                                    member.annotation.clone().unwrap_or_default()
                                ),
                                typepython_syntax::ClassMemberKind::Method
                                | typepython_syntax::ClassMemberKind::Overload => format!(
                                    "method {}{}",
                                    member.name,
                                    format_signature(&member.params, member.returns.as_deref())
                                ),
                            },
                            declaration: true,
                        });
                    }
                }
            }
            SyntaxStatement::OverloadDef(statement) | SyntaxStatement::FunctionDef(statement) => {
                let name = &statement.name;
                let line = statement.line;
                let params = &statement.params;
                let returns = statement.returns.as_deref();

                let canonical = format!("{module_key}.{}", name);
                local_symbols.insert(name.clone(), canonical.clone());
                if let Some(range) = find_name_range(&document.text, line, name) {
                    declarations.push(SymbolOccurrence {
                        canonical,
                        name: name.clone(),
                        uri: document.uri.clone(),
                        range,
                        detail: format!("function {}{}", name, format_signature(params, returns)),
                        declaration: true,
                    });
                }
            }
            SyntaxStatement::Import(statement) => {
                for binding in &statement.bindings {
                    local_symbols.insert(binding.local_name.clone(), binding.source_path.clone());
                }
            }
            SyntaxStatement::Value(statement) => {
                for name in &statement.names {
                    let canonical = format!("{module_key}.{name}");
                    local_symbols.insert(name.clone(), canonical.clone());
                    if let Some(range) = find_name_range(&document.text, statement.line, name) {
                        declarations.push(SymbolOccurrence {
                            canonical,
                            name: name.clone(),
                            uri: document.uri.clone(),
                            range,
                            detail: format!(
                                "value {}: {}",
                                name,
                                statement.annotation.clone().unwrap_or_default()
                            ),
                            declaration: true,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    (local_symbols, declarations)
}

fn collect_reference_occurrences(
    document: &DocumentState,
    member_symbols: &BTreeMap<String, Vec<String>>,
    declarations_by_canonical: &BTreeMap<String, SymbolOccurrence>,
) -> Vec<SymbolOccurrence> {
    tokenize_identifiers(&document.text)
        .into_iter()
        .filter_map(|token| {
            let local = document.local_symbols.get(&token.name).cloned();
            let member = if token.preceded_by_dot {
                resolve_member_symbol(document, member_symbols, declarations_by_canonical, &token)
            } else {
                None
            };
            let canonical = local.or(member)?;
            Some(SymbolOccurrence {
                canonical: canonical.clone(),
                name: token.name,
                uri: document.uri.clone(),
                range: token.range,
                detail: canonical,
                declaration: false,
            })
        })
        .collect()
}

fn resolve_member_symbol(
    document: &DocumentState,
    member_symbols: &BTreeMap<String, Vec<String>>,
    declarations_by_canonical: &BTreeMap<String, SymbolOccurrence>,
    token: &TokenOccurrence,
) -> Option<String> {
    let candidates = member_symbols.get(&token.name)?;
    if candidates.len() == 1 {
        return candidates.first().cloned();
    }

    let owner_canonical =
        resolve_member_owner_canonical(document, declarations_by_canonical, token).or_else(
            || {
                let receiver = member_receiver_name(&document.text, token.range.start)?;
                document.local_value_types.get(&receiver).cloned()
            },
        )?;
    let expected = format!("{}.{}", owner_canonical, token.name);
    candidates.iter().find(|candidate| *candidate == &expected).cloned()
}

fn resolve_member_owner_canonical(
    document: &DocumentState,
    declarations_by_canonical: &BTreeMap<String, SymbolOccurrence>,
    token: &TokenOccurrence,
) -> Option<String> {
    let line = token.range.start.line as usize + 1;
    for statement in &document.syntax.statements {
        match statement {
            SyntaxStatement::MethodCall(method_call)
                if method_call.line == line && method_call.method == token.name =>
            {
                return resolve_owner_canonical(
                    document,
                    declarations_by_canonical,
                    &method_call.owner_name,
                    method_call.through_instance,
                );
            }
            SyntaxStatement::MemberAccess(member_access)
                if member_access.line == line && member_access.member == token.name =>
            {
                return resolve_owner_canonical(
                    document,
                    declarations_by_canonical,
                    &member_access.owner_name,
                    member_access.through_instance,
                );
            }
            _ => {}
        }
    }
    None
}

fn resolve_completion_member_owner_types(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
) -> Vec<String> {
    let line = position.line as usize + 1;
    let owner_type = document
        .syntax
        .statements
        .iter()
        .find_map(|statement| match statement {
            SyntaxStatement::MethodCall(method_call) if method_call.line == line => {
                resolve_completion_owner_type_text(
                    workspace,
                    document,
                    position,
                    &method_call.owner_name,
                    method_call.through_instance,
                )
            }
            SyntaxStatement::MemberAccess(member_access) if member_access.line == line => {
                resolve_completion_owner_type_text(
                    workspace,
                    document,
                    position,
                    &member_access.owner_name,
                    member_access.through_instance,
                )
            }
            _ => None,
        })
        .or_else(|| {
            let receiver = member_receiver_name(&document.text, position)?;
            resolve_visible_name_type_text(workspace, document, position, &receiver, 0)
        });

    owner_type
        .map(|owner_type| resolve_type_canonicals(workspace, document, &owner_type))
        .unwrap_or_default()
}

fn resolve_completion_owner_type_text(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
    owner_name: &str,
    through_instance: bool,
) -> Option<String> {
    if through_instance {
        return resolve_callable_return_type_text(workspace, document, position, owner_name);
    }

    resolve_visible_name_type_text(workspace, document, position, owner_name, 0)
        .or_else(|| document.local_symbols.get(owner_name).cloned())
}

fn collect_member_completion_items(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
) -> Vec<Value> {
    let owner_types = resolve_completion_member_owner_types(workspace, document, position);
    if owner_types.is_empty() {
        let mut seen = BTreeSet::new();
        return workspace
            .declarations_by_canonical
            .values()
            .filter(|occurrence| occurrence.canonical.matches('.').count() >= 2)
            .filter(|occurrence| seen.insert(occurrence.name.clone()))
            .map(|occurrence| json!({"label": occurrence.name, "detail": occurrence.detail}))
            .collect();
    }

    let mut owner_members = owner_types
        .iter()
        .map(|owner| collect_visible_member_details(workspace, owner))
        .filter(|members| !members.is_empty())
        .collect::<Vec<_>>();
    if owner_members.is_empty() {
        return Vec::new();
    }

    let mut visible = owner_members.remove(0);
    for members in owner_members {
        visible.retain(|label, _| members.contains_key(label));
    }

    visible.into_iter().map(|(label, detail)| json!({"label": label, "detail": detail})).collect()
}

fn collect_visible_member_details(
    workspace: &WorkspaceState,
    owner_canonical: &str,
) -> BTreeMap<String, String> {
    let mut members = BTreeMap::new();
    let mut visited = BTreeSet::new();
    collect_visible_member_details_recursive(
        workspace,
        owner_canonical,
        &mut visited,
        &mut members,
    );
    members
}

fn collect_visible_member_details_recursive(
    workspace: &WorkspaceState,
    owner_canonical: &str,
    visited: &mut BTreeSet<String>,
    members: &mut BTreeMap<String, String>,
) {
    if !visited.insert(owner_canonical.to_owned()) {
        return;
    }

    let Some((node, declaration)) = resolve_top_level_declaration(workspace, owner_canonical)
    else {
        return;
    };

    for member in node.declarations.iter().filter(|candidate| {
        candidate.owner.as_ref().is_some_and(|owner| owner.name == declaration.name)
    }) {
        members.entry(member.name.clone()).or_insert_with(|| {
            let member_canonical = format!("{owner_canonical}.{}", member.name);
            workspace
                .declarations_by_canonical
                .get(&member_canonical)
                .map(|occurrence| occurrence.detail.clone())
                .unwrap_or_else(|| render_member_detail(member))
        });
    }

    let Some(owner_document) = document_for_module_key(workspace, &node.module_key) else {
        return;
    };
    for base in &declaration.bases {
        for base_canonical in resolve_type_canonicals(workspace, owner_document, base) {
            collect_visible_member_details_recursive(workspace, &base_canonical, visited, members);
        }
    }
}

fn resolve_top_level_declaration<'a>(
    workspace: &'a WorkspaceState,
    canonical: &str,
) -> Option<(&'a ModuleNode, &'a typepython_binding::Declaration)> {
    let (module_key, name) = canonical.rsplit_once('.')?;
    let node = workspace.graph.nodes.iter().find(|node| node.module_key == module_key)?;
    let declaration = node
        .declarations
        .iter()
        .find(|declaration| declaration.owner.is_none() && declaration.name == name)?;
    Some((node, declaration))
}

fn document_for_module_key<'a>(
    workspace: &'a WorkspaceState,
    module_key: &str,
) -> Option<&'a DocumentState> {
    workspace.documents.iter().find(|document| document.syntax.source.logical_module == module_key)
}

fn render_member_detail(member: &typepython_binding::Declaration) -> String {
    match member.kind {
        typepython_binding::DeclarationKind::Value => {
            let annotation = member.value_type.as_deref().unwrap_or(member.detail.as_str());
            format!("field {}: {}", member.name, annotation)
        }
        typepython_binding::DeclarationKind::Function
        | typepython_binding::DeclarationKind::Overload => {
            format!("method {}{}", member.name, member.detail)
        }
        _ => member.detail.clone(),
    }
}

fn resolve_visible_name_type_text(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
    name: &str,
    depth: usize,
) -> Option<String> {
    if depth > 8 {
        return None;
    }

    let line = position.line as usize + 1;
    let (owner_name, owner_type_name) = scope_context_at_position(document, position);
    if name == "self" {
        return owner_type_name;
    }
    if name == "cls" {
        return owner_type_name.map(|owner_type_name| format!("type[{owner_type_name}]"));
    }

    let base_type = resolve_parameter_annotation(
        document,
        owner_name.as_deref(),
        owner_type_name.as_deref(),
        name,
    )
    .or_else(|| {
        resolve_latest_assignment_type_text(
            workspace,
            document,
            line,
            owner_name.as_deref(),
            owner_type_name.as_deref(),
            name,
            depth,
        )
    })
    .or_else(|| {
        document
            .local_value_types
            .get(name)
            .and_then(|value_type| value_type.contains('.').then_some(value_type.clone()))
    })?;

    Some(apply_guard_narrowing(
        workspace,
        document,
        owner_name.as_deref(),
        owner_type_name.as_deref(),
        line,
        name,
        &base_type,
    ))
}

fn resolve_parameter_annotation(
    document: &DocumentState,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    name: &str,
) -> Option<String> {
    let params = if let Some(owner_type_name) = owner_type_name {
        document.syntax.statements.iter().find_map(|statement| match statement {
            SyntaxStatement::Interface(class_like)
            | SyntaxStatement::DataClass(class_like)
            | SyntaxStatement::SealedClass(class_like)
            | SyntaxStatement::ClassDef(class_like)
                if class_like.name == owner_type_name =>
            {
                class_like.members.iter().find_map(|member| {
                    (Some(member.name.as_str()) == owner_name).then_some(member.params.as_slice())
                })
            }
            _ => None,
        })
    } else {
        document.syntax.statements.iter().find_map(|statement| match statement {
            SyntaxStatement::FunctionDef(function) | SyntaxStatement::OverloadDef(function)
                if Some(function.name.as_str()) == owner_name =>
            {
                Some(function.params.as_slice())
            }
            _ => None,
        })
    }?;

    params.iter().find(|param| param.name == name).and_then(|param| param.annotation.clone())
}

fn resolve_latest_assignment_type_text(
    workspace: &WorkspaceState,
    document: &DocumentState,
    current_line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    name: &str,
    depth: usize,
) -> Option<String> {
    document.syntax.statements.iter().rev().find_map(|statement| {
        let SyntaxStatement::Value(value) = statement else {
            return None;
        };
        if value.line >= current_line
            || value.owner_name.as_deref() != owner_name
            || value.owner_type_name.as_deref() != owner_type_name
            || !value.names.iter().any(|candidate| candidate == name)
        {
            return None;
        }
        resolve_value_statement_type_text(workspace, document, value, current_line, depth)
    })
}

fn resolve_value_statement_type_text(
    workspace: &WorkspaceState,
    document: &DocumentState,
    value: &typepython_syntax::ValueStatement,
    current_line: usize,
    depth: usize,
) -> Option<String> {
    value
        .annotation
        .clone()
        .filter(|annotation| !annotation.trim().is_empty())
        .or_else(|| value.value_type.clone().filter(|value_type| !value_type.trim().is_empty()))
        .or_else(|| {
            value.value_callee.as_deref().and_then(|callee| {
                resolve_callable_return_type_text(
                    workspace,
                    document,
                    lsp_position(value.line),
                    callee,
                )
            })
        })
        .or_else(|| {
            value.value_name.as_deref().and_then(|value_name| {
                resolve_visible_name_type_text(
                    workspace,
                    document,
                    lsp_position(current_line),
                    value_name,
                    depth + 1,
                )
            })
        })
}

fn resolve_callable_return_type_text(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
    callee: &str,
) -> Option<String> {
    let (owner_name, owner_type_name) = scope_context_at_position(document, position);
    resolve_callable_return_type_in_scope(
        workspace,
        document,
        owner_name.as_deref(),
        owner_type_name.as_deref(),
        callee,
    )
}

fn resolve_callable_return_type_in_scope(
    workspace: &WorkspaceState,
    document: &DocumentState,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    callee: &str,
) -> Option<String> {
    if let Some(canonical) = document.local_symbols.get(callee) {
        if let Some((_, declaration)) = resolve_top_level_declaration(workspace, canonical) {
            if declaration.kind == typepython_binding::DeclarationKind::Class {
                return Some(callee.to_owned());
            }
            return parse_return_annotation(&declaration.detail);
        }
    }

    resolve_parameter_annotation(document, owner_name, owner_type_name, callee)
        .and_then(|annotation| parse_return_annotation(&annotation))
}

fn parse_return_annotation(detail: &str) -> Option<String> {
    detail
        .split_once("->")
        .map(|(_, returns)| returns.trim().to_owned())
        .filter(|returns| !returns.is_empty())
}

fn resolve_type_canonicals(
    workspace: &WorkspaceState,
    document: &DocumentState,
    type_text: &str,
) -> Vec<String> {
    let mut resolved = Vec::new();
    for branch in union_branches(type_text) {
        let normalized = strip_type_wrappers(&branch);
        if normalized.is_empty() || normalized == "None" {
            continue;
        }
        let head = strip_generic_args(&normalized);
        if workspace.declarations_by_canonical.contains_key(head) {
            push_unique(&mut resolved, head.to_owned());
            continue;
        }
        if let Some(canonical) = document.local_symbols.get(head) {
            push_unique(&mut resolved, canonical.clone());
            continue;
        }
        if let Some((module_key, name)) = head.rsplit_once('.') {
            if workspace.graph.nodes.iter().any(|node| {
                node.module_key == module_key
                    && node
                        .declarations
                        .iter()
                        .any(|declaration| declaration.owner.is_none() && declaration.name == name)
            }) {
                push_unique(&mut resolved, head.to_owned());
            }
        }
    }
    resolved
}

fn apply_guard_narrowing(
    workspace: &WorkspaceState,
    document: &DocumentState,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
    base_type: &str,
) -> String {
    let Some(node) = workspace.graph.nodes.iter().find(|node| node.module_path == document.path)
    else {
        return base_type.to_owned();
    };
    let mut narrowed = base_type.to_owned();

    let mut if_guards = node
        .if_guards
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == owner_name
                && guard.owner_type_name.as_deref() == owner_type_name
                && guard.line < current_line
                && !name_reassigned_after_line(
                    node,
                    owner_name,
                    owner_type_name,
                    value_name,
                    guard.line,
                    current_line,
                )
        })
        .filter_map(|guard| {
            let branch_true = if current_line >= guard.true_start_line
                && current_line <= guard.true_end_line
            {
                Some(true)
            } else if let (Some(start), Some(end)) = (guard.false_start_line, guard.false_end_line)
            {
                (current_line >= start && current_line <= end).then_some(false)
            } else {
                None
            }?;
            Some((guard.line, branch_true, guard.guard.as_ref()?))
        })
        .collect::<Vec<_>>();
    if_guards.sort_by_key(|(line, _, _)| *line);
    for (_, branch_true, guard) in if_guards {
        narrowed =
            apply_guard_condition(workspace, document, &narrowed, value_name, guard, branch_true);
    }

    let mut asserts = node
        .asserts
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == owner_name
                && guard.owner_type_name.as_deref() == owner_type_name
                && guard.line < current_line
                && !name_reassigned_after_line(
                    node,
                    owner_name,
                    owner_type_name,
                    value_name,
                    guard.line,
                    current_line,
                )
        })
        .filter_map(|guard| Some((guard.line, guard.guard.as_ref()?)))
        .collect::<Vec<_>>();
    asserts.sort_by_key(|(line, _)| *line);
    for (_, guard) in asserts {
        narrowed = apply_guard_condition(workspace, document, &narrowed, value_name, guard, true);
    }

    narrowed
}

fn name_reassigned_after_line(
    node: &ModuleNode,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    value_name: &str,
    after_line: usize,
    current_line: usize,
) -> bool {
    node.assignments.iter().any(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.as_deref() == owner_name
            && assignment.owner_type_name.as_deref() == owner_type_name
            && after_line < assignment.line
            && assignment.line < current_line
    }) || node.invalidations.iter().any(|site| {
        site.names.iter().any(|name| name == value_name)
            && site.owner_name.as_deref() == owner_name
            && site.owner_type_name.as_deref() == owner_type_name
            && after_line < site.line
            && site.line < current_line
    })
}

fn apply_guard_condition(
    workspace: &WorkspaceState,
    document: &DocumentState,
    base_type: &str,
    value_name: &str,
    guard: &typepython_binding::GuardConditionSite,
    branch_true: bool,
) -> String {
    match guard {
        typepython_binding::GuardConditionSite::IsNone { name, negated } if name == value_name => {
            match (branch_true, negated) {
                (true, false) | (false, true) => String::from("None"),
                (false, false) | (true, true) => {
                    remove_none_branch(base_type).unwrap_or_else(|| base_type.to_owned())
                }
            }
        }
        typepython_binding::GuardConditionSite::IsInstance { name, types }
            if name == value_name =>
        {
            if branch_true {
                narrow_to_instance_types(base_type, types)
            } else {
                remove_instance_types(base_type, types)
            }
        }
        typepython_binding::GuardConditionSite::PredicateCall { name, callee }
            if name == value_name =>
        {
            apply_predicate_guard(workspace, document, base_type, callee, branch_true)
        }
        typepython_binding::GuardConditionSite::TruthyName { name } if name == value_name => {
            apply_truthy_narrowing(base_type, branch_true)
        }
        typepython_binding::GuardConditionSite::Not(inner) => {
            apply_guard_condition(workspace, document, base_type, value_name, inner, !branch_true)
        }
        typepython_binding::GuardConditionSite::And(parts) => {
            if branch_true {
                parts.iter().fold(base_type.to_owned(), |current, part| {
                    apply_guard_condition(workspace, document, &current, value_name, part, true)
                })
            } else {
                join_type_candidates(
                    parts
                        .iter()
                        .scan(base_type.to_owned(), |current_true, part| {
                            let narrowed_false = apply_guard_condition(
                                workspace,
                                document,
                                current_true,
                                value_name,
                                part,
                                false,
                            );
                            *current_true = apply_guard_condition(
                                workspace,
                                document,
                                current_true,
                                value_name,
                                part,
                                true,
                            );
                            Some(narrowed_false)
                        })
                        .collect(),
                )
            }
        }
        typepython_binding::GuardConditionSite::Or(parts) => {
            if branch_true {
                join_type_candidates(
                    parts
                        .iter()
                        .scan(base_type.to_owned(), |current_false, part| {
                            let narrowed_true = apply_guard_condition(
                                workspace,
                                document,
                                current_false,
                                value_name,
                                part,
                                true,
                            );
                            *current_false = apply_guard_condition(
                                workspace,
                                document,
                                current_false,
                                value_name,
                                part,
                                false,
                            );
                            Some(narrowed_true)
                        })
                        .collect(),
                )
            } else {
                parts.iter().fold(base_type.to_owned(), |current, part| {
                    apply_guard_condition(workspace, document, &current, value_name, part, false)
                })
            }
        }
        _ => base_type.to_owned(),
    }
}

fn apply_predicate_guard(
    workspace: &WorkspaceState,
    document: &DocumentState,
    base_type: &str,
    callee: &str,
    branch_true: bool,
) -> String {
    let Some((kind, guarded_type)) = parse_guard_return_kind(workspace, document, callee) else {
        return base_type.to_owned();
    };
    match (kind.as_str(), branch_true) {
        ("TypeGuard", true) | ("TypeIs", true) => {
            narrow_to_instance_types(base_type, &[guarded_type])
        }
        ("TypeIs", false) => remove_instance_types(base_type, &[guarded_type]),
        _ => base_type.to_owned(),
    }
}

fn parse_guard_return_kind(
    workspace: &WorkspaceState,
    document: &DocumentState,
    callee: &str,
) -> Option<(String, String)> {
    let returns = resolve_callable_return_type_in_scope(workspace, document, None, None, callee)?;
    if let Some(inner) =
        returns.strip_prefix("TypeGuard[").and_then(|inner| inner.strip_suffix(']'))
    {
        return Some((String::from("TypeGuard"), inner.trim().to_owned()));
    }
    if let Some(inner) = returns.strip_prefix("TypeIs[").and_then(|inner| inner.strip_suffix(']')) {
        return Some((String::from("TypeIs"), inner.trim().to_owned()));
    }
    None
}

fn narrow_to_instance_types(base_type: &str, types: &[String]) -> String {
    let kept = union_branches(base_type)
        .into_iter()
        .filter(|branch| types.iter().any(|expected| type_branch_matches(branch, expected)))
        .collect::<Vec<_>>();
    if kept.is_empty() { join_type_candidates(types.to_vec()) } else { join_type_candidates(kept) }
}

fn remove_instance_types(base_type: &str, types: &[String]) -> String {
    let kept = union_branches(base_type)
        .into_iter()
        .filter(|branch| !types.iter().any(|expected| type_branch_matches(branch, expected)))
        .collect::<Vec<_>>();
    if kept.is_empty() { base_type.to_owned() } else { join_type_candidates(kept) }
}

fn remove_none_branch(base_type: &str) -> Option<String> {
    let kept =
        union_branches(base_type).into_iter().filter(|branch| branch != "None").collect::<Vec<_>>();
    (!kept.is_empty()).then(|| join_type_candidates(kept))
}

fn apply_truthy_narrowing(base_type: &str, branch_true: bool) -> String {
    let branches = union_branches(base_type);
    let non_none =
        branches.iter().filter(|branch| branch.as_str() != "None").cloned().collect::<Vec<_>>();
    if branches.iter().any(|branch| branch == "None") {
        return if branch_true { join_type_candidates(non_none) } else { String::from("None") };
    }
    base_type.to_owned()
}

fn join_type_candidates(candidates: Vec<String>) -> String {
    let mut unique = Vec::new();
    for candidate in candidates {
        for branch in union_branches(&candidate) {
            push_unique(&mut unique, branch);
        }
    }
    unique.join(" | ")
}

fn union_branches(type_text: &str) -> Vec<String> {
    let trimmed = type_text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Some(inner) = trimmed.strip_prefix("Union[").and_then(|inner| inner.strip_suffix(']')) {
        return split_top_level(inner, ',');
    }
    if trimmed.contains('|') {
        let branches = split_top_level(trimmed, '|');
        if branches.len() > 1 {
            return branches;
        }
    }
    vec![trimmed.to_owned()]
}

fn strip_type_wrappers(type_text: &str) -> String {
    let mut current = type_text.trim().to_owned();
    loop {
        let next = [
            "Annotated[",
            "ClassVar[",
            "Final[",
            "Required[",
            "NotRequired[",
            "ReadOnly[",
            "type[",
        ]
        .into_iter()
        .find_map(|prefix| unwrap_first_type_argument(&current, prefix));
        let Some(next) = next else {
            return current;
        };
        current = next;
    }
}

fn unwrap_first_type_argument(type_text: &str, prefix: &str) -> Option<String> {
    let inner = type_text.strip_prefix(prefix)?.strip_suffix(']')?;
    split_top_level(inner, ',').into_iter().next()
}

fn strip_generic_args(type_text: &str) -> &str {
    type_text.split_once('[').map_or(type_text, |(head, _)| head.trim())
}

fn split_top_level(text: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in text.chars() {
        match ch {
            '[' => {
                depth += 1;
                current.push(ch);
            }
            ']' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            _ if ch == separator && depth == 0 => {
                let part = current.trim();
                if !part.is_empty() {
                    parts.push(part.to_owned());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let part = current.trim();
    if !part.is_empty() {
        parts.push(part.to_owned());
    }
    parts
}

fn type_branch_matches(branch: &str, expected: &str) -> bool {
    strip_generic_args(&strip_type_wrappers(branch))
        == strip_generic_args(&strip_type_wrappers(expected))
}

fn push_unique(items: &mut Vec<String>, item: String) {
    if !items.contains(&item) {
        items.push(item);
    }
}

fn scope_context_at_position(
    document: &DocumentState,
    position: LspPosition,
) -> (Option<String>, Option<String>) {
    let line = position.line as usize + 1;
    let mut best = None;
    for statement in &document.syntax.statements {
        if let Some(candidate) = statement_scope_context(statement, &document.text, line) {
            if best.as_ref().is_none_or(|(best_line, _, _)| candidate.0 >= *best_line) {
                best = Some(candidate);
            }
        }
    }
    best.map(|(_, owner_name, owner_type_name)| (owner_name, owner_type_name)).unwrap_or_default()
}

fn statement_scope_context(
    statement: &SyntaxStatement,
    text: &str,
    current_line: usize,
) -> Option<(usize, Option<String>, Option<String>)> {
    match statement {
        SyntaxStatement::Value(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::Return(value) if value.line <= current_line => {
            Some((value.line, Some(value.owner_name.clone()), value.owner_type_name.clone()))
        }
        SyntaxStatement::Yield(value) if value.line <= current_line => {
            Some((value.line, Some(value.owner_name.clone()), value.owner_type_name.clone()))
        }
        SyntaxStatement::If(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::Assert(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::Invalidate(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::Match(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::For(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::With(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::ExceptHandler(value) if value.line <= current_line => {
            Some((value.line, value.owner_name.clone(), value.owner_type_name.clone()))
        }
        SyntaxStatement::FunctionDef(function) | SyntaxStatement::OverloadDef(function)
            if function.line < current_line
                && line_indentation(document_line_text(text, current_line))
                    > line_indentation(document_line_text(text, function.line)) =>
        {
            Some((function.line, Some(function.name.clone()), None))
        }
        SyntaxStatement::Interface(class_like)
        | SyntaxStatement::DataClass(class_like)
        | SyntaxStatement::SealedClass(class_like)
        | SyntaxStatement::ClassDef(class_like) => {
            class_member_scope_context(class_like, text, current_line)
        }
        _ => None,
    }
}

fn class_member_scope_context(
    class_like: &NamedBlockStatement,
    text: &str,
    current_line: usize,
) -> Option<(usize, Option<String>, Option<String>)> {
    class_like.members.iter().rev().find_map(|member| {
        (member.line < current_line
            && line_indentation(document_line_text(text, current_line))
                > line_indentation(document_line_text(text, member.line)))
        .then(|| (member.line, Some(member.name.clone()), Some(class_like.name.clone())))
    })
}

fn document_line_text(text: &str, line: usize) -> &str {
    text.lines().nth(line.saturating_sub(1)).unwrap_or("")
}

fn line_indentation(text: &str) -> usize {
    text.chars().take_while(|ch| ch.is_whitespace()).count()
}

fn lsp_position(line: usize) -> LspPosition {
    LspPosition { line: line.saturating_sub(1) as u32, character: 0 }
}

fn collect_missing_annotation_code_actions(
    workspace: &WorkspaceState,
    document: &DocumentState,
    range: LspRange,
) -> Vec<Value> {
    let line = range.start.line as usize + 1;
    document
        .syntax
        .statements
        .iter()
        .filter_map(|statement| {
            let SyntaxStatement::Value(value) = statement else {
                return None;
            };
            if value.line != line || value.annotation.is_some() || value.names.len() != 1 {
                return None;
            }
            let name = value.names.first()?;
            let inferred =
                resolve_value_statement_type_text(workspace, document, value, line + 1, 0)?;
            if inferred.is_empty() || inferred.contains("unknown") || inferred.contains("dynamic") {
                return None;
            }
            let name_range = find_name_range(&document.text, value.line, name)?;
            Some(code_action(
                format!("Add type annotation `{name}: {inferred}`"),
                &document.uri,
                vec![LspTextEdit {
                    range: LspRange { start: name_range.end, end: name_range.end },
                    new_text: format!(": {inferred}"),
                }],
            ))
        })
        .collect()
}

fn collect_diagnostic_suggestion_code_actions(
    document: &DocumentState,
    range: LspRange,
    params: &Value,
) -> Vec<Value> {
    let diagnostics = params
        .get("context")
        .and_then(|context| context.get("diagnostics"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut actions = Vec::new();
    for diagnostic in diagnostics {
        let Some(data) = diagnostic.get("data") else {
            continue;
        };
        let Some(suggestions) = data.get("suggestions").and_then(Value::as_array) else {
            continue;
        };
        for suggestion in suggestions {
            let applicability =
                suggestion.get("applicability").and_then(Value::as_str).unwrap_or_default();
            if applicability != "machineApplicable" {
                continue;
            }
            let Some(span) = suggestion.get("span") else {
                continue;
            };
            let suggestion_range = LspRange {
                start: LspPosition {
                    line: span.get("line").and_then(Value::as_u64).unwrap_or(1).saturating_sub(1)
                        as u32,
                    character: span
                        .get("column")
                        .and_then(Value::as_u64)
                        .unwrap_or(1)
                        .saturating_sub(1) as u32,
                },
                end: LspPosition {
                    line: span
                        .get("end_line")
                        .and_then(Value::as_u64)
                        .unwrap_or(1)
                        .saturating_sub(1) as u32,
                    character: span
                        .get("end_column")
                        .and_then(Value::as_u64)
                        .unwrap_or(1)
                        .saturating_sub(1) as u32,
                },
            };
            if !range_intersects(range, suggestion_range) {
                continue;
            }
            let replacement =
                suggestion.get("replacement").and_then(Value::as_str).unwrap_or_default();
            let title = suggestion
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Apply suggested fix")
                .to_owned();
            actions.push(code_action(
                title,
                &document.uri,
                vec![LspTextEdit { range: suggestion_range, new_text: replacement.to_owned() }],
            ));
        }
    }
    actions
}

fn collect_unsafe_code_actions(
    document: &DocumentState,
    range: LspRange,
    params: &Value,
) -> Vec<Value> {
    let diagnostics = params
        .get("context")
        .and_then(|context| context.get("diagnostics"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let line = range.start.line as usize + 1;
    let line_text = document_line_text(&document.text, line);
    if line_text.trim_start().starts_with("unsafe:") {
        return Vec::new();
    }
    let has_unsafe_diagnostic = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.get("code").and_then(Value::as_str) == Some("TPY4019"));
    if !has_unsafe_diagnostic {
        return Vec::new();
    }

    let indent = line_text.chars().take_while(|ch| ch.is_whitespace()).collect::<String>();
    let trimmed = line_text.trim_start();
    let replacement = format!("{indent}unsafe:\n{indent}    {trimmed}");
    vec![code_action(
        String::from("Wrap in `unsafe:` block"),
        &document.uri,
        vec![LspTextEdit {
            range: LspRange {
                start: LspPosition { line: range.start.line, character: 0 },
                end: LspPosition {
                    line: range.start.line,
                    character: line_text.chars().count() as u32,
                },
            },
            new_text: replacement,
        }],
    )]
}

fn collect_missing_import_code_actions(
    workspace: &WorkspaceState,
    document: &DocumentState,
    range: LspRange,
) -> Vec<Value> {
    let Some(token) = token_at_position(&document.text, range.start) else {
        return Vec::new();
    };
    if token.preceded_by_dot || document.local_symbols.contains_key(&token.name) {
        return Vec::new();
    }

    let current_module = &document.syntax.source.logical_module;
    let mut candidates = workspace
        .graph
        .nodes
        .iter()
        .filter(|node| node.module_key != *current_module)
        .filter_map(|node| {
            node.declarations
                .iter()
                .find(|declaration| declaration.owner.is_none() && declaration.name == token.name)
                .map(|_| node.module_key.clone())
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    if candidates.len() != 1 {
        return Vec::new();
    }

    let import_line = format!("from {} import {}\n", candidates[0], token.name);
    vec![code_action(
        format!("Import `{}` from `{}`", token.name, candidates[0]),
        &document.uri,
        vec![LspTextEdit { range: import_insertion_range(document), new_text: import_line }],
    )]
}

fn code_action(title: String, uri: &str, edits: Vec<LspTextEdit>) -> Value {
    json!({
        "title": title,
        "kind": "quickfix",
        "edit": {
            "changes": {
                uri: edits
            }
        }
    })
}

fn import_insertion_range(document: &DocumentState) -> LspRange {
    let import_line = document
        .syntax
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::Import(statement) => Some(statement.line),
            _ => None,
        })
        .max();
    let insert_line = import_line.unwrap_or(0);
    LspRange {
        start: LspPosition { line: insert_line as u32, character: 0 },
        end: LspPosition { line: insert_line as u32, character: 0 },
    }
}

fn token_at_position(text: &str, position: LspPosition) -> Option<TokenOccurrence> {
    tokenize_identifiers(text).into_iter().find(|token| range_contains(token.range, position))
}

fn resolve_owner_canonical(
    document: &DocumentState,
    declarations_by_canonical: &BTreeMap<String, SymbolOccurrence>,
    owner_name: &str,
    through_instance: bool,
) -> Option<String> {
    if !through_instance {
        return document
            .local_value_types
            .get(owner_name)
            .cloned()
            .or_else(|| document.local_symbols.get(owner_name).cloned());
    }

    let callable_canonical = document.local_symbols.get(owner_name)?.clone();
    let callable = declarations_by_canonical.get(&callable_canonical)?;
    let return_type = callable.detail.split_once("->")?.1.trim();
    document.local_symbols.get(return_type).cloned().or_else(|| Some(return_type.to_owned()))
}

fn member_receiver_name(text: &str, position: LspPosition) -> Option<String> {
    let line = text.lines().nth(position.line as usize)?;
    let prefix = line.chars().take(position.character as usize).collect::<String>();
    let mut chars = prefix.chars().collect::<Vec<_>>();
    while chars.last().is_some_and(|ch| ch.is_whitespace()) {
        chars.pop();
    }
    if chars.pop()? != '.' {
        return None;
    }
    while chars.last().is_some_and(|ch| ch.is_whitespace()) {
        chars.pop();
    }
    let end = chars.len();
    let mut start = end;
    while start > 0 {
        let ch = chars[start - 1];
        if ch.is_ascii_alphanumeric() || ch == '_' {
            start -= 1;
        } else {
            break;
        }
    }
    (start < end).then(|| chars[start..end].iter().collect())
}

fn collect_local_value_types(
    document: &DocumentState,
    local_symbols: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut types = BTreeMap::new();
    for statement in &document.syntax.statements {
        let SyntaxStatement::Value(statement) = statement else {
            continue;
        };
        let resolved_type = statement
            .annotation
            .as_ref()
            .and_then(|annotation| local_symbols.get(annotation))
            .cloned()
            .or_else(|| {
                statement
                    .value_callee
                    .as_ref()
                    .and_then(|callee| local_symbols.get(callee))
                    .cloned()
            })
            .or_else(|| {
                statement
                    .value_type
                    .as_ref()
                    .and_then(|value_type| local_symbols.get(value_type))
                    .cloned()
            });
        let Some(resolved_type) = resolved_type else {
            continue;
        };
        for name in &statement.names {
            types.insert(name.clone(), resolved_type.clone());
        }
    }
    types
}

fn dedupe_occurrences(occurrences: &mut Vec<SymbolOccurrence>) {
    let mut seen = BTreeSet::new();
    occurrences.retain(|occurrence| {
        seen.insert((
            occurrence.canonical.clone(),
            occurrence.uri.clone(),
            occurrence.range.start.line,
            occurrence.range.start.character,
            occurrence.range.end.line,
            occurrence.range.end.character,
            occurrence.declaration,
        ))
    });
}

#[derive(Debug)]
struct TokenOccurrence {
    name: String,
    range: LspRange,
    preceded_by_dot: bool,
}

fn tokenize_identifiers(text: &str) -> Vec<TokenOccurrence> {
    let mut tokens = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let chars = line.chars().collect::<Vec<_>>();
        let mut index = 0usize;
        while index < chars.len() {
            if chars[index].is_ascii_alphabetic() || chars[index] == '_' {
                let start = index;
                index += 1;
                while index < chars.len()
                    && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
                {
                    index += 1;
                }
                let name = chars[start..index].iter().collect::<String>();
                let preceded_by_dot = chars[..start]
                    .iter()
                    .rev()
                    .find(|ch| !ch.is_whitespace())
                    .is_some_and(|ch| *ch == '.');
                tokens.push(TokenOccurrence {
                    name,
                    range: LspRange {
                        start: LspPosition { line: line_index as u32, character: start as u32 },
                        end: LspPosition { line: line_index as u32, character: index as u32 },
                    },
                    preceded_by_dot,
                });
            } else {
                index += 1;
            }
        }
    }
    tokens
}

fn find_name_range(text: &str, line: usize, name: &str) -> Option<LspRange> {
    let line_text = text.lines().nth(line.saturating_sub(1))?;
    let column = line_text.find(name)?;
    Some(LspRange {
        start: LspPosition { line: line.saturating_sub(1) as u32, character: column as u32 },
        end: LspPosition {
            line: line.saturating_sub(1) as u32,
            character: (column + name.len()) as u32,
        },
    })
}

fn line_prefix(text: &str, position: LspPosition) -> String {
    text.lines()
        .nth(position.line as usize)
        .map(|line| line.chars().take(position.character as usize).collect())
        .unwrap_or_default()
}

fn format_signature(params: &[typepython_syntax::FunctionParam], returns: Option<&str>) -> String {
    format!(
        "({})->{}",
        params
            .iter()
            .map(|param| match &param.annotation {
                Some(annotation) => format!("{}:{}", param.name, annotation),
                None => param.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(","),
        returns.unwrap_or("")
    )
}

fn collect_project_source_paths(
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

fn collect_import_source_paths(syntax_trees: &[SyntaxTree]) -> Vec<String> {
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

fn import_resolves_within_modules(import_path: &str, module_keys: &BTreeSet<String>) -> bool {
    module_path_prefixes(import_path).any(|module_key| module_keys.contains(module_key))
}

fn matching_support_module_keys(
    import_path: &str,
    sources_by_module: &BTreeMap<String, Vec<DiscoveredSource>>,
) -> Vec<String> {
    module_path_prefixes(import_path)
        .filter(|module_key| sources_by_module.contains_key(*module_key))
        .map(str::to_owned)
        .collect()
}

fn module_path_prefixes(import_path: &str) -> impl Iterator<Item = &str> {
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

fn bundled_stdlib_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib")
}

fn bundled_stdlib_sources(target_python: &str) -> Result<Vec<DiscoveredSource>> {
    let root = bundled_stdlib_root();
    let mut sources = Vec::new();
    if root.exists() {
        walk_bundled_stdlib_directory(&root, &root, target_python, &mut sources)?;
    }
    Ok(sources)
}

fn walk_bundled_stdlib_directory(
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

fn external_resolution_sources(config: &ConfigHandle) -> Result<Vec<DiscoveredSource>> {
    let mut sources = Vec::new();
    for root in configured_external_type_roots(config)? {
        walk_external_type_root(&root, &mut sources)?;
    }
    sort_sources_by_type_authority(&mut sources);
    sources.dedup_by(|left, right| left.path == right.path);
    Ok(sources)
}

fn configured_external_type_roots(config: &ConfigHandle) -> Result<Vec<PathBuf>> {
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

fn discovered_python_type_roots(config: &ConfigHandle) -> Vec<PathBuf> {
    let interpreter = resolve_python_executable(config);
    python_type_roots_from_interpreter(&interpreter)
}

fn python_type_roots_from_interpreter(interpreter: &Path) -> Vec<PathBuf> {
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

fn bundled_stdlib_file_matches_target(path: &Path, target_python: &str) -> Result<bool> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("unable to read bundled stdlib file {}", path.display()))?;
    Ok(parse_bundled_stdlib_version_filter(&contents).allows(target_python))
}

#[derive(Debug, Default, Clone)]
struct BundledStdlibVersionFilter {
    min_python: Option<String>,
    max_python: Option<String>,
}

impl BundledStdlibVersionFilter {
    fn allows(&self, target_python: &str) -> bool {
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

fn parse_bundled_stdlib_version_filter(source: &str) -> BundledStdlibVersionFilter {
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

fn resolve_python_executable(config: &ConfigHandle) -> PathBuf {
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

fn walk_external_type_root(root: &Path, sources: &mut Vec<DiscoveredSource>) -> Result<()> {
    walk_external_type_root_directory(root, root, sources)
}

fn walk_external_type_root_directory(
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

fn external_source_allowed(root: &Path, path: &Path, kind: SourceKind) -> bool {
    match kind {
        SourceKind::Stub => true,
        SourceKind::Python => external_runtime_allowed(root, path),
        SourceKind::TypePython => false,
    }
}

fn external_runtime_allowed(root: &Path, path: &Path) -> bool {
    let Some(stub_root) = sibling_stub_distribution_root(root, path) else {
        return external_runtime_is_typed(root, path);
    };

    partial_stub_package_marker(&stub_root)
        && runtime_module_missing_from_stub_package(root, path, &stub_root)
}

fn external_logical_module_path(root: &Path, path: &Path) -> Option<String> {
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

fn external_runtime_is_typed(root: &Path, path: &Path) -> bool {
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

fn sibling_stub_distribution_root(root: &Path, path: &Path) -> Option<PathBuf> {
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

fn runtime_module_missing_from_stub_package(root: &Path, path: &Path, stub_root: &Path) -> bool {
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

fn sort_sources_by_type_authority(sources: &mut [DiscoveredSource]) {
    sources.sort_by(|left, right| {
        left.logical_module
            .cmp(&right.logical_module)
            .then_with(|| {
                source_kind_authority_rank(left.kind).cmp(&source_kind_authority_rank(right.kind))
            })
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn source_kind_authority_rank(kind: SourceKind) -> u8 {
    match kind {
        SourceKind::TypePython => 0,
        SourceKind::Stub => 1,
        SourceKind::Python => 2,
    }
}

fn partial_stub_package_marker(stub_root: &Path) -> bool {
    std::fs::read_to_string(stub_root.join("py.typed"))
        .ok()
        .is_some_and(|contents| contents.lines().any(|line| line.trim() == "partial"))
}

fn compile_patterns(config: &ConfigHandle, patterns: &[String]) -> Result<Vec<Pattern>> {
    patterns
        .iter()
        .map(|pattern| {
            Pattern::new(pattern).with_context(|| {
                format!("invalid glob pattern `{pattern}` in {}", config.config_path.display())
            })
        })
        .collect()
}

fn walk_directory(
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

fn is_selected_source_path(
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

fn source_root_for_path(config: &ConfigHandle, path: &Path) -> Option<PathBuf> {
    config
        .config
        .project
        .src
        .iter()
        .map(|root| config.resolve_relative_path(root))
        .find(|root| path.starts_with(root))
}

fn logical_module_path(root: &Path, path: &Path) -> Option<String> {
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

fn package_components(relative_parent: &Path) -> Option<Vec<String>> {
    let mut components = Vec::new();
    for component in relative_parent.components() {
        let name = component.as_os_str().to_str()?.to_owned();
        components.push(name);
    }
    Some(components)
}

fn collect_parse_diagnostics(syntax_trees: &[SyntaxTree]) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    for tree in syntax_trees {
        diagnostics.diagnostics.extend(tree.diagnostics.diagnostics.iter().cloned());
    }
    diagnostics
}

fn normalize_glob_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn normalize_path_string(path: &Path) -> String {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
}

fn path_to_uri(path: &Path) -> String {
    Url::from_file_path(path).expect("filesystem paths should convert to file:// URIs").into()
}

fn uri_to_path(uri: &str) -> Result<PathBuf, LspError> {
    let parsed = Url::parse(uri)
        .map_err(|error| LspError::Other(format!("unsupported URI `{uri}`: {error}")))?;
    parsed.to_file_path().map_err(|()| LspError::Other(format!("unsupported URI `{uri}`")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env, fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn handle_initialize_returns_required_capabilities() {
        let config = temp_config("handle_initialize_returns_required_capabilities", "pass\n");
        let mut server = Server::new(config);
        let responses = server
            .handle_message(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
            .expect("initialize should succeed");
        let capabilities = &responses[0]["result"]["capabilities"];
        assert_eq!(capabilities["textDocumentSync"]["openClose"], json!(true));
        assert_eq!(capabilities["textDocumentSync"]["change"], json!(2));
        assert_eq!(capabilities["hoverProvider"], json!(true));
        assert_eq!(capabilities["definitionProvider"], json!(true));
        assert_eq!(capabilities["referencesProvider"], json!(true));
        assert_eq!(capabilities["signatureHelpProvider"]["triggerCharacters"], json!(["(", ","]));
        assert_eq!(capabilities["documentSymbolProvider"], json!(true));
        assert_eq!(capabilities["renameProvider"], json!(true));
        assert_eq!(capabilities["codeActionProvider"], json!(true));
    }

    #[test]
    fn did_open_publishes_overlay_diagnostics() {
        let config = temp_config(
            "did_open_publishes_overlay_diagnostics",
            "def ok() -> int:\n    return 1\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let responses = server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def broken(:\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should publish diagnostics");
        assert!(
            responses
                .iter()
                .any(|response| response["method"] == json!("textDocument/publishDiagnostics"))
        );
        let payload = responses
            .iter()
            .find(|response| {
                response["method"] == json!("textDocument/publishDiagnostics")
                    && response["params"]["uri"] == json!(uri)
            })
            .expect("publishDiagnostics notification should be present");
        let diagnostics = payload["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics payload should be an array");
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn hover_definition_references_and_rename_work() {
        let config = temp_workspace(
            "hover_definition_references_and_rename_work",
            &[
                ("src/app/a.tpy", "def target(value: int) -> int:\n    return value\n"),
                (
                    "src/app/b.tpy",
                    "from app.a import target\n\ndef use() -> int:\n    return target(1)\n",
                ),
            ],
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/b.tpy"));

        let hover = server
            .handle_hover(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 3, "character": 11}
            }))
            .expect("hover should succeed");
        assert!(
            hover["contents"]["value"]
                .as_str()
                .expect("hover contents should be a string")
                .contains("function target")
        );

        let definition = server
            .handle_definition(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 3, "character": 11}
            }))
            .expect("definition should succeed");
        assert_eq!(definition.as_array().expect("definition should be an array").len(), 1);

        let references = server
            .handle_references(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 3, "character": 11},
                "context": {"includeDeclaration": true}
            }))
            .expect("references should succeed");
        assert!(references.as_array().expect("references should be an array").len() >= 2);

        let rename = server
            .handle_rename(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 3, "character": 11},
                "newName": "renamed"
            }))
            .expect("rename should succeed");
        assert!(rename["changes"].is_object());
    }

    #[test]
    fn signature_help_returns_function_signature() {
        let config = temp_config(
            "signature_help_returns_function_signature",
            "def target(a: int, b: str = \"x\") -> int:\n    return 1\n\nvalue = target(1, )\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
            .expect("source file should be readable");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let signature_help = server
            .handle_signature_help(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 3, "character": 18}
            }))
            .expect("signatureHelp should succeed");
        assert_eq!(signature_help["activeParameter"], json!(1));
        assert_eq!(signature_help["activeSignature"], json!(0));
        assert_eq!(
            signature_help["signatures"][0]["label"],
            json!("target(a: int, b: str = ...) -> int")
        );
    }

    #[test]
    fn signature_help_returns_member_signature_without_self() {
        let config = temp_config(
            "signature_help_returns_member_signature_without_self",
            "class Box:\n    def put(self, value: int, label: str) -> None:\n        pass\n\nbox = Box()\nbox.put(1, )\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
            .expect("source file should be readable");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let signature_help = server
            .handle_signature_help(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 5, "character": 11}
            }))
            .expect("signatureHelp should succeed");
        assert_eq!(signature_help["activeParameter"], json!(1));
        assert_eq!(
            signature_help["signatures"][0]["label"],
            json!("Box.put(value: int, label: str) -> None")
        );
    }

    #[test]
    fn document_symbol_returns_hierarchical_symbols() {
        let config = temp_config(
            "document_symbol_returns_hierarchical_symbols",
            "typealias UserId = int\n\nclass Box:\n    value: int\n    def get(self) -> int:\n        return self.value\n\ndef build() -> Box:\n    return Box()\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
            .expect("source file should be readable");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let symbols = server
            .handle_document_symbol(json!({
                "textDocument": {"uri": uri}
            }))
            .expect("documentSymbol should succeed");
        let symbols = symbols.as_array().expect("document symbols should be an array");
        assert_eq!(symbols.len(), 3);
        assert_eq!(symbols[0]["name"], json!("UserId"));
        assert_eq!(symbols[1]["name"], json!("Box"));
        assert_eq!(symbols[1]["kind"], json!(5));
        assert_eq!(symbols[1]["children"][0]["name"], json!("value"));
        assert_eq!(symbols[1]["children"][1]["name"], json!("get"));
        assert_eq!(symbols[2]["name"], json!("build"));
    }

    #[test]
    fn import_binding_definition_and_hover_resolve_to_original_declaration() {
        let config = temp_workspace(
            "import_binding_definition_and_hover_resolve_to_original_declaration",
            &[
                ("src/app/a.tpy", "def target(value: int) -> int:\n    return value\n"),
                ("src/app/b.tpy", "from app.a import target\n"),
            ],
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/b.tpy"));
        let a_uri = path_to_uri(&config.config_dir.join("src/app/a.tpy"));
        let text = fs::read_to_string(config.config_dir.join("src/app/b.tpy"))
            .expect("source file should be readable");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let hover = server
            .handle_hover(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 0, "character": 18}
            }))
            .expect("hover should succeed");
        assert!(
            hover["contents"]["value"]
                .as_str()
                .expect("hover contents should be a string")
                .contains("function target")
        );

        let definition = server
            .handle_definition(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 0, "character": 18}
            }))
            .expect("definition should succeed");
        let entries = definition.as_array().expect("definition should be an array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["uri"], json!(a_uri));
    }

    #[test]
    fn completion_returns_local_symbols_and_member_symbols() {
        let config = temp_workspace(
            "completion_returns_local_symbols_and_member_symbols",
            &[(
                "src/app/__init__.tpy",
                "class Box:\n    value: int\n    def method(self) -> int:\n        return self.value\n\ndef build() -> Box:\n    return Box()\n\nbox: Box = build()\nbox.method\n",
            )],
        );
        let mut server = Server::new(config.clone());
        let path = config.config_dir.join("src/app/__init__.tpy");
        let text = fs::read_to_string(&path).expect("source file should be readable");
        let uri = path_to_uri(&path);
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let symbols = server
            .handle_completion(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 8, "character": 3}
            }))
            .expect("completion for local symbols should succeed");
        assert!(
            symbols["items"]
                .as_array()
                .expect("completion items should be an array")
                .iter()
                .any(|item| item["label"] == json!("build"))
        );

        let members = server
            .handle_completion(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 9, "character": 4}
            }))
            .expect("completion for member symbols should succeed");
        assert!(
            members["items"]
                .as_array()
                .expect("member completion items should be an array")
                .iter()
                .any(|item| item["label"] == json!("method"))
        );
    }

    #[test]
    fn hover_and_definition_resolve_duplicate_member_names_by_receiver() {
        let config = temp_workspace(
            "hover_and_definition_resolve_duplicate_member_names_by_receiver",
            &[(
                "src/app/__init__.tpy",
                "class Foo:\n    def ping(self) -> int:\n        return 1\n\nclass Bar:\n    def ping(self) -> int:\n        return 2\n\nfoo = Foo()\nfoo.ping()\n",
            )],
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
            .expect("source file should be readable");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let hover = server
            .handle_hover(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 9, "character": 5}
            }))
            .expect("hover should succeed");
        assert_ne!(hover, Value::Null);

        let definition = server
            .handle_definition(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 9, "character": 5}
            }))
            .expect("definition should succeed");
        let entries = definition.as_array().expect("definition should be an array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["range"]["start"]["line"], json!(1));
    }

    #[test]
    fn completion_resolves_duplicate_member_names_by_receiver() {
        let config = temp_workspace(
            "completion_resolves_duplicate_member_names_by_receiver",
            &[(
                "src/app/__init__.tpy",
                "class Foo:
    def ping(self) -> int:
        return 1
    def only_foo(self) -> int:
        return 1

class Bar:
    def ping(self) -> int:
        return 2
    def only_bar(self) -> int:
        return 2

foo: Foo = Foo()
foo.ping()
",
            )],
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
            .expect("source file should be readable");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let completion = server
            .handle_completion(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 13, "character": 4}
            }))
            .expect("completion should succeed");
        let labels = completion["items"]
            .as_array()
            .expect("completion items should be an array")
            .iter()
            .map(|item| item["label"].as_str().expect("label should be a string"))
            .collect::<Vec<_>>();
        assert!(labels.contains(&"ping"));
        assert!(labels.contains(&"only_foo"));
        assert!(!labels.contains(&"only_bar"));
    }

    #[test]
    fn completion_includes_inherited_members() {
        let config = temp_workspace(
            "completion_includes_inherited_members",
            &[(
                "src/app/__init__.tpy",
                "class Base:\n    def inherited(self) -> int:\n        return 1\n\nclass Child(Base):\n    def own(self) -> int:\n        return 2\n\nchild: Child = Child()\nchild.inherited\n",
            )],
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
            .expect("source file should be readable");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let completion = server
            .handle_completion(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 9, "character": 6}
            }))
            .expect("completion should succeed");
        let labels = completion["items"]
            .as_array()
            .expect("completion items should be an array")
            .iter()
            .map(|item| item["label"].as_str().expect("label should be a string"))
            .collect::<Vec<_>>();
        assert!(labels.contains(&"inherited"));
        assert!(labels.contains(&"own"));
    }

    #[test]
    fn completion_uses_isinstance_narrowing_for_members() {
        let config = temp_workspace(
            "completion_uses_isinstance_narrowing_for_members",
            &[(
                "src/app/__init__.tpy",
                "class Foo:\n    def only_foo(self) -> int:\n        return 1\n\nclass Bar:\n    def only_bar(self) -> int:\n        return 2\n\ndef use(value: Foo | Bar) -> int:\n    if isinstance(value, Foo):\n        value.only_foo\n    return 0\n",
            )],
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
            .expect("source file should be readable");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let completion = server
            .handle_completion(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 10, "character": 14}
            }))
            .expect("completion should succeed");
        let labels = completion["items"]
            .as_array()
            .expect("completion items should be an array")
            .iter()
            .map(|item| item["label"].as_str().expect("label should be a string"))
            .collect::<Vec<_>>();
        assert!(labels.contains(&"only_foo"));
        assert!(!labels.contains(&"only_bar"));
    }

    #[test]
    fn code_actions_offer_missing_type_annotation() {
        let config = temp_workspace(
            "code_actions_offer_missing_type_annotation",
            &[(
                "src/app/__init__.tpy",
                "class Box:\n    pass\n\ndef build() -> Box:\n    return Box()\n\nbox = build()\n",
            )],
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

        let actions = server
            .handle_code_action(json!({
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": 6, "character": 0},
                    "end": {"line": 6, "character": 3}
                },
                "context": {"diagnostics": []}
            }))
            .expect("code action should succeed");
        let actions = actions.as_array().expect("code actions should be an array");
        let action = actions
            .iter()
            .find(|action| {
                action["title"].as_str().is_some_and(|title| title.contains("Add type annotation"))
            })
            .expect("missing type annotation action should be present");
        assert_eq!(action["edit"]["changes"][uri.as_str()][0]["newText"], json!(": Box"));
    }

    #[test]
    fn code_actions_offer_unsafe_wrapper_fix() {
        let config = temp_workspace(
            "code_actions_offer_unsafe_wrapper_fix",
            &[("src/app/__init__.tpy", "def run() -> None:\n    eval(\"1\")\n")],
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

        let actions = server
            .handle_code_action(json!({
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": 1, "character": 4},
                    "end": {"line": 1, "character": 11}
                },
                "context": {
                    "diagnostics": [{
                        "code": "TPY4019",
                        "range": {
                            "start": {"line": 1, "character": 4},
                            "end": {"line": 1, "character": 11}
                        },
                        "message": "unsafe boundary operation `eval(...)` must appear inside `unsafe:`"
                    }]
                }
            }))
            .expect("code action should succeed");
        let actions = actions.as_array().expect("code actions should be an array");
        let action = actions
            .iter()
            .find(|action| action["title"] == json!("Wrap in `unsafe:` block"))
            .expect("unsafe wrapper action should be present");
        assert_eq!(
            action["edit"]["changes"][uri.as_str()][0]["newText"],
            json!("    unsafe:\n        eval(\"1\")")
        );
    }

    #[test]
    fn code_actions_offer_missing_import_fix() {
        let config = temp_workspace(
            "code_actions_offer_missing_import_fix",
            &[
                ("src/app/__init__.tpy", "def run() -> None:\n    Foo()\n"),
                ("src/app/types.tpy", "class Foo:\n    pass\n"),
            ],
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

        let actions = server
            .handle_code_action(json!({
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": 1, "character": 4},
                    "end": {"line": 1, "character": 7}
                },
                "context": {"diagnostics": []}
            }))
            .expect("code action should succeed");
        let actions = actions.as_array().expect("code actions should be an array");
        let action = actions
            .iter()
            .find(|action| action["title"] == json!("Import `Foo` from `app.types`"))
            .expect("missing import action should be present");
        assert_eq!(
            action["edit"]["changes"][uri.as_str()][0]["newText"],
            json!("from app.types import Foo\n")
        );
    }

    #[test]
    fn code_actions_offer_machine_applicable_return_suggestion() {
        let config = temp_workspace(
            "code_actions_offer_machine_applicable_return_suggestion",
            &[(
                "src/app/__init__.tpy",
                "def build(flag: bool) -> int:\n    if flag:\n        return 1\n    return None\n",
            )],
        );
        let path = config.config_dir.join("src/app/__init__.tpy");
        let uri = path_to_uri(&path);
        let text = fs::read_to_string(&path).expect("source file should be readable");
        let syntax = parse_with_options(
            SourceFile {
                path: path.clone(),
                kind: SourceKind::TypePython,
                logical_module: String::from("app"),
                text: text.clone(),
            },
            ParseOptions::default(),
        );
        let document = DocumentState {
            uri: uri.clone(),
            path: path.clone(),
            text,
            syntax,
            local_symbols: BTreeMap::new(),
            local_value_types: BTreeMap::new(),
        };
        let diagnostics = DiagnosticReport {
            diagnostics: vec![
                typepython_diagnostics::Diagnostic::error(
                    "TPY4001",
                    "function `build` returns `None` where `build` expects `int`",
                )
                .with_span(typepython_diagnostics::Span::new(
                    path.display().to_string(),
                    4,
                    1,
                    4,
                    12,
                ))
                .with_suggestion(
                    "Add `| None` to the declared return type",
                    typepython_diagnostics::Span::new(path.display().to_string(), 1, 26, 1, 29),
                    String::from("int | None"),
                    typepython_diagnostics::SuggestionApplicability::MachineApplicable,
                ),
            ],
        };
        let diagnostics = diagnostics_by_uri(std::slice::from_ref(&document), &diagnostics);
        let diagnostics = serde_json::to_value(
            diagnostics.get(&uri).expect("diagnostics should be mapped to the document URI"),
        )
        .expect("diagnostics should serialize");

        let actions = collect_diagnostic_suggestion_code_actions(
            &document,
            LspRange {
                start: LspPosition { line: 0, character: 0 },
                end: LspPosition { line: 0, character: 30 },
            },
            &json!({
                "textDocument": {"uri": uri},
                "context": {"diagnostics": diagnostics}
            }),
        );
        let action = actions
            .iter()
            .find(|action| {
                action["title"]
                    .as_str()
                    .is_some_and(|title| title.contains("Add `| None` to the declared return type"))
            })
            .expect("return suggestion action should be present");
        assert_eq!(action["edit"]["changes"][uri.as_str()][0]["newText"], json!("int | None"));
    }

    #[test]
    fn did_change_reports_overlay_sync_failure_for_unopened_document() {
        let config = temp_config(
            "did_change_reports_overlay_sync_failure_for_unopened_document",
            "def ok() -> int:\n    return 1\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

        let error = server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didChange",
                "params": {
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [{"text": "def changed() -> int:\n    return 2\n"}]
                }
            }))
            .expect_err("didChange without prior didOpen should fail");

        assert!(error.to_string().contains("TPY6002"));
        assert!(error.to_string().contains("unopened overlay"));
    }

    #[test]
    fn did_change_reports_overlay_sync_failure_for_stale_version() {
        let config = temp_config(
            "did_change_reports_overlay_sync_failure_for_stale_version",
            "def ok() -> int:\n    return 1\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def ok() -> int:\n    return 1\n", "languageId": "typepython", "version": 3}}
            }))
            .expect("didOpen should succeed");

        let error = server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didChange",
                "params": {
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [{"text": "def stale() -> int:\n    return 0\n"}]
                }
            }))
            .expect_err("stale didChange version should fail");

        assert!(error.to_string().contains("TPY6002"));
        assert!(error.to_string().contains("out of sync"));
    }

    #[test]
    fn did_change_applies_multiple_incremental_content_changes() {
        let config = temp_config(
            "did_change_applies_multiple_incremental_content_changes",
            "def ok() -> int:\n    return 1\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def ok() -> int:\n    return 1\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let responses = server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didChange",
                "params": {
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [
                        {
                            "range": {
                                "start": {"line": 0, "character": 4},
                                "end": {"line": 0, "character": 6}
                            },
                            "text": "better"
                        },
                        {
                            "range": {
                                "start": {"line": 1, "character": 11},
                                "end": {"line": 1, "character": 12}
                            },
                            "text": "2"
                        }
                    ]
                }
            }))
            .expect("multi-change didChange should succeed");

        assert_eq!(
            server
                .overlays
                .get(&config.config_dir.join("src/app/__init__.tpy"))
                .expect("overlay should still be cached after multi-change update")
                .text,
            "def better() -> int:\n    return 2\n"
        );
        assert!(
            responses.iter().all(|response| response.get("method")
                == Some(&json!("textDocument/publishDiagnostics")))
        );
    }

    #[test]
    fn did_change_applies_ranged_content_change() {
        let config = temp_config(
            "did_change_applies_ranged_content_change",
            "def ok() -> int:\n    return 1\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def ok() -> int:\n    return 1\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didChange",
                "params": {
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [
                        {
                            "range": {
                                "start": {"line": 0, "character": 4},
                                "end": {"line": 0, "character": 6}
                            },
                            "text": "name"
                        }
                    ]
                }
            }))
            .expect("ranged didChange should succeed");

        assert_eq!(
            server
                .overlays
                .get(&config.config_dir.join("src/app/__init__.tpy"))
                .expect("overlay should still be cached after ranged update")
                .text,
            "def name() -> int:\n    return 1\n"
        );
    }

    #[test]
    fn did_change_reports_overlay_sync_failure_for_out_of_bounds_range() {
        let config = temp_config(
            "did_change_reports_overlay_sync_failure_for_out_of_bounds_range",
            "def ok() -> int:\n    return 1\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def ok() -> int:\n    return 1\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let error = server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didChange",
                "params": {
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [
                        {
                            "range": {
                                "start": {"line": 9, "character": 0},
                                "end": {"line": 9, "character": 1}
                            },
                            "text": "boom"
                        }
                    ]
                }
            }))
            .expect_err("out-of-bounds ranged didChange should fail");

        assert!(error.to_string().contains("TPY6002"));
        assert!(error.to_string().contains("references line 9 beyond the current document"));
    }

    #[test]
    fn file_uri_helpers_round_trip_paths_with_spaces() {
        let path = PathBuf::from("/tmp/typepython spaced/project/__init__.tpy");
        let uri = path_to_uri(&path);

        assert_eq!(uri, "file:///tmp/typepython%20spaced/project/__init__.tpy");
        assert_eq!(uri_to_path(&uri).expect("URI should decode to file path"), path);
    }

    #[test]
    fn did_change_updates_overlay_and_republishes_diagnostics() {
        let config = temp_config(
            "did_change_updates_overlay_and_republishes_diagnostics",
            "def ok() -> int:\n    return 1\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def broken(:\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let responses = server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didChange",
                "params": {
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [{"text": "def fixed() -> int:\n    return 1\n"}]
                }
            }))
            .expect("didChange should republish diagnostics");
        assert!(
            responses
                .iter()
                .any(|response| response["method"] == json!("textDocument/publishDiagnostics"))
        );
    }

    #[test]
    fn overlay_export_change_republishes_dependent_module_diagnostics() {
        let config = temp_workspace(
            "overlay_export_change_republishes_dependent_module_diagnostics",
            &[
                ("src/app/a.tpy", "class Producer:\n    pass\n"),
                ("src/app/b.tpy", "from app.a import Producer\nvalue = Producer()\n"),
            ],
        );
        let mut server = Server::new(config.clone());
        let a_uri = path_to_uri(&config.config_dir.join("src/app/a.tpy"));

        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": a_uri,
                        "text": "class Replacement:\n    pass\n",
                        "languageId": "typepython",
                        "version": 1
                    }
                }
            }))
            .expect("didOpen should publish dependent diagnostics");

        let workspace = server
            .cached_workspace
            .as_ref()
            .expect("workspace should be materialized after diagnostics publish");
        let diagnostics = workspace
            .check_diagnostics_by_module
            .get("app.b")
            .expect("dependent module diagnostics should be tracked");
        assert!(!diagnostics.is_empty());
        assert!(
            diagnostics[0].message.contains("Producer")
                || diagnostics[0].message.contains("import")
        );
    }

    #[test]
    fn incremental_workspace_keeps_public_fingerprints_stable_for_implementation_only_changes() {
        let config = temp_workspace(
            "incremental_workspace_keeps_public_fingerprints_stable_for_implementation_only_changes",
            &[
                ("src/app/a.tpy", "def produce() -> int:\n    return 1\n"),
                ("src/app/b.tpy", "from app.a import produce\nvalue: int = produce()\n"),
            ],
        );
        let a_path = config.config_dir.join("src/app/a.tpy");
        let a_uri = path_to_uri(&a_path);
        let overlays = BTreeMap::new();
        let mut workspace =
            IncrementalWorkspace::new(config.clone(), &overlays).expect("workspace should build");
        let before_fingerprint = workspace
            .incremental
            .fingerprints
            .get("app.a")
            .copied()
            .expect("module fingerprint should exist");
        let before_b_diagnostics =
            workspace.check_diagnostics_by_module.get("app.b").cloned().unwrap_or_default();

        let overlay = OverlayDocument {
            uri: a_uri,
            text: String::from("def produce() -> int:\n    value = 1\n    return value\n"),
            version: 1,
        };
        workspace
            .apply_project_path_update(&a_path, Some(&overlay))
            .expect("overlay update should be applied incrementally");

        let after_fingerprint = workspace
            .incremental
            .fingerprints
            .get("app.a")
            .copied()
            .expect("module fingerprint should still exist");
        let after_b_diagnostics =
            workspace.check_diagnostics_by_module.get("app.b").cloned().unwrap_or_default();

        assert_eq!(before_fingerprint, after_fingerprint);
        assert_eq!(before_b_diagnostics, after_b_diagnostics);
    }

    #[test]
    fn active_support_set_changes_stay_incremental() {
        let config = temp_config(
            "active_support_set_changes_stay_incremental",
            "def run() -> None:\n    pass\n",
        );
        let path = config.config_dir.join("src/app/__init__.tpy");
        let uri = path_to_uri(&path);
        let mut server = Server::new(config.clone());

        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "text": "def run() -> None:\n    pass\n",
                        "languageId": "typepython",
                        "version": 1
                    }
                }
            }))
            .expect("didOpen should initialize the workspace");
        let workspace =
            server.cached_workspace.as_ref().expect("workspace should be cached after didOpen");
        assert!(workspace.active_support_paths.is_empty());

        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didChange",
                "params": {
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [{
                        "text": "from typing import Final\n\nVALUE: Final[int] = 1\n"
                    }]
                }
            }))
            .expect("didChange should add support modules incrementally");
        let workspace = server
            .cached_workspace
            .as_ref()
            .expect("workspace should remain cached after support activation");
        assert!(!workspace.last_state_refresh_was_full);
        assert!(!workspace.active_support_paths.is_empty());
        assert!(workspace.state.documents.len() > 1);
        assert!(
            workspace
                .state
                .documents
                .iter()
                .any(|document| document.syntax.source.kind == SourceKind::Stub)
        );

        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didChange",
                "params": {
                    "textDocument": {"uri": uri, "version": 3},
                    "contentChanges": [{
                        "text": "def run() -> None:\n    pass\n"
                    }]
                }
            }))
            .expect("didChange should remove support modules incrementally");
        let workspace = server
            .cached_workspace
            .as_ref()
            .expect("workspace should remain cached after support removal");
        assert!(!workspace.last_state_refresh_was_full);
        assert!(workspace.active_support_paths.is_empty());
        assert_eq!(workspace.state.documents.len(), 1);
        assert!(
            workspace
                .state
                .documents
                .iter()
                .all(|document| document.syntax.source.kind != SourceKind::Stub)
        );
    }

    #[test]
    fn did_close_clears_overlay() {
        let config = temp_config("did_close_clears_overlay", "def ok() -> int:\n    return 1\n");
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def ok() -> int:\n    return 1\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didClose",
                "params": {"textDocument": {"uri": uri}}
            }))
            .expect("didClose should succeed");
    }

    #[test]
    fn hover_returns_null_for_whitespace_position() {
        let config = temp_config(
            "hover_returns_null_for_whitespace_position",
            "def ok() -> int:\n    return 1\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
            .expect("source file should be readable");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let hover = server
            .handle_hover(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 1, "character": 0}
            }))
            .expect("hover should succeed");
        assert_eq!(hover, Value::Null);
    }

    #[test]
    fn definition_returns_empty_for_unresolved_symbol() {
        let config = temp_config(
            "definition_returns_empty_for_unresolved_symbol",
            "def ok() -> int:\n    return nonexistent\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
            .expect("source file should be readable");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let definition = server
            .handle_definition(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 1, "character": 11}
            }))
            .expect("definition should succeed");
        assert_eq!(definition, Value::Null);
    }

    #[test]
    fn completion_returns_items_for_empty_prefix() {
        let config = temp_config(
            "completion_returns_items_for_empty_prefix",
            "def greet() -> int:\n    return 1\n\nclass Widget:\n    pass\n\n",
        );
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
            .expect("source file should be readable");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

        let completion = server
            .handle_completion(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 5, "character": 0}
            }))
            .expect("completion should succeed");
        let labels = completion["items"]
            .as_array()
            .expect("completion items should be an array")
            .iter()
            .map(|item| item["label"].as_str().expect("label should be a string"))
            .collect::<Vec<_>>();
        assert!(labels.contains(&"greet"));
        assert!(labels.contains(&"Widget"));
    }

    #[test]
    fn code_action_returns_empty_when_no_actions_apply() {
        let config = temp_config("code_action_returns_empty_when_no_actions_apply", "x: int = 1\n");
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

        let actions = server
            .handle_code_action(json!({
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 0, "character": 1}
                },
                "context": {"diagnostics": []}
            }))
            .expect("code action should succeed");
        let actions = actions.as_array().expect("code actions should be an array");
        assert!(actions.is_empty());
    }

    #[test]
    fn multi_file_workspace_navigation() {
        let config = temp_workspace(
            "multi_file_workspace_navigation",
            &[
                ("src/app/models.tpy", "class User:\n    name: str\n"),
                (
                    "src/app/services.tpy",
                    "from app.models import User\n\ndef create_user() -> User:\n    return User()\n",
                ),
                (
                    "src/app/handlers.tpy",
                    "from app.services import create_user\nfrom app.models import User\n\ndef handle() -> User:\n    return create_user()\n",
                ),
            ],
        );
        let mut server = Server::new(config.clone());

        let models_uri = path_to_uri(&config.config_dir.join("src/app/models.tpy"));
        let services_uri = path_to_uri(&config.config_dir.join("src/app/services.tpy"));
        let handlers_uri = path_to_uri(&config.config_dir.join("src/app/handlers.tpy"));

        let models_text = fs::read_to_string(config.config_dir.join("src/app/models.tpy"))
            .expect("source file should be readable");
        let services_text = fs::read_to_string(config.config_dir.join("src/app/services.tpy"))
            .expect("source file should be readable");
        let handlers_text = fs::read_to_string(config.config_dir.join("src/app/handlers.tpy"))
            .expect("source file should be readable");

        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": models_uri, "text": models_text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen models should succeed");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": services_uri, "text": services_text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen services should succeed");
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": handlers_uri, "text": handlers_text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen handlers should succeed");

        let definition = server
            .handle_definition(json!({
                "textDocument": {"uri": handlers_uri},
                "position": {"line": 4, "character": 11}
            }))
            .expect("definition should succeed");
        let entries = definition.as_array().expect("definition should be an array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["uri"], json!(services_uri));

        let references = server
            .handle_references(json!({
                "textDocument": {"uri": models_uri},
                "position": {"line": 0, "character": 6},
                "context": {"includeDeclaration": true}
            }))
            .expect("references should succeed");
        let refs = references.as_array().expect("references should be an array");
        assert!(refs.len() >= 2);
        let ref_uris = refs
            .iter()
            .map(|r| r["uri"].as_str().expect("uri should be a string"))
            .collect::<Vec<_>>();
        assert!(ref_uris.iter().any(|uri| uri.contains("models")));
        assert!(ref_uris.iter().any(|uri| uri.contains("services")));
        assert!(ref_uris.iter().any(|uri| uri.contains("handlers")));
    }

    #[test]
    fn did_open_with_syntax_error_reports_diagnostics() {
        let config = temp_config("did_open_with_syntax_error_reports_diagnostics", "pass\n");
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

        let responses = server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def missing_colon()\n    return 1\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should publish diagnostics");
        let payload = responses
            .iter()
            .find(|response| {
                response["method"] == json!("textDocument/publishDiagnostics")
                    && response["params"]["uri"] == json!(uri)
            })
            .expect("publishDiagnostics notification should be present");
        let diagnostics = payload["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics payload should be an array");
        assert!(!diagnostics.is_empty());
    }

    fn temp_config(test_name: &str, source: &str) -> ConfigHandle {
        temp_workspace(test_name, &[("src/app/__init__.tpy", source)])
    }

    fn temp_workspace(test_name: &str, files: &[(&str, &str)]) -> ConfigHandle {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("typepython-lsp-{test_name}-{unique}"));
        fs::create_dir_all(&root).expect("workspace root should be created");
        fs::write(root.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("typepython.toml should be written");
        for (path, content) in files {
            let file_path = root.join(path);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).expect("parent directory should be created");
                let src_root = root.join("src");
                let mut current = parent.to_path_buf();
                while current.starts_with(&src_root) && current != src_root {
                    let init_path = current.join("__init__.tpy");
                    if !init_path.exists() {
                        fs::write(&init_path, "pass\n").expect("package marker should be written");
                    }
                    current =
                        current.parent().expect("parent directory should exist").to_path_buf();
                }
            }
            fs::write(file_path, content).expect("workspace file should be written");
        }
        typepython_config::load(&root).expect("workspace config should load")
    }
}

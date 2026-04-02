use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
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
    apply_type_ignore_directives, parse_with_options, prepare_syntax_tree_for_external_formatter,
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
    serve_with_io(config, io::BufReader::new(stdin), stdout.lock())
}

#[doc(hidden)]
pub fn serve_with_io<R: BufRead + Send + 'static, W: Write>(
    config: &ConfigHandle,
    reader: R,
    writer: W,
) -> Result<(), LspError> {
    let mut server = Server::new(config.clone());
    server.serve(reader, writer)
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
    queries: WorkspaceQueries,
}

#[derive(Debug, Clone, Default)]
struct WorkspaceQueries {
    documents_by_uri: BTreeMap<String, DocumentState>,
    documents_by_module_key: BTreeMap<String, DocumentState>,
    occurrences_by_uri: BTreeMap<String, Vec<SymbolOccurrence>>,
    occurrences_by_canonical: BTreeMap<String, Vec<SymbolOccurrence>>,
    nodes_by_module_key: BTreeMap<String, ModuleNode>,
}

type QueryDocumentState = (String, DocumentState, Vec<SymbolOccurrence>, Vec<SymbolOccurrence>);

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LspWorkspaceSymbol {
    name: String,
    kind: u32,
    location: LspLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    container_name: Option<String>,
}

#[derive(Debug, Clone)]
struct FormatterCommand {
    label: String,
    program: PathBuf,
    args: Vec<String>,
    explicit: bool,
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

struct Server {
    analysis: AnalysisHost,
    scheduler: LspScheduler,
    shutdown_requested: bool,
    exited: bool,
}

mod analysis;
mod formatting;
mod requests;
mod scheduler;
mod server;
mod workspace;

use analysis::*;
use formatting::*;
use requests::*;
use scheduler::*;
use workspace::*;

#[cfg(test)]
mod tests;

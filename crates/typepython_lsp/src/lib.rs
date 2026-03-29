use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use anyhow::{Context, Result};
use glob::Pattern;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use typepython_binding::bind;
use typepython_checking::check_with_options;
use typepython_config::ConfigHandle;
use typepython_diagnostics::{DiagnosticReport, Severity};
use typepython_graph::{ModuleGraph, ModuleNode, build};
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

struct Server {
    config: ConfigHandle,
    overlays: BTreeMap<PathBuf, OverlayDocument>,
    shutdown_requested: bool,
    exited: bool,
}

impl Server {
    fn new(config: ConfigHandle) -> Self {
        Self { config, overlays: BTreeMap::new(), shutdown_requested: false, exited: false }
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
                        "textDocumentSync": 1,
                        "hoverProvider": true,
                        "definitionProvider": true,
                        "referencesProvider": true,
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
        self.overlays.insert(
            uri_to_path(uri)?,
            OverlayDocument { uri: uri.to_owned(), text: text.to_owned(), version },
        );
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
        let content_changes = params
            .get("contentChanges")
            .and_then(Value::as_array)
            .ok_or_else(|| LspError::Other(String::from("didChange missing contentChanges")))?;
        if content_changes.len() != 1 {
            return Err(LspError::Other(format!(
                "TPY6002: didChange received {} content changes for `{}` but the server only supports single full-text updates",
                content_changes.len(),
                uri
            )));
        }
        let change = content_changes.first().expect("single change should exist");
        if change.get("range").is_some() || change.get("rangeLength").is_some() {
            return Err(LspError::Other(format!(
                "TPY6002: didChange for `{}` uses ranged incremental edits but the server only supports single full-text updates",
                uri
            )));
        }
        let text = change
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Other(String::from("didChange missing full text")))?;
        self.overlays
            .insert(path, OverlayDocument { uri: uri.to_owned(), text: text.to_owned(), version });
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
        Ok(vec![publish_diagnostics_notification(uri, Vec::new())])
    }

    fn publish_diagnostics(&self) -> Result<Vec<Value>, LspError> {
        let workspace = self.rebuild_workspace()?;
        let mut notifications = workspace
            .diagnostics_by_uri
            .into_iter()
            .map(|(uri, diagnostics)| publish_diagnostics_notification(&uri, diagnostics))
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

    fn handle_hover(&self, params: Value) -> Result<Value, LspError> {
        let workspace = self.rebuild_workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let Some(symbol) = resolve_symbol(&workspace, &uri, position) else {
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

    fn handle_definition(&self, params: Value) -> Result<Value, LspError> {
        let workspace = self.rebuild_workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let Some(symbol) = resolve_symbol(&workspace, &uri, position) else {
            return Ok(Value::Null);
        };
        let Some(declaration) = workspace.declarations_by_canonical.get(&symbol.canonical) else {
            return Ok(Value::Null);
        };
        Ok(json!([LspLocation { uri: declaration.uri.clone(), range: declaration.range }]))
    }

    fn handle_references(&self, params: Value) -> Result<Value, LspError> {
        let workspace = self.rebuild_workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let include_declaration = params
            .get("context")
            .and_then(|context| context.get("includeDeclaration"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let Some(symbol) = resolve_symbol(&workspace, &uri, position) else {
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

    fn handle_rename(&self, params: Value) -> Result<Value, LspError> {
        let workspace = self.rebuild_workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let new_name = params
            .get("newName")
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Other(String::from("rename missing newName")))?;
        let Some(symbol) = resolve_symbol(&workspace, &uri, position) else {
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

    fn handle_code_action(&self, params: Value) -> Result<Value, LspError> {
        let workspace = self.rebuild_workspace()?;
        let (uri, range) = text_document_range(&params)?;
        let Some(document) = workspace.documents.iter().find(|document| document.uri == uri) else {
            return Ok(json!([]));
        };

        let mut actions = Vec::new();
        actions.extend(collect_diagnostic_suggestion_code_actions(document, range, &params));
        actions.extend(collect_missing_annotation_code_actions(&workspace, document, range));
        actions.extend(collect_unsafe_code_actions(document, range, &params));
        actions.extend(collect_missing_import_code_actions(&workspace, document, range));
        Ok(json!(actions))
    }

    fn handle_completion(&self, params: Value) -> Result<Value, LspError> {
        let workspace = self.rebuild_workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let Some(document) = workspace.documents.iter().find(|document| document.uri == uri) else {
            return Ok(json!([]));
        };
        let is_member_access = line_prefix(&document.text, position).trim_end().ends_with('.');

        let items = if is_member_access {
            collect_member_completion_items(&workspace, document, position)
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

    fn rebuild_workspace(&self) -> Result<WorkspaceState, LspError> {
        let sources = collect_source_paths(&self.config, &self.overlays)?;
        let syntax_trees = load_syntax_trees(
            &sources,
            &self.overlays,
            self.config.config.typing.conditional_returns,
        )?;

        let mut parse_diagnostics = collect_parse_diagnostics(&syntax_trees);
        apply_type_ignore_directives(&syntax_trees, &mut parse_diagnostics);
        let mut diagnostics = parse_diagnostics.clone();
        let bindings = syntax_trees.iter().map(bind).collect::<Vec<_>>();
        let graph = build(&bindings);
        if !parse_diagnostics.has_errors() {
            diagnostics.diagnostics.extend(
                check_with_options(
                    &graph,
                    self.config.config.typing.require_explicit_overrides,
                    self.config.config.typing.enable_sealed_exhaustiveness,
                    self.config.config.typing.report_deprecated,
                    self.config.config.typing.strict,
                    self.config.config.typing.warn_unsafe,
                )
                .diagnostics
                .diagnostics,
            );
            apply_type_ignore_directives(&syntax_trees, &mut diagnostics);
        }

        let mut documents = syntax_trees
            .into_iter()
            .map(|syntax| {
                let text = syntax.source.text.clone();
                let uri = path_to_uri(&syntax.source.path);
                DocumentState {
                    uri,
                    path: syntax.source.path.clone(),
                    text,
                    syntax,
                    local_symbols: BTreeMap::new(),
                    local_value_types: BTreeMap::new(),
                }
            })
            .collect::<Vec<_>>();

        let mut declarations = Vec::new();
        let mut member_symbols = BTreeMap::<String, Vec<String>>::new();
        for document in &mut documents {
            let (local_symbols, declared) = collect_declarations(document);
            let local_value_types = collect_local_value_types(document, &local_symbols);
            for occurrence in &declared {
                if occurrence.canonical.matches('.').count() >= 2 {
                    member_symbols
                        .entry(occurrence.name.clone())
                        .or_default()
                        .push(occurrence.canonical.clone());
                }
            }
            document.local_symbols = local_symbols;
            document.local_value_types = local_value_types;
            declarations.extend(declared);
        }

        let mut declarations_by_canonical = BTreeMap::new();
        for occurrence in &declarations {
            declarations_by_canonical
                .entry(occurrence.canonical.clone())
                .or_insert_with(|| occurrence.clone());
        }

        let mut occurrences = declarations.clone();
        for document in &documents {
            occurrences.extend(collect_reference_occurrences(
                document,
                &member_symbols,
                &declarations_by_canonical,
            ));
        }
        dedupe_occurrences(&mut occurrences);

        let diagnostics_by_uri = diagnostics_by_uri(&documents, &diagnostics);
        Ok(WorkspaceState {
            documents,
            diagnostics_by_uri,
            occurrences,
            declarations_by_canonical,
            graph,
        })
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
                    if let Some(range) =
                        find_name_range(&document.text, statement.line, &binding.local_name)
                    {
                        declarations.push(SymbolOccurrence {
                            canonical: binding.source_path.clone(),
                            name: binding.local_name.clone(),
                            uri: document.uri.clone(),
                            range,
                            detail: format!("import {}", binding.source_path),
                            declaration: true,
                        });
                    }
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

fn collect_source_paths(
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

    let mut sources = local_sources;
    let stdlib_root = bundled_stdlib_root();
    if stdlib_root.exists() {
        walk_bundled_stdlib_directory(
            &stdlib_root,
            &stdlib_root,
            &config.config.project.target_python,
            &mut sources,
        )?;
    }

    let mut external_sources = Vec::new();
    for root in configured_external_type_roots(config) {
        walk_external_type_root(&root, &mut external_sources)?;
    }
    sort_sources_by_type_authority(&mut external_sources);
    sources.extend(external_sources);
    Ok(sources)
}

fn bundled_stdlib_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib")
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

        let Some(logical_module) = logical_module_path(&root, &path) else {
            continue;
        };
        if !sources.iter().any(|source| source.path == path) {
            sources.push(DiscoveredSource { path, kind, logical_module });
        }
    }
    Ok(())
}

fn configured_external_type_roots(config: &ConfigHandle) -> Vec<PathBuf> {
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
    roots
}

fn discovered_python_type_roots(config: &ConfigHandle) -> Vec<PathBuf> {
    let interpreter = resolve_python_executable(config);
    python_type_roots_from_interpreter(&interpreter)
}

fn python_type_roots_from_interpreter(interpreter: &Path) -> Vec<PathBuf> {
    let output = ProcessCommand::new(&interpreter)
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
        for field in metadata.trim().split_whitespace() {
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
    let Some(first) =
        relative.components().next().and_then(|component| component.as_os_str().to_str())
    else {
        return None;
    };
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
    let Some(first) = components.next().and_then(|component| component.as_os_str().to_str()) else {
        return None;
    };
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

fn load_syntax_trees(
    sources: &[DiscoveredSource],
    overlays: &BTreeMap<PathBuf, OverlayDocument>,
    enable_conditional_returns: bool,
) -> Result<Vec<SyntaxTree>> {
    sources
        .iter()
        .map(|source| {
            let mut source_file = if let Some(overlay) = overlays.get(&source.path) {
                SourceFile {
                    path: source.path.clone(),
                    kind: source.kind,
                    logical_module: source.logical_module.clone(),
                    text: overlay.text.clone(),
                }
            } else {
                let mut source_file = SourceFile::from_path(&source.path)
                    .with_context(|| format!("unable to read {}", source.path.display()))?;
                source_file.logical_module = source.logical_module.clone();
                source_file
            };
            source_file.logical_module = source.logical_module.clone();
            Ok(parse_with_options(source_file, ParseOptions { enable_conditional_returns }))
        })
        .collect()
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
        assert_eq!(capabilities["hoverProvider"], json!(true));
        assert_eq!(capabilities["definitionProvider"], json!(true));
        assert_eq!(capabilities["referencesProvider"], json!(true));
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
        let server = Server::new(config.clone());
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
        let server = Server::new(config.clone());
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
        let server = Server::new(config.clone());
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
        let server = Server::new(config.clone());
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
    fn did_change_reports_overlay_sync_failure_for_multiple_content_changes() {
        let config = temp_config(
            "did_change_reports_overlay_sync_failure_for_multiple_content_changes",
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
                        {"text": "def first() -> int:\n    return 1\n"},
                        {"text": "def second() -> int:\n    return 2\n"}
                    ]
                }
            }))
            .expect_err(
                "multi-change didChange should fail until incremental patching is supported",
            );

        assert!(error.to_string().contains("TPY6002"));
        assert!(error.to_string().contains("only supports single full-text updates"));
    }

    #[test]
    fn did_change_reports_overlay_sync_failure_for_ranged_content_change() {
        let config = temp_config(
            "did_change_reports_overlay_sync_failure_for_ranged_content_change",
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
                                "start": {"line": 0, "character": 4},
                                "end": {"line": 0, "character": 6}
                            },
                            "text": "name"
                        }
                    ]
                }
            }))
            .expect_err("ranged didChange should fail until incremental patching is supported");

        assert!(error.to_string().contains("TPY6002"));
        assert!(error.to_string().contains("ranged incremental edits"));
    }

    #[test]
    fn file_uri_helpers_round_trip_paths_with_spaces() {
        let path = PathBuf::from("/tmp/typepython spaced/project/__init__.tpy");
        let uri = path_to_uri(&path);

        assert_eq!(uri, "file:///tmp/typepython%20spaced/project/__init__.tpy");
        assert_eq!(uri_to_path(&uri).expect("URI should decode to file path"), path);
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

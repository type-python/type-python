use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
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
use typepython_graph::build;
use typepython_syntax::{SourceFile, SourceKind, SyntaxStatement, SyntaxTree, parse};

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
}

#[derive(Debug, Clone, Serialize)]
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
        Self {
            config,
            overlays: BTreeMap::new(),
            shutdown_requested: false,
            exited: false,
        }
    }

    fn serve<R: BufRead, W: Write>(&mut self, mut reader: R, mut writer: W) -> Result<(), LspError> {
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
        self.overlays.insert(
            uri_to_path(uri)?,
            OverlayDocument {
                uri: uri.to_owned(),
                text: text.to_owned(),
            },
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
        let content_changes = params
            .get("contentChanges")
            .and_then(Value::as_array)
            .ok_or_else(|| LspError::Other(String::from("didChange missing contentChanges")))?;
        let text = content_changes
            .last()
            .and_then(|change| change.get("text"))
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Other(String::from("didChange missing full text")))?;
        self.overlays.insert(
            uri_to_path(uri)?,
            OverlayDocument {
                uri: uri.to_owned(),
                text: text.to_owned(),
            },
        );
        Ok(())
    }

    fn apply_did_close(&mut self, params: Value) -> Result<Vec<Value>, LspError> {
        let uri = params
            .get("textDocument")
            .and_then(|document| document.get("uri"))
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Other(String::from("didClose missing uri")))?;
        self.overlays.remove(&uri_to_path(uri)?);
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
        Ok(json!([LspLocation {
            uri: declaration.uri.clone(),
            range: declaration.range,
        }]))
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
            .map(|occurrence| LspLocation {
                uri: occurrence.uri.clone(),
                range: occurrence.range,
            })
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
            changes.entry(occurrence.uri.clone()).or_default().push(LspTextEdit {
                range: occurrence.range,
                new_text: new_name.to_owned(),
            });
        }
        Ok(json!({"changes": changes}))
    }

    fn handle_completion(&self, params: Value) -> Result<Value, LspError> {
        let workspace = self.rebuild_workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let Some(document) = workspace.documents.iter().find(|document| document.uri == uri) else {
            return Ok(json!([]));
        };
        let is_member_access = line_prefix(&document.text, position)
            .trim_end()
            .ends_with('.');

        let items = if is_member_access {
            let mut seen = BTreeSet::new();
            workspace
                .declarations_by_canonical
                .values()
                .filter(|occurrence| occurrence.canonical.matches('.').count() >= 2)
                .filter(|occurrence| seen.insert(occurrence.name.clone()))
                .map(|occurrence| json!({"label": occurrence.name, "detail": occurrence.detail}))
                .collect::<Vec<_>>()
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
        let syntax_trees = load_syntax_trees(&sources, &self.overlays)?;

        let parse_diagnostics = collect_parse_diagnostics(&syntax_trees);
        let mut diagnostics = parse_diagnostics.clone();
        if !parse_diagnostics.has_errors() {
            let bindings = syntax_trees.iter().map(bind).collect::<Vec<_>>();
            let graph = build(&bindings);
            diagnostics.diagnostics.extend(
                check_with_options(
                    &graph,
                    self.config.config.typing.require_explicit_overrides,
                    self.config.config.typing.enable_sealed_exhaustiveness,
                )
                .diagnostics
                .diagnostics,
            );
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
                }
            })
            .collect::<Vec<_>>();

        let mut declarations = Vec::new();
        let mut member_symbols = BTreeMap::<String, Vec<String>>::new();
        for document in &mut documents {
            let (local_symbols, declared) = collect_declarations(document);
            for occurrence in &declared {
                if occurrence.canonical.matches('.').count() >= 2 {
                    member_symbols
                        .entry(occurrence.name.clone())
                        .or_default()
                        .push(occurrence.canonical.clone());
                }
            }
            document.local_symbols = local_symbols;
            declarations.extend(declared);
        }

        let mut occurrences = declarations.clone();
        for document in &documents {
            occurrences.extend(collect_reference_occurrences(document, &member_symbols));
        }
        dedupe_occurrences(&mut occurrences);

        let mut declarations_by_canonical = BTreeMap::new();
        for occurrence in &declarations {
            declarations_by_canonical
                .entry(occurrence.canonical.clone())
                .or_insert_with(|| occurrence.clone());
        }

        let diagnostics_by_uri = diagnostics_by_uri(&documents, &diagnostics);
        Ok(WorkspaceState {
            documents,
            diagnostics_by_uri,
            occurrences,
            declarations_by_canonical,
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
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|error| LspError::Other(format!("invalid Content-Length: {error}")))?,
                );
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
        .ok_or_else(|| LspError::Other(String::from("textDocument/position request missing uri")))?;
    let position: LspPosition = serde_json::from_value(
        params
            .get("position")
            .cloned()
            .ok_or_else(|| LspError::Other(String::from("textDocument/position request missing position")))?,
    )?;
    Ok((uri.to_owned(), position))
}

fn resolve_symbol<'a>(workspace: &'a WorkspaceState, uri: &str, position: LspPosition) -> Option<&'a SymbolOccurrence> {
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

fn diagnostics_by_uri(documents: &[DocumentState], diagnostics: &DiagnosticReport) -> BTreeMap<String, Vec<LspDiagnostic>> {
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
        });
    }

    by_uri
}

fn collect_declarations(document: &DocumentState) -> (BTreeMap<String, String>, Vec<SymbolOccurrence>) {
    let mut local_symbols = BTreeMap::new();
    let mut declarations = Vec::new();
    let module_key = &document.syntax.source.logical_module;

    for statement in &document.syntax.statements {
        match statement {
            SyntaxStatement::TypeAlias(statement) => {
                let canonical = format!("{module_key}.{}", statement.name);
                local_symbols.insert(statement.name.clone(), canonical.clone());
                if let Some(range) = find_name_range(&document.text, statement.line, &statement.name) {
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
                if let Some(range) = find_name_range(&document.text, statement.line, &statement.name) {
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
                    if let Some(range) = find_name_range(&document.text, member.line, &member.name) {
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
                    if let Some(range) = find_name_range(&document.text, statement.line, &binding.local_name) {
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
                            detail: format!("value {}: {}", name, statement.annotation.clone().unwrap_or_default()),
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
) -> Vec<SymbolOccurrence> {
    tokenize_identifiers(&document.text)
        .into_iter()
        .filter_map(|token| {
            let local = document.local_symbols.get(&token.name).cloned();
            let member = if token.preceded_by_dot {
                member_symbols
                    .get(&token.name)
                    .filter(|candidates| candidates.len() == 1)
                    .and_then(|candidates| candidates.first().cloned())
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
                        start: LspPosition {
                            line: line_index as u32,
                            character: start as u32,
                        },
                        end: LspPosition {
                            line: line_index as u32,
                            character: index as u32,
                        },
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
        start: LspPosition {
            line: line.saturating_sub(1) as u32,
            character: column as u32,
        },
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
    let mut sources = Vec::new();

    for root in &source_roots {
        walk_directory(config, root, &include_patterns, &exclude_patterns, &mut sources)?;
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
        if !sources.iter().any(|source| source.path == *path) {
            sources.push(DiscoveredSource {
                path: path.clone(),
                kind,
                logical_module,
            });
        }
    }

    sources.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(sources)
}

fn compile_patterns(config: &ConfigHandle, patterns: &[String]) -> Result<Vec<Pattern>> {
    patterns
        .iter()
        .map(|pattern| {
            Pattern::new(pattern).with_context(|| {
                format!(
                    "invalid glob pattern `{pattern}` in {}",
                    config.config_path.display()
                )
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
        sources.push(DiscoveredSource {
            path,
            kind,
            logical_module,
        });
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
    Ok(
        include_patterns.iter().any(|pattern| pattern.matches(&relative))
            && !exclude_patterns.iter().any(|pattern| pattern.matches(&relative)),
    )
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
    let package_components = explicit_package_components(root, parent)?;
    let stem = path.file_stem()?.to_str()?;
    if stem == "__init__" {
        return (!package_components.is_empty()).then(|| package_components.join("."));
    }
    let mut components = package_components;
    components.push(stem.to_owned());
    Some(components.join("."))
}

fn explicit_package_components(root: &Path, relative_parent: &Path) -> Option<Vec<String>> {
    let mut components = Vec::new();
    let mut current = PathBuf::new();
    for component in relative_parent.components() {
        let name = component.as_os_str().to_str()?.to_owned();
        current.push(&name);
        if !is_explicit_package_dir(&root.join(&current)) {
            return None;
        }
        components.push(name);
    }
    Some(components)
}

fn is_explicit_package_dir(directory: &Path) -> bool {
    ["__init__.py", "__init__.tpy", "__init__.pyi"]
        .iter()
        .any(|entry| directory.join(entry).is_file())
}

fn load_syntax_trees(
    sources: &[DiscoveredSource],
    overlays: &BTreeMap<PathBuf, OverlayDocument>,
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
            Ok(parse(source_file))
        })
        .collect()
}

fn collect_parse_diagnostics(syntax_trees: &[SyntaxTree]) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    for tree in syntax_trees {
        diagnostics
            .diagnostics
            .extend(tree.diagnostics.diagnostics.iter().cloned());
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
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn path_to_uri(path: &Path) -> String {
    format!("file://{}", path.to_string_lossy())
}

fn uri_to_path(uri: &str) -> Result<PathBuf, LspError> {
    let Some(path) = uri.strip_prefix("file://") else {
        return Err(LspError::Other(format!("unsupported URI `{uri}`")));
    };
    Ok(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, time::{SystemTime, UNIX_EPOCH}};

    #[test]
    fn handle_initialize_returns_required_capabilities() {
        let config = temp_config("handle_initialize_returns_required_capabilities", "pass\n");
        let mut server = Server::new(config);
        let responses = server
            .handle_message(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
            .unwrap();
        let capabilities = &responses[0]["result"]["capabilities"];
        assert_eq!(capabilities["hoverProvider"], json!(true));
        assert_eq!(capabilities["definitionProvider"], json!(true));
        assert_eq!(capabilities["referencesProvider"], json!(true));
        assert_eq!(capabilities["renameProvider"], json!(true));
    }

    #[test]
    fn did_open_publishes_overlay_diagnostics() {
        let config = temp_config("did_open_publishes_overlay_diagnostics", "def ok() -> int:\n    return 1\n");
        let mut server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
        let responses = server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def broken(:\n", "languageId": "typepython", "version": 1}}
            }))
            .unwrap();
        assert!(responses.iter().any(|response| response["method"] == json!("textDocument/publishDiagnostics")));
        let payload = responses
            .iter()
            .find(|response| response["method"] == json!("textDocument/publishDiagnostics"))
            .unwrap();
        let diagnostics = payload["params"]["diagnostics"].as_array().unwrap();
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn hover_definition_references_and_rename_work() {
        let config = temp_workspace(
            "hover_definition_references_and_rename_work",
            &[
                ("src/app/a.tpy", "def target(value: int) -> int:\n    return value\n"),
                ("src/app/b.tpy", "from app.a import target\n\ndef use() -> int:\n    return target(1)\n"),
            ],
        );
        let server = Server::new(config.clone());
        let uri = path_to_uri(&config.config_dir.join("src/app/b.tpy"));

        let hover = server
            .handle_hover(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 3, "character": 11}
            }))
            .unwrap();
        assert!(hover["contents"]["value"].as_str().unwrap().contains("function target"));

        let definition = server
            .handle_definition(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 3, "character": 11}
            }))
            .unwrap();
        assert_eq!(definition.as_array().unwrap().len(), 1);

        let references = server
            .handle_references(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 3, "character": 11},
                "context": {"includeDeclaration": true}
            }))
            .unwrap();
        assert!(references.as_array().unwrap().len() >= 2);

        let rename = server
            .handle_rename(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 3, "character": 11},
                "newName": "renamed"
            }))
            .unwrap();
        assert!(rename["changes"].is_object());
    }

    #[test]
    fn completion_returns_local_symbols_and_member_symbols() {
        let config = temp_workspace(
            "completion_returns_local_symbols_and_member_symbols",
            &[
                ("src/app/__init__.tpy", "class Box:\n    value: int\n    def method(self) -> int:\n        return self.value\n\ndef build() -> Box:\n    return Box()\n\nbox: Box = build()\nbox.method\n"),
            ],
        );
        let mut server = Server::new(config.clone());
        let path = config.config_dir.join("src/app/__init__.tpy");
        let text = fs::read_to_string(&path).unwrap();
        let uri = path_to_uri(&path);
        server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .unwrap();

        let symbols = server
            .handle_completion(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 8, "character": 3}
            }))
            .unwrap();
        assert!(symbols["items"].as_array().unwrap().iter().any(|item| item["label"] == json!("build")));

        let members = server
            .handle_completion(json!({
                "textDocument": {"uri": uri},
                "position": {"line": 9, "character": 4}
            }))
            .unwrap();
        assert!(members["items"].as_array().unwrap().iter().any(|item| item["label"] == json!("method")));
    }

    fn temp_config(test_name: &str, source: &str) -> ConfigHandle {
        temp_workspace(test_name, &[("src/app/__init__.tpy", source)])
    }

    fn temp_workspace(test_name: &str, files: &[(&str, &str)]) -> ConfigHandle {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = env::temp_dir().join(format!("typepython-lsp-{test_name}-{unique}"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n").unwrap();
        for (path, content) in files {
            let file_path = root.join(path);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).unwrap();
                let src_root = root.join("src");
                let mut current = parent.to_path_buf();
                while current.starts_with(&src_root) && current != src_root {
                    let init_path = current.join("__init__.tpy");
                    if !init_path.exists() {
                        fs::write(&init_path, "pass\n").unwrap();
                    }
                    current = current.parent().unwrap().to_path_buf();
                }
            }
            fs::write(file_path, content).unwrap();
        }
        typepython_config::load(&root).unwrap()
    }
}

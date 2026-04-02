use super::*;

impl Server {
    pub(super) fn new(config: ConfigHandle) -> Self {
        Self {
            config,
            overlays: BTreeMap::new(),
            cached_workspace: None,
            shutdown_requested: false,
            exited: false,
        }
    }

    pub(super) fn serve<R: BufRead, W: Write>(
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

    pub(super) fn handle_message(&mut self, message: Value) -> Result<Vec<Value>, LspError> {
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
                        "documentFormattingProvider": true,
                        "signatureHelpProvider": {
                            "triggerCharacters": ["(", ","]
                        },
                        "documentSymbolProvider": true,
                        "workspaceSymbolProvider": true,
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
            "textDocument/formatting" => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": self.handle_formatting(params)?
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
            "workspace/symbol" => Ok(vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": self.handle_workspace_symbol(params)?
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

    pub(super) fn apply_did_open(&mut self, params: Value) -> Result<(), LspError> {
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

    pub(super) fn apply_did_change(&mut self, params: Value) -> Result<(), LspError> {
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

    pub(super) fn apply_did_close(&mut self, params: Value) -> Result<Vec<Value>, LspError> {
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

    pub(super) fn publish_diagnostics(&mut self) -> Result<Vec<Value>, LspError> {
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

    pub(super) fn handle_hover(&mut self, params: Value) -> Result<Value, LspError> {
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

    pub(super) fn handle_definition(&mut self, params: Value) -> Result<Value, LspError> {
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

    pub(super) fn handle_references(&mut self, params: Value) -> Result<Value, LspError> {
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
            .queries
            .occurrences_by_canonical
            .get(&symbol.canonical)
            .into_iter()
            .flatten()
            .filter(|occurrence| include_declaration || !occurrence.declaration)
            .map(|occurrence| LspLocation { uri: occurrence.uri.clone(), range: occurrence.range })
            .collect::<Vec<_>>();
        Ok(json!(references))
    }

    pub(super) fn handle_formatting(&mut self, params: Value) -> Result<Value, LspError> {
        let config = self.config.clone();
        let workspace = self.workspace()?;
        let uri = params
            .get("textDocument")
            .and_then(|document| document.get("uri"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                LspError::Other(String::from("textDocument/formatting request missing uri"))
            })?;
        let Some(document) = workspace.queries.documents_by_uri.get(uri) else {
            return Ok(json!([]));
        };

        let prepared =
            prepare_syntax_tree_for_external_formatter(&document.syntax).map_err(|report| {
                LspError::Other(format!(
                    "TPY6003: unable to prepare `{}` for formatting: {}",
                    document.path.display(),
                    report.as_text().trim()
                ))
            })?;
        let formatter_output = run_formatter(
            &resolve_formatter_commands(&config, &document.path),
            prepared.formatter_input(),
        )?;
        let restored = prepared.restore(&formatter_output);
        if restored == document.text {
            return Ok(json!([]));
        }

        Ok(json!([LspTextEdit { range: full_document_range(&document.text), new_text: restored }]))
    }

    pub(super) fn handle_signature_help(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let Some(document) = workspace.queries.documents_by_uri.get(&uri) else {
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

    pub(super) fn handle_document_symbol(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let uri = params
            .get("textDocument")
            .and_then(|document| document.get("uri"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                LspError::Other(String::from("textDocument/documentSymbol request missing uri"))
            })?;
        let Some(document) = workspace.queries.documents_by_uri.get(uri) else {
            return Ok(json!([]));
        };
        Ok(json!(collect_document_symbols(document)))
    }

    pub(super) fn handle_workspace_symbol(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let query = params.get("query").and_then(Value::as_str).unwrap_or_default().to_lowercase();
        let mut symbols = workspace
            .declarations_by_canonical
            .iter()
            .filter_map(|(canonical, declaration)| {
                if !query.is_empty() {
                    let name = declaration.name.to_lowercase();
                    let canonical_name = canonical.to_lowercase();
                    if !name.contains(&query) && !canonical_name.contains(&query) {
                        return None;
                    }
                }
                let (kind, container_name) = workspace_symbol_metadata(workspace, canonical)?;
                Some(LspWorkspaceSymbol {
                    name: declaration.name.clone(),
                    kind,
                    location: LspLocation {
                        uri: declaration.uri.clone(),
                        range: declaration.range,
                    },
                    container_name,
                })
            })
            .collect::<Vec<_>>();
        symbols.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.container_name.cmp(&right.container_name))
                .then_with(|| left.location.uri.cmp(&right.location.uri))
        });
        Ok(json!(symbols))
    }

    pub(super) fn handle_rename(&mut self, params: Value) -> Result<Value, LspError> {
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
        for occurrence in
            workspace.queries.occurrences_by_canonical.get(&symbol.canonical).into_iter().flatten()
        {
            changes
                .entry(occurrence.uri.clone())
                .or_default()
                .push(LspTextEdit { range: occurrence.range, new_text: new_name.to_owned() });
        }
        Ok(json!({"changes": changes}))
    }

    pub(super) fn handle_code_action(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let (uri, range) = text_document_range(&params)?;
        let Some(document) = workspace.queries.documents_by_uri.get(&uri) else {
            return Ok(json!([]));
        };

        let mut actions = Vec::new();
        actions.extend(collect_diagnostic_suggestion_code_actions(document, range, &params));
        actions.extend(collect_missing_annotation_code_actions(workspace, document, range));
        actions.extend(collect_unsafe_code_actions(document, range, &params));
        actions.extend(collect_missing_import_code_actions(workspace, document, range));
        Ok(json!(actions))
    }

    pub(super) fn handle_completion(&mut self, params: Value) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let (uri, position) = text_document_position(&params)?;
        let Some(document) = workspace.queries.documents_by_uri.get(&uri) else {
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

    pub(super) fn workspace(&mut self) -> Result<&WorkspaceState, LspError> {
        if self.cached_workspace.is_none() {
            self.cached_workspace =
                Some(IncrementalWorkspace::new(self.config.clone(), &self.overlays)?);
        }
        Ok(self.cached_workspace.as_ref().expect("workspace cache should be populated").workspace())
    }
}

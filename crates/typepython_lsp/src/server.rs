use super::*;

impl Server {
    pub(super) fn new(config: ConfigHandle) -> Self {
        let diagnostic_debounce_ms = config.config.watch.debounce_ms;
        Self {
            analysis: AnalysisHost::new(config),
            scheduler: LspScheduler::new(diagnostic_debounce_ms),
            shutdown_requested: false,
            exited: false,
        }
    }

    pub(super) fn serve<R: BufRead + Send + 'static, W: Write>(
        &mut self,
        reader: R,
        mut writer: W,
    ) -> Result<(), LspError> {
        let (sender, receiver) = std::sync::mpsc::channel::<Result<Option<Value>, LspError>>();
        std::thread::spawn(move || {
            let mut reader = reader;
            loop {
                let next = read_message(&mut reader);
                let terminal = !matches!(next, Ok(Some(_)));
                if sender.send(next).is_err() {
                    break;
                }
                if terminal {
                    break;
                }
            }
        });

        self.scheduler.enable_background_mode();

        loop {
            let incoming = match self.scheduler.next_wait_duration() {
                Some(timeout) => match receiver.recv_timeout(timeout) {
                    Ok(next) => Some(next),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Some(Ok(None)),
                },
                None => Some(receiver.recv().unwrap_or(Ok(None))),
            };

            match incoming {
                Some(Ok(Some(message))) => {
                    for response in self.handle_message(message)? {
                        write_message(&mut writer, &response)?;
                    }
                    writer.flush()?;
                }
                Some(Ok(None)) => {
                    for response in self.scheduler.flush_all() {
                        write_message(&mut writer, &response)?;
                    }
                    writer.flush()?;
                    break;
                }
                Some(Err(error)) => return Err(error),
                None => {
                    for response in self.scheduler.flush_due_timeout() {
                        write_message(&mut writer, &response)?;
                    }
                    writer.flush()?;
                }
            }

            if self.exited {
                for response in self.scheduler.flush_all() {
                    write_message(&mut writer, &response)?;
                }
                writer.flush()?;
                break;
            }
        }

        self.scheduler.disable_background_mode();
        Ok(())
    }

    pub(super) fn handle_message(&mut self, message: Value) -> Result<Vec<Value>, LspError> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(Vec::new());
        };
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        if self.scheduler.take_cancellation(id.as_ref()) {
            return Ok(Vec::new());
        }

        let mut responses = match self.dispatch_message(method, id.clone(), params) {
            Ok(responses) => responses,
            Err(error) => match id {
                Some(id) => vec![error.jsonrpc_response(id)],
                None => Vec::new(),
            },
        };

        responses.extend(self.scheduler.flush_due_after(method));
        Ok(responses)
    }

    fn dispatch_message(
        &mut self,
        method: &str,
        id: Option<Value>,
        params: Value,
    ) -> Result<Vec<Value>, LspError> {
        match method {
            "initialize" => Ok(request_ok_response(
                id,
                json!({
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
                }),
            )
            .into_iter()
            .collect()),
            "initialized" => Ok(Vec::new()),
            "$/cancelRequest" => {
                if let Some(request_id) = params.get("id") {
                    self.scheduler.cancel_request(request_id);
                }
                Ok(Vec::new())
            }
            "shutdown" => {
                self.shutdown_requested = true;
                Ok(request_ok_response(id, Value::Null).into_iter().collect())
            }
            "exit" => {
                self.exited = true;
                Ok(Vec::new())
            }
            "textDocument/didOpen" => {
                self.apply_did_open(params)?;
                self.schedule_diagnostics_batch(Vec::new())
            }
            "textDocument/didChange" => {
                self.apply_did_change(params)?;
                self.schedule_diagnostics_batch(Vec::new())
            }
            "textDocument/didClose" => {
                let cleared = self.apply_did_close(params)?;
                self.schedule_diagnostics_batch(cleared)
            }
            "textDocument/hover" => {
                Ok(request_ok_response(id, self.handle_hover(params)?).into_iter().collect())
            }
            "textDocument/definition" => {
                Ok(request_ok_response(id, self.handle_definition(params)?).into_iter().collect())
            }
            "textDocument/references" => {
                Ok(request_ok_response(id, self.handle_references(params)?).into_iter().collect())
            }
            "textDocument/formatting" => {
                Ok(request_ok_response(id, self.handle_formatting(params)?).into_iter().collect())
            }
            "textDocument/signatureHelp" => {
                Ok(request_ok_response(id, self.handle_signature_help(params)?)
                    .into_iter()
                    .collect())
            }
            "textDocument/documentSymbol" => {
                Ok(request_ok_response(id, self.handle_document_symbol(params)?)
                    .into_iter()
                    .collect())
            }
            "workspace/symbol" => {
                Ok(request_ok_response(id, self.handle_workspace_symbol(params)?)
                    .into_iter()
                    .collect())
            }
            "textDocument/rename" => {
                Ok(request_ok_response(id, self.handle_rename(params)?).into_iter().collect())
            }
            "textDocument/codeAction" => {
                Ok(request_ok_response(id, self.handle_code_action(params)?).into_iter().collect())
            }
            "textDocument/completion" => {
                Ok(request_ok_response(id, self.handle_completion(params)?).into_iter().collect())
            }
            _ if id.is_some() => Err(LspError::method_not_found(format!(
                "JSON-RPC method `{method}` is not supported by the TypePython language server"
            ))),
            _ => Ok(Vec::new()),
        }
    }

    fn schedule_diagnostics_batch(
        &mut self,
        mut notifications: Vec<Value>,
    ) -> Result<Vec<Value>, LspError> {
        notifications.extend(self.publish_diagnostics()?);
        self.scheduler.schedule_diagnostics(notifications);
        Ok(self.scheduler.immediate_or_deferred_notifications())
    }

    pub(super) fn apply_did_open(&mut self, params: Value) -> Result<(), LspError> {
        let text_document = params.get("textDocument").ok_or_else(|| {
            LspError::invalid_params(String::from("didOpen missing `params.textDocument`"))
        })?;
        let uri = text_document.get("uri").and_then(Value::as_str).ok_or_else(|| {
            LspError::invalid_params(String::from("didOpen missing `params.textDocument.uri`"))
        })?;
        let text = text_document.get("text").and_then(Value::as_str).ok_or_else(|| {
            LspError::invalid_params(String::from("didOpen missing `params.textDocument.text`"))
        })?;
        let version = text_document.get("version").and_then(Value::as_i64).ok_or_else(|| {
            LspError::invalid_params(String::from(
                "TPY6002: didOpen missing `params.textDocument.version`",
            ))
            .with_tpy_code("TPY6002")
        })?;
        self.analysis.open_document(uri, text, version)?;
        if self.scheduler.is_background_mode() {
            self.analysis.spawn_support_index_prewarm();
        }
        Ok(())
    }

    pub(super) fn apply_did_change(&mut self, params: Value) -> Result<(), LspError> {
        let text_document = params.get("textDocument").ok_or_else(|| {
            LspError::invalid_params(String::from("didChange missing `params.textDocument`"))
        })?;
        let uri = text_document.get("uri").and_then(Value::as_str).ok_or_else(|| {
            LspError::invalid_params(String::from("didChange missing `params.textDocument.uri`"))
        })?;
        let version = text_document.get("version").and_then(Value::as_i64).ok_or_else(|| {
            LspError::invalid_params(String::from(
                "TPY6002: didChange missing `params.textDocument.version`",
            ))
            .with_tpy_code("TPY6002")
        })?;
        let raw_content_changes = params.get("contentChanges").cloned().ok_or_else(|| {
            LspError::invalid_params(String::from("didChange missing `params.contentChanges`"))
        })?;
        let content_changes: Vec<LspContentChangeEvent> =
            serde_json::from_value(raw_content_changes).map_err(|error| {
                LspError::invalid_params(format!(
                    "didChange has invalid `params.contentChanges`: {error}"
                ))
            })?;
        self.analysis.change_document(uri, version, &content_changes)?;
        if self.scheduler.is_background_mode() {
            self.analysis.spawn_support_index_prewarm();
        }
        Ok(())
    }

    pub(super) fn apply_did_close(&mut self, params: Value) -> Result<Vec<Value>, LspError> {
        let uri = params
            .get("textDocument")
            .and_then(|document| document.get("uri"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                LspError::invalid_params(String::from("didClose missing `params.textDocument.uri`"))
            })?;
        let uri = self.analysis.close_document(uri)?;
        Ok(vec![publish_diagnostics_notification(&uri, Vec::new())])
    }

    pub(super) fn publish_diagnostics(&mut self) -> Result<Vec<Value>, LspError> {
        Ok(self
            .analysis
            .publish_diagnostics()?
            .into_iter()
            .map(|(uri, diagnostics)| publish_diagnostics_notification(&uri, diagnostics))
            .collect())
    }

    pub(super) fn handle_hover(&mut self, params: Value) -> Result<Value, LspError> {
        let (uri, position) = text_document_position(&params)?;
        self.analysis.hover(&uri, position)
    }

    pub(super) fn handle_definition(&mut self, params: Value) -> Result<Value, LspError> {
        let (uri, position) = text_document_position(&params)?;
        self.analysis.definition(&uri, position)
    }

    pub(super) fn handle_references(&mut self, params: Value) -> Result<Value, LspError> {
        let (uri, position) = text_document_position(&params)?;
        let include_declaration = params
            .get("context")
            .and_then(|context| context.get("includeDeclaration"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.analysis.references(&uri, position, include_declaration)
    }

    pub(super) fn handle_formatting(&mut self, params: Value) -> Result<Value, LspError> {
        let uri = params
            .get("textDocument")
            .and_then(|document| document.get("uri"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                LspError::invalid_params(String::from(
                    "textDocument/formatting request missing `params.textDocument.uri`",
                ))
            })?;
        self.analysis.formatting(uri)
    }

    pub(super) fn handle_signature_help(&mut self, params: Value) -> Result<Value, LspError> {
        let (uri, position) = text_document_position(&params)?;
        self.analysis.signature_help(&uri, position)
    }

    pub(super) fn handle_document_symbol(&mut self, params: Value) -> Result<Value, LspError> {
        let uri = params
            .get("textDocument")
            .and_then(|document| document.get("uri"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                LspError::invalid_params(String::from(
                    "textDocument/documentSymbol request missing `params.textDocument.uri`",
                ))
            })?;
        self.analysis.document_symbol(uri)
    }

    pub(super) fn handle_workspace_symbol(&mut self, params: Value) -> Result<Value, LspError> {
        let query = params.get("query").and_then(Value::as_str).unwrap_or_default();
        self.analysis.workspace_symbol(query)
    }

    pub(super) fn handle_rename(&mut self, params: Value) -> Result<Value, LspError> {
        let (uri, position) = text_document_position(&params)?;
        let new_name = params.get("newName").and_then(Value::as_str).ok_or_else(|| {
            LspError::invalid_params(String::from("rename request missing `params.newName`"))
        })?;
        self.analysis.rename(&uri, position, new_name)
    }

    pub(super) fn handle_code_action(&mut self, params: Value) -> Result<Value, LspError> {
        let (uri, range) = text_document_range(&params)?;
        self.analysis.code_action(&uri, range, &params)
    }

    pub(super) fn handle_completion(&mut self, params: Value) -> Result<Value, LspError> {
        let (uri, position) = text_document_position(&params)?;
        self.analysis.completion(&uri, position)
    }
}

fn request_ok_response(id: Option<Value>, result: Value) -> Option<Value> {
    let id = id?;
    Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
}

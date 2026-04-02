use super::*;

impl Server {
    pub(super) fn new(config: ConfigHandle) -> Self {
        Self { analysis: AnalysisHost::new(config), shutdown_requested: false, exited: false }
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
        self.analysis.open_document(uri, text, version)
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
        let content_changes: Vec<LspContentChangeEvent> =
            serde_json::from_value(params.get("contentChanges").cloned().ok_or_else(|| {
                LspError::Other(String::from("didChange missing contentChanges"))
            })?)?;
        self.analysis.change_document(uri, version, &content_changes)
    }

    pub(super) fn apply_did_close(&mut self, params: Value) -> Result<Vec<Value>, LspError> {
        let uri = params
            .get("textDocument")
            .and_then(|document| document.get("uri"))
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Other(String::from("didClose missing uri")))?;
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
                LspError::Other(String::from("textDocument/formatting request missing uri"))
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
                LspError::Other(String::from("textDocument/documentSymbol request missing uri"))
            })?;
        self.analysis.document_symbol(uri)
    }

    pub(super) fn handle_workspace_symbol(&mut self, params: Value) -> Result<Value, LspError> {
        let query = params.get("query").and_then(Value::as_str).unwrap_or_default();
        self.analysis.workspace_symbol(query)
    }

    pub(super) fn handle_rename(&mut self, params: Value) -> Result<Value, LspError> {
        let (uri, position) = text_document_position(&params)?;
        let new_name = params
            .get("newName")
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Other(String::from("rename missing newName")))?;
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

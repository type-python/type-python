use super::*;

pub(super) struct AnalysisHost {
    pub(super) config: ConfigHandle,
    pub(super) overlays: BTreeMap<PathBuf, OverlayDocument>,
    pub(super) cached_workspace: Option<IncrementalWorkspace>,
}

impl AnalysisHost {
    pub(super) fn new(config: ConfigHandle) -> Self {
        Self { config, overlays: BTreeMap::new(), cached_workspace: None }
    }

    pub(super) fn open_document(
        &mut self,
        uri: &str,
        text: &str,
        version: i64,
    ) -> Result<(), LspError> {
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

    pub(super) fn change_document(
        &mut self,
        uri: &str,
        version: i64,
        content_changes: &[LspContentChangeEvent],
    ) -> Result<(), LspError> {
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
        if content_changes.is_empty() {
            return Err(LspError::Other(format!(
                "TPY6002: didChange received no content changes for `{}`",
                uri
            )));
        }

        let text = apply_content_changes(&current.text, content_changes, uri)?;
        self.overlays.insert(path.clone(), OverlayDocument { uri: uri.to_owned(), text, version });
        if let Some(workspace) = self.cached_workspace.as_mut() {
            let overlay =
                self.overlays.get(&path).expect("changed overlay should be available in cache");
            workspace.apply_project_path_update(&path, Some(overlay))?;
        }
        Ok(())
    }

    pub(super) fn close_document(&mut self, uri: &str) -> Result<String, LspError> {
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
        Ok(uri.to_owned())
    }

    pub(super) fn publish_diagnostics(
        &mut self,
    ) -> Result<Vec<(String, Vec<LspDiagnostic>)>, LspError> {
        let workspace = self.workspace()?;
        let mut notifications = workspace
            .diagnostics_by_uri
            .iter()
            .map(|(uri, diagnostics)| (uri.clone(), diagnostics.clone()))
            .collect::<Vec<_>>();

        for overlay in self.overlays.values() {
            if !notifications.iter().any(|(uri, _)| uri == &overlay.uri) {
                notifications.push((overlay.uri.clone(), Vec::new()));
            }
        }

        Ok(notifications)
    }

    pub(super) fn hover(&mut self, uri: &str, position: LspPosition) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let Some(symbol) = resolve_symbol(workspace, uri, position) else {
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

    pub(super) fn definition(
        &mut self,
        uri: &str,
        position: LspPosition,
    ) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let Some(symbol) = resolve_symbol(workspace, uri, position) else {
            return Ok(Value::Null);
        };
        let Some(declaration) = workspace.declarations_by_canonical.get(&symbol.canonical) else {
            return Ok(Value::Null);
        };
        Ok(json!([LspLocation { uri: declaration.uri.clone(), range: declaration.range }]))
    }

    pub(super) fn references(
        &mut self,
        uri: &str,
        position: LspPosition,
        include_declaration: bool,
    ) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let Some(symbol) = resolve_symbol(workspace, uri, position) else {
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

    pub(super) fn formatting(&mut self, uri: &str) -> Result<Value, LspError> {
        let config = self.config.clone();
        let workspace = self.workspace()?;
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

    pub(super) fn signature_help(
        &mut self,
        uri: &str,
        position: LspPosition,
    ) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let Some(document) = workspace.queries.documents_by_uri.get(uri) else {
            return Ok(Value::Null);
        };
        let Some(active_call) = active_call(document, position, uri)? else {
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

    pub(super) fn document_symbol(&mut self, uri: &str) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let Some(document) = workspace.queries.documents_by_uri.get(uri) else {
            return Ok(json!([]));
        };
        Ok(json!(collect_document_symbols(document)))
    }

    pub(super) fn workspace_symbol(&mut self, query: &str) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let query = query.to_lowercase();
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

    pub(super) fn rename(
        &mut self,
        uri: &str,
        position: LspPosition,
        new_name: &str,
    ) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let Some(symbol) = resolve_symbol(workspace, uri, position) else {
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

    pub(super) fn code_action(
        &mut self,
        uri: &str,
        range: LspRange,
        params: &Value,
    ) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let Some(document) = workspace.queries.documents_by_uri.get(uri) else {
            return Ok(json!([]));
        };

        let mut actions = Vec::new();
        actions.extend(collect_diagnostic_suggestion_code_actions(document, range, params));
        actions.extend(collect_missing_annotation_code_actions(workspace, document, range));
        actions.extend(collect_unsafe_code_actions(document, range, params));
        actions.extend(collect_missing_import_code_actions(workspace, document, range));
        Ok(json!(actions))
    }

    pub(super) fn completion(
        &mut self,
        uri: &str,
        position: LspPosition,
    ) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let Some(document) = workspace.queries.documents_by_uri.get(uri) else {
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

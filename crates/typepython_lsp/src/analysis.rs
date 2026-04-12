use super::*;

pub(super) struct AnalysisHost {
    pub(super) config: ConfigHandle,
    pub(super) overlays: BTreeMap<PathBuf, OverlayDocument>,
    pub(super) cached_workspace: Option<IncrementalWorkspace>,
    pub(super) support_index_prewarm_started: bool,
    support_index_prewarm_task: Option<std::thread::JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct WorkspaceSymbolMatch {
    exact_substring_rank: u8,
    start_index: usize,
    gap_count: usize,
    candidate_len: usize,
}

impl AnalysisHost {
    pub(super) fn new(config: ConfigHandle) -> Self {
        Self {
            config,
            overlays: BTreeMap::new(),
            cached_workspace: None,
            support_index_prewarm_started: false,
            support_index_prewarm_task: None,
        }
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
            let overlay = self.overlays.get(&path).ok_or_else(|| {
                LspError::internal(format!(
                    "overlay cache lost newly opened document `{}` before workspace update",
                    uri
                ))
            })?;
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
            LspError::invalid_params(format!(
                "TPY6002: didChange received for unopened overlay `{}`",
                uri
            ))
            .with_tpy_code("TPY6002")
        })?;
        if version <= current.version {
            return Err(LspError::content_modified(format!(
                "TPY6002: didChange version {} is out of sync with overlay version {} for `{}`",
                version, current.version, uri
            ))
            .with_tpy_code("TPY6002"));
        }
        if content_changes.is_empty() {
            return Err(LspError::invalid_params(format!(
                "TPY6002: didChange received no content changes for `{}`",
                uri
            ))
            .with_tpy_code("TPY6002"));
        }

        let text = apply_content_changes(&current.text, content_changes, uri)?;
        self.overlays.insert(path.clone(), OverlayDocument { uri: uri.to_owned(), text, version });
        if let Some(workspace) = self.cached_workspace.as_mut() {
            let overlay = self.overlays.get(&path).ok_or_else(|| {
                LspError::internal(format!(
                    "overlay cache lost changed document `{}` before workspace update",
                    uri
                ))
            })?;
            workspace.apply_project_path_update(&path, Some(overlay))?;
        }
        Ok(())
    }

    pub(super) fn close_document(&mut self, uri: &str) -> Result<String, LspError> {
        let path = uri_to_path(uri)?;
        if self.overlays.remove(&path).is_none() {
            return Err(LspError::invalid_params(format!(
                "TPY6002: didClose received for unopened overlay `{}`",
                uri
            ))
            .with_tpy_code("TPY6002"));
        }
        if let Some(workspace) = self.cached_workspace.as_mut() {
            workspace.apply_project_path_update(&path, None)?;
        }
        Ok(uri.to_owned())
    }

    pub(super) fn spawn_support_index_prewarm(&mut self) {
        if self.support_index_prewarm_started {
            return;
        }
        if self
            .cached_workspace
            .as_ref()
            .is_some_and(|workspace| workspace.support_catalog.index.is_some())
        {
            self.support_index_prewarm_started = true;
            return;
        }

        self.support_index_prewarm_started = true;
        let config = self.config.clone();
        self.support_index_prewarm_task = Some(std::thread::spawn(move || {
            let _ = typepython_project::support_source_index(
                &config,
                &config.config.project.target_python.to_string(),
            );
        }));
    }

    #[cfg(test)]
    pub(super) fn wait_for_support_index_prewarm(&mut self) {
        if let Some(task) = self.support_index_prewarm_task.take() {
            task.join().expect("support source index prewarm task should not panic");
        }
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
                LspError::request_failed(format!(
                    "TPY6003: unable to prepare `{}` for formatting: {}",
                    document.path.display(),
                    report.as_text().trim()
                ))
                .with_tpy_code("TPY6003")
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
        let candidates =
            resolve_signature_candidates(workspace, document, position, &active_call.callee);
        if candidates.is_empty() {
            return Ok(Value::Null);
        }
        let call_site = active_call_site(document, position, &active_call.callee);
        let active_signature =
            select_active_signature(&candidates, active_call.active_parameter, call_site.as_ref());
        let signatures = candidates.into_iter().map(|candidate| candidate.info).collect::<Vec<_>>();
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
                let match_score = workspace_symbol_match(
                    &query,
                    &declaration.name.to_lowercase(),
                    &canonical.to_lowercase(),
                )?;
                let (kind, container_name) = workspace_symbol_metadata(workspace, canonical)?;
                Some((
                    match_score,
                    LspWorkspaceSymbol {
                        name: declaration.name.clone(),
                        kind,
                        location: LspLocation {
                            uri: declaration.uri.clone(),
                            range: declaration.range,
                        },
                        container_name,
                    },
                ))
            })
            .collect::<Vec<_>>();
        symbols.sort_by(|(left_score, left), (right_score, right)| {
            left_score.cmp(right_score).then_with(|| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| left.container_name.cmp(&right.container_name))
                    .then_with(|| left.location.uri.cmp(&right.location.uri))
            })
        });
        Ok(json!(symbols.into_iter().map(|(_, symbol)| symbol).collect::<Vec<_>>()))
    }
}

fn workspace_symbol_match(
    query: &str,
    name: &str,
    canonical_name: &str,
) -> Option<WorkspaceSymbolMatch> {
    if query.is_empty() {
        return Some(WorkspaceSymbolMatch {
            exact_substring_rank: 1,
            start_index: 0,
            gap_count: 0,
            candidate_len: name.len(),
        });
    }
    let name_match = fuzzy_symbol_match(query, name);
    let canonical_match = fuzzy_symbol_match(query, canonical_name);
    match (name_match, canonical_match) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(score), None) | (None, Some(score)) => Some(score),
        (None, None) => None,
    }
}

fn fuzzy_symbol_match(query: &str, candidate: &str) -> Option<WorkspaceSymbolMatch> {
    if let Some(start_index) = candidate.find(query) {
        return Some(WorkspaceSymbolMatch {
            exact_substring_rank: 0,
            start_index,
            gap_count: 0,
            candidate_len: candidate.len(),
        });
    }

    let mut matched_offsets = Vec::with_capacity(query.len());
    let mut search_start = 0usize;
    for query_ch in query.chars() {
        let remainder = &candidate[search_start..];
        let (relative_offset, _) = remainder
            .char_indices()
            .find(|(_, candidate_ch)| candidate_ch.eq_ignore_ascii_case(&query_ch))?;
        let absolute_offset = search_start + relative_offset;
        matched_offsets.push(absolute_offset);
        search_start = absolute_offset + query_ch.len_utf8();
    }

    let start_index = *matched_offsets.first()?;
    let gap_count =
        matched_offsets.windows(2).map(|window| window[1].saturating_sub(window[0] + 1)).sum();
    Some(WorkspaceSymbolMatch {
        exact_substring_rank: 1,
        start_index,
        gap_count,
        candidate_len: candidate.len(),
    })
}

impl AnalysisHost {
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
            let mut items = keyword_snippet_completion_items();
            let mut seen = BTreeSet::new();

            let mut local_keys = document.local_symbols.keys().cloned().collect::<Vec<_>>();
            local_keys.sort();
            for name in local_keys {
                let mut item = completion_item_from_canonical(
                    workspace,
                    name.clone(),
                    &document.local_symbols[&name],
                );
                item.sort_text = format!("1:{}", item.sort_text);
                seen.insert(item.label.clone());
                items.push(item);
            }

            let mut workspace_candidates = workspace
                .declarations_by_canonical
                .iter()
                .filter(|(canonical, occurrence)| {
                    binding_declaration_for_canonical(workspace, canonical)
                        .is_some_and(|(_, declaration)| declaration.owner.is_none())
                        && !seen.contains(&occurrence.name)
                })
                .map(|(canonical, occurrence)| (occurrence.name.clone(), canonical.clone()))
                .collect::<Vec<_>>();
            workspace_candidates
                .sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
            for (label, canonical) in workspace_candidates {
                let mut item = completion_item_from_canonical(workspace, label, &canonical);
                item.sort_text = format!("2:{}", item.sort_text);
                items.push(item);
            }

            items
        };

        Ok(json!({"isIncomplete": false, "items": items}))
    }

    pub(super) fn workspace(&mut self) -> Result<&WorkspaceState, LspError> {
        if self.cached_workspace.is_none() {
            self.cached_workspace =
                Some(IncrementalWorkspace::new(self.config.clone(), &self.overlays)?);
        }
        let workspace = self.cached_workspace.as_ref().ok_or_else(|| {
            LspError::internal(String::from(
                "workspace cache was not populated after incremental workspace construction",
            ))
        })?;
        Ok(workspace.workspace())
    }
}

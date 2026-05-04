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
                &config.analysis_python().to_string(),
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
        let detail = binding_declaration_for_canonical(workspace, &symbol.canonical)
            .map(|(node, declaration)| {
                if let Some(detail) = projected_shape_hover_detail(workspace, node, declaration) {
                    return detail;
                }
                if let Some(effect_summary) =
                    effect_summary_hover_detail(workspace, node, declaration)
                {
                    let rendered = if declaration.owner.is_some() {
                        render_member_detail(declaration)
                    } else {
                        render_declaration_detail(declaration)
                    };
                    return format!("{rendered}\n{effect_summary}");
                }
                if declaration.owner.is_some() {
                    render_member_detail(declaration)
                } else {
                    let rendered = render_declaration_detail(declaration);
                    if let Some(reduced) = reduced_type_level_alias_hover(node, declaration) {
                        format!("{rendered}\nReduced type-level alias: {reduced}")
                    } else {
                        rendered
                    }
                }
            })
            .unwrap_or_else(|| symbol.legacy_detail.clone());
        let explanation = hover_explanation(&detail);
        Ok(json!({
            "contents": {
                "kind": "markdown",
                "value": format!("```typepython\n{}\n```\n\n{}", detail, explanation)
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

    pub(super) fn find_type_source(
        &mut self,
        uri: &str,
        position: LspPosition,
    ) -> Result<Value, LspError> {
        let workspace = self.workspace()?;
        let Some(document) = workspace.queries.documents_by_uri.get(uri) else {
            return Ok(Value::Null);
        };
        let Some(token) = token_at_position(&document.text, position) else {
            return Ok(Value::Null);
        };

        if token.name == "Any" {
            return Ok(json!({
                "kind": "Any",
                "source": "typing.Any",
                "reason": "`Any` was written explicitly or imported from Python typing; checker-portable output preserves it as an escape hatch."
            }));
        }
        if token.name == "unknown" {
            return Ok(json!({
                "kind": "Unknown",
                "source": "TypePython unknown boundary",
                "reason": "`unknown` was written explicitly in TypePython source and must be narrowed before member access, calls, or emitted public surfaces."
            }));
        }

        if let Some(canonical) = document.local_symbols.get(&token.name) {
            if let Some(declaration) = workspace.declarations_by_canonical.get(canonical) {
                return Ok(json!({
                    "kind": "Symbol",
                    "source": canonical,
                    "location": LspLocation { uri: declaration.uri.clone(), range: declaration.range },
                    "reason": "resolved to a project declaration before TypePython rendered the public type surface"
                }));
            }
            return Ok(json!({
                "kind": "Unknown",
                "source": canonical,
                "reason": "this name resolves to an import target with no project declaration, so strict analysis treats it as an unknown import boundary"
            }));
        }

        Ok(Value::Null)
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

    pub(super) fn preview_emit(&mut self, uri: &str) -> Result<Value, LspError> {
        let config = self.config.clone();
        let workspace = self.workspace()?;
        let Some(document) = workspace.queries.documents_by_uri.get(uri) else {
            return Err(LspError::invalid_params(format!(
                "typepython.previewEmit could not find open document `{uri}`"
            )));
        };
        if document.syntax.source.kind != typepython_syntax::SourceKind::TypePython {
            return Err(LspError::invalid_params(format!(
                "typepython.previewEmit only supports TypePython source files, got `{}`",
                document.path.display()
            )));
        }

        let lowering = lower_with_options(
            &document.syntax,
            &LoweringOptions {
                target_python: config.config.project.target_python,
                emit_style: config.config.emit.emit_style,
                experimental_shape_transforms: false,
            },
        );
        if lowering.diagnostics.has_errors() {
            return Err(LspError::request_failed(format!(
                "TPY6005: unable to preview emitted output for `{}`: {}",
                document.path.display(),
                lowering.diagnostics.as_text().trim()
            ))
            .with_tpy_code("TPY6005"));
        }
        let stub_source = typepython_emit::generate_typepython_stub_source(
            &lowering.module,
            &typepython_emit::TypePythonStubContext::default(),
        )
        .map_err(|error| {
            LspError::request_failed(format!(
                "TPY6005: unable to preview emitted stub for `{}`: {error}",
                document.path.display()
            ))
            .with_tpy_code("TPY6005")
        })?;

        Ok(json!({
            "command": "typepython.previewEmit",
            "uri": uri,
            "path": document.path.display().to_string(),
            "python": lowering.module.python_source,
            "stub": stub_source,
        }))
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

fn reduced_type_level_alias_hover(
    node: &ModuleNode,
    declaration: &typepython_binding::Declaration,
) -> Option<String> {
    if declaration.kind != typepython_binding::DeclarationKind::TypeAlias {
        return None;
    }
    reduce_restricted_type_level_alias_text(node, &declaration.type_alias_body_text()?)
}

fn reduce_restricted_type_level_alias_text(node: &ModuleNode, value: &str) -> Option<String> {
    let trimmed = value.trim();
    let fields = type_level_shape_fields(node);
    if let Some(keys) = typepython_syntax::reduce_key_set_alias_text(trimmed, &fields) {
        return Some(format!(
            "Literal[{}]",
            keys.into_iter().map(|key| format!("\"{key}\"")).collect::<Vec<_>>().join(", ")
        ));
    }
    typepython_syntax::reduce_typeif_is_subtype_text(trimmed)
}

fn type_level_shape_fields(node: &ModuleNode) -> Vec<typepython_syntax::TypeLevelShapeField> {
    node.declarations
        .iter()
        .filter_map(|member| {
            let owner = member.owner.as_ref()?;
            Some((owner.name.clone(), member))
        })
        .filter(|(_, member)| member.kind == typepython_binding::DeclarationKind::Value)
        .map(|(owner, member)| {
            let annotation =
                member.value_annotation().map(typepython_binding::BoundTypeExpr::render);
            typepython_syntax::TypeLevelShapeField {
                owner,
                name: member.name.clone(),
                required: typepython_syntax::type_level_shape_field_required(
                    annotation.as_deref(),
                    true,
                ),
                annotation,
            }
        })
        .collect()
}

type SharedShapeField = typepython_syntax::ShapeProjectionField;

fn projected_shape_hover_detail(
    workspace: &WorkspaceState,
    node: &ModuleNode,
    declaration: &typepython_binding::Declaration,
) -> Option<String> {
    if declaration.kind != typepython_binding::DeclarationKind::TypeAlias {
        return None;
    }
    let body = declaration.type_alias_body_text()?;
    let fields = resolve_projected_hover_shape(workspace, node, &body)?;
    let mut lines = vec![format!("Shape {}", declaration.name)];
    for field in fields {
        let mut flags = Vec::new();
        if !field.required {
            flags.push("optional");
        }
        if field.readonly {
            flags.push("readonly");
        }
        if flags.is_empty() {
            lines.push(format!("  {}: {}", field.name, field.normalized_annotation()));
        } else {
            lines.push(format!(
                "  {}: {} | {}",
                field.name,
                field.normalized_annotation(),
                flags.join(", ")
            ));
        }
    }
    Some(lines.join("\n"))
}

fn hover_explanation(detail: &str) -> &'static str {
    if detail.contains("Effect summary:") {
        "**Type explanation:** callable signature plus TypePython effect summary; effects are author-time facts used for diagnostics while emitted Python stays standard."
    } else if detail.starts_with("function ") {
        "**Type explanation:** callable signature; parameter annotations describe accepted inputs and the return annotation describes the value produced."
    } else if detail.starts_with("method ") {
        "**Type explanation:** method signature; receiver-specific overload and member resolution chose this callable surface."
    } else if detail.starts_with("value ") || detail.starts_with("field ") {
        "**Type explanation:** value type; this annotation or inferred type is what TypePython uses for downstream checks and emitted stubs."
    } else if detail.starts_with("typealias ") {
        "**Type explanation:** type alias expansion; this is the public type expression consumers see after TypePython resolves the alias."
    } else if detail.starts_with("class ") {
        "**Type explanation:** class surface; bases and generated members determine constructor, attribute, and stub behavior."
    } else if detail.starts_with("Shape ") {
        "**Type explanation:** projected shape; requiredness and readonly markers show the checker-portable TypedDict surface."
    } else {
        "**Type explanation:** resolved symbol surface used by TypePython analysis and editor navigation."
    }
}

fn effect_summary_hover_detail(
    workspace: &WorkspaceState,
    node: &ModuleNode,
    declaration: &typepython_binding::Declaration,
) -> Option<String> {
    if !matches!(
        declaration.kind,
        typepython_binding::DeclarationKind::Function
            | typepython_binding::DeclarationKind::Overload
    ) {
        return None;
    }
    let summary_key = hover_effect_summary_key(declaration);
    let summaries = hover_effect_summaries(workspace, node)?;
    let summary = summaries.get(&summary_key)?;
    if !summary.pure && summary.effects.is_empty() {
        return None;
    }
    let row = if summary.effects.is_empty() {
        String::from("pure")
    } else {
        summary.effects.iter().cloned().collect::<Vec<_>>().join(", ")
    };
    let label = if summary.inferred && !summary.effects.is_empty() {
        format!("inferred row [{row}]")
    } else if summary.pure {
        format!("pure; row [{row}]")
    } else {
        format!("row [{row}]")
    };
    Some(format!("Effect summary: {label}"))
}

#[derive(Debug, Clone, Default)]
struct HoverEffectSummary {
    pure: bool,
    effects: BTreeSet<String>,
    inferred: bool,
}

fn hover_effect_summary_key(
    declaration: &typepython_binding::Declaration,
) -> (Option<String>, String) {
    (declaration.owner.as_ref().map(|owner| owner.name.clone()), declaration.name.clone())
}

fn hover_effect_summaries(
    workspace: &WorkspaceState,
    node: &ModuleNode,
) -> Option<BTreeMap<(Option<String>, String), HoverEffectSummary>> {
    hover_effect_summaries_with_import_depth(workspace, node, 1)
}

fn hover_effect_summaries_with_import_depth(
    workspace: &WorkspaceState,
    node: &ModuleNode,
    import_depth: usize,
) -> Option<BTreeMap<(Option<String>, String), HoverEffectSummary>> {
    let document = workspace.queries.documents_by_module_key.get(&node.module_key)?;
    let decorator_info = typepython_syntax::collect_decorator_transform_module_info(&document.text);
    let framework_info = typepython_syntax::collect_framework_transform_module_info(&document.text);
    let adapter_effects = framework_effect_labels_by_provider(&framework_info);
    let mut summaries = BTreeMap::new();
    for site in decorator_info.callables {
        let summary = explicit_hover_effect_summary(&site.decorators, &adapter_effects);
        if summary.pure || !summary.effects.is_empty() {
            summaries.insert((site.owner_type_name, site.name), summary);
        }
    }
    add_stdlib_hover_effect_summaries(node, &mut summaries);
    if import_depth > 0 {
        add_imported_hover_effect_summaries(workspace, node, import_depth - 1, &mut summaries);
    }
    infer_hover_effect_summaries(&document.text, node, &mut summaries);
    Some(summaries)
}

fn explicit_hover_effect_summary(
    decorators: &[String],
    adapter_effects: &BTreeMap<String, Vec<&'static str>>,
) -> HoverEffectSummary {
    let mut summary = HoverEffectSummary::default();
    for decorator in decorators {
        let short = if decorator.contains("effect:") {
            decorator.as_str()
        } else {
            decorator.rsplit('.').next().unwrap_or(decorator.as_str())
        };
        if let Some(effect) = effect_label_from_decorator(short) {
            summary.effects.insert(effect.to_owned());
            continue;
        }
        match short {
            "effect_pure" | "pure" => summary.pure = true,
            "effect_unsafe" => {
                summary.effects.insert(String::from("unsafe"));
            }
            "effect_io_fs" => {
                summary.effects.insert(String::from("io.fs"));
            }
            "effect_io_net" => {
                summary.effects.insert(String::from("io.net"));
            }
            "effect_io_proc" => {
                summary.effects.insert(String::from("io.proc"));
            }
            "effect_time" => {
                summary.effects.insert(String::from("time"));
            }
            "effect_random" => {
                summary.effects.insert(String::from("random"));
            }
            "effect_runtime_validation" | "trusted_validator" => {
                summary.effects.insert(String::from("runtime.validation"));
            }
            "effect_taint_sanitize" | "sanitizer" => {
                summary.effects.insert(String::from("taint.sanitize"));
            }
            "must_use" | "must_call" | "must_close" | "must_dispose" | "must_await"
            | "must_consume" => {
                summary.effects.insert(String::from("resource.lifecycle"));
            }
            "source" => {
                summary.effects.insert(String::from("taint.source"));
            }
            "sink" => {
                summary.effects.insert(String::from("taint.sink"));
            }
            _ => {}
        }
        if let Some(adapter_labels) =
            adapter_effects.get(short).or_else(|| adapter_effects.get(decorator.as_str()))
        {
            summary.effects.extend(adapter_labels.iter().map(|label| (*label).to_owned()));
        }
    }
    summary
}

fn infer_hover_effect_summaries(
    document_text: &str,
    node: &ModuleNode,
    summaries: &mut BTreeMap<(Option<String>, String), HoverEffectSummary>,
) {
    let explicit_keys = summaries
        .iter()
        .filter_map(|(key, summary)| (!summary.inferred).then_some(key.clone()))
        .collect::<BTreeSet<_>>();
    for _ in 0..8 {
        let mut changed = false;
        let function_effects = hover_function_effects_by_name(summaries);
        let method_effects = hover_method_effects_by_name(summaries);
        for assignment in &node.assignments {
            let Some(owner) = assignment.owner_name.as_ref() else {
                continue;
            };
            let owner_key = (assignment.owner_type_name.clone(), owner.clone());
            if explicit_keys.contains(&owner_key) {
                continue;
            }
            if let Some(callee) = assignment.value_callee.as_deref()
                && let Some(effects) = function_effects.get(callee)
            {
                changed |=
                    extend_inferred_hover_effect_summary(summaries, owner_key.clone(), effects);
            }
            if let Some(effects) = hover_method_effects(
                &method_effects,
                assignment.value_method_owner_name.as_deref(),
                assignment.value_method_name.as_deref(),
            ) {
                changed |= extend_inferred_hover_effect_summary(summaries, owner_key, effects);
            }
        }
        for return_site in &node.returns {
            let owner_key = (return_site.owner_type_name.clone(), return_site.owner_name.clone());
            if explicit_keys.contains(&owner_key) {
                continue;
            }
            if let Some(callee) = return_site.value_callee.as_deref()
                && let Some(effects) = function_effects.get(callee)
            {
                changed |=
                    extend_inferred_hover_effect_summary(summaries, owner_key.clone(), effects);
            }
            if let Some(effects) = hover_method_effects(
                &method_effects,
                return_site.value_method_owner_name.as_deref(),
                return_site.value_method_name.as_deref(),
            ) {
                changed |= extend_inferred_hover_effect_summary(summaries, owner_key, effects);
            }
        }
        for call_site in typepython_syntax::collect_direct_call_context_sites(document_text) {
            let Some(owner) = call_site.owner_name.as_ref() else {
                continue;
            };
            let owner_key = (call_site.owner_type_name.clone(), owner.clone());
            if explicit_keys.contains(&owner_key) {
                continue;
            }
            if let Some(effects) = function_effects.get(&call_site.callee) {
                changed |= extend_inferred_hover_effect_summary(summaries, owner_key, effects);
            }
        }
        for method_call in &node.method_calls {
            let Some(owner) = method_call.current_owner_name.as_ref() else {
                continue;
            };
            let owner_key = (method_call.current_owner_type_name.clone(), owner.clone());
            if explicit_keys.contains(&owner_key) {
                continue;
            }
            if let Some(effects) = hover_method_effects(
                &method_effects,
                Some(method_call.owner_name.as_str()),
                Some(method_call.method.as_str()),
            ) {
                changed |= extend_inferred_hover_effect_summary(summaries, owner_key, effects);
            }
        }
        if !changed {
            break;
        }
    }
}

fn hover_function_effects_by_name(
    summaries: &BTreeMap<(Option<String>, String), HoverEffectSummary>,
) -> BTreeMap<String, BTreeSet<String>> {
    summaries
        .iter()
        .filter(|((owner, _), summary)| owner.is_none() && !summary.effects.is_empty())
        .map(|((_, name), summary)| (name.clone(), summary.effects.clone()))
        .collect()
}

fn hover_method_effects_by_name(
    summaries: &BTreeMap<(Option<String>, String), HoverEffectSummary>,
) -> BTreeMap<(String, String), BTreeSet<String>> {
    summaries
        .iter()
        .filter_map(|((owner, name), summary)| {
            owner.as_ref().and_then(|owner| {
                (!summary.effects.is_empty())
                    .then(|| ((owner.clone(), name.clone()), summary.effects.clone()))
            })
        })
        .collect()
}

fn hover_method_effects<'a>(
    method_effects: &'a BTreeMap<(String, String), BTreeSet<String>>,
    owner_name: Option<&str>,
    method_name: Option<&str>,
) -> Option<&'a BTreeSet<String>> {
    method_effects.get(&(owner_name?.to_owned(), method_name?.to_owned()))
}

fn extend_inferred_hover_effect_summary(
    summaries: &mut BTreeMap<(Option<String>, String), HoverEffectSummary>,
    key: (Option<String>, String),
    effects: &BTreeSet<String>,
) -> bool {
    if effects.is_empty() {
        return false;
    }
    let entry = summaries.entry(key).or_insert_with(|| HoverEffectSummary {
        pure: false,
        effects: BTreeSet::new(),
        inferred: true,
    });
    let before = entry.effects.len();
    entry.effects.extend(effects.iter().cloned());
    entry.inferred = true;
    entry.effects.len() != before
}

fn add_stdlib_hover_effect_summaries(
    node: &ModuleNode,
    summaries: &mut BTreeMap<(Option<String>, String), HoverEffectSummary>,
) {
    for declaration in node
        .declarations
        .iter()
        .filter(|declaration| declaration.owner.is_none())
        .filter(|declaration| declaration.kind == typepython_binding::DeclarationKind::Import)
    {
        let Some(target) = declaration.import_target() else {
            continue;
        };
        if let Some(symbol_target) = &target.symbol_target
            && let Some(effect) =
                stdlib_hover_effect_label(&symbol_target.module_key, &symbol_target.symbol_name)
        {
            summaries.insert(
                (None, declaration.name.clone()),
                HoverEffectSummary {
                    pure: false,
                    effects: BTreeSet::from([effect.to_owned()]),
                    inferred: true,
                },
            );
            continue;
        }
        for (method, effect) in stdlib_hover_effect_methods(&target.raw_target) {
            summaries.insert(
                (Some(declaration.name.clone()), method.to_owned()),
                HoverEffectSummary {
                    pure: false,
                    effects: BTreeSet::from([effect.to_owned()]),
                    inferred: true,
                },
            );
        }
    }
}

fn add_imported_hover_effect_summaries(
    workspace: &WorkspaceState,
    node: &ModuleNode,
    import_depth: usize,
    summaries: &mut BTreeMap<(Option<String>, String), HoverEffectSummary>,
) {
    for declaration in node
        .declarations
        .iter()
        .filter(|declaration| declaration.owner.is_none())
        .filter(|declaration| declaration.kind == typepython_binding::DeclarationKind::Import)
    {
        let Some((provider_node, target_declaration)) =
            resolve_hover_import_target(workspace, declaration)
        else {
            continue;
        };
        let Some(provider_summaries) =
            hover_effect_summaries_with_import_depth(workspace, provider_node, import_depth)
        else {
            continue;
        };
        match target_declaration.kind {
            typepython_binding::DeclarationKind::Function => {
                if let Some(summary) =
                    provider_summaries.get(&(None, target_declaration.name.clone()))
                {
                    summaries.insert((None, declaration.name.clone()), summary.clone());
                }
            }
            typepython_binding::DeclarationKind::Class => {
                for ((owner, method), summary) in &provider_summaries {
                    if owner.as_deref() == Some(target_declaration.name.as_str()) {
                        summaries.insert(
                            (Some(declaration.name.clone()), method.clone()),
                            summary.clone(),
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn resolve_hover_import_target<'a>(
    workspace: &'a WorkspaceState,
    declaration: &'a typepython_binding::Declaration,
) -> Option<(&'a ModuleNode, &'a typepython_binding::Declaration)> {
    let target = declaration.import_target()?;
    let symbol_target = target.symbol_target.as_ref()?;
    let provider_node = workspace.queries.nodes_by_module_key.get(&symbol_target.module_key)?;
    let target_declaration = provider_node.declarations.iter().find(|candidate| {
        candidate.owner.is_none() && candidate.name == symbol_target.symbol_name
    })?;
    Some((provider_node, target_declaration))
}

fn stdlib_hover_effect_methods(module: &str) -> Vec<(&'static str, &'static str)> {
    stdlib_hover_effect_symbols_for_module(module)
        .into_iter()
        .filter_map(|symbol| {
            stdlib_hover_effect_label(module, symbol).map(|effect| (symbol, effect))
        })
        .collect()
}

fn stdlib_hover_effect_symbols_for_module(module: &str) -> Vec<&'static str> {
    match module {
        "time" => vec!["monotonic", "perf_counter", "process_time", "sleep", "time"],
        "random" => vec!["choice", "choices", "randint", "random", "randrange", "seed", "shuffle"],
        "secrets" => vec!["choice", "randbelow", "token_bytes", "token_hex", "token_urlsafe"],
        "subprocess" => vec!["Popen", "call", "check_call", "check_output", "run"],
        "os" => vec![
            "chdir",
            "getcwd",
            "listdir",
            "makedirs",
            "mkdir",
            "popen",
            "remove",
            "removedirs",
            "rename",
            "replace",
            "rmdir",
            "scandir",
            "stat",
            "system",
            "unlink",
        ],
        "urllib.request" => vec!["urlopen", "urlretrieve"],
        _ => Vec::new(),
    }
}

fn stdlib_hover_effect_label(module: &str, symbol: &str) -> Option<&'static str> {
    match module {
        "time" => {
            matches!(symbol, "monotonic" | "perf_counter" | "process_time" | "sleep" | "time")
                .then_some("time")
        }
        "random" => matches!(
            symbol,
            "choice" | "choices" | "randint" | "random" | "randrange" | "seed" | "shuffle"
        )
        .then_some("random"),
        "secrets" => {
            matches!(symbol, "choice" | "randbelow" | "token_bytes" | "token_hex" | "token_urlsafe")
                .then_some("random")
        }
        "subprocess" => matches!(symbol, "Popen" | "call" | "check_call" | "check_output" | "run")
            .then_some("io.proc"),
        "os" => match symbol {
            "popen" | "system" => Some("io.proc"),
            "chdir" | "getcwd" | "listdir" | "makedirs" | "mkdir" | "remove" | "removedirs"
            | "rename" | "replace" | "rmdir" | "scandir" | "stat" | "unlink" => Some("io.fs"),
            _ => None,
        },
        "urllib.request" => matches!(symbol, "urlopen" | "urlretrieve").then_some("io.net"),
        _ => None,
    }
}

fn effect_label_from_decorator(decorator: &str) -> Option<&str> {
    let (target, label) = decorator.split_once(':')?;
    (target.rsplit('.').next() == Some("effect") && !label.is_empty()).then_some(label)
}

fn framework_effect_labels_by_provider(
    info: &typepython_syntax::FrameworkTransformModuleInfo,
) -> BTreeMap<String, Vec<&'static str>> {
    info.providers
        .iter()
        .filter(|provider| {
            provider.provider_kind
                == Some(typepython_syntax::FrameworkTransformProviderKind::FunctionDecorator)
        })
        .filter_map(|provider| {
            let labels = provider
                .capabilities
                .iter()
                .filter_map(framework_capability_effect_label)
                .collect::<Vec<_>>();
            (!labels.is_empty()).then(|| (provider.name.clone(), labels))
        })
        .collect()
}

fn framework_capability_effect_label(
    capability: &typepython_syntax::FrameworkTransformCapability,
) -> Option<&'static str> {
    match capability {
        typepython_syntax::FrameworkTransformCapability::EffectUnsafe => Some("unsafe"),
        typepython_syntax::FrameworkTransformCapability::EffectIoFs => Some("io.fs"),
        typepython_syntax::FrameworkTransformCapability::EffectIoNet => Some("io.net"),
        typepython_syntax::FrameworkTransformCapability::EffectIoProc => Some("io.proc"),
        typepython_syntax::FrameworkTransformCapability::EffectTime => Some("time"),
        typepython_syntax::FrameworkTransformCapability::EffectRandom => Some("random"),
        typepython_syntax::FrameworkTransformCapability::EffectRuntimeValidation
        | typepython_syntax::FrameworkTransformCapability::ValidatorWitness => {
            Some("runtime.validation")
        }
        typepython_syntax::FrameworkTransformCapability::EffectTaintSanitize
        | typepython_syntax::FrameworkTransformCapability::TaintSanitizer => Some("taint.sanitize"),
        typepython_syntax::FrameworkTransformCapability::TaintSource => Some("taint.source"),
        typepython_syntax::FrameworkTransformCapability::TaintSink => Some("taint.sink"),
        _ => None,
    }
}

fn resolve_projected_hover_shape(
    workspace: &WorkspaceState,
    node: &ModuleNode,
    expression: &str,
) -> Option<Vec<SharedShapeField>> {
    let expression = expression.trim();
    if let Some(inner) = bracket_inner(expression, "Partial") {
        return resolve_projected_hover_shape(workspace, node, inner)
            .map(|fields| hover_shape_from_fields("hover", fields).partial().fields);
    }
    if let Some(inner) = bracket_inner(expression, "Required_") {
        return resolve_projected_hover_shape(workspace, node, inner)
            .map(|fields| hover_shape_from_fields("hover", fields).required_fields().fields);
    }
    if let Some(inner) = bracket_inner(expression, "Readonly") {
        return resolve_projected_hover_shape(workspace, node, inner)
            .map(|fields| hover_shape_from_fields("hover", fields).readonly_fields().fields);
    }
    if let Some(inner) = bracket_inner(expression, "Mutable") {
        return resolve_projected_hover_shape(workspace, node, inner)
            .map(|fields| hover_shape_from_fields("hover", fields).mutable_fields().fields);
    }
    if let Some((target, keys)) = keyed_transform_inner(expression, "Pick") {
        return resolve_projected_hover_shape(workspace, node, target).map(|fields| {
            let keys = keys.iter().map(String::as_str).collect::<Vec<_>>();
            hover_shape_from_fields("hover", fields).pick(&keys).fields
        });
    }
    if let Some((target, keys)) = keyed_transform_inner(expression, "Omit") {
        return resolve_projected_hover_shape(workspace, node, target).map(|fields| {
            let keys = keys.iter().map(String::as_str).collect::<Vec<_>>();
            hover_shape_from_fields("hover", fields).omit(&keys).fields
        });
    }
    source_hover_shape(workspace, node, expression)
}

fn source_hover_shape(
    workspace: &WorkspaceState,
    node: &ModuleNode,
    type_name: &str,
) -> Option<Vec<SharedShapeField>> {
    let document = workspace.queries.documents_by_module_key.get(&node.module_key)?;
    if let Some(fields) = document.syntax.statements.iter().find_map(|statement| match statement {
        SyntaxStatement::ClassDef(class)
            if class.name == type_name
                && class.bases.iter().any(|base| {
                    matches!(
                        base.as_str(),
                        "TypedDict" | "typing.TypedDict" | "typing_extensions.TypedDict"
                    )
                }) =>
        {
            Some(typepython_syntax::ShapeProjection::from_typed_dict_block(class).fields)
        }
        SyntaxStatement::DataClass(class) if class.name == type_name => Some(
            typepython_syntax::ShapeProjection::from_named_block(
                class,
                typepython_syntax::ShapeProjectionSourceKind::DataClass,
            )
            .fields,
        ),
        SyntaxStatement::ClassDef(class) if class.name == type_name && !class.bases.is_empty() => {
            Some(
                typepython_syntax::ShapeProjection::from_named_block(
                    class,
                    typepython_syntax::ShapeProjectionSourceKind::FrameworkTransform,
                )
                .fields,
            )
        }
        _ => None,
    }) {
        return Some(fields);
    }

    dataclass_or_framework_hover_shape(&document.text, type_name)
}

fn dataclass_or_framework_hover_shape(
    source: &str,
    type_name: &str,
) -> Option<Vec<SharedShapeField>> {
    let metadata = typepython_syntax::collect_module_surface_metadata(source);
    let class =
        metadata.dataclass_transform.classes.iter().find(|class| class.name == type_name)?;
    if !class_uses_shape_provider(class, &metadata.framework_transform.providers) {
        return None;
    }
    Some(
        class
            .fields
            .iter()
            .filter(|field| !field.is_class_var)
            .map(|field| typepython_syntax::ShapeProjectionField {
                name: field.name.clone(),
                public_alias: field
                    .field_specifier_alias
                    .clone()
                    .unwrap_or_else(|| field.name.clone()),
                annotation: Some(field.rendered_annotation()),
                required: !(field.has_default
                    || field.field_specifier_has_default
                    || field.field_specifier_has_default_factory),
                readonly: class.plain_dataclass_frozen
                    || field.field_specifier_frozen == Some(true),
                source_kind: typepython_syntax::ShapeProjectionFieldSourceKind::SourceField,
            })
            .collect(),
    )
}

fn class_uses_shape_provider(
    class: &typepython_syntax::DataclassTransformClassSite,
    framework_providers: &[typepython_syntax::FrameworkTransformProviderSite],
) -> bool {
    class.plain_dataclass_frozen
        || class.plain_dataclass_kw_only
        || !class.decorators.is_empty()
        || (!class.bases.is_empty() && !framework_providers.is_empty())
        || framework_providers.iter().any(|provider| match provider.provider_kind {
            Some(typepython_syntax::FrameworkTransformProviderKind::ClassDecorator) => {
                class.decorators.iter().any(|decorator| decorator == &provider.name)
            }
            Some(typepython_syntax::FrameworkTransformProviderKind::BaseClass) => {
                class.bases.iter().any(|base| base == &provider.name)
            }
            Some(typepython_syntax::FrameworkTransformProviderKind::Metaclass) => {
                class.metaclass.as_deref() == Some(provider.name.as_str())
            }
            _ => false,
        })
}

fn hover_shape_from_fields(
    name: &str,
    fields: Vec<SharedShapeField>,
) -> typepython_syntax::ShapeProjection {
    typepython_syntax::ShapeProjection {
        name: name.to_owned(),
        source_kind: typepython_syntax::ShapeProjectionSourceKind::TypedDict,
        fields,
    }
}

fn bracket_inner<'a>(expression: &'a str, transform: &str) -> Option<&'a str> {
    let prefix = format!("{transform}[");
    expression.strip_prefix(&prefix)?.strip_suffix(']')
}

fn keyed_transform_inner<'a>(
    expression: &'a str,
    transform: &str,
) -> Option<(&'a str, BTreeSet<String>)> {
    let inner = bracket_inner(expression, transform)?;
    let mut parts = inner.split(',');
    let target = parts.next()?.trim();
    let keys = parts
        .filter_map(|part| {
            let trimmed = part.trim();
            trimmed.strip_prefix('"').and_then(|value| value.strip_suffix('"')).map(str::to_owned)
        })
        .collect::<BTreeSet<_>>();
    (!keys.is_empty()).then_some((target, keys))
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
        actions.extend(collect_common_migration_code_actions(document, range));
        actions.extend(collect_diagnostic_suggestion_code_actions(document, range, params));
        actions.extend(collect_missing_annotation_code_actions(workspace, document, range));
        actions.extend(collect_unsafe_code_actions(document, range, params));
        actions.extend(collect_effect_declaration_code_actions(document, range, params));
        actions.extend(collect_missing_import_code_actions(workspace, document, range));
        actions.extend(collect_portable_typing_rewrite_code_actions(document, range));
        actions.extend(collect_type_source_code_actions(document, range));
        actions.extend(collect_project_workflow_code_actions(document));
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

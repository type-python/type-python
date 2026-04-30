use super::*;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum EffectKind {
    Unsafe,
    IoFs,
    IoNet,
    IoProc,
    Time,
    Random,
    RuntimeValidation,
    TaintSanitize,
    Declared(String),
}

impl EffectKind {
    fn label(&self) -> String {
        match self {
            Self::Unsafe => String::from("unsafe"),
            Self::IoFs => String::from("io.fs"),
            Self::IoNet => String::from("io.net"),
            Self::IoProc => String::from("io.proc"),
            Self::Time => String::from("time"),
            Self::Random => String::from("random"),
            Self::RuntimeValidation => String::from("runtime.validation"),
            Self::TaintSanitize => String::from("taint.sanitize"),
            Self::Declared(name) => name.clone(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct EffectRow {
    effects: BTreeSet<EffectKind>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct CapabilityScope {
    pub(super) line_start: usize,
    pub(super) line_end: usize,
    pub(super) grants: EffectRow,
    pub(super) source: EffectSource,
}

impl EffectRow {
    fn empty() -> Self {
        Self { effects: BTreeSet::new() }
    }

    fn insert(&mut self, effect: EffectKind) {
        self.effects.insert(effect);
    }

    fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    fn labels(&self) -> Vec<String> {
        self.effects.iter().map(EffectKind::label).collect()
    }

    fn from_labels(labels: impl IntoIterator<Item = String>) -> Self {
        let mut row = Self::empty();
        for label in labels {
            row.insert(effect_kind_from_label(&label));
        }
        row
    }

    fn covers(&self, required: &EffectRow) -> bool {
        required.effects.iter().all(|effect| self.effects.contains(effect))
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum EffectSource {
    Decorator(String),
    Lifecycle(String),
    UnsafeBlock,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct EffectSummary {
    pub(super) callable: String,
    pub(super) owner_type_name: Option<String>,
    pub(super) pure: bool,
    pub(super) row: EffectRow,
    pub(super) sources: Vec<EffectSource>,
    pub(super) line: usize,
}

impl EffectSummary {
    fn display_name(&self) -> String {
        self.owner_type_name
            .as_ref()
            .map(|owner| format!("{owner}.{}", self.callable))
            .unwrap_or_else(|| self.callable.clone())
    }
}

pub(super) fn effect_capability_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    strict: bool,
) -> Vec<Diagnostic> {
    if !strict || node.module_kind != SourceKind::TypePython {
        return Vec::new();
    }

    let mut diagnostics = taint_source_sink_diagnostics(context, node);
    let summaries = collect_visible_effect_summaries(context, node);
    let capability_scopes = collect_capability_scopes(context, node);
    if summaries.is_empty() {
        return diagnostics;
    }

    let effectful_by_name = effectful_summaries_by_name(&summaries);
    if effectful_by_name.is_empty() {
        return diagnostics;
    }

    let summary_by_name = summaries
        .iter()
        .map(|summary| (summary.callable.as_str(), summary))
        .collect::<BTreeMap<_, _>>();

    let mut reported = BTreeSet::new();
    for assignment in &node.assignments {
        let Some(owner) = assignment.owner_name.as_deref() else {
            continue;
        };
        let Some(callee) = assignment.value_callee.as_deref() else {
            continue;
        };
        if let Some(callee_summary) = effectful_by_name.get(callee)
            && !caller_effect_row_allows(summary_by_name.get(owner).copied(), &callee_summary.row)
            && !capability_scopes_allow(&capability_scopes, assignment.line, &callee_summary.row)
            && reported.insert((owner.to_owned(), assignment.line, callee_summary.display_name()))
        {
            diagnostics.push(effect_call_diagnostic(
                node,
                owner,
                summary_by_name.get(owner).copied(),
                callee_summary,
                assignment.line,
            ));
        }
    }
    for return_site in &node.returns {
        let Some(callee) = return_site.value_callee.as_deref() else {
            continue;
        };
        if let Some(callee_summary) = effectful_by_name.get(callee)
            && !caller_effect_row_allows(
                summary_by_name.get(return_site.owner_name.as_str()).copied(),
                &callee_summary.row,
            )
            && !capability_scopes_allow(&capability_scopes, return_site.line, &callee_summary.row)
            && reported.insert((
                return_site.owner_name.clone(),
                return_site.line,
                callee_summary.display_name(),
            ))
        {
            diagnostics.push(effect_call_diagnostic(
                node,
                &return_site.owner_name,
                summary_by_name.get(return_site.owner_name.as_str()).copied(),
                callee_summary,
                return_site.line,
            ));
        }
    }
    for call_site in context.load_direct_call_context_sites(node) {
        let Some(owner) = call_site.owner_name.as_deref() else {
            continue;
        };
        if let Some(callee_summary) = effectful_by_name.get(&call_site.callee)
            && !caller_effect_row_allows(summary_by_name.get(owner).copied(), &callee_summary.row)
            && !capability_scopes_allow(&capability_scopes, call_site.line, &callee_summary.row)
            && reported.insert((owner.to_owned(), call_site.line, callee_summary.display_name()))
        {
            diagnostics.push(effect_call_diagnostic(
                node,
                owner,
                summary_by_name.get(owner).copied(),
                callee_summary,
                call_site.line,
            ));
        }
    }
    diagnostics
}

pub(super) fn collect_effect_summary_facts(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<SummaryEffectFact> {
    let mut facts = collect_effect_summaries(context, node)
        .into_iter()
        .map(|summary| SummaryEffectFact {
            name: summary.callable,
            owner_type_name: summary.owner_type_name,
            pure: summary.pure,
            effects: summary.row.labels(),
            sources: summary
                .sources
                .into_iter()
                .map(|source| match source {
                    EffectSource::Decorator(name) => name,
                    EffectSource::Lifecycle(name) => name,
                    EffectSource::UnsafeBlock => String::from("unsafe:"),
                })
                .collect(),
            line: summary.line,
        })
        .collect::<Vec<_>>();
    facts.sort_by(|left, right| {
        left.owner_type_name
            .cmp(&right.owner_type_name)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.line.cmp(&right.line))
    });
    facts
}

fn collect_visible_effect_summaries(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<EffectSummary> {
    let mut summaries = collect_effect_summaries(context, node);
    summaries.extend(collect_imported_effect_summaries(context, node));
    summaries
}

fn effectful_summaries_by_name(summaries: &[EffectSummary]) -> BTreeMap<String, &EffectSummary> {
    let mut by_name = BTreeMap::new();
    for summary in summaries.iter().filter(|summary| !summary.row.is_empty()) {
        by_name.entry(summary.callable.clone()).or_insert(summary);
    }
    by_name
}

fn collect_imported_effect_summaries(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<EffectSummary> {
    node.declarations
        .iter()
        .filter(|declaration| declaration.owner.is_none())
        .filter(|declaration| declaration.kind == DeclarationKind::Import)
        .filter_map(|declaration| {
            let target = resolve_imported_symbol_semantic_target_from_declaration(
                context.nodes,
                declaration,
            )?;
            let target_declaration = target.declaration_target()?;
            if target_declaration.owner.is_some()
                || target_declaration.kind != DeclarationKind::Function
            {
                return None;
            }
            let fact =
                collect_effect_summary_facts(context, target.provider_node).into_iter().find(
                    |fact| fact.owner_type_name.is_none() && fact.name == target_declaration.name,
                )?;
            Some(effect_summary_from_fact(declaration.name.clone(), fact))
        })
        .collect()
}

fn effect_summary_from_fact(local_name: String, fact: SummaryEffectFact) -> EffectSummary {
    EffectSummary {
        callable: local_name,
        owner_type_name: fact.owner_type_name,
        pure: fact.pure,
        row: EffectRow::from_labels(fact.effects),
        sources: fact.sources.into_iter().map(EffectSource::Decorator).collect(),
        line: fact.line,
    }
}

fn caller_effect_row_allows(caller: Option<&EffectSummary>, required: &EffectRow) -> bool {
    caller.is_some_and(|summary| !summary.pure && summary.row.covers(required))
}

fn capability_scopes_allow(scopes: &[CapabilityScope], line: usize, row: &EffectRow) -> bool {
    !row.is_empty()
        && scopes.iter().any(|scope| {
            line >= scope.line_start && line <= scope.line_end && scope.grants.covers(row)
        })
}

fn taint_source_sink_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<Diagnostic> {
    let Some(info) = context.load_decorator_transform_module_info(node) else {
        return Vec::new();
    };
    let decorated = info
        .callables
        .into_iter()
        .map(|site| {
            let decorators = site
                .decorators
                .into_iter()
                .filter_map(|decorator| decorator_short_name(&decorator).map(str::to_owned))
                .collect::<BTreeSet<_>>();
            (site.name, decorators)
        })
        .collect::<BTreeMap<_, _>>();
    let adapter_taint_decorators = adapter_taint_decorators(context, node);
    let decorated = decorated
        .into_iter()
        .map(|(name, mut decorators)| {
            for decorator in decorators.clone() {
                if let Some(adapter_fact) = adapter_taint_decorators.get(&decorator) {
                    decorators.insert(adapter_fact.clone());
                }
            }
            (name, decorators)
        })
        .collect::<BTreeMap<_, _>>();
    let mut diagnostics = Vec::new();
    let mut tainted_locals: BTreeMap<Option<String>, BTreeSet<String>> = BTreeMap::new();
    let owner_ranges = owner_line_ranges(node);
    let owner_markers = owner_line_markers(node);
    let mut assignments = node.assignments.iter().peekable();
    let mut calls = node.calls.iter().peekable();

    while assignments.peek().is_some() || calls.peek().is_some() {
        let next_assignment_line = assignments.peek().map(|assignment| assignment.line);
        let next_call_line = calls.peek().map(|call| call.line);
        if next_call_line.is_some()
            && (next_assignment_line.is_none() || next_call_line <= next_assignment_line)
        {
            let call = calls.next().expect("peeked call exists");
            let owner = owner_for_line(&owner_ranges, &owner_markers, call.line);
            if decorated.get(&call.callee).is_some_and(|decorators| decorators.contains("sink"))
                && call.arg_values.iter().any(|arg| {
                    argument_is_unsanitized_source(
                        arg,
                        &decorated,
                        &tainted_locals,
                        owner.as_deref(),
                    )
                })
            {
                diagnostics.push(
                    Diagnostic::error(
                        "TPY4028",
                        format!(
                            "tainted source result flows into sink `{}` without a sanitizer",
                            call.callee
                        ),
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        call.line,
                        1,
                        call.line,
                        1,
                    ))
                    .with_note(
                        "wrap the source value in a callable marked `@sanitizer` before passing it to a `@sink`",
                    ),
                );
            }
        } else {
            let assignment = assignments.next().expect("peeked assignment exists");
            let owner = assignment.owner_name.clone();
            let is_tainted = assignment.value.as_ref().is_some_and(|value| {
                argument_is_unsanitized_source(value, &decorated, &tainted_locals, owner.as_deref())
            }) || assignment
                .value_callee
                .as_deref()
                .is_some_and(|callee| decorated.get(callee).is_some_and(|d| d.contains("source")));
            let is_sanitized = assignment.value_callee.as_deref().is_some_and(|callee| {
                decorated.get(callee).is_some_and(|d| d.contains("sanitizer"))
            });
            let locals = tainted_locals.entry(owner).or_default();
            if is_sanitized {
                locals.remove(&assignment.name);
            } else if is_tainted {
                locals.insert(assignment.name.clone());
            } else {
                locals.remove(&assignment.name);
            }
        }
    }
    diagnostics
}

fn adapter_taint_decorators(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<String, String> {
    let Some(info) = context.load_framework_transform_module_info(node) else {
        return BTreeMap::new();
    };
    info.providers
        .into_iter()
        .filter(|provider| {
            provider.provider_kind
                == Some(typepython_syntax::FrameworkTransformProviderKind::FunctionDecorator)
        })
        .filter_map(|provider| {
            let taint_fact =
                provider.capabilities.iter().find_map(|capability| match capability {
                    typepython_syntax::FrameworkTransformCapability::TaintSource => Some("source"),
                    typepython_syntax::FrameworkTransformCapability::TaintSink => Some("sink"),
                    typepython_syntax::FrameworkTransformCapability::TaintSanitizer => {
                        Some("sanitizer")
                    }
                    _ => None,
                })?;
            Some((provider.name, taint_fact.to_owned()))
        })
        .collect()
}

fn argument_is_unsanitized_source(
    arg: &typepython_syntax::DirectExprMetadata,
    decorated: &BTreeMap<String, BTreeSet<String>>,
    tainted_locals: &BTreeMap<Option<String>, BTreeSet<String>>,
    owner_name: Option<&str>,
) -> bool {
    if let Some(callee) = arg.value_callee.as_deref() {
        if decorated.get(callee).is_some_and(|decorators| decorators.contains("sanitizer")) {
            return false;
        }
        if decorated.get(callee).is_some_and(|decorators| decorators.contains("source")) {
            return true;
        }
    }
    if let Some(name) = arg.value_name.as_deref()
        && tainted_locals
            .get(&owner_name.map(str::to_owned))
            .is_some_and(|locals| locals.contains(name))
    {
        return true;
    }
    arg.value_bool_left.as_deref().is_some_and(|inner| {
        argument_is_unsanitized_source(inner, decorated, tainted_locals, owner_name)
    }) || arg.value_bool_right.as_deref().is_some_and(|inner| {
        argument_is_unsanitized_source(inner, decorated, tainted_locals, owner_name)
    })
}

fn owner_line_ranges(
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<Option<String>, (usize, usize)> {
    let mut ranges: BTreeMap<Option<String>, (usize, usize)> = BTreeMap::new();
    for assignment in &node.assignments {
        extend_owner_range(&mut ranges, assignment.owner_name.clone(), assignment.line);
    }
    for return_site in &node.returns {
        extend_owner_range(&mut ranges, Some(return_site.owner_name.clone()), return_site.line);
    }
    ranges
}

fn owner_line_markers(node: &typepython_graph::ModuleNode) -> Vec<(usize, Option<String>)> {
    let mut markers = node
        .assignments
        .iter()
        .map(|assignment| (assignment.line, assignment.owner_name.clone()))
        .chain(
            node.returns
                .iter()
                .map(|return_site| (return_site.line, Some(return_site.owner_name.clone()))),
        )
        .collect::<Vec<_>>();
    markers.sort_by_key(|(line, _)| *line);
    markers
}

fn extend_owner_range(
    ranges: &mut BTreeMap<Option<String>, (usize, usize)>,
    owner: Option<String>,
    line: usize,
) {
    ranges
        .entry(owner)
        .and_modify(|(start, end)| {
            *start = (*start).min(line);
            *end = (*end).max(line);
        })
        .or_insert((line, line));
}

fn owner_for_line(
    ranges: &BTreeMap<Option<String>, (usize, usize)>,
    markers: &[(usize, Option<String>)],
    line: usize,
) -> Option<String> {
    ranges
        .iter()
        .find_map(|(owner, (start, end))| {
            owner.as_ref().and_then(|name| (*start <= line && line <= *end).then(|| name.clone()))
        })
        .or_else(|| {
            markers
                .iter()
                .rev()
                .find(|(marker_line, _)| *marker_line <= line)
                .and_then(|(_, owner)| owner.clone())
        })
}

fn collect_capability_scopes(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<CapabilityScope> {
    let mut ranges = context.load_unsafe_capability_ranges(node);
    if ranges.is_empty() {
        ranges = context
            .load_unsafe_operation_sites(node)
            .into_iter()
            .filter(|site| site.in_unsafe_block)
            .map(|site| (site.line, site.line))
            .collect();
    }
    ranges
        .into_iter()
        .map(|(line_start, line_end)| {
            let mut grants = EffectRow::empty();
            grants.insert(EffectKind::Unsafe);
            CapabilityScope { line_start, line_end, grants, source: EffectSource::UnsafeBlock }
        })
        .collect()
}

fn collect_effect_summaries(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<EffectSummary> {
    let Some(info) = context.load_decorator_transform_module_info(node) else {
        return Vec::new();
    };
    info.callables
        .into_iter()
        .filter_map(|site| {
            let mut pure = false;
            let mut row = EffectRow::empty();
            let mut sources = Vec::new();
            for decorator in &site.decorators {
                let short = decorator_short_name(decorator).unwrap_or(decorator.as_str());
                if matches!(short, "effect_pure" | "pure") {
                    pure = true;
                    sources.push(EffectSource::Decorator(decorator.clone()));
                } else if let Some(effect) = effect_kind_from_decorator(short) {
                    row.insert(effect);
                    sources.push(EffectSource::Decorator(decorator.clone()));
                } else if lifecycle_effect_decorator(short) {
                    row.insert(EffectKind::Declared(String::from("resource.lifecycle")));
                    sources.push(EffectSource::Lifecycle(decorator.clone()));
                }
            }
            (pure || !row.is_empty()).then_some(EffectSummary {
                callable: site.name,
                owner_type_name: site.owner_type_name,
                pure,
                row,
                sources,
                line: site.line,
            })
        })
        .collect()
}

fn decorator_short_name(decorator: &str) -> Option<&str> {
    decorator.contains("effect:").then_some(decorator).or_else(|| decorator.rsplit('.').next())
}

fn lifecycle_effect_decorator(short: &str) -> bool {
    matches!(
        short,
        "must_use" | "must_call" | "must_close" | "must_dispose" | "must_await" | "must_consume"
    )
}

fn effect_kind_from_decorator(short: &str) -> Option<EffectKind> {
    if let Some(effect) = effect_label_from_decorator(short) {
        return Some(effect_kind_from_label(effect));
    }
    match short {
        "effect" | "effectful" => Some(EffectKind::Declared(String::from("declared"))),
        "effect_unsafe" => Some(EffectKind::Unsafe),
        "effect_io_fs" => Some(EffectKind::IoFs),
        "effect_io_net" => Some(EffectKind::IoNet),
        "effect_io_proc" => Some(EffectKind::IoProc),
        "effect_time" => Some(EffectKind::Time),
        "effect_random" => Some(EffectKind::Random),
        "effect_runtime_validation" => Some(EffectKind::RuntimeValidation),
        "effect_taint_sanitize" => Some(EffectKind::TaintSanitize),
        "source" => Some(EffectKind::Declared(String::from("taint.source"))),
        "sink" => Some(EffectKind::Declared(String::from("taint.sink"))),
        "sanitizer" => Some(EffectKind::TaintSanitize),
        "trusted_validator" => Some(EffectKind::RuntimeValidation),
        _ => None,
    }
}

fn effect_label_from_decorator(decorator: &str) -> Option<&str> {
    let (target, label) = decorator.split_once(':')?;
    (target.rsplit('.').next() == Some("effect") && !label.is_empty()).then_some(label)
}

fn effect_kind_from_label(label: &str) -> EffectKind {
    match label {
        "unsafe" => EffectKind::Unsafe,
        "io.fs" => EffectKind::IoFs,
        "io.net" => EffectKind::IoNet,
        "io.proc" => EffectKind::IoProc,
        "time" => EffectKind::Time,
        "random" => EffectKind::Random,
        "runtime.validation" => EffectKind::RuntimeValidation,
        "taint.sanitize" => EffectKind::TaintSanitize,
        other => EffectKind::Declared(other.to_owned()),
    }
}

fn effect_call_diagnostic(
    node: &typepython_graph::ModuleNode,
    owner: &str,
    caller: Option<&EffectSummary>,
    callee: &EffectSummary,
    line: usize,
) -> Diagnostic {
    let labels = callee.row.labels().join(", ");
    let source_note = effect_source_note(callee);
    let caller_kind = caller
        .map_or("function", |summary| if summary.pure { "pure function" } else { "function" });
    Diagnostic::warning(
        "TPY4026",
        format!(
            "{caller_kind} `{owner}` calls effectful function `{}` with uncovered effect row `{labels}`",
            callee.display_name(),
        ),
    )
    .with_span(Span::new(node.module_path.display().to_string(), line, 1, line, 1))
    .with_note(format!(
        "{}; add an effect declaration to the caller, remove `@effect_pure`, or isolate the call behind an explicit capability boundary",
        source_note,
    ))
}

fn effect_source_note(summary: &EffectSummary) -> String {
    let source = summary
        .sources
        .first()
        .map(|source| match source {
            EffectSource::Decorator(name) => format!("effect declared by `{name}`"),
            EffectSource::Lifecycle(name) => format!("resource effect inferred from `{name}`"),
            EffectSource::UnsafeBlock => String::from("capability granted by `unsafe:` scope"),
        })
        .unwrap_or_else(|| String::from("effect source is unknown"));
    format!("{source} on line {}", summary.line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_rows_render_deterministically() {
        let mut row = EffectRow::empty();
        row.insert(EffectKind::Random);
        row.insert(EffectKind::IoNet);

        assert_eq!(row.labels(), vec![String::from("io.net"), String::from("random")]);
    }

    #[test]
    fn capability_scope_can_represent_unsafe_grants() {
        let mut grants = EffectRow::empty();
        grants.insert(EffectKind::Unsafe);
        let scope = CapabilityScope {
            line_start: 3,
            line_end: 5,
            grants,
            source: EffectSource::UnsafeBlock,
        };

        assert_eq!(scope.grants.labels(), vec![String::from("unsafe")]);
    }
}

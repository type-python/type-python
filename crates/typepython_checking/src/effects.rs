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

    fn extend(&mut self, other: &EffectRow) {
        self.effects.extend(other.effects.iter().cloned());
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
    Inferred(String),
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

    fn key(&self) -> (Option<String>, String) {
        (self.owner_type_name.clone(), self.callable.clone())
    }

    fn has_declared_effect_row(&self) -> bool {
        !self.row.is_empty()
            && self.sources.iter().any(|source| !matches!(source, EffectSource::Inferred(_)))
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

    let effectful_by_name = effectful_function_summaries_by_name(&summaries);
    let effectful_by_method = effectful_method_summaries_by_name(&summaries);
    if effectful_by_name.is_empty() && effectful_by_method.is_empty() {
        return diagnostics;
    }

    let summary_by_name = function_summaries_by_name(&summaries);
    let summary_by_method = method_summaries_by_name(&summaries);

    let mut reported = BTreeSet::new();
    for assignment in &node.assignments {
        let Some(owner) = assignment.owner_name.as_deref() else {
            continue;
        };
        let caller = caller_effect_summary(
            &summary_by_name,
            &summary_by_method,
            owner,
            assignment.owner_type_name.as_deref(),
        );
        if let Some(callee) = assignment.value_callee.as_deref()
            && let Some(callee_summary) = effectful_by_name.get(callee).copied()
        {
            maybe_push_effect_call_diagnostic(
                &mut diagnostics,
                &mut reported,
                node,
                &capability_scopes,
                owner,
                caller,
                callee_summary,
                assignment.line,
            );
        }
        if let Some(callee_summary) = effectful_method_summary(
            &effectful_by_method,
            assignment.value_method_owner_name.as_deref(),
            assignment.value_method_name.as_deref(),
        ) {
            maybe_push_effect_call_diagnostic(
                &mut diagnostics,
                &mut reported,
                node,
                &capability_scopes,
                owner,
                caller,
                callee_summary,
                assignment.line,
            );
        }
    }
    for return_site in &node.returns {
        let caller = caller_effect_summary(
            &summary_by_name,
            &summary_by_method,
            &return_site.owner_name,
            return_site.owner_type_name.as_deref(),
        );
        if let Some(callee) = return_site.value_callee.as_deref()
            && let Some(callee_summary) = effectful_by_name.get(callee).copied()
        {
            maybe_push_effect_call_diagnostic(
                &mut diagnostics,
                &mut reported,
                node,
                &capability_scopes,
                &return_site.owner_name,
                caller,
                callee_summary,
                return_site.line,
            );
        }
        if let Some(callee_summary) = effectful_method_summary(
            &effectful_by_method,
            return_site.value_method_owner_name.as_deref(),
            return_site.value_method_name.as_deref(),
        ) {
            maybe_push_effect_call_diagnostic(
                &mut diagnostics,
                &mut reported,
                node,
                &capability_scopes,
                &return_site.owner_name,
                caller,
                callee_summary,
                return_site.line,
            );
        }
    }
    for call_site in context.load_direct_call_context_sites(node) {
        let Some(owner) = call_site.owner_name.as_deref() else {
            continue;
        };
        let caller = caller_effect_summary(
            &summary_by_name,
            &summary_by_method,
            owner,
            call_site.owner_type_name.as_deref(),
        );
        if let Some(callee_summary) = effectful_by_name.get(&call_site.callee).copied() {
            maybe_push_effect_call_diagnostic(
                &mut diagnostics,
                &mut reported,
                node,
                &capability_scopes,
                owner,
                caller,
                callee_summary,
                call_site.line,
            );
        }
    }
    for method_call in &node.method_calls {
        let Some(owner) = method_call.current_owner_name.as_deref() else {
            continue;
        };
        let caller = caller_effect_summary(
            &summary_by_name,
            &summary_by_method,
            owner,
            method_call.current_owner_type_name.as_deref(),
        );
        if let Some(callee_summary) = effectful_method_summary(
            &effectful_by_method,
            Some(method_call.owner_name.as_str()),
            Some(method_call.method.as_str()),
        ) {
            maybe_push_effect_call_diagnostic(
                &mut diagnostics,
                &mut reported,
                node,
                &capability_scopes,
                owner,
                caller,
                callee_summary,
                method_call.line,
            );
        }
    }
    diagnostics
}

pub(super) fn collect_effect_summary_facts(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<SummaryEffectFact> {
    let local_summaries = collect_local_effect_summaries(context, node);
    let mut summaries = local_summaries.clone();
    summaries.extend(collect_inferred_effect_summaries(context, node, &local_summaries));
    let mut facts = summaries
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
                    EffectSource::Inferred(name) => format!("inferred:{name}"),
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
    let mut summaries = collect_local_effect_summaries(context, node);
    summaries.extend(collect_imported_effect_summaries(context, node));
    summaries.extend(collect_inferred_effect_summaries(context, node, &summaries));
    summaries
}

fn collect_inferred_effect_summaries(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    seeds: &[EffectSummary],
) -> Vec<EffectSummary> {
    let declared_keys = seeds
        .iter()
        .filter(|summary| summary.pure || summary.has_declared_effect_row())
        .map(EffectSummary::key)
        .collect::<BTreeSet<_>>();
    let mut summaries = seeds.to_vec();
    let mut inferred = BTreeMap::<(Option<String>, String), EffectSummary>::new();

    for _ in 0..8 {
        let function_effects = effectful_function_summaries_by_name(&summaries);
        let method_effects = effectful_method_summaries_by_name(&summaries);
        let mut changed = false;

        for edge in effect_call_edges(context, node) {
            if declared_keys.contains(&edge.owner_key()) {
                continue;
            }
            let callee = match &edge.callee {
                EffectCallCallee::Function(callee) => function_effects.get(callee).copied(),
                EffectCallCallee::Method { owner_type_name, method_name } => {
                    method_effects.get(&(owner_type_name.clone(), method_name.clone())).copied()
                }
            };
            let Some(callee) = callee else {
                continue;
            };
            if callee.row.is_empty() {
                continue;
            }
            let key = edge.owner_key();
            let entry = inferred.entry(key.clone()).or_insert_with(|| EffectSummary {
                callable: key.1.clone(),
                owner_type_name: key.0.clone(),
                pure: false,
                row: EffectRow::empty(),
                sources: Vec::new(),
                line: edge.line,
            });
            let before = entry.row.clone();
            entry.row.extend(&callee.row);
            let inferred_source = EffectSource::Inferred(callee.display_name());
            if !entry.sources.contains(&inferred_source) {
                entry.sources.push(inferred_source);
            }
            entry.line = entry.line.min(edge.line);
            if entry.row != before {
                changed = true;
            }
        }

        summaries = seeds.iter().cloned().chain(inferred.values().cloned()).collect();
        if !changed {
            break;
        }
    }

    inferred.into_values().collect()
}

#[derive(Debug, Clone)]
struct EffectCallEdge {
    owner_type_name: Option<String>,
    owner_name: String,
    callee: EffectCallCallee,
    line: usize,
}

impl EffectCallEdge {
    fn owner_key(&self) -> (Option<String>, String) {
        (self.owner_type_name.clone(), self.owner_name.clone())
    }
}

#[derive(Debug, Clone)]
enum EffectCallCallee {
    Function(String),
    Method { owner_type_name: String, method_name: String },
}

fn effect_call_edges(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<EffectCallEdge> {
    let mut edges = Vec::new();
    for assignment in &node.assignments {
        let Some(owner_name) = assignment.owner_name.clone() else {
            continue;
        };
        if let Some(callee) = assignment.value_callee.clone() {
            edges.push(EffectCallEdge {
                owner_type_name: assignment.owner_type_name.clone(),
                owner_name: owner_name.clone(),
                callee: EffectCallCallee::Function(callee),
                line: assignment.line,
            });
        }
        collect_expr_method_call_edges(
            assignment.value.as_ref(),
            assignment.owner_type_name.clone(),
            owner_name,
            assignment.line,
            &mut edges,
        );
    }
    for return_site in &node.returns {
        if let Some(callee) = return_site.value_callee.clone() {
            edges.push(EffectCallEdge {
                owner_type_name: return_site.owner_type_name.clone(),
                owner_name: return_site.owner_name.clone(),
                callee: EffectCallCallee::Function(callee),
                line: return_site.line,
            });
        }
        collect_expr_method_call_edges(
            return_site.value.as_ref(),
            return_site.owner_type_name.clone(),
            return_site.owner_name.clone(),
            return_site.line,
            &mut edges,
        );
    }
    for call_site in context.load_direct_call_context_sites(node) {
        let Some(owner_name) = call_site.owner_name.clone() else {
            continue;
        };
        edges.push(EffectCallEdge {
            owner_type_name: call_site.owner_type_name,
            owner_name,
            callee: EffectCallCallee::Function(call_site.callee),
            line: call_site.line,
        });
    }
    for method_call in &node.method_calls {
        let Some(owner_name) = method_call.current_owner_name.clone() else {
            continue;
        };
        edges.push(EffectCallEdge {
            owner_type_name: method_call.current_owner_type_name.clone(),
            owner_name,
            callee: EffectCallCallee::Method {
                owner_type_name: method_call.owner_name.clone(),
                method_name: method_call.method.clone(),
            },
            line: method_call.line,
        });
    }
    edges
}

fn collect_expr_method_call_edges(
    value: Option<&typepython_syntax::DirectExprMetadata>,
    owner_type_name: Option<String>,
    owner_name: String,
    line: usize,
    edges: &mut Vec<EffectCallEdge>,
) {
    let Some(value) = value else {
        return;
    };
    if let (Some(callee_owner), Some(method_name)) =
        (&value.value_method_owner_name, &value.value_method_name)
    {
        edges.push(EffectCallEdge {
            owner_type_name: owner_type_name.clone(),
            owner_name: owner_name.clone(),
            callee: EffectCallCallee::Method {
                owner_type_name: callee_owner.clone(),
                method_name: method_name.clone(),
            },
            line,
        });
    }
    collect_expr_method_call_edges(
        value.value_bool_left.as_deref(),
        owner_type_name.clone(),
        owner_name.clone(),
        line,
        edges,
    );
    collect_expr_method_call_edges(
        value.value_bool_right.as_deref(),
        owner_type_name.clone(),
        owner_name.clone(),
        line,
        edges,
    );
    collect_expr_method_call_edges(
        value.value_if_true.as_deref(),
        owner_type_name.clone(),
        owner_name.clone(),
        line,
        edges,
    );
    collect_expr_method_call_edges(
        value.value_if_false.as_deref(),
        owner_type_name,
        owner_name,
        line,
        edges,
    );
}

fn function_summaries_by_name(summaries: &[EffectSummary]) -> BTreeMap<String, &EffectSummary> {
    let mut by_name = BTreeMap::new();
    for summary in summaries.iter().filter(|summary| summary.owner_type_name.is_none()) {
        by_name.entry(summary.callable.clone()).or_insert(summary);
    }
    by_name
}

fn method_summaries_by_name(
    summaries: &[EffectSummary],
) -> BTreeMap<(String, String), &EffectSummary> {
    let mut by_name = BTreeMap::new();
    for summary in summaries.iter().filter_map(|summary| {
        summary
            .owner_type_name
            .as_ref()
            .map(|owner| ((owner.clone(), summary.callable.clone()), summary))
    }) {
        by_name.entry(summary.0).or_insert(summary.1);
    }
    by_name
}

fn effectful_function_summaries_by_name(
    summaries: &[EffectSummary],
) -> BTreeMap<String, &EffectSummary> {
    function_summaries_by_name(summaries)
        .into_iter()
        .filter(|(_, summary)| !summary.row.is_empty())
        .collect()
}

fn effectful_method_summaries_by_name(
    summaries: &[EffectSummary],
) -> BTreeMap<(String, String), &EffectSummary> {
    method_summaries_by_name(summaries)
        .into_iter()
        .filter(|(_, summary)| !summary.row.is_empty())
        .collect()
}

fn caller_effect_summary<'a>(
    by_name: &'a BTreeMap<String, &'a EffectSummary>,
    by_method: &'a BTreeMap<(String, String), &'a EffectSummary>,
    owner_name: &str,
    owner_type_name: Option<&str>,
) -> Option<&'a EffectSummary> {
    owner_type_name
        .and_then(|owner_type_name| {
            by_method.get(&(owner_type_name.to_owned(), owner_name.to_owned())).copied()
        })
        .or_else(|| by_name.get(owner_name).copied())
}

fn effectful_method_summary<'a>(
    by_method: &'a BTreeMap<(String, String), &'a EffectSummary>,
    owner_name: Option<&str>,
    method_name: Option<&str>,
) -> Option<&'a EffectSummary> {
    let owner_name = owner_name?;
    let method_name = method_name?;
    by_method.get(&(owner_name.to_owned(), method_name.to_owned())).copied()
}

#[expect(
    clippy::too_many_arguments,
    reason = "effect diagnostics share the same caller/callee/capability tuple across call surfaces"
)]
fn maybe_push_effect_call_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    reported: &mut BTreeSet<(String, usize, String)>,
    node: &typepython_graph::ModuleNode,
    capability_scopes: &[CapabilityScope],
    owner: &str,
    caller: Option<&EffectSummary>,
    callee: &EffectSummary,
    line: usize,
) {
    if !caller_effect_row_allows(caller, &callee.row)
        && !capability_scopes_allow(capability_scopes, line, &callee.row)
        && reported.insert((owner.to_owned(), line, callee.display_name()))
    {
        diagnostics.push(effect_call_diagnostic(node, owner, caller, callee, line));
    }
}

fn collect_imported_effect_summaries(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<EffectSummary> {
    let mut summaries = Vec::new();
    for declaration in node
        .declarations
        .iter()
        .filter(|declaration| declaration.owner.is_none())
        .filter(|declaration| declaration.kind == DeclarationKind::Import)
    {
        let Some(target) =
            resolve_imported_symbol_semantic_target_from_declaration(context.nodes, declaration)
        else {
            continue;
        };
        let Some(target_declaration) = target.declaration_target() else {
            continue;
        };
        if target_declaration.owner.is_some() {
            continue;
        }

        let facts = collect_effect_summary_facts(context, target.provider_node);
        match target_declaration.kind {
            DeclarationKind::Function => {
                if let Some(fact) = facts.into_iter().find(|fact| {
                    fact.owner_type_name.is_none() && fact.name == target_declaration.name
                }) {
                    summaries
                        .push(effect_summary_from_function_fact(declaration.name.clone(), fact));
                }
            }
            DeclarationKind::Class => {
                summaries.extend(facts.into_iter().filter_map(|fact| {
                    (fact.owner_type_name.as_deref() == Some(target_declaration.name.as_str()))
                        .then(|| effect_summary_from_method_fact(declaration.name.clone(), fact))
                }));
            }
            _ => {}
        }
    }
    summaries
}

fn effect_summary_from_function_fact(local_name: String, fact: SummaryEffectFact) -> EffectSummary {
    effect_summary_from_fact(local_name, None, fact)
}

fn effect_summary_from_method_fact(
    local_owner_type_name: String,
    fact: SummaryEffectFact,
) -> EffectSummary {
    effect_summary_from_fact(fact.name.clone(), Some(local_owner_type_name), fact)
}

fn effect_summary_from_fact(
    callable: String,
    owner_type_name: Option<String>,
    fact: SummaryEffectFact,
) -> EffectSummary {
    EffectSummary {
        callable,
        owner_type_name,
        pure: fact.pure,
        row: EffectRow::from_labels(fact.effects),
        sources: fact.sources.into_iter().map(effect_source_from_fact_source).collect(),
        line: fact.line,
    }
}

fn effect_source_from_fact_source(source: String) -> EffectSource {
    source
        .strip_prefix("inferred:")
        .map(|name| EffectSource::Inferred(name.to_owned()))
        .unwrap_or(EffectSource::Decorator(source))
}

fn caller_effect_row_allows(caller: Option<&EffectSummary>, required: &EffectRow) -> bool {
    caller.is_some_and(|summary| {
        !summary.pure && summary.has_declared_effect_row() && summary.row.covers(required)
    })
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
    let adapter_taint_decorators = adapter_taint_decorators(context, node);
    let mut decorated = BTreeMap::new();
    let mut decorated_methods = BTreeMap::new();
    for site in info.callables {
        let mut decorators = BTreeSet::new();
        for decorator in site.decorators {
            decorators.extend(taint_decorator_facts_from_decorator(&decorator));
            if let Some(short) = decorator_short_name(&decorator)
                && let Some(adapter_fact) = adapter_taint_decorators.get(short)
            {
                decorators.insert(adapter_fact.clone());
            }
            if let Some(adapter_fact) = adapter_taint_decorators.get(decorator.as_str()) {
                decorators.insert(adapter_fact.clone());
            }
        }
        if decorators.is_empty() {
            continue;
        }
        if let Some(owner_type_name) = site.owner_type_name {
            decorated_methods.insert((owner_type_name, site.name), decorators);
        } else {
            decorated.insert(site.name, decorators);
        }
    }
    for (name, facts) in imported_taint_decorators(context, node) {
        decorated.entry(name).or_default().extend(facts);
    }
    for (method, facts) in imported_taint_method_decorators(context, node) {
        decorated_methods.entry(method).or_default().extend(facts);
    }
    let mut diagnostics = Vec::new();
    let mut tainted_locals: BTreeMap<Option<String>, BTreeSet<String>> = BTreeMap::new();
    let mut source_method_call_lines: BTreeSet<(Option<String>, usize)> = BTreeSet::new();
    let mut sanitizer_method_call_lines: BTreeSet<(Option<String>, usize)> = BTreeSet::new();
    let owner_ranges = owner_line_ranges(node);
    let owner_markers = owner_line_markers(node);

    let mut events = Vec::new();
    events.extend(node.assignments.iter().map(TaintEvent::Assignment));
    events.extend(node.calls.iter().map(TaintEvent::Call));
    events.extend(node.method_calls.iter().map(TaintEvent::MethodCall));
    events.sort_by_key(TaintEvent::sort_key);

    for event in events {
        match event {
            TaintEvent::Call(call) => {
                let owner = owner_for_line(&owner_ranges, &owner_markers, call.line);
                if decorated.get(&call.callee).is_some_and(|decorators| decorators.contains("sink"))
                    && call.arg_values.iter().any(|arg| {
                        argument_is_unsanitized_source(
                            arg,
                            &decorated,
                            &decorated_methods,
                            &tainted_locals,
                            owner.as_deref(),
                        )
                    })
                {
                    diagnostics.push(taint_sink_diagnostic(node, &call.callee, call.line));
                }
            }
            TaintEvent::MethodCall(call) => {
                let owner = call
                    .current_owner_name
                    .clone()
                    .or_else(|| owner_for_line(&owner_ranges, &owner_markers, call.line));
                let method_facts = taint_method_facts(
                    &decorated_methods,
                    Some(call.owner_name.as_str()),
                    Some(call.method.as_str()),
                );
                if method_facts.is_some_and(|decorators| decorators.contains("source")) {
                    source_method_call_lines.insert((owner.clone(), call.line));
                }
                if method_facts.is_some_and(|decorators| decorators.contains("sanitizer")) {
                    sanitizer_method_call_lines.insert((owner.clone(), call.line));
                }
                if method_facts.is_some_and(|decorators| decorators.contains("sink"))
                    && call.arg_values.iter().any(|arg| {
                        argument_is_unsanitized_source(
                            arg,
                            &decorated,
                            &decorated_methods,
                            &tainted_locals,
                            owner.as_deref(),
                        )
                    })
                {
                    diagnostics.push(taint_sink_diagnostic(
                        node,
                        &format!("{}.{}", call.owner_name, call.method),
                        call.line,
                    ));
                }
            }
            TaintEvent::Assignment(assignment) => {
                let owner = assignment.owner_name.clone();
                let owner_line = (owner.clone(), assignment.line);
                let is_tainted = assignment.value.as_ref().is_some_and(|value| {
                    argument_is_unsanitized_source(
                        value,
                        &decorated,
                        &decorated_methods,
                        &tainted_locals,
                        owner.as_deref(),
                    )
                }) || assignment.value_callee.as_deref().is_some_and(|callee| {
                    decorated.get(callee).is_some_and(|d| d.contains("source"))
                }) || taint_method_facts(
                    &decorated_methods,
                    assignment.value_method_owner_name.as_deref(),
                    assignment.value_method_name.as_deref(),
                )
                .is_some_and(|decorators| decorators.contains("source"));
                let is_sanitized = assignment.value_callee.as_deref().is_some_and(|callee| {
                    decorated.get(callee).is_some_and(|d| d.contains("sanitizer"))
                }) || taint_method_facts(
                    &decorated_methods,
                    assignment.value_method_owner_name.as_deref(),
                    assignment.value_method_name.as_deref(),
                )
                .is_some_and(|decorators| decorators.contains("sanitizer"));
                let is_tainted = is_tainted || source_method_call_lines.contains(&owner_line);
                let is_sanitized =
                    is_sanitized || sanitizer_method_call_lines.contains(&owner_line);
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
    }
    diagnostics
}

enum TaintEvent<'a> {
    Assignment(&'a typepython_binding::AssignmentSite),
    Call(&'a typepython_binding::CallSite),
    MethodCall(&'a typepython_binding::MethodCallSite),
}

impl TaintEvent<'_> {
    fn sort_key(&self) -> (usize, u8) {
        match self {
            Self::Call(call) => (call.line, 0),
            Self::MethodCall(call) => (call.line, 0),
            Self::Assignment(assignment) => (assignment.line, 1),
        }
    }
}

fn taint_sink_diagnostic(
    node: &typepython_graph::ModuleNode,
    sink_name: &str,
    line: usize,
) -> Diagnostic {
    Diagnostic::error(
        "TPY4028",
        format!("tainted source result flows into sink `{sink_name}` without a sanitizer"),
    )
    .with_span(Span::new(node.module_path.display().to_string(), line, 1, line, 1))
    .with_note(
        "wrap the source value in a callable marked `@sanitizer` before passing it to a `@sink`",
    )
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

fn imported_taint_decorators(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<String, BTreeSet<String>> {
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
            let facts = taint_decorator_facts_from_effect_summary(&fact);
            (!facts.is_empty()).then(|| (declaration.name.clone(), facts))
        })
        .collect()
}

fn imported_taint_method_decorators(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut imported = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for declaration in node
        .declarations
        .iter()
        .filter(|declaration| declaration.owner.is_none())
        .filter(|declaration| declaration.kind == DeclarationKind::Import)
    {
        let Some(target) =
            resolve_imported_symbol_semantic_target_from_declaration(context.nodes, declaration)
        else {
            continue;
        };
        let Some(target_declaration) = target.declaration_target() else {
            continue;
        };
        if target_declaration.owner.is_some() || target_declaration.kind != DeclarationKind::Class {
            continue;
        }
        for fact in
            collect_effect_summary_facts(context, target.provider_node).into_iter().filter(|fact| {
                fact.owner_type_name.as_deref() == Some(target_declaration.name.as_str())
            })
        {
            let facts = taint_decorator_facts_from_effect_summary(&fact);
            if !facts.is_empty() {
                imported.entry((declaration.name.clone(), fact.name)).or_default().extend(facts);
            }
        }
    }
    imported
}

fn taint_decorator_facts_from_effect_summary(fact: &SummaryEffectFact) -> BTreeSet<String> {
    let mut facts = BTreeSet::new();
    for effect in &fact.effects {
        if let Some(fact) = taint_fact_from_effect_label(effect) {
            facts.insert(fact.to_owned());
        }
    }
    facts
}

fn taint_decorator_facts_from_decorator(decorator: &str) -> BTreeSet<String> {
    let short = decorator_short_name(decorator).unwrap_or(decorator);
    let mut facts = BTreeSet::new();
    match short {
        "source" => {
            facts.insert(String::from("source"));
        }
        "sink" => {
            facts.insert(String::from("sink"));
        }
        "sanitizer" | "effect_taint_sanitize" => {
            facts.insert(String::from("sanitizer"));
        }
        _ => {}
    }
    if let Some(label) = effect_label_from_decorator(short)
        && let Some(fact) = taint_fact_from_effect_label(label)
    {
        facts.insert(fact.to_owned());
    }
    facts
}

fn taint_fact_from_effect_label(label: &str) -> Option<&'static str> {
    match label {
        "taint.source" => Some("source"),
        "taint.sink" => Some("sink"),
        "taint.sanitize" => Some("sanitizer"),
        _ => None,
    }
}

fn adapter_effect_rows(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<String, (EffectRow, EffectSource)> {
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
            let mut row = EffectRow::empty();
            for capability in &provider.capabilities {
                if let Some(effect) = framework_capability_effect_kind(capability) {
                    row.insert(effect);
                }
            }
            (!row.is_empty()).then(|| {
                (
                    provider.name.clone(),
                    (
                        row,
                        EffectSource::Decorator(format!(
                            "framework_transform capability on `{}`",
                            provider.name
                        )),
                    ),
                )
            })
        })
        .collect()
}

fn framework_capability_effect_kind(
    capability: &typepython_syntax::FrameworkTransformCapability,
) -> Option<EffectKind> {
    match capability {
        typepython_syntax::FrameworkTransformCapability::EffectUnsafe => Some(EffectKind::Unsafe),
        typepython_syntax::FrameworkTransformCapability::EffectIoFs => Some(EffectKind::IoFs),
        typepython_syntax::FrameworkTransformCapability::EffectIoNet => Some(EffectKind::IoNet),
        typepython_syntax::FrameworkTransformCapability::EffectIoProc => Some(EffectKind::IoProc),
        typepython_syntax::FrameworkTransformCapability::EffectTime => Some(EffectKind::Time),
        typepython_syntax::FrameworkTransformCapability::EffectRandom => Some(EffectKind::Random),
        typepython_syntax::FrameworkTransformCapability::EffectRuntimeValidation
        | typepython_syntax::FrameworkTransformCapability::ValidatorWitness => {
            Some(EffectKind::RuntimeValidation)
        }
        typepython_syntax::FrameworkTransformCapability::EffectTaintSanitize
        | typepython_syntax::FrameworkTransformCapability::TaintSanitizer => {
            Some(EffectKind::TaintSanitize)
        }
        typepython_syntax::FrameworkTransformCapability::TaintSource => {
            Some(EffectKind::Declared(String::from("taint.source")))
        }
        typepython_syntax::FrameworkTransformCapability::TaintSink => {
            Some(EffectKind::Declared(String::from("taint.sink")))
        }
        _ => None,
    }
}

fn argument_is_unsanitized_source(
    arg: &typepython_syntax::DirectExprMetadata,
    decorated: &BTreeMap<String, BTreeSet<String>>,
    decorated_methods: &BTreeMap<(String, String), BTreeSet<String>>,
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
    if let Some(decorators) = taint_method_facts(
        decorated_methods,
        arg.value_method_owner_name.as_deref(),
        arg.value_method_name.as_deref(),
    ) {
        if decorators.contains("sanitizer") {
            return false;
        }
        if decorators.contains("source") {
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
        argument_is_unsanitized_source(
            inner,
            decorated,
            decorated_methods,
            tainted_locals,
            owner_name,
        )
    }) || arg.value_bool_right.as_deref().is_some_and(|inner| {
        argument_is_unsanitized_source(
            inner,
            decorated,
            decorated_methods,
            tainted_locals,
            owner_name,
        )
    })
}

fn taint_method_facts<'a>(
    decorated_methods: &'a BTreeMap<(String, String), BTreeSet<String>>,
    owner_name: Option<&str>,
    method_name: Option<&str>,
) -> Option<&'a BTreeSet<String>> {
    let owner_name = owner_name?;
    let method_name = method_name?;
    decorated_methods.get(&(owner_name.to_owned(), method_name.to_owned()))
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

fn collect_local_effect_summaries(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Vec<EffectSummary> {
    let Some(info) = context.load_decorator_transform_module_info(node) else {
        return Vec::new();
    };
    let adapter_effects = adapter_effect_rows(context, node);
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
                if let Some((adapter_row, adapter_source)) =
                    adapter_effects.get(short).or_else(|| adapter_effects.get(decorator.as_str()))
                {
                    row.extend(adapter_row);
                    sources.push(adapter_source.clone());
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
    let callee_kind = if callee.owner_type_name.is_some() { "method" } else { "function" };
    Diagnostic::warning(
        "TPY4026",
        format!(
            "{caller_kind} `{owner}` calls effectful {callee_kind} `{}` with uncovered effect row `{labels}`",
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
            EffectSource::Inferred(name) => format!("effect inferred from `{name}`"),
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

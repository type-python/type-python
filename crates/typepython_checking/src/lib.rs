//! Type-checking boundary for TypePython.
//!
//! Naming conventions used throughout this crate:
//! - `direct_*` operates on the directly bound surface for a module: declaration
//!   details, call sites, returns, assignments, and member accesses extracted by
//!   `typepython_binding`, before any secondary import/runtime synthesis.
//! - `contextual_*` refines a local expression using an expected type supplied by
//!   the surrounding assignment, call, or return site.
//! - `imported_*` handles behavior that depends on information loaded from an
//!   imported module rather than the current module's direct sites.
//! - `instantiated_*` indicates that generic substitutions have already been
//!   applied to a callable or signature.
//! - `synthetic_*` refers to checker-authored helper surfaces such as built-in
//!   signatures or synthesized stub methods that do not come directly from user
//!   source text.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use typepython_binding::{BindingTable, Declaration, DeclarationKind, DeclarationOwnerKind};
use typepython_config::{DiagnosticLevel, ImportFallback};
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span, SuggestionApplicability};
use typepython_graph::ModuleGraph;
use typepython_incremental::{
    IncrementalState, ModuleSolverFacts, PublicSummary, SealedRootSummary, SnapshotMetadata,
    SummaryCallableSignature, SummaryDeclarationFact, SummaryExport, SummaryImportSymbolTarget,
    SummaryImportTarget, SummarySignatureParam, SummaryTypeParam, snapshot_with_summaries,
};
use typepython_syntax::SourceKind;
use typepython_target::{RuntimeFeature, RuntimeTypingForm, RuntimeTypingSemantics};
mod assignments;
mod calls;
mod declaration_semantics;
mod declarations;
mod generic_solver;
mod semantic;
mod source_facts;
mod stubs;
mod type_core;
mod type_system;

pub(crate) use self::assignments::*;
pub(crate) use self::calls::*;
pub(crate) use self::declaration_semantics::*;
pub(crate) use self::declarations::*;
pub(crate) use self::generic_solver::*;
pub(crate) use self::semantic::*;
pub(crate) use self::source_facts::*;
pub use self::stubs::{collect_effective_callable_stub_overrides, collect_synthetic_method_stubs};
pub(crate) use self::type_core::*;
pub(crate) use self::type_system::*;
pub(crate) use typepython_syntax::{
    CallableParamExpr, TypeExpr, annotated_inner, normalize_callable_param_expr,
    normalize_type_text, parse_callable_annotation, parse_callable_annotation_parts,
    split_top_level_type_args, union_branches, unpack_inner,
};

const BUILTIN_FUNCTION_RETURN_TYPES: &[(&str, &str)] = &[
    ("len", "int"),
    ("str", "str"),
    ("int", "int"),
    ("float", "float"),
    ("bool", "bool"),
    ("bytes", "bytes"),
    ("list", "list[Any]"),
    ("dict", "dict[Any, Any]"),
    ("tuple", "tuple[Any, ...]"),
    ("set", "set[Any]"),
    ("frozenset", "frozenset[Any]"),
    ("range", "range"),
    ("input", "str"),
    ("print", "None"),
    ("ord", "int"),
    ("chr", "str"),
    ("hash", "int"),
    ("id", "int"),
    ("cast", "Any"),
    ("typing.cast", "Any"),
];

const TYPING_SYNTHETIC_CALLABLE_SIGNATURES: &[(&str, &str)] = &[
    ("TypeVar", "(name:str)->TypeVar"),
    ("typing.TypeVar", "(name:str)->TypeVar"),
    ("ParamSpec", "(name:str)->ParamSpec"),
    ("typing.ParamSpec", "(name:str)->ParamSpec"),
    ("TypeVarTuple", "(name:str)->TypeVarTuple"),
    ("typing.TypeVarTuple", "(name:str)->TypeVarTuple"),
    ("NewType", "(name:str,typ:)->NewType"),
    ("typing.NewType", "(name:str,typ:)->NewType"),
];

/// Result of running the checker.
#[derive(Debug, Clone, Default)]
pub struct CheckResult {
    /// Diagnostics produced by the checker.
    pub diagnostics: DiagnosticReport,
}

#[derive(Debug, Clone, Default)]
pub struct ModuleCheckResult {
    pub diagnostics_by_module: BTreeMap<String, Vec<Diagnostic>>,
}

impl ModuleCheckResult {
    /// Flattens per-module diagnostics into a single report in module iteration order.
    #[must_use]
    pub fn diagnostics(&self) -> DiagnosticReport {
        let mut diagnostics = DiagnosticReport::default();
        for module_diagnostics in self.diagnostics_by_module.values() {
            diagnostics.diagnostics.extend(module_diagnostics.iter().cloned());
        }
        diagnostics
    }
}

/// Stub override derived from checker-resolved callable information.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EffectiveCallableStubOverride {
    pub module_key: String,
    pub owner_type_name: Option<String>,
    pub name: String,
    pub line: usize,
    pub params: Vec<typepython_syntax::FunctionParam>,
    pub returns: String,
}

/// Synthetic method emitted into authoritative stubs for checker-synthesized behavior.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SyntheticMethodStub {
    pub module_key: String,
    pub owner_type_name: String,
    pub class_line: usize,
    pub name: String,
    pub method_kind: typepython_syntax::MethodKind,
    pub params: Vec<typepython_syntax::FunctionParam>,
    pub returns: Option<String>,
}

#[derive(Debug)]
struct CheckerContext<'a> {
    nodes: &'a [typepython_graph::ModuleNode],
    import_fallback: ImportFallback,
    strict: bool,
    source_facts: CheckerSourceFactsProvider<'a>,
}

impl<'a> CheckerContext<'a> {
    fn new(
        nodes: &'a [typepython_graph::ModuleNode],
        import_fallback: ImportFallback,
        source_overrides: Option<&'a BTreeMap<String, String>>,
    ) -> Self {
        Self::new_with_bound_surface_facts(nodes, import_fallback, source_overrides, None)
    }

    fn new_with_bound_surface_facts(
        nodes: &'a [typepython_graph::ModuleNode],
        import_fallback: ImportFallback,
        source_overrides: Option<&'a BTreeMap<String, String>>,
        bound_surface_facts: Option<&'a BTreeMap<String, typepython_binding::ModuleSurfaceFacts>>,
    ) -> Self {
        Self::new_with_bound_surface_facts_and_strict(
            nodes,
            import_fallback,
            source_overrides,
            bound_surface_facts,
            false,
        )
    }

    fn new_with_bound_surface_facts_and_strict(
        nodes: &'a [typepython_graph::ModuleNode],
        import_fallback: ImportFallback,
        source_overrides: Option<&'a BTreeMap<String, String>>,
        bound_surface_facts: Option<&'a BTreeMap<String, typepython_binding::ModuleSurfaceFacts>>,
        strict: bool,
    ) -> Self {
        Self {
            nodes,
            import_fallback,
            strict,
            source_facts: CheckerSourceFactsProvider::new(source_overrides, bound_surface_facts),
        }
    }

    fn import_fallback_type(&self) -> &'static str {
        match self.import_fallback {
            ImportFallback::Unknown => "unknown",
            ImportFallback::Dynamic => "dynamic",
        }
    }

    fn load_typed_dict_class_metadata(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> TypedDictClassMetadataByName {
        self.source_facts.typed_dict_class_metadata(node)
    }

    fn load_direct_function_signatures(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> DirectFunctionSignaturesByName {
        self.source_facts.direct_function_signatures(node)
    }

    fn load_direct_method_signatures(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> DirectMethodSignaturesByName {
        self.source_facts.direct_method_signatures(node)
    }

    fn load_decorator_transform_module_info(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> Option<typepython_syntax::DecoratorTransformModuleInfo> {
        self.source_facts.decorator_transform_module_info(node)
    }

    fn load_dataclass_transform_module_info(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> Option<typepython_syntax::DataclassTransformModuleInfo> {
        self.source_facts.dataclass_transform_module_info(node)
    }

    fn load_declaration_semantics(&self, declaration: &Declaration) -> SemanticDeclarationFacts {
        self.source_facts.declaration_semantics(declaration)
    }

    fn load_typed_dict_literal_sites(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> Vec<typepython_syntax::TypedDictLiteralSite> {
        self.source_facts.typed_dict_literal_sites(node)
    }

    fn load_typed_dict_mutation_sites(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> Vec<typepython_syntax::TypedDictMutationSite> {
        self.source_facts.typed_dict_mutation_sites(node)
    }

    fn load_frozen_field_mutation_sites(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> Vec<typepython_syntax::FrozenFieldMutationSite> {
        self.source_facts.frozen_field_mutation_sites(node)
    }

    fn load_unsafe_operation_sites(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> Vec<typepython_syntax::UnsafeOperationSite> {
        self.source_facts.unsafe_operation_sites(node)
    }

    fn load_conditional_return_sites(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> Vec<typepython_syntax::ConditionalReturnSite> {
        self.source_facts.conditional_return_sites(node)
    }
}

fn binding_surface_facts_by_module(
    bindings: &[BindingTable],
) -> BTreeMap<String, typepython_binding::ModuleSurfaceFacts> {
    bindings
        .iter()
        .map(|binding| (binding.module_key.clone(), binding.surface_facts.clone()))
        .collect()
}

/// Runs the checker over the module graph.
#[must_use]
pub fn check(graph: &ModuleGraph) -> CheckResult {
    check_with_options(
        graph,
        false,
        true,
        DiagnosticLevel::Warning,
        false,
        false,
        ImportFallback::Unknown,
    )
}

/// Runs the checker with the caller-controlled option surface used by the CLI and tests.
#[must_use]
pub fn check_with_options(
    graph: &ModuleGraph,
    require_explicit_overrides: bool,
    enable_sealed_exhaustiveness: bool,
    report_deprecated: DiagnosticLevel,
    strict: bool,
    warn_unsafe: bool,
    import_fallback: ImportFallback,
) -> CheckResult {
    check_with_source_overrides(
        graph,
        require_explicit_overrides,
        enable_sealed_exhaustiveness,
        report_deprecated,
        strict,
        warn_unsafe,
        import_fallback,
        None,
    )
}

#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the public checker option surface while threading binding metadata"
)]
/// Runs the checker with precomputed binding metadata to avoid recomputing source-derived facts.
pub fn check_with_binding_metadata(
    graph: &ModuleGraph,
    bindings: &[BindingTable],
    require_explicit_overrides: bool,
    enable_sealed_exhaustiveness: bool,
    report_deprecated: DiagnosticLevel,
    strict: bool,
    warn_unsafe: bool,
    import_fallback: ImportFallback,
    source_overrides: Option<&BTreeMap<String, String>>,
) -> CheckResult {
    let module_keys = graph.nodes.iter().map(|node| node.module_key.clone()).collect();
    let diagnostics = check_modules_with_binding_metadata(
        graph,
        bindings,
        &module_keys,
        require_explicit_overrides,
        enable_sealed_exhaustiveness,
        report_deprecated,
        strict,
        warn_unsafe,
        import_fallback,
        source_overrides,
    )
    .diagnostics();
    CheckResult { diagnostics }
}

#[must_use]
pub fn semantic_incremental_state_with_binding_metadata(
    graph: &ModuleGraph,
    bindings: &[BindingTable],
    import_fallback: ImportFallback,
    source_overrides: Option<&BTreeMap<String, String>>,
    stdlib_snapshot: Option<String>,
    metadata: SnapshotMetadata,
) -> IncrementalState {
    let bound_surface_facts = binding_surface_facts_by_module(bindings);
    let context = CheckerContext::new_with_bound_surface_facts(
        &graph.nodes,
        import_fallback,
        source_overrides,
        Some(&bound_surface_facts),
    );
    let summaries =
        graph.nodes.iter().map(|node| semantic_public_summary(&context, node)).collect();
    snapshot_with_summaries(summaries, stdlib_snapshot, metadata)
}

#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "incremental summary reuse threads build context and summary state together"
)]
pub fn semantic_incremental_state_with_reused_summaries(
    graph: &ModuleGraph,
    bindings: &[BindingTable],
    import_fallback: ImportFallback,
    source_overrides: Option<&BTreeMap<String, String>>,
    previous_summaries: &[PublicSummary],
    summary_rebuild_modules: &BTreeSet<String>,
    stdlib_snapshot: Option<String>,
    metadata: SnapshotMetadata,
) -> IncrementalState {
    let bound_surface_facts = binding_surface_facts_by_module(bindings);
    let context = CheckerContext::new_with_bound_surface_facts(
        &graph.nodes,
        import_fallback,
        source_overrides,
        Some(&bound_surface_facts),
    );
    let previous_by_module = previous_summaries
        .iter()
        .map(|summary| (summary.module.clone(), summary.clone()))
        .collect::<BTreeMap<_, _>>();
    let summaries = graph
        .nodes
        .iter()
        .map(|node| {
            if summary_rebuild_modules.contains(&node.module_key)
                || !previous_by_module.contains_key(&node.module_key)
            {
                semantic_public_summary(&context, node)
            } else {
                previous_by_module
                    .get(&node.module_key)
                    .cloned()
                    .unwrap_or_else(|| semantic_public_summary(&context, node))
            }
        })
        .collect();
    snapshot_with_summaries(summaries, stdlib_snapshot, metadata)
}

#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the public checker option surface while adding LSP source overrides"
)]
pub fn check_with_source_overrides(
    graph: &ModuleGraph,
    require_explicit_overrides: bool,
    enable_sealed_exhaustiveness: bool,
    report_deprecated: DiagnosticLevel,
    strict: bool,
    warn_unsafe: bool,
    import_fallback: ImportFallback,
    source_overrides: Option<&BTreeMap<String, String>>,
) -> CheckResult {
    let module_keys = graph.nodes.iter().map(|node| node.module_key.clone()).collect();
    let diagnostics = check_modules_internal(
        graph,
        None,
        &module_keys,
        require_explicit_overrides,
        enable_sealed_exhaustiveness,
        report_deprecated,
        strict,
        warn_unsafe,
        import_fallback,
        source_overrides,
    )
    .diagnostics();
    CheckResult { diagnostics }
}

#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the public checker option surface while adding subset recheck inputs"
)]
pub fn check_modules_with_source_overrides(
    graph: &ModuleGraph,
    module_keys: &BTreeSet<String>,
    require_explicit_overrides: bool,
    enable_sealed_exhaustiveness: bool,
    report_deprecated: DiagnosticLevel,
    strict: bool,
    warn_unsafe: bool,
    import_fallback: ImportFallback,
    source_overrides: Option<&BTreeMap<String, String>>,
) -> ModuleCheckResult {
    check_modules_internal(
        graph,
        None,
        module_keys,
        require_explicit_overrides,
        enable_sealed_exhaustiveness,
        report_deprecated,
        strict,
        warn_unsafe,
        import_fallback,
        source_overrides,
    )
}

#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the public checker option surface while threading binding metadata and subset recheck inputs"
)]
pub fn check_modules_with_binding_metadata(
    graph: &ModuleGraph,
    bindings: &[BindingTable],
    module_keys: &BTreeSet<String>,
    require_explicit_overrides: bool,
    enable_sealed_exhaustiveness: bool,
    report_deprecated: DiagnosticLevel,
    strict: bool,
    warn_unsafe: bool,
    import_fallback: ImportFallback,
    source_overrides: Option<&BTreeMap<String, String>>,
) -> ModuleCheckResult {
    let bound_surface_facts = binding_surface_facts_by_module(bindings);
    check_modules_internal(
        graph,
        Some(&bound_surface_facts),
        module_keys,
        require_explicit_overrides,
        enable_sealed_exhaustiveness,
        report_deprecated,
        strict,
        warn_unsafe,
        import_fallback,
        source_overrides,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "shared implementation for checker option surfaces with optional bound metadata"
)]
fn check_modules_internal(
    graph: &ModuleGraph,
    bound_surface_facts: Option<&BTreeMap<String, typepython_binding::ModuleSurfaceFacts>>,
    module_keys: &BTreeSet<String>,
    require_explicit_overrides: bool,
    enable_sealed_exhaustiveness: bool,
    report_deprecated: DiagnosticLevel,
    strict: bool,
    warn_unsafe: bool,
    import_fallback: ImportFallback,
    source_overrides: Option<&BTreeMap<String, String>>,
) -> ModuleCheckResult {
    let context = CheckerContext::new_with_bound_surface_facts_and_strict(
        &graph.nodes,
        import_fallback,
        source_overrides,
        bound_surface_facts,
        strict,
    );
    let mut diagnostics_by_module = BTreeMap::new();
    let options = CheckerPassOptions {
        require_explicit_overrides,
        enable_sealed_exhaustiveness,
        report_deprecated,
        strict,
        warn_unsafe,
    };

    for node in &graph.nodes {
        if !module_keys.contains(&node.module_key) {
            continue;
        }
        let mut diagnostics = DiagnosticReport::default();
        collect_node_diagnostics(&context, &mut diagnostics, node, options);
        diagnostics_by_module.insert(node.module_key.clone(), diagnostics.diagnostics);
    }

    ModuleCheckResult { diagnostics_by_module }
}

#[derive(Clone, Copy)]
struct CheckerPassOptions {
    require_explicit_overrides: bool,
    enable_sealed_exhaustiveness: bool,
    report_deprecated: DiagnosticLevel,
    strict: bool,
    warn_unsafe: bool,
}

fn collect_node_diagnostics(
    context: &CheckerContext<'_>,
    diagnostics: &mut DiagnosticReport,
    node: &typepython_graph::ModuleNode,
    options: CheckerPassOptions,
) {
    collect_node_semantic_diagnostics(context, diagnostics, node, options);
    collect_node_call_diagnostics(context, diagnostics, node);
    collect_node_assignment_diagnostics(context, diagnostics, node);
    collect_node_declaration_diagnostics(context, diagnostics, node, options);
}

fn semantic_public_summary(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> PublicSummary {
    let top_level_declarations = node
        .declarations
        .iter()
        .filter(|declaration| declaration.owner.is_none())
        .collect::<Vec<_>>();

    let mut exports = top_level_declarations
        .iter()
        .map(|declaration| semantic_summary_export(context, node, declaration))
        .collect::<Vec<_>>();
    exports.sort_by(|left, right| left.name.cmp(&right.name));

    let mut imports = top_level_declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Import)
        .filter_map(|declaration| {
            context
                .load_declaration_semantics(declaration)
                .import_target
                .map(|target| target.raw_target)
                .or_else(|| declaration.import_target().map(|target| target.raw_target.clone()))
        })
        .collect::<Vec<_>>();
    imports.sort();
    imports.dedup();

    let mut import_targets = top_level_declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Import)
        .filter_map(|declaration| {
            context
                .load_declaration_semantics(declaration)
                .import_target
                .as_ref()
                .map(summary_import_target_from_semantics)
                .or_else(|| declaration.import_target().map(summary_import_target_from_bound))
        })
        .collect::<Vec<_>>();
    import_targets.sort_by(|left, right| left.raw_target.cmp(&right.raw_target));
    import_targets.dedup_by(|left, right| left.raw_target == right.raw_target);

    let mut sealed_roots = top_level_declarations
        .iter()
        .filter(|declaration| declaration.class_kind == Some(DeclarationOwnerKind::SealedClass))
        .map(|declaration| {
            let mut members = top_level_declarations
                .iter()
                .filter(|candidate| {
                    candidate.name != declaration.name
                        && candidate.has_class_base(&declaration.name)
                })
                .map(|candidate| candidate.name.clone())
                .collect::<Vec<_>>();
            members.sort();
            SealedRootSummary { root: declaration.name.clone(), members }
        })
        .collect::<Vec<_>>();
    sealed_roots.sort_by(|left, right| left.root.cmp(&right.root));

    let mut declaration_facts = top_level_declarations
        .iter()
        .map(|declaration| semantic_declaration_fact(context, declaration))
        .collect::<Vec<_>>();
    declaration_facts
        .sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.kind.cmp(&right.kind)));

    PublicSummary {
        module: node.module_key.clone(),
        is_package_entry: is_package_entry_path(&node.module_path),
        exports,
        imports,
        import_targets,
        sealed_roots,
        solver_facts: ModuleSolverFacts { declaration_facts },
    }
}

fn semantic_summary_export(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
) -> SummaryExport {
    let semantics = context.load_declaration_semantics(declaration);
    let declaration_signature =
        semantics.callable.as_ref().map(summary_callable_signature_from_semantics).or_else(|| {
            declaration.callable_signature().map(summary_callable_signature_from_bound)
        });
    let type_expr =
        summary_export_type_expr(declaration, &semantics, declaration_signature.as_ref());
    let exported_type = type_expr
        .as_ref()
        .map(TypeExpr::render)
        .or_else(|| semantic_exported_type(node, declaration, &semantics));
    let exported_type_expr = declaration_exported_type_expr(declaration, &semantics);
    let required_runtime_features = required_runtime_features_for_export(
        declaration,
        declaration_signature.as_ref(),
        exported_type.as_deref(),
    );
    SummaryExport {
        name: declaration.name.clone(),
        kind: summary_kind_string(declaration),
        type_repr: exported_type.clone().unwrap_or_else(|| declaration.name.clone()),
        type_expr,
        declaration_signature: declaration_signature.clone(),
        exported_type,
        exported_type_expr,
        type_params: declaration.type_params.iter().map(summary_type_param).collect(),
        runtime_semantics: summary_runtime_semantics(node.module_kind, declaration),
        required_runtime_features,
        public: !declaration.name.starts_with('_'),
    }
}

fn summary_runtime_semantics(
    module_kind: SourceKind,
    declaration: &Declaration,
) -> Option<RuntimeTypingSemantics> {
    if module_kind != SourceKind::Python {
        return None;
    }
    match declaration.kind {
        DeclarationKind::TypeAlias => Some(RuntimeTypingSemantics {
            form: RuntimeTypingForm::TypeAliasType,
            type_param_names: declaration
                .type_params
                .iter()
                .map(|type_param| type_param.name.clone())
                .collect(),
            annotation_scope_owner: Some(declaration.name.clone()),
            lazy_alias_value: true,
            local_type_params_hidden_from_globals: true,
            required_features: runtime_semantic_features(declaration, true),
        }),
        DeclarationKind::Class if !declaration.type_params.is_empty() => {
            Some(RuntimeTypingSemantics {
                form: RuntimeTypingForm::NativeGenericClass,
                type_param_names: declaration
                    .type_params
                    .iter()
                    .map(|type_param| type_param.name.clone())
                    .collect(),
                annotation_scope_owner: Some(declaration.name.clone()),
                lazy_alias_value: false,
                local_type_params_hidden_from_globals: true,
                required_features: runtime_semantic_features(declaration, false),
            })
        }
        DeclarationKind::Function | DeclarationKind::Overload
            if !declaration.type_params.is_empty() =>
        {
            Some(RuntimeTypingSemantics {
                form: RuntimeTypingForm::NativeGenericFunction,
                type_param_names: declaration
                    .type_params
                    .iter()
                    .map(|type_param| type_param.name.clone())
                    .collect(),
                annotation_scope_owner: Some(declaration.name.clone()),
                lazy_alias_value: false,
                local_type_params_hidden_from_globals: true,
                required_features: runtime_semantic_features(declaration, false),
            })
        }
        _ => None,
    }
}

fn runtime_semantic_features(declaration: &Declaration, is_alias: bool) -> Vec<RuntimeFeature> {
    let mut features = Vec::new();
    if is_alias {
        features.push(RuntimeFeature::TypeStmt);
    }
    if !declaration.type_params.is_empty() {
        features.push(RuntimeFeature::InlineTypeParams);
    }
    if declaration.type_params.iter().any(|type_param| type_param.rendered_default().is_some()) {
        features.push(RuntimeFeature::GenericDefaults);
    }
    features
}

fn required_runtime_features_for_export(
    declaration: &Declaration,
    declaration_signature: Option<&SummaryCallableSignature>,
    exported_type: Option<&str>,
) -> Vec<String> {
    let mut features = BTreeSet::<String>::new();
    if !declaration.type_params.is_empty() {
        features.insert(String::from("inline_type_params"));
        if declaration.type_params.iter().any(|type_param| type_param.rendered_default().is_some())
        {
            features.insert(String::from("generic_defaults"));
        }
        if declaration.kind == DeclarationKind::TypeAlias {
            features.insert(String::from("type_stmt"));
        }
    }

    if let Some(exported_type) = exported_type {
        extend_runtime_features_from_type_text(&mut features, exported_type);
    }
    if let Some(signature) = declaration_signature {
        if let Some(returns) = &signature.returns {
            extend_runtime_features_from_type_text(&mut features, returns);
        }
        for param in &signature.params {
            if let Some(annotation) = &param.annotation {
                extend_runtime_features_from_type_text(&mut features, annotation);
            }
        }
    }

    features.into_iter().collect()
}

fn extend_runtime_features_from_type_text(features: &mut BTreeSet<String>, text: &str) {
    if text.contains("TypeIs[") {
        features.insert(String::from("typing_type_is"));
    }
    if text.contains("ReadOnly[") {
        features.insert(String::from("typing_readonly"));
    }
    if text == "NoDefault" || text.contains("NoDefault[") {
        features.insert(String::from("typing_no_default"));
    }
}

fn semantic_declaration_fact(
    context: &CheckerContext<'_>,
    declaration: &Declaration,
) -> SummaryDeclarationFact {
    let semantics = context.load_declaration_semantics(declaration);
    let type_expr_structured = declaration_fact_type_expr(declaration, &semantics);
    SummaryDeclarationFact {
        name: declaration.name.clone(),
        kind: summary_kind_string(declaration),
        signature: semantics
            .callable
            .as_ref()
            .map(summary_callable_signature_from_semantics)
            .or_else(|| {
                declaration.callable_signature().map(summary_callable_signature_from_bound)
            }),
        type_expr: type_expr_structured
            .as_ref()
            .map(TypeExpr::render)
            .or_else(|| {
                semantics.type_alias.as_ref().map(|type_alias| type_alias.body_text.clone())
            })
            .or_else(|| semantics.value.as_ref().and_then(|value| value.annotation_text.clone()))
            .or_else(|| declaration.inferred_value_type_semantic_text()),
        type_expr_structured,
        import_target: semantics
            .import_target
            .as_ref()
            .map(|target| target.raw_target.clone())
            .or_else(|| declaration.import_target().map(|target| target.raw_target.clone())),
        import_target_structured: semantics
            .import_target
            .as_ref()
            .map(summary_import_target_from_semantics)
            .or_else(|| declaration.import_target().map(summary_import_target_from_bound)),
        bases: declaration.rendered_class_bases(),
    }
}

fn semantic_exported_type(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    semantics: &SemanticDeclarationFacts,
) -> Option<String> {
    match declaration.kind {
        DeclarationKind::Class => Some(declaration.name.clone()),
        DeclarationKind::Function | DeclarationKind::Overload => semantics
            .callable
            .as_ref()
            .map(|callable| {
                diagnostic_type_text(&SemanticType::Callable {
                    params: SemanticCallableParams::ParamList(
                        callable
                            .semantic_params
                            .iter()
                            .map(SemanticCallableParam::annotation_or_dynamic)
                            .collect(),
                    ),
                    return_type: Box::new(
                        callable
                            .return_type
                            .clone()
                            .unwrap_or_else(|| SemanticType::Name(String::from("dynamic"))),
                    ),
                })
            })
            .or_else(|| declaration.callable_signature().map(|signature| signature.rendered())),
        DeclarationKind::Value => semantics
            .value
            .as_ref()
            .and_then(|value| {
                value
                    .annotation
                    .as_ref()
                    .map(diagnostic_type_text)
                    .or_else(|| value.annotation_text.clone())
            })
            .or_else(|| declaration.inferred_value_type_semantic_text())
            .map(|text| rewrite_imported_typing_aliases(node, &text)),
        DeclarationKind::TypeAlias => semantics
            .type_alias
            .as_ref()
            .map(|type_alias| diagnostic_type_text(&type_alias.body))
            .or_else(|| {
                declaration.type_alias_value().map(typepython_binding::BoundTypeExpr::render)
            }),
        DeclarationKind::Import => semantics
            .import_target
            .as_ref()
            .map(|target| target.raw_target.clone())
            .or_else(|| declaration.import_target().map(|target| target.raw_target.clone())),
    }
}

fn summary_callable_signature_from_bound(
    signature: &typepython_binding::BoundCallableSignature,
) -> SummaryCallableSignature {
    SummaryCallableSignature {
        params: signature.params.iter().map(summary_signature_param).collect(),
        returns: signature.returns.as_ref().map(typepython_binding::BoundTypeExpr::render),
        returns_expr: signature.returns.as_ref().map(|returns| returns.expr.clone()),
    }
}

fn summary_callable_signature_from_semantics(
    callable: &SemanticCallableDeclaration,
) -> SummaryCallableSignature {
    SummaryCallableSignature {
        params: callable
            .semantic_params
            .iter()
            .map(|param| SummarySignatureParam {
                name: param.name.clone(),
                annotation: param.rendered_annotation_text(),
                annotation_expr: param.annotation.as_ref().map(semantic_type_to_type_expr),
                has_default: param.has_default,
                positional_only: param.positional_only,
                keyword_only: param.keyword_only,
                variadic: param.variadic,
                keyword_variadic: param.keyword_variadic,
            })
            .collect(),
        returns: callable.rendered_return_annotation_text(),
        returns_expr: callable.return_type.as_ref().map(semantic_type_to_type_expr),
    }
}

fn summary_signature_param(
    param: &typepython_syntax::DirectFunctionParamSite,
) -> SummarySignatureParam {
    SummarySignatureParam {
        name: param.name.clone(),
        annotation: param.rendered_annotation(),
        annotation_expr: param.annotation_expr.clone(),
        has_default: param.has_default,
        positional_only: param.positional_only,
        keyword_only: param.keyword_only,
        variadic: param.variadic,
        keyword_variadic: param.keyword_variadic,
    }
}

fn summary_signature_param_type_expr(param: &SummarySignatureParam) -> typepython_syntax::TypeExpr {
    param
        .annotation_expr
        .clone()
        .or_else(|| param.annotation.as_deref().and_then(TypeExpr::parse))
        .unwrap_or_else(|| typepython_syntax::TypeExpr::Name(String::from("dynamic")))
}

fn summary_callable_type_expr(signature: &SummaryCallableSignature) -> typepython_syntax::TypeExpr {
    typepython_syntax::TypeExpr::Callable {
        params: Box::new(typepython_syntax::CallableParamExpr::ParamList(
            signature.params.iter().map(summary_signature_param_type_expr).collect(),
        )),
        return_type: Box::new(
            signature
                .returns_expr
                .clone()
                .or_else(|| signature.returns.as_deref().and_then(TypeExpr::parse))
                .unwrap_or_else(|| typepython_syntax::TypeExpr::Name(String::from("dynamic"))),
        ),
    }
}

fn summary_export_type_expr(
    declaration: &Declaration,
    semantics: &SemanticDeclarationFacts,
    declaration_signature: Option<&SummaryCallableSignature>,
) -> Option<TypeExpr> {
    match declaration.kind {
        DeclarationKind::Function | DeclarationKind::Overload => {
            declaration_signature.map(summary_callable_type_expr)
        }
        DeclarationKind::Value | DeclarationKind::TypeAlias => {
            declaration_exported_type_expr(declaration, semantics)
        }
        DeclarationKind::Class | DeclarationKind::Import => None,
    }
}

fn summary_type_param(type_param: &typepython_binding::GenericTypeParam) -> SummaryTypeParam {
    SummaryTypeParam {
        kind: Some(match type_param.kind {
            typepython_binding::GenericTypeParamKind::TypeVar => String::from("typevar"),
            typepython_binding::GenericTypeParamKind::ParamSpec => String::from("paramspec"),
            typepython_binding::GenericTypeParamKind::TypeVarTuple => String::from("typevartuple"),
        }),
        name: type_param.name.clone(),
        bound: type_param.rendered_bound(),
        bound_expr: type_param.bound_expr.as_ref().map(|expr| expr.expr.clone()),
        constraints: type_param.rendered_constraints(),
        constraint_exprs: type_param
            .constraint_exprs
            .iter()
            .map(|expr| expr.expr.clone())
            .collect(),
        default: type_param.rendered_default(),
        default_expr: type_param.default_expr.as_ref().map(|expr| expr.expr.clone()),
    }
}

fn declaration_fact_type_expr(
    declaration: &Declaration,
    semantics: &SemanticDeclarationFacts,
) -> Option<TypeExpr> {
    semantics
        .type_alias
        .as_ref()
        .map(|type_alias| semantic_type_to_type_expr(&type_alias.body))
        .or_else(|| {
            semantics
                .value
                .as_ref()
                .and_then(|value| value.annotation.as_ref().map(semantic_type_to_type_expr))
        })
        .or_else(|| declaration.type_alias_value().map(|value| value.expr.clone()))
        .or_else(|| declaration.value_annotation().map(|annotation| annotation.expr.clone()))
}

fn declaration_exported_type_expr(
    declaration: &Declaration,
    semantics: &SemanticDeclarationFacts,
) -> Option<TypeExpr> {
    match declaration.kind {
        DeclarationKind::Value => semantics
            .value
            .as_ref()
            .and_then(|value| value.annotation.as_ref().map(semantic_type_to_type_expr))
            .or_else(|| declaration.inferred_value_type().map(|expr| expr.expr.clone()))
            .or_else(|| declaration.value_annotation().map(|annotation| annotation.expr.clone())),
        DeclarationKind::TypeAlias => semantics
            .type_alias
            .as_ref()
            .map(|type_alias| semantic_type_to_type_expr(&type_alias.body))
            .or_else(|| declaration.type_alias_value().map(|value| value.expr.clone())),
        DeclarationKind::Function | DeclarationKind::Overload => semantics
            .callable
            .as_ref()
            .and_then(|callable| callable.return_type.as_ref().map(semantic_type_to_type_expr))
            .or_else(|| {
                declaration.callable_signature().and_then(|signature| {
                    signature.returns.as_ref().map(|returns| returns.expr.clone())
                })
            }),
        DeclarationKind::Class | DeclarationKind::Import => None,
    }
}

fn summary_import_target_from_bound(
    target: &typepython_binding::BoundImportTarget,
) -> SummaryImportTarget {
    SummaryImportTarget {
        raw_target: target.raw_target.clone(),
        module_target: target.module_target.clone(),
        symbol_target: target.symbol_target.as_ref().map(|symbol| SummaryImportSymbolTarget {
            module_key: symbol.module_key.clone(),
            symbol_name: symbol.symbol_name.clone(),
        }),
    }
}

fn summary_import_target_from_semantics(target: &SemanticImportTargetRef) -> SummaryImportTarget {
    SummaryImportTarget {
        raw_target: target.raw_target.clone(),
        module_target: target.module_target.clone(),
        symbol_target: target.symbol_target.as_ref().map(|symbol| SummaryImportSymbolTarget {
            module_key: symbol.module_key.clone(),
            symbol_name: symbol.symbol_name.clone(),
        }),
    }
}

fn summary_kind_string(declaration: &Declaration) -> String {
    match declaration.kind {
        DeclarationKind::TypeAlias => String::from("typealias"),
        DeclarationKind::Class => String::from("class"),
        DeclarationKind::Function => String::from("function"),
        DeclarationKind::Overload => String::from("overload"),
        DeclarationKind::Value => String::from("value"),
        DeclarationKind::Import => String::from("import"),
    }
}

fn is_package_entry_path(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "__init__.py" || name == "__init__.tpy")
}

fn collect_node_semantic_diagnostics(
    context: &CheckerContext<'_>,
    diagnostics: &mut DiagnosticReport,
    node: &typepython_graph::ModuleNode,
    options: CheckerPassOptions,
) {
    push_diagnostics(diagnostics, ambiguous_overload_call_diagnostics(node, context.nodes));
    push_diagnostics(
        diagnostics,
        direct_unknown_operation_diagnostics(context, node, context.nodes),
    );
    push_diagnostics(diagnostics, unresolved_import_diagnostics(node, context.nodes));
    push_diagnostics(diagnostics, direct_member_access_diagnostics(node, context.nodes));
    push_diagnostics(
        diagnostics,
        unsafe_boundary_diagnostics(context, node, options.strict, options.warn_unsafe),
    );
    push_diagnostics(
        diagnostics,
        deprecated_use_diagnostics(node, context.nodes, options.report_deprecated),
    );
    push_diagnostics(diagnostics, direct_method_call_diagnostics(context, node, context.nodes));
    push_diagnostics(diagnostics, direct_return_type_diagnostics(node, context.nodes));
    push_diagnostics(diagnostics, direct_yield_type_diagnostics(node, context.nodes));
    push_diagnostics(diagnostics, for_loop_target_diagnostics(node, context.nodes));
    push_diagnostics(diagnostics, destructuring_assignment_diagnostics(node, context.nodes));
    push_diagnostics(diagnostics, with_statement_diagnostics(node, context.nodes));
}

fn collect_node_call_diagnostics(
    context: &CheckerContext<'_>,
    diagnostics: &mut DiagnosticReport,
    node: &typepython_graph::ModuleNode,
) {
    push_diagnostics(diagnostics, direct_call_arity_diagnostics(context, node, context.nodes));
    push_diagnostics(diagnostics, direct_call_type_diagnostics(context, node, context.nodes));
    push_diagnostics(diagnostics, direct_call_keyword_diagnostics(context, node, context.nodes));
    push_diagnostics(
        diagnostics,
        direct_unresolved_paramspec_call_diagnostics(node, context.nodes),
    );
}

fn collect_node_assignment_diagnostics(
    context: &CheckerContext<'_>,
    diagnostics: &mut DiagnosticReport,
    node: &typepython_graph::ModuleNode,
) {
    push_diagnostics(
        diagnostics,
        annotated_assignment_type_diagnostics(context, node, context.nodes),
    );
    push_diagnostics(
        diagnostics,
        simple_name_augmented_assignment_diagnostics(node, context.nodes),
    );
    push_unique_diagnostics(
        diagnostics,
        typed_dict_literal_diagnostics(context, node, context.nodes),
    );
    push_diagnostics(
        diagnostics,
        typed_dict_readonly_mutation_diagnostics(context, node, context.nodes),
    );
    push_diagnostics(
        diagnostics,
        subscript_assignment_type_diagnostics(context, node, context.nodes),
    );
    push_diagnostics(
        diagnostics,
        frozen_dataclass_transform_mutation_diagnostics(context, node, context.nodes),
    );
    push_diagnostics(
        diagnostics,
        frozen_plain_dataclass_mutation_diagnostics(context, node, context.nodes),
    );
    push_diagnostics(
        diagnostics,
        attribute_assignment_type_diagnostics(context, node, context.nodes),
    );
}

fn collect_node_declaration_diagnostics(
    context: &CheckerContext<'_>,
    diagnostics: &mut DiagnosticReport,
    node: &typepython_graph::ModuleNode,
    options: CheckerPassOptions,
) {
    push_diagnostics(
        diagnostics,
        duplicate_diagnostics(&node.module_path, node.module_kind, &node.declarations),
    );
    push_diagnostics(diagnostics, override_diagnostics(node, context.nodes));
    push_diagnostics(diagnostics, override_compatibility_diagnostics(node, context.nodes));
    push_diagnostics(
        diagnostics,
        undecidable_decorator_diagnostics(context, node, context.nodes, options.strict),
    );
    if options.require_explicit_overrides && node.module_kind == SourceKind::TypePython {
        push_diagnostics(diagnostics, missing_override_diagnostics(node, context.nodes));
    }
    push_diagnostics(diagnostics, final_decorator_diagnostics(node, context.nodes));
    push_diagnostics(diagnostics, final_override_diagnostics(node, context.nodes));
    push_diagnostics(diagnostics, abstract_member_diagnostics(node, context.nodes));
    push_diagnostics(diagnostics, abstract_instantiation_diagnostics(node, context.nodes));
    push_diagnostics(diagnostics, interface_implementation_diagnostics(node, context.nodes));
    if options.enable_sealed_exhaustiveness {
        push_diagnostics(diagnostics, sealed_match_exhaustiveness_diagnostics(node, context.nodes));
        push_diagnostics(diagnostics, enum_match_exhaustiveness_diagnostics(node, context.nodes));
        push_diagnostics(
            diagnostics,
            literal_match_exhaustiveness_diagnostics(node, context.nodes),
        );
    }
    push_diagnostics(
        diagnostics,
        conditional_return_coverage_diagnostics(context, node, context.nodes),
    );
}

fn push_diagnostics(
    report: &mut DiagnosticReport,
    new_diagnostics: impl IntoIterator<Item = Diagnostic>,
) {
    for diagnostic in new_diagnostics {
        report.push(diagnostic);
    }
}

fn push_unique_diagnostics(
    report: &mut DiagnosticReport,
    new_diagnostics: impl IntoIterator<Item = Diagnostic>,
) {
    for diagnostic in new_diagnostics {
        if !report.diagnostics.contains(&diagnostic) {
            report.push(diagnostic);
        }
    }
}

#[cfg(test)]
mod tests;

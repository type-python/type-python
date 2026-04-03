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
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
};

use typepython_binding::{BindingTable, Declaration, DeclarationKind, DeclarationOwnerKind};
use typepython_config::{DiagnosticLevel, ImportFallback};
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span, SuggestionApplicability};
use typepython_graph::ModuleGraph;
use typepython_syntax::SourceKind;
mod assignments;
mod calls;
mod declaration_semantics;
mod declarations;
mod generic_solver;
mod semantic;
mod stubs;
mod type_core;
mod type_expr;
mod type_system;

pub(crate) use self::assignments::*;
pub(crate) use self::calls::*;
pub(crate) use self::declaration_semantics::*;
pub(crate) use self::declarations::*;
pub(crate) use self::generic_solver::*;
pub(crate) use self::semantic::*;
pub use self::stubs::{collect_effective_callable_stub_overrides, collect_synthetic_method_stubs};
pub(crate) use self::type_core::*;
pub(crate) use self::type_expr::*;
pub(crate) use self::type_system::*;

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
    #[must_use]
    pub fn diagnostics(&self) -> DiagnosticReport {
        let mut diagnostics = DiagnosticReport::default();
        for module_diagnostics in self.diagnostics_by_module.values() {
            diagnostics.diagnostics.extend(module_diagnostics.iter().cloned());
        }
        diagnostics
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EffectiveCallableStubOverride {
    pub module_key: String,
    pub owner_type_name: Option<String>,
    pub name: String,
    pub line: usize,
    pub params: Vec<typepython_syntax::FunctionParam>,
    pub returns: String,
}

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

type TypedDictClassMetadataByName = BTreeMap<String, typepython_syntax::TypedDictClassMetadata>;
type DirectFunctionSignaturesByName =
    BTreeMap<String, Vec<typepython_syntax::DirectFunctionParamSite>>;
type DirectMethodSignaturesByName =
    BTreeMap<(String, String), Vec<typepython_syntax::DirectFunctionParamSite>>;

#[derive(Debug, Default)]
struct FallbackModuleSourceFacts {
    source_loaded: bool,
    source_text: Option<String>,
    typed_dict_class_metadata: Option<BTreeMap<String, typepython_syntax::TypedDictClassMetadata>>,
    direct_function_signatures:
        Option<BTreeMap<String, Vec<typepython_syntax::DirectFunctionParamSite>>>,
    direct_method_signatures:
        Option<BTreeMap<(String, String), Vec<typepython_syntax::DirectFunctionParamSite>>>,
    decorator_transform_module_info:
        Option<Option<typepython_syntax::DecoratorTransformModuleInfo>>,
    dataclass_transform_module_info:
        Option<Option<typepython_syntax::DataclassTransformModuleInfo>>,
}

impl FallbackModuleSourceFacts {
    fn source_text<'a>(
        &'a mut self,
        node: &typepython_graph::ModuleNode,
        source_overrides: Option<&BTreeMap<String, String>>,
    ) -> Option<&'a str> {
        if !self.source_loaded {
            self.source_text = source_overrides
                .and_then(|overrides| {
                    overrides.get(&node.module_path.display().to_string()).cloned()
                })
                .or_else(|| fs::read_to_string(&node.module_path).ok());
            self.source_loaded = true;
        }

        self.source_text.as_deref()
    }
}

#[derive(Debug, Default)]
struct CheckerSourceFactsProvider<'a> {
    bound_surface_facts: Option<&'a BTreeMap<String, typepython_binding::ModuleSurfaceFacts>>,
    modules: RefCell<BTreeMap<String, FallbackModuleSourceFacts>>,
    source_overrides: Option<&'a BTreeMap<String, String>>,
}

impl<'a> CheckerSourceFactsProvider<'a> {
    fn new(
        source_overrides: Option<&'a BTreeMap<String, String>>,
        bound_surface_facts: Option<&'a BTreeMap<String, typepython_binding::ModuleSurfaceFacts>>,
    ) -> Self {
        Self {
            bound_surface_facts,
            modules: RefCell::new(BTreeMap::new()),
            source_overrides,
        }
    }

    fn with_module_facts<T>(
        &self,
        node: &typepython_graph::ModuleNode,
        action: impl FnOnce(&mut FallbackModuleSourceFacts) -> T,
    ) -> T {
        let cache_key = node.module_path.display().to_string();
        let mut modules = self.modules.borrow_mut();
        let facts = modules.entry(cache_key).or_default();
        action(facts)
    }

    fn bound_surface_facts(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> Option<&typepython_binding::ModuleSurfaceFacts> {
        self.bound_surface_facts.and_then(|facts| facts.get(&node.module_key))
    }

    fn declaration_semantics(&self, declaration: &Declaration) -> SemanticDeclarationFacts {
        declaration_semantic_facts(declaration)
    }

    fn typed_dict_class_metadata(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> TypedDictClassMetadataByName {
        if let Some(bound) = self.bound_surface_facts(node) {
            return bound.typed_dict_class_metadata.clone();
        }
        if node.module_path.to_string_lossy().starts_with('<') {
            return BTreeMap::new();
        }

        self.with_module_facts(node, |facts| {
            if let Some(metadata) = facts.typed_dict_class_metadata.clone() {
                return metadata;
            }

            let metadata = match facts.source_text(node, self.source_overrides) {
                Some(source) => typepython_syntax::collect_typed_dict_class_metadata(source)
                    .into_iter()
                    .map(|metadata| (metadata.name.clone(), metadata))
                    .collect(),
                None => BTreeMap::new(),
            };
            facts.typed_dict_class_metadata = Some(metadata.clone());
            metadata
        })
    }

    fn direct_function_signatures(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> DirectFunctionSignaturesByName {
        if let Some(bound) = self.bound_surface_facts(node) {
            return bound.direct_function_signatures.clone();
        }
        if node.module_path.to_string_lossy().starts_with('<') {
            return BTreeMap::new();
        }

        self.with_module_facts(node, |facts| {
            if let Some(signatures) = facts.direct_function_signatures.clone() {
                return signatures;
            }

            let signatures = match facts.source_text(node, self.source_overrides) {
                Some(source) => typepython_syntax::collect_direct_function_signature_sites(source)
                    .into_iter()
                    .map(|signature| (signature.name, signature.params))
                    .collect(),
                None => BTreeMap::new(),
            };
            facts.direct_function_signatures = Some(signatures.clone());
            signatures
        })
    }

    fn direct_method_signatures(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> DirectMethodSignaturesByName {
        if let Some(bound) = self.bound_surface_facts(node) {
            return bound.direct_method_signatures.clone();
        }
        if node.module_path.to_string_lossy().starts_with('<') {
            return BTreeMap::new();
        }

        self.with_module_facts(node, |facts| {
            if let Some(signatures) = facts.direct_method_signatures.clone() {
                return signatures;
            }

            let signatures = match facts.source_text(node, self.source_overrides) {
                Some(source) => typepython_syntax::collect_direct_method_signature_sites(source)
                    .into_iter()
                    .map(|signature| {
                        let params = match signature.method_kind {
                            typepython_syntax::MethodKind::Static
                            | typepython_syntax::MethodKind::Property => signature.params,
                            typepython_syntax::MethodKind::Instance
                            | typepython_syntax::MethodKind::Class
                            | typepython_syntax::MethodKind::PropertySetter => {
                                signature.params.into_iter().skip(1).collect()
                            }
                        };
                        ((signature.owner_type_name, signature.name), params)
                    })
                    .collect(),
                None => BTreeMap::new(),
            };
            facts.direct_method_signatures = Some(signatures.clone());
            signatures
        })
    }

    fn decorator_transform_module_info(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> Option<typepython_syntax::DecoratorTransformModuleInfo> {
        if let Some(bound) = self.bound_surface_facts(node) {
            return Some(bound.decorator_transform_module_info.clone());
        }
        if node.module_path.to_string_lossy().starts_with('<') {
            return None;
        }

        self.with_module_facts(node, |facts| {
            if let Some(info) = &facts.decorator_transform_module_info {
                return info.clone();
            }

            let info = facts
                .source_text(node, self.source_overrides)
                .map(typepython_syntax::collect_decorator_transform_module_info);
            facts.decorator_transform_module_info = Some(info.clone());
            info
        })
    }

    fn dataclass_transform_module_info(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> Option<typepython_syntax::DataclassTransformModuleInfo> {
        if let Some(bound) = self.bound_surface_facts(node) {
            return Some(bound.dataclass_transform_module_info.clone());
        }
        if node.module_path.to_string_lossy().starts_with('<') {
            return None;
        }

        self.with_module_facts(node, |facts| {
            if let Some(info) = &facts.dataclass_transform_module_info {
                return info.clone();
            }

            let info = facts
                .source_text(node, self.source_overrides)
                .map(typepython_syntax::collect_dataclass_transform_module_info);
            facts.dataclass_transform_module_info = Some(info.clone());
            info
        })
    }
}

#[derive(Debug)]
struct CheckerContext<'a> {
    nodes: &'a [typepython_graph::ModuleNode],
    import_fallback: ImportFallback,
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
        Self {
            nodes,
            import_fallback,
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
    let context = CheckerContext::new_with_bound_surface_facts(
        &graph.nodes,
        import_fallback,
        source_overrides,
        bound_surface_facts,
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
        unsafe_boundary_diagnostics(node, options.strict, options.warn_unsafe),
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

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

use typepython_binding::{Declaration, DeclarationKind, DeclarationOwnerKind};
use typepython_config::{DiagnosticLevel, ImportFallback};
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span, SuggestionApplicability};
use typepython_graph::ModuleGraph;
use typepython_syntax::SourceKind;

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
struct ModuleSourceFacts {
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

impl ModuleSourceFacts {
    fn source_text<'a>(&'a mut self, node: &typepython_graph::ModuleNode) -> Option<&'a str> {
        if !self.source_loaded {
            self.source_text = fs::read_to_string(&node.module_path).ok();
            self.source_loaded = true;
        }

        self.source_text.as_deref()
    }
}

#[derive(Debug, Default)]
struct CheckerSourceFactsProvider {
    modules: RefCell<BTreeMap<String, ModuleSourceFacts>>,
}

impl CheckerSourceFactsProvider {
    fn with_module_facts<T>(
        &self,
        node: &typepython_graph::ModuleNode,
        action: impl FnOnce(&mut ModuleSourceFacts) -> T,
    ) -> T {
        let cache_key = node.module_path.display().to_string();
        let mut modules = self.modules.borrow_mut();
        let facts = modules.entry(cache_key).or_default();
        action(facts)
    }

    fn typed_dict_class_metadata(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> TypedDictClassMetadataByName {
        if node.module_path.to_string_lossy().starts_with('<') {
            return BTreeMap::new();
        }

        self.with_module_facts(node, |facts| {
            if let Some(metadata) = facts.typed_dict_class_metadata.clone() {
                return metadata;
            }

            let metadata = match facts.source_text(node) {
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
        if node.module_path.to_string_lossy().starts_with('<') {
            return BTreeMap::new();
        }

        self.with_module_facts(node, |facts| {
            if let Some(signatures) = facts.direct_function_signatures.clone() {
                return signatures;
            }

            let signatures = match facts.source_text(node) {
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
        if node.module_path.to_string_lossy().starts_with('<') {
            return BTreeMap::new();
        }

        self.with_module_facts(node, |facts| {
            if let Some(signatures) = facts.direct_method_signatures.clone() {
                return signatures;
            }

            let signatures = match facts.source_text(node) {
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
        if node.module_path.to_string_lossy().starts_with('<') {
            return None;
        }

        self.with_module_facts(node, |facts| {
            if let Some(info) = &facts.decorator_transform_module_info {
                return info.clone();
            }

            let info = facts
                .source_text(node)
                .map(typepython_syntax::collect_decorator_transform_module_info);
            facts.decorator_transform_module_info = Some(info.clone());
            info
        })
    }

    fn dataclass_transform_module_info(
        &self,
        node: &typepython_graph::ModuleNode,
    ) -> Option<typepython_syntax::DataclassTransformModuleInfo> {
        if node.module_path.to_string_lossy().starts_with('<') {
            return None;
        }

        self.with_module_facts(node, |facts| {
            if let Some(info) = &facts.dataclass_transform_module_info {
                return info.clone();
            }

            let info = facts
                .source_text(node)
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
    source_facts: CheckerSourceFactsProvider,
}

impl<'a> CheckerContext<'a> {
    fn new(nodes: &'a [typepython_graph::ModuleNode], import_fallback: ImportFallback) -> Self {
        Self { nodes, import_fallback, source_facts: CheckerSourceFactsProvider::default() }
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
    let context = CheckerContext::new(&graph.nodes, import_fallback);
    let mut diagnostics = DiagnosticReport::default();
    let options = CheckerPassOptions {
        require_explicit_overrides,
        enable_sealed_exhaustiveness,
        report_deprecated,
        strict,
        warn_unsafe,
    };

    for node in &graph.nodes {
        collect_node_diagnostics(&context, &mut diagnostics, node, options);
    }

    CheckResult { diagnostics }
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
    push_diagnostics(diagnostics, annotated_assignment_type_diagnostics(node, context.nodes));
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
        frozen_dataclass_transform_mutation_diagnostics(node, context.nodes),
    );
    push_diagnostics(diagnostics, frozen_plain_dataclass_mutation_diagnostics(node, context.nodes));
    push_diagnostics(diagnostics, attribute_assignment_type_diagnostics(node, context.nodes));
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

#[must_use]
pub fn collect_effective_callable_stub_overrides(
    graph: &ModuleGraph,
) -> Vec<EffectiveCallableStubOverride> {
    let context = CheckerContext::new(&graph.nodes, ImportFallback::Unknown);
    let mut overrides = graph
        .nodes
        .iter()
        .filter(|node| node.module_kind == SourceKind::TypePython)
        .flat_map(|node| {
            node.declarations
                .iter()
                .filter(|declaration| declaration.kind == DeclarationKind::Function)
                .filter_map(|declaration| {
                    let site =
                        resolve_decorated_callable_site_with_context(&context, node, declaration)?;
                    let callable =
                        resolve_decorated_callable_annotation_for_declaration_with_context(
                            &context,
                            node,
                            context.nodes,
                            declaration,
                        )?;
                    let params =
                        direct_function_signature_sites_from_callable_annotation(&callable)?;
                    let returns =
                        decorated_function_return_type_from_callable_annotation(&callable)?;
                    Some(EffectiveCallableStubOverride {
                        module_key: node.module_key.clone(),
                        owner_type_name: site.owner_type_name,
                        name: site.name,
                        line: site.line,
                        params: function_params_from_direct_sites(&params),
                        returns,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    overrides.sort_by(|left, right| {
        left.module_key
            .cmp(&right.module_key)
            .then(left.owner_type_name.cmp(&right.owner_type_name))
            .then(left.line.cmp(&right.line))
            .then(left.name.cmp(&right.name))
    });
    overrides
}

#[must_use]
pub fn collect_synthetic_method_stubs(graph: &ModuleGraph) -> Vec<SyntheticMethodStub> {
    let context = CheckerContext::new(&graph.nodes, ImportFallback::Unknown);
    let mut methods = graph
        .nodes
        .iter()
        .filter(|node| node.module_kind == SourceKind::TypePython)
        .flat_map(|node| {
            let module_info =
                context.load_dataclass_transform_module_info(node).unwrap_or_default();
            node.declarations
                .iter()
                .filter(|declaration| {
                    declaration.owner.is_none() && declaration.kind == DeclarationKind::Class
                })
                .filter_map(|declaration| {
                    let class_line = module_info
                        .classes
                        .iter()
                        .find(|class_site| class_site.name == declaration.name)
                        .map(|class_site| class_site.line)?;
                    let shape = resolve_dataclass_transform_class_shape_from_decl(
                        &graph.nodes,
                        node,
                        declaration,
                        &mut BTreeSet::new(),
                    )
                    .or_else(|| {
                        resolve_plain_dataclass_class_shape_from_decl(
                            &graph.nodes,
                            node,
                            declaration,
                            &mut BTreeSet::new(),
                        )
                    })?;
                    if shape.has_explicit_init {
                        return None;
                    }
                    let mut params = vec![typepython_syntax::FunctionParam {
                        name: String::from("self"),
                        annotation: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    }];
                    params.extend(shape.fields.iter().map(|field| {
                        typepython_syntax::FunctionParam {
                            name: field.keyword_name.clone(),
                            annotation: Some(field.annotation.clone()),
                            has_default: !field.required,
                            positional_only: false,
                            keyword_only: field.kw_only,
                            variadic: false,
                            keyword_variadic: false,
                        }
                    }));
                    Some(SyntheticMethodStub {
                        module_key: node.module_key.clone(),
                        owner_type_name: declaration.name.clone(),
                        class_line,
                        name: String::from("__init__"),
                        method_kind: typepython_syntax::MethodKind::Instance,
                        params,
                        returns: Some(String::from("None")),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    methods.sort_by(|left, right| {
        left.module_key
            .cmp(&right.module_key)
            .then(left.owner_type_name.cmp(&right.owner_type_name))
            .then(left.class_line.cmp(&right.class_line))
            .then(left.name.cmp(&right.name))
    });
    methods
}

fn function_params_from_direct_sites(
    params: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<typepython_syntax::FunctionParam> {
    params
        .iter()
        .map(|param| typepython_syntax::FunctionParam {
            name: param.name.clone(),
            annotation: param.annotation.clone(),
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect()
}

fn unsafe_boundary_diagnostics(
    node: &typepython_graph::ModuleNode,
    strict: bool,
    warn_unsafe: bool,
) -> Vec<Diagnostic> {
    if !strict || !warn_unsafe || node.module_kind != SourceKind::TypePython {
        return Vec::new();
    }
    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };
    typepython_syntax::collect_unsafe_operation_sites(&source)
        .into_iter()
        .filter(|site| !site.in_unsafe_block)
        .map(|site| {
            Diagnostic::warning(
                "TPY4019",
                match site.kind {
                    typepython_syntax::UnsafeOperationKind::EvalCall => String::from(
                        "unsafe boundary operation `eval(...)` must appear inside `unsafe:`",
                    ),
                    typepython_syntax::UnsafeOperationKind::ExecCall => String::from(
                        "unsafe boundary operation `exec(...)` must appear inside `unsafe:`",
                    ),
                    typepython_syntax::UnsafeOperationKind::GlobalsWrite => {
                        String::from("writes through `globals()` must appear inside `unsafe:`")
                    }
                    typepython_syntax::UnsafeOperationKind::LocalsWrite => {
                        String::from("writes through `locals()` must appear inside `unsafe:`")
                    }
                    typepython_syntax::UnsafeOperationKind::DictWrite => {
                        String::from("writes through `__dict__` must appear inside `unsafe:`")
                    }
                    typepython_syntax::UnsafeOperationKind::SetAttrNonLiteral => String::from(
                        "non-literal `setattr(obj, name, value)` must appear inside `unsafe:`",
                    ),
                    typepython_syntax::UnsafeOperationKind::DelAttrNonLiteral => String::from(
                        "non-literal `delattr(obj, name)` must appear inside `unsafe:`",
                    ),
                },
            )
            .with_span(Span::new(
                node.module_path.display().to_string(),
                site.line,
                1,
                site.line,
                1,
            ))
        })
        .collect()
}

fn ambiguous_overload_call_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.calls
        .iter()
        .filter_map(|call| {
            let overloads = resolve_direct_overloads(node, nodes, &call.callee);
            if overloads.len() < 2 {
                return None;
            }

            let applicable = overloads
                .into_iter()
                .filter(|declaration| {
                    overload_is_applicable_with_context(node, nodes, call, declaration)
                })
                .collect::<Vec<_>>();
            if applicable.len() < 2 || select_most_specific_overload(node, nodes, &applicable).is_some() {
                return None;
            }

            Some(Diagnostic::error(
                "TPY4012",
                format!(
                    "call to `{}` in module `{}` is ambiguous across {} overloads after applicability filtering",
                    call.callee,
                    node.module_path.display(),
                    applicable.len()
                ),
            ))
        })
        .collect()
}

fn resolve_direct_overloads<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Vec<&'a Declaration> {
    let local = node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.name == callee
                && declaration.owner.is_none()
                && declaration.kind == DeclarationKind::Overload
        })
        .collect::<Vec<_>>();
    if !local.is_empty() {
        return local;
    }

    let Some(import) = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == callee
    }) else {
        return Vec::new();
    };
    let Some((module_key, symbol_name)) = import.detail.rsplit_once('.') else {
        return Vec::new();
    };
    let Some(target_node) = nodes.iter().find(|candidate| candidate.module_key == module_key)
    else {
        return Vec::new();
    };
    target_node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.name == symbol_name
                && declaration.owner.is_none()
                && declaration.kind == DeclarationKind::Overload
        })
        .collect()
}

fn overload_is_more_specific(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    candidate: &Declaration,
    baseline: &Declaration,
) -> bool {
    let Some(candidate_params) = direct_signature_params(&candidate.detail) else {
        return false;
    };
    let Some(baseline_params) = direct_signature_params(&baseline.detail) else {
        return false;
    };
    if candidate_params.len() != baseline_params.len() {
        return false;
    }

    let mut strictly_more_specific = false;
    for (candidate_param, baseline_param) in candidate_params.iter().zip(baseline_params.iter()) {
        if candidate_param.name != baseline_param.name
            || candidate_param.has_default != baseline_param.has_default
            || candidate_param.positional_only != baseline_param.positional_only
            || candidate_param.keyword_only != baseline_param.keyword_only
            || candidate_param.variadic != baseline_param.variadic
            || candidate_param.keyword_variadic != baseline_param.keyword_variadic
        {
            return false;
        }
        if candidate_param.annotation.is_empty() || baseline_param.annotation.is_empty() {
            if candidate_param.annotation != baseline_param.annotation {
                return false;
            }
            continue;
        }
        if !direct_type_is_assignable(
            node,
            nodes,
            &baseline_param.annotation,
            &candidate_param.annotation,
        ) {
            return false;
        }
        if normalize_type_text(&candidate_param.annotation)
            != normalize_type_text(&baseline_param.annotation)
        {
            strictly_more_specific = true;
        }
    }

    strictly_more_specific
}

fn select_most_specific_overload<'a>(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    applicable: &[&'a Declaration],
) -> Option<&'a Declaration> {
    if applicable.len() == 1 {
        return applicable.first().copied();
    }

    let best = applicable
        .iter()
        .copied()
        .filter(|candidate| {
            applicable.iter().copied().all(|other| {
                std::ptr::eq::<Declaration>(*candidate, other)
                    || overload_is_more_specific(node, nodes, candidate, other)
            })
        })
        .collect::<Vec<_>>();

    if best.len() == 1 { Some(best[0]) } else { None }
}

#[cfg(test)]
#[allow(dead_code)]
fn overload_is_applicable(call: &typepython_binding::CallSite, declaration: &Declaration) -> bool {
    let node = typepython_graph::ModuleNode {
        module_path: std::path::PathBuf::from("<overload-test>"),
        module_key: String::new(),
        module_kind: SourceKind::Python,
        declarations: Vec::new(),
        member_accesses: Vec::new(),
        returns: Vec::new(),
        yields: Vec::new(),
        if_guards: Vec::new(),
        asserts: Vec::new(),
        invalidations: Vec::new(),
        matches: Vec::new(),
        for_loops: Vec::new(),
        with_statements: Vec::new(),
        except_handlers: Vec::new(),
        assignments: Vec::new(),
        summary_fingerprint: 0,
        calls: Vec::new(),
        method_calls: Vec::new(),
    };
    overload_is_applicable_with_context(&node, &[], call, declaration)
}

fn overload_is_applicable_with_context(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    declaration: &Declaration,
) -> bool {
    let params = direct_signature_params(&declaration.detail).unwrap_or_default();
    call_signature_params_are_applicable(node, nodes, call, &params)
}

fn call_signature_params_are_applicable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    params: &[DirectSignatureParam],
) -> bool {
    let positional_params = params
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .collect::<Vec<_>>();
    let has_variadic = params.iter().any(|param| param.variadic);
    let starred_positional = resolved_starred_positional_expansions(node, nodes, call);
    let expected_positional_arg_types =
        expected_positional_arg_types_from_direct_signature(params, call.arg_count);
    let expected_keyword_arg_types =
        expected_keyword_arg_types_from_direct_signature(params, &call.keyword_names);
    if call.arg_values.iter().enumerate().any(|(index, metadata)| {
        resolve_contextual_call_arg_type(
            node,
            nodes,
            call.line,
            metadata,
            expected_positional_arg_types.get(index).and_then(|expected| expected.as_deref()),
        )
        .is_some_and(|result| !result.diagnostics.is_empty())
    }) {
        return false;
    }
    if call.keyword_arg_values.iter().enumerate().any(|(index, metadata)| {
        resolve_contextual_call_arg_type(
            node,
            nodes,
            call.line,
            metadata,
            expected_keyword_arg_types.get(index).and_then(|expected| expected.as_deref()),
        )
        .is_some_and(|result| !result.diagnostics.is_empty())
    }) {
        return false;
    }
    let resolved_keyword_arg_types =
        resolved_keyword_arg_types(node, nodes, call, &expected_keyword_arg_types);
    let mut positional_types =
        resolved_call_arg_types(node, nodes, call, &expected_positional_arg_types);
    let mut variadic_starred_types = Vec::new();
    for expansion in &starred_positional {
        match expansion {
            PositionalExpansion::Fixed(types) => positional_types.extend(types.clone()),
            PositionalExpansion::Variadic(element_type) => {
                variadic_starred_types.push(element_type.clone())
            }
        }
    }
    if !has_variadic
        && (positional_types.len() > positional_params.len() || !variadic_starred_types.is_empty())
    {
        return false;
    }
    let provided_keywords = call.keyword_names.iter().collect::<BTreeSet<_>>();
    let accepts_extra_keywords = params.iter().any(|param| param.keyword_variadic);
    let keyword_expansions = resolved_keyword_expansions(node, nodes, call);
    if call.keyword_names.iter().any(|keyword| {
        !params.iter().any(|param| param.name == **keyword && !param.positional_only)
            && !accepts_extra_keywords
    }) {
        return false;
    }
    if keyword_expansions.iter().any(|expansion| match expansion {
        KeywordExpansion::TypedDict(shape) => {
            (typed_dict_shape_has_unbounded_extra_keys(shape) && !accepts_extra_keywords)
                || shape.fields.keys().any(|key| {
                    !params.iter().any(|param| param.name == *key && !param.positional_only)
                        && !accepts_extra_keywords
                })
        }
        KeywordExpansion::Mapping(_) => !accepts_extra_keywords,
    }) {
        return false;
    }
    if keyword_duplicates_positional_arguments(call, params) {
        return false;
    }
    let positional_param_names =
        positional_params.iter().map(|param| param.name.as_str()).collect::<Vec<_>>();
    if keyword_expansions.iter().any(|expansion| match expansion {
        KeywordExpansion::TypedDict(shape) => shape.fields.keys().any(|key| {
            call.keyword_names.iter().any(|existing| existing == key)
                || positional_param_names
                    .iter()
                    .take(positional_types.len())
                    .any(|name| *name == key.as_str())
        }),
        KeywordExpansion::Mapping(_) => false,
    }) {
        return false;
    }
    if params.iter().enumerate().any(|(index, param)| {
        !param.has_default
            && if param.keyword_only {
                !provided_keywords.contains(&param.name)
                    && !keyword_expansions.iter().any(|expansion| match expansion {
                        KeywordExpansion::TypedDict(shape) => {
                            shape.fields.get(&param.name).is_some_and(|field| field.required)
                        }
                        KeywordExpansion::Mapping(_) => false,
                    })
            } else if param.variadic || param.keyword_variadic {
                false
            } else {
                index >= positional_types.len()
                    && (param.positional_only
                        || (!provided_keywords.contains(&param.name)
                            && !keyword_expansions.iter().any(|expansion| match expansion {
                                KeywordExpansion::TypedDict(shape) => shape
                                    .fields
                                    .get(&param.name)
                                    .is_some_and(|field| field.required),
                                KeywordExpansion::Mapping(_) => false,
                            })))
            }
    }) {
        return false;
    }

    let param_types = params.iter().map(|param| param.annotation.clone()).collect::<Vec<_>>();
    let variadic_type =
        params.iter().find(|param| param.variadic).map(|param| param.annotation.as_str());
    let keyword_variadic_type =
        params.iter().find(|param| param.keyword_variadic).map(|param| param.annotation.as_str());
    let positional_ok =
        positional_types.iter().take(positional_params.len()).zip(param_types.iter()).all(
            |(arg_ty, param_ty)| {
                if arg_ty.is_empty() || param_ty.is_empty() {
                    true
                } else {
                    direct_type_is_assignable(node, nodes, param_ty, arg_ty)
                }
            },
        ) && positional_types.iter().skip(positional_params.len()).all(|arg_ty| {
            let Some(param_ty) = variadic_type else {
                return false;
            };
            arg_ty.is_empty()
                || param_ty.is_empty()
                || direct_type_is_assignable(node, nodes, param_ty, arg_ty)
        }) && variadic_starred_types.iter().all(|arg_ty| {
            let Some(param_ty) = variadic_type else {
                return false;
            };
            arg_ty.is_empty()
                || param_ty.is_empty()
                || direct_type_matches(node, nodes, param_ty, arg_ty)
        });
    let keyword_ok =
        call.keyword_names.iter().zip(&resolved_keyword_arg_types).all(|(keyword, arg_ty)| {
            let Some(index) = params.iter().position(|param| param.name == *keyword) else {
                let Some(param_ty) = keyword_variadic_type else {
                    return false;
                };
                return arg_ty.is_empty()
                    || param_ty.is_empty()
                    || direct_type_is_assignable(node, nodes, param_ty, arg_ty);
            };
            let param_ty = &param_types[index];
            arg_ty.is_empty()
                || param_ty.is_empty()
                || direct_type_is_assignable(node, nodes, param_ty, arg_ty)
        }) && keyword_expansions.iter().all(|expansion| match expansion {
            KeywordExpansion::TypedDict(shape) => shape.fields.iter().all(|(key, field)| {
                if let Some(index) = params.iter().position(|param| param.name == *key) {
                    let param = &params[index];
                    if param.positional_only {
                        return false;
                    }
                    if !field.required && !param.has_default {
                        return false;
                    }
                    let param_ty = &param_types[index];
                    return field.value_type.is_empty()
                        || param_ty.is_empty()
                        || direct_type_matches(node, nodes, param_ty, &field.value_type);
                }
                let Some(param_ty) = keyword_variadic_type else {
                    return false;
                };
                field.value_type.is_empty()
                    || param_ty.is_empty()
                    || direct_type_matches(node, nodes, param_ty, &field.value_type)
            }),
            KeywordExpansion::Mapping(value_ty) => {
                let Some(param_ty) = keyword_variadic_type else {
                    return false;
                };
                value_ty.is_empty()
                    || param_ty.is_empty()
                    || direct_type_matches(node, nodes, param_ty, value_ty)
            }
        });

    positional_ok && keyword_ok
}

#[derive(Debug, Clone)]
enum PositionalExpansion {
    Fixed(Vec<String>),
    Variadic(String),
}

#[derive(Debug, Clone)]
enum KeywordExpansion {
    TypedDict(TypedDictShape),
    Mapping(String),
}

fn resolved_starred_positional_expansions(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
) -> Vec<PositionalExpansion> {
    let mut expansions = Vec::new();
    let count = call.starred_arg_values.len().max(call.starred_arg_types.len());
    for index in 0..count {
        let value_type = call
            .starred_arg_values
            .get(index)
            .and_then(|metadata| {
                resolve_direct_expression_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .unwrap_or_else(|| call.starred_arg_types.get(index).cloned().unwrap_or_default());
        if let Some(expansion) = parse_positional_expansion(&value_type) {
            expansions.push(expansion);
        }
    }
    expansions
}

fn parse_positional_expansion(value_type: &str) -> Option<PositionalExpansion> {
    let normalized = normalize_type_text(value_type);
    if normalized == "tuple[()]" {
        return Some(PositionalExpansion::Fixed(Vec::new()));
    }
    let (head, args) = split_generic_type(&normalized)?;
    match head {
        "tuple" if args.len() == 2 && args[1] == "..." => {
            Some(PositionalExpansion::Variadic(args[0].clone()))
        }
        "tuple" => Some(PositionalExpansion::Fixed(args)),
        "list" | "Sequence" if args.len() == 1 => {
            Some(PositionalExpansion::Variadic(args[0].clone()))
        }
        _ => None,
    }
}

fn resolved_keyword_expansions(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
) -> Vec<KeywordExpansion> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    resolved_keyword_expansions_with_context(&context, node, nodes, call)
}

fn resolved_keyword_expansions_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
) -> Vec<KeywordExpansion> {
    let mut expansions = Vec::new();
    let count = call.keyword_expansion_values.len().max(call.keyword_expansion_types.len());
    for index in 0..count {
        let value_type = call
            .keyword_expansion_values
            .get(index)
            .and_then(|metadata| {
                resolve_direct_expression_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .unwrap_or_else(|| {
                call.keyword_expansion_types.get(index).cloned().unwrap_or_default()
            });
        if let Some(expansion) = parse_keyword_expansion(context, node, nodes, &value_type) {
            expansions.push(expansion);
        }
    }
    expansions
}

fn parse_keyword_expansion(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    value_type: &str,
) -> Option<KeywordExpansion> {
    let normalized = normalize_type_text(value_type);
    if let Some(shape) =
        resolve_known_typed_dict_shape_from_type_with_context(context, node, nodes, &normalized)
    {
        return Some(KeywordExpansion::TypedDict(shape));
    }
    let (head, args) = split_generic_type(&normalized)?;
    match head {
        "dict" if args.len() == 2 && args[0] == "str" => {
            Some(KeywordExpansion::Mapping(args[1].clone()))
        }
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct DirectSignatureParam {
    name: String,
    annotation: String,
    has_default: bool,
    positional_only: bool,
    keyword_only: bool,
    variadic: bool,
    keyword_variadic: bool,
}

fn direct_unknown_operation_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for access in &node.member_accesses {
        if name_is_unknown_boundary(context, node, nodes, &access.owner_name) {
            diagnostics.push(Diagnostic::error(
                "TPY4003",
                format!(
                    "member access `{}` in module `{}` is unsupported because `{}` has type `unknown`",
                    access.member,
                    node.module_path.display(),
                    access.owner_name
                ),
            ));
        }
    }

    for call in &node.method_calls {
        if name_is_unknown_boundary(context, node, nodes, &call.owner_name) {
            diagnostics.push(Diagnostic::error(
                "TPY4003",
                format!(
                    "method call `{}.{}` in module `{}` is unsupported because `{}` has type `unknown`",
                    call.owner_name,
                    call.method,
                    node.module_path.display(),
                    call.owner_name
                ),
            ));
        }
    }

    for call in &node.calls {
        if plain_dataclass_field_specifier_call(context, node, &call.callee, call.line) {
            continue;
        }
        if name_is_unknown_boundary(context, node, nodes, &call.callee) {
            diagnostics.push(Diagnostic::error(
                "TPY4003",
                format!(
                    "call to `{}` in module `{}` is unsupported because `{}` has type `unknown`",
                    call.callee,
                    node.module_path.display(),
                    call.callee
                ),
            ));
        }
    }

    diagnostics
}

fn plain_dataclass_field_specifier_call(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    _callee: &str,
    line: usize,
) -> bool {
    let info = context.load_dataclass_transform_module_info(node).unwrap_or_default();
    info.classes.iter().any(|class_site| {
        class_site
            .decorators
            .iter()
            .any(|decorator| matches!(decorator.as_str(), "dataclass" | "dataclasses.dataclass"))
            && class_site.fields.iter().any(|field| {
                field.line == line
                    && field
                        .field_specifier_name
                        .as_ref()
                        .is_some_and(|name| matches!(name.as_str(), "field" | "dataclasses.field"))
            })
    })
}

fn conditional_return_coverage_diagnostics(
    _context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_conditional_return_sites(&source)
        .into_iter()
        .filter_map(|site| {
            let expected = normalize_type_text(&site.target_type);
            let expected_branches = union_branches(&expected).unwrap_or_else(|| vec![expected.clone()]);
            let covered = site
                .case_input_types
                .iter()
                .map(|case_type| normalize_type_text(case_type))
                .collect::<Vec<_>>();
            let missing = expected_branches
                .into_iter()
                .filter(|branch| {
                    !covered
                        .iter()
                        .any(|covered_branch| direct_type_matches(node, nodes, branch, covered_branch))
                })
                .collect::<Vec<_>>();
            (!missing.is_empty()).then(|| {
                Diagnostic::error(
                    "TPY4018",
                    format!(
                        "conditional return for `{}` in module `{}` does not cover parameter `{}`; missing: {}",
                        site.function_name,
                        node.module_path.display(),
                        site.target_name,
                        missing.join(", ")
                    ),
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    site.line,
                    1,
                    site.line,
                    1,
                ))
            })
        })
        .collect()
}

fn name_is_unknown_boundary(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    name: &str,
) -> bool {
    if resolve_typing_callable_signature(name).is_some()
        || resolve_builtin_return_type(name).is_some()
        || resolve_direct_function(node, nodes, name).is_some()
        || resolve_direct_base(nodes, node, name).is_some()
    {
        return false;
    }

    if resolve_direct_name_reference_type_with_context(
        context,
        node,
        nodes,
        None,
        None,
        None,
        None,
        usize::MAX,
        name,
    )
    .is_some_and(|resolved| normalize_type_text(&resolved) == "unknown")
    {
        return true;
    }

    if let Some((head, _)) = name.split_once('.')
        && unresolved_import_boundary_type_with_context(context, node, nodes, head)
            .is_some_and(|boundary| boundary == "unknown")
    {
        return true;
    }

    if resolve_imported_module_target(node, nodes, name).is_some() {
        return false;
    }

    unresolved_import_boundary_type_with_context(context, node, nodes, name)
        .is_some_and(|boundary| boundary == "unknown")
}

fn unresolved_import_boundary_type_with_context<'a>(
    context: &'a CheckerContext<'_>,
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    local_name: &str,
) -> Option<&'static str> {
    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == local_name
    })?;
    if resolve_import_target(node, nodes, import).is_some()
        || resolve_imported_module_target(node, nodes, local_name).is_some()
    {
        return None;
    }
    Some(context.import_fallback_type())
}

fn resolve_direct_type_alias<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    name: &str,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    if let Some(local) = node.declarations.iter().find(|declaration| {
        declaration.name == name
            && declaration.owner.is_none()
            && declaration.kind == DeclarationKind::TypeAlias
    }) {
        return Some((node, local));
    }

    if let Some((module_key, symbol_name)) = name.rsplit_once('.') {
        if let Some(target_node) = nodes.iter().find(|candidate| candidate.module_key == module_key)
            && let Some(target_decl) = target_node.declarations.iter().find(|declaration| {
                declaration.name == symbol_name
                    && declaration.owner.is_none()
                    && declaration.kind == DeclarationKind::TypeAlias
            })
        {
            return Some((target_node, target_decl));
        }

        if let Some(import) = node.declarations.iter().find(|declaration| {
            declaration.kind == DeclarationKind::Import && declaration.name == module_key
        }) && let Some(target_node) =
            nodes.iter().find(|candidate| candidate.module_key == import.detail)
            && let Some(target_decl) = target_node.declarations.iter().find(|declaration| {
                declaration.name == symbol_name
                    && declaration.owner.is_none()
                    && declaration.kind == DeclarationKind::TypeAlias
            })
        {
            return Some((target_node, target_decl));
        }
    }

    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == name
    })?;
    let (module_key, symbol_name) = import.detail.rsplit_once('.')?;
    let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
    let target_decl = target_node.declarations.iter().find(|declaration| {
        declaration.name == symbol_name
            && declaration.owner.is_none()
            && declaration.kind == DeclarationKind::TypeAlias
    })?;
    Some((target_node, target_decl))
}

fn direct_return_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for return_site in &node.returns {
        let Some(target) = node.declarations.iter().find(|declaration| {
            declaration.name == return_site.owner_name
                && declaration.kind == DeclarationKind::Function
                && match (&return_site.owner_type_name, &declaration.owner) {
                    (Some(owner_type), Some(owner)) => owner.name == *owner_type,
                    (None, None) => true,
                    _ => false,
                }
        }) else {
            continue;
        };

        let expected_text = rewrite_imported_typing_aliases(
            node,
            &substitute_self_annotation(
                target.detail.split_once("->").map(|(_, annotation)| annotation).unwrap_or(""),
                return_site.owner_type_name.as_deref(),
            ),
        );
        let Some(expected) =
            normalized_direct_return_annotation(&expected_text).map(normalize_type_text)
        else {
            continue;
        };

        let contextual =
            resolve_contextual_return_type(node, nodes, return_site, &expected, &target.detail);
        diagnostics.extend(contextual.diagnostics);
        let Some(actual) = contextual.actual_type else {
            continue;
        };

        if !direct_type_is_assignable(node, nodes, &expected, &actual) {
            let diagnostic = Diagnostic::error(
                "TPY4001",
                match &return_site.owner_type_name {
                    Some(owner_type) => format!(
                        "type `{}` in module `{}` returns `{}` where member `{}` expects `{}`",
                        owner_type,
                        node.module_path.display(),
                        actual,
                        return_site.owner_name,
                        expected
                    ),
                    None => format!(
                        "function `{}` in module `{}` returns `{}` where `{}` expects `{}`",
                        return_site.owner_name,
                        node.module_path.display(),
                        actual,
                        return_site.owner_name,
                        expected
                    ),
                },
            )
            .with_span(Span::new(
                node.module_path.display().to_string(),
                return_site.line,
                1,
                return_site.line,
                1,
            ));
            let diagnostic =
                attach_type_mismatch_notes(diagnostic, node, nodes, &expected, &actual);
            let diagnostic = attach_return_inference_trace(
                diagnostic,
                node,
                nodes,
                return_site,
                &expected,
                &actual,
                &target.detail,
            );
            diagnostics.push(attach_missing_none_return_suggestion(
                diagnostic,
                node,
                nodes,
                return_site,
                &expected_text,
                &expected,
                &actual,
                &target.detail,
            ));
        }
    }

    diagnostics
}

struct ContextualReturnTypeResult {
    actual_type: Option<String>,
    diagnostics: Vec<Diagnostic>,
}

fn resolve_contextual_return_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected: &str,
    signature: &str,
) -> ContextualReturnTypeResult {
    let metadata = direct_expr_metadata_from_return_site(return_site);
    if let Some(actual_type) = resolve_contextual_lambda_callable_type(
        node,
        nodes,
        return_site.line,
        &metadata,
        Some(expected),
    ) {
        return ContextualReturnTypeResult {
            actual_type: Some(actual_type),
            diagnostics: Vec::new(),
        };
    }
    if let Some(result) = resolve_contextual_typed_dict_literal_type(
        node,
        nodes,
        return_site.line,
        &metadata,
        Some(expected),
    ) {
        return ContextualReturnTypeResult {
            actual_type: Some(result.actual_type),
            diagnostics: result.diagnostics,
        };
    }
    if let Some(result) = resolve_contextual_collection_literal_type(
        node,
        nodes,
        return_site.line,
        &metadata,
        Some(expected),
    ) {
        return ContextualReturnTypeResult {
            actual_type: Some(result.actual_type),
            diagnostics: result.diagnostics,
        };
    }
    ContextualReturnTypeResult {
        actual_type: resolve_direct_expression_type(
            node,
            nodes,
            Some(signature),
            None,
            Some(return_site.owner_name.as_str()),
            return_site.owner_type_name.as_deref(),
            return_site.line,
            return_site.value_type.as_deref(),
            return_site.is_awaited,
            return_site.value_callee.as_deref(),
            return_site.value_name.as_deref(),
            return_site.value_member_owner_name.as_deref(),
            return_site.value_member_name.as_deref(),
            return_site.value_member_through_instance,
            return_site.value_method_owner_name.as_deref(),
            return_site.value_method_name.as_deref(),
            return_site.value_method_through_instance,
            return_site.value_subscript_target.as_deref(),
            return_site.value_subscript_string_key.as_deref(),
            return_site.value_subscript_index.as_deref(),
            return_site.value_if_true.as_deref(),
            return_site.value_if_false.as_deref(),
            return_site.value_if_guard.as_ref(),
            return_site.value_bool_left.as_deref(),
            return_site.value_bool_right.as_deref(),
            return_site.value_binop_left.as_deref(),
            return_site.value_binop_right.as_deref(),
            return_site.value_binop_operator.as_deref(),
        ),
        diagnostics: Vec::new(),
    }
}

fn direct_expr_metadata_from_return_site(
    return_site: &typepython_binding::ReturnSite,
) -> typepython_syntax::DirectExprMetadata {
    typepython_syntax::DirectExprMetadata {
        value_type: return_site.value_type.clone(),
        is_awaited: return_site.is_awaited,
        value_callee: return_site.value_callee.clone(),
        value_name: return_site.value_name.clone(),
        value_member_owner_name: return_site.value_member_owner_name.clone(),
        value_member_name: return_site.value_member_name.clone(),
        value_member_through_instance: return_site.value_member_through_instance,
        value_method_owner_name: return_site.value_method_owner_name.clone(),
        value_method_name: return_site.value_method_name.clone(),
        value_method_through_instance: return_site.value_method_through_instance,
        value_subscript_target: return_site.value_subscript_target.clone(),
        value_subscript_string_key: return_site.value_subscript_string_key.clone(),
        value_subscript_index: return_site.value_subscript_index.clone(),
        value_if_true: return_site.value_if_true.clone(),
        value_if_false: return_site.value_if_false.clone(),
        value_if_guard: return_site.value_if_guard.as_ref().map(site_to_guard),
        value_bool_left: return_site.value_bool_left.clone(),
        value_bool_right: return_site.value_bool_right.clone(),
        value_binop_left: return_site.value_binop_left.clone(),
        value_binop_right: return_site.value_binop_right.clone(),
        value_binop_operator: return_site.value_binop_operator.clone(),
        value_lambda: return_site.value_lambda.clone(),
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: return_site.value_list_elements.clone(),
        value_set_elements: return_site.value_set_elements.clone(),
        value_dict_entries: return_site.value_dict_entries.clone(),
    }
}

fn direct_expr_metadata_from_yield_site(
    yield_site: &typepython_binding::YieldSite,
) -> typepython_syntax::DirectExprMetadata {
    typepython_syntax::DirectExprMetadata {
        value_type: yield_site.value_type.clone(),
        is_awaited: false,
        value_callee: yield_site.value_callee.clone(),
        value_name: yield_site.value_name.clone(),
        value_member_owner_name: yield_site.value_member_owner_name.clone(),
        value_member_name: yield_site.value_member_name.clone(),
        value_member_through_instance: yield_site.value_member_through_instance,
        value_method_owner_name: yield_site.value_method_owner_name.clone(),
        value_method_name: yield_site.value_method_name.clone(),
        value_method_through_instance: yield_site.value_method_through_instance,
        value_subscript_target: yield_site.value_subscript_target.clone(),
        value_subscript_string_key: yield_site.value_subscript_string_key.clone(),
        value_subscript_index: yield_site.value_subscript_index.clone(),
        value_if_true: yield_site.value_if_true.clone(),
        value_if_false: yield_site.value_if_false.clone(),
        value_if_guard: yield_site.value_if_guard.as_ref().map(site_to_guard),
        value_bool_left: yield_site.value_bool_left.clone(),
        value_bool_right: yield_site.value_bool_right.clone(),
        value_binop_left: yield_site.value_binop_left.clone(),
        value_binop_right: yield_site.value_binop_right.clone(),
        value_binop_operator: yield_site.value_binop_operator.clone(),
        value_lambda: yield_site.value_lambda.clone(),
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: yield_site.value_list_elements.clone(),
        value_set_elements: yield_site.value_set_elements.clone(),
        value_dict_entries: yield_site.value_dict_entries.clone(),
    }
}

struct ContextualYieldTypeResult {
    actual_type: Option<String>,
    diagnostics: Vec<Diagnostic>,
}

fn resolve_contextual_yield_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    yield_site: &typepython_binding::YieldSite,
    expected: &str,
    signature: &str,
) -> ContextualYieldTypeResult {
    let metadata = direct_expr_metadata_from_yield_site(yield_site);
    if !yield_site.is_yield_from {
        if let Some(actual_type) = resolve_contextual_lambda_callable_type(
            node,
            nodes,
            yield_site.line,
            &metadata,
            Some(expected),
        ) {
            return ContextualYieldTypeResult {
                actual_type: Some(actual_type),
                diagnostics: Vec::new(),
            };
        }
        if let Some(result) = resolve_contextual_typed_dict_literal_type(
            node,
            nodes,
            yield_site.line,
            &metadata,
            Some(expected),
        ) {
            return ContextualYieldTypeResult {
                actual_type: Some(result.actual_type),
                diagnostics: result.diagnostics,
            };
        }
        if let Some(result) = resolve_contextual_collection_literal_type(
            node,
            nodes,
            yield_site.line,
            &metadata,
            Some(expected),
        ) {
            return ContextualYieldTypeResult {
                actual_type: Some(result.actual_type),
                diagnostics: result.diagnostics,
            };
        }
    }
    ContextualYieldTypeResult {
        actual_type: resolve_direct_expression_type(
            node,
            nodes,
            Some(signature),
            None,
            Some(yield_site.owner_name.as_str()),
            yield_site.owner_type_name.as_deref(),
            yield_site.line,
            yield_site.value_type.as_deref(),
            false,
            yield_site.value_callee.as_deref(),
            yield_site.value_name.as_deref(),
            yield_site.value_member_owner_name.as_deref(),
            yield_site.value_member_name.as_deref(),
            yield_site.value_member_through_instance,
            yield_site.value_method_owner_name.as_deref(),
            yield_site.value_method_name.as_deref(),
            yield_site.value_method_through_instance,
            yield_site.value_subscript_target.as_deref(),
            yield_site.value_subscript_string_key.as_deref(),
            yield_site.value_subscript_index.as_deref(),
            yield_site.value_if_true.as_deref(),
            yield_site.value_if_false.as_deref(),
            yield_site.value_if_guard.as_ref(),
            yield_site.value_bool_left.as_deref(),
            yield_site.value_bool_right.as_deref(),
            yield_site.value_binop_left.as_deref(),
            yield_site.value_binop_right.as_deref(),
            yield_site.value_binop_operator.as_deref(),
        ),
        diagnostics: Vec::new(),
    }
}

fn site_to_guard(
    guard: &typepython_binding::GuardConditionSite,
) -> typepython_syntax::GuardCondition {
    match guard {
        typepython_binding::GuardConditionSite::IsNone { name, negated } => {
            typepython_syntax::GuardCondition::IsNone { name: name.clone(), negated: *negated }
        }
        typepython_binding::GuardConditionSite::IsInstance { name, types } => {
            typepython_syntax::GuardCondition::IsInstance {
                name: name.clone(),
                types: types.clone(),
            }
        }
        typepython_binding::GuardConditionSite::PredicateCall { name, callee } => {
            typepython_syntax::GuardCondition::PredicateCall {
                name: name.clone(),
                callee: callee.clone(),
            }
        }
        typepython_binding::GuardConditionSite::TruthyName { name } => {
            typepython_syntax::GuardCondition::TruthyName { name: name.clone() }
        }
        typepython_binding::GuardConditionSite::Not(inner) => {
            typepython_syntax::GuardCondition::Not(Box::new(site_to_guard(inner)))
        }
        typepython_binding::GuardConditionSite::And(parts) => {
            typepython_syntax::GuardCondition::And(parts.iter().map(site_to_guard).collect())
        }
        typepython_binding::GuardConditionSite::Or(parts) => {
            typepython_syntax::GuardCondition::Or(parts.iter().map(site_to_guard).collect())
        }
    }
}

fn direct_yield_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for yield_site in &node.yields {
        let target = node.declarations.iter().find(|declaration| {
            declaration.name == yield_site.owner_name
                && declaration.kind == DeclarationKind::Function
                && match (&yield_site.owner_type_name, &declaration.owner) {
                    (Some(owner_type_name), Some(owner)) => owner.name == *owner_type_name,
                    (None, None) => true,
                    _ => false,
                }
        });
        let Some(target) = target else {
            continue;
        };

        let Some((_, returns)) = target.detail.split_once("->") else {
            continue;
        };
        let Some(expected) = unwrap_generator_yield_type(returns.trim()) else {
            continue;
        };
        let contextual =
            resolve_contextual_yield_type(node, nodes, yield_site, &expected, &target.detail);
        diagnostics.extend(contextual.diagnostics);
        let Some(actual) = contextual.actual_type else {
            continue;
        };

        let actual = if yield_site.is_yield_from {
            unwrap_yield_from_type(&actual).unwrap_or(actual)
        } else {
            actual
        };

        if !direct_type_is_assignable(node, nodes, &expected, &actual) {
            diagnostics.push(Diagnostic::error(
                    "TPY4001",
                    match &yield_site.owner_type_name {
                        Some(owner_type_name) => format!(
                            "type `{}` in module `{}` yields `{}` where member `{}` expects `Generator[{}, ...]`",
                            owner_type_name,
                            node.module_path.display(),
                            actual,
                            yield_site.owner_name,
                            expected
                        ),
                        None => format!(
                            "function `{}` in module `{}` yields `{}` where `Generator[{}, ...]` expects `{}`",
                            yield_site.owner_name,
                            node.module_path.display(),
                            actual,
                            expected,
                            expected
                        ),
                    },
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    yield_site.line,
                    1,
                    yield_site.line,
                    1,
                ))
                );
        }
    }

    diagnostics
}

fn for_loop_target_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.for_loops
        .iter()
        .filter(|for_loop| !for_loop.target_names.is_empty())
        .filter_map(|for_loop| {
            let iter_type = resolve_direct_expression_type(
                node,
                nodes,
                resolve_for_owner_signature(node, for_loop),
                None,
                for_loop.owner_name.as_deref(),
                for_loop.owner_type_name.as_deref(),
                for_loop.line,
                for_loop.iter_type.as_deref(),
                for_loop.iter_is_awaited,
                for_loop.iter_callee.as_deref(),
                for_loop.iter_name.as_deref(),
                for_loop.iter_member_owner_name.as_deref(),
                for_loop.iter_member_name.as_deref(),
                for_loop.iter_member_through_instance,
                for_loop.iter_method_owner_name.as_deref(),
                for_loop.iter_method_name.as_deref(),
                for_loop.iter_method_through_instance,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )?;

            let element_type = unwrap_for_iterable_type(&iter_type)?;
            let tuple_elements = unwrap_fixed_tuple_elements(&element_type)?;

            (tuple_elements.len() != for_loop.target_names.len()).then(|| {
                Diagnostic::error(
                    "TPY4001",
                    match (&for_loop.owner_type_name, &for_loop.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` destructures `for` target `({})` with {} name(s) from tuple element type `{}` with {} element(s) in `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            for_loop.target_names.join(", "),
                            for_loop.target_names.len(),
                            element_type,
                            tuple_elements.len(),
                            owner_name,
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` destructures `for` target `({})` with {} name(s) from tuple element type `{}` with {} element(s)",
                            owner_name,
                            node.module_path.display(),
                            for_loop.target_names.join(", "),
                            for_loop.target_names.len(),
                            element_type,
                            tuple_elements.len(),
                        ),
                        _ => format!(
                            "module `{}` destructures `for` target `({})` with {} name(s) from tuple element type `{}` with {} element(s)",
                            node.module_path.display(),
                            for_loop.target_names.join(", "),
                            for_loop.target_names.len(),
                            element_type,
                            tuple_elements.len(),
                        ),
                    },
                )
            })
        })
        .collect()
}

fn destructuring_assignment_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.assignments
        .iter()
        .filter(|assignment| assignment.destructuring_index == Some(0))
        .filter_map(|assignment| {
            let target_names = assignment.destructuring_target_names.as_ref()?;
            let signature = resolve_assignment_owner_signature(node, assignment);
            let actual = resolve_direct_expression_type(
                node,
                nodes,
                signature,
                Some(assignment.name.as_str()),
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                assignment.value_type.as_deref(),
                assignment.is_awaited,
                assignment.value_callee.as_deref(),
                assignment.value_name.as_deref(),
                assignment.value_member_owner_name.as_deref(),
                assignment.value_member_name.as_deref(),
                assignment.value_member_through_instance,
                assignment.value_method_owner_name.as_deref(),
                assignment.value_method_name.as_deref(),
                assignment.value_method_through_instance,
                assignment.value_subscript_target.as_deref(),
                assignment.value_subscript_string_key.as_deref(),
                assignment.value_subscript_index.as_deref(),
                assignment.value_if_true.as_deref(),
                assignment.value_if_false.as_deref(),
                assignment.value_if_guard.as_ref(),
                assignment.value_bool_left.as_deref(),
                assignment.value_bool_right.as_deref(),
                assignment.value_binop_left.as_deref(),
                assignment.value_binop_right.as_deref(),
                assignment.value_binop_operator.as_deref(),
            )?;
            let tuple_elements = unwrap_fixed_tuple_elements(&actual)?;
            (tuple_elements.len() != target_names.len()).then(|| {
                Diagnostic::error(
                    "TPY4001",
                    match (&assignment.owner_type_name, &assignment.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` destructures assignment target `({})` with {} name(s) from tuple type `{}` with {} element(s) in `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            target_names.join(", "),
                            target_names.len(),
                            actual,
                            tuple_elements.len(),
                            owner_name,
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` destructures assignment target `({})` with {} name(s) from tuple type `{}` with {} element(s)",
                            owner_name,
                            node.module_path.display(),
                            target_names.join(", "),
                            target_names.len(),
                            actual,
                            tuple_elements.len(),
                        ),
                        _ => format!(
                            "module `{}` destructures assignment target `({})` with {} name(s) from tuple type `{}` with {} element(s)",
                            node.module_path.display(),
                            target_names.join(", "),
                            target_names.len(),
                            actual,
                            tuple_elements.len(),
                        ),
                    },
                )
            })
        })
        .collect()
}

fn with_statement_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.with_statements
        .iter()
        .filter_map(|with_site| {
            let signature = resolve_with_owner_signature(node, with_site);
            resolve_with_target_type_for_signature(node, nodes, signature, with_site)
                .is_none()
                .then(|| {
                Diagnostic::error(
                    "TPY4001",
                    match (&with_site.owner_type_name, &with_site.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` uses `with` target `{}` with an expression that lacks compatible `__enter__`/`__exit__` members in `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            display_with_target_name(with_site),
                            owner_name,
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` uses `with` target `{}` with an expression that lacks compatible `__enter__`/`__exit__` members",
                            owner_name,
                            node.module_path.display(),
                            display_with_target_name(with_site),
                        ),
                        _ => format!(
                            "module `{}` uses `with` target `{}` with an expression that lacks compatible `__enter__`/`__exit__` members",
                            node.module_path.display(),
                            display_with_target_name(with_site),
                        ),
                    },
                )
            })
        })
        .collect()
}

fn display_with_target_name(with_site: &typepython_binding::WithSite) -> &str {
    with_site.target_name.as_deref().unwrap_or("<ignored>")
}

fn annotated_assignment_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for assignment in &node.assignments {
        let Some(annotation) = assignment.annotation.as_deref() else {
            continue;
        };
        let Some(expected) = normalized_assignment_annotation(annotation).map(normalize_type_text)
        else {
            continue;
        };

        if let Some(callable_result) =
            callable_assignment_result(node, nodes, assignment, &expected)
        {
            if let Some(diagnostic) = callable_result {
                diagnostics.push(diagnostic);
            }
            continue;
        }

        let assignment_metadata = direct_expr_metadata_from_assignment_site(assignment);
        if let Some(result) = resolve_contextual_typed_dict_literal_type(
            node,
            nodes,
            assignment.line,
            &assignment_metadata,
            Some(&expected),
        ) {
            diagnostics.extend(result.diagnostics);
            if !direct_type_is_assignable(node, nodes, &expected, &result.actual_type) {
                diagnostics.push(Diagnostic::error(
                    "TPY4001",
                    match (&assignment.owner_type_name, &assignment.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` assigns `{}` where local `{}` in `{}` expects `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            owner_name,
                            expected
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` assigns `{}` where local `{}` expects `{}`",
                            owner_name,
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            expected
                        ),
                        _ => format!(
                            "module `{}` assigns `{}` where `{}` expects `{}`",
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            expected
                        ),
                    },
                ));
            }
            continue;
        }

        if let Some(result) = resolve_contextual_collection_literal_type(
            node,
            nodes,
            assignment.line,
            &assignment_metadata,
            Some(&expected),
        ) {
            diagnostics.extend(result.diagnostics);
            if !direct_type_is_assignable(node, nodes, &expected, &result.actual_type) {
                diagnostics.push(Diagnostic::error(
                    "TPY4001",
                    match (&assignment.owner_type_name, &assignment.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` assigns `{}` where local `{}` in `{}` expects `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            owner_name,
                            expected
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` assigns `{}` where local `{}` expects `{}`",
                            owner_name,
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            expected
                        ),
                        _ => format!(
                            "module `{}` assigns `{}` where `{}` expects `{}`",
                            node.module_path.display(),
                            result.actual_type,
                            assignment.name,
                            expected
                        ),
                    },
                ));
            }
            continue;
        }

        let signature = resolve_assignment_owner_signature(node, assignment);
        let Some(actual) = resolve_assignment_site_type(node, nodes, signature, assignment) else {
            continue;
        };
        if !direct_type_is_assignable(node, nodes, &expected, &actual) {
            diagnostics.push(Diagnostic::error(
                "TPY4001",
                match (&assignment.owner_type_name, &assignment.owner_name) {
                    (Some(owner_type_name), Some(owner_name)) => format!(
                        "type `{}` in module `{}` assigns `{}` where local `{}` in `{}` expects `{}`",
                        owner_type_name,
                        node.module_path.display(),
                        actual,
                        assignment.name,
                        owner_name,
                        expected
                    ),
                    (None, Some(owner_name)) => format!(
                        "function `{}` in module `{}` assigns `{}` where local `{}` expects `{}`",
                        owner_name,
                        node.module_path.display(),
                        actual,
                        assignment.name,
                        expected
                    ),
                    _ => format!(
                        "module `{}` assigns `{}` where `{}` expects `{}`",
                        node.module_path.display(),
                        actual,
                        assignment.name,
                        expected
                    ),
                },
            ));
        }
    }

    diagnostics
}

fn simple_name_augmented_assignment_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.assignments
        .iter()
        .filter(|assignment| assignment.annotation.is_none())
        .filter(|assignment| {
            assignment
                .value_binop_left
                .as_deref()
                .and_then(|left| left.value_name.as_deref())
                == Some(assignment.name.as_str())
                && assignment.value_binop_right.is_some()
                && assignment.value_binop_operator.is_some()
        })
        .filter(|assignment| {
            node.invalidations.iter().any(|site| {
                site.kind == typepython_binding::InvalidationKind::RebindLike
                    && site.line == assignment.line
                    && site.owner_name == assignment.owner_name
                    && site.owner_type_name == assignment.owner_type_name
                    && site.names.iter().any(|name| name == &assignment.name)
            })
        })
        .filter_map(|assignment| {
            if current_augmented_assignment_target_is_final(node, assignment) {
                return Some(Diagnostic::error(
                    "TPY4006",
                    match (&assignment.owner_type_name, &assignment.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` reassigns Final binding `{}` in `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            assignment.name,
                            owner_name,
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` reassigns Final binding `{}`",
                            owner_name,
                            node.module_path.display(),
                            assignment.name,
                        ),
                        _ => format!(
                            "module `{}` reassigns Final binding `{}`",
                            node.module_path.display(),
                            assignment.name,
                        ),
                    },
                ));
            }
            let signature = resolve_assignment_owner_signature(node, assignment);
            let expected = resolve_current_augmented_assignment_target_type(
                node,
                nodes,
                signature,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                &assignment.name,
            )?;
            let actual = resolve_augmented_assignment_result_type(
                node,
                nodes,
                signature,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                assignment.value_binop_operator.as_deref(),
                &expected,
                assignment.value_binop_right.as_deref()?,
            )?;
            (!direct_type_matches(node, nodes, &expected, &actual)).then(|| {
                Diagnostic::error(
                    "TPY4001",
                    match (&assignment.owner_type_name, &assignment.owner_name) {
                        (Some(owner_type_name), Some(owner_name)) => format!(
                            "type `{}` in module `{}` augmented-assigns `{}` where local `{}` in `{}` expects `{}`",
                            owner_type_name,
                            node.module_path.display(),
                            actual,
                            assignment.name,
                            owner_name,
                            expected
                        ),
                        (None, Some(owner_name)) => format!(
                            "function `{}` in module `{}` augmented-assigns `{}` where local `{}` expects `{}`",
                            owner_name,
                            node.module_path.display(),
                            actual,
                            assignment.name,
                            expected
                        ),
                        _ => format!(
                            "module `{}` augmented-assigns `{}` where `{}` expects `{}`",
                            node.module_path.display(),
                            actual,
                            assignment.name,
                            expected
                        ),
                    },
                )
            })
        })
        .collect()
}

fn current_augmented_assignment_target_is_final(
    node: &typepython_graph::ModuleNode,
    assignment: &typepython_binding::AssignmentSite,
) -> bool {
    if assignment.owner_name.is_none()
        && node.declarations.iter().any(|declaration| {
            declaration.kind == DeclarationKind::Value
                && declaration.owner.is_none()
                && declaration.name == assignment.name
                && declaration.is_final
        })
    {
        return true;
    }

    node.assignments.iter().rev().any(|previous| {
        previous.name == assignment.name
            && previous.owner_name == assignment.owner_name
            && previous.owner_type_name == assignment.owner_type_name
            && previous.line < assignment.line
            && previous.annotation.as_deref().is_some_and(is_final_annotation_text)
    })
}

fn final_attribute_reassignment_diagnostic(
    module_path: &std::path::Path,
    owner_type_name: &str,
    member_name: &str,
) -> Diagnostic {
    Diagnostic::error(
        "TPY4006",
        format!(
            "type `{}` in module `{}` reassigns Final binding `{}`",
            owner_type_name,
            module_path.display(),
            member_name,
        ),
    )
}

fn is_final_annotation_text(annotation: &str) -> bool {
    let annotation = annotation.trim();
    annotation == "Final"
        || annotation.starts_with("Final[")
        || annotation == "typing.Final"
        || annotation.starts_with("typing.Final[")
}

fn resolve_current_augmented_assignment_target_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<String> {
    if let Some(signature) = signature {
        let signature = rewrite_imported_typing_aliases(
            node,
            &substitute_self_annotation(signature, current_owner_type_name),
        );
        if let Some(param_type) = resolve_direct_return_name_type(&signature, value_name) {
            return Some(param_type);
        }
    }

    match current_owner_name {
        Some(owner_name) => resolve_local_assignment_reference_type(
            node,
            nodes,
            signature,
            Some(owner_name),
            current_owner_type_name,
            current_line,
            value_name,
        ),
        None => resolve_module_level_assignment_reference_type(
            node,
            nodes,
            signature,
            current_line,
            value_name,
        ),
    }
}

fn direct_expr_metadata_from_assignment_site(
    assignment: &typepython_binding::AssignmentSite,
) -> typepython_syntax::DirectExprMetadata {
    typepython_syntax::DirectExprMetadata {
        value_type: assignment.value_type.clone(),
        is_awaited: assignment.is_awaited,
        value_callee: assignment.value_callee.clone(),
        value_name: assignment.value_name.clone(),
        value_member_owner_name: assignment.value_member_owner_name.clone(),
        value_member_name: assignment.value_member_name.clone(),
        value_member_through_instance: assignment.value_member_through_instance,
        value_method_owner_name: assignment.value_method_owner_name.clone(),
        value_method_name: assignment.value_method_name.clone(),
        value_method_through_instance: assignment.value_method_through_instance,
        value_subscript_target: assignment.value_subscript_target.clone(),
        value_subscript_string_key: assignment.value_subscript_string_key.clone(),
        value_subscript_index: assignment.value_subscript_index.clone(),
        value_if_true: assignment.value_if_true.clone(),
        value_if_false: assignment.value_if_false.clone(),
        value_if_guard: assignment.value_if_guard.as_ref().map(site_to_guard),
        value_bool_left: assignment.value_bool_left.clone(),
        value_bool_right: assignment.value_bool_right.clone(),
        value_binop_left: assignment.value_binop_left.clone(),
        value_binop_right: assignment.value_binop_right.clone(),
        value_binop_operator: assignment.value_binop_operator.clone(),
        value_lambda: assignment.value_lambda.clone(),
        value_list_comprehension: assignment.value_list_comprehension.clone(),
        value_generator_comprehension: assignment.value_generator_comprehension.clone(),
        value_list_elements: assignment.value_list_elements.clone(),
        value_set_elements: assignment.value_set_elements.clone(),
        value_dict_entries: assignment.value_dict_entries.clone(),
    }
}

#[derive(Debug, Clone)]
struct TypedDictFieldShape {
    value_type: String,
    required: bool,
    readonly: bool,
}

#[derive(Debug, Clone)]
struct TypedDictExtraItemsShape {
    value_type: String,
    readonly: bool,
}

#[derive(Debug, Clone)]
struct TypedDictShape {
    name: String,
    fields: BTreeMap<String, TypedDictFieldShape>,
    closed: bool,
    extra_items: Option<TypedDictExtraItemsShape>,
}

#[derive(Debug, Clone)]
struct DataclassTransformFieldShape {
    name: String,
    keyword_name: String,
    annotation: String,
    required: bool,
    kw_only: bool,
}

#[derive(Debug, Clone)]
struct DataclassTransformClassShape {
    fields: Vec<DataclassTransformFieldShape>,
    frozen: bool,
    has_explicit_init: bool,
}

fn is_typed_dict_base_name(base: &str) -> bool {
    matches!(base.trim(), "TypedDict" | "typing.TypedDict" | "typing_extensions.TypedDict")
}

fn typed_dict_known_or_extra_field<'a>(
    shape: &'a TypedDictShape,
    key: &str,
) -> Option<TypedDictFieldShapeRef<'a>> {
    if let Some(field) = shape.fields.get(key) {
        return Some(TypedDictFieldShapeRef::Known(field));
    }
    shape.extra_items.as_ref().map(TypedDictFieldShapeRef::Extra)
}

fn typed_dict_shape_has_unbounded_extra_keys(shape: &TypedDictShape) -> bool {
    !shape.closed && shape.extra_items.is_none()
}

enum TypedDictFieldShapeRef<'a> {
    Known(&'a TypedDictFieldShape),
    Extra(&'a TypedDictExtraItemsShape),
}

impl<'a> TypedDictFieldShapeRef<'a> {
    fn value_type(&self) -> &str {
        match self {
            Self::Known(field) => &field.value_type,
            Self::Extra(field) => &field.value_type,
        }
    }

    fn readonly(&self) -> bool {
        match self {
            Self::Known(field) => field.readonly,
            Self::Extra(field) => field.readonly,
        }
    }
}

fn typed_dict_literal_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    let mut diagnostics = Vec::new();
    for site in typepython_syntax::collect_typed_dict_literal_sites(&source) {
        let Some(annotation) = normalized_assignment_annotation(&site.annotation) else {
            continue;
        };
        let annotation = rewrite_imported_typing_aliases(node, annotation);
        let Some(target_shape) = resolve_known_typed_dict_shape_from_type_with_context(
            context,
            node,
            nodes,
            &annotation,
        ) else {
            continue;
        };

        let signature = resolve_scope_owner_signature(
            node,
            site.owner_name.as_deref(),
            site.owner_type_name.as_deref(),
        );
        diagnostics.extend(typed_dict_literal_entry_diagnostics(
            node,
            nodes,
            site.line,
            &site.entries,
            &target_shape,
            signature,
            site.owner_name.as_deref(),
            site.owner_type_name.as_deref(),
        ));
    }

    diagnostics
}

#[allow(clippy::too_many_arguments)]
fn typed_dict_literal_entry_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    line: usize,
    entries: &[typepython_syntax::TypedDictLiteralEntry],
    target_shape: &TypedDictShape,
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut guaranteed_keys = BTreeSet::new();

    for entry in entries {
        if entry.is_expansion {
            let Some(expansion_type) = resolve_direct_expression_type_from_metadata(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                line,
                &entry.value,
            ) else {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    line,
                    format!(
                        "TypedDict literal for `{}` uses invalid `**` expansion",
                        target_shape.name
                    ),
                ));
                continue;
            };

            let Some(expansion_shape) =
                resolve_known_typed_dict_shape_from_type(node, nodes, &expansion_type)
            else {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    line,
                    format!(
                        "TypedDict literal for `{}` uses invalid `**` expansion of `{}`",
                        target_shape.name, expansion_type
                    ),
                ));
                continue;
            };

            if typed_dict_shape_has_unbounded_extra_keys(&expansion_shape)
                && target_shape.extra_items.is_none()
            {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    line,
                    format!(
                        "TypedDict literal for `{}` cannot expand open TypedDict `{}` because it may contain undeclared keys",
                        target_shape.name, expansion_shape.name
                    ),
                ));
                continue;
            }

            for (key, field) in &expansion_shape.fields {
                let Some(target_field) = typed_dict_known_or_extra_field(target_shape, key) else {
                    diagnostics.push(typed_dict_literal_diagnostic(
                        node,
                        line,
                        format!(
                            "TypedDict literal for `{}` expands unknown key `{}`",
                            target_shape.name, key
                        ),
                    ));
                    continue;
                };

                if !direct_type_matches(node, nodes, target_field.value_type(), &field.value_type) {
                    diagnostics.push(typed_dict_literal_diagnostic(
                        node,
                        line,
                        format!(
                            "TypedDict literal for `{}` expands `{}` with `{}` where `{}` expects `{}`",
                            target_shape.name,
                            key,
                            field.value_type,
                            key,
                            target_field.value_type()
                        ),
                    ));
                }

                if field.required {
                    guaranteed_keys.insert(key.clone());
                }
            }

            if let Some(extra_items) = &expansion_shape.extra_items
                && target_shape.extra_items.as_ref().is_none_or(|target_extra| {
                    !direct_type_matches(
                        node,
                        nodes,
                        &target_extra.value_type,
                        &extra_items.value_type,
                    )
                })
            {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    line,
                    format!(
                        "TypedDict literal for `{}` expands `{}` with additional keys of type `{}` that are not accepted by the target",
                        target_shape.name,
                        expansion_shape.name,
                        extra_items.value_type
                    ),
                ));
            }

            continue;
        }

        let Some(key) = entry.key.as_deref() else {
            diagnostics.push(typed_dict_literal_diagnostic(
                node,
                line,
                format!("TypedDict literal for `{}` uses a non-literal key", target_shape.name),
            ));
            continue;
        };

        let Some(target_field) = typed_dict_known_or_extra_field(target_shape, key) else {
            diagnostics.push(typed_dict_literal_diagnostic(
                node,
                line,
                format!("TypedDict literal for `{}` uses unknown key `{}`", target_shape.name, key),
            ));
            continue;
        };

        if let Some(actual_type) = resolve_direct_expression_type_from_metadata(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            line,
            &entry.value,
        ) {
            if !direct_type_matches(node, nodes, target_field.value_type(), &actual_type) {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    line,
                    format!(
                        "TypedDict literal for `{}` assigns `{}` to key `{}` where `{}` expects `{}`",
                        target_shape.name,
                        actual_type,
                        key,
                        key,
                        target_field.value_type()
                    ),
                ));
            }
        }

        guaranteed_keys.insert(key.to_owned());
    }

    for (key, field) in &target_shape.fields {
        if field.required && !guaranteed_keys.contains(key) {
            diagnostics.push(typed_dict_literal_diagnostic(
                node,
                line,
                format!(
                    "TypedDict literal for `{}` is missing required key `{}`",
                    target_shape.name, key
                ),
            ));
        }
    }

    diagnostics
}

fn typed_dict_literal_diagnostic(
    node: &typepython_graph::ModuleNode,
    line: usize,
    message: String,
) -> Diagnostic {
    Diagnostic::error("TPY4013", message).with_span(Span::new(
        node.module_path.display().to_string(),
        line,
        1,
        line,
        1,
    ))
}

fn direct_expr_metadata_for_known_type(value_type: &str) -> typepython_syntax::DirectExprMetadata {
    typepython_syntax::DirectExprMetadata {
        value_type: Some(String::from(value_type)),
        is_awaited: false,
        value_callee: None,
        value_name: None,
        value_member_owner_name: None,
        value_member_name: None,
        value_member_through_instance: false,
        value_method_owner_name: None,
        value_method_name: None,
        value_method_through_instance: false,
        value_subscript_target: None,
        value_subscript_string_key: None,
        value_subscript_index: None,
        value_if_true: None,
        value_if_false: None,
        value_if_guard: None,
        value_bool_left: None,
        value_bool_right: None,
        value_binop_left: None,
        value_binop_right: None,
        value_binop_operator: None,
        value_lambda: None,
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_augmented_assignment_result_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    current_line: usize,
    operator: Option<&str>,
    left_type: &str,
    value: &typepython_syntax::DirectExprMetadata,
) -> Option<String> {
    let left = direct_expr_metadata_for_known_type(left_type);
    resolve_direct_binop_type(
        node,
        nodes,
        signature,
        owner_name,
        owner_type_name,
        current_line,
        Some(&left),
        Some(value),
        operator.filter(|operator| !operator.is_empty()),
    )
    .or_else(|| {
        resolve_direct_expression_type_from_metadata(
            node,
            nodes,
            signature,
            owner_name,
            owner_type_name,
            current_line,
            value,
        )
    })
}

fn typed_dict_readonly_mutation_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_typed_dict_mutation_sites(&source)
        .into_iter()
        .filter_map(|site| {
            let signature = resolve_scope_owner_signature(
                node,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
            );
            let owner_type = resolve_direct_expression_type_from_metadata(
                node,
                nodes,
                signature,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
                site.line,
                &site.target,
            )?;
            let key = site.key.as_deref()?;
            let target_shape =
                resolve_known_typed_dict_shape_from_type_with_context(context, node, nodes, &owner_type)?;
            let Some(field) = typed_dict_known_or_extra_field(&target_shape, key) else {
                return Some(
                    Diagnostic::error(
                        "TPY4001",
                        format!(
                            "TypedDict item `{}` on `{}` in module `{}` is not a declared key",
                            key,
                            target_shape.name,
                            node.module_path.display()
                        ),
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        site.line,
                        1,
                        site.line,
                        1,
                    )),
                );
            };
            if field.readonly() {
                return Some(
                    Diagnostic::error(
                        "TPY4016",
                        match site.kind {
                            typepython_syntax::TypedDictMutationKind::Assignment => format!(
                                "TypedDict item `{}` on `{}` in module `{}` is read-only and cannot be assigned",
                                key,
                                target_shape.name,
                                node.module_path.display()
                            ),
                            typepython_syntax::TypedDictMutationKind::AugmentedAssignment => format!(
                                "TypedDict item `{}` on `{}` in module `{}` is read-only and cannot be updated with augmented assignment",
                                key,
                                target_shape.name,
                                node.module_path.display()
                            ),
                            typepython_syntax::TypedDictMutationKind::Delete => format!(
                                "TypedDict item `{}` on `{}` in module `{}` is read-only and cannot be deleted",
                                key,
                                target_shape.name,
                                node.module_path.display()
                            ),
                        },
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        site.line,
                        1,
                        site.line,
                        1,
                    )),
                );
            }

            match site.kind {
                typepython_syntax::TypedDictMutationKind::Assignment => {
                    let value = site.value.as_ref()?;
                    let contextual = resolve_contextual_call_arg_type(
                        node,
                        nodes,
                        site.line,
                        value,
                        Some(field.value_type()),
                    );
                    if let Some(mut result) = contextual {
                        if let Some(diagnostic) = result.diagnostics.pop() {
                            return Some(diagnostic);
                        }
                        let actual = result.actual_type;
                        if !direct_type_matches(node, nodes, field.value_type(), &actual) {
                            return Some(
                                Diagnostic::error(
                                    "TPY4001",
                                    format!(
                                        "TypedDict item `{}` on `{}` in module `{}` assigns `{}` where `{}` expects `{}`",
                                        key,
                                        target_shape.name,
                                        node.module_path.display(),
                                        actual,
                                        key,
                                        field.value_type()
                                    ),
                                )
                                .with_span(Span::new(
                                    node.module_path.display().to_string(),
                                    site.line,
                                    1,
                                    site.line,
                                    1,
                                )),
                            );
                        }
                        return None;
                    }

                    let actual = resolve_direct_expression_type_from_metadata(
                        node,
                        nodes,
                        signature,
                        site.owner_name.as_deref(),
                        site.owner_type_name.as_deref(),
                        site.line,
                        value,
                    )?;
                    if !direct_type_matches(node, nodes, field.value_type(), &actual) {
                        return Some(
                            Diagnostic::error(
                                "TPY4001",
                                format!(
                                    "TypedDict item `{}` on `{}` in module `{}` assigns `{}` where `{}` expects `{}`",
                                    key,
                                    target_shape.name,
                                    node.module_path.display(),
                                    actual,
                                    key,
                                    field.value_type()
                                ),
                            )
                            .with_span(Span::new(
                                node.module_path.display().to_string(),
                                site.line,
                                1,
                                site.line,
                                1,
                            )),
                        );
                    }
                }
                typepython_syntax::TypedDictMutationKind::AugmentedAssignment => {
                    let value = site.value.as_ref()?;
                    let actual = resolve_augmented_assignment_result_type(
                        node,
                        nodes,
                        signature,
                        site.owner_name.as_deref(),
                        site.owner_type_name.as_deref(),
                        site.line,
                        site.operator.as_deref(),
                        field.value_type(),
                        value,
                    )?;
                    if !direct_type_matches(node, nodes, field.value_type(), &actual) {
                        return Some(
                            Diagnostic::error(
                                "TPY4001",
                                format!(
                                    "augmented assignment on TypedDict item `{}` on `{}` in module `{}` produces `{}` where `{}` expects `{}`",
                                    key,
                                    target_shape.name,
                                    node.module_path.display(),
                                    actual,
                                    key,
                                    field.value_type()
                                ),
                            )
                            .with_span(Span::new(
                                node.module_path.display().to_string(),
                                site.line,
                                1,
                                site.line,
                                1,
                            )),
                        );
                    }
                }
                typepython_syntax::TypedDictMutationKind::Delete => {}
            }

            None
        })
        .collect()
}

enum WritableSubscriptSignature {
    Writable { key_type: String, value_type: String },
    ReadOnly,
}

fn resolve_writable_subscript_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    owner_type_name: &str,
) -> Option<WritableSubscriptSignature> {
    let normalized = normalize_type_text(owner_type_name);
    if let Some((head, args)) = split_generic_type(&normalized) {
        match head {
            "Mapping" | "typing.Mapping" | "collections.abc.Mapping" if args.len() == 2 => {
                return Some(WritableSubscriptSignature::ReadOnly);
            }
            _ => {}
        }
    }

    let nominal_owner_name = split_generic_type(&normalized)
        .map(|(head, _)| head.to_owned())
        .unwrap_or_else(|| normalized.clone());
    let (class_node, class_decl) = resolve_direct_base(nodes, node, &nominal_owner_name)?;
    if let Some(setitem) =
        find_owned_callable_declaration(nodes, class_node, class_decl, "__setitem__")
    {
        let signature = rewrite_imported_typing_aliases(
            node,
            &substitute_self_annotation(&setitem.detail, Some(&normalized)),
        );
        let params = direct_param_types(&signature)?;
        let params = match setitem.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
            typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => {
                params
            }
            _ => params.into_iter().skip(1).collect(),
        };
        if params.len() == 2 {
            return Some(WritableSubscriptSignature::Writable {
                key_type: normalize_type_text(&params[0]),
                value_type: normalize_type_text(&params[1]),
            });
        }
    }

    find_owned_callable_declaration(nodes, class_node, class_decl, "__getitem__")
        .map(|_| WritableSubscriptSignature::ReadOnly)
}

fn subscript_assignment_type_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_typed_dict_mutation_sites(&source)
        .into_iter()
        .filter_map(|site| {
            if site.kind == typepython_syntax::TypedDictMutationKind::Delete {
                return None;
            }

            let signature = resolve_scope_owner_signature(
                node,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
            );
            let owner_type = resolve_direct_expression_type_from_metadata(
                node,
                nodes,
                signature,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
                site.line,
                &site.target,
            )?;

            if resolve_known_typed_dict_shape_from_type_with_context(context, node, nodes, &owner_type)
                .is_some()
            {
                return None;
            }

            match resolve_writable_subscript_signature(node, nodes, &owner_type)? {
                WritableSubscriptSignature::ReadOnly => Some(
                    Diagnostic::error(
                        "TPY4001",
                        format!(
                            "subscript assignment target `{}` in module `{}` is not writable via `__setitem__`",
                            owner_type,
                            node.module_path.display(),
                        ),
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        site.line,
                        1,
                        site.line,
                        1,
                    )),
                ),
                WritableSubscriptSignature::Writable { key_type, value_type } => {
                    let actual_key = resolve_direct_expression_type_from_metadata(
                        node,
                        nodes,
                        signature,
                        site.owner_name.as_deref(),
                        site.owner_type_name.as_deref(),
                        site.line,
                        &site.key_value,
                    )?;
                    if !direct_type_is_assignable(node, nodes, &key_type, &actual_key) {
                        return Some(
                            Diagnostic::error(
                                "TPY4001",
                                format!(
                                    "subscript assignment on `{}` in module `{}` passes key `{}` where `__setitem__` expects `{}`",
                                    owner_type,
                                    node.module_path.display(),
                                    actual_key,
                                    key_type,
                                ),
                            )
                            .with_span(Span::new(
                                node.module_path.display().to_string(),
                                site.line,
                                1,
                                site.line,
                                1,
                            )),
                        );
                    }

                    let value = site.value.as_ref()?;
                    match site.kind {
                        typepython_syntax::TypedDictMutationKind::Assignment => {
                            let contextual = resolve_contextual_call_arg_type(
                                node,
                                nodes,
                                site.line,
                                value,
                                Some(&value_type),
                            );
                            if let Some(mut result) = contextual {
                                if let Some(diagnostic) = result.diagnostics.pop() {
                                    return Some(diagnostic);
                                }
                                let actual_value = result.actual_type;
                                if !direct_type_is_assignable(node, nodes, &value_type, &actual_value) {
                                    return Some(
                                        Diagnostic::error(
                                            "TPY4001",
                                            format!(
                                                "subscript assignment on `{}` in module `{}` passes value `{}` where `__setitem__` expects `{}`",
                                                owner_type,
                                                node.module_path.display(),
                                                actual_value,
                                                value_type,
                                            ),
                                        )
                                        .with_span(Span::new(
                                            node.module_path.display().to_string(),
                                            site.line,
                                            1,
                                            site.line,
                                            1,
                                        )),
                                    );
                                }
                                return None;
                            }
                            let actual_value = resolve_direct_expression_type_from_metadata(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                value,
                            )?;
                            if !direct_type_is_assignable(node, nodes, &value_type, &actual_value) {
                                return Some(
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "subscript assignment on `{}` in module `{}` passes value `{}` where `__setitem__` expects `{}`",
                                            owner_type,
                                            node.module_path.display(),
                                            actual_value,
                                            value_type,
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    )),
                                );
                            }
                        }
                        typepython_syntax::TypedDictMutationKind::AugmentedAssignment => {
                            let Some(readable_type) = resolve_subscript_type_from_target_type(
                                node,
                                nodes,
                                &owner_type,
                                site.key.as_deref(),
                                None,
                            ) else {
                                return Some(
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "augmented subscript assignment target `{}` in module `{}` is not readable via `__getitem__`",
                                            owner_type,
                                            node.module_path.display(),
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    )),
                                );
                            };
                            let actual_value = resolve_augmented_assignment_result_type(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                site.operator.as_deref(),
                                &readable_type,
                                value,
                            )?;
                            if !direct_type_is_assignable(node, nodes, &value_type, &actual_value) {
                                return Some(
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "augmented subscript assignment on `{}` in module `{}` produces `{}` where `__setitem__` expects `{}`",
                                            owner_type,
                                            node.module_path.display(),
                                            actual_value,
                                            value_type,
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    )),
                                );
                            }
                        }
                        typepython_syntax::TypedDictMutationKind::Delete => {}
                    }

                    None
                }
            }
        })
        .collect()
}

fn frozen_dataclass_transform_mutation_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_frozen_field_mutation_sites(&source)
        .into_iter()
        .filter_map(|site| {
            let signature = resolve_scope_owner_signature(
                node,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
            );
            let target_type = resolve_direct_expression_type_from_metadata(
                node,
                nodes,
                signature,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
                site.line,
                &site.target,
            )?;
            let shape = resolve_known_dataclass_transform_shape_from_type(node, nodes, &target_type)?;
            if !shape.frozen || !shape.fields.iter().any(|field| field.name == site.field_name) {
                return None;
            }

            let in_initializer = site.owner_name.as_deref() == Some("__init__")
                && site.owner_type_name.as_deref() == Some(target_type.as_str())
                && site.target.value_name.as_deref() == Some("self");
            if in_initializer {
                return None;
            }

            let message = match site.kind {
                typepython_syntax::FrozenFieldMutationKind::Assignment => format!(
                    "frozen dataclass-transform field `{}` on `{}` in module `{}` cannot be assigned after initialization",
                    site.field_name,
                    target_type,
                    node.module_path.display()
                ),
                typepython_syntax::FrozenFieldMutationKind::AugmentedAssignment => format!(
                    "frozen dataclass-transform field `{}` on `{}` in module `{}` cannot be updated with augmented assignment after initialization",
                    site.field_name,
                    target_type,
                    node.module_path.display()
                ),
                typepython_syntax::FrozenFieldMutationKind::Delete => format!(
                    "frozen dataclass-transform field `{}` on `{}` in module `{}` cannot be deleted after initialization",
                    site.field_name,
                    target_type,
                    node.module_path.display()
                ),
            };
            Some(Diagnostic::error("TPY4001", message).with_span(Span::new(
                node.module_path.display().to_string(),
                site.line,
                1,
                site.line,
                1,
            )))
        })
        .collect()
}

fn frozen_plain_dataclass_mutation_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_frozen_field_mutation_sites(&source)
        .into_iter()
        .filter_map(|site| {
            let signature = resolve_scope_owner_signature(
                node,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
            );
            let target_type = resolve_direct_expression_type_from_metadata(
                node,
                nodes,
                signature,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
                site.line,
                &site.target,
            )?;
            let shape = resolve_known_plain_dataclass_shape_from_type(node, nodes, &target_type)?;
            if !shape.frozen || !shape.fields.iter().any(|field| field.name == site.field_name) {
                return None;
            }

            let in_initializer = site.owner_name.as_deref() == Some("__init__")
                && site.owner_type_name.as_deref() == Some(target_type.as_str())
                && site.target.value_name.as_deref() == Some("self");
            if in_initializer {
                return None;
            }

            let message = match site.kind {
                typepython_syntax::FrozenFieldMutationKind::Assignment => format!(
                    "frozen dataclass field `{}` on `{}` in module `{}` cannot be assigned after initialization",
                    site.field_name,
                    target_type,
                    node.module_path.display()
                ),
                typepython_syntax::FrozenFieldMutationKind::AugmentedAssignment => format!(
                    "frozen dataclass field `{}` on `{}` in module `{}` cannot be updated with augmented assignment after initialization",
                    site.field_name,
                    target_type,
                    node.module_path.display()
                ),
                typepython_syntax::FrozenFieldMutationKind::Delete => format!(
                    "frozen dataclass field `{}` on `{}` in module `{}` cannot be deleted after initialization",
                    site.field_name,
                    target_type,
                    node.module_path.display()
                ),
            };
            Some(Diagnostic::error("TPY4001", message).with_span(Span::new(
                node.module_path.display().to_string(),
                site.line,
                1,
                site.line,
                1,
            )))
        })
        .collect()
}

enum WritableAttributeTarget<'a> {
    Value(&'a Declaration),
    PropertySetter(&'a Declaration),
    ReadOnlyProperty,
    NonWritable,
}

fn find_owned_writable_member_target<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<WritableAttributeTarget<'a>> {
    if let Some(declaration) =
        find_owned_value_declaration(nodes, class_node, class_decl, member_name)
        && !declaration.is_class_var
    {
        return Some(WritableAttributeTarget::Value(declaration));
    }

    let callables = find_owned_callable_declarations(nodes, class_node, class_decl, member_name);
    if let Some(setter) = callables.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Function
            && declaration.method_kind == Some(typepython_syntax::MethodKind::PropertySetter)
    }) {
        return Some(WritableAttributeTarget::PropertySetter(setter));
    }
    if callables.iter().any(|declaration| {
        declaration.kind == DeclarationKind::Function
            && declaration.method_kind == Some(typepython_syntax::MethodKind::Property)
    }) {
        return Some(WritableAttributeTarget::ReadOnlyProperty);
    }

    Some(WritableAttributeTarget::NonWritable)
}

fn resolve_writable_member_type(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    owner_type_name: &str,
) -> Option<String> {
    match declaration.kind {
        DeclarationKind::Value => resolve_readable_member_type(node, declaration, owner_type_name),
        DeclarationKind::Function
            if declaration.method_kind == Some(typepython_syntax::MethodKind::PropertySetter) =>
        {
            let signature = rewrite_imported_typing_aliases(
                node,
                &substitute_self_annotation(&declaration.detail, Some(owner_type_name)),
            );
            let params = direct_param_types(&signature)?;
            let params = params.into_iter().skip(1).collect::<Vec<_>>();
            (params.len() == 1).then(|| normalize_type_text(&params[0]))
        }
        _ => None,
    }
}

fn should_defer_attribute_assignment_to_frozen_checks(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    site: &typepython_syntax::FrozenFieldMutationSite,
    target_type: &str,
) -> bool {
    if let Some(shape) = resolve_known_dataclass_transform_shape_from_type(node, nodes, target_type)
        && shape.frozen
        && shape.fields.iter().any(|field| field.name == site.field_name)
    {
        let in_initializer = site.owner_name.as_deref() == Some("__init__")
            && site.owner_type_name.as_deref() == Some(target_type)
            && site.target.value_name.as_deref() == Some("self");
        return !in_initializer;
    }
    if let Some(shape) = resolve_known_plain_dataclass_shape_from_type(node, nodes, target_type)
        && shape.frozen
        && shape.fields.iter().any(|field| field.name == site.field_name)
    {
        let in_initializer = site.owner_name.as_deref() == Some("__init__")
            && site.owner_type_name.as_deref() == Some(target_type)
            && site.target.value_name.as_deref() == Some("self");
        return !in_initializer;
    }
    false
}

fn attribute_assignment_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_frozen_field_mutation_sites(&source)
        .into_iter()
        .filter_map(|site| {
            if site.kind == typepython_syntax::FrozenFieldMutationKind::Delete {
                return None;
            }

            if site.owner_name.as_deref() == Some("__init__")
                && site.target.value_name.as_deref() == Some("self")
            {
                return None;
            }

            let signature = resolve_scope_owner_signature(
                node,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
            );
            let target_type = resolve_direct_expression_type_from_metadata(
                node,
                nodes,
                signature,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
                site.line,
                &site.target,
            )?;

            if should_defer_attribute_assignment_to_frozen_checks(node, nodes, &site, &target_type) {
                return None;
            }

            let (class_node, class_decl) = resolve_direct_base(nodes, node, &target_type)?;
            match find_owned_writable_member_target(nodes, class_node, class_decl, &site.field_name) {
                Some(WritableAttributeTarget::Value(declaration)) => {
                    if declaration.is_final {
                        return Some(final_attribute_reassignment_diagnostic(
                            &node.module_path,
                            &target_type,
                            &site.field_name,
                        ));
                    }
                    let expected = resolve_writable_member_type(node, declaration, &target_type)?;
                    let value = site.value.as_ref()?;
                    match site.kind {
                        typepython_syntax::FrozenFieldMutationKind::Assignment => {
                            let contextual = resolve_contextual_call_arg_type(
                                node,
                                nodes,
                                site.line,
                                value,
                                Some(&expected),
                            );
                            if let Some(mut result) = contextual {
                                if let Some(diagnostic) = result.diagnostics.pop() {
                                    return Some(diagnostic);
                                }
                                let actual = result.actual_type;
                                return (!direct_type_matches(node, nodes, &expected, &actual))
                                    .then(|| {
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "attribute assignment on `{}` in module `{}` assigns `{}` where member `{}` expects `{}`",
                                            target_type,
                                            node.module_path.display(),
                                            actual,
                                            site.field_name,
                                            expected,
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    ))
                                });
                            }
                            let actual = resolve_direct_expression_type_from_metadata(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                value,
                            )?;
                            (!direct_type_matches(node, nodes, &expected, &actual)).then(|| {
                                Diagnostic::error(
                                    "TPY4001",
                                    format!(
                                        "attribute assignment on `{}` in module `{}` assigns `{}` where member `{}` expects `{}`",
                                        target_type,
                                        node.module_path.display(),
                                        actual,
                                        site.field_name,
                                        expected,
                                    ),
                                )
                                .with_span(Span::new(
                                    node.module_path.display().to_string(),
                                    site.line,
                                    1,
                                    site.line,
                                    1,
                                ))
                            })
                        }
                        typepython_syntax::FrozenFieldMutationKind::AugmentedAssignment => {
                            let actual = resolve_augmented_assignment_result_type(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                site.operator.as_deref(),
                                &expected,
                                value,
                            )?;
                            (!direct_type_matches(node, nodes, &expected, &actual)).then(|| {
                                Diagnostic::error(
                                    "TPY4001",
                                    format!(
                                        "augmented attribute assignment on `{}` in module `{}` produces `{}` where member `{}` expects `{}`",
                                        target_type,
                                        node.module_path.display(),
                                        actual,
                                        site.field_name,
                                        expected,
                                    ),
                                )
                                .with_span(Span::new(
                                    node.module_path.display().to_string(),
                                    site.line,
                                    1,
                                    site.line,
                                    1,
                                ))
                            })
                        }
                        typepython_syntax::FrozenFieldMutationKind::Delete => None,
                    }
                }
                Some(WritableAttributeTarget::PropertySetter(declaration)) => {
                    let expected = resolve_writable_member_type(node, declaration, &target_type)?;
                    let value = site.value.as_ref()?;
                    match site.kind {
                        typepython_syntax::FrozenFieldMutationKind::Assignment => {
                            let contextual = resolve_contextual_call_arg_type(
                                node,
                                nodes,
                                site.line,
                                value,
                                Some(&expected),
                            );
                            if let Some(mut result) = contextual {
                                if let Some(diagnostic) = result.diagnostics.pop() {
                                    return Some(diagnostic);
                                }
                                let actual = result.actual_type;
                                return (!direct_type_matches(node, nodes, &expected, &actual))
                                    .then(|| {
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "attribute assignment on `{}` in module `{}` assigns `{}` where member `{}` expects `{}`",
                                            target_type,
                                            node.module_path.display(),
                                            actual,
                                            site.field_name,
                                            expected,
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    ))
                                });
                            }
                            let actual = resolve_direct_expression_type_from_metadata(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                value,
                            )?;
                            (!direct_type_matches(node, nodes, &expected, &actual)).then(|| {
                                Diagnostic::error(
                                    "TPY4001",
                                    format!(
                                        "attribute assignment on `{}` in module `{}` assigns `{}` where member `{}` expects `{}`",
                                        target_type,
                                        node.module_path.display(),
                                        actual,
                                        site.field_name,
                                        expected,
                                    ),
                                )
                                .with_span(Span::new(
                                    node.module_path.display().to_string(),
                                    site.line,
                                    1,
                                    site.line,
                                    1,
                                ))
                            })
                        }
                        typepython_syntax::FrozenFieldMutationKind::AugmentedAssignment => {
                            let Some(readable) = find_owned_readable_member_declaration(
                                nodes,
                                class_node,
                                class_decl,
                                &site.field_name,
                            ) else {
                                return Some(
                                    Diagnostic::error(
                                        "TPY4001",
                                        format!(
                                            "attribute `{}` on `{}` in module `{}` is not readable for augmented assignment",
                                            site.field_name,
                                            target_type,
                                            node.module_path.display(),
                                        ),
                                    )
                                    .with_span(Span::new(
                                        node.module_path.display().to_string(),
                                        site.line,
                                        1,
                                        site.line,
                                        1,
                                    )),
                                );
                            };
                            let readable_type = resolve_readable_member_type(node, readable, &target_type)?;
                            let actual = resolve_augmented_assignment_result_type(
                                node,
                                nodes,
                                signature,
                                site.owner_name.as_deref(),
                                site.owner_type_name.as_deref(),
                                site.line,
                                site.operator.as_deref(),
                                &readable_type,
                                value,
                            )?;
                            (!direct_type_matches(node, nodes, &expected, &actual)).then(|| {
                                Diagnostic::error(
                                    "TPY4001",
                                    format!(
                                        "augmented attribute assignment on `{}` in module `{}` produces `{}` where member `{}` expects `{}`",
                                        target_type,
                                        node.module_path.display(),
                                        actual,
                                        site.field_name,
                                        expected,
                                    ),
                                )
                                .with_span(Span::new(
                                    node.module_path.display().to_string(),
                                    site.line,
                                    1,
                                    site.line,
                                    1,
                                ))
                            })
                        }
                        typepython_syntax::FrozenFieldMutationKind::Delete => None,
                    }
                }
                Some(WritableAttributeTarget::ReadOnlyProperty) => Some(
                    Diagnostic::error(
                        "TPY4001",
                        format!(
                            "property `{}` on `{}` in module `{}` is not writable",
                            site.field_name,
                            target_type,
                            node.module_path.display(),
                        ),
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        site.line,
                        1,
                        site.line,
                        1,
                    )),
                ),
                Some(WritableAttributeTarget::NonWritable) | None => None,
            }
        })
        .collect()
}

fn resolve_direct_expression_type_from_metadata(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
) -> Option<String> {
    if let Some(lambda) = metadata.value_lambda.as_deref() {
        let (param_types, return_type) = resolve_contextual_lambda_callable_signature(
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            current_line,
            lambda,
            signature,
            None,
        )?;
        return Some(format_callable_annotation(&param_types, &return_type));
    }
    let value_if_guard = metadata.value_if_guard.as_ref().map(guard_to_site);
    resolve_direct_expression_type(
        node,
        nodes,
        signature,
        None,
        current_owner_name,
        current_owner_type_name,
        current_line,
        metadata.value_type.as_deref(),
        metadata.is_awaited,
        metadata.value_callee.as_deref(),
        metadata.value_name.as_deref(),
        metadata.value_member_owner_name.as_deref(),
        metadata.value_member_name.as_deref(),
        metadata.value_member_through_instance,
        metadata.value_method_owner_name.as_deref(),
        metadata.value_method_name.as_deref(),
        metadata.value_method_through_instance,
        metadata.value_subscript_target.as_deref(),
        metadata.value_subscript_string_key.as_deref(),
        metadata.value_subscript_index.as_deref(),
        metadata.value_if_true.as_deref(),
        metadata.value_if_false.as_deref(),
        value_if_guard.as_ref(),
        metadata.value_bool_left.as_deref(),
        metadata.value_bool_right.as_deref(),
        metadata.value_binop_left.as_deref(),
        metadata.value_binop_right.as_deref(),
        metadata.value_binop_operator.as_deref(),
    )
}

fn resolve_known_typed_dict_shape_from_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<TypedDictShape> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    resolve_known_typed_dict_shape_from_type_with_context(&context, node, nodes, type_name)
}

fn resolve_known_typed_dict_shape_from_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<TypedDictShape> {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    resolve_known_typed_dict_shape_with_context(context, node, nodes, &type_name)
}

fn resolve_known_typed_dict_shape_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<TypedDictShape> {
    let (class_node, class_decl) = resolve_direct_base(nodes, node, type_name)?;
    if !is_typed_dict_class(nodes, class_node, class_decl, &mut BTreeSet::new()) {
        return None;
    }

    let typed_dict_metadata = load_typed_dict_class_metadata(context, class_node);
    let mut fields = BTreeMap::new();
    collect_typed_dict_fields(
        context,
        nodes,
        class_node,
        class_decl,
        &typed_dict_metadata,
        &mut BTreeSet::new(),
        &mut fields,
    );
    let (closed, extra_items) = collect_typed_dict_openness(
        context,
        node,
        nodes,
        class_node,
        class_decl,
        &mut BTreeSet::new(),
    )?;
    Some(TypedDictShape { name: class_decl.name.clone(), fields, closed, extra_items })
}

fn is_typed_dict_class(
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visited: &mut BTreeSet<(String, String)>,
) -> bool {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visited.insert(key) {
        return false;
    }

    is_typed_dict_base_name(&class_decl.name)
        || class_decl.bases.iter().any(|base| {
            is_typed_dict_base_name(base)
                || resolve_direct_base(nodes, class_node, base).is_some_and(
                    |(base_node, base_decl)| {
                        is_typed_dict_class(nodes, base_node, base_decl, visited)
                    },
                )
        })
}

fn collect_typed_dict_fields(
    context: &CheckerContext<'_>,
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    typed_dict_metadata: &BTreeMap<String, typepython_syntax::TypedDictClassMetadata>,
    visited: &mut BTreeSet<(String, String)>,
    fields: &mut BTreeMap<String, TypedDictFieldShape>,
) {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visited.insert(key) {
        return;
    }

    for base in &class_decl.bases {
        if let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) {
            if is_typed_dict_class(nodes, base_node, base_decl, &mut BTreeSet::new()) {
                collect_typed_dict_fields(
                    context,
                    nodes,
                    base_node,
                    base_decl,
                    &load_typed_dict_class_metadata(context, base_node),
                    visited,
                    fields,
                );
            }
        }
    }

    let total_default = typed_dict_metadata
        .get(&class_decl.name)
        .and_then(|metadata| metadata.total)
        .unwrap_or(true);
    for declaration in class_node.declarations.iter().filter(|declaration| {
        declaration.kind == DeclarationKind::Value
            && declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && !declaration.detail.is_empty()
    }) {
        fields.insert(
            declaration.name.clone(),
            parse_typed_dict_field_shape(
                &rewrite_imported_typing_aliases(class_node, &declaration.detail),
                total_default,
            ),
        );
    }
}

fn collect_typed_dict_openness(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visited: &mut BTreeSet<(String, String)>,
) -> Option<(bool, Option<TypedDictExtraItemsShape>)> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visited.insert(key) {
        return Some((false, None));
    }

    let mut inherited_closed = false;
    let mut inherited_extra_items = None;
    for base in &class_decl.bases {
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) else {
            continue;
        };
        if !is_typed_dict_class(nodes, base_node, base_decl, &mut BTreeSet::new()) {
            continue;
        }
        let (base_closed, base_extra_items) =
            collect_typed_dict_openness(context, node, nodes, base_node, base_decl, visited)?;
        inherited_closed |= base_closed;
        if inherited_extra_items.is_none() {
            inherited_extra_items = base_extra_items;
        }
    }

    let metadata = load_typed_dict_class_metadata(context, class_node);
    let metadata = metadata.get(&class_decl.name);
    let mut closed = inherited_closed;
    let mut extra_items = inherited_extra_items;

    if let Some(annotation) = metadata.and_then(|metadata| metadata.extra_items.as_ref()) {
        if let Some(parsed) = parse_typed_dict_extra_items(node, &annotation.annotation) {
            if parsed.value_type == "Never" {
                closed = true;
                extra_items = None;
            } else {
                closed = false;
                extra_items = Some(parsed);
            }
        }
    }

    if let Some(explicit_closed) = metadata.and_then(|metadata| metadata.closed) {
        if explicit_closed {
            closed = true;
            extra_items = None;
        } else if extra_items.is_none() {
            closed = false;
        }
    }

    Some((closed, extra_items))
}

fn load_typed_dict_class_metadata(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<String, typepython_syntax::TypedDictClassMetadata> {
    context.load_typed_dict_class_metadata(node)
}

fn load_dataclass_transform_module_info(
    node: &typepython_graph::ModuleNode,
) -> Option<typepython_syntax::DataclassTransformModuleInfo> {
    load_dataclass_transform_module_info_uncached(node)
}

fn parse_typed_dict_extra_items(
    node: &typepython_graph::ModuleNode,
    annotation: &str,
) -> Option<TypedDictExtraItemsShape> {
    let mut value_type = normalize_type_text(&rewrite_imported_typing_aliases(node, annotation));
    let mut readonly = false;

    if let Some(inner) =
        value_type.strip_prefix("ReadOnly[").and_then(|inner| inner.strip_suffix(']'))
    {
        value_type = normalize_type_text(inner);
        readonly = true;
    }

    Some(TypedDictExtraItemsShape { value_type, readonly })
}

fn parse_typed_dict_field_shape(annotation: &str, total_default: bool) -> TypedDictFieldShape {
    let mut value_type = normalize_type_text(annotation);
    let mut required = total_default;
    let mut readonly = false;

    loop {
        if let Some(inner) =
            value_type.strip_prefix("Required[").and_then(|inner| inner.strip_suffix(']'))
        {
            value_type = normalize_type_text(inner);
            required = true;
            continue;
        }
        if let Some(inner) =
            value_type.strip_prefix("NotRequired[").and_then(|inner| inner.strip_suffix(']'))
        {
            value_type = normalize_type_text(inner);
            required = false;
            continue;
        }
        if let Some(inner) =
            value_type.strip_prefix("ReadOnly[").and_then(|inner| inner.strip_suffix(']'))
        {
            value_type = normalize_type_text(inner);
            readonly = true;
            continue;
        }
        break;
    }

    TypedDictFieldShape { value_type, required, readonly }
}

fn callable_assignment_result(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    assignment: &typepython_binding::AssignmentSite,
    expected: &str,
) -> Option<Option<Diagnostic>> {
    let (expected_params, expected_return) = parse_callable_annotation(expected)?;
    let (actual_params, actual_return) =
        resolve_callable_assignment_signature(node, nodes, assignment)?;

    let params_match = expected_params.as_ref().is_none_or(|expected_params| {
        expected_params.len() == actual_params.len()
            && expected_params.iter().zip(actual_params.iter()).all(
                |(expected_param, actual_param)| {
                    direct_type_is_assignable(node, nodes, expected_param, actual_param)
                },
            )
    });

    let matches =
        params_match && direct_type_is_assignable(node, nodes, &expected_return, &actual_return);

    Some((!matches).then(|| {
        let actual_signature = format!("({})->{}", actual_params.join(","), actual_return);
        Diagnostic::error(
            "TPY4001",
            match (&assignment.owner_type_name, &assignment.owner_name) {
                (Some(owner_type_name), Some(owner_name)) => format!(
                    "type `{}` in module `{}` assigns callable `{}` where local `{}` in `{}` expects `{}`",
                    owner_type_name,
                    node.module_path.display(),
                    actual_signature,
                    assignment.name,
                    owner_name,
                    expected
                ),
                (None, Some(owner_name)) => format!(
                    "function `{}` in module `{}` assigns callable `{}` where local `{}` expects `{}`",
                    owner_name,
                    node.module_path.display(),
                    actual_signature,
                    assignment.name,
                    expected
                ),
                _ => format!(
                    "module `{}` assigns callable `{}` where `{}` expects `{}`",
                    node.module_path.display(),
                    actual_signature,
                    assignment.name,
                    expected
                ),
            },
        )
    }))
}

fn resolve_callable_assignment_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    assignment: &typepython_binding::AssignmentSite,
) -> Option<(Vec<String>, String)> {
    if let Some(lambda) = assignment.value_lambda.as_deref() {
        let expected = normalized_assignment_annotation(assignment.annotation.as_deref()?)?;
        return resolve_contextual_lambda_callable_signature(
            node,
            nodes,
            assignment.owner_name.as_deref(),
            assignment.owner_type_name.as_deref(),
            assignment.line,
            lambda,
            Some(expected),
            None,
        );
    }

    if let Some(value_name) = assignment.value_name.as_deref() {
        let function = resolve_direct_function(node, nodes, value_name)?;
        let actual_params = direct_param_types(&function.detail).unwrap_or_default();
        let actual_return = resolve_direct_callable_return_type(node, nodes, value_name)?;
        return Some((actual_params, actual_return));
    }

    let owner_name = assignment.value_member_owner_name.as_deref()?;
    let member_name = assignment.value_member_name.as_deref()?;
    resolve_direct_member_callable_signature(
        node,
        nodes,
        assignment.owner_name.as_deref(),
        assignment.owner_type_name.as_deref(),
        assignment.line,
        owner_name,
        member_name,
        assignment.value_member_through_instance,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "member callable resolution needs the current scope and member context"
)]
fn resolve_direct_member_callable_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    owner_name: &str,
    member_name: &str,
    through_instance: bool,
) -> Option<(Vec<String>, String)> {
    let owner_type_name = if through_instance {
        resolve_direct_callable_return_type(node, nodes, owner_name)
            .map(|return_type| normalize_type_text(&return_type))
            .or_else(|| Some(owner_name.to_owned()))
    } else {
        resolve_direct_name_reference_type(
            node,
            nodes,
            None,
            None,
            current_owner_name,
            current_owner_type_name,
            current_line,
            owner_name,
        )
        .or_else(|| Some(owner_name.to_owned()))
    }?;

    let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
    let method =
        find_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
            matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })?;

    let (actual_params, actual_return) = if let Some(callable_annotation) =
        resolve_decorated_callable_annotation_for_declaration(class_node, nodes, method)
    {
        let (params, return_type) = parse_callable_annotation(&callable_annotation)?;
        (params.unwrap_or_default(), return_type)
    } else {
        let method_signature = rewrite_imported_typing_aliases(
            node,
            &substitute_self_annotation(&method.detail, Some(&owner_type_name)),
        );
        let actual_params = direct_param_types(&method_signature).unwrap_or_default();
        let return_text = rewrite_imported_typing_aliases(
            node,
            &substitute_self_annotation(
                method.detail.split_once("->")?.1.trim(),
                Some(&owner_type_name),
            ),
        );
        let actual_return =
            normalized_direct_return_annotation(&return_text).map(normalize_type_text)?;
        (actual_params, actual_return)
    };
    let bound_params = match method.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
        typepython_syntax::MethodKind::Static => actual_params,
        typepython_syntax::MethodKind::Property => return None,
        typepython_syntax::MethodKind::Instance
        | typepython_syntax::MethodKind::Class
        | typepython_syntax::MethodKind::PropertySetter => {
            actual_params.into_iter().skip(1).collect()
        }
    };
    Some((bound_params, actual_return))
}

fn parse_callable_annotation(text: &str) -> Option<(Option<Vec<String>>, String)> {
    let (params, return_type) = parse_callable_annotation_parts(text)?;
    if params == "..." {
        return Some((None, return_type));
    }
    let params = params.strip_prefix('[').and_then(|inner| inner.strip_suffix(']'))?;
    let param_types = if params.trim().is_empty() {
        Vec::new()
    } else {
        split_top_level_type_args(params).into_iter().map(normalize_type_text).collect()
    };
    Some((Some(param_types), return_type))
}

fn parse_callable_annotation_parts(text: &str) -> Option<(String, String)> {
    let text = normalize_type_text(text);
    let inner = text.strip_prefix("Callable[").and_then(|inner| inner.strip_suffix(']'))?;
    let parts = split_top_level_type_args(inner);
    if parts.len() != 2 {
        return None;
    }
    Some((normalize_callable_param_expr(parts[0]), normalize_type_text(parts[1])))
}

fn normalize_callable_param_expr(params: &str) -> String {
    let params = params.trim();
    if params == "..." || params.is_empty() {
        return params.to_owned();
    }
    if let Some(inner) = params.strip_prefix('[').and_then(|inner| inner.strip_suffix(']')) {
        let rendered = split_top_level_type_args(inner)
            .into_iter()
            .map(normalize_type_text)
            .collect::<Vec<_>>();
        return format!("[{}]", rendered.join(", "));
    }
    if let Some(inner) =
        params.strip_prefix("Concatenate[").and_then(|inner| inner.strip_suffix(']'))
    {
        let rendered = split_top_level_type_args(inner)
            .into_iter()
            .map(normalize_type_text)
            .collect::<Vec<_>>();
        return format!("Concatenate[{}]", rendered.join(", "));
    }
    normalize_type_text(params)
}

#[allow(clippy::too_many_arguments)]
fn resolve_contextual_lambda_callable_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    lambda: &typepython_syntax::LambdaMetadata,
    expected: Option<&str>,
    outer_bindings: Option<&BTreeMap<String, String>>,
) -> Option<(Vec<String>, String)> {
    let expected_params = expected
        .and_then(parse_callable_annotation)
        .and_then(|(expected_params, _)| expected_params);
    if let Some(expected_params) = expected_params.as_ref()
        && expected_params.len() != lambda.params.len()
    {
        return Some((vec![String::from("dynamic"); lambda.params.len()], String::from("dynamic")));
    }
    let param_types = lambda
        .params
        .iter()
        .enumerate()
        .map(|(index, param)| {
            param
                .annotation
                .as_deref()
                .map(normalize_type_text)
                .or_else(|| {
                    expected_params
                        .as_ref()
                        .and_then(|expected_params| expected_params.get(index).cloned())
                })
                .unwrap_or_else(|| String::from("dynamic"))
        })
        .collect::<Vec<_>>();
    let mut local_bindings = outer_bindings.cloned().unwrap_or_default();
    local_bindings.extend(
        lambda.params.iter().map(|param| param.name.clone()).zip(param_types.iter().cloned()),
    );
    let actual_return = resolve_direct_expression_type_from_metadata_with_bindings(
        node,
        nodes,
        None,
        current_owner_name,
        current_owner_type_name,
        current_line,
        &lambda.body,
        &local_bindings,
    )?;
    Some((param_types, actual_return))
}

fn format_callable_annotation(param_types: &[String], return_type: &str) -> String {
    normalize_type_text(&format!("Callable[[{}], {}]", param_types.join(", "), return_type))
}

struct ContextualTypedDictLiteralResult {
    actual_type: String,
    diagnostics: Vec<Diagnostic>,
}

struct ContextualCallArgResult {
    actual_type: String,
    diagnostics: Vec<Diagnostic>,
}

fn resolve_contextual_lambda_callable_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&str>,
) -> Option<String> {
    let lambda = metadata.value_lambda.as_deref()?;
    let (param_types, return_type) = resolve_contextual_lambda_callable_signature(
        node,
        nodes,
        None,
        None,
        current_line,
        lambda,
        expected,
        None,
    )?;
    Some(format_callable_annotation(&param_types, &return_type))
}

fn resolve_contextual_typed_dict_literal_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&str>,
) -> Option<ContextualTypedDictLiteralResult> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    resolve_contextual_typed_dict_literal_type_with_context(
        &context,
        node,
        nodes,
        current_line,
        metadata,
        expected,
    )
}

fn resolve_contextual_typed_dict_literal_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&str>,
) -> Option<ContextualTypedDictLiteralResult> {
    let entries = metadata.value_dict_entries.as_ref()?;
    let expected = expected?;
    let actual_type = normalize_type_text(expected);
    let target_shape =
        resolve_known_typed_dict_shape_from_type_with_context(context, node, nodes, &actual_type)?;
    let diagnostics = typed_dict_literal_entry_diagnostics(
        node,
        nodes,
        current_line,
        entries,
        &target_shape,
        None,
        None,
        None,
    );
    Some(ContextualTypedDictLiteralResult { actual_type, diagnostics })
}

fn resolve_contextual_collection_literal_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&str>,
) -> Option<ContextualCallArgResult> {
    let expected = normalize_type_text(expected?);
    let (head, args) = split_generic_type(&expected)?;
    match head {
        "list" if args.len() == 1 => {
            let elements = metadata.value_list_elements.as_ref()?;
            let diagnostics = elements
                .iter()
                .flat_map(|element| {
                    resolve_contextual_call_arg_type(
                        node,
                        nodes,
                        current_line,
                        element,
                        Some(&args[0]),
                    )
                    .into_iter()
                    .flat_map(|result| result.diagnostics)
                })
                .collect::<Vec<_>>();
            let actual_element_types = if elements.is_empty() {
                vec![args[0].clone()]
            } else {
                elements
                    .iter()
                    .map(|element| {
                        resolve_contextual_call_arg_type(
                            node,
                            nodes,
                            current_line,
                            element,
                            Some(&args[0]),
                        )
                        .map(|result| result.actual_type)
                        .or_else(|| {
                            resolve_direct_expression_type_from_metadata(
                                node,
                                nodes,
                                None,
                                None,
                                None,
                                current_line,
                                element,
                            )
                        })
                        .unwrap_or_else(|| String::from("Any"))
                    })
                    .collect::<Vec<_>>()
            };
            Some(ContextualCallArgResult {
                actual_type: format!("list[{}]", join_type_candidates(actual_element_types)),
                diagnostics,
            })
        }
        "set" if args.len() == 1 => {
            let elements = metadata.value_set_elements.as_ref()?;
            let diagnostics = elements
                .iter()
                .flat_map(|element| {
                    resolve_contextual_call_arg_type(
                        node,
                        nodes,
                        current_line,
                        element,
                        Some(&args[0]),
                    )
                    .into_iter()
                    .flat_map(|result| result.diagnostics)
                })
                .collect::<Vec<_>>();
            let actual_element_types = if elements.is_empty() {
                vec![args[0].clone()]
            } else {
                elements
                    .iter()
                    .map(|element| {
                        resolve_contextual_call_arg_type(
                            node,
                            nodes,
                            current_line,
                            element,
                            Some(&args[0]),
                        )
                        .map(|result| result.actual_type)
                        .or_else(|| {
                            resolve_direct_expression_type_from_metadata(
                                node,
                                nodes,
                                None,
                                None,
                                None,
                                current_line,
                                element,
                            )
                        })
                        .unwrap_or_else(|| String::from("Any"))
                    })
                    .collect::<Vec<_>>()
            };
            Some(ContextualCallArgResult {
                actual_type: format!("set[{}]", join_type_candidates(actual_element_types)),
                diagnostics,
            })
        }
        "dict" if args.len() == 2 => {
            let entries = metadata.value_dict_entries.as_ref()?;
            if entries.iter().any(|entry| entry.is_expansion) {
                return None;
            }
            let diagnostics = entries
                .iter()
                .flat_map(|entry| {
                    let key_diagnostics = entry
                        .key_value
                        .as_deref()
                        .and_then(|key| {
                            resolve_contextual_call_arg_type(
                                node,
                                nodes,
                                current_line,
                                key,
                                Some(&args[0]),
                            )
                        })
                        .into_iter()
                        .flat_map(|result| result.diagnostics);
                    let value_diagnostics = resolve_contextual_call_arg_type(
                        node,
                        nodes,
                        current_line,
                        &entry.value,
                        Some(&args[1]),
                    )
                    .into_iter()
                    .flat_map(|result| result.diagnostics);
                    key_diagnostics.chain(value_diagnostics)
                })
                .collect::<Vec<_>>();
            let actual_key_types = if entries.is_empty() {
                vec![args[0].clone()]
            } else {
                entries
                    .iter()
                    .map(|entry| {
                        entry
                            .key_value
                            .as_deref()
                            .and_then(|key| {
                                resolve_contextual_call_arg_type(
                                    node,
                                    nodes,
                                    current_line,
                                    key,
                                    Some(&args[0]),
                                )
                            })
                            .map(|result| result.actual_type)
                            .or_else(|| {
                                entry.key_value.as_deref().and_then(|key| {
                                    resolve_direct_expression_type_from_metadata(
                                        node,
                                        nodes,
                                        None,
                                        None,
                                        None,
                                        current_line,
                                        key,
                                    )
                                })
                            })
                            .unwrap_or_else(|| String::from("Any"))
                    })
                    .collect::<Vec<_>>()
            };
            let actual_value_types = if entries.is_empty() {
                vec![args[1].clone()]
            } else {
                entries
                    .iter()
                    .map(|entry| {
                        resolve_contextual_call_arg_type(
                            node,
                            nodes,
                            current_line,
                            &entry.value,
                            Some(&args[1]),
                        )
                        .map(|result| result.actual_type)
                        .or_else(|| {
                            resolve_direct_expression_type_from_metadata(
                                node,
                                nodes,
                                None,
                                None,
                                None,
                                current_line,
                                &entry.value,
                            )
                        })
                        .unwrap_or_else(|| String::from("Any"))
                    })
                    .collect::<Vec<_>>()
            };
            Some(ContextualCallArgResult {
                actual_type: format!(
                    "dict[{}, {}]",
                    join_type_candidates(actual_key_types),
                    join_type_candidates(actual_value_types)
                ),
                diagnostics,
            })
        }
        _ => None,
    }
}

fn resolve_contextual_call_arg_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&str>,
) -> Option<ContextualCallArgResult> {
    if let Some(actual_type) =
        resolve_contextual_lambda_callable_type(node, nodes, current_line, metadata, expected)
    {
        return Some(ContextualCallArgResult { actual_type, diagnostics: Vec::new() });
    }
    if let Some(actual_type) =
        resolve_contextual_named_callable_type(node, nodes, metadata, expected)
    {
        return Some(ContextualCallArgResult { actual_type, diagnostics: Vec::new() });
    }
    if let Some(result) =
        resolve_contextual_typed_dict_literal_type(node, nodes, current_line, metadata, expected)
    {
        return Some(ContextualCallArgResult {
            actual_type: result.actual_type,
            diagnostics: result.diagnostics,
        });
    }
    resolve_contextual_collection_literal_type(node, nodes, current_line, metadata, expected)
}

fn resolve_contextual_named_callable_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    metadata: &typepython_syntax::DirectExprMetadata,
    expected: Option<&str>,
) -> Option<String> {
    parse_callable_annotation_parts(expected?)?;
    let function_name = metadata.value_name.as_deref()?;
    let function = resolve_direct_function(node, nodes, function_name)?;
    let param_types = direct_signature_sites_from_detail(&function.detail)
        .into_iter()
        .map(|param| param.annotation.unwrap_or_else(|| String::from("dynamic")))
        .collect::<Vec<_>>();
    let return_type = function.detail.split_once("->")?.1.trim();
    Some(format_callable_annotation(&param_types, return_type))
}

fn expected_positional_arg_types_from_direct_signature(
    params: &[DirectSignatureParam],
    arg_count: usize,
) -> Vec<Option<String>> {
    let positional_params = params
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .collect::<Vec<_>>();
    let variadic_type = params
        .iter()
        .find(|param| param.variadic)
        .and_then(|param| (!param.annotation.is_empty()).then(|| param.annotation.clone()));

    (0..arg_count)
        .map(|index| {
            positional_params
                .get(index)
                .and_then(|param| (!param.annotation.is_empty()).then(|| param.annotation.clone()))
                .or_else(|| variadic_type.clone())
        })
        .collect()
}

fn expected_keyword_arg_types_from_direct_signature(
    params: &[DirectSignatureParam],
    keyword_names: &[String],
) -> Vec<Option<String>> {
    let keyword_variadic_type = params
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| (!param.annotation.is_empty()).then(|| param.annotation.clone()));

    keyword_names
        .iter()
        .map(|keyword| {
            params
                .iter()
                .find(|param| param.name == *keyword && !param.positional_only)
                .and_then(|param| (!param.annotation.is_empty()).then(|| param.annotation.clone()))
                .or_else(|| keyword_variadic_type.clone())
        })
        .collect()
}

fn expected_positional_arg_types_from_signature_sites(
    signature: &[typepython_syntax::DirectFunctionParamSite],
    arg_count: usize,
) -> Vec<Option<String>> {
    let positional_params = signature
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .collect::<Vec<_>>();
    let variadic_type =
        signature.iter().find(|param| param.variadic).and_then(|param| param.annotation.clone());

    (0..arg_count)
        .map(|index| {
            positional_params
                .get(index)
                .and_then(|param| param.annotation.clone())
                .or_else(|| variadic_type.clone())
        })
        .collect()
}

fn expected_keyword_arg_types_from_signature_sites(
    signature: &[typepython_syntax::DirectFunctionParamSite],
    keyword_names: &[String],
) -> Vec<Option<String>> {
    let keyword_variadic_type = signature
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| param.annotation.clone());

    keyword_names
        .iter()
        .map(|keyword| {
            signature
                .iter()
                .find(|param| param.name == *keyword && !param.positional_only)
                .and_then(|param| param.annotation.clone())
                .or_else(|| keyword_variadic_type.clone())
        })
        .collect()
}

fn resolve_assignment_owner_signature<'a>(
    node: &'a typepython_graph::ModuleNode,
    assignment: &typepython_binding::AssignmentSite,
) -> Option<&'a str> {
    resolve_scope_owner_signature(
        node,
        assignment.owner_name.as_deref(),
        assignment.owner_type_name.as_deref(),
    )
}

fn resolve_scope_owner_signature<'a>(
    node: &'a typepython_graph::ModuleNode,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<&'a str> {
    resolve_scope_owner_declaration(node, owner_name, owner_type_name)
        .map(|declaration| declaration.detail.as_str())
}

fn resolve_scope_owner_declaration<'a>(
    node: &'a typepython_graph::ModuleNode,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<&'a Declaration> {
    let owner_name = owner_name?;
    node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Function
            && declaration.name == owner_name
            && match (owner_type_name, &declaration.owner) {
                (Some(owner_type_name), Some(owner)) => owner.name == *owner_type_name,
                (None, None) => true,
                _ => false,
            }
    })
}

fn resolve_for_owner_signature<'a>(
    node: &'a typepython_graph::ModuleNode,
    for_loop: &typepython_binding::ForSite,
) -> Option<&'a str> {
    let owner_name = for_loop.owner_name.as_deref()?;
    node.declarations
        .iter()
        .find(|declaration| {
            declaration.kind == DeclarationKind::Function
                && declaration.name == owner_name
                && match (&for_loop.owner_type_name, &declaration.owner) {
                    (Some(owner_type_name), Some(owner)) => owner.name == *owner_type_name,
                    (None, None) => true,
                    _ => false,
                }
        })
        .map(|declaration| declaration.detail.as_str())
}

fn normalized_direct_return_annotation(annotation: &str) -> Option<&str> {
    let annotation = annotation.trim();
    (!annotation.is_empty()).then_some(annotation)
}

fn substitute_self_annotation(text: &str, owner_type_name: Option<&str>) -> String {
    let Some(owner_type_name) = owner_type_name else {
        return text.trim().to_owned();
    };

    let mut output = String::new();
    let mut token = String::new();
    for character in text.trim().chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            token.push(character);
            continue;
        }
        if !token.is_empty() {
            if token == "Self" {
                output.push_str(owner_type_name);
            } else {
                output.push_str(&token);
            }
            token.clear();
        }
        output.push(character);
    }
    if !token.is_empty() {
        if token == "Self" {
            output.push_str(owner_type_name);
        } else {
            output.push_str(&token);
        }
    }
    output
}

fn rewrite_imported_typing_aliases(node: &typepython_graph::ModuleNode, text: &str) -> String {
    let mut output = String::new();
    let mut token = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            token.push(character);
            continue;
        }
        if !token.is_empty() {
            output.push_str(&rewrite_imported_typing_token(node, &token));
            token.clear();
        }
        output.push(character);
    }
    if !token.is_empty() {
        output.push_str(&rewrite_imported_typing_token(node, &token));
    }
    output
}

fn rewrite_imported_typing_token(node: &typepython_graph::ModuleNode, token: &str) -> String {
    let Some(import_decl) = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == token
    }) else {
        return token.to_owned();
    };

    let Some((module_name, symbol_name)) = import_decl.detail.rsplit_once('.') else {
        return token.to_owned();
    };
    if matches!(module_name, "typing" | "typing_extensions" | "collections.abc")
        && matches!(
            symbol_name,
            "Annotated"
                | "Any"
                | "Awaitable"
                | "Callable"
                | "ClassVar"
                | "Concatenate"
                | "Coroutine"
                | "Final"
                | "Generator"
                | "Literal"
                | "NewType"
                | "NotRequired"
                | "Optional"
                | "ParamSpec"
                | "Protocol"
                | "ReadOnly"
                | "Required"
                | "Sequence"
                | "TypeGuard"
                | "TypeIs"
                | "TypeVar"
                | "TypeVarTuple"
                | "TypedDict"
                | "Union"
                | "Unpack"
        )
    {
        return symbol_name.to_owned();
    }

    token.to_owned()
}

fn normalized_assignment_annotation(annotation: &str) -> Option<&str> {
    let annotation = annotation.trim();
    if annotation.is_empty() {
        return None;
    }
    if let Some(inner) = annotation.strip_prefix("Final[").and_then(|inner| inner.strip_suffix(']'))
    {
        return normalized_assignment_annotation(inner);
    }
    if let Some(inner) =
        annotation.strip_prefix("ClassVar[").and_then(|inner| inner.strip_suffix(']'))
    {
        return normalized_assignment_annotation(inner);
    }
    match annotation {
        "Final" | "ClassVar" => None,
        _ => Some(annotation),
    }
}

fn normalize_type_text(text: &str) -> String {
    let text = text.trim();
    let text = text.strip_prefix("typing.").unwrap_or(text);

    if let Some(open_index) = text.find('[') {
        if let Some(inner) = text.strip_suffix(']') {
            let head = normalize_type_head(&inner[..open_index]);
            let args = split_top_level_type_args(&inner[open_index + 1..])
                .into_iter()
                .map(normalize_type_text)
                .collect::<Vec<_>>()
                .join(", ");
            return format!("{head}[{args}]");
        }
    }

    normalize_type_head(text).to_owned()
}

fn expand_type_alias_once(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    text: &str,
) -> Option<String> {
    let normalized = normalize_type_text(text);
    let (head, args) = split_generic_type(&normalized)
        .map(|(head, args)| (head.to_owned(), args))
        .unwrap_or_else(|| (normalized.clone(), Vec::new()));
    let (alias_node, alias_decl) = resolve_direct_type_alias(nodes, node, &head)?;
    let substitutions = alias_type_param_substitutions(alias_decl, &args)?;
    let detail = rewrite_imported_typing_aliases(alias_node, &alias_decl.detail);
    let expanded = if substitutions.is_empty() {
        detail
    } else {
        substitute_type_substitutions(&detail, &substitutions)
    };
    let expanded = normalize_type_text(&expanded);
    (expanded != normalized).then_some(expanded)
}

fn alias_type_param_substitutions(
    alias_decl: &Declaration,
    args: &[String],
) -> Option<BTreeMap<String, String>> {
    if args.len() > alias_decl.type_params.len() {
        return None;
    }

    let mut substitutions = BTreeMap::new();
    for (index, type_param) in alias_decl.type_params.iter().enumerate() {
        let argument = args
            .get(index)
            .cloned()
            .or_else(|| type_param.default.as_ref().map(|default| normalize_type_text(default)))?;
        substitutions.insert(type_param.name.clone(), argument);
    }
    Some(substitutions)
}

fn direct_type_matches(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    let expected = normalize_type_text(expected);
    let actual = normalize_type_text(actual);
    let mut visiting = BTreeSet::new();

    direct_type_matches_normalized(node, nodes, &expected, &actual, &mut visiting)
}

fn direct_type_is_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    let expected = normalize_type_text(expected);
    let actual = normalize_type_text(actual);
    let mut visiting = BTreeSet::new();
    direct_type_is_assignable_normalized(node, nodes, &expected, &actual, &mut visiting)
}

fn direct_type_matches_normalized(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
    visiting: &mut BTreeSet<(String, String)>,
) -> bool {
    if let Some(inner) = annotated_inner(expected) {
        return direct_type_matches_normalized(node, nodes, &inner, actual, visiting);
    }
    if let Some(inner) = annotated_inner(actual) {
        return direct_type_matches_normalized(node, nodes, expected, &inner, visiting);
    }

    if expected == actual || expected == "Any" || actual == "Any" {
        return true;
    }

    let key = (expected.to_owned(), actual.to_owned());
    if !visiting.insert(key.clone()) {
        return true;
    }

    let result = if let Some(expanded_expected) = expand_type_alias_once(node, nodes, expected) {
        direct_type_matches_normalized(node, nodes, &expanded_expected, actual, visiting)
    } else if let Some(expanded_actual) = expand_type_alias_once(node, nodes, actual) {
        direct_type_matches_normalized(node, nodes, expected, &expanded_actual, visiting)
    } else if let Some(branches) = union_branches(expected) {
        if let Some(actual_branches) = union_branches(actual) {
            actual_branches.iter().all(|actual_branch| {
                branches.iter().any(|expected_branch| {
                    direct_type_matches_normalized(
                        node,
                        nodes,
                        expected_branch,
                        actual_branch,
                        visiting,
                    )
                })
            }) && branches.iter().all(|expected_branch| {
                actual_branches.iter().any(|actual_branch| {
                    direct_type_matches_normalized(
                        node,
                        nodes,
                        expected_branch,
                        actual_branch,
                        visiting,
                    )
                })
            })
        } else {
            branches.into_iter().any(|branch| {
                direct_type_matches_normalized(node, nodes, &branch, actual, visiting)
            })
        }
    } else if enum_member_owner_name(actual).is_some_and(|owner| owner == expected) {
        true
    } else {
        match (split_generic_type(expected), split_generic_type(actual)) {
            (Some((expected_head, expected_args)), Some((actual_head, actual_args)))
                if expected_head == actual_head && expected_args.len() == actual_args.len() =>
            {
                expected_args.iter().zip(actual_args.iter()).all(|(expected_arg, actual_arg)| {
                    direct_type_matches_normalized(node, nodes, expected_arg, actual_arg, visiting)
                })
            }
            _ => false,
        }
    };

    visiting.remove(&key);
    result
}

fn direct_type_matches_normalized_plain(expected: &str, actual: &str) -> bool {
    if let Some(inner) = annotated_inner(expected) {
        return direct_type_matches_normalized_plain(&inner, actual);
    }
    if let Some(inner) = annotated_inner(actual) {
        return direct_type_matches_normalized_plain(expected, &inner);
    }

    if expected == actual || expected == "Any" || actual == "Any" {
        return true;
    }

    if let Some(branches) = union_branches(expected) {
        if let Some(actual_branches) = union_branches(actual) {
            return actual_branches.iter().all(|actual_branch| {
                branches.iter().any(|expected_branch| {
                    direct_type_matches_normalized_plain(expected_branch, actual_branch)
                })
            }) && branches.iter().all(|expected_branch| {
                actual_branches.iter().any(|actual_branch| {
                    direct_type_matches_normalized_plain(expected_branch, actual_branch)
                })
            });
        }
        return branches
            .into_iter()
            .any(|branch| direct_type_matches_normalized_plain(&branch, actual));
    }

    if enum_member_owner_name(actual).is_some_and(|owner| owner == expected) {
        return true;
    }

    match (split_generic_type(expected), split_generic_type(actual)) {
        (Some((expected_head, expected_args)), Some((actual_head, actual_args)))
            if expected_head == actual_head && expected_args.len() == actual_args.len() =>
        {
            expected_args.iter().zip(actual_args.iter()).all(|(expected_arg, actual_arg)| {
                direct_type_matches_normalized_plain(expected_arg, actual_arg)
            })
        }
        _ => false,
    }
}

fn direct_type_is_assignable_normalized(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
    visiting: &mut BTreeSet<(String, String)>,
) -> bool {
    if let Some(inner) = annotated_inner(expected) {
        return direct_type_is_assignable_normalized(node, nodes, &inner, actual, visiting);
    }
    if let Some(inner) = annotated_inner(actual) {
        return direct_type_is_assignable_normalized(node, nodes, expected, &inner, visiting);
    }

    if expected == actual
        || expected == "Any"
        || expected == "unknown"
        || expected == "dynamic"
        || actual == "Any"
        || actual == "unknown"
        || actual == "dynamic"
    {
        return true;
    }

    let key = (expected.to_owned(), actual.to_owned());
    if !visiting.insert(key.clone()) {
        return true;
    }

    let result = if let Some(expanded_expected) = expand_type_alias_once(node, nodes, expected) {
        direct_type_is_assignable_normalized(node, nodes, &expanded_expected, actual, visiting)
    } else if let Some(expanded_actual) = expand_type_alias_once(node, nodes, actual) {
        direct_type_is_assignable_normalized(node, nodes, expected, &expanded_actual, visiting)
    } else if let Some(branches) = union_branches(expected) {
        if let Some(actual_branches) = union_branches(actual) {
            actual_branches.iter().all(|actual_branch| {
                branches.iter().any(|expected_branch| {
                    direct_type_is_assignable_normalized(
                        node,
                        nodes,
                        expected_branch,
                        actual_branch,
                        visiting,
                    )
                })
            })
        } else {
            branches.into_iter().any(|branch| {
                direct_type_is_assignable_normalized(node, nodes, &branch, actual, visiting)
            })
        }
    } else if enum_member_owner_name(actual).is_some_and(|owner| owner == expected)
        || protocol_assignable(node, nodes, expected, actual)
        || nominal_subclass_assignable(node, nodes, expected, actual)
    {
        true
    } else if let Some(result) = assignable_generic_bridge(node, nodes, expected, actual) {
        result
    } else {
        direct_type_matches(node, nodes, expected, actual)
    };

    visiting.remove(&key);
    result
}

fn nominal_subclass_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    if expected == actual {
        return true;
    }
    let Some((actual_node, actual_decl)) = resolve_direct_base(nodes, node, actual) else {
        return false;
    };
    actual_decl.bases.iter().any(|base| {
        normalize_type_text(base) == expected
            || direct_type_is_assignable(actual_node, nodes, expected, base)
    })
}

fn protocol_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    let Some((interface_node, interface_decl)) = resolve_direct_base(nodes, node, expected) else {
        return false;
    };
    if !is_interface_like_declaration(interface_node, interface_decl, nodes) {
        return false;
    }
    let Some((actual_node, actual_decl)) = resolve_direct_base(nodes, node, actual) else {
        return false;
    };
    type_satisfies_interface(nodes, actual_node, actual_decl, interface_node, interface_decl)
}

fn type_satisfies_interface(
    nodes: &[typepython_graph::ModuleNode],
    actual_node: &typepython_graph::ModuleNode,
    actual_decl: &Declaration,
    interface_node: &typepython_graph::ModuleNode,
    interface_decl: &Declaration,
) -> bool {
    collect_interface_members(interface_node, interface_decl, nodes).into_iter().all(|required| {
        actual_member_satisfies_requirement(nodes, actual_node, actual_decl, &required)
    })
}

#[derive(Debug, Clone)]
struct InterfaceMemberRequirement {
    name: String,
    declaration: Declaration,
}

fn collect_interface_members(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<InterfaceMemberRequirement> {
    let mut visited = BTreeSet::new();
    let mut requirements = BTreeMap::new();
    collect_interface_members_with_visited(
        node,
        declaration,
        nodes,
        &mut visited,
        &mut requirements,
    );
    requirements.into_values().collect()
}

fn collect_interface_members_with_visited(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    nodes: &[typepython_graph::ModuleNode],
    visited: &mut BTreeSet<(String, String)>,
    requirements: &mut BTreeMap<String, InterfaceMemberRequirement>,
) {
    let key = (node.module_key.clone(), declaration.name.clone());
    if !visited.insert(key) {
        return;
    }

    for member in node.declarations.iter().filter(|candidate| {
        candidate.owner.as_ref().is_some_and(|owner| owner.name == declaration.name)
            && matches!(candidate.kind, DeclarationKind::Value | DeclarationKind::Function)
    }) {
        requirements.entry(member.name.clone()).or_insert_with(|| InterfaceMemberRequirement {
            name: member.name.clone(),
            declaration: member.clone(),
        });
    }

    for base in &declaration.bases {
        if let Some((base_node, base_decl)) = resolve_direct_base(nodes, node, base)
            && is_interface_like_declaration(base_node, base_decl, nodes)
        {
            collect_interface_members_with_visited(
                base_node,
                base_decl,
                nodes,
                visited,
                requirements,
            );
        }
    }
}

fn actual_member_satisfies_requirement(
    nodes: &[typepython_graph::ModuleNode],
    actual_node: &typepython_graph::ModuleNode,
    actual_decl: &Declaration,
    requirement: &InterfaceMemberRequirement,
) -> bool {
    match requirement.declaration.kind {
        DeclarationKind::Function => {
            find_apparent_callable_declaration(nodes, actual_node, actual_decl, &requirement.name)
                .is_some_and(|member| {
                    methods_are_compatible_for_override(
                        actual_node,
                        nodes,
                        member,
                        &requirement.declaration,
                    )
                })
        }
        DeclarationKind::Value => {
            find_apparent_value_declaration(nodes, actual_node, actual_decl, &requirement.name)
                .is_some_and(|member| {
                    let expected = normalize_type_text(requirement.declaration.detail.as_str());
                    let actual = normalize_type_text(member.detail.as_str());
                    expected.is_empty()
                        || actual.is_empty()
                        || direct_type_is_assignable(actual_node, nodes, &expected, &actual)
                })
        }
        _ => false,
    }
}

fn find_apparent_value_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<&'a Declaration> {
    find_apparent_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
        declaration.kind == DeclarationKind::Value
    })
}

fn find_apparent_callable_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<&'a Declaration> {
    find_apparent_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
        declaration.kind == DeclarationKind::Function
    })
}

fn find_apparent_member_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
    predicate: impl Fn(&Declaration) -> bool + Copy,
) -> Option<&'a Declaration> {
    let mut visited = BTreeSet::new();
    find_apparent_member_declaration_with_visited(
        nodes,
        class_node,
        class_decl,
        member_name,
        predicate,
        &mut visited,
    )
}

fn find_apparent_member_declaration_with_visited<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
    predicate: impl Fn(&Declaration) -> bool + Copy,
    visited: &mut BTreeSet<(String, String)>,
) -> Option<&'a Declaration> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visited.insert(key) {
        return None;
    }

    if let Some(local) = class_node.declarations.iter().find(|declaration| {
        declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration.name == member_name
            && predicate(declaration)
    }) {
        return Some(local);
    }

    for base in &class_decl.bases {
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) else {
            continue;
        };
        if is_interface_like_declaration(base_node, base_decl, nodes) {
            continue;
        }
        if let Some(inherited) = find_apparent_member_declaration_with_visited(
            nodes,
            base_node,
            base_decl,
            member_name,
            predicate,
            visited,
        ) {
            return Some(inherited);
        }
    }

    None
}

fn assignable_generic_bridge(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> Option<bool> {
    let (expected_head, expected_args) = split_generic_type(expected)?;
    let (actual_head, actual_args) = split_generic_type(actual)?;

    if expected_head == actual_head && expected_args.len() == actual_args.len() {
        return same_head_generic_assignable(
            node,
            nodes,
            expected_head,
            &expected_args,
            &actual_args,
        );
    }

    match (expected_head, actual_head) {
        ("Sequence", "list") | ("Sequence", "tuple") if !expected_args.is_empty() => {
            if actual_head == "tuple" && actual_args.len() == 2 && actual_args[1] == "..." {
                return Some(direct_type_is_assignable(
                    node,
                    nodes,
                    &expected_args[0],
                    &actual_args[0],
                ));
            }
            let element = if actual_head == "tuple" {
                join_branch_types(actual_args)
            } else {
                actual_args.first().cloned().unwrap_or_default()
            };
            return Some(direct_type_is_assignable(node, nodes, &expected_args[0], &element));
        }
        ("Mapping", "dict") if expected_args.len() == 2 && actual_args.len() == 2 => {
            return Some(
                invariant_type_matches(node, nodes, &expected_args[0], &actual_args[0])
                    && direct_type_is_assignable(node, nodes, &expected_args[1], &actual_args[1]),
            );
        }
        _ => {}
    }

    None
}

#[derive(Clone, Copy)]
enum GenericVariance {
    Invariant,
    Covariant,
    Contravariant,
}

fn same_head_generic_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    head: &str,
    expected_args: &[String],
    actual_args: &[String],
) -> Option<bool> {
    if head == "Callable" {
        return callable_annotation_assignable(node, nodes, expected_args, actual_args);
    }

    let variances = variances_for_generic_head(head, expected_args.len());
    Some(expected_args.iter().zip(actual_args.iter()).zip(variances).all(
        |((expected_arg, actual_arg), variance)| match variance {
            GenericVariance::Invariant => {
                invariant_type_matches(node, nodes, expected_arg, actual_arg)
            }
            GenericVariance::Covariant => {
                direct_type_is_assignable(node, nodes, expected_arg, actual_arg)
            }
            GenericVariance::Contravariant => {
                direct_type_is_assignable(node, nodes, actual_arg, expected_arg)
            }
        },
    ))
}

fn callable_annotation_assignable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_args: &[String],
    actual_args: &[String],
) -> Option<bool> {
    let expected = format!("Callable[{}]", expected_args.join(", "));
    let actual = format!("Callable[{}]", actual_args.join(", "));
    let (expected_params, expected_return) = parse_callable_annotation(&expected)?;
    let (actual_params, actual_return) = parse_callable_annotation(&actual)?;

    if !direct_type_is_assignable(node, nodes, &expected_return, &actual_return) {
        return Some(false);
    }

    match (expected_params, actual_params) {
        (None, _) | (_, None) => Some(true),
        (Some(expected_params), Some(actual_params)) => {
            if expected_params.len() != actual_params.len() {
                return Some(false);
            }
            Some(expected_params.iter().zip(actual_params.iter()).all(
                |(expected_param, actual_param)| {
                    direct_type_is_assignable(node, nodes, actual_param, expected_param)
                },
            ))
        }
    }
}

fn invariant_type_matches(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> bool {
    (direct_type_matches(node, nodes, expected, actual)
        && direct_type_matches(node, nodes, actual, expected))
        || recursive_type_alias_head(node, nodes, expected)
            .is_some_and(|_| direct_type_is_assignable(node, nodes, expected, actual))
}

fn recursive_type_alias_head(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    text: &str,
) -> Option<String> {
    let normalized = normalize_type_text(text);
    let head =
        split_generic_type(&normalized).map(|(head, _)| head.to_owned()).unwrap_or(normalized);
    let (alias_node, alias_decl) = resolve_direct_type_alias(nodes, node, &head)?;
    let mut visiting = BTreeSet::new();
    type_alias_eventually_mentions(
        alias_node,
        nodes,
        alias_decl.name.as_str(),
        &head,
        &mut visiting,
    )
    .then_some(head)
}

fn type_alias_eventually_mentions(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current: &str,
    target: &str,
    visiting: &mut BTreeSet<(String, String)>,
) -> bool {
    let Some((alias_node, alias_decl)) = resolve_direct_type_alias(nodes, node, current) else {
        return false;
    };
    let key = (alias_node.module_key.clone(), alias_decl.name.clone());
    if !visiting.insert(key.clone()) {
        return alias_decl.name == target;
    }

    let result =
        type_expr_mentions_alias(alias_node, nodes, alias_decl.detail.as_str(), target, visiting);
    visiting.remove(&key);
    result
}

fn type_expr_mentions_alias(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    text: &str,
    target: &str,
    visiting: &mut BTreeSet<(String, String)>,
) -> bool {
    let normalized = normalize_type_text(text);

    if let Some(inner) = annotated_inner(&normalized) {
        return type_expr_mentions_alias(node, nodes, &inner, target, visiting);
    }
    if let Some(branches) = union_branches(&normalized) {
        return branches
            .into_iter()
            .any(|branch| type_expr_mentions_alias(node, nodes, &branch, target, visiting));
    }
    if let Some((head, args)) = split_generic_type(&normalized) {
        return head == target
            || type_alias_eventually_mentions(node, nodes, head, target, visiting)
            || args.iter().any(|arg| type_expr_mentions_alias(node, nodes, arg, target, visiting));
    }

    normalized == target
        || type_alias_eventually_mentions(node, nodes, &normalized, target, visiting)
}

fn variances_for_generic_head(head: &str, arity: usize) -> Vec<GenericVariance> {
    match head {
        "Sequence" | "Iterable" | "Iterator" | "Reversible" | "Collection" | "AbstractSet"
        | "frozenset" | "tuple" | "type" => vec![GenericVariance::Covariant; arity],
        "Mapping" if arity == 2 => {
            vec![GenericVariance::Invariant, GenericVariance::Covariant]
        }
        "Generator" if arity == 3 => vec![
            GenericVariance::Covariant,
            GenericVariance::Contravariant,
            GenericVariance::Covariant,
        ],
        _ => vec![GenericVariance::Invariant; arity],
    }
}

fn enum_member_owner_name(text: &str) -> Option<String> {
    let inner = text.strip_prefix("Literal[")?.strip_suffix(']')?;
    let (owner, _member) = inner.rsplit_once('.')?;
    Some(normalize_type_text(owner))
}

fn union_branches(text: &str) -> Option<Vec<String>> {
    let text = text.trim();
    if let Some(inner) = annotated_inner(text) {
        return union_branches(&inner).or(Some(vec![inner]));
    }
    if let Some(inner) = text.strip_prefix("Optional[").and_then(|inner| inner.strip_suffix(']')) {
        return Some(vec![normalize_type_text(inner), String::from("None")]);
    }
    if let Some(inner) = text.strip_prefix("Union[").and_then(|inner| inner.strip_suffix(']')) {
        return Some(
            split_top_level_type_args(inner).into_iter().map(normalize_type_text).collect(),
        );
    }
    let pipe_branches = split_top_level_union_branches(text);
    if pipe_branches.len() > 1 {
        return Some(pipe_branches.into_iter().map(normalize_type_text).collect());
    }
    None
}

fn split_top_level_union_branches(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                parts.push(text[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim());
    parts
}

fn annotated_inner(text: &str) -> Option<String> {
    let text = text.trim();
    let inner = text.strip_prefix("Annotated[").and_then(|inner| inner.strip_suffix(']'))?;
    let mut args = split_top_level_type_args(inner).into_iter();
    let first = args.next()?;
    Some(normalize_type_text(first))
}

fn split_generic_type(text: &str) -> Option<(&str, Vec<String>)> {
    let text = text.trim();
    let open_index = text.find('[')?;
    let inner = text.strip_suffix(']')?;
    let head = &inner[..open_index];
    let args = split_top_level_type_args(&inner[open_index + 1..])
        .into_iter()
        .map(normalize_type_text)
        .collect::<Vec<_>>();
    Some((head, args))
}

fn normalize_type_head(head: &str) -> &str {
    match head.trim() {
        "List" => "list",
        "Dict" => "dict",
        "Tuple" => "tuple",
        "Set" => "set",
        "FrozenSet" => "frozenset",
        "Type" => "type",
        "Callable" => "Callable",
        "Literal" => "Literal",
        "NewType" => "NewType",
        other => other,
    }
}

fn split_top_level_type_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;

    for (index, ch) in args.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(args[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }

    let tail = args[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }

    parts
}

fn resolve_builtin_return_type(callee: &str) -> Option<&'static str> {
    BUILTIN_FUNCTION_RETURN_TYPES
        .iter()
        .find_map(|(name, return_type)| (*name == callee).then_some(*return_type))
}

fn resolve_typing_callable_signature(callee: &str) -> Option<&'static str> {
    TYPING_SYNTHETIC_CALLABLE_SIGNATURES
        .iter()
        .find_map(|(name, signature)| (*name == callee).then_some(*signature))
}

#[expect(
    clippy::too_many_arguments,
    reason = "direct expression resolution is driven by parsed expression metadata fields"
)]
fn resolve_direct_expression_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_type: Option<&str>,
    is_awaited: bool,
    value_callee: Option<&str>,
    value_name: Option<&str>,
    value_member_owner_name: Option<&str>,
    value_member_name: Option<&str>,
    value_member_through_instance: bool,
    value_method_owner_name: Option<&str>,
    value_method_name: Option<&str>,
    value_method_through_instance: bool,
    value_subscript_target: Option<&typepython_syntax::DirectExprMetadata>,
    value_subscript_string_key: Option<&str>,
    value_subscript_index: Option<&str>,
    value_if_true: Option<&typepython_syntax::DirectExprMetadata>,
    value_if_false: Option<&typepython_syntax::DirectExprMetadata>,
    value_if_guard: Option<&typepython_binding::GuardConditionSite>,
    value_bool_left: Option<&typepython_syntax::DirectExprMetadata>,
    value_bool_right: Option<&typepython_syntax::DirectExprMetadata>,
    value_binop_left: Option<&typepython_syntax::DirectExprMetadata>,
    value_binop_right: Option<&typepython_syntax::DirectExprMetadata>,
    value_binop_operator: Option<&str>,
) -> Option<String> {
    let resolved = value_type
        .filter(|value_type| !value_type.is_empty())
        .map(str::trim)
        .map(normalize_type_text)
        .or_else(|| {
            value_callee
                .and_then(|callee| {
                    resolve_direct_callable_return_type_for_line(node, nodes, callee, current_line)
                        .or_else(|| resolve_direct_callable_return_type(node, nodes, callee))
                })
                .map(|return_type| normalize_type_text(&return_type))
        })
        .or_else(|| {
            value_name.and_then(|value_name| {
                resolve_direct_name_reference_type(
                    node,
                    nodes,
                    signature,
                    exclude_name,
                    current_owner_name,
                    current_owner_type_name,
                    current_line,
                    value_name,
                )
            })
        })
        .or_else(|| {
            value_method_owner_name.and_then(|owner_name| {
                value_method_name.and_then(|method_name| {
                    resolve_direct_method_return_type(
                        node,
                        nodes,
                        signature,
                        exclude_name,
                        current_owner_name,
                        current_owner_type_name,
                        current_line,
                        owner_name,
                        method_name,
                        value_method_through_instance,
                    )
                })
            })
        })
        .or_else(|| {
            value_member_owner_name.and_then(|owner_name| {
                value_member_name.and_then(|member_name| {
                    resolve_direct_member_reference_type(
                        node,
                        nodes,
                        signature,
                        exclude_name,
                        current_owner_name,
                        current_owner_type_name,
                        current_line,
                        owner_name,
                        member_name,
                        value_member_through_instance,
                    )
                })
            })
        })
        .or_else(|| {
            value_subscript_target.and_then(|target| {
                resolve_direct_subscript_reference_type(
                    node,
                    nodes,
                    signature,
                    exclude_name,
                    current_owner_name,
                    current_owner_type_name,
                    current_line,
                    target,
                    value_subscript_string_key,
                    value_subscript_index,
                )
            })
        })
        .or_else(|| {
            let true_branch = value_if_true?;
            let false_branch = value_if_false?;
            if let Some(guard) = value_if_guard {
                let base_bindings = resolve_guard_scope_bindings(
                    node,
                    nodes,
                    signature,
                    exclude_name,
                    current_owner_name,
                    current_owner_type_name,
                    current_line,
                    guard,
                );
                let true_bindings =
                    apply_guard_to_local_bindings(node, nodes, &base_bindings, guard, true);
                let false_bindings =
                    apply_guard_to_local_bindings(node, nodes, &base_bindings, guard, false);
                return Some(join_branch_types(vec![
                    resolve_direct_expression_type_from_metadata_with_bindings(
                        node,
                        nodes,
                        signature,
                        current_owner_name,
                        current_owner_type_name,
                        current_line,
                        true_branch,
                        &true_bindings,
                    )?,
                    resolve_direct_expression_type_from_metadata_with_bindings(
                        node,
                        nodes,
                        signature,
                        current_owner_name,
                        current_owner_type_name,
                        current_line,
                        false_branch,
                        &false_bindings,
                    )?,
                ]));
            }
            Some(join_branch_types(vec![
                resolve_direct_expression_type_from_metadata(
                    node,
                    nodes,
                    signature,
                    current_owner_name,
                    current_owner_type_name,
                    current_line,
                    true_branch,
                )?,
                resolve_direct_expression_type_from_metadata(
                    node,
                    nodes,
                    signature,
                    current_owner_name,
                    current_owner_type_name,
                    current_line,
                    false_branch,
                )?,
            ]))
        })
        .or_else(|| {
            resolve_direct_boolop_type(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                value_bool_left,
                value_bool_right,
                value_if_guard,
                value_binop_operator,
            )
        })
        .or_else(|| {
            resolve_direct_binop_type(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                value_binop_left,
                value_binop_right,
                value_binop_operator,
            )
        });

    resolved.and_then(
        |resolved| {
            if is_awaited { unwrap_awaitable_type(&resolved) } else { Some(resolved) }
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn resolve_direct_boolop_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    left: Option<&typepython_syntax::DirectExprMetadata>,
    right: Option<&typepython_syntax::DirectExprMetadata>,
    guard: Option<&typepython_binding::GuardConditionSite>,
    operator: Option<&str>,
) -> Option<String> {
    let operator = operator?;
    if operator != "and" && operator != "or" {
        return None;
    }
    let left_type = resolve_direct_expression_type_from_metadata(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        left?,
    )?;
    let right_type = if let Some(guard) = guard {
        let base_bindings = resolve_guard_scope_bindings(
            node,
            nodes,
            signature,
            None,
            current_owner_name,
            current_owner_type_name,
            current_line,
            guard,
        );
        let narrowed_bindings =
            apply_guard_to_local_bindings(node, nodes, &base_bindings, guard, operator == "and");
        resolve_direct_expression_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            right?,
            &narrowed_bindings,
        )?
    } else {
        resolve_direct_expression_type_from_metadata(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            right?,
        )?
    };
    Some(join_branch_types(vec![left_type, right_type]))
}

#[allow(clippy::too_many_arguments)]
fn resolve_direct_binop_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    left: Option<&typepython_syntax::DirectExprMetadata>,
    right: Option<&typepython_syntax::DirectExprMetadata>,
    operator: Option<&str>,
) -> Option<String> {
    let operator = operator?;
    let left_type = resolve_direct_expression_type_from_metadata(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        left?,
    )?;
    let right_type = resolve_direct_expression_type_from_metadata(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        right?,
    )?;
    match operator.trim() {
        "+" => resolve_plus_result_type(&left_type, &right_type),
        "-" | "*" | "/" | "//" | "%"
            if is_numeric_type(&left_type) && is_numeric_type(&right_type) =>
        {
            Some(join_numeric_result_type(&left_type, &right_type))
        }
        _ => None,
    }
}

fn resolve_plus_result_type(left: &str, right: &str) -> Option<String> {
    if left == "str" && right == "str" {
        return Some(String::from("str"));
    }
    if is_numeric_type(left) && is_numeric_type(right) {
        return Some(join_numeric_result_type(left, right));
    }
    let (left_head, left_args) = split_generic_type(left)?;
    let (right_head, right_args) = split_generic_type(right)?;
    match (left_head, right_head) {
        ("list", "list") if left_args.len() == 1 && right_args.len() == 1 => Some(format!(
            "list[{}]",
            join_type_candidates(vec![left_args[0].clone(), right_args[0].clone()])
        )),
        ("tuple", "tuple") => {
            let mut args = left_args;
            args.extend(right_args);
            Some(format!("tuple[{}]", args.join(", ")))
        }
        _ => None,
    }
}

fn is_numeric_type(text: &str) -> bool {
    matches!(normalize_type_text(text).as_str(), "int" | "float" | "complex")
}

fn join_numeric_result_type(left: &str, right: &str) -> String {
    let left = normalize_type_text(left);
    let right = normalize_type_text(right);
    if left == "complex" || right == "complex" {
        String::from("complex")
    } else if left == "float" || right == "float" || left == "/" || right == "/" {
        String::from("float")
    } else {
        String::from("int")
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "subscript resolution needs the same expression context as other direct expression forms"
)]
fn resolve_direct_subscript_reference_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    _exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    target: &typepython_syntax::DirectExprMetadata,
    string_key: Option<&str>,
    index_text: Option<&str>,
) -> Option<String> {
    let target_type = resolve_direct_expression_type_from_metadata(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        target,
    )?;
    resolve_subscript_type_from_target_type(node, nodes, &target_type, string_key, index_text)
}

fn resolve_subscript_type_from_target_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    target_type: &str,
    string_key: Option<&str>,
    index_text: Option<&str>,
) -> Option<String> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    resolve_subscript_type_from_target_type_with_context(
        &context,
        node,
        nodes,
        target_type,
        string_key,
        index_text,
    )
}

fn resolve_subscript_type_from_target_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    target_type: &str,
    string_key: Option<&str>,
    index_text: Option<&str>,
) -> Option<String> {
    if let Some(key) = string_key
        && let Some(shape) =
            resolve_known_typed_dict_shape_from_type_with_context(context, node, nodes, target_type)
    {
        return typed_dict_known_or_extra_field(&shape, key)
            .map(|field| field.value_type().to_owned());
    }

    let normalized_target = normalize_type_text(target_type);
    if let Some((head, args)) = split_generic_type(&normalized_target) {
        return match head {
            "dict" | "Mapping" | "typing.Mapping" | "collections.abc.Mapping"
                if args.len() == 2 =>
            {
                Some(args[1].clone())
            }
            "list" | "Sequence" | "typing.Sequence" | "collections.abc.Sequence"
                if !args.is_empty() =>
            {
                Some(args[0].clone())
            }
            "tuple" if !args.is_empty() => {
                if args.len() == 2 && args[1] == "..." {
                    return Some(args[0].clone());
                }
                index_text
                    .and_then(|index| index.parse::<usize>().ok())
                    .and_then(|index| args.get(index).cloned())
                    .or_else(|| Some(join_type_candidates(args)))
            }
            _ => resolve_nominal_getitem_return_type(node, nodes, &normalized_target),
        };
    }

    resolve_nominal_getitem_return_type(node, nodes, &normalized_target)
}

fn resolve_nominal_getitem_return_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    owner_type_name: &str,
) -> Option<String> {
    let nominal_owner_name = split_generic_type(owner_type_name)
        .map(|(head, _)| head.to_owned())
        .unwrap_or_else(|| owner_type_name.to_owned());
    let (class_node, class_decl) = resolve_direct_base(nodes, node, &nominal_owner_name)?;
    let getitem = find_owned_callable_declaration(nodes, class_node, class_decl, "__getitem__")?;
    let return_text = rewrite_imported_typing_aliases(
        node,
        &substitute_self_annotation(
            getitem.detail.split_once("->")?.1.trim(),
            Some(owner_type_name),
        ),
    );
    normalized_direct_return_annotation(&return_text).map(normalize_type_text)
}

fn resolve_direct_return_name_type(signature: &str, value_name: &str) -> Option<String> {
    let param_names = direct_param_names(signature)?;
    let param_types = direct_param_types(signature)?;
    param_names.iter().zip(param_types.iter()).find_map(|(param_name, param_type)| {
        (param_name == value_name).then_some(normalize_type_text(param_type))
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "name reference resolution needs scope and source-position context"
)]
fn resolve_direct_name_reference_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<String> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    resolve_direct_name_reference_type_with_context(
        &context,
        node,
        nodes,
        signature,
        exclude_name,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    )
}

fn resolve_direct_name_reference_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<String> {
    if let Some(receiver_type) =
        resolve_receiver_name_type(node, current_owner_name, current_owner_type_name, value_name)
    {
        return Some(receiver_type);
    }

    let signature =
        signature.map(|signature| substitute_self_annotation(signature, current_owner_type_name));
    let base_type = resolve_unnarrowed_name_reference_type_with_context(
        context,
        node,
        nodes,
        signature.as_deref(),
        exclude_name,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    )?;

    Some(apply_guard_narrowing(
        node,
        nodes,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
        &base_type,
    ))
}

fn resolve_receiver_name_type(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    value_name: &str,
) -> Option<String> {
    let owner_type_name = current_owner_type_name?;
    let owner_name = current_owner_name?;
    let declaration = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Function
            && declaration.name == owner_name
            && declaration.owner.as_ref().is_some_and(|owner| owner.name == owner_type_name)
    })?;

    match (declaration.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance), value_name) {
        (typepython_syntax::MethodKind::Instance, "self")
        | (typepython_syntax::MethodKind::Property, "self")
        | (typepython_syntax::MethodKind::PropertySetter, "self") => {
            Some(String::from(owner_type_name))
        }
        (typepython_syntax::MethodKind::Class, "cls") => Some(format!("type[{owner_type_name}]")),
        _ => None,
    }
}

fn find_member_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
    predicate: impl Fn(&Declaration) -> bool + Copy,
) -> Option<&'a Declaration> {
    let mut visited = BTreeSet::new();
    find_member_declaration_with_visited(
        nodes,
        class_node,
        class_decl,
        member_name,
        predicate,
        &mut visited,
    )
}

fn find_member_declaration_with_visited<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
    predicate: impl Fn(&Declaration) -> bool + Copy,
    visited: &mut BTreeSet<(String, String)>,
) -> Option<&'a Declaration> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visited.insert(key) {
        return None;
    }

    if let Some(member) = class_node.declarations.iter().find(|declaration| {
        declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration.name == member_name
            && predicate(declaration)
    }) {
        return Some(member);
    }

    for base in &class_decl.bases {
        if let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) {
            if let Some(member) = find_member_declaration_with_visited(
                nodes,
                base_node,
                base_decl,
                member_name,
                predicate,
                visited,
            ) {
                return Some(member);
            }
        }
    }

    None
}

fn find_owned_value_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<&'a Declaration> {
    find_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
        declaration.kind == DeclarationKind::Value
    })
}

fn find_owned_readable_member_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<&'a Declaration> {
    find_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
        declaration.kind == DeclarationKind::Value
            || (declaration.kind == DeclarationKind::Function
                && declaration.method_kind == Some(typepython_syntax::MethodKind::Property))
    })
}

fn resolve_readable_member_type(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    owner_type_name: &str,
) -> Option<String> {
    match declaration.kind {
        DeclarationKind::Value => {
            let detail = rewrite_imported_typing_aliases(
                node,
                &substitute_self_annotation(&declaration.detail, Some(owner_type_name)),
            );
            normalized_direct_return_annotation(&detail).map(normalize_type_text).or_else(|| {
                declaration.value_type.as_deref().map(|value| {
                    normalize_type_text(&rewrite_imported_typing_aliases(
                        node,
                        &substitute_self_annotation(value, Some(owner_type_name)),
                    ))
                })
            })
        }
        DeclarationKind::Function
            if declaration.method_kind == Some(typepython_syntax::MethodKind::Property) =>
        {
            let return_text = rewrite_imported_typing_aliases(
                node,
                &substitute_self_annotation(
                    declaration.detail.split_once("->")?.1.trim(),
                    Some(owner_type_name),
                ),
            );
            normalized_direct_return_annotation(&return_text).map(normalize_type_text)
        }
        _ => None,
    }
}

fn resolve_member_access_owner_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    access: &typepython_binding::MemberAccessSite,
) -> Option<String> {
    if access.through_instance {
        resolve_direct_callable_return_type(node, nodes, &access.owner_name)
            .map(|return_type| normalize_type_text(&return_type))
            .or_else(|| Some(access.owner_name.clone()))
    } else {
        resolve_direct_name_reference_type(
            node,
            nodes,
            None,
            None,
            access.current_owner_name.as_deref(),
            access.current_owner_type_name.as_deref(),
            access.line,
            &access.owner_name,
        )
        .or_else(|| Some(access.owner_name.clone()))
    }
}

fn find_owned_callable_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Option<&'a Declaration> {
    find_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
        matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
    })
}

fn find_owned_callable_declarations<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
) -> Vec<&'a Declaration> {
    let mut visited = BTreeSet::new();
    find_owned_callable_declarations_with_visited(
        nodes,
        class_node,
        class_decl,
        member_name,
        &mut visited,
    )
}

fn find_owned_callable_declarations_with_visited<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    class_node: &'a typepython_graph::ModuleNode,
    class_decl: &'a Declaration,
    member_name: &str,
    visited: &mut BTreeSet<(String, String)>,
) -> Vec<&'a Declaration> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visited.insert(key) {
        return Vec::new();
    }

    let local = class_node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
                && declaration.name == member_name
                && matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })
        .collect::<Vec<_>>();
    if !local.is_empty() {
        return local;
    }

    for base in &class_decl.bases {
        if let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) {
            let inherited = find_owned_callable_declarations_with_visited(
                nodes,
                base_node,
                base_decl,
                member_name,
                visited,
            );
            if !inherited.is_empty() {
                return inherited;
            }
        }
    }

    Vec::new()
}

#[expect(
    clippy::too_many_arguments,
    reason = "unnarrowed name resolution needs scope and source-position context"
)]
fn resolve_unnarrowed_name_reference_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<String> {
    if let Some(signature) = signature {
        let signature = rewrite_imported_typing_aliases(
            node,
            &substitute_self_annotation(signature, current_owner_type_name),
        );
        if let Some(param_type) = resolve_direct_return_name_type(&signature, value_name) {
            return Some(param_type);
        }
    }

    if exclude_name.is_some_and(|name| name == value_name) {
        return None;
    }

    if let Some(exception_type) = resolve_exception_binding_type(
        node,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    ) {
        return Some(exception_type);
    }

    if let Some(loop_type) = resolve_for_loop_target_type(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    ) {
        return Some(loop_type);
    }

    if let Some(with_type) = resolve_with_target_name_type(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    ) {
        return Some(with_type);
    }

    if let Some(local_type) = resolve_local_assignment_reference_type(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        value_name,
    ) {
        return Some(local_type);
    }

    if current_owner_name.is_none() {
        if let Some(module_type) = resolve_module_level_assignment_reference_type(
            node,
            nodes,
            signature,
            current_line,
            value_name,
        ) {
            return Some(module_type);
        }
    }

    if let Some(local_value) = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Value
            && declaration.owner.is_none()
            && declaration.name == value_name
            && !declaration.detail.is_empty()
    }) {
        let detail = rewrite_imported_typing_aliases(
            node,
            &substitute_self_annotation(&local_value.detail, current_owner_type_name),
        );
        return normalized_direct_return_annotation(&detail).map(normalize_type_text);
    }

    if let Some(function) = resolve_direct_function(node, nodes, value_name) {
        if let Some(callable_annotation) =
            resolve_decorated_function_callable_annotation_with_context(
                context, node, nodes, value_name,
            )
        {
            return Some(callable_annotation);
        }
        let param_types = direct_signature_sites_from_detail(&function.detail)
            .into_iter()
            .map(|param| param.annotation.unwrap_or_else(|| String::from("dynamic")))
            .collect::<Vec<_>>();
        let return_text = resolve_direct_callable_return_type(node, nodes, value_name)?;
        return Some(format_callable_annotation(&param_types, &return_text));
    }

    if let Some(boundary_type) =
        unresolved_import_boundary_type_with_context(context, node, nodes, value_name)
    {
        return Some(String::from(boundary_type));
    }

    if let Some((head, _)) = value_name.split_once('.')
        && let Some(boundary_type) =
            unresolved_import_boundary_type_with_context(context, node, nodes, head)
    {
        return Some(String::from(boundary_type));
    }

    None
}

fn apply_guard_narrowing(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
    base_type: &str,
) -> String {
    let mut narrowed = normalize_type_text(base_type);

    let mut if_guards = node
        .if_guards
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == current_owner_name
                && guard.owner_type_name.as_deref() == current_owner_type_name
                && guard.line < current_line
                && !name_reassigned_after_line(
                    node,
                    current_owner_name,
                    current_owner_type_name,
                    value_name,
                    guard.line,
                    current_line,
                )
        })
        .filter_map(|guard| {
            let branch_true = if current_line >= guard.true_start_line
                && current_line <= guard.true_end_line
            {
                Some(true)
            } else if let (Some(start), Some(end)) = (guard.false_start_line, guard.false_end_line)
            {
                (current_line >= start && current_line <= end).then_some(false)
            } else {
                None
            }?;
            Some((guard.line, branch_true, guard.guard.as_ref()?))
        })
        .collect::<Vec<_>>();
    if_guards.sort_by_key(|(line, _, _)| *line);
    for (_, branch_true, guard) in if_guards {
        narrowed = apply_guard_condition(node, nodes, &narrowed, value_name, guard, branch_true);
    }

    let mut post_if_guards = node
        .if_guards
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == current_owner_name
                && guard.owner_type_name.as_deref() == current_owner_type_name
                && guard.line < current_line
                && !name_reassigned_after_line(
                    node,
                    current_owner_name,
                    current_owner_type_name,
                    value_name,
                    guard.line,
                    current_line,
                )
                && current_line > guard.false_end_line.unwrap_or(guard.true_end_line)
        })
        .filter_map(|guard| {
            let true_terminal = branch_has_return(
                node,
                current_owner_name,
                current_owner_type_name,
                guard.true_start_line,
                guard.true_end_line,
            );
            let false_terminal =
                guard.false_start_line.zip(guard.false_end_line).is_some_and(|(start, end)| {
                    branch_has_return(node, current_owner_name, current_owner_type_name, start, end)
                });
            let branch_true =
                match (true_terminal, false_terminal, guard.false_start_line.is_some()) {
                    (true, false, _) => Some(false),
                    (false, true, true) => Some(true),
                    _ => None,
                }?;
            Some((guard.line, branch_true, guard.guard.as_ref()?))
        })
        .collect::<Vec<_>>();
    post_if_guards.sort_by_key(|(line, _, _)| *line);
    for (_, branch_true, guard) in post_if_guards {
        narrowed = apply_guard_condition(node, nodes, &narrowed, value_name, guard, branch_true);
    }

    let mut asserts = node
        .asserts
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == current_owner_name
                && guard.owner_type_name.as_deref() == current_owner_type_name
                && guard.line < current_line
                && !name_reassigned_after_line(
                    node,
                    current_owner_name,
                    current_owner_type_name,
                    value_name,
                    guard.line,
                    current_line,
                )
        })
        .filter_map(|guard| Some((guard.line, guard.guard.as_ref()?)))
        .collect::<Vec<_>>();
    asserts.sort_by_key(|(line, _)| *line);
    for (_, guard) in asserts {
        narrowed = apply_guard_condition(node, nodes, &narrowed, value_name, guard, true);
    }

    narrowed
}

fn branch_has_return(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    start_line: usize,
    end_line: usize,
) -> bool {
    node.returns.iter().any(|site| {
        site.owner_name == current_owner_name.unwrap_or_default()
            && site.owner_type_name.as_deref() == current_owner_type_name
            && start_line <= site.line
            && site.line <= end_line
    })
}

fn name_reassigned_after_line(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    value_name: &str,
    after_line: usize,
    current_line: usize,
) -> bool {
    node.assignments.iter().any(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.as_deref() == current_owner_name
            && assignment.owner_type_name.as_deref() == current_owner_type_name
            && after_line < assignment.line
            && assignment.line < current_line
    }) || node.invalidations.iter().any(|site| {
        site.names.iter().any(|name| name == value_name)
            && site.owner_name.as_deref() == current_owner_name
            && site.owner_type_name.as_deref() == current_owner_type_name
            && after_line < site.line
            && site.line < current_line
    })
}

fn latest_delete_invalidation_line(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<usize> {
    node.invalidations
        .iter()
        .rev()
        .find(|site| {
            site.kind == typepython_binding::InvalidationKind::Delete
                && site.names.iter().any(|name| name == value_name)
                && site.owner_name.as_deref() == current_owner_name
                && site.owner_type_name.as_deref() == current_owner_type_name
                && site.line < current_line
        })
        .map(|site| site.line)
}

fn apply_guard_condition(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    base_type: &str,
    value_name: &str,
    guard: &typepython_binding::GuardConditionSite,
    branch_true: bool,
) -> String {
    match guard {
        typepython_binding::GuardConditionSite::IsNone { name, negated } if name == value_name => {
            match (branch_true, negated) {
                (true, false) | (false, true) => String::from("None"),
                (false, false) | (true, true) => {
                    remove_none_branch(base_type).unwrap_or_else(|| normalize_type_text(base_type))
                }
            }
        }
        typepython_binding::GuardConditionSite::IsInstance { name, types }
            if name == value_name =>
        {
            if branch_true {
                narrow_to_instance_types(base_type, types)
            } else {
                remove_instance_types(base_type, types)
            }
        }
        typepython_binding::GuardConditionSite::PredicateCall { name, callee }
            if name == value_name =>
        {
            apply_predicate_guard(node, nodes, base_type, callee, branch_true)
        }
        typepython_binding::GuardConditionSite::TruthyName { name } if name == value_name => {
            apply_truthy_narrowing(base_type, branch_true)
        }
        typepython_binding::GuardConditionSite::Not(inner) => {
            apply_guard_condition(node, nodes, base_type, value_name, inner, !branch_true)
        }
        typepython_binding::GuardConditionSite::And(parts) => {
            if branch_true {
                parts.iter().fold(normalize_type_text(base_type), |current, part| {
                    apply_guard_condition(node, nodes, &current, value_name, part, true)
                })
            } else {
                let mut joined = Vec::new();
                let mut current_true = normalize_type_text(base_type);
                for part in parts {
                    joined.push(apply_guard_condition(
                        node,
                        nodes,
                        &current_true,
                        value_name,
                        part,
                        false,
                    ));
                    current_true =
                        apply_guard_condition(node, nodes, &current_true, value_name, part, true);
                }
                join_type_candidates(joined)
            }
        }
        typepython_binding::GuardConditionSite::Or(parts) => {
            if branch_true {
                let mut joined = Vec::new();
                let mut current_false = normalize_type_text(base_type);
                for part in parts {
                    joined.push(apply_guard_condition(
                        node,
                        nodes,
                        &current_false,
                        value_name,
                        part,
                        true,
                    ));
                    current_false =
                        apply_guard_condition(node, nodes, &current_false, value_name, part, false);
                }
                join_type_candidates(joined)
            } else {
                parts.iter().fold(normalize_type_text(base_type), |current, part| {
                    apply_guard_condition(node, nodes, &current, value_name, part, false)
                })
            }
        }
        _ => normalize_type_text(base_type),
    }
}

fn apply_predicate_guard(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    base_type: &str,
    callee: &str,
    branch_true: bool,
) -> String {
    let Some((kind, guarded_type)) = parse_guard_return_kind(node, nodes, callee) else {
        return normalize_type_text(base_type);
    };
    match (kind.as_str(), branch_true) {
        ("TypeGuard", true) | ("TypeIs", true) => {
            narrow_to_instance_types(base_type, &[guarded_type])
        }
        ("TypeIs", false) => remove_instance_types(base_type, &[guarded_type]),
        _ => normalize_type_text(base_type),
    }
}

fn parse_guard_return_kind(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<(String, String)> {
    let function = resolve_direct_function(node, nodes, callee)?;
    let returns = normalized_direct_return_annotation(function.detail.split_once("->")?.1.trim())?;
    if let Some(inner) =
        returns.strip_prefix("TypeGuard[").and_then(|inner| inner.strip_suffix(']'))
    {
        return Some((String::from("TypeGuard"), normalize_type_text(inner)));
    }
    if let Some(inner) = returns.strip_prefix("TypeIs[").and_then(|inner| inner.strip_suffix(']')) {
        return Some((String::from("TypeIs"), normalize_type_text(inner)));
    }
    None
}

fn narrow_to_instance_types(base_type: &str, types: &[String]) -> String {
    let normalized_types = types.iter().map(|ty| normalize_type_text(ty)).collect::<Vec<_>>();
    if let Some(branches) = union_branches(base_type) {
        let kept = branches
            .into_iter()
            .filter(|branch| {
                normalized_types.iter().any(|ty| direct_type_matches_normalized_plain(ty, branch))
            })
            .collect::<Vec<_>>();
        if !kept.is_empty() {
            return join_union_branches(kept);
        }
    }
    join_union_branches(normalized_types)
}

fn remove_instance_types(base_type: &str, types: &[String]) -> String {
    let normalized = normalize_type_text(base_type);
    let Some(branches) = union_branches(&normalized) else {
        return normalized;
    };
    let normalized_types = types.iter().map(|ty| normalize_type_text(ty)).collect::<Vec<_>>();
    let kept = branches
        .into_iter()
        .filter(|branch| {
            !normalized_types.iter().any(|ty| direct_type_matches_normalized_plain(ty, branch))
        })
        .collect::<Vec<_>>();
    if kept.is_empty() { normalized } else { join_union_branches(kept) }
}

fn remove_none_branch(base_type: &str) -> Option<String> {
    let normalized = normalize_type_text(base_type);
    let branches = union_branches(&normalized)?;
    let kept = branches.into_iter().filter(|branch| branch != "None").collect::<Vec<_>>();
    (!kept.is_empty()).then(|| join_union_branches(kept))
}

fn join_union_branches(branches: Vec<String>) -> String {
    if branches.len() == 1 {
        branches.into_iter().next().unwrap_or_default()
    } else {
        format!("Union[{}]", branches.join(", "))
    }
}

fn join_type_candidates(candidates: Vec<String>) -> String {
    let mut branches = Vec::new();
    for candidate in candidates {
        if let Some(candidate_branches) = union_branches(&candidate) {
            for branch in candidate_branches {
                if !branches.contains(&branch) {
                    branches.push(branch);
                }
            }
        } else if !branches.contains(&candidate) {
            branches.push(candidate);
        }
    }
    join_union_branches(branches)
}

fn apply_truthy_narrowing(base_type: &str, branch_true: bool) -> String {
    let normalized = normalize_type_text(base_type);
    if normalized == "Literal[True]" {
        return if branch_true { normalized } else { String::from("Literal[False]") };
    }
    if normalized == "Literal[False]" {
        return if branch_true { String::from("Literal[True]") } else { normalized };
    }
    if normalized == "bool" {
        return normalized;
    }

    let Some(branches) = union_branches(&normalized) else {
        return normalized;
    };
    let non_none =
        branches.iter().filter(|branch| branch.as_str() != "None").cloned().collect::<Vec<_>>();
    if branches.iter().any(|branch| branch == "None")
        && non_none.iter().all(|branch| is_definitely_truthy_branch(branch))
    {
        return if branch_true { join_union_branches(non_none) } else { String::from("None") };
    }

    normalized
}

fn is_definitely_truthy_branch(branch: &str) -> bool {
    let normalized = normalize_type_text(branch);
    if normalized == "Literal[True]" {
        return true;
    }
    if normalized == "Literal[False]" || normalized == "None" || normalized == "bool" {
        return false;
    }
    matches!(
        normalized.as_str(),
        "bytes" | "str" | "int" | "float" | "complex" | "list" | "dict" | "set" | "tuple"
    )
    .then_some(false)
    .unwrap_or(true)
}

fn resolve_exception_binding_type(
    node: &typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<String> {
    let except_site = node.except_handlers.iter().rev().find(|except_site| {
        except_site.binding_name.as_deref() == Some(value_name)
            && except_site.owner_name.as_deref() == current_owner_name
            && except_site.owner_type_name.as_deref() == current_owner_type_name
            && except_site.line < current_line
            && current_line <= except_site.end_line
    })?;

    Some(normalize_exception_binding_type(&except_site.exception_type))
}

fn normalize_exception_binding_type(text: &str) -> String {
    let text = text.trim();
    if let Some(inner) = text.strip_prefix('(').and_then(|inner| inner.strip_suffix(')')) {
        let members = split_top_level_type_args(inner)
            .into_iter()
            .map(normalize_type_text)
            .collect::<Vec<_>>();
        return format!("Union[{}]", members.join(", "));
    }
    normalize_type_text(text)
}

fn resolve_for_loop_target_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<String> {
    let loop_site = node.for_loops.iter().rev().find(|for_loop| {
        (for_loop.target_name == value_name
            || for_loop.target_names.iter().any(|name| name == value_name))
            && for_loop.owner_name.as_deref() == current_owner_name
            && for_loop.owner_type_name.as_deref() == current_owner_type_name
            && for_loop.line < current_line
    })?;

    let iter_type = resolve_direct_expression_type(
        node,
        nodes,
        signature,
        None,
        loop_site.owner_name.as_deref(),
        loop_site.owner_type_name.as_deref(),
        loop_site.line,
        loop_site.iter_type.as_deref(),
        loop_site.iter_is_awaited,
        loop_site.iter_callee.as_deref(),
        loop_site.iter_name.as_deref(),
        loop_site.iter_member_owner_name.as_deref(),
        loop_site.iter_member_name.as_deref(),
        loop_site.iter_member_through_instance,
        loop_site.iter_method_owner_name.as_deref(),
        loop_site.iter_method_name.as_deref(),
        loop_site.iter_method_through_instance,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    let element_type = unwrap_for_iterable_type(&iter_type)?;

    if let Some(index) = loop_site.target_names.iter().position(|name| name == value_name) {
        if let Some(elements) = unwrap_fixed_tuple_elements(&element_type) {
            if elements.len() == loop_site.target_names.len() {
                return elements.get(index).cloned();
            }
            return None;
        }
        return unwrap_for_iterable_type(&element_type);
    }

    Some(element_type)
}

fn unwrap_fixed_tuple_elements(text: &str) -> Option<Vec<String>> {
    let text = normalize_type_text(text);
    let inner = text.strip_prefix("tuple[").and_then(|inner| inner.strip_suffix(']'))?;
    let args = split_top_level_type_args(inner);
    if args.len() == 2 && args[1] == "..." {
        return None;
    }
    Some(args.into_iter().map(normalize_type_text).collect())
}

fn resolve_with_target_name_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<String> {
    let with_site = node.with_statements.iter().rev().find(|with_site| {
        with_site.target_name.as_deref() == Some(value_name)
            && with_site.owner_name.as_deref() == current_owner_name
            && with_site.owner_type_name.as_deref() == current_owner_type_name
            && with_site.line < current_line
    })?;

    resolve_with_target_type_for_signature(node, nodes, signature, with_site)
}

fn resolve_with_owner_signature<'a>(
    node: &'a typepython_graph::ModuleNode,
    with_site: &typepython_binding::WithSite,
) -> Option<&'a str> {
    let owner_name = with_site.owner_name.as_deref()?;
    node.declarations
        .iter()
        .find(|declaration| {
            declaration.kind == DeclarationKind::Function
                && declaration.name == owner_name
                && match (&with_site.owner_type_name, &declaration.owner) {
                    (Some(owner_type_name), Some(owner)) => owner.name == *owner_type_name,
                    (None, None) => true,
                    _ => false,
                }
        })
        .map(|declaration| declaration.detail.as_str())
}

fn resolve_with_target_type_for_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    with_site: &typepython_binding::WithSite,
) -> Option<String> {
    let context_type = resolve_direct_expression_type(
        node,
        nodes,
        signature,
        None,
        with_site.owner_name.as_deref(),
        with_site.owner_type_name.as_deref(),
        with_site.line,
        with_site.context_type.as_deref(),
        with_site.context_is_awaited,
        with_site.context_callee.as_deref(),
        with_site.context_name.as_deref(),
        with_site.context_member_owner_name.as_deref(),
        with_site.context_member_name.as_deref(),
        with_site.context_member_through_instance,
        with_site.context_method_owner_name.as_deref(),
        with_site.context_method_name.as_deref(),
        with_site.context_method_through_instance,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    let (class_node, class_decl) = resolve_direct_base(nodes, node, &context_type)?;
    let enter = class_node.declarations.iter().find(|declaration| {
        declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration.name == "__enter__"
            && declaration.kind == DeclarationKind::Function
    })?;
    let exit = class_node.declarations.iter().find(|declaration| {
        declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration.name == "__exit__"
            && declaration.kind == DeclarationKind::Function
    })?;
    let _ = exit;

    normalized_direct_return_annotation(enter.detail.split_once("->")?.1.trim())
        .map(normalize_type_text)
}

fn resolve_local_assignment_reference_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<String> {
    let owner_name = current_owner_name?;
    let deleted_after_line = latest_delete_invalidation_line(
        node,
        Some(owner_name),
        current_owner_type_name,
        current_line,
        value_name,
    );
    if let Some(joined) = resolve_post_if_joined_assignment_type(
        node,
        nodes,
        signature,
        Some(owner_name),
        current_owner_type_name,
        current_line,
        value_name,
    )
    .filter(|_| deleted_after_line.is_none())
    {
        return Some(joined);
    }
    let assignment = node.assignments.iter().rev().find(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.as_deref() == Some(owner_name)
            && assignment.owner_type_name.as_deref() == current_owner_type_name
            && assignment.line < current_line
            && deleted_after_line.is_none_or(|deleted_line| assignment.line > deleted_line)
    })?;
    resolve_assignment_site_type(node, nodes, signature, assignment)
}

fn resolve_module_level_assignment_reference_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<String> {
    let deleted_after_line =
        latest_delete_invalidation_line(node, None, None, current_line, value_name);
    if let Some(joined) = resolve_post_if_joined_assignment_type(
        node,
        nodes,
        signature,
        None,
        None,
        current_line,
        value_name,
    )
    .filter(|_| deleted_after_line.is_none())
    {
        return Some(joined);
    }
    let assignment = node.assignments.iter().rev().find(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.is_none()
            && assignment.line < current_line
            && deleted_after_line.is_none_or(|deleted_line| assignment.line > deleted_line)
    })?;
    resolve_assignment_site_type(node, nodes, signature, assignment)
}

fn resolve_assignment_site_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    assignment: &typepython_binding::AssignmentSite,
) -> Option<String> {
    if let Some(index) = assignment.destructuring_index {
        let tuple_elements = unwrap_fixed_tuple_elements(&resolve_direct_expression_type(
            node,
            nodes,
            signature,
            Some(assignment.name.as_str()),
            assignment.owner_name.as_deref(),
            assignment.owner_type_name.as_deref(),
            assignment.line,
            assignment.value_type.as_deref(),
            assignment.is_awaited,
            assignment.value_callee.as_deref(),
            assignment.value_name.as_deref(),
            assignment.value_member_owner_name.as_deref(),
            assignment.value_member_name.as_deref(),
            assignment.value_member_through_instance,
            assignment.value_method_owner_name.as_deref(),
            assignment.value_method_name.as_deref(),
            assignment.value_method_through_instance,
            assignment.value_subscript_target.as_deref(),
            assignment.value_subscript_string_key.as_deref(),
            assignment.value_subscript_index.as_deref(),
            assignment.value_if_true.as_deref(),
            assignment.value_if_false.as_deref(),
            assignment.value_if_guard.as_ref(),
            assignment.value_bool_left.as_deref(),
            assignment.value_bool_right.as_deref(),
            assignment.value_binop_left.as_deref(),
            assignment.value_binop_right.as_deref(),
            assignment.value_binop_operator.as_deref(),
        )?)?;
        let target_names = assignment.destructuring_target_names.as_ref()?;
        if tuple_elements.len() == target_names.len() {
            return tuple_elements.get(index).cloned();
        }
        return None;
    }
    if let Some(comprehension) = assignment.value_list_comprehension.as_deref() {
        return match comprehension.kind {
            typepython_syntax::ComprehensionKind::List => resolve_list_comprehension_type(
                node,
                nodes,
                signature,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                comprehension,
            ),
            typepython_syntax::ComprehensionKind::Set => resolve_set_comprehension_type(
                node,
                nodes,
                signature,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                comprehension,
            ),
            typepython_syntax::ComprehensionKind::Dict => resolve_dict_comprehension_type(
                node,
                nodes,
                signature,
                assignment.owner_name.as_deref(),
                assignment.owner_type_name.as_deref(),
                assignment.line,
                comprehension,
            ),
            typepython_syntax::ComprehensionKind::Generator => {
                resolve_generator_comprehension_type(
                    node,
                    nodes,
                    signature,
                    assignment.owner_name.as_deref(),
                    assignment.owner_type_name.as_deref(),
                    assignment.line,
                    comprehension,
                )
            }
        };
    }
    if let Some(comprehension) = assignment.value_generator_comprehension.as_deref() {
        return resolve_generator_comprehension_type(
            node,
            nodes,
            signature,
            assignment.owner_name.as_deref(),
            assignment.owner_type_name.as_deref(),
            assignment.line,
            comprehension,
        );
    }

    resolve_direct_expression_type(
        node,
        nodes,
        signature,
        Some(assignment.name.as_str()),
        assignment.owner_name.as_deref(),
        assignment.owner_type_name.as_deref(),
        assignment.line,
        assignment.value_type.as_deref(),
        assignment.is_awaited,
        assignment.value_callee.as_deref(),
        assignment.value_name.as_deref(),
        assignment.value_member_owner_name.as_deref(),
        assignment.value_member_name.as_deref(),
        assignment.value_member_through_instance,
        assignment.value_method_owner_name.as_deref(),
        assignment.value_method_name.as_deref(),
        assignment.value_method_through_instance,
        assignment.value_subscript_target.as_deref(),
        assignment.value_subscript_string_key.as_deref(),
        assignment.value_subscript_index.as_deref(),
        assignment.value_if_true.as_deref(),
        assignment.value_if_false.as_deref(),
        assignment.value_if_guard.as_ref(),
        assignment.value_bool_left.as_deref(),
        assignment.value_bool_right.as_deref(),
        assignment.value_binop_left.as_deref(),
        assignment.value_binop_right.as_deref(),
        assignment.value_binop_operator.as_deref(),
    )
}

fn resolve_comprehension_local_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    comprehension: &typepython_syntax::ComprehensionMetadata,
) -> Option<BTreeMap<String, String>> {
    let mut local_bindings = BTreeMap::new();
    for clause in &comprehension.clauses {
        let iter_type = resolve_direct_expression_type_from_metadata(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            clause.iter.as_ref(),
        )?;
        let element_type = unwrap_for_iterable_type(&iter_type)?;
        bind_list_comprehension_targets(&mut local_bindings, &clause.target_names, &element_type);
        for guard in &clause.filters {
            for (name, value_type) in local_bindings.clone() {
                local_bindings.insert(
                    name.clone(),
                    apply_guard_condition(
                        node,
                        nodes,
                        &value_type,
                        &name,
                        &guard_to_site(guard),
                        true,
                    ),
                );
            }
        }
    }
    Some(local_bindings)
}

fn resolve_list_comprehension_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    comprehension: &typepython_syntax::ComprehensionMetadata,
) -> Option<String> {
    let local_bindings = resolve_comprehension_local_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension,
    )?;

    let element_type = resolve_direct_expression_type_from_metadata_with_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension.element.as_ref(),
        &local_bindings,
    )?;
    Some(format!("list[{element_type}]"))
}

fn resolve_set_comprehension_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    comprehension: &typepython_syntax::ComprehensionMetadata,
) -> Option<String> {
    let local_bindings = resolve_comprehension_local_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension,
    )?;

    let element_type = resolve_direct_expression_type_from_metadata_with_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension.element.as_ref(),
        &local_bindings,
    )?;
    Some(format!("set[{element_type}]"))
}

fn resolve_dict_comprehension_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    comprehension: &typepython_syntax::ComprehensionMetadata,
) -> Option<String> {
    let local_bindings = resolve_comprehension_local_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension,
    )?;
    let key_type = resolve_direct_expression_type_from_metadata_with_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension.key.as_deref()?,
        &local_bindings,
    )?;
    let value_type = resolve_direct_expression_type_from_metadata_with_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension.element.as_ref(),
        &local_bindings,
    )?;
    Some(format!("dict[{key_type}, {value_type}]"))
}

fn resolve_generator_comprehension_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    comprehension: &typepython_syntax::ComprehensionMetadata,
) -> Option<String> {
    let local_bindings = resolve_comprehension_local_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension,
    )?;

    let element_type = resolve_direct_expression_type_from_metadata_with_bindings(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        comprehension.element.as_ref(),
        &local_bindings,
    )?;
    Some(format!("Generator[{element_type}, None, None]"))
}

fn collect_guard_binding_names(
    guard: &typepython_binding::GuardConditionSite,
    names: &mut BTreeSet<String>,
) {
    match guard {
        typepython_binding::GuardConditionSite::IsNone { name, .. }
        | typepython_binding::GuardConditionSite::IsInstance { name, .. }
        | typepython_binding::GuardConditionSite::PredicateCall { name, .. }
        | typepython_binding::GuardConditionSite::TruthyName { name } => {
            names.insert(name.clone());
        }
        typepython_binding::GuardConditionSite::Not(inner) => {
            collect_guard_binding_names(inner, names);
        }
        typepython_binding::GuardConditionSite::And(parts)
        | typepython_binding::GuardConditionSite::Or(parts) => {
            for part in parts {
                collect_guard_binding_names(part, names);
            }
        }
    }
}

fn apply_guard_to_local_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    local_bindings: &BTreeMap<String, String>,
    guard: &typepython_binding::GuardConditionSite,
    branch_true: bool,
) -> BTreeMap<String, String> {
    let mut narrowed = local_bindings.clone();
    let mut names = BTreeSet::new();
    collect_guard_binding_names(guard, &mut names);
    for name in names {
        if let Some(base_type) = local_bindings.get(&name) {
            narrowed.insert(
                name.clone(),
                apply_guard_condition(node, nodes, base_type, &name, guard, branch_true),
            );
        }
    }
    narrowed
}

#[allow(clippy::too_many_arguments)]
fn resolve_guard_scope_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    guard: &typepython_binding::GuardConditionSite,
) -> BTreeMap<String, String> {
    let mut bindings = BTreeMap::new();
    let mut names = BTreeSet::new();
    collect_guard_binding_names(guard, &mut names);
    for name in names {
        if let Some(base_type) = resolve_direct_name_reference_type(
            node,
            nodes,
            signature,
            exclude_name,
            current_owner_name,
            current_owner_type_name,
            current_line,
            &name,
        ) {
            bindings.insert(name, base_type);
        }
    }
    bindings
}

#[allow(clippy::too_many_arguments)]
fn resolve_direct_expression_type_from_metadata_with_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
    local_bindings: &BTreeMap<String, String>,
) -> Option<String> {
    if let Some(lambda) = metadata.value_lambda.as_deref() {
        let (param_types, return_type) = resolve_contextual_lambda_callable_signature(
            node,
            nodes,
            current_owner_name,
            current_owner_type_name,
            current_line,
            lambda,
            signature,
            Some(local_bindings),
        )?;
        return Some(format_callable_annotation(&param_types, &return_type));
    }
    if let Some(value_name) = metadata.value_name.as_deref()
        && let Some(bound_type) = local_bindings.get(value_name)
    {
        return Some(bound_type.clone());
    }
    if let Some(target) = metadata.value_subscript_target.as_deref() {
        let target_type = resolve_direct_expression_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            target,
            local_bindings,
        )?;
        return resolve_subscript_type_from_target_type(
            node,
            nodes,
            &target_type,
            metadata.value_subscript_string_key.as_deref(),
            metadata.value_subscript_index.as_deref(),
        );
    }
    if let (Some(true_branch), Some(false_branch)) =
        (metadata.value_if_true.as_deref(), metadata.value_if_false.as_deref())
    {
        if let Some(guard) = metadata.value_if_guard.as_ref() {
            let guard = guard_to_site(guard);
            let true_bindings =
                apply_guard_to_local_bindings(node, nodes, local_bindings, &guard, true);
            let false_bindings =
                apply_guard_to_local_bindings(node, nodes, local_bindings, &guard, false);
            let true_type = resolve_direct_expression_type_from_metadata_with_bindings(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                true_branch,
                &true_bindings,
            )?;
            let false_type = resolve_direct_expression_type_from_metadata_with_bindings(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                false_branch,
                &false_bindings,
            )?;
            return Some(join_branch_types(vec![true_type, false_type]));
        }
        let true_type = resolve_direct_expression_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            true_branch,
            local_bindings,
        )?;
        let false_type = resolve_direct_expression_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            false_branch,
            local_bindings,
        )?;
        return Some(join_branch_types(vec![true_type, false_type]));
    }
    if let (Some(left), Some(right), Some(operator)) = (
        metadata.value_bool_left.as_deref(),
        metadata.value_bool_right.as_deref(),
        metadata.value_binop_operator.as_deref(),
    ) && (operator == "and" || operator == "or")
    {
        let left_type = resolve_direct_expression_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            left,
            local_bindings,
        )?;
        let right_type = if let Some(guard) = metadata.value_if_guard.as_ref() {
            let narrowed_bindings = apply_guard_to_local_bindings(
                node,
                nodes,
                local_bindings,
                &guard_to_site(guard),
                operator == "and",
            );
            resolve_direct_expression_type_from_metadata_with_bindings(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                right,
                &narrowed_bindings,
            )?
        } else {
            resolve_direct_expression_type_from_metadata_with_bindings(
                node,
                nodes,
                signature,
                current_owner_name,
                current_owner_type_name,
                current_line,
                right,
                local_bindings,
            )?
        };
        return Some(join_branch_types(vec![left_type, right_type]));
    }
    if let (Some(left), Some(right), Some(operator)) = (
        metadata.value_binop_left.as_deref(),
        metadata.value_binop_right.as_deref(),
        metadata.value_binop_operator.as_deref(),
    ) {
        let left_type = resolve_direct_expression_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            left,
            local_bindings,
        )?;
        let right_type = resolve_direct_expression_type_from_metadata_with_bindings(
            node,
            nodes,
            signature,
            current_owner_name,
            current_owner_type_name,
            current_line,
            right,
            local_bindings,
        )?;
        if let Some(result) = match operator {
            "+" => resolve_plus_result_type(&left_type, &right_type),
            "-" | "*" | "/" | "//" | "%"
                if is_numeric_type(&left_type) && is_numeric_type(&right_type) =>
            {
                Some(join_numeric_result_type(&left_type, &right_type))
            }
            _ => None,
        } {
            return Some(result);
        }
    }

    resolve_direct_expression_type_from_metadata(
        node,
        nodes,
        signature,
        current_owner_name,
        current_owner_type_name,
        current_line,
        metadata,
    )
}

fn bind_list_comprehension_targets(
    local_bindings: &mut BTreeMap<String, String>,
    target_names: &[String],
    element_type: &str,
) {
    if target_names.is_empty() {
        return;
    }
    if target_names.len() == 1 {
        local_bindings.insert(target_names[0].clone(), normalize_type_text(element_type));
        return;
    }
    if let Some(tuple_elements) = unwrap_fixed_tuple_elements(element_type)
        && tuple_elements.len() == target_names.len()
    {
        for (name, value_type) in target_names.iter().zip(tuple_elements) {
            local_bindings.insert(name.clone(), value_type);
        }
        return;
    }
    for name in target_names {
        local_bindings.insert(name.clone(), normalize_type_text(element_type));
    }
}

fn guard_to_site(
    guard: &typepython_syntax::GuardCondition,
) -> typepython_binding::GuardConditionSite {
    match guard {
        typepython_syntax::GuardCondition::IsNone { name, negated } => {
            typepython_binding::GuardConditionSite::IsNone { name: name.clone(), negated: *negated }
        }
        typepython_syntax::GuardCondition::IsInstance { name, types } => {
            typepython_binding::GuardConditionSite::IsInstance {
                name: name.clone(),
                types: types.clone(),
            }
        }
        typepython_syntax::GuardCondition::PredicateCall { name, callee } => {
            typepython_binding::GuardConditionSite::PredicateCall {
                name: name.clone(),
                callee: callee.clone(),
            }
        }
        typepython_syntax::GuardCondition::TruthyName { name } => {
            typepython_binding::GuardConditionSite::TruthyName { name: name.clone() }
        }
        typepython_syntax::GuardCondition::Not(inner) => {
            typepython_binding::GuardConditionSite::Not(Box::new(guard_to_site(inner)))
        }
        typepython_syntax::GuardCondition::And(parts) => {
            typepython_binding::GuardConditionSite::And(parts.iter().map(guard_to_site).collect())
        }
        typepython_syntax::GuardCondition::Or(parts) => {
            typepython_binding::GuardConditionSite::Or(parts.iter().map(guard_to_site).collect())
        }
    }
}

fn resolve_post_if_joined_assignment_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    value_name: &str,
) -> Option<String> {
    let mut guards = node
        .if_guards
        .iter()
        .filter(|guard| {
            guard.owner_name.as_deref() == current_owner_name
                && guard.owner_type_name.as_deref() == current_owner_type_name
                && guard.false_start_line.is_some()
                && guard.false_end_line.is_some()
        })
        .filter_map(|guard| {
            let false_end = guard.false_end_line?;
            let after_line = guard.true_end_line.max(false_end);
            (current_line > after_line).then_some((after_line, guard))
        })
        .collect::<Vec<_>>();
    guards.sort_by_key(|(after_line, _)| *after_line);

    for (after_line, guard) in guards.into_iter().rev() {
        if name_reassigned_after_line(
            node,
            current_owner_name,
            current_owner_type_name,
            value_name,
            after_line,
            current_line,
        ) {
            continue;
        }

        let true_assignment = latest_assignment_in_range(
            node,
            current_owner_name,
            current_owner_type_name,
            value_name,
            guard.true_start_line,
            guard.true_end_line,
        )?;
        let false_assignment = latest_assignment_in_range(
            node,
            current_owner_name,
            current_owner_type_name,
            value_name,
            guard.false_start_line?,
            guard.false_end_line?,
        )?;
        let true_type = resolve_assignment_site_type(node, nodes, signature, true_assignment)?;
        let false_type = resolve_assignment_site_type(node, nodes, signature, false_assignment)?;
        return Some(join_branch_types(vec![true_type, false_type]));
    }

    None
}

fn latest_assignment_in_range<'a>(
    node: &'a typepython_graph::ModuleNode,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    value_name: &str,
    start_line: usize,
    end_line: usize,
) -> Option<&'a typepython_binding::AssignmentSite> {
    node.assignments.iter().rev().find(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.as_deref() == current_owner_name
            && assignment.owner_type_name.as_deref() == current_owner_type_name
            && start_line <= assignment.line
            && assignment.line <= end_line
    })
}

fn join_branch_types(types: Vec<String>) -> String {
    if types.iter().any(|ty| ty == "Any") {
        return String::from("Any");
    }
    join_type_candidates(types)
}

#[expect(
    clippy::too_many_arguments,
    reason = "member reference resolution needs source metadata and scope context"
)]
fn resolve_direct_member_reference_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    owner_name: &str,
    member_name: &str,
    through_instance: bool,
) -> Option<String> {
    if !through_instance
        && let Some(reference_type) =
            resolve_imported_module_member_reference_type(node, nodes, owner_name, member_name)
    {
        return Some(reference_type);
    }

    let owner_type_name = if through_instance {
        resolve_direct_callable_return_type(node, nodes, owner_name)
            .map(|return_type| normalize_type_text(&return_type))
            .or_else(|| Some(owner_name.to_owned()))
    } else {
        resolve_direct_name_reference_type(
            node,
            nodes,
            signature,
            exclude_name,
            current_owner_name,
            current_owner_type_name,
            current_line,
            owner_name,
        )
        .or_else(|| Some(owner_name.to_owned()))
    }?;

    let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
    let member =
        find_owned_readable_member_declaration(nodes, class_node, class_decl, member_name)?;
    if is_enum_like_class(nodes, class_node, class_decl) {
        return Some(format!("Literal[{}.{}]", class_decl.name, member_name));
    }
    resolve_readable_member_type(node, member, &owner_type_name)
}

fn is_enum_like_class(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
) -> bool {
    declaration.bases.iter().any(|base| {
        matches!(
            base.as_str(),
            "Enum"
                | "IntEnum"
                | "StrEnum"
                | "Flag"
                | "IntFlag"
                | "enum.Enum"
                | "enum.IntEnum"
                | "enum.StrEnum"
                | "enum.Flag"
                | "enum.IntFlag"
        ) || resolve_direct_base(nodes, node, base)
            .is_some_and(|(base_node, base_decl)| is_enum_like_class(nodes, base_node, base_decl))
    })
}

fn is_flag_enum_like_class(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
) -> bool {
    declaration.bases.iter().any(|base| {
        matches!(base.as_str(), "Flag" | "IntFlag" | "enum.Flag" | "enum.IntFlag")
            || resolve_direct_base(nodes, node, base).is_some_and(|(base_node, base_decl)| {
                is_flag_enum_like_class(nodes, base_node, base_decl)
            })
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "method return resolution needs source metadata and scope context"
)]
fn resolve_direct_method_return_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    exclude_name: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    owner_name: &str,
    method_name: &str,
    through_instance: bool,
) -> Option<String> {
    if !through_instance
        && let Some(return_type) = resolve_imported_module_method_return_type(
            node,
            nodes,
            current_line,
            owner_name,
            method_name,
        )
    {
        return Some(return_type);
    }

    let owner_type_name = if through_instance {
        resolve_direct_callable_return_type(node, nodes, owner_name)
            .map(|return_type| normalize_type_text(&return_type))
            .or_else(|| Some(owner_name.to_owned()))
    } else {
        resolve_direct_name_reference_type(
            node,
            nodes,
            signature,
            exclude_name,
            current_owner_name,
            current_owner_type_name,
            current_line,
            owner_name,
        )
        .or_else(|| Some(owner_name.to_owned()))
    }?;

    let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
    let methods = find_owned_callable_declarations(nodes, class_node, class_decl, method_name);
    let method = if methods.iter().any(|declaration| declaration.kind == DeclarationKind::Overload)
    {
        let call = node.method_calls.iter().find(|call| {
            call.owner_name == owner_name
                && call.method == method_name
                && call.through_instance == through_instance
                && call.line == current_line
        })?;
        let call = typepython_binding::CallSite {
            callee: format!("{}.{}", class_decl.name, method_name),
            arg_count: call.arg_count,
            arg_types: call.arg_types.clone(),
            arg_values: call.arg_values.clone(),
            starred_arg_types: call.starred_arg_types.clone(),
            starred_arg_values: call.starred_arg_values.clone(),
            keyword_names: call.keyword_names.clone(),
            keyword_arg_types: call.keyword_arg_types.clone(),
            keyword_arg_values: call.keyword_arg_values.clone(),
            keyword_expansion_types: call.keyword_expansion_types.clone(),
            keyword_expansion_values: call.keyword_expansion_values.clone(),
            line: 1,
        };
        let applicable = methods
            .iter()
            .copied()
            .filter(|declaration| {
                method_overload_is_applicable(node, nodes, &call, declaration, &owner_type_name)
            })
            .collect::<Vec<_>>();
        if applicable.len() == 1 {
            applicable[0]
        } else {
            return None;
        }
    } else {
        *methods.first()?
    };
    let return_text = rewrite_imported_typing_aliases(
        node,
        &substitute_self_annotation(
            method.detail.split_once("->")?.1.trim(),
            Some(&owner_type_name),
        ),
    );
    normalized_direct_return_annotation(&return_text).map(normalize_type_text)
}

fn unwrap_awaitable_type(text: &str) -> Option<String> {
    let text = normalize_type_text(text);
    if let Some(inner) = text.strip_prefix("Awaitable[").and_then(|inner| inner.strip_suffix(']')) {
        return Some(normalize_type_text(inner));
    }
    if let Some(inner) = text.strip_prefix("Coroutine[").and_then(|inner| inner.strip_suffix(']')) {
        let args = split_top_level_type_args(inner);
        if args.len() == 3 {
            return Some(normalize_type_text(args[2]));
        }
    }
    None
}

fn unwrap_generator_yield_type(text: &str) -> Option<String> {
    let text = normalize_type_text(text);
    let inner = text.strip_prefix("Generator[").and_then(|inner| inner.strip_suffix(']'))?;
    let args = split_top_level_type_args(inner);
    args.first().map(|arg| normalize_type_text(arg))
}

fn unwrap_yield_from_type(text: &str) -> Option<String> {
    let text = normalize_type_text(text);

    if let Some(inner) = text.strip_prefix("Generator[").and_then(|inner| inner.strip_suffix(']')) {
        let args = split_top_level_type_args(inner);
        return args.first().map(|arg| normalize_type_text(arg));
    }

    if let Some(inner) = text.strip_prefix("Iterator[").and_then(|inner| inner.strip_suffix(']')) {
        return Some(normalize_type_text(inner));
    }

    if let Some(inner) = text.strip_prefix("Iterable[").and_then(|inner| inner.strip_suffix(']')) {
        return Some(normalize_type_text(inner));
    }

    if let Some(inner) = text.strip_prefix("Sequence[").and_then(|inner| inner.strip_suffix(']')) {
        return Some(normalize_type_text(inner));
    }

    for head in ["list", "tuple", "set", "frozenset"] {
        if let Some(inner) =
            text.strip_prefix(&format!("{head}[")).and_then(|inner| inner.strip_suffix(']'))
        {
            let args = split_top_level_type_args(inner);
            return args.first().map(|arg| normalize_type_text(arg));
        }
    }

    None
}

fn unwrap_for_iterable_type(text: &str) -> Option<String> {
    let text = normalize_type_text(text);

    if text == "range" {
        return Some(String::from("int"));
    }

    unwrap_yield_from_type(&text)
}

fn direct_member_access_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.member_accesses
        .iter()
        .filter_map(|access| {
            if !access.through_instance
                && let Some(module_node) =
                    resolve_imported_module_target(node, nodes, &access.owner_name)
            {
                let has_member = module_node.declarations.iter().any(|declaration| {
                    declaration.owner.is_none() && declaration.name == access.member
                });
                return (!has_member).then(|| {
                    Diagnostic::error(
                        "TPY4002",
                        format!(
                            "module `{}` in module `{}` has no member `{}`",
                            module_node.module_key,
                            node.module_path.display(),
                            access.member
                        ),
                    )
                });
            }

            let owner_type_name = resolve_member_access_owner_type(node, nodes, access)?;
            if let Some(branches) = union_branches(&owner_type_name) {
                let available = branches
                    .iter()
                    .filter(|branch| type_has_readable_member(node, nodes, branch, &access.member))
                    .cloned()
                    .collect::<Vec<_>>();
                if available.len() == branches.len() {
                    return None;
                }
                let mut diagnostic = Diagnostic::error(
                    "TPY4002",
                    format!(
                        "type `{}` in module `{}` has no member `{}` on every union branch",
                        owner_type_name,
                        node.module_path.display(),
                        access.member
                    ),
                );
                if let Some((span, replacement)) =
                    union_member_guard_suggestion(&node.module_path, access, &available)
                {
                    diagnostic = diagnostic.with_suggestion(
                        format!(
                            "Insert `isinstance` guard for `{}` before accessing `{}`",
                            access.owner_name, access.member
                        ),
                        span,
                        replacement,
                        SuggestionApplicability::MachineApplicable,
                    );
                }
                return Some(diagnostic);
            }
            let (class_node, class_decl) = resolve_direct_base(nodes, node, &owner_type_name)?;
            let has_member = find_owned_readable_member_declaration(
                nodes,
                class_node,
                class_decl,
                &access.member,
            )
            .is_some();

            (!has_member).then(|| {
                Diagnostic::error(
                    "TPY4002",
                    format!(
                        "type `{}` in module `{}` has no member `{}`",
                        class_decl.name,
                        node.module_path.display(),
                        access.member
                    ),
                )
            })
        })
        .collect()
}

fn type_has_readable_member(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
    member: &str,
) -> bool {
    let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, type_name) else {
        return false;
    };
    find_owned_readable_member_declaration(nodes, class_node, class_decl, member).is_some()
}

fn union_member_guard_suggestion(
    module_path: &std::path::Path,
    access: &typepython_binding::MemberAccessSite,
    available_branches: &[String],
) -> Option<(Span, String)> {
    let guard_types = available_branches
        .iter()
        .filter_map(|branch| isinstance_guard_type_name(branch))
        .collect::<Vec<_>>();
    if guard_types.is_empty() {
        return None;
    }
    let source = fs::read_to_string(module_path).ok()?;
    let line_text = source.lines().nth(access.line.checked_sub(1)?)?;
    let indent = leading_space_count(line_text);
    let guard = if guard_types.len() == 1 {
        guard_types[0].clone()
    } else {
        format!("({})", guard_types.join(", "))
    };
    Some((
        Span::new(module_path.display().to_string(), access.line, 1, access.line, 1),
        format!("{}assert isinstance({}, {})\n", " ".repeat(indent), access.owner_name, guard),
    ))
}

fn isinstance_guard_type_name(type_name: &str) -> Option<String> {
    let normalized = normalize_type_text(type_name);
    if normalized.is_empty()
        || matches!(normalized.as_str(), "None" | "dynamic" | "unknown")
        || normalized.contains('[')
        || normalized.contains('|')
    {
        return None;
    }
    Some(normalized)
}

fn direct_method_call_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let direct_method_signatures = context.load_direct_method_signatures(node);

    for call in &node.method_calls {
        if !call.through_instance
            && let Some(module_diagnostics) =
                imported_module_method_call_diagnostics(node, nodes, call)
        {
            diagnostics.extend(module_diagnostics);
            continue;
        }

        let Some(owner_type_name) = resolve_method_call_owner_type(context, node, nodes, call)
        else {
            continue;
        };
        let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &owner_type_name)
        else {
            continue;
        };
        let candidates =
            find_owned_callable_declarations(nodes, class_node, class_decl, &call.method);
        let Some(target) = candidates.first().copied() else {
            continue;
        };

        let direct_call = typepython_binding::CallSite {
            callee: format!("{}.{}", class_decl.name, call.method),
            arg_count: call.arg_count,
            arg_types: call.arg_types.clone(),
            arg_values: call.arg_values.clone(),
            starred_arg_types: call.starred_arg_types.clone(),
            starred_arg_values: call.starred_arg_values.clone(),
            keyword_names: call.keyword_names.clone(),
            keyword_arg_types: call.keyword_arg_types.clone(),
            keyword_arg_values: call.keyword_arg_values.clone(),
            keyword_expansion_types: call.keyword_expansion_types.clone(),
            keyword_expansion_values: call.keyword_expansion_values.clone(),
            line: 1,
        };

        let overloads = candidates
            .iter()
            .copied()
            .filter(|declaration| declaration.kind == DeclarationKind::Overload)
            .collect::<Vec<_>>();
        if !overloads.is_empty() {
            let applicable = overloads
                .iter()
                .copied()
                .filter(|declaration| {
                    method_overload_is_applicable(
                        node,
                        nodes,
                        &direct_call,
                        declaration,
                        &owner_type_name,
                    )
                })
                .collect::<Vec<_>>();
            if applicable.len() >= 2 {
                diagnostics.push(Diagnostic::error(
                    "TPY4012",
                    format!(
                        "call to `{}.{}` in module `{}` is ambiguous across {} overloads after applicability filtering",
                        class_decl.name,
                        call.method,
                        node.module_path.display(),
                        applicable.len()
                    ),
                ));
                continue;
            }
            if let Some(applicable) = applicable.first().copied() {
                let signature = direct_method_signature_sites(applicable, &owner_type_name);
                if let Some(diagnostic) = direct_source_function_arity_diagnostic_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &signature,
                ) {
                    diagnostics.push(diagnostic);
                }
                diagnostics.extend(direct_source_function_keyword_diagnostics_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &signature,
                ));
                diagnostics.extend(direct_source_function_type_diagnostics_with_context(
                    context,
                    node,
                    nodes,
                    &direct_call,
                    &signature,
                ));
                continue;
            }
        }

        if let Some(signature) =
            direct_method_signatures.get(&(class_decl.name.clone(), call.method.clone()))
        {
            if let Some(diagnostic) = direct_source_function_arity_diagnostic_with_context(
                context,
                node,
                nodes,
                &direct_call,
                signature,
            ) {
                diagnostics.push(diagnostic);
            }
            diagnostics.extend(direct_source_function_keyword_diagnostics_with_context(
                context,
                node,
                nodes,
                &direct_call,
                signature,
            ));
            diagnostics.extend(direct_source_function_type_diagnostics_with_context(
                context,
                node,
                nodes,
                &direct_call,
                signature,
            ));
            continue;
        }

        let method_signature = substitute_self_annotation(&target.detail, Some(&class_decl.name));
        let fallback_signature = direct_signature_params(&method_signature)
            .unwrap_or_default()
            .into_iter()
            .map(|param| typepython_syntax::DirectFunctionParamSite {
                name: param.name,
                annotation: (!param.annotation.is_empty()).then_some(param.annotation),
                has_default: param.has_default,
                positional_only: param.positional_only,
                keyword_only: param.keyword_only,
                variadic: param.variadic,
                keyword_variadic: param.keyword_variadic,
            })
            .collect::<Vec<_>>();
        let fallback_signature =
            match target.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
                typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => {
                    fallback_signature
                }
                typepython_syntax::MethodKind::Instance
                | typepython_syntax::MethodKind::Class
                | typepython_syntax::MethodKind::PropertySetter => {
                    fallback_signature.into_iter().skip(1).collect()
                }
            };
        if let Some(diagnostic) = direct_source_function_arity_diagnostic_with_context(
            context,
            node,
            nodes,
            &direct_call,
            &fallback_signature,
        ) {
            diagnostics.push(diagnostic);
        }
        diagnostics.extend(direct_source_function_keyword_diagnostics_with_context(
            context,
            node,
            nodes,
            &direct_call,
            &fallback_signature,
        ));
        diagnostics.extend(direct_source_function_type_diagnostics_with_context(
            context,
            node,
            nodes,
            &direct_call,
            &fallback_signature,
        ));
    }

    diagnostics
}

fn direct_method_signature_sites(
    declaration: &Declaration,
    owner_type_name: &str,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    let method_signature = substitute_self_annotation(&declaration.detail, Some(owner_type_name));
    let params = direct_signature_params(&method_signature).unwrap_or_default();
    let params = match declaration.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
        typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => params,
        typepython_syntax::MethodKind::Instance
        | typepython_syntax::MethodKind::Class
        | typepython_syntax::MethodKind::PropertySetter => params.into_iter().skip(1).collect(),
    };

    params
        .into_iter()
        .map(|param| typepython_syntax::DirectFunctionParamSite {
            name: param.name,
            annotation: (!param.annotation.is_empty()).then_some(param.annotation),
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect()
}

fn method_overload_is_applicable(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    declaration: &Declaration,
    owner_type_name: &str,
) -> bool {
    let method_signature = substitute_self_annotation(&declaration.detail, Some(owner_type_name));
    let params = direct_signature_params(&method_signature).unwrap_or_default();
    let params = match declaration.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
        typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => params,
        typepython_syntax::MethodKind::Instance
        | typepython_syntax::MethodKind::Class
        | typepython_syntax::MethodKind::PropertySetter => params.into_iter().skip(1).collect(),
    };
    call_signature_params_are_applicable(node, nodes, call, &params)
}

fn resolve_method_call_owner_type(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::MethodCallSite,
) -> Option<String> {
    if call.through_instance {
        return resolve_direct_callable_return_type(node, nodes, &call.owner_name)
            .map(|return_type| normalize_type_text(&return_type))
            .or_else(|| Some(call.owner_name.clone()));
    }

    resolve_direct_name_reference_type_with_context(
        context,
        node,
        nodes,
        None,
        None,
        None,
        None,
        call.line,
        &call.owner_name,
    )
    .or_else(|| Some(call.owner_name.clone()))
}

fn direct_call_arity_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.calls
        .iter()
        .filter_map(|call| {
            if let Some(shape) = resolve_synthesized_dataclass_class_shape(node, nodes, &call.callee)
                && !shape.has_explicit_init
            {
                return dataclass_transform_constructor_arity_diagnostic(node, call, &shape);
            }
            if let Some(signature) =
                resolve_direct_callable_signature_sites_with_context(
                    context,
                    node,
                    nodes,
                    &call.callee,
                )
            {
                return direct_source_function_arity_diagnostic_with_context(
                    context,
                    node,
                    nodes,
                    call,
                    &signature,
                );
            }
            let (expected, _) = resolve_direct_callable_signature(node, nodes, &call.callee)?;
            (call.arg_count != expected).then(|| {
                Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` expects {} positional argument(s) but received {}",
                        call.callee,
                        node.module_path.display(),
                        expected,
                        call.arg_count
                    ),
                )
            })
        })
        .collect()
}

fn direct_param_count(signature: &str) -> Option<usize> {
    Some(direct_signature_params(signature)?.len())
}

fn direct_param_names(signature: &str) -> Option<Vec<String>> {
    Some(direct_signature_params(signature)?.into_iter().map(|param| param.name).collect())
}

fn direct_param_types(signature: &str) -> Option<Vec<String>> {
    let inner = signature.strip_prefix('(')?.split_once(')')?.0;
    if inner.is_empty() {
        return Some(Vec::new());
    }

    Some(direct_signature_params(signature)?.into_iter().map(|param| param.annotation).collect())
}

fn direct_signature_params(signature: &str) -> Option<Vec<DirectSignatureParam>> {
    let inner = signature.strip_prefix('(')?.split_once(')')?.0;
    if inner.is_empty() {
        return Some(Vec::new());
    }

    let parts = split_top_level_type_args(inner);
    let slash_index = parts.iter().position(|part| part.trim() == "/");
    let star_index = parts.iter().position(|part| part.trim() == "*");
    let mut params = Vec::new();
    let mut keyword_only_active = false;
    for (index, part) in parts.into_iter().enumerate() {
        let part = part.trim();
        if part == "/" {
            continue;
        }
        if part == "*" {
            keyword_only_active = true;
            continue;
        }

        let has_default = part.ends_with('=');
        let part = part.trim_end_matches('=').trim();
        let (part, variadic, keyword_variadic) = if let Some(part) = part.strip_prefix("**") {
            (part.trim(), false, true)
        } else if let Some(part) = part.strip_prefix('*') {
            keyword_only_active = true;
            (part.trim(), true, false)
        } else {
            (part, false, false)
        };
        let (name, annotation) = part
            .split_once(':')
            .map(|(name, annotation)| (name.trim(), annotation.trim().to_owned()))
            .unwrap_or((part, String::new()));
        params.push(DirectSignatureParam {
            name: name.to_owned(),
            annotation,
            has_default,
            positional_only: slash_index.is_some_and(|slash_index| index < slash_index),
            keyword_only: !variadic
                && !keyword_variadic
                && (star_index.is_some_and(|star_index| index > star_index) || keyword_only_active),
            variadic,
            keyword_variadic,
        });
    }

    Some(params)
}

fn direct_call_type_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.calls
        .iter()
        .flat_map(|call| {
            if let Some(shape) =
                resolve_synthesized_dataclass_class_shape(node, nodes, &call.callee)
                && !shape.has_explicit_init
            {
                return dataclass_transform_constructor_type_diagnostics(
                    node,
                    nodes,
                    call,
                    &shape,
                );
            }
            if let Some(function) = resolve_direct_function(node, nodes, &call.callee)
                && let Some(signature) =
                    resolve_instantiated_direct_function_signature(node, nodes, function, call)
            {
                return direct_source_function_type_diagnostics_with_context(
                    context,
                    node,
                    nodes,
                    call,
                    &signature,
                );
            }
            if let Some(function) = resolve_direct_function(node, nodes, &call.callee)
                && function.type_params.iter().any(|type_param| {
                    type_param.kind == typepython_binding::GenericTypeParamKind::ParamSpec
                })
            {
                return vec![
                    Diagnostic::error(
                        "TPY4014",
                        format!(
                            "call to `{}` in module `{}` is invalid because generic parameter list of `{}` could not be resolved from this call",
                            call.callee, node.module_key, call.callee
                        ),
                    )
                    .with_span(Span::new(
                        node.module_path.display().to_string(),
                        call.line,
                        1,
                        call.line,
                        1,
                    )),
                ];
            }
            if let Some(signature) =
                resolve_direct_callable_signature_sites_with_context(context, node, nodes, &call.callee)
            {
                return direct_source_function_type_diagnostics_with_context(
                    context,
                    node,
                    nodes,
                    call,
                    &signature,
                );
            }
            let Some(param_types) = resolve_direct_callable_param_types(node, nodes, &call.callee)
            else {
                return Vec::new();
            };
            let param_names =
                direct_param_names_from_signature(node, nodes, &call.callee).unwrap_or_default();
            positional_and_keyword_type_diagnostics(
                node,
                nodes,
                call,
                call.arg_types.as_slice(),
                call.keyword_arg_types.as_slice(),
                &param_types,
                &param_names,
                None,
                None,
                None,
                &[],
                &[],
            )
        })
        .collect()
}

fn direct_call_keyword_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for call in &node.calls {
        if let Some(shape) = resolve_synthesized_dataclass_class_shape(node, nodes, &call.callee)
            && !shape.has_explicit_init
        {
            diagnostics
                .extend(dataclass_transform_constructor_keyword_diagnostics(node, call, &shape));
            continue;
        }
        if let Some(signature) =
            resolve_direct_callable_signature_sites_with_context(context, node, nodes, &call.callee)
        {
            diagnostics.extend(direct_source_function_keyword_diagnostics_with_context(
                context, node, nodes, call, &signature,
            ));
            continue;
        }
        let Some((_, param_names)) = resolve_direct_callable_signature(node, nodes, &call.callee)
        else {
            continue;
        };
        for keyword in &call.keyword_names {
            if !param_names.iter().any(|param| param == keyword) {
                diagnostics.push(Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` uses unknown keyword `{}`",
                        call.callee,
                        node.module_path.display(),
                        keyword
                    ),
                ));
            }
        }
    }

    diagnostics
}

fn direct_source_function_arity_diagnostic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Option<Diagnostic> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    direct_source_function_arity_diagnostic_with_context(&context, node, nodes, call, signature)
}

fn direct_source_function_arity_diagnostic_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Option<Diagnostic> {
    let positional_params = signature
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .collect::<Vec<_>>();
    let has_variadic = signature.iter().any(|param| param.variadic);
    let expected_positional_arg_types =
        expected_positional_arg_types_from_signature_sites(signature, call.arg_count);
    let (positional_types, variadic_starred_types) =
        expanded_positional_arg_types(node, nodes, call, &expected_positional_arg_types);
    if !has_variadic
        && (positional_types.len() > positional_params.len() || !variadic_starred_types.is_empty())
    {
        return Some(Diagnostic::error(
            "TPY4001",
            format!(
                "call to `{}` in module `{}` expects at most {} positional argument(s) but received {}",
                call.callee,
                node.module_path.display(),
                positional_params.len(),
                positional_types.len()
            ),
        ));
    }

    let provided_keywords = call.keyword_names.iter().collect::<BTreeSet<_>>();
    let keyword_expansions = resolved_keyword_expansions_with_context(context, node, nodes, call);
    let unpack_shape =
        unpack_typed_dict_shape_from_signature_with_context(context, node, nodes, signature);
    let missing = signature
        .iter()
        .enumerate()
        .filter(|(index, param)| {
            if param.has_default {
                return false;
            }
            if param.keyword_only {
                return !provided_keywords.contains(&param.name)
                    && !keyword_expansions.iter().any(|expansion| match expansion {
                        KeywordExpansion::TypedDict(shape) => {
                            shape.fields.get(&param.name).is_some_and(|field| field.required)
                        }
                        KeywordExpansion::Mapping(_) => false,
                    });
            }
            if param.variadic || param.keyword_variadic {
                return false;
            }
            *index >= positional_types.len()
                && !provided_keywords.contains(&param.name)
                && !keyword_expansions.iter().any(|expansion| match expansion {
                    KeywordExpansion::TypedDict(shape) => {
                        shape.fields.get(&param.name).is_some_and(|field| field.required)
                    }
                    KeywordExpansion::Mapping(_) => false,
                })
        })
        .map(|(_, param)| param.name.clone())
        .collect::<Vec<_>>();
    let mut missing = missing;
    if let Some(shape) = unpack_shape {
        missing.extend(
            shape
                .fields
                .iter()
                .filter(|(key, field)| field.required && !provided_keywords.contains(*key))
                .map(|(key, _)| key.clone()),
        );
    }
    (!missing.is_empty()).then(|| {
        Diagnostic::error(
            "TPY4001",
            format!(
                "call to `{}` in module `{}` is missing required argument(s): {}",
                call.callee,
                node.module_path.display(),
                missing.join(", ")
            ),
        )
    })
}

fn direct_source_function_keyword_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<Diagnostic> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    direct_source_function_keyword_diagnostics_with_context(&context, node, nodes, call, signature)
}

fn direct_source_function_keyword_diagnostics_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<Diagnostic> {
    let unpack_shape =
        unpack_typed_dict_shape_from_signature_with_context(context, node, nodes, signature);
    let keyword_variadic_annotation = signature
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| param.annotation.as_deref())
        .map(normalize_type_text);
    let param_names = signature
        .iter()
        .filter(|param| !param.keyword_variadic)
        .map(|param| param.name.as_str())
        .collect::<BTreeSet<_>>();
    let accepts_extra_keywords = keyword_variadic_annotation
        .as_deref()
        .is_some_and(|annotation| !annotation.starts_with("Unpack["))
        || unpack_shape.as_ref().is_some_and(|shape| shape.extra_items.is_some());
    let expected_positional_arg_types =
        expected_positional_arg_types_from_signature_sites(signature, call.arg_count);
    let (positional_types, _) =
        expanded_positional_arg_types(node, nodes, call, &expected_positional_arg_types);
    let keyword_expansions = resolved_keyword_expansions_with_context(context, node, nodes, call);
    let mut diagnostics = call.keyword_names
        .iter()
        .filter_map(|keyword| {
            let matching = signature.iter().find(|param| param.name == *keyword);
            Some(match matching {
                Some(param) if param.positional_only => Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` passes positional-only parameter `{}` as a keyword",
                        call.callee,
                        node.module_path.display(),
                        keyword
                    ),
                ),
                None if unpack_shape
                    .as_ref()
                    .is_some_and(|shape| shape.fields.contains_key(keyword.as_str())) =>
                {
                    return None;
                }
                None if unpack_shape.as_ref().is_some_and(|shape| {
                    !shape.fields.contains_key(keyword.as_str()) && shape.extra_items.is_none()
                }) => {
                    Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` uses unknown unpacked keyword `{}`",
                        call.callee,
                        node.module_path.display(),
                        keyword
                    ),
                )
                }
                None if !accepts_extra_keywords && !param_names.contains(keyword.as_str()) => {
                    Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` uses unknown keyword `{}`",
                        call.callee,
                        node.module_path.display(),
                        keyword
                    ),
                )
                }
                _ => return None,
            })
        })
        .collect::<Vec<_>>();

    let positional_param_names = signature
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .map(|param| param.name.as_str())
        .collect::<Vec<_>>();
    for keyword in &call.keyword_names {
        if positional_param_names
            .iter()
            .take(positional_types.len())
            .any(|name| *name == keyword.as_str())
        {
            diagnostics.push(Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` binds parameter `{}` both positionally and by keyword",
                    call.callee,
                    node.module_path.display(),
                    keyword
                ),
            ));
        }
    }

    diagnostics.extend(keyword_expansions.into_iter().flat_map(|expansion| match expansion {
        KeywordExpansion::TypedDict(shape) => shape
            .fields
            .iter()
            .filter_map(|(key, field)| {
                let duplicate = call.keyword_names.iter().any(|keyword| keyword == key)
                    || positional_param_names.iter().take(positional_types.len()).any(|name| *name == key.as_str());
                if duplicate {
                    return Some(Diagnostic::error(
                        "TPY4013",
                        format!(
                            "call to `{}` in module `{}` expands `**{}` with duplicate key `{}`",
                            call.callee,
                            node.module_path.display(),
                            shape.name,
                            key
                        ),
                    ));
                }
                match signature.iter().find(|param| param.name == *key) {
                    Some(param) if param.positional_only => Some(Diagnostic::error(
                        "TPY4013",
                        format!(
                            "call to `{}` in module `{}` cannot satisfy positional-only parameter `{}` via `**{}`",
                            call.callee,
                            node.module_path.display(),
                            key,
                            shape.name
                        ),
                    )),
                    Some(param) if !field.required && !param.has_default => Some(Diagnostic::error(
                        "TPY4013",
                        format!(
                            "call to `{}` in module `{}` cannot satisfy required parameter `{}` from optional TypedDict key in `**{}`",
                            call.callee,
                            node.module_path.display(),
                            key,
                            shape.name
                        ),
                    )),
                    None if !accepts_extra_keywords => Some(Diagnostic::error(
                        "TPY4013",
                        format!(
                            "call to `{}` in module `{}` uses unknown `**{}` key `{}`",
                            call.callee,
                            node.module_path.display(),
                            shape.name,
                            key
                        ),
                    )),
                    _ => None,
                }
            })
            .chain(
                (typed_dict_shape_has_unbounded_extra_keys(&shape) && !accepts_extra_keywords).then(
                    || {
                        Diagnostic::error(
                            "TPY4013",
                            format!(
                                "call to `{}` in module `{}` cannot expand open TypedDict `{}` because it may contain undeclared keys",
                                call.callee,
                                node.module_path.display(),
                                shape.name
                            ),
                        )
                    },
                ),
            )
            .chain(
                shape
                    .extra_items
                    .as_ref()
                    .into_iter()
                    .filter(|_| !accepts_extra_keywords)
                    .map(|extra| {
                        Diagnostic::error(
                            "TPY4013",
                            format!(
                                "call to `{}` in module `{}` cannot expand additional `**{}` keys of type `{}` without `**kwargs`",
                                call.callee,
                                node.module_path.display(),
                                shape.name,
                                extra.value_type
                            ),
                        )
                    }),
            )
            .collect::<Vec<_>>(),
        KeywordExpansion::Mapping(value_ty) => (!accepts_extra_keywords).then(|| {
            Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` cannot expand `**dict[str, {}]` without `**kwargs`",
                    call.callee,
                    node.module_path.display(),
                    value_ty
                ),
            )
        }).into_iter().collect(),
    }));

    diagnostics
}

fn keyword_duplicates_positional_arguments(
    call: &typepython_binding::CallSite,
    params: &[DirectSignatureParam],
) -> bool {
    let positional_param_names = params
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .map(|param| param.name.as_str())
        .collect::<Vec<_>>();
    call.keyword_names.iter().any(|keyword| {
        positional_param_names.iter().take(call.arg_count).any(|name| *name == keyword.as_str())
    })
}

fn direct_source_function_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<Diagnostic> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    direct_source_function_type_diagnostics_with_context(&context, node, nodes, call, signature)
}

fn direct_source_function_type_diagnostics_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<Diagnostic> {
    let expected_positional_arg_types =
        expected_positional_arg_types_from_signature_sites(signature, call.arg_count);
    let expected_keyword_arg_types =
        expected_keyword_arg_types_from_signature_sites(signature, &call.keyword_names);
    let mut diagnostics = call
        .arg_values
        .iter()
        .enumerate()
        .flat_map(|(index, metadata)| {
            resolve_contextual_call_arg_type(
                node,
                nodes,
                call.line,
                metadata,
                expected_positional_arg_types.get(index).and_then(|expected| expected.as_deref()),
            )
            .into_iter()
            .flat_map(|result| result.diagnostics)
        })
        .collect::<Vec<_>>();
    diagnostics.extend(call.keyword_arg_values.iter().enumerate().flat_map(|(index, metadata)| {
        resolve_contextual_call_arg_type(
            node,
            nodes,
            call.line,
            metadata,
            expected_keyword_arg_types.get(index).and_then(|expected| expected.as_deref()),
        )
        .into_iter()
        .flat_map(|result| result.diagnostics)
    }));
    let resolved_keyword_arg_types =
        resolved_keyword_arg_types(node, nodes, call, &expected_keyword_arg_types);
    let (expanded_arg_types, variadic_starred_types) =
        expanded_positional_arg_types(node, nodes, call, &expected_positional_arg_types);
    let keyword_expansions = resolved_keyword_expansions_with_context(context, node, nodes, call);
    let param_types = signature
        .iter()
        .filter(|param| !param.keyword_variadic)
        .map(|param| param.annotation.clone().unwrap_or_default())
        .collect::<Vec<_>>();
    let param_names = signature
        .iter()
        .filter(|param| !param.keyword_variadic)
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    let variadic_type =
        signature.iter().find(|param| param.variadic).and_then(|param| param.annotation.as_deref());
    let unpack_shape =
        unpack_typed_dict_shape_from_signature_with_context(context, node, nodes, signature);
    let keyword_variadic_type = signature
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| param.annotation.as_deref())
        .filter(|annotation| !normalize_type_text(annotation).starts_with("Unpack["));
    diagnostics.extend(positional_and_keyword_type_diagnostics(
        node,
        nodes,
        call,
        &expanded_arg_types,
        &resolved_keyword_arg_types,
        &param_types,
        &param_names,
        variadic_type,
        keyword_variadic_type,
        unpack_shape.as_ref(),
        &variadic_starred_types,
        &keyword_expansions,
    ));
    diagnostics
}

fn expanded_positional_arg_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<String>],
) -> (Vec<String>, Vec<String>) {
    let mut positional_types = resolved_call_arg_types(node, nodes, call, expected_types);
    if positional_types.len() < call.arg_count {
        positional_types
            .extend(std::iter::repeat_n(String::new(), call.arg_count - positional_types.len()));
    }
    let mut variadic_starred_types = Vec::new();
    for expansion in resolved_starred_positional_expansions(node, nodes, call) {
        match expansion {
            PositionalExpansion::Fixed(types) => positional_types.extend(types),
            PositionalExpansion::Variadic(element_type) => {
                variadic_starred_types.push(element_type)
            }
        }
    }
    (positional_types, variadic_starred_types)
}

fn resolved_call_arg_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<String>],
) -> Vec<String> {
    if call.arg_values.is_empty() {
        return call.arg_types.clone();
    }
    call.arg_values
        .iter()
        .enumerate()
        .map(|(index, metadata)| {
            resolve_contextual_call_arg_type(
                node,
                nodes,
                call.line,
                metadata,
                expected_types.get(index).and_then(|expected| expected.as_deref()),
            )
            .map(|result| result.actual_type)
            .or_else(|| {
                resolve_direct_expression_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .unwrap_or_else(|| call.arg_types.get(index).cloned().unwrap_or_default())
        })
        .collect()
}

fn resolved_keyword_arg_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    expected_types: &[Option<String>],
) -> Vec<String> {
    if call.keyword_arg_values.is_empty() {
        return call.keyword_arg_types.clone();
    }
    call.keyword_arg_values
        .iter()
        .enumerate()
        .map(|(index, metadata)| {
            resolve_contextual_call_arg_type(
                node,
                nodes,
                call.line,
                metadata,
                expected_types.get(index).and_then(|expected| expected.as_deref()),
            )
            .map(|result| result.actual_type)
            .or_else(|| {
                resolve_direct_expression_type_from_metadata(
                    node, nodes, None, None, None, call.line, metadata,
                )
            })
            .unwrap_or_else(|| call.keyword_arg_types.get(index).cloned().unwrap_or_default())
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn positional_and_keyword_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    arg_types: &[String],
    keyword_arg_types: &[String],
    param_types: &[String],
    param_names: &[String],
    variadic_type: Option<&str>,
    keyword_variadic_type: Option<&str>,
    unpack_shape: Option<&TypedDictShape>,
    variadic_starred_types: &[String],
    keyword_expansions: &[KeywordExpansion],
) -> Vec<Diagnostic> {
    let unpack_extra_items_type = unpack_shape
        .and_then(|shape| shape.extra_items.as_ref())
        .map(|extra| extra.value_type.as_str());
    let mut diagnostics = arg_types
        .iter()
        .take(param_types.len())
        .zip(param_types.iter())
        .filter(|(arg_ty, param_ty)| {
            !arg_ty.is_empty()
                && !param_ty.is_empty()
                && !direct_type_is_assignable(node, nodes, param_ty, arg_ty)
        })
        .map(|(arg_ty, param_ty)| {
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` where parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    param_ty
                ),
            );
            attach_type_mismatch_notes(diagnostic, node, nodes, param_ty, arg_ty)
        })
        .collect::<Vec<_>>();

    for arg_ty in arg_types.iter().skip(param_types.len()) {
        let Some(param_ty) = variadic_type else {
            break;
        };
        if !arg_ty.is_empty()
            && !param_ty.is_empty()
            && !direct_type_is_assignable(node, nodes, param_ty, arg_ty)
        {
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` where variadic parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    param_ty
                ),
            );
            diagnostics.push(attach_type_mismatch_notes(diagnostic, node, nodes, param_ty, arg_ty));
        }
    }

    for arg_ty in variadic_starred_types {
        let Some(param_ty) = variadic_type else {
            continue;
        };
        if !arg_ty.is_empty()
            && !param_ty.is_empty()
            && !direct_type_is_assignable(node, nodes, param_ty, arg_ty)
        {
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` expands `*args` element type `{}` where variadic parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    param_ty
                ),
            );
            diagnostics.push(attach_type_mismatch_notes(diagnostic, node, nodes, param_ty, arg_ty));
        }
    }

    for (keyword, arg_ty) in call.keyword_names.iter().zip(keyword_arg_types) {
        let Some(index) = param_names.iter().position(|param| param == keyword) else {
            if let Some(shape) = unpack_shape
                && let Some(field) = shape.fields.get(keyword.as_str())
            {
                if !arg_ty.is_empty()
                    && !field.value_type.is_empty()
                    && !direct_type_matches(node, nodes, &field.value_type, arg_ty)
                {
                    let diagnostic = Diagnostic::error(
                        "TPY4001",
                        format!(
                            "call to `{}` in module `{}` passes `{}` for unpacked keyword `{}` where TypedDict key expects `{}`",
                            call.callee,
                            node.module_path.display(),
                            arg_ty,
                            keyword,
                            field.value_type
                        ),
                    );
                    diagnostics.push(attach_type_mismatch_notes(
                        diagnostic,
                        node,
                        nodes,
                        &field.value_type,
                        arg_ty,
                    ));
                }
                continue;
            }
            let Some(param_ty) = unpack_extra_items_type.or(keyword_variadic_type) else {
                continue;
            };
            if !arg_ty.is_empty()
                && !param_ty.is_empty()
                && !direct_type_matches(node, nodes, param_ty, arg_ty)
            {
                let diagnostic = Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}` in module `{}` passes `{}` for keyword `{}` where variadic keyword parameter expects `{}`",
                        call.callee,
                        node.module_path.display(),
                        arg_ty,
                        keyword,
                        param_ty
                    ),
                );
                diagnostics
                    .push(attach_type_mismatch_notes(diagnostic, node, nodes, param_ty, arg_ty));
            }
            continue;
        };
        let Some(param_ty) = param_types.get(index) else {
            continue;
        };
        if !arg_ty.is_empty()
            && !param_ty.is_empty()
            && !direct_type_matches(node, nodes, param_ty, arg_ty)
        {
            let diagnostic = Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` for keyword `{}` where parameter expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    keyword,
                    param_ty
                ),
            );
            diagnostics.push(attach_type_mismatch_notes(diagnostic, node, nodes, param_ty, arg_ty));
        }
    }

    for expansion in keyword_expansions {
        match expansion {
            KeywordExpansion::TypedDict(shape) => {
                for (key, field) in &shape.fields {
                    if let Some(index) = param_names.iter().position(|param| param == key) {
                        let param_ty = &param_types[index];
                        if !field.value_type.is_empty()
                            && !param_ty.is_empty()
                            && !direct_type_is_assignable(node, nodes, param_ty, &field.value_type)
                        {
                            diagnostics.push(Diagnostic::error(
                                "TPY4013",
                                format!(
                                    "call to `{}` in module `{}` expands `**{}` key `{}` with type `{}` where parameter expects `{}`",
                                    call.callee,
                                    node.module_path.display(),
                                    shape.name,
                                    key,
                                    field.value_type,
                                    param_ty
                                ),
                            ));
                        }
                    } else if let Some(param_ty) = unpack_extra_items_type.or(keyword_variadic_type)
                    {
                        if !field.value_type.is_empty()
                            && !param_ty.is_empty()
                            && !direct_type_is_assignable(node, nodes, param_ty, &field.value_type)
                        {
                            diagnostics.push(Diagnostic::error(
                                "TPY4013",
                                format!(
                                    "call to `{}` in module `{}` expands `**{}` key `{}` with type `{}` where `**kwargs` expects `{}`",
                                    call.callee,
                                    node.module_path.display(),
                                    shape.name,
                                    key,
                                    field.value_type,
                                    param_ty
                                ),
                            ));
                        }
                    }
                }
                if let Some(extra_items) = &shape.extra_items {
                    if let Some(param_ty) = unpack_extra_items_type.or(keyword_variadic_type) {
                        if !extra_items.value_type.is_empty()
                            && !param_ty.is_empty()
                            && !direct_type_is_assignable(
                                node,
                                nodes,
                                param_ty,
                                &extra_items.value_type,
                            )
                        {
                            diagnostics.push(Diagnostic::error(
                                "TPY4013",
                                format!(
                                    "call to `{}` in module `{}` expands additional `**{}` keys of type `{}` where extra keywords expect `{}`",
                                    call.callee,
                                    node.module_path.display(),
                                    shape.name,
                                    extra_items.value_type,
                                    param_ty
                                ),
                            ));
                        }
                    }
                }
            }
            KeywordExpansion::Mapping(value_ty) => {
                if let Some(param_ty) = keyword_variadic_type
                    && !value_ty.is_empty()
                    && !param_ty.is_empty()
                    && !direct_type_is_assignable(node, nodes, param_ty, value_ty)
                {
                    diagnostics.push(Diagnostic::error(
                        "TPY4001",
                        format!(
                            "call to `{}` in module `{}` expands `**dict[str, {}]` where `**kwargs` expects `{}`",
                            call.callee,
                            node.module_path.display(),
                            value_ty,
                            param_ty
                        ),
                    ));
                }
            }
        }
    }

    diagnostics
}

fn unpack_typed_dict_shape_from_signature_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Option<TypedDictShape> {
    let annotation = signature
        .iter()
        .find(|param| param.keyword_variadic)
        .and_then(|param| param.annotation.as_deref())?;
    let annotation = normalize_type_text(annotation);
    let inner = annotation.strip_prefix("Unpack[")?.strip_suffix(']')?;
    resolve_known_typed_dict_shape_from_type_with_context(context, node, nodes, inner)
}

fn load_direct_init_signatures_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<String, Vec<typepython_syntax::DirectFunctionParamSite>> {
    context
        .load_direct_method_signatures(node)
        .into_iter()
        .filter(|((_, method_name), _)| method_name == "__init__")
        .map(|((owner_type_name, _), params)| (owner_type_name, params))
        .collect()
}

fn direct_signature_sites_from_detail(
    detail: &str,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    direct_signature_params(detail)
        .unwrap_or_default()
        .into_iter()
        .map(|param| typepython_syntax::DirectFunctionParamSite {
            name: param.name,
            annotation: (!param.annotation.is_empty()).then_some(param.annotation),
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect()
}

fn resolve_direct_callable_signature_sites(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    resolve_direct_callable_signature_sites_with_context(&context, node, nodes, callee)
}

fn resolve_direct_callable_signature_sites_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    if let Some(callable_annotation) =
        resolve_decorated_function_callable_annotation_with_context(context, node, nodes, callee)
    {
        return direct_function_signature_sites_from_callable_annotation(&callable_annotation);
    }
    let direct_function_signatures = context.load_direct_function_signatures(node);
    if let Some(signature) = direct_function_signatures.get(callee) {
        return Some(signature.clone());
    }

    let direct_init_signatures = load_direct_init_signatures_with_context(context, node);
    if let Some(signature) = direct_init_signatures.get(callee) {
        return Some(signature.clone());
    }

    if let Some(function) = resolve_direct_function(node, nodes, callee) {
        return Some(direct_signature_sites_from_detail(&function.detail));
    }

    if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, callee) {
        let init = class_node.declarations.iter().find(|declaration| {
            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
                && declaration.name == "__init__"
                && declaration.kind == DeclarationKind::Function
        })?;
        return Some(
            direct_signature_sites_from_detail(&init.detail).into_iter().skip(1).collect(),
        );
    }

    resolve_typing_callable_signature(callee).map(direct_signature_sites_from_detail)
}

fn direct_param_names_from_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<Vec<String>> {
    if let Some(local) = resolve_direct_function(node, nodes, callee) {
        return direct_param_names(&local.detail);
    }

    if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, callee) {
        let init = class_node.declarations.iter().find(|declaration| {
            declaration.kind == DeclarationKind::Function
                && declaration.name == "__init__"
                && declaration.owner.as_ref().is_some_and(|owner| {
                    owner.kind == DeclarationOwnerKind::Class && owner.name == class_decl.name
                })
        })?;

        return direct_param_names(&init.detail).map(|names| names.into_iter().skip(1).collect());
    }

    None
}

fn dataclass_transform_constructor_arity_diagnostic(
    node: &typepython_graph::ModuleNode,
    call: &typepython_binding::CallSite,
    shape: &DataclassTransformClassShape,
) -> Option<Diagnostic> {
    let positional_fields = shape.fields.iter().filter(|field| !field.kw_only).collect::<Vec<_>>();
    if call.arg_count > positional_fields.len() {
        return Some(Diagnostic::error(
            "TPY4001",
            format!(
                "call to `{}` in module `{}` expects at most {} positional argument(s) but received {}",
                call.callee,
                node.module_path.display(),
                positional_fields.len(),
                call.arg_count
            ),
        ));
    }

    let provided_keywords = call.keyword_names.iter().collect::<BTreeSet<_>>();
    let missing_required = shape
        .fields
        .iter()
        .enumerate()
        .filter(|(index, field)| {
            field.required
                && if field.kw_only {
                    !provided_keywords.contains(&field.keyword_name)
                } else {
                    *index >= call.arg_count && !provided_keywords.contains(&field.keyword_name)
                }
        })
        .map(|(_, field)| field.keyword_name.clone())
        .collect::<Vec<_>>();
    (!missing_required.is_empty()).then(|| {
        Diagnostic::error(
            "TPY4001",
            format!(
                "call to `{}` in module `{}` is missing required synthesized dataclass-transform field(s): {}",
                call.callee,
                node.module_path.display(),
                missing_required.join(", ")
            ),
        )
    })
}

fn dataclass_transform_constructor_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    shape: &DataclassTransformClassShape,
) -> Vec<Diagnostic> {
    let positional_fields = shape.fields.iter().filter(|field| !field.kw_only).collect::<Vec<_>>();
    let mut diagnostics = positional_fields
        .iter()
        .take(call.arg_count)
        .zip(call.arg_types.iter())
        .filter(|(field, arg_ty)| {
            !arg_ty.is_empty()
                && !field.annotation.is_empty()
                && !direct_type_matches(node, nodes, &field.annotation, arg_ty)
        })
        .map(|(field, arg_ty)| {
            Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` where synthesized dataclass-transform field `{}` expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    field.name,
                    field.annotation
                ),
            )
        })
        .collect::<Vec<_>>();

    for (keyword, arg_ty) in call.keyword_names.iter().zip(&call.keyword_arg_types) {
        let Some(field) = shape.fields.iter().find(|field| field.keyword_name == *keyword) else {
            continue;
        };
        if !arg_ty.is_empty()
            && !field.annotation.is_empty()
            && !direct_type_matches(node, nodes, &field.annotation, arg_ty)
        {
            diagnostics.push(Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` passes `{}` for synthesized keyword `{}` where field `{}` expects `{}`",
                    call.callee,
                    node.module_path.display(),
                    arg_ty,
                    keyword,
                    field.name,
                    field.annotation
                ),
            ));
        }
    }

    diagnostics
}

fn dataclass_transform_constructor_keyword_diagnostics(
    node: &typepython_graph::ModuleNode,
    call: &typepython_binding::CallSite,
    shape: &DataclassTransformClassShape,
) -> Vec<Diagnostic> {
    let valid_names =
        shape.fields.iter().map(|field| field.keyword_name.as_str()).collect::<BTreeSet<_>>();
    let positional_field_names = shape
        .fields
        .iter()
        .filter(|field| !field.kw_only)
        .map(|field| field.keyword_name.as_str())
        .collect::<Vec<_>>();
    let mut diagnostics = call.keyword_names
        .iter()
        .filter(|keyword| !valid_names.contains(keyword.as_str()))
        .map(|keyword| {
            Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` uses unknown synthesized dataclass-transform keyword `{}`",
                    call.callee,
                    node.module_path.display(),
                    keyword
                ),
            )
        })
        .collect::<Vec<_>>();

    for keyword in &call.keyword_names {
        if positional_field_names.iter().take(call.arg_count).any(|name| *name == keyword.as_str())
        {
            diagnostics.push(Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` binds synthesized field `{}` both positionally and by keyword",
                    call.callee,
                    node.module_path.display(),
                    keyword
                ),
            ));
        }
    }

    diagnostics
}

fn direct_unresolved_paramspec_call_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return Vec::new();
    };

    typepython_syntax::collect_direct_call_context_sites(&source)
        .into_iter()
        .filter_map(|call_site| {
            let owner_has_paramspec = resolve_scope_owner_declaration(
                node,
                call_site.owner_name.as_deref(),
                call_site.owner_type_name.as_deref(),
            )
            .is_some_and(|declaration| {
                declaration.type_params.iter().any(|type_param| {
                    type_param.kind == typepython_binding::GenericTypeParamKind::ParamSpec
                })
            });
            let signature = resolve_scope_owner_signature(
                node,
                call_site.owner_name.as_deref(),
                call_site.owner_type_name.as_deref(),
            );
            let callable_type = resolve_direct_name_reference_type(
                node,
                nodes,
                signature,
                None,
                call_site.owner_name.as_deref(),
                call_site.owner_type_name.as_deref(),
                call_site.line,
                &call_site.callee,
            )?;
            let callable_type = rewrite_imported_typing_aliases(node, &callable_type);
            if owner_has_paramspec && callable_has_unresolved_paramlist(&callable_type) {
                return None;
            }
            callable_has_unresolved_paramlist(&callable_type).then(|| {
                Diagnostic::error(
                    "TPY4014",
                    format!(
                        "call to `{}` in module `{}` is invalid because callable type `{}` still contains an unresolved ParamSpec or Concatenate tail",
                        call_site.callee,
                        node.module_path.display(),
                        callable_type
                    ),
                )
                .with_span(Span::new(
                    node.module_path.display().to_string(),
                    call_site.line,
                    1,
                    call_site.line,
                    1,
                ))
            })
        })
        .collect()
}

fn callable_has_unresolved_paramlist(text: &str) -> bool {
    let text = normalize_type_text(text);
    let Some(inner) = text.strip_prefix("Callable[").and_then(|inner| inner.strip_suffix(']'))
    else {
        return false;
    };
    let parts = split_top_level_type_args(inner);
    if parts.len() != 2 {
        return false;
    }

    callable_params_are_unresolved(parts[0])
}

fn callable_params_are_unresolved(params: &str) -> bool {
    let params = params.trim();
    if params == "..." || params.is_empty() {
        return false;
    }
    if params.starts_with('[') && params.ends_with(']') {
        return false;
    }
    if let Some(inner) =
        params.strip_prefix("Concatenate[").and_then(|inner| inner.strip_suffix(']'))
    {
        return split_top_level_type_args(inner)
            .last()
            .is_some_and(|tail| callable_params_are_unresolved(tail));
    }

    true
}

fn resolve_direct_callable_param_types(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<Vec<String>> {
    if let Some(local) = resolve_direct_function(node, nodes, callee) {
        return Some(direct_param_types(&local.detail).unwrap_or_default());
    }

    if let Some(shape) = resolve_synthesized_dataclass_class_shape(node, nodes, callee)
        && !shape.has_explicit_init
    {
        return Some(
            shape
                .fields
                .iter()
                .filter(|field| !field.kw_only)
                .map(|field| field.annotation.clone())
                .collect(),
        );
    }

    if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, callee) {
        let init = class_node.declarations.iter().find(|declaration| {
            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
                && declaration.name == "__init__"
                && declaration.kind == DeclarationKind::Function
        });
        let param_types = init
            .and_then(|declaration| direct_param_types(&declaration.detail))
            .unwrap_or_default();
        return Some(param_types.into_iter().skip(1).collect());
    }

    if let Some(signature) = resolve_typing_callable_signature(callee) {
        return direct_param_types(signature);
    }

    None
}

fn resolve_instantiated_direct_function_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    function: &Declaration,
    call: &typepython_binding::CallSite,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    if function.type_params.is_empty() {
        return None;
    }

    let signature = direct_signature_sites_from_detail(&function.detail);
    let substitutions =
        infer_generic_type_param_substitutions(node, nodes, function, &signature, call)?;
    instantiate_direct_function_signature(&signature, &substitutions)
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
struct GenericTypeParamSubstitutions {
    types: BTreeMap<String, String>,
    param_lists: BTreeMap<String, ParamListBinding>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ParamListBinding {
    params: Vec<typepython_syntax::DirectFunctionParamSite>,
}

fn infer_generic_type_param_substitutions(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    function: &Declaration,
    signature: &[typepython_syntax::DirectFunctionParamSite],
    call: &typepython_binding::CallSite,
) -> Option<GenericTypeParamSubstitutions> {
    let type_names = function
        .type_params
        .iter()
        .filter(|type_param| type_param.kind == typepython_binding::GenericTypeParamKind::TypeVar)
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();
    let param_spec_names = function
        .type_params
        .iter()
        .filter(|type_param| type_param.kind == typepython_binding::GenericTypeParamKind::ParamSpec)
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();
    let mut substitutions = GenericTypeParamSubstitutions::default();

    for (index, (param, actual)) in signature
        .iter()
        .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
        .zip(call.arg_types.iter())
        .enumerate()
    {
        let Some(annotation) = param.annotation.as_deref() else {
            continue;
        };
        let annotation_mentions_param_spec = parse_callable_annotation_parts(annotation)
            .is_some_and(|(params_expr, _)| {
                callable_param_expr_mentions_param_spec(&params_expr, &param_spec_names)
            });
        bind_callable_param_spec_type_params(
            node,
            nodes,
            annotation,
            actual,
            call.arg_values.get(index),
            &type_names,
            &param_spec_names,
            &mut substitutions,
        )?;
        if annotation_mentions_param_spec {
            continue;
        }
        bind_generic_type_params(
            node,
            nodes,
            annotation,
            actual,
            &type_names,
            &mut substitutions.types,
        )?;
    }

    for (index, (keyword, actual)) in
        call.keyword_names.iter().zip(&call.keyword_arg_types).enumerate()
    {
        let Some(param) = signature.iter().find(|param| param.name == *keyword) else {
            continue;
        };
        let Some(annotation) = param.annotation.as_deref() else {
            continue;
        };
        let annotation_mentions_param_spec = parse_callable_annotation_parts(annotation)
            .is_some_and(|(params_expr, _)| {
                callable_param_expr_mentions_param_spec(&params_expr, &param_spec_names)
            });
        bind_callable_param_spec_type_params(
            node,
            nodes,
            annotation,
            actual,
            call.keyword_arg_values.get(index),
            &type_names,
            &param_spec_names,
            &mut substitutions,
        )?;
        if annotation_mentions_param_spec {
            continue;
        }
        bind_generic_type_params(
            node,
            nodes,
            annotation,
            actual,
            &type_names,
            &mut substitutions.types,
        )?;
    }

    for type_param in &function.type_params {
        match type_param.kind {
            typepython_binding::GenericTypeParamKind::TypeVar => {
                if !substitutions.types.contains_key(&type_param.name)
                    && let Some(default) = &type_param.default
                {
                    substitutions
                        .types
                        .insert(type_param.name.clone(), normalize_type_text(default));
                }
                let Some(actual) = substitutions.types.get(&type_param.name) else {
                    continue;
                };
                if !generic_type_param_accepts_actual(node, nodes, type_param, actual) {
                    return None;
                }
            }
            typepython_binding::GenericTypeParamKind::ParamSpec => {
                if substitutions.param_lists.contains_key(&type_param.name) {
                    continue;
                }
                let Some(default) = &type_param.default else {
                    continue;
                };
                substitutions
                    .param_lists
                    .insert(type_param.name.clone(), param_list_binding_from_default(default)?);
            }
        }
    }

    Some(substitutions)
}

fn instantiate_direct_function_signature(
    signature: &[typepython_syntax::DirectFunctionParamSite],
    substitutions: &GenericTypeParamSubstitutions,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    let mut instantiated = Vec::new();
    let mut expanded_param_specs = BTreeSet::new();

    for param in signature {
        let param_spec_name = param.annotation.as_deref().and_then(|annotation| {
            if param.variadic {
                extract_param_spec_args_name(annotation)
            } else if param.keyword_variadic {
                extract_param_spec_kwargs_name(annotation)
            } else {
                None
            }
        });
        if let Some(param_spec_name) = param_spec_name {
            let binding = substitutions.param_lists.get(param_spec_name)?;
            if expanded_param_specs.insert(param_spec_name.to_owned()) {
                instantiated.extend(
                    binding
                        .params
                        .iter()
                        .cloned()
                        .map(|param| instantiate_direct_function_param(param, substitutions)),
                );
            }
            continue;
        }
        instantiated.push(instantiate_direct_function_param(param.clone(), substitutions));
    }

    Some(instantiated)
}

fn instantiate_direct_function_param(
    mut param: typepython_syntax::DirectFunctionParamSite,
    substitutions: &GenericTypeParamSubstitutions,
) -> typepython_syntax::DirectFunctionParamSite {
    param.annotation = param
        .annotation
        .as_deref()
        .map(|annotation| substitute_generic_type_params(annotation, substitutions));
    param
}

#[allow(clippy::too_many_arguments)]
fn bind_callable_param_spec_type_params(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &str,
    actual: &str,
    actual_value: Option<&typepython_syntax::DirectExprMetadata>,
    type_names: &BTreeSet<String>,
    param_spec_names: &BTreeSet<String>,
    substitutions: &mut GenericTypeParamSubstitutions,
) -> Option<()> {
    if param_spec_names.is_empty() {
        return Some(());
    }

    let Some((expected_params_expr, expected_return)) = parse_callable_annotation_parts(annotation)
    else {
        return Some(());
    };
    if !callable_param_expr_mentions_param_spec(&expected_params_expr, param_spec_names) {
        return Some(());
    }

    let (actual_binding, actual_return) =
        resolve_callable_shape_from_actual(node, nodes, actual, actual_value)?;
    bind_callable_param_expr(
        node,
        nodes,
        &expected_params_expr,
        &actual_binding,
        type_names,
        param_spec_names,
        substitutions,
    )?;
    bind_generic_type_params(
        node,
        nodes,
        &expected_return,
        &actual_return,
        type_names,
        &mut substitutions.types,
    )
}

fn callable_param_expr_mentions_param_spec(
    params_expr: &str,
    param_spec_names: &BTreeSet<String>,
) -> bool {
    if param_spec_names.contains(params_expr.trim()) {
        return true;
    }
    if let Some(inner) =
        params_expr.trim().strip_prefix("Concatenate[").and_then(|inner| inner.strip_suffix(']'))
    {
        return split_top_level_type_args(inner)
            .last()
            .is_some_and(|tail| param_spec_names.contains(tail.trim()));
    }
    false
}

fn bind_callable_param_expr(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_params_expr: &str,
    actual_binding: &ParamListBinding,
    type_names: &BTreeSet<String>,
    param_spec_names: &BTreeSet<String>,
    substitutions: &mut GenericTypeParamSubstitutions,
) -> Option<()> {
    let expected_params_expr = expected_params_expr.trim();
    if let Some(param_spec_name) =
        param_spec_names.contains(expected_params_expr).then_some(expected_params_expr)
    {
        return insert_param_spec_binding(substitutions, param_spec_name, actual_binding.clone());
    }

    if let Some(inner) =
        expected_params_expr.strip_prefix("Concatenate[").and_then(|inner| inner.strip_suffix(']'))
    {
        let parts = split_top_level_type_args(inner);
        let (tail, prefixes) = parts.split_last()?;
        let tail = tail.trim();
        if !param_spec_names.contains(tail) || actual_binding.params.len() < prefixes.len() {
            return None;
        }
        for (expected_prefix, actual_param) in prefixes.iter().zip(actual_binding.params.iter()) {
            let actual_annotation = actual_param
                .annotation
                .as_deref()
                .filter(|annotation| !annotation.is_empty())
                .unwrap_or("dynamic");
            bind_generic_type_params(
                node,
                nodes,
                expected_prefix,
                actual_annotation,
                type_names,
                &mut substitutions.types,
            )?;
        }
        return insert_param_spec_binding(
            substitutions,
            tail,
            ParamListBinding { params: actual_binding.params[prefixes.len()..].to_vec() },
        );
    }

    Some(())
}

fn insert_param_spec_binding(
    substitutions: &mut GenericTypeParamSubstitutions,
    name: &str,
    binding: ParamListBinding,
) -> Option<()> {
    match substitutions.param_lists.get(name) {
        Some(existing) if existing != &binding => None,
        Some(_) => Some(()),
        None => {
            substitutions.param_lists.insert(name.to_owned(), binding);
            Some(())
        }
    }
}

fn resolve_callable_shape_from_actual(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    actual: &str,
    actual_value: Option<&typepython_syntax::DirectExprMetadata>,
) -> Option<(ParamListBinding, String)> {
    if let Some(actual_value) = actual_value
        && let Some(shape) = resolve_callable_shape_from_metadata(node, nodes, actual_value, actual)
    {
        return Some(shape);
    }

    let (params, return_type) = parse_callable_annotation(actual)?;
    Some((ParamListBinding { params: synthesize_param_list_binding(params?) }, return_type))
}

fn resolve_callable_shape_from_metadata(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    actual_value: &typepython_syntax::DirectExprMetadata,
    actual: &str,
) -> Option<(ParamListBinding, String)> {
    if let Some(lambda) = actual_value.value_lambda.as_deref() {
        let (param_types, return_type) = parse_callable_annotation(actual)?;
        let param_types = param_types?;
        if param_types.len() != lambda.params.len() {
            return None;
        }
        let params = lambda
            .params
            .iter()
            .zip(param_types)
            .map(|(param, annotation)| typepython_syntax::DirectFunctionParamSite {
                name: param.name.clone(),
                annotation: Some(annotation),
                has_default: param.has_default,
                positional_only: param.positional_only,
                keyword_only: param.keyword_only,
                variadic: param.variadic,
                keyword_variadic: param.keyword_variadic,
            })
            .collect();
        return Some((ParamListBinding { params }, return_type));
    }

    let function_name = actual_value.value_name.as_deref()?;
    if let Some(callable_annotation) =
        resolve_decorated_function_callable_annotation(node, nodes, function_name)
    {
        let signature =
            direct_function_signature_sites_from_callable_annotation(&callable_annotation)?;
        let return_type =
            decorated_function_return_type_from_callable_annotation(&callable_annotation)?;
        return Some((ParamListBinding { params: signature }, return_type));
    }
    let function = resolve_direct_function(node, nodes, function_name)?;
    let return_type = function.detail.split_once("->")?.1.trim().to_owned();
    Some((
        ParamListBinding { params: direct_signature_sites_from_detail(&function.detail) },
        return_type,
    ))
}

fn synthesize_param_list_binding(
    param_types: Vec<String>,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    param_types
        .into_iter()
        .enumerate()
        .map(|(index, annotation)| typepython_syntax::DirectFunctionParamSite {
            name: format!("arg{index}"),
            annotation: Some(annotation),
            has_default: false,
            positional_only: false,
            keyword_only: false,
            variadic: false,
            keyword_variadic: false,
        })
        .collect()
}

fn param_list_binding_from_default(default: &str) -> Option<ParamListBinding> {
    let default = normalize_callable_param_expr(default);
    if default == "..." {
        return Some(ParamListBinding { params: Vec::new() });
    }
    if let Some(inner) = default.strip_prefix('[').and_then(|inner| inner.strip_suffix(']')) {
        let params = if inner.trim().is_empty() {
            Vec::new()
        } else {
            synthesize_param_list_binding(
                split_top_level_type_args(inner).into_iter().map(normalize_type_text).collect(),
            )
        };
        return Some(ParamListBinding { params });
    }
    None
}

fn extract_param_spec_args_name(annotation: &str) -> Option<&str> {
    annotation.strip_suffix(".args").map(str::trim).filter(|name| !name.is_empty())
}

fn extract_param_spec_kwargs_name(annotation: &str) -> Option<&str> {
    annotation.strip_suffix(".kwargs").map(str::trim).filter(|name| !name.is_empty())
}

fn generic_type_param_accepts_actual(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_param: &typepython_binding::GenericTypeParam,
    actual: &str,
) -> bool {
    if actual.is_empty() {
        return true;
    }
    if let Some(bound) = &type_param.bound {
        return direct_type_is_assignable(node, nodes, bound, actual);
    }
    if !type_param.constraints.is_empty() {
        return type_param
            .constraints
            .iter()
            .any(|constraint| direct_type_is_assignable(node, nodes, constraint, actual));
    }
    true
}

fn bind_generic_type_params(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &str,
    actual: &str,
    generic_names: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, String>,
) -> Option<()> {
    let inferred = infer_generic_type_param_bindings(
        node,
        nodes,
        annotation,
        actual,
        generic_names,
        substitutions,
    )?;
    for (name, actual_type) in inferred {
        match substitutions.get(&name) {
            Some(existing) if existing != &actual_type => {
                substitutions.insert(name, merge_generic_type_candidates(existing, &actual_type));
            }
            Some(_) => {}
            None => {
                substitutions.insert(name, actual_type);
            }
        }
    }
    Some(())
}

fn infer_generic_type_param_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &str,
    actual: &str,
    generic_names: &BTreeSet<String>,
    substitutions: &BTreeMap<String, String>,
) -> Option<BTreeMap<String, String>> {
    let annotation = normalize_type_text(annotation);
    let actual = normalize_type_text(actual);
    if actual.is_empty() {
        return Some(BTreeMap::new());
    }

    if generic_names.contains(&annotation) {
        let candidate = substitutions
            .get(&annotation)
            .map(|existing| merge_generic_type_candidates(existing, &actual))
            .unwrap_or(actual);
        let mut inferred = BTreeMap::new();
        inferred.insert(annotation, candidate);
        return Some(inferred);
    }

    if let Some(branches) = union_branches(&actual)
        && branches.len() > 1
    {
        let mut candidates = Vec::new();
        for branch in branches {
            let candidate = infer_generic_type_param_bindings(
                node,
                nodes,
                &annotation,
                &branch,
                generic_names,
                substitutions,
            )?;
            let combined = combine_generic_substitutions(substitutions, &candidate);
            let substituted_annotation = substitute_type_substitutions(&annotation, &combined);
            if !direct_type_is_assignable(node, nodes, &substituted_annotation, &branch) {
                return None;
            }
            candidates.push(candidate);
        }
        return merge_union_branch_bindings(candidates);
    }

    if let Some(branches) = union_branches(&annotation)
        && branches.len() > 1
    {
        let candidates = branches
            .into_iter()
            .filter_map(|branch| {
                let candidate = infer_generic_type_param_bindings(
                    node,
                    nodes,
                    &branch,
                    &actual,
                    generic_names,
                    substitutions,
                )?;
                let combined = combine_generic_substitutions(substitutions, &candidate);
                let substituted_branch = substitute_type_substitutions(&branch, &combined);
                direct_type_is_assignable(node, nodes, &substituted_branch, &actual)
                    .then_some(candidate)
            })
            .collect::<Vec<_>>();
        return select_best_union_branch_binding(candidates);
    }

    match (split_generic_type(&annotation), split_generic_type(&actual)) {
        (Some((expected_head, expected_args)), Some((actual_head, actual_args)))
            if expected_head == actual_head && expected_args.len() == actual_args.len() =>
        {
            let mut inferred: BTreeMap<String, String> = BTreeMap::new();
            for (expected_arg, actual_arg) in expected_args.iter().zip(actual_args.iter()) {
                let nested = infer_generic_type_param_bindings(
                    node,
                    nodes,
                    expected_arg,
                    actual_arg,
                    generic_names,
                    substitutions,
                )?;
                for (name, actual_type) in nested {
                    match inferred.get(&name) {
                        Some(existing) if existing != &actual_type => {
                            let merged = merge_generic_type_candidates(existing, &actual_type);
                            inferred.insert(name, merged);
                        }
                        Some(_) => {}
                        None => {
                            inferred.insert(name, actual_type);
                        }
                    }
                }
            }
            Some(inferred)
        }
        _ => {
            direct_type_is_assignable(node, nodes, &annotation, &actual).then_some(BTreeMap::new())
        }
    }
}

fn combine_generic_substitutions(
    existing: &BTreeMap<String, String>,
    inferred: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut combined = existing.clone();
    combined.extend(inferred.clone());
    combined
}

fn select_best_union_branch_binding(
    candidates: Vec<BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    let min_len = candidates.iter().map(BTreeMap::len).min()?;
    let mut filtered = candidates.into_iter().filter(|candidate| candidate.len() == min_len);
    let first = filtered.next()?;
    if filtered.all(|candidate| candidate == first) { Some(first) } else { None }
}

fn merge_union_branch_bindings(
    candidates: Vec<BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    let mut merged: BTreeMap<String, String> = BTreeMap::new();
    for candidate in candidates {
        for (name, actual_type) in candidate {
            match merged.get(&name) {
                Some(existing) if existing == &actual_type => {}
                Some(existing) => {
                    merged.insert(name, join_type_candidates(vec![existing.clone(), actual_type]));
                }
                None => {
                    merged.insert(name, actual_type);
                }
            }
        }
    }
    Some(merged)
}

fn merge_generic_type_candidates(existing: &str, actual: &str) -> String {
    if existing == actual {
        existing.to_owned()
    } else {
        join_type_candidates(vec![existing.to_owned(), actual.to_owned()])
    }
}

fn substitute_type_substitutions(
    annotation: &str,
    substitutions: &BTreeMap<String, String>,
) -> String {
    let mut output = String::new();
    let mut token = String::new();
    for character in annotation.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            token.push(character);
            continue;
        }
        if !token.is_empty() {
            output.push_str(substitutions.get(&token).map(String::as_str).unwrap_or(&token));
            token.clear();
        }
        output.push(character);
    }
    if !token.is_empty() {
        output.push_str(substitutions.get(&token).map(String::as_str).unwrap_or(&token));
    }
    output
}

fn substitute_generic_type_params(
    annotation: &str,
    substitutions: &GenericTypeParamSubstitutions,
) -> String {
    if let Some((params_expr, return_type)) = parse_callable_annotation_parts(annotation) {
        let substituted_params = substitute_callable_param_expr(&params_expr, substitutions);
        let substituted_return = substitute_generic_type_params(&return_type, substitutions);
        return format!("Callable[{substituted_params}, {substituted_return}]");
    }

    substitute_type_substitutions(annotation, &substitutions.types)
}

fn substitute_callable_param_expr(
    params_expr: &str,
    substitutions: &GenericTypeParamSubstitutions,
) -> String {
    let params_expr = params_expr.trim();
    if params_expr == "..." || params_expr.is_empty() {
        return params_expr.to_owned();
    }
    if let Some(binding) = substitutions.param_lists.get(params_expr) {
        return render_param_list_binding_for_callable(binding, substitutions);
    }
    if let Some(inner) =
        params_expr.strip_prefix("Concatenate[").and_then(|inner| inner.strip_suffix(']'))
    {
        let parts = split_top_level_type_args(inner);
        if let Some((tail, prefixes)) = parts.split_last()
            && let Some(binding) = substitutions.param_lists.get(tail.trim())
        {
            let mut rendered = prefixes
                .iter()
                .map(|part| substitute_generic_type_params(part, substitutions))
                .collect::<Vec<_>>();
            rendered.extend(binding.params.iter().map(|param| {
                param
                    .annotation
                    .as_deref()
                    .map(|annotation| substitute_generic_type_params(annotation, substitutions))
                    .unwrap_or_else(|| String::from("dynamic"))
            }));
            return format!("[{}]", rendered.join(", "));
        }
    }
    if let Some(inner) = params_expr.strip_prefix('[').and_then(|inner| inner.strip_suffix(']')) {
        let rendered = if inner.trim().is_empty() {
            Vec::new()
        } else {
            split_top_level_type_args(inner)
                .into_iter()
                .map(|part| substitute_generic_type_params(part, substitutions))
                .collect::<Vec<_>>()
        };
        return format!("[{}]", rendered.join(", "));
    }

    params_expr.to_owned()
}

fn render_param_list_binding_for_callable(
    binding: &ParamListBinding,
    substitutions: &GenericTypeParamSubstitutions,
) -> String {
    let rendered = binding
        .params
        .iter()
        .map(|param| {
            param
                .annotation
                .as_deref()
                .map(|annotation| substitute_generic_type_params(annotation, substitutions))
                .unwrap_or_else(|| String::from("dynamic"))
        })
        .collect::<Vec<_>>();
    format!("[{}]", rendered.join(", "))
}

fn resolve_direct_function_with_node<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    resolve_function_provider_with_node(nodes, node, callee)
}

fn resolve_direct_function<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Option<&'a Declaration> {
    resolve_direct_function_with_node(node, nodes, callee).map(|(_, declaration)| declaration)
}

fn resolve_function_provider_with_node<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    name: &str,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    if let Some(local) = node.declarations.iter().find(|declaration| {
        declaration.owner.is_none()
            && declaration.kind == DeclarationKind::Function
            && declaration.name == name
    }) {
        return Some((node, local));
    }

    if let Some((module_path, symbol_name)) = name.rsplit_once('.') {
        if let Some(target_node) =
            nodes.iter().find(|candidate| candidate.module_key == module_path)
            && let Some(target) = target_node.declarations.iter().find(|declaration| {
                declaration.owner.is_none()
                    && declaration.kind == DeclarationKind::Function
                    && declaration.name == symbol_name
            })
        {
            return Some((target_node, target));
        }

        if let Some((head, tail)) = module_path.split_once('.')
            && let Some(import) = node.declarations.iter().find(|declaration| {
                declaration.kind == DeclarationKind::Import && declaration.name == head
            })
        {
            let resolved_module = format!("{}.{}", import.detail, tail);
            if let Some(target_node) =
                nodes.iter().find(|candidate| candidate.module_key == resolved_module)
                && let Some(target) = target_node.declarations.iter().find(|declaration| {
                    declaration.owner.is_none()
                        && declaration.kind == DeclarationKind::Function
                        && declaration.name == symbol_name
                })
            {
                return Some((target_node, target));
            }
        }
    }

    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == name
    })?;
    let (module_key, symbol_name) = import.detail.rsplit_once('.')?;
    let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
    target_node
        .declarations
        .iter()
        .find(|declaration| {
            declaration.owner.is_none()
                && declaration.kind == DeclarationKind::Function
                && declaration.name == symbol_name
        })
        .map(|declaration| (target_node, declaration))
}

fn resolve_decorated_callable_site_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
) -> Option<typepython_syntax::DecoratedCallableSite> {
    let info = context.load_decorator_transform_module_info(node)?;
    info.callables.into_iter().find(|site| {
        site.name == declaration.name
            && site.owner_type_name.as_deref()
                == declaration.owner.as_ref().map(|owner| owner.name.as_str())
    })
}

fn callable_annotation_from_signature_sites_in_module(
    node: &typepython_graph::ModuleNode,
    signature: &[typepython_syntax::DirectFunctionParamSite],
    return_type: &str,
) -> String {
    let param_types = signature
        .iter()
        .map(|param| {
            param
                .annotation
                .as_deref()
                .map(|annotation| rewrite_imported_typing_aliases(node, annotation))
                .unwrap_or_else(|| String::from("dynamic"))
        })
        .collect::<Vec<_>>();
    let return_type = rewrite_imported_typing_aliases(node, return_type);
    format_callable_annotation(&param_types, &return_type)
}

fn synthetic_direct_expr_metadata(value_type: &str) -> typepython_syntax::DirectExprMetadata {
    typepython_syntax::DirectExprMetadata {
        value_type: Some(value_type.to_owned()),
        is_awaited: false,
        value_callee: None,
        value_name: None,
        value_member_owner_name: None,
        value_member_name: None,
        value_member_through_instance: false,
        value_method_owner_name: None,
        value_method_name: None,
        value_method_through_instance: false,
        value_subscript_target: None,
        value_subscript_string_key: None,
        value_subscript_index: None,
        value_if_true: None,
        value_if_false: None,
        value_if_guard: None,
        value_bool_left: None,
        value_bool_right: None,
        value_binop_left: None,
        value_binop_right: None,
        value_binop_operator: None,
        value_lambda: None,
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None,
    }
}

fn synthetic_decorator_application_call(
    decorator_name: &str,
    callable_annotation: &str,
) -> typepython_binding::CallSite {
    typepython_binding::CallSite {
        callee: decorator_name.to_owned(),
        arg_count: 1,
        arg_types: vec![callable_annotation.to_owned()],
        arg_values: vec![synthetic_direct_expr_metadata(callable_annotation)],
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line: 1,
    }
}

fn decorator_transform_accepts_callable_annotation_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> bool {
    direct_source_function_arity_diagnostic_with_context(context, node, nodes, call, signature)
        .is_none()
        && direct_source_function_keyword_diagnostics_with_context(
            context, node, nodes, call, signature,
        )
        .is_empty()
        && direct_source_function_type_diagnostics_with_context(
            context, node, nodes, call, signature,
        )
        .is_empty()
}

#[cfg(test)]
fn apply_named_callable_decorator_transform(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    decorator_name: &str,
    current_callable: &str,
) -> Option<String> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    apply_named_callable_decorator_transform_with_context(
        &context,
        node,
        nodes,
        decorator_name,
        current_callable,
    )
}

fn apply_named_callable_decorator_transform_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    decorator_name: &str,
    current_callable: &str,
) -> Option<String> {
    let (decorator_node, decorator) =
        resolve_function_provider_with_node(nodes, node, decorator_name)?;
    let call = synthetic_decorator_application_call(decorator_name, current_callable);
    let signature = if decorator.type_params.is_empty() {
        direct_signature_sites_from_detail(&decorator.detail)
    } else {
        resolve_instantiated_direct_function_signature(decorator_node, nodes, decorator, &call)?
    };
    if !decorator_transform_accepts_callable_annotation_with_context(
        context,
        decorator_node,
        nodes,
        &call,
        &signature,
    ) {
        return None;
    }

    let transformed = if decorator.type_params.is_empty() {
        rewrite_imported_typing_aliases(decorator_node, decorator.detail.split_once("->")?.1.trim())
    } else {
        let instantiated = resolve_instantiated_callable_return_type_from_declaration(
            decorator_node,
            nodes,
            decorator,
            &call,
        )?;
        rewrite_imported_typing_aliases(decorator_node, &instantiated)
    };
    let (params, return_type) = parse_callable_annotation(&transformed)?;
    Some(format_callable_annotation(&params?, &return_type))
}

fn resolve_decorated_callable_annotation_for_declaration(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
) -> Option<String> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    resolve_decorated_callable_annotation_for_declaration_with_context(
        &context,
        node,
        nodes,
        declaration,
    )
}

fn resolve_decorated_callable_annotation_for_declaration_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
) -> Option<String> {
    let decorated = resolve_decorated_callable_site_with_context(context, node, declaration)?;
    if decorated.decorators.is_empty() {
        return None;
    }

    let base_signature = direct_signature_sites_from_detail(&declaration.detail);
    let base_return = if declaration.is_async {
        format!("Awaitable[{}]", declaration.detail.split_once("->")?.1.trim())
    } else {
        declaration.detail.split_once("->")?.1.trim().to_owned()
    };
    let mut current =
        callable_annotation_from_signature_sites_in_module(node, &base_signature, &base_return);
    for decorator in decorated.decorators.iter().rev() {
        current = apply_named_callable_decorator_transform_with_context(
            context, node, nodes, decorator, &current,
        )?;
    }
    Some(current)
}

fn resolve_decorated_function_callable_annotation(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<String> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown);
    resolve_decorated_function_callable_annotation_with_context(&context, node, nodes, callee)
}

fn resolve_decorated_function_callable_annotation_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<String> {
    let (function_node, function) = resolve_direct_function_with_node(node, nodes, callee)?;
    resolve_decorated_callable_annotation_for_declaration_with_context(
        context,
        function_node,
        nodes,
        function,
    )
}

fn direct_function_signature_sites_from_callable_annotation(
    callable_annotation: &str,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    let (params, _return_type) = parse_callable_annotation(callable_annotation)?;
    Some(synthesize_param_list_binding(params?).into_iter().collect())
}

fn decorated_function_return_type_from_callable_annotation(
    callable_annotation: &str,
) -> Option<String> {
    parse_callable_annotation(callable_annotation).map(|(_, return_type)| return_type)
}

fn resolve_direct_callable_return_type<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Option<String> {
    if let Some(callable_annotation) =
        resolve_decorated_function_callable_annotation(node, nodes, callee)
    {
        return decorated_function_return_type_from_callable_annotation(&callable_annotation);
    }
    if let Some(function) = resolve_direct_function(node, nodes, callee) {
        let return_type = substitute_self_annotation(
            function.detail.split_once("->")?.1.trim(),
            function.owner.as_ref().map(|owner| owner.name.as_str()),
        );
        return Some(if function.is_async && !return_type.is_empty() {
            format!("Awaitable[{return_type}]")
        } else {
            return_type
        });
    }

    if let Some((_, class_decl)) = resolve_direct_base(nodes, node, callee) {
        return Some(class_decl.name.clone());
    }

    if let Some(signature) = resolve_typing_callable_signature(callee) {
        return Some(signature.split_once("->")?.1.trim().to_owned());
    }

    resolve_builtin_return_type(callee).map(str::to_owned)
}

fn resolve_instantiated_callable_return_type_from_declaration(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    declaration: &Declaration,
    call: &typepython_binding::CallSite,
) -> Option<String> {
    if declaration.type_params.is_empty() {
        return Some(declaration.detail.split_once("->")?.1.trim().to_owned());
    }
    let signature = direct_signature_sites_from_detail(&declaration.detail);
    let substitutions =
        infer_generic_type_param_substitutions(node, nodes, declaration, &signature, call)?;
    Some(substitute_generic_type_params(
        declaration.detail.split_once("->")?.1.trim(),
        &substitutions,
    ))
}

fn resolve_direct_callable_return_type_for_line(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
    line: usize,
) -> Option<String> {
    let call = node
        .calls
        .iter()
        .find(|call| call.callee == callee && call.line == line)
        .or_else(|| node.calls.iter().find(|call| call.callee == callee))?;
    let overloads = resolve_direct_overloads(node, nodes, callee);
    if !overloads.is_empty() {
        let applicable = overloads
            .into_iter()
            .filter(|declaration| {
                overload_is_applicable_with_context(node, nodes, call, declaration)
            })
            .collect::<Vec<_>>();
        let selected = select_most_specific_overload(node, nodes, &applicable)?;
        return resolve_instantiated_callable_return_type_from_declaration(
            node, nodes, selected, call,
        );
    }
    if let Some(callable_annotation) =
        resolve_decorated_function_callable_annotation(node, nodes, callee)
    {
        return decorated_function_return_type_from_callable_annotation(&callable_annotation);
    }
    let function = resolve_direct_function(node, nodes, callee)?;
    resolve_instantiated_callable_return_type_from_declaration(node, nodes, function, call)
}

fn resolve_direct_callable_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<(usize, Vec<String>)> {
    if let Some(signature) = resolve_direct_callable_signature_sites(node, nodes, callee) {
        return Some((
            signature
                .iter()
                .filter(|param| !param.keyword_only && !param.variadic && !param.keyword_variadic)
                .count(),
            signature.iter().map(|param| param.name.clone()).collect(),
        ));
    }
    if let Some(local) = resolve_direct_function(node, nodes, callee) {
        return Some((
            direct_param_count(&local.detail).unwrap_or_default(),
            direct_param_names(&local.detail).unwrap_or_default(),
        ));
    }

    if let Some(shape) = resolve_synthesized_dataclass_class_shape(node, nodes, callee)
        && !shape.has_explicit_init
    {
        return Some((
            shape.fields.iter().filter(|field| !field.kw_only).count(),
            shape.fields.iter().map(|field| field.keyword_name.clone()).collect(),
        ));
    }

    if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, callee) {
        let init = class_node.declarations.iter().find(|declaration| {
            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
                && declaration.name == "__init__"
                && declaration.kind == DeclarationKind::Function
        });
        let param_names = init
            .and_then(|declaration| direct_param_names(&declaration.detail))
            .unwrap_or_default();
        let arg_count = param_names.len().saturating_sub(1);
        return Some((arg_count, param_names.into_iter().skip(1).collect()));
    }

    if let Some(signature) = resolve_typing_callable_signature(callee) {
        return Some((
            direct_param_count(signature).unwrap_or_default(),
            direct_param_names(signature).unwrap_or_default(),
        ));
    }

    let function = resolve_direct_function(node, nodes, callee)?;
    Some((
        direct_param_count(&function.detail).unwrap_or_default(),
        direct_param_names(&function.detail).unwrap_or_default(),
    ))
}

fn resolve_synthesized_dataclass_class_shape(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<DataclassTransformClassShape> {
    resolve_dataclass_transform_class_shape(node, nodes, callee)
        .or_else(|| resolve_plain_dataclass_class_shape(node, nodes, callee))
}

fn resolve_plain_dataclass_class_shape(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<DataclassTransformClassShape> {
    let (class_node, class_decl) = resolve_direct_base(nodes, node, callee)?;
    resolve_plain_dataclass_class_shape_from_decl(
        nodes,
        class_node,
        class_decl,
        &mut BTreeSet::new(),
    )
}

fn resolve_plain_dataclass_class_shape_from_decl(
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visiting: &mut BTreeSet<(String, String)>,
) -> Option<DataclassTransformClassShape> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visiting.insert(key) {
        return None;
    }

    let info = load_dataclass_transform_module_info(class_node)?;
    let class_site = info.classes.iter().find(|class_site| class_site.name == class_decl.name)?;
    let is_plain_dataclass = class_decl.class_kind == Some(DeclarationOwnerKind::DataClass)
        || class_site
            .decorators
            .iter()
            .any(|decorator| matches!(decorator.as_str(), "dataclass" | "dataclasses.dataclass"));
    if !is_plain_dataclass {
        return None;
    }

    let has_explicit_init = !class_site.plain_dataclass_init
        || class_site.methods.iter().any(|method| method == "__init__");

    let mut fields = Vec::new();
    for base in &class_site.bases {
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) else {
            continue;
        };
        let mut branch_visiting = visiting.clone();
        let inherited = resolve_plain_dataclass_class_shape_from_decl(
            nodes,
            base_node,
            base_decl,
            &mut branch_visiting,
        )
        .or_else(|| {
            resolve_dataclass_transform_class_shape_from_decl(
                nodes,
                base_node,
                base_decl,
                &mut branch_visiting,
            )
        });
        let Some(inherited) = inherited else {
            continue;
        };
        for field in inherited.fields {
            if let Some(index) = fields
                .iter()
                .position(|existing: &DataclassTransformFieldShape| existing.name == field.name)
            {
                fields.remove(index);
            }
            fields.push(field);
        }
    }

    let local_fields = class_site
        .fields
        .iter()
        .filter(|field| !field.is_class_var)
        .filter_map(|field| {
            let recognized_field_specifier = field
                .field_specifier_name
                .as_ref()
                .is_some_and(|name| matches!(name.as_str(), "field" | "dataclasses.field"));
            if recognized_field_specifier && field.field_specifier_init == Some(false) {
                return None;
            }
            Some(DataclassTransformFieldShape {
                name: field.name.clone(),
                keyword_name: field.name.clone(),
                annotation: rewrite_imported_typing_aliases(class_node, &field.annotation),
                required: if recognized_field_specifier {
                    !(field.field_specifier_has_default
                        || field.field_specifier_has_default_factory)
                } else {
                    !field.has_default
                },
                kw_only: if recognized_field_specifier {
                    field.field_specifier_kw_only.unwrap_or(class_site.plain_dataclass_kw_only)
                } else {
                    class_site.plain_dataclass_kw_only
                },
            })
        })
        .collect::<Vec<_>>();
    for field in local_fields {
        if let Some(index) = fields
            .iter()
            .position(|existing: &DataclassTransformFieldShape| existing.name == field.name)
        {
            fields.remove(index);
        }
        fields.push(field);
    }

    Some(DataclassTransformClassShape {
        fields,
        frozen: class_site.plain_dataclass_frozen,
        has_explicit_init,
    })
}

fn resolve_known_plain_dataclass_shape_from_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<DataclassTransformClassShape> {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    resolve_plain_dataclass_class_shape(node, nodes, &type_name)
}

fn resolve_dataclass_transform_class_shape(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<DataclassTransformClassShape> {
    let (class_node, class_decl) = resolve_direct_base(nodes, node, callee)?;
    resolve_dataclass_transform_class_shape_from_decl(
        nodes,
        class_node,
        class_decl,
        &mut BTreeSet::new(),
    )
}

fn resolve_known_dataclass_transform_shape_from_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<DataclassTransformClassShape> {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    resolve_dataclass_transform_class_shape(node, nodes, &type_name)
}

fn resolve_dataclass_transform_metadata_from_decl(
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visiting: &mut BTreeSet<(String, String)>,
) -> Option<typepython_syntax::DataclassTransformMetadata> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visiting.insert(key) {
        return None;
    }

    let info = load_dataclass_transform_module_info(class_node)?;
    let class_site = info.classes.iter().find(|class_site| class_site.name == class_decl.name)?;

    for decorator in &class_site.decorators {
        if let Some(provider) = resolve_dataclass_transform_provider(nodes, class_node, decorator) {
            return Some(provider.metadata.clone());
        }
    }
    if let Some(provider_name) = class_site
        .bases
        .iter()
        .find(|base| resolve_dataclass_transform_provider(nodes, class_node, base).is_some())
    {
        return resolve_dataclass_transform_provider(nodes, class_node, provider_name)
            .map(|provider| provider.metadata.clone());
    }
    if let Some(metaclass) = class_site.metaclass.as_deref() {
        if let Some(provider) = resolve_dataclass_transform_provider(nodes, class_node, metaclass) {
            return Some(provider.metadata.clone());
        }
    }

    class_site.bases.iter().find_map(|base| {
        let (base_node, base_decl) = resolve_direct_base(nodes, class_node, base)?;
        let mut branch_visiting = visiting.clone();
        resolve_dataclass_transform_metadata_from_decl(
            nodes,
            base_node,
            base_decl,
            &mut branch_visiting,
        )
    })
}

fn resolve_dataclass_transform_class_shape_from_decl(
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visiting: &mut BTreeSet<(String, String)>,
) -> Option<DataclassTransformClassShape> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visiting.insert(key) {
        return None;
    }

    let has_explicit_init = class_node.declarations.iter().any(|declaration| {
        declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration.name == "__init__"
            && declaration.kind == DeclarationKind::Function
    });

    let info = load_dataclass_transform_module_info(class_node)?;
    let class_site = info.classes.iter().find(|class_site| class_site.name == class_decl.name)?;

    let mut metadata = None;
    for decorator in &class_site.decorators {
        if let Some(provider) = resolve_dataclass_transform_provider(nodes, class_node, decorator) {
            metadata = Some(provider.metadata.clone());
            break;
        }
    }
    if metadata.is_none() {
        if let Some(provider_name) = class_site
            .bases
            .iter()
            .find(|base| resolve_dataclass_transform_provider(nodes, class_node, base).is_some())
        {
            metadata = resolve_dataclass_transform_provider(nodes, class_node, provider_name)
                .map(|provider| provider.metadata.clone());
        }
    }
    if metadata.is_none() {
        metadata = class_site
            .metaclass
            .as_deref()
            .and_then(|metaclass| {
                resolve_dataclass_transform_provider(nodes, class_node, metaclass)
            })
            .map(|provider| provider.metadata.clone());
    }
    if metadata.is_none() {
        metadata = class_site.bases.iter().find_map(|base| {
            let (base_node, base_decl) = resolve_direct_base(nodes, class_node, base)?;
            let mut branch_visiting = visiting.clone();
            resolve_dataclass_transform_metadata_from_decl(
                nodes,
                base_node,
                base_decl,
                &mut branch_visiting,
            )
        });
    }
    let metadata = metadata?;

    let mut fields = Vec::new();
    for base in &class_site.bases {
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) else {
            continue;
        };
        let mut branch_visiting = visiting.clone();
        let Some(base_shape) = resolve_dataclass_transform_class_shape_from_decl(
            nodes,
            base_node,
            base_decl,
            &mut branch_visiting,
        ) else {
            continue;
        };
        for field in base_shape.fields {
            if let Some(index) = fields
                .iter()
                .position(|existing: &DataclassTransformFieldShape| existing.name == field.name)
            {
                fields.remove(index);
            }
            fields.push(field);
        }
    }

    for field in &class_site.fields {
        if field.is_class_var {
            continue;
        }
        let recognized_specifier = field.field_specifier_name.as_ref().is_some_and(|name| {
            metadata
                .field_specifiers
                .iter()
                .any(|candidate| candidate == name || candidate.ends_with(&format!(".{name}")))
        });
        if !recognized_specifier
            && field
                .value_metadata
                .as_ref()
                .and_then(|metadata| {
                    resolve_direct_expression_type_from_metadata(
                        class_node,
                        nodes,
                        None,
                        None,
                        Some(&class_decl.name),
                        field.line,
                        metadata,
                    )
                })
                .is_some_and(|value_type| is_descriptor_type(nodes, class_node, &value_type))
        {
            continue;
        }
        let init =
            if recognized_specifier { field.field_specifier_init.unwrap_or(true) } else { true };
        if !init {
            continue;
        }
        let required = if recognized_specifier {
            !(field.field_specifier_has_default
                || field.field_specifier_has_default_factory
                || (field.has_default && field.field_specifier_name.is_none()))
        } else {
            !field.has_default
        };
        let kw_only = if recognized_specifier {
            field.field_specifier_kw_only.unwrap_or(metadata.kw_only_default)
        } else {
            metadata.kw_only_default
        };
        let synthesized = DataclassTransformFieldShape {
            name: field.name.clone(),
            keyword_name: field.field_specifier_alias.clone().unwrap_or_else(|| field.name.clone()),
            annotation: rewrite_imported_typing_aliases(class_node, &field.annotation),
            required,
            kw_only,
        };
        if let Some(index) = fields.iter().position(|existing| existing.name == synthesized.name) {
            fields.remove(index);
        }
        fields.push(synthesized);
    }

    Some(DataclassTransformClassShape {
        fields,
        frozen: metadata.frozen_default,
        has_explicit_init,
    })
}

fn is_descriptor_type(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    type_name: &str,
) -> bool {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &type_name) else {
        return false;
    };

    ["__get__", "__set__", "__delete__"].iter().any(|member_name| {
        find_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
            matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })
        .is_some()
    })
}

fn load_dataclass_transform_module_info_uncached(
    node: &typepython_graph::ModuleNode,
) -> Option<typepython_syntax::DataclassTransformModuleInfo> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return None;
    }
    let source = fs::read_to_string(&node.module_path).ok()?;
    Some(typepython_syntax::collect_dataclass_transform_module_info(&source))
}

fn resolve_dataclass_transform_provider<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    name: &str,
) -> Option<typepython_syntax::DataclassTransformProviderSite> {
    if let Some(local) = load_dataclass_transform_module_info(node)?
        .providers
        .into_iter()
        .find(|provider| provider.name == name)
    {
        return Some(local);
    }

    if let Some((module_alias, symbol_name)) = name.rsplit_once('.') {
        if let Some(import) = node.declarations.iter().find(|declaration| {
            declaration.kind == DeclarationKind::Import && declaration.name == module_alias
        }) {
            if let Some(target_node) =
                nodes.iter().find(|candidate| candidate.module_key == import.detail)
            {
                return load_dataclass_transform_module_info(target_node)?
                    .providers
                    .into_iter()
                    .find(|provider| provider.name == symbol_name);
            }
        }
    }

    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == name
    })?;
    let (module_key, symbol_name) = import.detail.rsplit_once('.')?;
    let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
    load_dataclass_transform_module_info(target_node)?
        .providers
        .into_iter()
        .find(|provider| provider.name == symbol_name)
}

fn unresolved_import_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let project_roots: BTreeSet<_> = nodes
        .iter()
        .filter_map(|candidate| candidate.module_key.split('.').next())
        .map(str::to_owned)
        .collect();

    node.declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Import)
        .filter_map(|declaration| {
            let root = declaration.detail.split('.').next()?;
            if !project_roots.contains(root) {
                return None;
            }

            let resolves = nodes.iter().any(|candidate| candidate.module_key == declaration.detail)
                || declaration
                    .detail
                    .rsplit_once('.')
                    .and_then(|(module_key, symbol_name)| {
                        nodes.iter().find(|candidate| candidate.module_key == module_key).map(
                            |target| {
                                target.declarations.iter().any(|declaration| {
                                    declaration.owner.is_none() && declaration.name == symbol_name
                                })
                            },
                        )
                    })
                    .unwrap_or(false);

            (!resolves).then(|| {
                Diagnostic::error(
                    "TPY3001",
                    format!(
                        "module `{}` imports unresolved same-project target `{}`",
                        node.module_path.display(),
                        declaration.detail
                    ),
                )
            })
        })
        .collect()
}

fn deprecated_use_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    report_deprecated: DiagnosticLevel,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for declaration in
        node.declarations.iter().filter(|declaration| declaration.kind == DeclarationKind::Import)
    {
        if let Some(target) = resolve_import_target(node, nodes, declaration) {
            if target.is_deprecated {
                if let Some(diagnostic) = deprecated_diagnostic(
                    report_deprecated,
                    format!(
                        "module `{}` imports deprecated declaration `{}`",
                        node.module_path.display(),
                        declaration.name
                    ),
                    target.deprecation_message.as_deref(),
                ) {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    for call in &node.calls {
        if let Some(target) = resolve_direct_function(node, nodes, &call.callee) {
            if target.is_deprecated {
                if let Some(diagnostic) = deprecated_diagnostic(
                    report_deprecated,
                    format!(
                        "module `{}` calls deprecated declaration `{}`",
                        node.module_path.display(),
                        call.callee
                    ),
                    target.deprecation_message.as_deref(),
                ) {
                    diagnostics.push(diagnostic);
                }
            }
        } else if let Some((_, target)) = resolve_direct_base(nodes, node, &call.callee) {
            if target.is_deprecated {
                if let Some(diagnostic) = deprecated_diagnostic(
                    report_deprecated,
                    format!(
                        "module `{}` instantiates deprecated declaration `{}`",
                        node.module_path.display(),
                        call.callee
                    ),
                    target.deprecation_message.as_deref(),
                ) {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    for access in &node.member_accesses {
        if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &access.owner_name)
        {
            if let Some(member) =
                find_owned_value_declaration(nodes, class_node, class_decl, &access.member)
            {
                if member.is_deprecated {
                    if let Some(diagnostic) = deprecated_diagnostic(
                        report_deprecated,
                        format!(
                            "module `{}` uses deprecated member `{}` on `{}`",
                            node.module_path.display(),
                            access.member,
                            access.owner_name
                        ),
                        member.deprecation_message.as_deref(),
                    ) {
                        diagnostics.push(diagnostic);
                    }
                }
            }
        }
    }

    for call in &node.method_calls {
        if let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &call.owner_name) {
            if let Some(method) =
                find_owned_callable_declaration(nodes, class_node, class_decl, &call.method)
            {
                if method.is_deprecated {
                    if let Some(diagnostic) = deprecated_diagnostic(
                        report_deprecated,
                        format!(
                            "module `{}` calls deprecated member `{}` on `{}`",
                            node.module_path.display(),
                            call.method,
                            call.owner_name
                        ),
                        method.deprecation_message.as_deref(),
                    ) {
                        diagnostics.push(diagnostic);
                    }
                }
            }
        }
    }

    diagnostics
}

fn deprecated_diagnostic(
    report_deprecated: DiagnosticLevel,
    message: String,
    deprecation_message: Option<&str>,
) -> Option<Diagnostic> {
    let diagnostic = match report_deprecated {
        DiagnosticLevel::Ignore => return None,
        DiagnosticLevel::Warning => Diagnostic::warning("TPY4101", message),
        DiagnosticLevel::Error => Diagnostic::error("TPY4101", message),
    };
    Some(match deprecation_message {
        Some(note) if !note.is_empty() => diagnostic.with_note(note),
        _ => diagnostic,
    })
}

fn type_supports_mismatch_path(text: &str) -> bool {
    union_branches(text).is_some() || split_generic_type(text).is_some()
}

fn attach_type_mismatch_notes(
    diagnostic: Diagnostic,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
) -> Diagnostic {
    let expected = normalize_type_text(expected);
    let actual = normalize_type_text(actual);
    if !type_supports_mismatch_path(&expected) && !type_supports_mismatch_path(&actual) {
        return diagnostic;
    }

    let mut diagnostic = diagnostic
        .with_note(format!("source: `{actual}`"))
        .with_note(format!("target: `{expected}`"));
    let mut path = Vec::new();
    if let Some(detail) = first_type_mismatch_detail(node, nodes, &expected, &actual, &mut path, 8)
    {
        if !path.is_empty() {
            diagnostic = diagnostic.with_note(format!(
                "mismatch at: {}",
                path.iter().map(|segment| format!("-> {segment}")).collect::<Vec<_>>().join(" ")
            ));
        }
        diagnostic = diagnostic.with_note(detail);
    }
    diagnostic
}

fn first_type_mismatch_detail(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected: &str,
    actual: &str,
    path: &mut Vec<String>,
    depth: usize,
) -> Option<String> {
    if depth == 0 {
        return None;
    }

    let expected = normalize_type_text(expected);
    let actual = normalize_type_text(actual);
    if path.last().is_none_or(|segment| segment != &actual) {
        path.push(actual.clone());
    }

    if let (Some(expected_branches), Some(actual_branches)) =
        (union_branches(&expected), union_branches(&actual))
    {
        if let Some(unmatched) = actual_branches.iter().find(|branch| {
            !expected_branches
                .iter()
                .any(|target_branch| direct_type_is_assignable(node, nodes, target_branch, branch))
        }) {
            if path.last().is_none_or(|segment| segment != unmatched) {
                path.push(unmatched.clone());
            }
            return Some(format!(
                "union branch `{}` is not assignable to any target branch in `{}`",
                unmatched, expected
            ));
        }
    }

    if let (Some((expected_head, expected_args)), Some((actual_head, actual_args))) =
        (split_generic_type(&expected), split_generic_type(&actual))
    {
        if expected_head == "tuple"
            && actual_head == "tuple"
            && actual_args.len() == 2
            && actual_args[1] == "..."
            && !(expected_args.len() == 2 && expected_args[1] == "...")
        {
            return Some(format!(
                "`{actual}` is a variable-length tuple and is not assignable to fixed tuple `{expected}`"
            ));
        }

        if expected_head == actual_head && expected_args.len() == actual_args.len() {
            for (expected_arg, actual_arg) in expected_args.iter().zip(actual_args.iter()) {
                if !direct_type_is_assignable(node, nodes, expected_arg, actual_arg) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        expected_arg,
                        actual_arg,
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!("`{}` is not assignable to `{}`", actual_arg, expected_arg))
                    });
                }
            }
            if expected_head == "tuple" && expected_args != actual_args {
                return Some(format!("tuple shape `{actual}` is not assignable to `{expected}`"));
            }
        }

        match (expected_head, actual_head) {
            ("Sequence", "list") | ("Sequence", "tuple") if !expected_args.is_empty() => {
                let actual_element = if actual_head == "tuple" {
                    if actual_args.len() == 2 && actual_args[1] == "..." {
                        actual_args[0].clone()
                    } else {
                        join_branch_types(actual_args.clone())
                    }
                } else {
                    actual_args.first().cloned().unwrap_or_default()
                };
                if !direct_type_is_assignable(node, nodes, &expected_args[0], &actual_element) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        &expected_args[0],
                        &actual_element,
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!(
                            "element type `{}` is not assignable to `{}`",
                            actual_element, expected_args[0]
                        ))
                    });
                }
            }
            ("Mapping", "dict") if expected_args.len() == 2 && actual_args.len() == 2 => {
                if !direct_type_is_assignable(node, nodes, &expected_args[0], &actual_args[0]) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        &expected_args[0],
                        &actual_args[0],
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!(
                            "mapping key type `{}` is not assignable to `{}`",
                            actual_args[0], expected_args[0]
                        ))
                    });
                }
                if !direct_type_is_assignable(node, nodes, &expected_args[1], &actual_args[1]) {
                    return first_type_mismatch_detail(
                        node,
                        nodes,
                        &expected_args[1],
                        &actual_args[1],
                        path,
                        depth - 1,
                    )
                    .or_else(|| {
                        Some(format!(
                            "mapping value type `{}` is not assignable to `{}`",
                            actual_args[1], expected_args[1]
                        ))
                    });
                }
            }
            _ => {}
        }
    }

    None
}

fn same_return_owner(
    left: &typepython_binding::ReturnSite,
    right: &typepython_binding::ReturnSite,
) -> bool {
    left.owner_name == right.owner_name && left.owner_type_name == right.owner_type_name
}

fn describe_return_trace_expression(return_site: &typepython_binding::ReturnSite) -> String {
    if return_site.value_name.is_none()
        && return_site.value_member_name.is_none()
        && return_site.value_method_name.is_none()
        && return_site.value_callee.is_none()
        && return_site.value_lambda.is_none()
        && return_site.value_list_elements.is_none()
        && return_site.value_set_elements.is_none()
        && return_site.value_dict_entries.is_none()
        && return_site.value_subscript_target.is_none()
    {
        return String::from("bare return");
    }
    if let Some(name) = &return_site.value_name {
        return format!("return {name}");
    }
    if let (Some(owner), Some(member)) =
        (&return_site.value_member_owner_name, &return_site.value_member_name)
    {
        return format!("return {}.{}", owner, member);
    }
    if let (Some(owner), Some(method)) =
        (&return_site.value_method_owner_name, &return_site.value_method_name)
    {
        return format!("return {}.{}(...)", owner, method);
    }
    if let Some(callee) = &return_site.value_callee {
        return format!("return {}(...)", callee);
    }
    if let Some(key) = &return_site.value_subscript_string_key {
        return format!("return [...][\"{key}\"]");
    }
    if return_site.value_lambda.is_some() {
        return String::from("return lambda");
    }
    if return_site.value_dict_entries.is_some() {
        return String::from("return dict literal");
    }
    if return_site.value_list_elements.is_some() {
        return String::from("return list literal");
    }
    if return_site.value_set_elements.is_some() {
        return String::from("return set literal");
    }
    String::from("return expression")
}

fn inferred_return_type_for_owner(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected: &str,
    signature: &str,
) -> Option<String> {
    let related_returns = node
        .returns
        .iter()
        .filter(|candidate| same_return_owner(candidate, return_site))
        .collect::<Vec<_>>();
    if related_returns.is_empty() {
        return None;
    }

    let mut trace_types = Vec::new();
    for candidate in related_returns {
        let contextual =
            resolve_contextual_return_type(node, nodes, candidate, expected, signature);
        let candidate_type = contextual
            .actual_type
            .or_else(|| candidate.value_type.clone())
            .unwrap_or_else(|| String::from("unknown"));
        trace_types.push(candidate_type);
    }

    Some(if trace_types.len() > 1 {
        join_branch_types(trace_types)
    } else {
        normalize_type_text(trace_types.first().expect("single return type should exist"))
    })
}

fn attach_return_inference_trace(
    mut diagnostic: Diagnostic,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected: &str,
    actual: &str,
    signature: &str,
) -> Diagnostic {
    let related_returns = node
        .returns
        .iter()
        .filter(|candidate| same_return_owner(candidate, return_site))
        .collect::<Vec<_>>();
    if related_returns.is_empty() {
        return diagnostic;
    }

    let mut trace_types = Vec::new();
    let mut trace_lines = Vec::new();

    for candidate in related_returns {
        let contextual =
            resolve_contextual_return_type(node, nodes, candidate, expected, signature);
        let candidate_type = contextual
            .actual_type
            .or_else(|| candidate.value_type.clone())
            .unwrap_or_else(|| String::from("unknown"));
        trace_types.push(candidate_type.clone());
        trace_lines.push(format!(
            "line {}: {} -> {}",
            candidate.line,
            describe_return_trace_expression(candidate),
            normalize_type_text(&candidate_type)
        ));
    }

    let inferred_return_type =
        inferred_return_type_for_owner(node, nodes, return_site, expected, signature)
            .unwrap_or_else(|| normalize_type_text(actual));
    diagnostic = diagnostic
        .with_note(format!(
            "inferred return type: `{}`",
            normalize_type_text(&inferred_return_type)
        ))
        .with_note(format!("declared return type: `{}`", normalize_type_text(expected)))
        .with_note(String::from("inference trace:"));

    for line in trace_lines {
        diagnostic = diagnostic.with_note(line);
    }

    if node.returns.iter().filter(|candidate| same_return_owner(candidate, return_site)).count() > 1
    {
        diagnostic =
            diagnostic.with_note(format!("join: `{}`", normalize_type_text(&inferred_return_type)));
    }

    diagnostic
}

fn find_method_line(source: &str, owner_type_name: &str, method_name: &str) -> Option<usize> {
    typepython_syntax::collect_direct_method_signature_sites(source)
        .into_iter()
        .find(|site| site.owner_type_name == owner_type_name && site.name == method_name)
        .map(|site| site.line)
}

fn find_function_line(source: &str, function_name: &str) -> Option<usize> {
    typepython_syntax::collect_direct_function_signature_sites(source)
        .into_iter()
        .find(|site| site.name == function_name)
        .map(|site| site.line)
}

fn single_line_return_annotation_span(
    source: &str,
    owner_type_name: Option<&str>,
    function_name: &str,
) -> Option<Span> {
    let line = match owner_type_name {
        Some(owner_type_name) => find_method_line(source, owner_type_name, function_name)?,
        None => find_function_line(source, function_name)?,
    };
    let line_text = source.lines().nth(line.saturating_sub(1))?;
    let arrow = line_text.find("->")?;
    let colon = line_text[arrow + 2..].find(':')? + arrow + 2;
    let start_column = arrow
        + 3
        + line_text[arrow + 2..].chars().take_while(|character| character.is_whitespace()).count();
    let end_trimmed = line_text[..colon].trim_end();
    Some(Span::new(String::new(), line, start_column, line, end_trimmed.chars().count() + 1))
}

fn override_insertion_span(
    source: &str,
    owner_type_name: &str,
    method_name: &str,
    path: &std::path::Path,
) -> Option<Span> {
    let line = find_method_line(source, owner_type_name, method_name)?;
    let line_text = source.lines().nth(line.saturating_sub(1))?;
    let indent = line_text.chars().take_while(|character| character.is_whitespace()).count() + 1;
    Some(Span::new(path.display().to_string(), line, indent, line, indent))
}

#[allow(clippy::too_many_arguments)]
fn attach_missing_none_return_suggestion(
    diagnostic: Diagnostic,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    return_site: &typepython_binding::ReturnSite,
    expected_text: &str,
    expected: &str,
    actual: &str,
    signature: &str,
) -> Diagnostic {
    let inferred_actual =
        inferred_return_type_for_owner(node, nodes, return_site, expected, signature)
            .unwrap_or_else(|| normalize_type_text(actual));
    if union_branches(expected)
        .is_some_and(|branches| branches.iter().any(|branch| branch == "None"))
        || !union_branches(&inferred_actual)
            .is_some_and(|branches| branches.iter().any(|branch| branch == "None"))
    {
        return diagnostic;
    }
    let Some(without_none) = remove_none_branch(&inferred_actual) else {
        return diagnostic;
    };
    if !direct_type_is_assignable(node, nodes, expected, &without_none) {
        return diagnostic;
    }
    if node.module_path.to_string_lossy().starts_with('<') {
        return diagnostic;
    }
    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return diagnostic;
    };
    let Some(mut span) = single_line_return_annotation_span(
        &source,
        return_site.owner_type_name.as_deref(),
        &return_site.owner_name,
    ) else {
        return diagnostic;
    };
    span.path = node.module_path.display().to_string();
    diagnostic.with_suggestion(
        "Add `| None` to the declared return type",
        span,
        format!("{} | None", expected_text.trim()),
        SuggestionApplicability::MachineApplicable,
    )
}

fn resolve_import_target<'a>(
    _node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    declaration: &'a Declaration,
) -> Option<&'a Declaration> {
    let (module_key, symbol_name) = declaration.detail.rsplit_once('.')?;
    let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
    target_node
        .declarations
        .iter()
        .find(|target| target.owner.is_none() && target.name == symbol_name)
}

fn resolve_imported_module_target<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    local_name: &str,
) -> Option<&'a typepython_graph::ModuleNode> {
    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == local_name
    })?;
    nodes.iter().find(|candidate| candidate.module_key == import.detail)
}

fn resolve_imported_module_member_reference_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    owner_name: &str,
    member_name: &str,
) -> Option<String> {
    let module_node = resolve_imported_module_target(node, nodes, owner_name)?;
    let declaration = module_node
        .declarations
        .iter()
        .find(|declaration| declaration.owner.is_none() && declaration.name == member_name)?;
    match declaration.kind {
        DeclarationKind::Value => {
            let detail = rewrite_imported_typing_aliases(node, &declaration.detail);
            normalized_direct_return_annotation(&detail).map(normalize_type_text)
        }
        DeclarationKind::Function => {
            let param_types = direct_signature_sites_from_detail(&declaration.detail)
                .into_iter()
                .map(|param| param.annotation.unwrap_or_else(|| String::from("dynamic")))
                .collect::<Vec<_>>();
            let return_type = declaration.detail.split_once("->")?.1.trim();
            Some(format_callable_annotation(&param_types, return_type))
        }
        _ => None,
    }
}

fn resolve_imported_module_method_return_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    owner_name: &str,
    method_name: &str,
) -> Option<String> {
    let module_node = resolve_imported_module_target(node, nodes, owner_name)?;
    let methods = module_node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.owner.is_none()
                && declaration.name == method_name
                && matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })
        .collect::<Vec<_>>();
    let method = if methods.iter().any(|declaration| declaration.kind == DeclarationKind::Overload)
    {
        let call = node.method_calls.iter().find(|call| {
            call.owner_name == owner_name
                && call.method == method_name
                && !call.through_instance
                && call.line == current_line
        })?;
        let call = imported_module_method_call_site(module_node, call);
        let applicable = methods
            .iter()
            .copied()
            .filter(|declaration| {
                overload_is_applicable_with_context(node, nodes, &call, declaration)
            })
            .collect::<Vec<_>>();
        if applicable.len() == 1 {
            applicable[0]
        } else {
            return None;
        }
    } else {
        *methods.first()?
    };
    let return_text =
        rewrite_imported_typing_aliases(node, method.detail.split_once("->")?.1.trim());
    normalized_direct_return_annotation(&return_text).map(normalize_type_text)
}

fn imported_module_method_call_site(
    module_node: &typepython_graph::ModuleNode,
    call: &typepython_binding::MethodCallSite,
) -> typepython_binding::CallSite {
    typepython_binding::CallSite {
        callee: format!("{}.{}", module_node.module_key, call.method),
        arg_count: call.arg_count,
        arg_types: call.arg_types.clone(),
        arg_values: call.arg_values.clone(),
        starred_arg_types: call.starred_arg_types.clone(),
        starred_arg_values: call.starred_arg_values.clone(),
        keyword_names: call.keyword_names.clone(),
        keyword_arg_types: call.keyword_arg_types.clone(),
        keyword_arg_values: call.keyword_arg_values.clone(),
        keyword_expansion_types: call.keyword_expansion_types.clone(),
        keyword_expansion_values: call.keyword_expansion_values.clone(),
        line: 1,
    }
}

fn imported_module_method_call_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::MethodCallSite,
) -> Option<Vec<Diagnostic>> {
    let module_node = resolve_imported_module_target(node, nodes, &call.owner_name)?;
    let direct_call = imported_module_method_call_site(module_node, call);
    let callable_candidates = module_node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.owner.is_none()
                && declaration.name == call.method
                && matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })
        .collect::<Vec<_>>();
    if callable_candidates.is_empty() {
        let has_member = module_node
            .declarations
            .iter()
            .any(|declaration| declaration.owner.is_none() && declaration.name == call.method);
        return Some(if has_member {
            Vec::new()
        } else {
            vec![Diagnostic::error(
                "TPY4002",
                format!(
                    "module `{}` in module `{}` has no member `{}`",
                    module_node.module_key,
                    node.module_path.display(),
                    call.method
                ),
            )]
        });
    }

    let mut diagnostics = Vec::new();
    let overloads = callable_candidates
        .iter()
        .copied()
        .filter(|declaration| declaration.kind == DeclarationKind::Overload)
        .collect::<Vec<_>>();
    if !overloads.is_empty() {
        let applicable = overloads
            .iter()
            .copied()
            .filter(|declaration| {
                overload_is_applicable_with_context(node, nodes, &direct_call, declaration)
            })
            .collect::<Vec<_>>();
        if applicable.len() >= 2 {
            diagnostics.push(Diagnostic::error(
                "TPY4012",
                format!(
                    "call to `{}.{}` in module `{}` is ambiguous across {} overloads after applicability filtering",
                    module_node.module_key,
                    call.method,
                    node.module_path.display(),
                    applicable.len()
                ),
            ));
            return Some(diagnostics);
        }
        if let Some(applicable) = applicable.first().copied() {
            let signature = direct_signature_sites_from_detail(&applicable.detail);
            if let Some(diagnostic) =
                direct_source_function_arity_diagnostic(node, nodes, &direct_call, &signature)
            {
                diagnostics.push(diagnostic);
            }
            diagnostics.extend(direct_source_function_keyword_diagnostics(
                node,
                nodes,
                &direct_call,
                &signature,
            ));
            diagnostics.extend(direct_source_function_type_diagnostics(
                node,
                nodes,
                &direct_call,
                &signature,
            ));
            return Some(diagnostics);
        }
    }

    let signature = direct_signature_sites_from_detail(&callable_candidates[0].detail);
    if let Some(diagnostic) =
        direct_source_function_arity_diagnostic(node, nodes, &direct_call, &signature)
    {
        diagnostics.push(diagnostic);
    }
    diagnostics.extend(direct_source_function_keyword_diagnostics(
        node,
        nodes,
        &direct_call,
        &signature,
    ));
    diagnostics.extend(direct_source_function_type_diagnostics(
        node,
        nodes,
        &direct_call,
        &signature,
    ));
    Some(diagnostics)
}

fn override_compatibility_diagnostics<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let declarations = &node.declarations;
    let mut diagnostics = Vec::new();

    for class_declaration in declarations.iter().filter(|declaration| {
        declaration.kind == DeclarationKind::Class && declaration.owner.is_none()
    }) {
        for member in declarations.iter().filter(|declaration| {
            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_declaration.name)
        }) {
            for base in &class_declaration.bases {
                if let Some((base_node, base_decl)) = resolve_direct_base(nodes, node, base) {
                    if let Some(base_member) = base_node.declarations.iter().find(|declaration| {
                        declaration.owner.as_ref().is_some_and(|owner| owner.name == base_decl.name)
                            && declaration.name == member.name
                            && declaration.kind == member.kind
                    }) {
                        if !methods_are_compatible_for_override(node, nodes, member, base_member) {
                            diagnostics.push(Diagnostic::error(
                            "TPY4005",
                            format!(
                                "type `{}` in module `{}` overrides member `{}` from base `{}` with an incompatible signature or annotation",
                                class_declaration.name,
                                node.module_path.display(),
                                member.name,
                                base_decl.name
                            ),
                        ));
                        }
                    }
                }
            }
        }
    }

    diagnostics
}

fn methods_are_compatible_for_override(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    member: &Declaration,
    base_member: &Declaration,
) -> bool {
    if base_member.detail == member.detail && base_member.method_kind == member.method_kind {
        return true;
    }

    if matches!(member.name.as_str(), "__enter__" | "__exit__")
        && base_member.owner.as_ref().is_some_and(|owner| {
            matches!(owner.name.as_str(), "ContextManager" | "AbstractContextManager")
        })
        && member.method_kind == Some(typepython_syntax::MethodKind::Instance)
        && base_member.method_kind == Some(typepython_syntax::MethodKind::Instance)
    {
        return direct_param_count(&member.detail) == direct_param_count(&base_member.detail);
    }

    if member.method_kind != base_member.method_kind {
        return false;
    }

    let Some(member_params) = direct_signature_params(&member.detail) else {
        return false;
    };
    let Some(base_params) = direct_signature_params(&base_member.detail) else {
        return false;
    };
    if member_params.len() != base_params.len() {
        return false;
    }

    let params_compatible = member_params.iter().zip(base_params.iter()).all(|(child, base)| {
        child.positional_only == base.positional_only
            && child.keyword_only == base.keyword_only
            && child.variadic == base.variadic
            && child.keyword_variadic == base.keyword_variadic
            && child.has_default == base.has_default
            && child.name == base.name
            && (child.annotation.is_empty()
                || base.annotation.is_empty()
                || direct_type_is_assignable(node, nodes, &child.annotation, &base.annotation))
    });
    if !params_compatible {
        return false;
    }

    let child_return = member.detail.split_once("->").map(|(_, right)| right.trim()).unwrap_or("");
    let base_return =
        base_member.detail.split_once("->").map(|(_, right)| right.trim()).unwrap_or("");
    child_return.is_empty()
        || base_return.is_empty()
        || direct_type_is_assignable(node, nodes, base_return, child_return)
}

fn missing_override_diagnostics<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let declarations = &node.declarations;
    let source = if node.module_path.to_string_lossy().starts_with('<') {
        None
    } else {
        fs::read_to_string(&node.module_path).ok()
    };
    let mut diagnostics = Vec::new();

    for declaration in declarations.iter().filter(|declaration| {
        declaration.owner.is_some()
            && declaration.kind == DeclarationKind::Function
            && !declaration.is_override
    }) {
        let Some(owner) = declaration.owner.as_ref() else {
            continue;
        };
        let owner_decl = declarations.iter().find(|candidate| {
            candidate.name == owner.name
                && candidate.owner.is_none()
                && candidate.class_kind == Some(owner.kind)
        });
        let overrides_any = owner_decl.is_some_and(|owner_decl| {
            owner_decl.bases.iter().any(|base| {
                resolve_direct_base(nodes, node, base).is_some_and(|(base_node, base_decl)| {
                    base_node.declarations.iter().any(|candidate| {
                        candidate.name == declaration.name
                            && candidate.owner.as_ref().is_some_and(|candidate_owner| {
                                candidate_owner.name == base_decl.name
                            })
                    })
                })
            })
        });

        if overrides_any {
            let diagnostic = Diagnostic::error(
                "TPY4005",
                format!(
                    "member `{}` in type `{}` in module `{}` overrides a direct base member but is missing @override",
                    declaration.name,
                    owner.name,
                    node.module_path.display()
                ),
            );
            let diagnostic = source
                .as_deref()
                .and_then(|source| {
                    override_insertion_span(
                        source,
                        &owner.name,
                        &declaration.name,
                        &node.module_path,
                    )
                })
                .map(|span| {
                    diagnostic.clone().with_suggestion(
                        "Insert `@override` above the overriding method",
                        span,
                        String::from("@override\n"),
                        SuggestionApplicability::MachineApplicable,
                    )
                })
                .unwrap_or(diagnostic);
            diagnostics.push(diagnostic);
        }
    }

    diagnostics
}

fn final_decorator_diagnostics<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let declarations = &node.declarations;

    let mut diagnostics = Vec::new();

    for class_declaration in declarations.iter().filter(|declaration| {
        declaration.kind == DeclarationKind::Class && declaration.owner.is_none()
    }) {
        for base in &class_declaration.bases {
            if let Some((base_node, base_decl)) = resolve_direct_base(nodes, node, base) {
                if base_decl.is_final_decorator {
                    diagnostics.push(Diagnostic::error(
                        "TPY4005",
                        format!(
                            "type `{}` in module `{}` subclasses final class `{}`",
                            class_declaration.name,
                            node.module_path.display(),
                            base_decl.name
                        ),
                    ));
                }

                for member in declarations.iter().filter(|declaration| {
                    declaration
                        .owner
                        .as_ref()
                        .is_some_and(|owner| owner.name == class_declaration.name)
                }) {
                    if base_node.declarations.iter().any(|declaration| {
                        declaration.owner.as_ref().is_some_and(|owner| owner.name == base_decl.name)
                            && declaration.name == member.name
                            && declaration.is_final_decorator
                    }) {
                        diagnostics.push(Diagnostic::error(
                        "TPY4005",
                        format!(
                            "type `{}` in module `{}` overrides final member `{}` from base `{}`",
                            class_declaration.name,
                            node.module_path.display(),
                            member.name,
                            base_decl.name
                        ),
                    ));
                    }
                }
            }
        }
    }

    diagnostics
}

fn abstract_member_diagnostics<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let declarations = &node.declarations;

    let mut diagnostics = Vec::new();

    for class_declaration in declarations.iter().filter(|declaration| {
        declaration.kind == DeclarationKind::Class
            && declaration.owner.is_none()
            && declaration.class_kind == Some(DeclarationOwnerKind::Class)
    }) {
        let class_is_abstract = declarations.iter().any(|declaration| {
            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_declaration.name)
                && declaration.is_abstract_method
        });
        if class_is_abstract {
            continue;
        }

        for base in &class_declaration.bases {
            let Some((base_node, base_decl)) = resolve_direct_base(nodes, node, base) else {
                continue;
            };
            for ((abstract_owner, member_name), member_kind) in
                abstract_member_index(&base_node.declarations)
            {
                if abstract_owner != base_decl.name {
                    continue;
                }

                let implemented = declarations.iter().any(|declaration| {
                    declaration
                        .owner
                        .as_ref()
                        .is_some_and(|owner| owner.name == class_declaration.name)
                        && declaration.name == *member_name
                        && declaration.kind == member_kind
                        && !declaration.is_abstract_method
                });
                if !implemented {
                    diagnostics.push(Diagnostic::error(
                        "TPY4008",
                        format!(
                            "type `{}` in module `{}` does not implement abstract member `{}` from `{}`",
                            class_declaration.name,
                            node.module_path.display(),
                            member_name,
                            base_decl.name
                        ),
                    ));
                }
            }
        }
    }

    diagnostics
}

fn abstract_instantiation_diagnostics<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let declarations = &node.declarations;

    let abstract_classes: BTreeSet<_> = declarations
        .iter()
        .filter(|declaration| {
            declaration.kind == DeclarationKind::Class && declaration.owner.is_none()
        })
        .filter_map(|class_declaration| {
            let own_abstract = declarations.iter().any(|declaration| {
                declaration.owner.as_ref().is_some_and(|owner| owner.name == class_declaration.name)
                    && declaration.is_abstract_method
            });
            let inherited_abstract = class_declaration.bases.iter().any(|base| {
                let Some((base_node, base_decl)) = resolve_direct_base(nodes, node, base) else {
                    return false;
                };
                abstract_member_index(&base_node.declarations).iter().any(
                    |((abstract_owner, member_name), member_kind)| {
                        abstract_owner == &base_decl.name
                            && !declarations.iter().any(|declaration| {
                                declaration
                                    .owner
                                    .as_ref()
                                    .is_some_and(|owner| owner.name == class_declaration.name)
                                    && declaration.name == *member_name
                                    && declaration.kind == *member_kind
                                    && !declaration.is_abstract_method
                            })
                    },
                )
            });

            (own_abstract || inherited_abstract).then(|| class_declaration.name.clone())
        })
        .collect();

    node.calls
        .iter()
        .filter_map(|call| {
            let abstract_name = if abstract_classes.contains(&call.callee) {
                Some(call.callee.as_str())
            } else {
                resolve_direct_base(nodes, node, &call.callee).and_then(
                    |(base_node, declaration)| {
                        let own_abstract =
                            base_node.declarations.iter().any(|declaration_member| {
                                declaration_member
                                    .owner
                                    .as_ref()
                                    .is_some_and(|owner| owner.name == declaration.name)
                                    && declaration_member.is_abstract_method
                            });
                        let inherited_abstract = declaration.bases.iter().any(|base| {
                            let Some((resolved_node, resolved_decl)) =
                                resolve_direct_base(nodes, base_node, base)
                            else {
                                return false;
                            };
                            abstract_member_index(&resolved_node.declarations).iter().any(
                                |((abstract_owner, member_name), member_kind)| {
                                    abstract_owner == &resolved_decl.name
                                        && !base_node.declarations.iter().any(
                                            |declaration_member| {
                                                declaration_member.owner.as_ref().is_some_and(
                                                    |owner| owner.name == declaration.name,
                                                ) && declaration_member.name == *member_name
                                                    && declaration_member.kind == *member_kind
                                                    && !declaration_member.is_abstract_method
                                            },
                                        )
                                },
                            )
                        });

                        (own_abstract || inherited_abstract).then_some(declaration.name.as_str())
                    },
                )
            }?;

            Some(Diagnostic::error(
                "TPY4007",
                format!(
                    "module `{}` directly instantiates abstract class `{}`",
                    node.module_path.display(),
                    abstract_name
                ),
            ))
        })
        .collect()
}

fn abstract_member_index(
    declarations: &[Declaration],
) -> BTreeMap<(String, String), DeclarationKind> {
    declarations
        .iter()
        .filter(|declaration| declaration.owner.is_some() && declaration.is_abstract_method)
        .filter_map(|declaration| {
            declaration
                .owner
                .as_ref()
                .map(|owner| ((owner.name.clone(), declaration.name.clone()), declaration.kind))
        })
        .collect()
}

fn resolve_direct_base<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    base_name: &str,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    if let Some(local) = node.declarations.iter().find(|declaration| {
        declaration.name == base_name
            && declaration.owner.is_none()
            && declaration.kind == DeclarationKind::Class
    }) {
        return Some((node, local));
    }

    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == base_name
    })?;
    let (module_key, symbol_name) = import.detail.rsplit_once('.')?;
    let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
    let target_decl = target_node.declarations.iter().find(|declaration| {
        declaration.name == symbol_name
            && declaration.owner.is_none()
            && declaration.kind == DeclarationKind::Class
    })?;
    Some((target_node, target_decl))
}

fn sealed_match_exhaustiveness_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.matches
        .iter()
        .filter_map(|match_site| {
            if match_site
                .cases
                .iter()
                .any(|case| !case.has_guard && case.patterns.iter().any(|pattern| matches!(pattern, typepython_binding::MatchPatternSite::Wildcard)))
            {
                return None;
            }

            let subject_type = resolve_match_subject_type(node, nodes, match_site)?;
            let (sealed_node, sealed_decl) = resolve_sealed_root(nodes, node, &subject_type)?;
            let sealed_closure = collect_sealed_descendants(sealed_node, &sealed_decl.name);
            if sealed_closure.is_empty() {
                return None;
            }

            let mut covered = BTreeSet::new();
            for case in match_site.cases.iter().filter(|case| !case.has_guard) {
                for pattern in &case.patterns {
                    if let Some(case_type) = pattern_class_name(pattern) {
                        if let Some((case_node, case_decl)) = resolve_direct_base(nodes, node, case_type) {
                            if case_node.module_key == sealed_node.module_key {
                                if case_decl.name == sealed_decl.name {
                                    covered.extend(sealed_closure.iter().cloned());
                                } else if sealed_descends_from(nodes, case_node, case_decl, &sealed_decl.name) {
                                    covered.insert(case_decl.name.clone());
                                    covered.extend(collect_sealed_descendants(sealed_node, &case_decl.name));
                                }
                            }
                        }
                    }
                }
            }

            let missing = sealed_closure
                .into_iter()
                .filter(|name| !covered.contains(name))
                .collect::<Vec<_>>();
            if missing.is_empty() {
                return None;
            }

            let diagnostic = Diagnostic::error(
                "TPY4009",
                format!(
                    "non-exhaustive `match` over sealed root `{}` in module `{}`; missing subclasses: {}",
                    sealed_decl.name,
                    node.module_path.display(),
                    missing.join(", ")
                ),
            );
            let rendered_cases = missing
                .iter()
                .map(|name| format!("case {name}:\n    ..."))
                .collect::<Vec<_>>();
            Some(attach_match_case_suggestion(
                diagnostic,
                &node.module_path,
                match_site,
                &rendered_cases,
            ))
        })
        .collect()
}

fn enum_match_exhaustiveness_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.matches
        .iter()
        .filter_map(|match_site| {
            if match_site.cases.iter().any(|case| {
                !case.has_guard
                    && case.patterns.iter().any(|pattern| {
                        matches!(pattern, typepython_binding::MatchPatternSite::Wildcard)
                    })
            }) {
                return None;
            }

            let subject_type =
                normalize_type_text(&resolve_match_subject_type(node, nodes, match_site)?);
            let enum_type = enum_member_owner_name(&subject_type).unwrap_or(subject_type);
            let (enum_node, enum_decl) = resolve_direct_base(nodes, node, &enum_type)?;
            if !is_enum_like_class(nodes, enum_node, enum_decl)
                || is_flag_enum_like_class(nodes, enum_node, enum_decl)
            {
                return None;
            }

            let members = enum_node
                .declarations
                .iter()
                .filter(|declaration| {
                    declaration.kind == DeclarationKind::Value
                        && declaration
                            .owner
                            .as_ref()
                            .is_some_and(|owner| owner.name == enum_decl.name)
                })
                .map(|declaration| declaration.name.clone())
                .collect::<BTreeSet<_>>();
            if members.is_empty() {
                return None;
            }

            let mut covered = BTreeSet::new();
            for case in match_site.cases.iter().filter(|case| !case.has_guard) {
                for pattern in &case.patterns {
                    if let Some(member_name) =
                        enum_member_name_from_pattern(pattern, &enum_decl.name)
                    {
                        covered.insert(member_name);
                    }
                }
            }

            let missing =
                members.into_iter().filter(|member| !covered.contains(member)).collect::<Vec<_>>();
            if missing.is_empty() {
                return None;
            }

            let diagnostic = Diagnostic::error(
                "TPY4009",
                format!(
                    "non-exhaustive `match` over enum `{}` in module `{}`; missing members: {}",
                    enum_decl.name,
                    node.module_path.display(),
                    missing.join(", ")
                ),
            );
            let rendered_cases = missing
                .iter()
                .map(|name| format!("case {}.{name}:\n    ...", enum_decl.name))
                .collect::<Vec<_>>();
            Some(attach_match_case_suggestion(
                diagnostic,
                &node.module_path,
                match_site,
                &rendered_cases,
            ))
        })
        .collect()
}

fn attach_match_case_suggestion(
    diagnostic: Diagnostic,
    module_path: &std::path::Path,
    match_site: &typepython_binding::MatchSite,
    rendered_cases: &[String],
) -> Diagnostic {
    let Some((span, replacement)) =
        match_case_insertion_edit(module_path, match_site, rendered_cases)
    else {
        return diagnostic;
    };
    diagnostic.with_suggestion(
        "Add missing `match` case arms",
        span,
        replacement,
        SuggestionApplicability::MachineApplicable,
    )
}

fn match_case_insertion_edit(
    module_path: &std::path::Path,
    match_site: &typepython_binding::MatchSite,
    rendered_cases: &[String],
) -> Option<(Span, String)> {
    if rendered_cases.is_empty() {
        return None;
    }
    let source = fs::read_to_string(module_path).ok()?;
    let lines = source.lines().collect::<Vec<_>>();
    let match_line = *lines.get(match_site.line.checked_sub(1)?)?;
    let match_indent = leading_space_count(match_line);
    let case_indent = match_site
        .cases
        .iter()
        .filter_map(|case| lines.get(case.line.checked_sub(1)?).copied())
        .map(leading_space_count)
        .find(|indent| *indent > match_indent)
        .unwrap_or(match_indent + 4);
    let body_indent = case_indent + 4;
    let rendered = rendered_cases
        .iter()
        .map(|case| {
            case.lines()
                .enumerate()
                .map(|(index, line)| {
                    let indent = if index == 0 { case_indent } else { body_indent };
                    format!("{}{}", " ".repeat(indent), line.trim_start())
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let insertion_line =
        lines.iter().enumerate().skip(match_site.line).find_map(|(index, line)| {
            let trimmed = line.trim();
            (!trimmed.is_empty() && leading_space_count(line) <= match_indent).then_some(index + 1)
        });

    if let Some(insertion_line) = insertion_line {
        return Some((
            Span::new(module_path.display().to_string(), insertion_line, 1, insertion_line, 1),
            format!("{rendered}\n"),
        ));
    }

    let last_line = lines.len().max(1);
    let last_col = lines.last().map(|line| line.chars().count() + 1).unwrap_or(1);
    Some((
        Span::new(module_path.display().to_string(), last_line, last_col, last_line, last_col),
        format!("\n{rendered}\n"),
    ))
}

fn leading_space_count(line: &str) -> usize {
    line.chars().take_while(|character| *character == ' ').count()
}

fn resolve_match_subject_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    match_site: &typepython_binding::MatchSite,
) -> Option<String> {
    let signature = match_site.owner_name.as_deref().and_then(|owner_name| {
        node.declarations
            .iter()
            .find(|declaration| {
                declaration.kind == DeclarationKind::Function
                    && declaration.name == owner_name
                    && match (&match_site.owner_type_name, &declaration.owner) {
                        (Some(owner_type_name), Some(owner)) => owner.name == *owner_type_name,
                        (None, None) => true,
                        _ => false,
                    }
            })
            .map(|declaration| declaration.detail.as_str())
    });

    resolve_direct_expression_type(
        node,
        nodes,
        signature,
        None,
        match_site.owner_name.as_deref(),
        match_site.owner_type_name.as_deref(),
        match_site.line,
        match_site.subject_type.as_deref(),
        match_site.subject_is_awaited,
        match_site.subject_callee.as_deref(),
        match_site.subject_name.as_deref(),
        match_site.subject_member_owner_name.as_deref(),
        match_site.subject_member_name.as_deref(),
        match_site.subject_member_through_instance,
        match_site.subject_method_owner_name.as_deref(),
        match_site.subject_method_name.as_deref(),
        match_site.subject_method_through_instance,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

fn resolve_sealed_root<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    type_name: &str,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    let mut visited = BTreeSet::new();
    resolve_sealed_root_with_visited(nodes, node, type_name, &mut visited)
}

fn resolve_sealed_root_with_visited<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    type_name: &str,
    visited: &mut BTreeSet<(String, String)>,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    let (resolved_node, resolved_decl) = resolve_direct_base(nodes, node, type_name)?;
    let key = (resolved_node.module_key.clone(), resolved_decl.name.clone());
    if !visited.insert(key) {
        return None;
    }
    if resolved_decl.class_kind == Some(DeclarationOwnerKind::SealedClass) {
        return Some((resolved_node, resolved_decl));
    }
    resolved_decl
        .bases
        .iter()
        .find_map(|base| resolve_sealed_root_with_visited(nodes, resolved_node, base, visited))
}

fn collect_sealed_descendants(
    node: &typepython_graph::ModuleNode,
    root_name: &str,
) -> BTreeSet<String> {
    let mut descendants = BTreeSet::new();
    let mut stack = vec![root_name.to_owned()];
    while let Some(current) = stack.pop() {
        for declaration in node.declarations.iter().filter(|declaration| {
            declaration.kind == DeclarationKind::Class
                && declaration.owner.is_none()
                && declaration.bases.iter().any(|base| base == &current)
        }) {
            if descendants.insert(declaration.name.clone()) {
                stack.push(declaration.name.clone());
            }
        }
    }
    descendants
}

fn sealed_descends_from(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    root_name: &str,
) -> bool {
    let mut visited = BTreeSet::new();
    sealed_descends_from_with_visited(nodes, node, declaration, root_name, &mut visited)
}

fn sealed_descends_from_with_visited(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    root_name: &str,
    visited: &mut BTreeSet<(String, String)>,
) -> bool {
    let key = (node.module_key.clone(), declaration.name.clone());
    if !visited.insert(key) {
        return false;
    }
    declaration.bases.iter().any(|base| {
        if base == root_name {
            return true;
        }
        resolve_direct_base(nodes, node, base).is_some_and(|(base_node, base_decl)| {
            sealed_descends_from_with_visited(nodes, base_node, base_decl, root_name, visited)
        })
    })
}

fn pattern_class_name(pattern: &typepython_binding::MatchPatternSite) -> Option<&str> {
    match pattern {
        typepython_binding::MatchPatternSite::Class(name) => Some(name.as_str()),
        _ => None,
    }
}

fn enum_member_name_from_pattern(
    pattern: &typepython_binding::MatchPatternSite,
    enum_name: &str,
) -> Option<String> {
    let typepython_binding::MatchPatternSite::Literal(value) = pattern else {
        return None;
    };
    let (owner, member) = value.rsplit_once('.')?;
    (owner == enum_name).then(|| member.to_owned())
}

fn is_interface_like_declaration(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    nodes: &[typepython_graph::ModuleNode],
) -> bool {
    let mut visited = BTreeSet::new();
    is_interface_like_declaration_with_visited(node, declaration, nodes, &mut visited)
}

fn is_interface_like_declaration_with_visited(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    nodes: &[typepython_graph::ModuleNode],
    visited: &mut BTreeSet<(String, String)>,
) -> bool {
    if declaration.class_kind == Some(DeclarationOwnerKind::Interface) {
        return true;
    }

    let key = (node.module_key.clone(), declaration.name.clone());
    if !visited.insert(key) {
        return false;
    }

    declaration.bases.iter().any(|base| {
        resolve_direct_base(nodes, node, base).is_some_and(|(base_node, base_decl)| {
            is_interface_like_declaration_with_visited(base_node, base_decl, nodes, visited)
        })
    })
}

fn override_diagnostics<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let declarations = &node.declarations;
    let mut diagnostics = Vec::new();

    for declaration in declarations.iter().filter(|declaration| declaration.is_override) {
        let message = match declaration.owner.as_ref() {
            None => Some(format!(
                "declaration `{}` in module `{}` is marked with @override but has no base member to override",
                declaration.name,
                node.module_path.display()
            )),
            Some(owner) => {
                let owner_decl = declarations.iter().find(|candidate| {
                    candidate.name == owner.name
                        && candidate.owner.is_none()
                        && candidate.class_kind == Some(owner.kind)
                });
                let overrides_any = owner_decl.is_some_and(|owner_decl| {
                    owner_decl.bases.iter().any(|base| {
                        resolve_direct_base(nodes, node, base).is_some_and(
                            |(base_node, base_decl)| {
                                base_node.declarations.iter().any(|candidate| {
                                    candidate.name == declaration.name
                                        && candidate.owner.as_ref().is_some_and(|candidate_owner| {
                                            candidate_owner.name == base_decl.name
                                        })
                                })
                            },
                        )
                    })
                });

                (!overrides_any).then(|| {
                    format!(
                        "member `{}` in type `{}` in module `{}` is marked with @override but no direct base member was found",
                        declaration.name,
                        owner.name,
                        node.module_path.display()
                    )
                })
            }
        };

        if let Some(message) = message {
            diagnostics.push(Diagnostic::error("TPY4005", message));
        }
    }

    diagnostics
}

fn final_override_diagnostics<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let declarations = &node.declarations;
    let mut diagnostics = Vec::new();

    for class_declaration in declarations.iter().filter(|declaration| {
        declaration.kind == DeclarationKind::Class && declaration.owner.is_none()
    }) {
        for base in &class_declaration.bases {
            let Some((base_node, base_decl)) = resolve_direct_base(nodes, node, base) else {
                continue;
            };
            for member in declarations {
                let Some(owner) = member.owner.as_ref() else {
                    continue;
                };
                if owner.name != class_declaration.name {
                    continue;
                }
                if base_node.declarations.iter().any(|declaration| {
                    declaration.owner.as_ref().is_some_and(|owner| owner.name == base_decl.name)
                        && declaration.name == member.name
                        && declaration.is_final
                }) {
                    diagnostics.push(Diagnostic::error(
                        "TPY4006",
                        format!(
                            "type `{}` in module `{}` overrides Final member `{}` from base `{}`",
                            class_declaration.name,
                            node.module_path.display(),
                            member.name,
                            base_decl.name
                        ),
                    ));
                }
            }
        }
    }

    diagnostics
}

fn interface_implementation_diagnostics<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let declarations = &node.declarations;
    let mut diagnostics = Vec::new();

    for class_declaration in declarations.iter().filter(|declaration| {
        declaration.kind == DeclarationKind::Class
            && declaration.owner.is_none()
            && declaration.class_kind != Some(DeclarationOwnerKind::Interface)
    }) {
        for base in &class_declaration.bases {
            let Some((base_node, base_decl)) = resolve_direct_base(nodes, node, base) else {
                continue;
            };
            if !is_interface_like_declaration(base_node, base_decl, nodes) {
                continue;
            }

            for requirement in collect_interface_members(base_node, base_decl, nodes) {
                if !actual_member_satisfies_requirement(
                    nodes,
                    node,
                    class_declaration,
                    &requirement,
                ) {
                    diagnostics.push(Diagnostic::error(
                        "TPY4008",
                        format!(
                            "type `{}` in module `{}` does not implement interface member `{}` from `{}`",
                            class_declaration.name,
                            node.module_path.display(),
                            requirement.name,
                            base_decl.name
                        ),
                    ));
                }
            }
        }
    }

    diagnostics
}

fn duplicate_diagnostics(
    module_path: &std::path::Path,
    module_kind: SourceKind,
    declarations: &[Declaration],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(property_setter_compatibility_diagnostics(module_path, declarations));

    for (owner_name, owner_kind, space_declarations) in declaration_spaces(declarations) {
        for declaration in &space_declarations {
            if let Some(diagnostic) =
                classvar_placement_diagnostic(module_path, owner_name.as_deref(), declaration)
            {
                diagnostics.push(diagnostic);
            }
        }

        for duplicate in invalid_duplicates(&space_declarations) {
            if let Some(diagnostic) = final_reassignment_diagnostic(
                module_path,
                owner_name.as_deref(),
                duplicate,
                &space_declarations,
            ) {
                diagnostics.push(diagnostic);
            } else if let Some(diagnostic) = overload_shape_diagnostic(
                module_path,
                module_kind,
                owner_name.as_deref(),
                owner_kind,
                duplicate,
                &space_declarations,
            ) {
                diagnostics.push(diagnostic);
            } else if is_permitted_external_overload_group(
                module_kind,
                duplicate,
                &space_declarations,
            ) {
                continue;
            } else {
                diagnostics.push(Diagnostic::error(
                    "TPY4004",
                    duplicate_message(module_path, owner_name.as_deref(), duplicate),
                ));
            }
        }
    }

    diagnostics
}

fn classvar_placement_diagnostic(
    module_path: &std::path::Path,
    owner_name: Option<&str>,
    declaration: &Declaration,
) -> Option<Diagnostic> {
    if !declaration.is_class_var || owner_name.is_some() {
        return None;
    }

    Some(Diagnostic::error(
        "TPY4001",
        format!(
            "module `{}` uses ClassVar binding `{}` outside a class attribute declaration",
            module_path.display(),
            declaration.name
        ),
    ))
}

fn final_reassignment_diagnostic(
    module_path: &std::path::Path,
    owner_name: Option<&str>,
    duplicate: &str,
    declarations: &[Declaration],
) -> Option<Diagnostic> {
    let final_count = declarations
        .iter()
        .filter(|declaration| declaration.name == duplicate && declaration.is_final)
        .count();
    if final_count == 0 {
        return None;
    }

    let total_count =
        declarations.iter().filter(|declaration| declaration.name == duplicate).count();
    if total_count <= 1 {
        return None;
    }

    Some(Diagnostic::error(
        "TPY4006",
        match owner_name {
            Some(owner_name) => format!(
                "type `{owner_name}` in module `{}` reassigns Final binding `{duplicate}`",
                module_path.display()
            ),
            None => {
                format!("module `{}` reassigns Final binding `{duplicate}`", module_path.display())
            }
        },
    ))
}

fn declaration_spaces(
    declarations: &[Declaration],
) -> Vec<(Option<String>, Option<DeclarationOwnerKind>, Vec<Declaration>)> {
    let mut spaces: BTreeMap<(Option<String>, Option<DeclarationOwnerKind>), Vec<Declaration>> =
        BTreeMap::new();

    for declaration in declarations {
        let key = declaration.owner.as_ref().map(|owner| owner.name.clone());
        let owner_kind = declaration.owner.as_ref().map(|owner| owner.kind);
        spaces.entry((key, owner_kind)).or_default().push(declaration.clone());
    }

    spaces
        .into_iter()
        .map(|((owner_name, owner_kind), declarations)| (owner_name, owner_kind, declarations))
        .collect()
}

fn duplicate_message(
    module_path: &std::path::Path,
    owner_name: Option<&str>,
    duplicate: &str,
) -> String {
    match owner_name {
        Some(owner_name) => format!(
            "type `{owner_name}` in module `{}` declares member `{duplicate}` more than once in the same declaration space",
            module_path.display()
        ),
        None => format!(
            "module `{}` declares `{duplicate}` more than once in the same declaration space",
            module_path.display()
        ),
    }
}

fn is_permitted_external_overload_group(
    module_kind: SourceKind,
    duplicate: &str,
    declarations: &[Declaration],
) -> bool {
    if module_kind == SourceKind::TypePython {
        return false;
    }

    declarations
        .iter()
        .filter(|declaration| declaration.name == duplicate)
        .all(|declaration| declaration.kind == DeclarationKind::Overload)
}

fn invalid_duplicates(declarations: &[Declaration]) -> BTreeSet<&str> {
    let mut by_name: BTreeMap<&str, Vec<&Declaration>> = BTreeMap::new();

    for declaration in declarations {
        by_name.entry(&declaration.name).or_default().push(declaration);
    }

    by_name
        .into_iter()
        .filter_map(|(name, declarations)| {
            is_invalid_duplicate_group(&declarations).then_some(name)
        })
        .collect()
}

fn is_invalid_duplicate_group(declarations: &[&Declaration]) -> bool {
    if declarations.len() <= 1 {
        return false;
    }

    let overload_count = declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Overload)
        .count();
    let function_count = declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Function)
        .count();

    if overload_count >= 1
        && function_count == 1
        && overload_count + function_count == declarations.len()
    {
        return false;
    }

    let property_pair = declarations.len() == 2
        && declarations.iter().all(|declaration| declaration.kind == DeclarationKind::Function)
        && declarations.iter().any(|declaration| {
            declaration.method_kind == Some(typepython_syntax::MethodKind::Property)
        })
        && declarations.iter().any(|declaration| {
            declaration.method_kind == Some(typepython_syntax::MethodKind::PropertySetter)
        });
    if property_pair {
        return false;
    }

    true
}

fn property_setter_compatibility_diagnostics(
    module_path: &std::path::Path,
    declarations: &[Declaration],
) -> Vec<Diagnostic> {
    let mut groups: BTreeMap<(Option<String>, String), Vec<&Declaration>> = BTreeMap::new();
    for declaration in declarations {
        groups
            .entry((
                declaration.owner.as_ref().map(|owner| owner.name.clone()),
                declaration.name.clone(),
            ))
            .or_default()
            .push(declaration);
    }

    groups
        .into_iter()
        .filter_map(|((owner_name, member_name), decls)| {
            let getter = decls.iter().find(|decl| {
                decl.kind == DeclarationKind::Function
                    && decl.method_kind == Some(typepython_syntax::MethodKind::Property)
            })?;
            let setter = decls.iter().find(|decl| {
                decl.kind == DeclarationKind::Function
                    && decl.method_kind == Some(typepython_syntax::MethodKind::PropertySetter)
            })?;
            let getter_type = normalize_type_text(getter.detail.split_once("->")?.1.trim());
            let setter_params = direct_param_types(&setter.detail)?;
            let setter_type = normalize_type_text(setter_params.get(1)?);
            (getter_type != setter_type).then(|| {
                Diagnostic::error(
                    "TPY4001",
                    match owner_name {
                        Some(owner_name) => format!(
                            "type `{}` in module `{}` declares property `{}` with getter type `{}` but setter expects `{}`",
                            owner_name,
                            module_path.display(),
                            member_name,
                            getter_type,
                            setter_type,
                        ),
                        None => format!(
                            "module `{}` declares property `{}` with getter type `{}` but setter expects `{}`",
                            module_path.display(),
                            member_name,
                            getter_type,
                            setter_type,
                        ),
                    },
                )
            })
        })
        .collect()
}

fn overload_shape_diagnostic(
    module_path: &std::path::Path,
    module_kind: SourceKind,
    owner_name: Option<&str>,
    owner_kind: Option<DeclarationOwnerKind>,
    duplicate: &str,
    declarations: &[Declaration],
) -> Option<Diagnostic> {
    if matches!(owner_kind, Some(DeclarationOwnerKind::Interface)) {
        return None;
    }

    let overload_count = declarations
        .iter()
        .filter(|declaration| {
            declaration.name == duplicate && declaration.kind == DeclarationKind::Overload
        })
        .count();
    if overload_count == 0 {
        return None;
    }

    let function_count = declarations
        .iter()
        .filter(|declaration| {
            declaration.name == duplicate && declaration.kind == DeclarationKind::Function
        })
        .count();

    if module_kind != SourceKind::TypePython && function_count == 0 {
        return None;
    }

    let message = match function_count {
        0 => overload_shape_message(
            module_path,
            owner_name,
            duplicate,
            "without a concrete implementation",
        ),
        1 => return None,
        _ => overload_shape_message(
            module_path,
            owner_name,
            duplicate,
            "with more than one concrete implementation",
        ),
    };

    Some(Diagnostic::error("TPY4004", message))
}

fn overload_shape_message(
    module_path: &std::path::Path,
    owner_name: Option<&str>,
    duplicate: &str,
    suffix: &str,
) -> String {
    match owner_name {
        Some(owner_name) => format!(
            "type `{owner_name}` in module `{}` declares overloads for `{duplicate}` {suffix}",
            module_path.display()
        ),
        None => format!(
            "module `{}` declares overloads for `{duplicate}` {suffix}",
            module_path.display()
        ),
    }
}

#[cfg(test)]
mod tests;

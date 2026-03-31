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
mod assignments;
mod calls;
mod declarations;
mod semantic;

pub(crate) use self::assignments::*;
pub(crate) use self::calls::*;
pub(crate) use self::declarations::*;
pub(crate) use self::semantic::*;

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

#[cfg(test)]
mod tests;

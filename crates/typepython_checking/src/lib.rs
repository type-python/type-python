//! Type-checking boundary for TypePython.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use typepython_binding::{Declaration, DeclarationKind, DeclarationOwnerKind};
use typepython_config::DiagnosticLevel;
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span};
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

/// Runs the checker over the module graph.
#[must_use]
pub fn check(graph: &ModuleGraph) -> CheckResult {
    check_with_options(graph, false, true, DiagnosticLevel::Warning)
}

#[must_use]
pub fn check_with_options(
    graph: &ModuleGraph,
    require_explicit_overrides: bool,
    enable_sealed_exhaustiveness: bool,
    report_deprecated: DiagnosticLevel,
) -> CheckResult {
    let mut diagnostics = DiagnosticReport::default();

    for node in &graph.nodes {
        for alias_diagnostic in recursive_type_alias_diagnostics(node, &graph.nodes) {
            diagnostics.push(alias_diagnostic);
        }
        for overload_diagnostic in ambiguous_overload_call_diagnostics(node, &graph.nodes) {
            diagnostics.push(overload_diagnostic);
        }
        for unknown_diagnostic in direct_unknown_operation_diagnostics(node, &graph.nodes) {
            diagnostics.push(unknown_diagnostic);
        }
        for resolution_diagnostic in unresolved_import_diagnostics(node, &graph.nodes) {
            diagnostics.push(resolution_diagnostic);
        }
        for access_diagnostic in direct_member_access_diagnostics(node, &graph.nodes) {
            diagnostics.push(access_diagnostic);
        }
        for deprecated_diagnostic in
            deprecated_use_diagnostics(node, &graph.nodes, report_deprecated)
        {
            diagnostics.push(deprecated_diagnostic);
        }
        for call_diagnostic in direct_method_call_diagnostics(node, &graph.nodes) {
            diagnostics.push(call_diagnostic);
        }
        for return_diagnostic in direct_return_type_diagnostics(node, &graph.nodes) {
            diagnostics.push(return_diagnostic);
        }
        for yield_diagnostic in direct_yield_type_diagnostics(node, &graph.nodes) {
            diagnostics.push(yield_diagnostic);
        }
        for for_diagnostic in for_loop_target_diagnostics(node, &graph.nodes) {
            diagnostics.push(for_diagnostic);
        }
        for with_diagnostic in with_statement_diagnostics(node, &graph.nodes) {
            diagnostics.push(with_diagnostic);
        }
        for call_diagnostic in direct_call_arity_diagnostics(node, &graph.nodes) {
            diagnostics.push(call_diagnostic);
        }
        for call_diagnostic in direct_call_type_diagnostics(node, &graph.nodes) {
            diagnostics.push(call_diagnostic);
        }
        for call_diagnostic in direct_call_keyword_diagnostics(node, &graph.nodes) {
            diagnostics.push(call_diagnostic);
        }
        for call_diagnostic in direct_unresolved_paramspec_call_diagnostics(node, &graph.nodes) {
            diagnostics.push(call_diagnostic);
        }
        for assignment_diagnostic in annotated_assignment_type_diagnostics(node, &graph.nodes) {
            diagnostics.push(assignment_diagnostic);
        }
        for typed_dict_diagnostic in typed_dict_literal_diagnostics(node, &graph.nodes) {
            diagnostics.push(typed_dict_diagnostic);
        }
        for typed_dict_diagnostic in typed_dict_readonly_mutation_diagnostics(node, &graph.nodes) {
            diagnostics.push(typed_dict_diagnostic);
        }
        for dataclass_diagnostic in
            frozen_dataclass_transform_mutation_diagnostics(node, &graph.nodes)
        {
            diagnostics.push(dataclass_diagnostic);
        }
        for duplicate in
            duplicate_diagnostics(&node.module_path, node.module_kind, &node.declarations)
        {
            diagnostics.push(duplicate);
        }
        for override_violation in override_diagnostics(node, &graph.nodes) {
            diagnostics.push(override_violation);
        }
        for override_violation in override_compatibility_diagnostics(node, &graph.nodes) {
            diagnostics.push(override_violation);
        }
        if require_explicit_overrides && node.module_kind == SourceKind::TypePython {
            for override_violation in missing_override_diagnostics(node, &graph.nodes) {
                diagnostics.push(override_violation);
            }
        }
        for final_violation in final_decorator_diagnostics(node, &graph.nodes) {
            diagnostics.push(final_violation);
        }
        for override_violation in final_override_diagnostics(node, &graph.nodes) {
            diagnostics.push(override_violation);
        }
        for abstract_violation in abstract_member_diagnostics(node, &graph.nodes) {
            diagnostics.push(abstract_violation);
        }
        for instantiation_violation in abstract_instantiation_diagnostics(node, &graph.nodes) {
            diagnostics.push(instantiation_violation);
        }
        for implementation_violation in interface_implementation_diagnostics(node, &graph.nodes) {
            diagnostics.push(implementation_violation);
        }
        if enable_sealed_exhaustiveness {
            for match_violation in sealed_match_exhaustiveness_diagnostics(node, &graph.nodes) {
                diagnostics.push(match_violation);
            }
        }
        for conditional_return_diagnostic in conditional_return_coverage_diagnostics(node) {
            diagnostics.push(conditional_return_diagnostic);
        }
    }

    CheckResult { diagnostics }
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
                .filter(|declaration| overload_is_applicable(call, declaration))
                .collect::<Vec<_>>();
            if applicable.len() < 2 {
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

fn overload_is_applicable(call: &typepython_binding::CallSite, declaration: &Declaration) -> bool {
    let param_names = direct_param_names(&declaration.detail).unwrap_or_default();
    if call.arg_count != param_names.len() {
        return false;
    }
    if call.keyword_names.iter().any(|keyword| !param_names.iter().any(|param| param == keyword)) {
        return false;
    }

    let param_types = direct_param_types(&declaration.detail).unwrap_or_default();
    call.arg_types.iter().zip(param_types.iter()).all(|(arg_ty, param_ty)| {
        if arg_ty.is_empty() || param_ty.is_empty() {
            true
        } else {
            direct_type_matches(arg_ty, param_ty)
        }
    })
}

fn direct_unknown_operation_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for access in &node.member_accesses {
        if name_is_unknown_boundary(node, nodes, &access.owner_name) {
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
        if name_is_unknown_boundary(node, nodes, &call.owner_name) {
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
        if name_is_unknown_boundary(node, nodes, &call.callee) {
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

fn conditional_return_coverage_diagnostics(node: &typepython_graph::ModuleNode) -> Vec<Diagnostic> {
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
                        .any(|covered_branch| direct_type_matches(branch, covered_branch))
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

    if resolve_direct_name_reference_type(node, nodes, None, None, None, None, usize::MAX, name)
        .is_some_and(|resolved| normalize_type_text(&resolved) == "unknown")
    {
        return true;
    }

    node.declarations
        .iter()
        .find(|declaration| declaration.kind == DeclarationKind::Import && declaration.name == name)
        .is_some_and(|import| resolve_import_target(node, nodes, import).is_none())
}

fn recursive_type_alias_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let mut seen_cycles = BTreeSet::new();

    for declaration in node.declarations.iter().filter(|declaration| {
        declaration.kind == DeclarationKind::TypeAlias && declaration.owner.is_none()
    }) {
        let alias_id = format!("{}::{}", node.module_key, declaration.name);
        let mut stack = Vec::new();
        let mut visiting = BTreeSet::new();
        collect_recursive_type_alias_diagnostics(
            nodes,
            node,
            declaration,
            &alias_id,
            &mut stack,
            &mut visiting,
            &mut seen_cycles,
            &mut diagnostics,
        );
    }

    diagnostics
}

#[expect(clippy::too_many_arguments, reason = "recursive alias traversal threads shared state through helper recursion")]
fn collect_recursive_type_alias_diagnostics(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    alias_id: &str,
    stack: &mut Vec<String>,
    visiting: &mut BTreeSet<String>,
    seen_cycles: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(index) = stack.iter().position(|entry| entry == alias_id) {
        let cycle = stack[index..]
            .iter()
            .cloned()
            .chain(std::iter::once(alias_id.to_owned()))
            .collect::<Vec<_>>();
        let mut cycle_key_parts = cycle.clone();
        cycle_key_parts.sort();
        cycle_key_parts.dedup();
        let cycle_key = cycle_key_parts.join("|");
        if seen_cycles.insert(cycle_key) {
            diagnostics.push(Diagnostic::error(
                "TPY4001",
                format!(
                    "type alias `{}` in module `{}` is recursively defined: {}",
                    declaration.name,
                    node.module_path.display(),
                    cycle
                        .iter()
                        .map(|entry| entry
                            .rsplit_once("::")
                            .map(|(_, name)| name)
                            .unwrap_or(entry.as_str()))
                        .collect::<Vec<_>>()
                        .join(" -> ")
                ),
            ));
        }
        return;
    }

    if !visiting.insert(alias_id.to_owned()) {
        return;
    }
    stack.push(alias_id.to_owned());

    for reference in referenced_type_aliases(nodes, node, declaration) {
        let next_id = format!("{}::{}", reference.0.module_key, reference.1.name);
        collect_recursive_type_alias_diagnostics(
            nodes,
            reference.0,
            reference.1,
            &next_id,
            stack,
            visiting,
            seen_cycles,
            diagnostics,
        );
    }

    stack.pop();
    visiting.remove(alias_id);
}

fn referenced_type_aliases<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    declaration: &'a Declaration,
) -> Vec<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    referenced_type_identifiers(&declaration.detail)
        .into_iter()
        .filter_map(|name| resolve_type_alias_reference(nodes, node, &name))
        .collect()
}

fn referenced_type_identifiers(text: &str) -> Vec<String> {
    let mut identifiers = Vec::new();
    let mut token = String::new();

    for character in text.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            token.push(character);
            continue;
        }
        if !token.is_empty() {
            identifiers.push(std::mem::take(&mut token));
        }
    }
    if !token.is_empty() {
        identifiers.push(token);
    }

    identifiers
}

fn resolve_type_alias_reference<'a>(
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
    node.returns
        .iter()
        .filter_map(|return_site| {
            let target = node.declarations.iter().find(|declaration| {
                declaration.name == return_site.owner_name
                    && declaration.kind == DeclarationKind::Function
                    && match (&return_site.owner_type_name, &declaration.owner) {
                        (Some(owner_type), Some(owner)) => owner.name == *owner_type,
                        (None, None) => true,
                        _ => false,
                    }
            })?;

            let expected_text = rewrite_imported_typing_aliases(
                node,
                &substitute_self_annotation(
                    target.detail.split_once("->").map(|(_, annotation)| annotation).unwrap_or(""),
                    return_site.owner_type_name.as_deref(),
                ),
            );
            let expected =
                normalized_direct_return_annotation(&expected_text).map(normalize_type_text)?;

            let actual = resolve_direct_expression_type(
                node,
                nodes,
                Some(&target.detail),
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
            )?;

            (!direct_type_matches(&expected, &actual)).then(|| {
                Diagnostic::error(
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
                ))
            })
        })
        .collect()
}

fn direct_yield_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.yields
        .iter()
        .filter_map(|yield_site| {
            let target = node.declarations.iter().find(|declaration| {
                declaration.name == yield_site.owner_name
                    && declaration.kind == DeclarationKind::Function
                    && match (&yield_site.owner_type_name, &declaration.owner) {
                        (Some(owner_type_name), Some(owner)) => owner.name == *owner_type_name,
                        (None, None) => true,
                        _ => false,
                    }
            })?;

            let expected = unwrap_generator_yield_type(target.detail.split_once("->")?.1.trim())?;
            let actual = resolve_direct_expression_type(
                node,
                nodes,
                Some(&target.detail),
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
            )?;

            let actual = if yield_site.is_yield_from {
                unwrap_yield_from_type(&actual).unwrap_or(actual)
            } else {
                actual
            };

            (!direct_type_matches(&expected, &actual)).then(|| {
                Diagnostic::error(
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
            })
        })
        .collect()
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
    node.assignments
        .iter()
        .filter_map(|assignment| {
            let expected = normalized_assignment_annotation(assignment.annotation.as_deref()?)
                .map(normalize_type_text)?;
            if let Some(callable_result) = callable_assignment_result(node, nodes, assignment, &expected) {
                return callable_result;
            }
            let signature = resolve_assignment_owner_signature(node, assignment);
            let actual = resolve_direct_expression_type(
                node,
                nodes,
                signature,
                Some(&assignment.name),
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
            )?;
            (!direct_type_matches(&expected, &actual)).then(|| {
                Diagnostic::error(
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
                )
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
struct TypedDictFieldShape {
    value_type: String,
    required: bool,
    readonly: bool,
}

#[derive(Debug, Clone)]
struct TypedDictShape {
    name: String,
    fields: BTreeMap<String, TypedDictFieldShape>,
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

fn typed_dict_literal_diagnostics(
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
        let Some(target_shape) = resolve_known_typed_dict_shape_from_type(node, nodes, &annotation)
        else {
            continue;
        };

        let signature = resolve_scope_owner_signature(
            node,
            site.owner_name.as_deref(),
            site.owner_type_name.as_deref(),
        );
        let mut guaranteed_keys = BTreeSet::new();

        for entry in &site.entries {
            if entry.is_expansion {
                let Some(expansion_type) = resolve_direct_expression_type_from_metadata(
                    node,
                    nodes,
                    signature,
                    site.owner_name.as_deref(),
                    site.owner_type_name.as_deref(),
                    site.line,
                    &entry.value,
                ) else {
                    diagnostics.push(typed_dict_literal_diagnostic(
                        node,
                        site.line,
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
                        site.line,
                        format!(
                            "TypedDict literal for `{}` uses invalid `**` expansion of `{}`",
                            target_shape.name, expansion_type
                        ),
                    ));
                    continue;
                };

                for (key, field) in &expansion_shape.fields {
                    let Some(target_field) = target_shape.fields.get(key) else {
                        diagnostics.push(typed_dict_literal_diagnostic(
                            node,
                            site.line,
                            format!(
                                "TypedDict literal for `{}` expands unknown key `{}`",
                                target_shape.name, key
                            ),
                        ));
                        continue;
                    };

                    if !direct_type_matches(&target_field.value_type, &field.value_type) {
                        diagnostics.push(
                            typed_dict_literal_diagnostic(
                                node,
                                site.line,
                                format!(
                                    "TypedDict literal for `{}` expands `{}` with `{}` where `{}` expects `{}`",
                                    target_shape.name,
                                    key,
                                    field.value_type,
                                    key,
                                    target_field.value_type
                                ),
                            ),
                        );
                    }

                    if field.required {
                        guaranteed_keys.insert(key.clone());
                    }
                }

                continue;
            }

            let Some(key) = entry.key.as_deref() else {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    site.line,
                    format!("TypedDict literal for `{}` uses a non-literal key", target_shape.name),
                ));
                continue;
            };

            let Some(target_field) = target_shape.fields.get(key) else {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    site.line,
                    format!(
                        "TypedDict literal for `{}` uses unknown key `{}`",
                        target_shape.name, key
                    ),
                ));
                continue;
            };

            if let Some(actual_type) = resolve_direct_expression_type_from_metadata(
                node,
                nodes,
                signature,
                site.owner_name.as_deref(),
                site.owner_type_name.as_deref(),
                site.line,
                &entry.value,
            ) {
                if !direct_type_matches(&target_field.value_type, &actual_type) {
                    diagnostics.push(
                        typed_dict_literal_diagnostic(
                            node,
                            site.line,
                            format!(
                                "TypedDict literal for `{}` assigns `{}` to key `{}` where `{}` expects `{}`",
                                target_shape.name,
                                actual_type,
                                key,
                                key,
                                target_field.value_type
                            ),
                        ),
                    );
                }
            }

            guaranteed_keys.insert(key.to_owned());
        }

        for (key, field) in &target_shape.fields {
            if field.required && !guaranteed_keys.contains(key) {
                diagnostics.push(typed_dict_literal_diagnostic(
                    node,
                    site.line,
                    format!(
                        "TypedDict literal for `{}` is missing required key `{}`",
                        target_shape.name, key
                    ),
                ));
            }
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

fn typed_dict_readonly_mutation_diagnostics(
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
            let target_shape = resolve_known_typed_dict_shape_from_type(node, nodes, &owner_type)?;
            let field = target_shape.fields.get(key)?;
            field.readonly.then(|| {
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
                ))
            })
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

fn resolve_direct_expression_type_from_metadata(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    current_owner_name: Option<&str>,
    current_owner_type_name: Option<&str>,
    current_line: usize,
    metadata: &typepython_syntax::DirectExprMetadata,
) -> Option<String> {
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
    )
}

fn resolve_known_typed_dict_shape_from_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<TypedDictShape> {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    resolve_known_typed_dict_shape(node, nodes, &type_name)
}

fn resolve_known_typed_dict_shape(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<TypedDictShape> {
    let (class_node, class_decl) = resolve_direct_base(nodes, node, type_name)?;
    if !is_typed_dict_class(nodes, class_node, class_decl, &mut BTreeSet::new()) {
        return None;
    }

    let mut fields = BTreeMap::new();
    collect_typed_dict_fields(nodes, class_node, class_decl, &mut BTreeSet::new(), &mut fields);
    Some(TypedDictShape { name: class_decl.name.clone(), fields })
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
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
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
                collect_typed_dict_fields(nodes, base_node, base_decl, visited, fields);
            }
        }
    }

    for declaration in class_node.declarations.iter().filter(|declaration| {
        declaration.kind == DeclarationKind::Value
            && declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && !declaration.detail.is_empty()
    }) {
        fields.insert(
            declaration.name.clone(),
            parse_typed_dict_field_shape(&rewrite_imported_typing_aliases(
                class_node,
                &declaration.detail,
            )),
        );
    }
}

fn parse_typed_dict_field_shape(annotation: &str) -> TypedDictFieldShape {
    let mut value_type = normalize_type_text(annotation);
    let mut required = true;
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
    let (actual_params, actual_return) = resolve_callable_assignment_signature(node, nodes, assignment)?;

    let params_match = expected_params.as_ref().is_none_or(|expected_params| {
        expected_params.len() == actual_params.len()
            && expected_params.iter().zip(actual_params.iter()).all(
                |(expected_param, actual_param)| direct_type_matches(expected_param, actual_param),
            )
    });

    let matches = params_match && direct_type_matches(&expected_return, &actual_return);

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

#[expect(clippy::too_many_arguments, reason = "member callable resolution needs the current scope and member context")]
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

    let method_signature = rewrite_imported_typing_aliases(
        node,
        &substitute_self_annotation(&method.detail, Some(&owner_type_name)),
    );
    let actual_params = direct_param_types(&method_signature).unwrap_or_default();
    let bound_params = match method.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
        typepython_syntax::MethodKind::Static => actual_params,
        typepython_syntax::MethodKind::Property => return None,
        _ => actual_params.into_iter().skip(1).collect(),
    };
    let return_text = rewrite_imported_typing_aliases(
        node,
        &substitute_self_annotation(
            method.detail.split_once("->")?.1.trim(),
            Some(&owner_type_name),
        ),
    );
    let actual_return =
        normalized_direct_return_annotation(&return_text).map(normalize_type_text)?;
    Some((bound_params, actual_return))
}

fn parse_callable_annotation(text: &str) -> Option<(Option<Vec<String>>, String)> {
    let text = normalize_type_text(text);
    let inner = text.strip_prefix("Callable[").and_then(|inner| inner.strip_suffix(']'))?;
    let parts = split_top_level_type_args(inner);
    if parts.len() != 2 {
        return None;
    }
    let params = parts[0].trim();
    if params == "..." {
        return Some((None, normalize_type_text(parts[1])));
    }
    let params = params.strip_prefix('[').and_then(|inner| inner.strip_suffix(']'))?;
    let param_types = if params.trim().is_empty() {
        Vec::new()
    } else {
        split_top_level_type_args(params).into_iter().map(normalize_type_text).collect()
    };
    Some((Some(param_types), normalize_type_text(parts[1])))
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
    let owner_name = owner_name?;
    node.declarations
        .iter()
        .find(|declaration| {
            declaration.kind == DeclarationKind::Function
                && declaration.name == owner_name
                && match (owner_type_name, &declaration.owner) {
                    (Some(owner_type_name), Some(owner)) => owner.name == *owner_type_name,
                    (None, None) => true,
                    _ => false,
                }
        })
        .map(|declaration| declaration.detail.as_str())
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

fn direct_type_matches(expected: &str, actual: &str) -> bool {
    let expected = normalize_type_text(expected);
    let actual = normalize_type_text(actual);

    direct_type_matches_normalized(&expected, &actual)
}

fn direct_type_matches_normalized(expected: &str, actual: &str) -> bool {
    if let Some(inner) = annotated_inner(expected) {
        return direct_type_matches_normalized(&inner, actual);
    }
    if let Some(inner) = annotated_inner(actual) {
        return direct_type_matches_normalized(expected, &inner);
    }

    if expected == actual || expected == "Any" || actual == "Any" {
        return true;
    }

    if let Some(branches) = union_branches(expected) {
        if let Some(actual_branches) = union_branches(actual) {
            return actual_branches.iter().all(|actual_branch| {
                branches.iter().any(|expected_branch| {
                    direct_type_matches_normalized(expected_branch, actual_branch)
                })
            }) && branches.iter().all(|expected_branch| {
                actual_branches.iter().any(|actual_branch| {
                    direct_type_matches_normalized(expected_branch, actual_branch)
                })
            });
        }
        return branches.into_iter().any(|branch| direct_type_matches_normalized(&branch, actual));
    }

    match (split_generic_type(expected), split_generic_type(actual)) {
        (Some((expected_head, expected_args)), Some((actual_head, actual_args)))
            if expected_head == actual_head && expected_args.len() == actual_args.len() =>
        {
            expected_args.iter().zip(actual_args.iter()).all(|(expected_arg, actual_arg)| {
                direct_type_matches_normalized(expected_arg, actual_arg)
            })
        }
        _ => false,
    }
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

#[expect(clippy::too_many_arguments, reason = "direct expression resolution is driven by parsed expression metadata fields")]
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
) -> Option<String> {
    let resolved = value_type
        .filter(|value_type| !value_type.is_empty())
        .map(str::trim)
        .map(normalize_type_text)
        .or_else(|| {
            value_callee
                .and_then(|callee| resolve_direct_callable_return_type(node, nodes, callee))
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
        });

    resolved.and_then(
        |resolved| {
            if is_awaited { unwrap_awaitable_type(&resolved) } else { Some(resolved) }
        },
    )
}

fn resolve_direct_return_name_type(signature: &str, value_name: &str) -> Option<String> {
    let param_names = direct_param_names(signature)?;
    let param_types = direct_param_types(signature)?;
    param_names.iter().zip(param_types.iter()).find_map(|(param_name, param_type)| {
        (param_name == value_name).then_some(normalize_type_text(param_type))
    })
}

#[expect(clippy::too_many_arguments, reason = "name reference resolution needs scope and source-position context")]
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
    if let Some(receiver_type) =
        resolve_receiver_name_type(node, current_owner_name, current_owner_type_name, value_name)
    {
        return Some(receiver_type);
    }

    let signature =
        signature.map(|signature| substitute_self_annotation(signature, current_owner_type_name));
    let base_type = resolve_unnarrowed_name_reference_type(
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
        | (typepython_syntax::MethodKind::Property, "self") => Some(String::from(owner_type_name)),
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

#[expect(clippy::too_many_arguments, reason = "unnarrowed name resolution needs scope and source-position context")]
fn resolve_unnarrowed_name_reference_type(
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
        let return_text = rewrite_imported_typing_aliases(
            node,
            &substitute_self_annotation(
                function.detail.split_once("->")?.1,
                function.owner.as_ref().map(|owner| owner.name.as_str()),
            ),
        );
        return normalized_direct_return_annotation(&return_text).map(normalize_type_text);
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
                normalized_types.iter().any(|ty| direct_type_matches_normalized(ty, branch))
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
            !normalized_types.iter().any(|ty| direct_type_matches_normalized(ty, branch))
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
    if let Some(joined) = resolve_post_if_joined_assignment_type(
        node,
        nodes,
        signature,
        Some(owner_name),
        current_owner_type_name,
        current_line,
        value_name,
    ) {
        return Some(joined);
    }
    let assignment = node.assignments.iter().rev().find(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.as_deref() == Some(owner_name)
            && assignment.owner_type_name.as_deref() == current_owner_type_name
            && assignment.line < current_line
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
    if let Some(joined) = resolve_post_if_joined_assignment_type(
        node,
        nodes,
        signature,
        None,
        None,
        current_line,
        value_name,
    ) {
        return Some(joined);
    }
    let assignment = node.assignments.iter().rev().find(|assignment| {
        assignment.name == value_name
            && assignment.owner_name.is_none()
            && assignment.line < current_line
    })?;
    resolve_assignment_site_type(node, nodes, signature, assignment)
}

fn resolve_assignment_site_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    signature: Option<&str>,
    assignment: &typepython_binding::AssignmentSite,
) -> Option<String> {
    if let Some(annotation) = assignment.annotation.as_deref() {
        if let Some(annotation) = normalized_assignment_annotation(annotation) {
            return Some(normalize_type_text(annotation));
        }
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
    )
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

#[expect(clippy::too_many_arguments, reason = "member reference resolution needs source metadata and scope context")]
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
    let member = find_owned_value_declaration(nodes, class_node, class_decl, member_name)?;
    if is_enum_like_class(nodes, class_node, class_decl) {
        return Some(class_decl.name.clone());
    }
    let detail = rewrite_imported_typing_aliases(
        node,
        &substitute_self_annotation(&member.detail, Some(&owner_type_name)),
    );
    normalized_direct_return_annotation(&detail).map(normalize_type_text).or_else(|| {
        member.value_type.as_deref().map(|value| {
            normalize_type_text(&rewrite_imported_typing_aliases(
                node,
                &substitute_self_annotation(value, Some(&owner_type_name)),
            ))
        })
    })
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
                | "Flag"
                | "IntFlag"
                | "enum.Enum"
                | "enum.IntEnum"
                | "enum.Flag"
                | "enum.IntFlag"
        ) || resolve_direct_base(nodes, node, base)
            .is_some_and(|(base_node, base_decl)| is_enum_like_class(nodes, base_node, base_decl))
    })
}

#[expect(clippy::too_many_arguments, reason = "method return resolution needs source metadata and scope context")]
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
    let method = find_owned_callable_declaration(nodes, class_node, class_decl, method_name)?;
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
            let (class_node, class_decl) = resolve_direct_base(nodes, node, &access.owner_name)?;
            let has_member =
                find_owned_value_declaration(nodes, class_node, class_decl, &access.member)
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

fn direct_method_call_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for call in &node.method_calls {
        let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &call.owner_name)
        else {
            continue;
        };
        let Some(target) =
            find_owned_callable_declaration(nodes, class_node, class_decl, &call.method)
        else {
            continue;
        };

        let method_signature = substitute_self_annotation(&target.detail, Some(&class_decl.name));
        let param_names = direct_param_names(&method_signature).unwrap_or_default();
        let expected = match target.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
            typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => {
                param_names.len()
            }
            _ => param_names.len().saturating_sub(1),
        };

        if call.arg_count != expected {
            diagnostics.push(Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}.{}` in module `{}` expects {} positional argument(s) but received {}",
                    class_decl.name,
                    call.method,
                    node.module_path.display(),
                    expected,
                    call.arg_count
                ),
            ));
        }

        let expected_names: Vec<String> =
            match target.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
                typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => {
                    param_names
                }
                _ => param_names.into_iter().skip(1).collect(),
            };
        for keyword in &call.keyword_names {
            if !expected_names.iter().any(|param| param == keyword) {
                diagnostics.push(Diagnostic::error(
                    "TPY4001",
                    format!(
                        "call to `{}.{}` in module `{}` uses unknown keyword `{}`",
                        class_decl.name,
                        call.method,
                        node.module_path.display(),
                        keyword
                    ),
                ));
            }
        }
    }

    diagnostics
}

fn direct_call_arity_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let direct_function_signatures = load_direct_function_signatures(node);
    let direct_init_signatures = load_direct_init_signatures(node);
    node.calls
        .iter()
        .filter_map(|call| {
            if let Some(shape) = resolve_dataclass_transform_class_shape(node, nodes, &call.callee)
                && !shape.has_explicit_init
            {
                return dataclass_transform_constructor_arity_diagnostic(node, call, &shape);
            }
            if let Some(signature) = direct_init_signatures.get(&call.callee) {
                return direct_source_function_arity_diagnostic(node, call, signature);
            }
            if let Some(signature) = direct_function_signatures.get(&call.callee) {
                return direct_source_function_arity_diagnostic(node, call, signature);
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
    let inner = signature.strip_prefix('(')?.split_once(')')?.0;
    if inner.is_empty() { Some(0) } else { Some(split_top_level_type_args(inner).len()) }
}

fn direct_param_names(signature: &str) -> Option<Vec<String>> {
    let inner = signature.strip_prefix('(')?.split_once(')')?.0;
    if inner.is_empty() {
        return Some(Vec::new());
    }

    Some(
        split_top_level_type_args(inner)
            .into_iter()
            .map(|part| part.split(':').next().unwrap_or(part).trim().to_owned())
            .collect(),
    )
}

fn direct_param_types(signature: &str) -> Option<Vec<String>> {
    let inner = signature.strip_prefix('(')?.split_once(')')?.0;
    if inner.is_empty() {
        return Some(Vec::new());
    }

    Some(
        split_top_level_type_args(inner)
            .into_iter()
            .map(|part| {
                part.split_once(':')
                    .map(|(_, annotation)| annotation.trim().to_owned())
                    .unwrap_or_default()
            })
            .collect(),
    )
}

fn direct_call_type_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.calls
        .iter()
        .flat_map(|call| {
            if let Some(shape) = resolve_dataclass_transform_class_shape(node, nodes, &call.callee)
                && !shape.has_explicit_init
            {
                return dataclass_transform_constructor_type_diagnostics(node, call, &shape);
            }
            let Some(param_types) = resolve_direct_callable_param_types(node, nodes, &call.callee)
            else {
                return Vec::new();
            };
            call.arg_types
                .iter()
                .zip(param_types.iter())
                .filter(|(arg_ty, param_ty)| {
                    !arg_ty.is_empty()
                        && !param_ty.is_empty()
                        && arg_ty.as_str() != param_ty.as_str()
                })
                .map(|(arg_ty, param_ty)| {
                    Diagnostic::error(
                        "TPY4001",
                        format!(
                            "call to `{}` in module `{}` passes `{}` where parameter expects `{}`",
                            call.callee,
                            node.module_path.display(),
                            arg_ty,
                            param_ty
                        ),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn direct_call_keyword_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let direct_function_signatures = load_direct_function_signatures(node);
    let direct_init_signatures = load_direct_init_signatures(node);

    for call in &node.calls {
        if let Some(shape) = resolve_dataclass_transform_class_shape(node, nodes, &call.callee)
            && !shape.has_explicit_init
        {
            diagnostics
                .extend(dataclass_transform_constructor_keyword_diagnostics(node, call, &shape));
            continue;
        }
        if let Some(signature) = direct_init_signatures.get(&call.callee) {
            diagnostics.extend(direct_source_function_keyword_diagnostics(node, call, signature));
            continue;
        }
        if let Some(signature) = direct_function_signatures.get(&call.callee) {
            diagnostics.extend(direct_source_function_keyword_diagnostics(node, call, signature));
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
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Option<Diagnostic> {
    let positional_params =
        signature.iter().filter(|param| !param.keyword_only).collect::<Vec<_>>();
    if call.arg_count > positional_params.len() {
        return Some(Diagnostic::error(
            "TPY4001",
            format!(
                "call to `{}` in module `{}` expects at most {} positional argument(s) but received {}",
                call.callee,
                node.module_path.display(),
                positional_params.len(),
                call.arg_count
            ),
        ));
    }

    let provided_keywords = call.keyword_names.iter().collect::<BTreeSet<_>>();
    let missing = signature
        .iter()
        .enumerate()
        .filter(|(index, param)| {
            if param.has_default {
                return false;
            }
            if param.keyword_only {
                return !provided_keywords.contains(&param.name);
            }
            *index >= call.arg_count && !provided_keywords.contains(&param.name)
        })
        .map(|(_, param)| param.name.clone())
        .collect::<Vec<_>>();
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
    call: &typepython_binding::CallSite,
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<Diagnostic> {
    let param_names = signature.iter().map(|param| param.name.as_str()).collect::<BTreeSet<_>>();
    call.keyword_names
        .iter()
        .filter(|keyword| !param_names.contains(keyword.as_str()))
        .map(|keyword| {
            Diagnostic::error(
                "TPY4001",
                format!(
                    "call to `{}` in module `{}` uses unknown keyword `{}`",
                    call.callee,
                    node.module_path.display(),
                    keyword
                ),
            )
        })
        .collect()
}

fn load_direct_function_signatures(
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<String, Vec<typepython_syntax::DirectFunctionParamSite>> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return BTreeMap::new();
    }
    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return BTreeMap::new();
    };

    typepython_syntax::collect_direct_function_signature_sites(&source)
        .into_iter()
        .map(|signature| (signature.name, signature.params))
        .collect()
}

fn load_direct_init_signatures(
    node: &typepython_graph::ModuleNode,
) -> BTreeMap<String, Vec<typepython_syntax::DirectFunctionParamSite>> {
    if node.module_path.to_string_lossy().starts_with('<') {
        return BTreeMap::new();
    }
    let Ok(source) = fs::read_to_string(&node.module_path) else {
        return BTreeMap::new();
    };

    typepython_syntax::collect_direct_method_signature_sites(&source)
        .into_iter()
        .filter(|signature| signature.name == "__init__")
        .map(|signature| {
            let params = signature.params.into_iter().skip(1).collect::<Vec<_>>();
            (signature.owner_type_name, params)
        })
        .collect()
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
    call: &typepython_binding::CallSite,
    shape: &DataclassTransformClassShape,
) -> Vec<Diagnostic> {
    shape
        .fields
        .iter()
        .filter(|field| !field.kw_only)
        .take(call.arg_count)
        .zip(call.arg_types.iter())
        .filter(|(field, arg_ty)| {
            !arg_ty.is_empty() && !field.annotation.is_empty() && !direct_type_matches(&field.annotation, arg_ty)
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
        .collect()
}

fn dataclass_transform_constructor_keyword_diagnostics(
    node: &typepython_graph::ModuleNode,
    call: &typepython_binding::CallSite,
    shape: &DataclassTransformClassShape,
) -> Vec<Diagnostic> {
    let valid_names =
        shape.fields.iter().map(|field| field.keyword_name.as_str()).collect::<BTreeSet<_>>();
    call.keyword_names
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
        .collect()
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

    if let Some(shape) = resolve_dataclass_transform_class_shape(node, nodes, callee)
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

fn resolve_direct_callable_return_type<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Option<String> {
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

fn resolve_direct_function<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    callee: &str,
) -> Option<&'a Declaration> {
    if let Some(local) = node.declarations.iter().find(|declaration| {
        declaration.name == callee
            && declaration.owner.is_none()
            && declaration.kind == DeclarationKind::Function
    }) {
        return Some(local);
    }

    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == callee
    })?;
    let (module_key, symbol_name) = import.detail.rsplit_once('.')?;
    let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
    target_node.declarations.iter().find(|declaration| {
        declaration.name == symbol_name
            && declaration.owner.is_none()
            && declaration.kind == DeclarationKind::Function
    })
}

fn resolve_direct_callable_signature(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<(usize, Vec<String>)> {
    if let Some(local) = resolve_direct_function(node, nodes, callee) {
        return Some((
            direct_param_count(&local.detail).unwrap_or_default(),
            direct_param_names(&local.detail).unwrap_or_default(),
        ));
    }

    if let Some(shape) = resolve_dataclass_transform_class_shape(node, nodes, callee)
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
            resolve_dataclass_transform_class_shape_from_decl(
                nodes,
                base_node,
                base_decl,
                &mut branch_visiting,
            )
            .map(|shape| typepython_syntax::DataclassTransformMetadata {
                frozen_default: shape.frozen,
                ..typepython_syntax::DataclassTransformMetadata::default()
            })
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

fn load_dataclass_transform_module_info(
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
                        if !methods_are_compatible_for_override(member, base_member) {
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

fn methods_are_compatible_for_override(member: &Declaration, base_member: &Declaration) -> bool {
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

    false
}

fn missing_override_diagnostics<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let declarations = &node.declarations;
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
            diagnostics.push(Diagnostic::error(
                "TPY4005",
                format!(
                    "member `{}` in type `{}` in module `{}` overrides a direct base member but is missing @override",
                    declaration.name,
                    owner.name,
                    node.module_path.display()
                ),
            ));
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

            Some(Diagnostic::error(
                "TPY4009",
                format!(
                    "non-exhaustive `match` over sealed root `{}` in module `{}`; missing subclasses: {}",
                    sealed_decl.name,
                    node.module_path.display(),
                    missing.join(", ")
                ),
            ))
        })
        .collect()
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

            let interface_members: BTreeMap<_, _> = base_node
                .declarations
                .iter()
                .filter(|declaration| {
                    declaration.kind == DeclarationKind::Value
                        || declaration.kind == DeclarationKind::Function
                })
                .filter_map(|declaration| {
                    let owner = declaration.owner.as_ref()?;
                    (owner.name == base_decl.name).then(|| {
                        (
                            (owner.name.clone(), declaration.name.clone()),
                            (declaration.kind, declaration.method_kind, declaration.detail.clone()),
                        )
                    })
                })
                .collect::<BTreeMap<_, _>>();

            for ((interface_name, member_name), (member_kind, member_method_kind, member_detail)) in
                &interface_members
            {
                if interface_name != &base_decl.name {
                    continue;
                }

                let implemented = declarations.iter().find(|declaration| {
                    declaration
                        .owner
                        .as_ref()
                        .is_some_and(|owner| owner.name == class_declaration.name)
                        && declaration.name == *member_name
                        && declaration.kind == *member_kind
                });

                match implemented {
                    None => {
                        diagnostics.push(Diagnostic::error(
                            "TPY4008",
                            format!(
                                "type `{}` in module `{}` does not implement interface member `{}` from `{}`",
                                class_declaration.name,
                                node.module_path.display(),
                                member_name,
                                base_decl.name
                            ),
                        ));
                    }
                    Some(implementation)
                        if implementation.detail != *member_detail
                            || implementation.method_kind != *member_method_kind =>
                    {
                        diagnostics.push(Diagnostic::error(
                            "TPY4008",
                            format!(
                                "type `{}` in module `{}` implements interface member `{}` from `{}` with an incompatible signature or annotation",
                                class_declaration.name,
                                node.module_path.display(),
                                member_name,
                                base_decl.name
                            ),
                        ));
                    }
                    Some(_) => {}
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
    let mut by_name: BTreeMap<&str, Vec<DeclarationKind>> = BTreeMap::new();

    for declaration in declarations {
        by_name.entry(&declaration.name).or_default().push(declaration.kind);
    }

    by_name
        .into_iter()
        .filter_map(|(name, kinds)| is_invalid_duplicate_group(&kinds).then_some(name))
        .collect()
}

fn is_invalid_duplicate_group(kinds: &[DeclarationKind]) -> bool {
    if kinds.len() <= 1 {
        return false;
    }

    let overload_count = kinds.iter().filter(|kind| **kind == DeclarationKind::Overload).count();
    let function_count = kinds.iter().filter(|kind| **kind == DeclarationKind::Function).count();

    if overload_count >= 1 && function_count == 1 && overload_count + function_count == kinds.len()
    {
        return false;
    }

    true
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
mod tests {
    use super::{check, check_with_options};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    use typepython_binding::{
        Declaration, DeclarationKind, DeclarationOwner, DeclarationOwnerKind, bind,
    };
    use typepython_config::DiagnosticLevel;
    use typepython_graph::{ModuleGraph, ModuleNode, build};
    use typepython_syntax::{SourceFile, SourceKind, parse};

    fn check_temp_typepython_source(source_text: &str) -> super::CheckResult {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir()
            .join(format!("typepython-checking-{unique}-{}", std::process::id()));
        fs::create_dir_all(&root).expect("temp directory should be created");
        let path = root.join("app.tpy");
        fs::write(&path, source_text).expect("temp source should be written");

        let source = SourceFile {
            path: path.clone(),
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: source_text.to_owned(),
        };
        let tree = parse(source);
        let binding = bind(&tree);
        let graph = build(&[binding]);
        let result = check(&graph);

        let _ = fs::remove_dir_all(&root);
        result
    }

    #[test]
    fn check_reports_duplicate_module_symbols() {
        let graph = ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("User"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("User"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        };

        let result = check(&graph);

        let rendered = result.diagnostics.as_text();
        assert!(result.diagnostics.has_errors());
        assert!(rendered.contains("TPY4004"));
        assert!(rendered.contains("`User`"));
    }

    #[test]
    fn check_reports_missing_required_typed_dict_key() {
        let result = check_temp_typepython_source(
            "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n    name: str\n\npayload: User = {\"id\": 1}\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4013"));
        assert!(rendered.contains("missing required key `name`"));
    }

    #[test]
    fn check_reports_unknown_typed_dict_key() {
        let result = check_temp_typepython_source(
            "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n\npayload: User = {\"id\": 1, \"name\": \"Ada\"}\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4013"));
        assert!(rendered.contains("unknown key `name`"));
    }

    #[test]
    fn check_reports_incompatible_typed_dict_value() {
        let result = check_temp_typepython_source(
            "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n\npayload: User = {\"id\": \"oops\"}\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4013"));
        assert!(rendered.contains("assigns `str` to key `id`"));
    }

    #[test]
    fn check_reports_invalid_typed_dict_expansion() {
        let result = check_temp_typepython_source(
            "from typing import TypedDict\n\nclass User(TypedDict):\n    id: int\n\nclass Extra(TypedDict):\n    name: str\n\nextra: Extra = {\"name\": \"Ada\"}\npayload: User = {**extra}\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4013"));
        assert!(rendered.contains("expands unknown key `name`"));
    }

    #[test]
    fn check_reports_unresolved_paramspec_call() {
        let result = check_temp_typepython_source(
            "from typing import Callable, ParamSpec\n\nP = ParamSpec(\"P\")\n\ndef invoke(cb: Callable[P, int]) -> int:\n    return cb()\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4014"));
        assert!(rendered.contains("Callable[P, int]"));
    }

    #[test]
    fn check_reports_unresolved_concatenate_call() {
        let result = check_temp_typepython_source(
            "from typing import Callable, Concatenate, ParamSpec\n\nP = ParamSpec(\"P\")\n\ndef invoke(cb: Callable[Concatenate[int, P], int]) -> int:\n    return cb(1)\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4014"));
        assert!(rendered.contains("Concatenate[int, P]"));
    }

    #[test]
    fn check_accepts_keyword_and_default_arguments_in_direct_calls() {
        let result = check_temp_typepython_source(
            "def field(default=None, init=True, kw_only=False):\n    return default\n\nfield(default=\"Ada\", init=False)\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(!result.diagnostics.has_errors(), "{rendered}");
    }

    #[test]
    fn check_accepts_keyword_constructor_calls_for_explicit_init() {
        let result = check_temp_typepython_source(
            "class User:\n    def __init__(self, age: int, name: str = \"Ada\"):\n        self.age = age\n        self.name = name\n\nUser(age=1)\nUser(age=1, name=\"Grace\")\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(!result.diagnostics.has_errors(), "{rendered}");
    }

    #[test]
    fn check_reports_incomplete_conditional_return_coverage() {
        let result = check_temp_typepython_source(
            "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4018"));
        assert!(rendered.contains("missing: None"));
    }

    #[test]
    fn check_accepts_complete_conditional_return_coverage() {
        let result = check_temp_typepython_source(
            "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(!rendered.contains("TPY4018"));
        assert!(!result.diagnostics.has_errors());
    }

    #[test]
    fn check_accepts_dataclass_transform_decorator_constructor_call() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n    age: int\n\nuser: User = User(\"Ada\", 1)\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(!result.diagnostics.has_errors(), "{rendered}");
    }

    #[test]
    fn check_accepts_dataclass_transform_base_class_constructor_call() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\nclass ModelBase:\n    pass\n\nclass User(ModelBase):\n    name: str\n\nuser: User = User(\"Ada\")\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(!result.diagnostics.has_errors(), "{rendered}");
    }

    #[test]
    fn check_accepts_dataclass_transform_metaclass_constructor_call() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\nclass ModelMeta:\n    pass\n\nclass User(metaclass=ModelMeta):\n    name: str\n\nuser: User = User(\"Ada\")\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(!result.diagnostics.has_errors(), "{rendered}");
    }

    #[test]
    fn check_reports_dataclass_transform_constructor_arity_mismatch() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n    age: int\n\nuser: User = User(\"Ada\")\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(
            rendered.contains("missing required synthesized dataclass-transform field(s): age")
        );
    }

    #[test]
    fn check_reports_dataclass_transform_constructor_type_mismatch() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    age: int\n\nuser: User = User(\"Ada\")\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("synthesized dataclass-transform field `age` expects `int`"));
    }

    #[test]
    fn check_accepts_dataclass_transform_default_and_classvar_fields() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    role: ClassVar[str]\n    name: str\n    age: int = 1\n\nuser: User = User(\"Ada\")\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(!result.diagnostics.has_errors(), "{rendered}");
    }

    #[test]
    fn check_accepts_dataclass_transform_inherited_fields() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass Base:\n    name: str\n\nclass User(Base):\n    age: int\n\nuser: User = User(\"Ada\", 1)\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(!result.diagnostics.has_errors(), "{rendered}");
    }

    #[test]
    fn check_excludes_descriptor_defaults_from_dataclass_transform_fields() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\nclass Descriptor:\n    def __get__(self, instance, owner):\n        return 0\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: int = Descriptor()\n\nuser: User = User()\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(!result.diagnostics.has_errors(), "{rendered}");
    }

    #[test]
    fn check_accepts_frozen_dataclass_transform_assignment_in_init() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\n    def __init__(self, name: str):\n        self.name = name\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(!rendered.contains("frozen dataclass-transform field"), "{rendered}");
    }

    #[test]
    fn check_reports_frozen_dataclass_transform_field_assignment_after_init() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\nuser: User = User(\"Ada\")\nuser.name = \"Grace\"\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("frozen dataclass-transform field `name`"));
    }

    #[test]
    fn check_reports_frozen_dataclass_transform_field_assignment_after_init_with_explicit_init() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\n    def __init__(self, name: str):\n        self.name = name\n\nuser: User = User(\"Ada\")\nuser.name = \"Grace\"\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("frozen dataclass-transform field `name`"));
    }

    #[test]
    fn check_reports_frozen_dataclass_transform_augmented_assignment_after_init() {
        let result = check_temp_typepython_source(
            "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    count: int\n\nuser: User = User(1)\nuser.count += 1\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("augmented assignment after initialization"));
    }

    #[test]
    fn check_reports_readonly_typed_dict_item_assignment() {
        let result = check_temp_typepython_source(
            "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    user[\"name\"] = \"Grace\"\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4016"));
        assert!(rendered.contains("cannot be assigned"));
    }

    #[test]
    fn check_reports_readonly_typed_dict_item_augmented_assignment() {
        let result = check_temp_typepython_source(
            "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    user[\"name\"] += \"!\"\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4016"));
        assert!(rendered.contains("augmented assignment"));
    }

    #[test]
    fn check_reports_readonly_typed_dict_item_delete() {
        let result = check_temp_typepython_source(
            "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    del user[\"name\"]\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4016"));
        assert!(rendered.contains("cannot be deleted"));
    }

    #[test]
    fn check_reports_readonly_typed_dict_item_delete_for_qualified_base() {
        let result = check_temp_typepython_source(
            "import typing\nfrom typing_extensions import ReadOnly\n\nclass User(typing.TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    del user[\"name\"]\n",
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4016"));
        assert!(rendered.contains("cannot be deleted"));
    }

    #[test]
    fn check_accepts_unique_module_symbols() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("UserId"),
                        kind: DeclarationKind::TypeAlias,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("User"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_overload_sets_with_one_implementation() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_overloads_without_concrete_implementation() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4004"));
        assert!(rendered.contains("without a concrete implementation"));
    }

    #[test]
    fn check_reports_overloads_with_multiple_concrete_implementations() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4004"));
        assert!(rendered.contains("more than one concrete implementation"));
    }

    #[test]
    fn check_reports_ambiguous_overload_resolution() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::from("(value:int)->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::from("(value:int)->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(value:int)->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("parse"),
                    arg_count: 1,
                    arg_types: vec![String::from("int")],
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4012"));
        assert!(rendered.contains("ambiguous across 2 overloads"));
    }

    #[test]
    fn check_accepts_stub_only_overload_sets_in_pyi_modules() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("types/module.pyi"),
                module_key: String::new(),
                module_kind: SourceKind::Stub,
                declarations: vec![
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_duplicate_interface_members() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("SupportsClose"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("SupportsClose"),
                            kind: DeclarationOwnerKind::Interface,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("SupportsClose"),
                            kind: DeclarationOwnerKind::Interface,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4004"));
        assert!(rendered.contains("SupportsClose"));
        assert!(rendered.contains("member `close` more than once"));
    }

    #[test]
    fn check_accepts_class_method_overload_group() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("Parser"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Parser"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Parser"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_final_reassignment_in_module_scope() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("MAX_SIZE"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: true,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("MAX_SIZE"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4006"));
        assert!(rendered.contains("Final binding `MAX_SIZE`"));
    }

    #[test]
    fn check_reports_final_reassignment_in_class_scope() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: true,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4006"));
        assert!(rendered.contains("type `Box`"));
        assert!(rendered.contains("Final binding `limit`"));
    }

    #[test]
    fn check_reports_overriding_base_final_member() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.py"),
                module_key: String::new(),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: true,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Derived"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Derived"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4006"));
        assert!(rendered.contains("overrides Final member `limit` from base `Base`"));
    }

    #[test]
    fn check_reports_subclassing_final_class() {
        let graph = ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::new(),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: true,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("flag"),
                    annotation: Some(String::from("bool")),
                    value_type: Some(String::from("int")),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        };

        let result = check(&graph);
        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4005"));
        assert!(rendered.contains("subclasses final class `Base`"));
    }

    #[test]
    fn check_reports_subclassing_imported_final_class() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/base.py"),
                    module_key: String::from("app.base"),
                    module_kind: SourceKind::Python,
                    declarations: vec![Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: true,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    }],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                ModuleNode {
                    module_path: PathBuf::from("src/app/child.py"),
                    module_key: String::from("app.child"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Base"),
                            kind: DeclarationKind::Import,
                            detail: String::from("app.base.Base"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Child"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Base"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Base")],
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 2,
                },
            ],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4005"));
        assert!(rendered.contains("subclasses final class `Base`"));
    }

    #[test]
    fn check_reports_overriding_final_method() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::new(),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: true,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Child"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("missing"),
                    annotation: Some(String::from("None")),
                    value_type: Some(String::from("int")),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4005"));
        assert!(rendered.contains("overrides final member `run` from base `Base`"));
    }

    #[test]
    fn check_reports_missing_interface_members() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("SupportsClose"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("SupportsClose"),
                            kind: DeclarationOwnerKind::Interface,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Widget"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("SupportsClose")],
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4008"));
        assert!(rendered.contains("does not implement interface member `close`"));
    }

    #[test]
    fn check_reports_incompatible_interface_member_signature() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("SupportsClose"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("SupportsClose"),
                            kind: DeclarationOwnerKind::Interface,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Widget"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("SupportsClose")],
                    },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Widget"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("flag"),
                    annotation: Some(String::from("bool")),
                    value_type: Some(String::from("int")),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4008"));
        assert!(rendered.contains("incompatible signature or annotation"));
    }

    #[test]
    fn check_reports_incompatible_imported_interface_member_signature() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/protocols.tpy"),
                    module_key: String::from("app.protocols"),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![
                        Declaration {
                            name: String::from("SupportsClose"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("close"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->int"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("SupportsClose"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                ModuleNode {
                    module_path: PathBuf::from("src/app/impl.tpy"),
                    module_key: String::from("app.impl"),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![
                        Declaration {
                            name: String::from("SupportsClose"),
                            kind: DeclarationKind::Import,
                            detail: String::from("app.protocols.SupportsClose"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Widget"),
                            kind: DeclarationKind::Class,
                            detail: String::from("SupportsClose"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("SupportsClose")],
                        },
                        Declaration {
                            name: String::from("close"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->str"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Widget"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 2,
                },
            ],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4008"));
        assert!(rendered.contains("incompatible signature or annotation"));
    }

    #[test]
    fn check_reports_missing_abstract_base_members() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.py"),
                module_key: String::new(),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: true,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4008"));
        assert!(rendered.contains("does not implement abstract member `run`"));
    }

    #[test]
    fn check_reports_direct_instantiation_of_abstract_class() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.py"),
                module_key: String::new(),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: true,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("Base"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4007"));
        assert!(rendered.contains("directly instantiates abstract class `Base`"));
    }

    #[test]
    fn check_reports_direct_instantiation_of_imported_abstract_class() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/base.py"),
                    module_key: String::from("app.base"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Base"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->None"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Base"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: true,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                ModuleNode {
                    module_path: PathBuf::from("src/app/use.py"),
                    module_key: String::from("app.use"),
                    module_kind: SourceKind::Python,
                    declarations: vec![Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Import,
                        detail: String::from("app.base.Base"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    }],
                    calls: vec![typepython_binding::CallSite {
                        callee: String::from("Base"),
                        arg_count: 0,
                        arg_types: Vec::new(),
                        keyword_names: Vec::new(),
                    }],
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 2,
                },
            ],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4007"));
        assert!(rendered.contains("directly instantiates abstract class `Base`"));
    }

    #[test]
    fn check_reports_unresolved_same_project_imports() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/use.py"),
                module_key: String::from("app.use"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("Missing"),
                    kind: DeclarationKind::Import,
                    detail: String::from("app.missing.Missing"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("missing"),
                    annotation: Some(String::from("None")),
                    value_type: Some(String::from("int")),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY3001"));
        assert!(rendered.contains("app.missing.Missing"));
    }

    #[test]
    fn check_reports_direct_call_arity_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(x:int,y:int)->None"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("build"),
                    arg_count: 1,
                    arg_types: Vec::new(),
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("expects 2 positional argument(s) but received 1"));
    }

    #[test]
    fn check_reports_direct_call_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(x:int,y:str)->None"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("build"),
                    arg_count: 2,
                    arg_types: vec![String::from("str"), String::from("int")],
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("passes `str` where parameter expects `int`"));
    }

    #[test]
    fn check_reports_direct_return_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::from("str")),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `str` where `build` expects `int`"));
    }

    #[test]
    fn check_reports_direct_bool_return_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->bool"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::from("str")),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `str` where `build` expects `bool`"));
    }

    #[test]
    fn check_reports_direct_none_return_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->None"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::from("int")),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `int` where `build` expects `None`"));
    }

    #[test]
    fn check_accepts_direct_returned_call_result_type_match() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("helper"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_direct_returned_call_result_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("helper"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `str` where `build` expects `int`"));
    }

    #[test]
    fn check_accepts_direct_returned_constructor_type_match() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->Box"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("Box"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("Box")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_direct_returned_constructor_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("Box"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("Box")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `Box` where `build` expects `str`"));
    }

    #[test]
    fn check_accepts_direct_returned_parameter_type_match() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:int)->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_direct_returned_parameter_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:str)->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `str` where `build` expects `int`"));
    }

    #[test]
    fn check_accepts_direct_returned_member_type_match() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        detail: String::from("str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(box:Box)->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("box")),
                    value_member_name: Some(String::from("value")),
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_direct_returned_member_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        detail: String::from("str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(box:Box)->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("box")),
                    value_member_name: Some(String::from("value")),
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `str` where `build` expects `int`"));
    }

    #[test]
    fn check_accepts_direct_returned_constructor_member_type_match() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        detail: String::from("str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("Box")),
                    value_member_name: Some(String::from("value")),
                    value_member_through_instance: true,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_direct_returned_constructor_member_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        detail: String::from("str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("Box")),
                    value_member_name: Some(String::from("value")),
                    value_member_through_instance: true,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 1,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `str` where `build` expects `int`"));
    }

    #[test]
    fn check_reports_bool_annotated_assignment_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("flag"),
                    kind: DeclarationKind::Value,
                    detail: String::from("bool"),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("flag"),
                    annotation: Some(String::from("bool")),
                    value_type: Some(String::from("int")),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("assigns `int` where `flag` expects `bool`"));
    }

    #[test]
    fn check_reports_none_annotated_assignment_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("missing"),
                    kind: DeclarationKind::Value,
                    detail: String::from("None"),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("missing"),
                    annotation: Some(String::from("None")),
                    value_type: Some(String::from("int")),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("assigns `int` where `missing` expects `None`"));
    }

    #[test]
    fn check_accepts_direct_call_annotated_assignment_type_match() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        detail: String::from("int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("value"),
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_direct_call_annotated_assignment_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        detail: String::from("int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("value"),
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("assigns `str` where `value` expects `int`"));
    }

    #[test]
    fn check_accepts_direct_name_annotated_assignment_type_match() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("source"),
                        kind: DeclarationKind::Value,
                        detail: String::from("str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("target"),
                        kind: DeclarationKind::Value,
                        detail: String::from("str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("target"),
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("source")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_direct_member_annotated_assignment_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        detail: String::from("str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("box"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Box"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("target"),
                        kind: DeclarationKind::Value,
                        detail: String::from("int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("target"),
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("box")),
                    value_member_name: Some(String::from("value")),
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("assigns `str` where `target` expects `int`"));
    }

    #[test]
    fn check_reports_local_annotated_assignment_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:str)->None"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("function `build` in module `src/app/module.py` assigns `str` where local `result` expects `int`"));
    }

    #[test]
    fn check_accepts_return_from_local_bare_assignment() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 3,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("value"),
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 2,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_local_annotated_assignment_from_bare_assignment_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->None"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![
                    typepython_binding::AssignmentSite {
                        name: String::from("value"),
                        annotation: None,
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: Some(String::from("helper")),
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: Some(String::from("build")),
                        owner_type_name: None,
                        line: 2,
                    },
                    typepython_binding::AssignmentSite {
                        name: String::from("result"),
                        annotation: Some(String::from("int")),
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: None,
                        value_name: Some(String::from("value")),
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: Some(String::from("build")),
                        owner_type_name: None,
                        line: 3,
                    },
                ],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("function `build` in module `src/app/module.py` assigns `str` where local `result` expects `int`"));
    }

    #[test]
    fn check_accepts_module_level_bare_assignment_name_reference() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("result"),
                        kind: DeclarationKind::Value,
                        detail: String::from("int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![
                    typepython_binding::AssignmentSite {
                        name: String::from("value"),
                        annotation: None,
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: Some(String::from("helper")),
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: None,
                        owner_type_name: None,
                        line: 1,
                    },
                    typepython_binding::AssignmentSite {
                        name: String::from("result"),
                        annotation: Some(String::from("int")),
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: None,
                        value_name: Some(String::from("value")),
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: None,
                        owner_type_name: None,
                        line: 2,
                    },
                ],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_module_level_bare_assignment_name_reference_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("value"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("result"),
                        kind: DeclarationKind::Value,
                        detail: String::from("int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![
                    typepython_binding::AssignmentSite {
                        name: String::from("value"),
                        annotation: None,
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: Some(String::from("helper")),
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: None,
                        owner_type_name: None,
                        line: 1,
                    },
                    typepython_binding::AssignmentSite {
                        name: String::from("result"),
                        annotation: Some(String::from("int")),
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: None,
                        value_name: Some(String::from("value")),
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: None,
                        owner_type_name: None,
                        line: 2,
                    },
                ],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("assigns `str` where `result` expects `int`"));
    }

    #[test]
    fn check_accepts_local_chained_bare_assignments_for_annotated_target() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->None"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![
                    typepython_binding::AssignmentSite {
                        name: String::from("x"),
                        annotation: None,
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: Some(String::from("helper")),
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: Some(String::from("build")),
                        owner_type_name: None,
                        line: 1,
                    },
                    typepython_binding::AssignmentSite {
                        name: String::from("y"),
                        annotation: None,
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: None,
                        value_name: Some(String::from("x")),
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: Some(String::from("build")),
                        owner_type_name: None,
                        line: 2,
                    },
                    typepython_binding::AssignmentSite {
                        name: String::from("result"),
                        annotation: Some(String::from("int")),
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: None,
                        value_name: Some(String::from("y")),
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: Some(String::from("build")),
                        owner_type_name: None,
                        line: 3,
                    },
                ],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_local_chained_bare_assignments_for_return() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("y")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 3,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: vec![
                    typepython_binding::AssignmentSite {
                        name: String::from("x"),
                        annotation: None,
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: Some(String::from("helper")),
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: Some(String::from("build")),
                        owner_type_name: None,
                        line: 1,
                    },
                    typepython_binding::AssignmentSite {
                        name: String::from("y"),
                        annotation: None,
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: None,
                        value_name: Some(String::from("x")),
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: Some(String::from("build")),
                        owner_type_name: None,
                        line: 2,
                    },
                ],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_module_level_chained_bare_assignment_name_reference_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("x"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("y"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("result"),
                        kind: DeclarationKind::Value,
                        detail: String::from("int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![
                    typepython_binding::AssignmentSite {
                        name: String::from("x"),
                        annotation: None,
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: Some(String::from("helper")),
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: None,
                        owner_type_name: None,
                        line: 1,
                    },
                    typepython_binding::AssignmentSite {
                        name: String::from("y"),
                        annotation: None,
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: None,
                        value_name: Some(String::from("x")),
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: None,
                        owner_type_name: None,
                        line: 2,
                    },
                    typepython_binding::AssignmentSite {
                        name: String::from("result"),
                        annotation: Some(String::from("int")),
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: None,
                        value_name: Some(String::from("y")),
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: None,
                        owner_type_name: None,
                        line: 3,
                    },
                ],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("assigns `str` where `result` expects `int`"));
    }

    #[test]
    fn check_accepts_builtin_return_types_in_assignments_and_returns() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("count"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("size"),
                        kind: DeclarationKind::Value,
                        detail: String::from("int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("count"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("len")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 2,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("size"),
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("len")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_builtin_return_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("name"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("name"),
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("len")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("assigns `int` where `name` expects `str`"));
    }

    #[test]
    fn check_accepts_generic_alias_normalization() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("make_items"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->List[int]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("items"),
                        kind: DeclarationKind::Value,
                        detail: String::from("list[int]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("items"),
                    annotation: Some(String::from("list[int]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("make_items")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_callable_assignment_compatibility() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("handler"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Callable[[int], str]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("my_func"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(x:int)->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("handler"),
                    annotation: Some(String::from("Callable[[int], str]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("my_func")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_annotated_type_equivalence() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Annotated[str, tag])->str"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 2,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_callable_assignment_compatibility_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("handler"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Callable[[int], str]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("my_func"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(x:str)->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("handler"),
                    annotation: Some(String::from("Callable[[int], str]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("my_func")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains(
            "assigns callable `(str)->str` where `handler` expects `Callable[[int], str]`"
        ));
    }

    #[test]
    fn check_accepts_callable_ellipsis_assignment_compatibility() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("handler"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Callable[..., str]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("my_func"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(x:str,y:int)->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("handler"),
                    annotation: Some(String::from("Callable[..., str]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("my_func")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_callable_ellipsis_return_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("handler"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Callable[..., int]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("my_func"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(x:str,y:int)->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("handler"),
                    annotation: Some(String::from("Callable[..., int]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("my_func")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains(
            "assigns callable `(str,int)->str` where `handler` expects `Callable[..., int]`"
        ));
    }

    #[test]
    fn check_accepts_callable_assignment_from_bound_method() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("get"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:int)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("box"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Box"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("handler"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Callable[[int], str]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("handler"),
                    annotation: Some(String::from("Callable[[int], str]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("box")),
                    value_member_name: Some(String::from("get")),
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_callable_assignment_from_bound_method_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("get"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:str)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("box"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Box"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("handler"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Callable[[int], str]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("handler"),
                    annotation: Some(String::from("Callable[[int], str]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("box")),
                    value_member_name: Some(String::from("get")),
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains(
            "assigns callable `(str)->str` where `handler` expects `Callable[[int], str]`"
        ));
    }

    #[test]
    fn check_accepts_callable_assignment_from_bound_method_through_instance() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("get"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:int)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("make_box"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->Box"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("handler"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Callable[[int], str]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("handler"),
                    annotation: Some(String::from("Callable[[int], str]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("make_box")),
                    value_member_name: Some(String::from("get")),
                    value_member_through_instance: true,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_callable_assignment_from_bound_method_through_instance_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("get"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:str)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("make_box"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->Box"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("handler"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Callable[[int], str]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("handler"),
                    annotation: Some(String::from("Callable[[int], str]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("make_box")),
                    value_member_name: Some(String::from("get")),
                    value_member_through_instance: true,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains(
            "assigns callable `(str)->str` where `handler` expects `Callable[[int], str]`"
        ));
    }

    #[test]
    fn check_accepts_builtin_container_generic_any_match() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("items"),
                    kind: DeclarationKind::Value,
                    detail: String::from("list[str]"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("items"),
                    annotation: Some(String::from("list[str]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("list")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_any_optional_and_union_direct_matches() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("anything"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Any"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("maybe"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Optional[int]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("choice"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Union[int, str]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("measure"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("measure"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("len")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 4,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: vec![
                    typepython_binding::AssignmentSite {
                        name: String::from("anything"),
                        annotation: Some(String::from("Any")),
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: Some(String::from("len")),
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: None,
                        owner_type_name: None,
                        line: 1,
                    },
                    typepython_binding::AssignmentSite {
                        name: String::from("maybe"),
                        annotation: Some(String::from("Optional[int]")),
                        value_type: Some(String::from("None")),
                        is_awaited: false,
                        value_callee: None,
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: None,
                        owner_type_name: None,
                        line: 2,
                    },
                    typepython_binding::AssignmentSite {
                        name: String::from("choice"),
                        annotation: Some(String::from("Union[int, str]")),
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: Some(String::from("len")),
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: None,
                        owner_type_name: None,
                        line: 3,
                    },
                ],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_optional_direct_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("name"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Optional[str]"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("name"),
                    annotation: Some(String::from("Optional[str]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("len")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("assigns `int` where `name` expects `Optional[str]`"));
    }

    #[test]
    fn check_accepts_cast_builtin_return_type() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("text"),
                        kind: DeclarationKind::Value,
                        detail: String::from("str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->Any"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("cast"),
                        kind: DeclarationKind::Import,
                        detail: String::from("typing.cast"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("cast")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 2,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("text"),
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("cast")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_typing_typevar_assignment() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("TypeVar"),
                        kind: DeclarationKind::Import,
                        detail: String::from("typing.TypeVar"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("T"),
                        kind: DeclarationKind::Value,
                        detail: String::from("TypeVar"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("TypeVar"),
                    arg_count: 1,
                    arg_types: vec![String::from("str")],
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("T"),
                    annotation: Some(String::from("TypeVar")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("TypeVar")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_typing_typevar_argument_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("TypeVar"),
                    kind: DeclarationKind::Import,
                    detail: String::from("typing.TypeVar"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("TypeVar"),
                    arg_count: 1,
                    arg_types: vec![String::from("int")],
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("call to `TypeVar` in module `src/app/module.py` passes `int` where parameter expects `str`"));
    }

    #[test]
    fn check_accepts_typing_extensions_typevar_assignment() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("TypeVar"),
                            kind: DeclarationKind::Import,
                            detail: String::from("typing_extensions.TypeVar"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("T"),
                            kind: DeclarationKind::Value,
                            detail: String::from("TypeVar"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: vec![typepython_binding::CallSite {
                        callee: String::from("TypeVar"),
                        arg_count: 1,
                        arg_types: vec![String::from("str")],
                        keyword_names: Vec::new(),
                    }],
                    method_calls: Vec::new(),
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
                    assignments: vec![typepython_binding::AssignmentSite {
                        name: String::from("T"),
                        annotation: Some(String::from("TypeVar")),
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: Some(String::from("TypeVar")),
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: None,
                        owner_type_name: None,
                        line: 1,
                    }],
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<typing-extensions-prelude>"),
                    module_key: String::from("typing_extensions"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![typepython_binding::Declaration {
                        name: String::from("TypeVar"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(name:str)->TypeVar"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    }],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_typing_extensions_protocol_missing_member() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Protocol"),
                            kind: DeclarationKind::Import,
                            detail: String::from("typing_extensions.Protocol"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Reader"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Protocol"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Protocol")],
                        },
                        Declaration {
                            name: String::from("read"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->str"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Reader"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("BadReader"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Reader"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Reader")],
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<typing-extensions-prelude>"),
                    module_key: String::from("typing_extensions"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![typepython_binding::Declaration {
                        name: String::from("Protocol"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    }],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4008"));
        assert!(rendered.contains("does not implement interface member `read` from `Reader`"));
    }

    #[test]
    fn check_accepts_collections_abc_async_iterator_base() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("AsyncIterator"),
                            kind: DeclarationKind::Import,
                            detail: String::from("collections.abc.AsyncIterator"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Stream"),
                            kind: DeclarationKind::Class,
                            detail: String::from("AsyncIterator"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("AsyncIterator")],
                        },
                        Declaration {
                            name: String::from("__aiter__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->AsyncIterator[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Stream"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("__anext__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->Awaitable[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Stream"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                    module_key: String::from("collections.abc"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![
                        typepython_binding::Declaration {
                            name: String::from("AsyncIterable"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("__aiter__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->AsyncIterator[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("AsyncIterable"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("AsyncIterator"),
                            kind: DeclarationKind::Class,
                            detail: String::from("AsyncIterable"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("AsyncIterable")],
                        },
                        typepython_binding::Declaration {
                            name: String::from("__anext__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->Awaitable[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("AsyncIterator"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_collections_abc_async_iterator_missing_member() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("AsyncIterator"),
                            kind: DeclarationKind::Import,
                            detail: String::from("collections.abc.AsyncIterator"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("BadStream"),
                            kind: DeclarationKind::Class,
                            detail: String::from("AsyncIterator"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("AsyncIterator")],
                        },
                        Declaration {
                            name: String::from("__aiter__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->AsyncIterator[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("BadStream"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                    module_key: String::from("collections.abc"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![
                        typepython_binding::Declaration {
                            name: String::from("AsyncIterable"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("__aiter__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->AsyncIterator[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("AsyncIterable"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("AsyncIterator"),
                            kind: DeclarationKind::Class,
                            detail: String::from("AsyncIterable"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("AsyncIterable")],
                        },
                        typepython_binding::Declaration {
                            name: String::from("__anext__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->Awaitable[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("AsyncIterator"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4008"));
        assert!(
            rendered
                .contains("does not implement interface member `__anext__` from `AsyncIterator`")
        );
    }

    #[test]
    fn check_accepts_newtype_assignment() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("NewType"),
                        kind: DeclarationKind::Import,
                        detail: String::from("typing.NewType"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("UserId"),
                        kind: DeclarationKind::Value,
                        detail: String::from("NewType"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("NewType"),
                    arg_count: 2,
                    arg_types: vec![String::from("str"), String::new()],
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("UserId"),
                    annotation: Some(String::from("NewType")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("NewType")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_newtype_argument_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("NewType"),
                    kind: DeclarationKind::Import,
                    detail: String::from("typing.NewType"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("NewType"),
                    arg_count: 2,
                    arg_types: vec![String::from("int"), String::new()],
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("call to `NewType` in module `src/app/module.py` passes `int` where parameter expects `str`"));
    }

    #[test]
    fn check_accepts_protocol_derived_base_implementation() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Protocol"),
                            kind: DeclarationKind::Import,
                            detail: String::from("typing.Protocol"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Reader"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Protocol"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Protocol")],
                        },
                        Declaration {
                            name: String::from("read"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->str"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Reader"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("FileReader"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Reader"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Reader")],
                        },
                        Declaration {
                            name: String::from("read"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->str"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("FileReader"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<typing-prelude>"),
                    module_key: String::from("typing"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![typepython_binding::Declaration {
                        name: String::from("Protocol"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    }],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_protocol_derived_base_missing_member() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Protocol"),
                            kind: DeclarationKind::Import,
                            detail: String::from("typing.Protocol"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Reader"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Protocol"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Protocol")],
                        },
                        Declaration {
                            name: String::from("read"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->str"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Reader"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("BadReader"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Reader"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Reader")],
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<typing-prelude>"),
                    module_key: String::from("typing"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![typepython_binding::Declaration {
                        name: String::from("Protocol"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    }],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4008"));
        assert!(rendered.contains("does not implement interface member `read` from `Reader`"));
    }

    #[test]
    fn check_accepts_collections_abc_sized_base() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Sized"),
                            kind: DeclarationKind::Import,
                            detail: String::from("collections.abc.Sized"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Box"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Sized"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Sized")],
                        },
                        Declaration {
                            name: String::from("__len__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->int"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Box"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                    module_key: String::from("collections.abc"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![
                        typepython_binding::Declaration {
                            name: String::from("Sized"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("__len__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->int"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Sized"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_collections_abc_sized_missing_member() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Sized"),
                            kind: DeclarationKind::Import,
                            detail: String::from("collections.abc.Sized"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("BadBox"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Sized"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Sized")],
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                    module_key: String::from("collections.abc"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![
                        typepython_binding::Declaration {
                            name: String::from("Sized"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("__len__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->int"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Sized"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4008"));
        assert!(rendered.contains("does not implement interface member `__len__` from `Sized`"));
    }

    #[test]
    fn check_accepts_collections_abc_callable_base() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Callable"),
                            kind: DeclarationKind::Import,
                            detail: String::from("collections.abc.Callable"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Runner"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Callable"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Callable")],
                        },
                        Declaration {
                            name: String::from("__call__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->Any"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Runner"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                    module_key: String::from("collections.abc"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![
                        typepython_binding::Declaration {
                            name: String::from("Callable"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("__call__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->Any"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Callable"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_collections_abc_iterator_missing_member() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Iterator"),
                            kind: DeclarationKind::Import,
                            detail: String::from("collections.abc.Iterator"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Cursor"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Iterator"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Iterator")],
                        },
                        Declaration {
                            name: String::from("__iter__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->Iterator[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Cursor"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<collections.abc-prelude>"),
                    module_key: String::from("collections.abc"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![
                        typepython_binding::Declaration {
                            name: String::from("Sized"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("__len__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->int"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Sized"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("Iterable"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Sized"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Sized")],
                        },
                        typepython_binding::Declaration {
                            name: String::from("__iter__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->Iterator[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Iterable"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("Iterator"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Iterable"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Iterable")],
                        },
                        typepython_binding::Declaration {
                            name: String::from("__next__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->Any"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Iterator"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4008"));
        assert!(
            rendered.contains("does not implement interface member `__next__` from `Iterator`")
        );
    }

    #[test]
    fn check_accepts_typing_awaitable_base() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Awaitable"),
                            kind: DeclarationKind::Import,
                            detail: String::from("typing.Awaitable"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Job"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Awaitable"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Awaitable")],
                        },
                        Declaration {
                            name: String::from("__await__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->Iterator[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Job"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<typing-prelude>"),
                    module_key: String::from("typing"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![
                        typepython_binding::Declaration {
                            name: String::from("Awaitable"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("__await__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->Iterator[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Awaitable"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_typing_awaitable_missing_member() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/module.py"),
                    module_key: String::from("app.module"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Awaitable"),
                            kind: DeclarationKind::Import,
                            detail: String::from("typing.Awaitable"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("BadJob"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Awaitable"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Awaitable")],
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                typepython_graph::ModuleNode {
                    module_path: std::path::PathBuf::from("<typing-prelude>"),
                    module_key: String::from("typing"),
                    module_kind: SourceKind::Stub,
                    declarations: vec![
                        typepython_binding::Declaration {
                            name: String::from("Awaitable"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        typepython_binding::Declaration {
                            name: String::from("__await__"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->Iterator[Any]"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(typepython_binding::DeclarationOwner {
                                name: String::from("Awaitable"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
            ],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4008"));
        assert!(
            rendered.contains("does not implement interface member `__await__` from `Awaitable`")
        );
    }

    #[test]
    fn check_accepts_async_function_call_as_awaitable() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("fetch"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: true,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("task"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Awaitable[int]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("task"),
                    annotation: Some(String::from("Awaitable[int]")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("fetch")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_async_function_call_non_awaitable_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("fetch"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: true,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("result"),
                        kind: DeclarationKind::Value,
                        detail: String::from("int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("fetch")),
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("assigns `Awaitable[int]` where `result` expects `int`"));
    }

    #[test]
    fn check_accepts_direct_await_of_async_function() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("fetch"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: true,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: true,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![
                    typepython_binding::ReturnSite {
                        owner_name: String::from("fetch"),
                        owner_type_name: None,
                        value_type: Some(String::from("int")),
                        is_awaited: false,
                        value_callee: None,
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        line: 2,
                    },
                    typepython_binding::ReturnSite {
                        owner_name: String::from("build"),
                        owner_type_name: None,
                        value_type: Some(String::new()),
                        is_awaited: true,
                        value_callee: Some(String::from("fetch")),
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        line: 5,
                    },
                ],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_direct_await_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("fetch"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: true,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: true,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: vec![
                    typepython_binding::ReturnSite {
                        owner_name: String::from("fetch"),
                        owner_type_name: None,
                        value_type: Some(String::from("int")),
                        is_awaited: false,
                        value_callee: None,
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        line: 2,
                    },
                    typepython_binding::ReturnSite {
                        owner_name: String::from("build"),
                        owner_type_name: None,
                        value_type: Some(String::new()),
                        is_awaited: true,
                        value_callee: Some(String::from("fetch")),
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        line: 5,
                    },
                ],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("function `build` in module `src/app/module.py` returns `int` where `build` expects `str`"));
    }

    #[test]
    fn check_accepts_generator_yield_type() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("produce"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Generator[int, None, None]"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: vec![typepython_binding::YieldSite {
                    owner_name: String::from("produce"),
                    owner_type_name: None,
                    value_type: Some(String::from("int")),
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    is_yield_from: false,
                    line: 2,
                }],
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_generator_yield_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("produce"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Generator[int, None, None]"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: vec![typepython_binding::YieldSite {
                    owner_name: String::from("produce"),
                    owner_type_name: None,
                    value_type: Some(String::from("str")),
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    is_yield_from: false,
                    line: 2,
                }],
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("function `produce` in module `src/app/module.py` yields `str` where `Generator[int, ...]` expects `int`"));
    }

    #[test]
    fn check_accepts_yield_from_iterable_type() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("values"),
                        kind: DeclarationKind::Value,
                        detail: String::from("list[int]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("relay"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->Generator[int, None, None]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: vec![typepython_binding::YieldSite {
                    owner_name: String::from("relay"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    value_callee: None,
                    value_name: Some(String::from("values")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    is_yield_from: true,
                    line: 2,
                }],
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_unknown_direct_call_keyword() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(x:int,y:int)->None"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("build"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    keyword_names: vec![String::from("z")],
                }],
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("unknown keyword `z`"));
    }

    #[test]
    fn check_reports_unknown_member_access() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::from("unknown"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: vec![typepython_binding::MemberAccessSite {
                    owner_name: String::from("value"),
                    member: String::from("name"),
                    through_instance: false,
                }],
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4003"));
        assert!(rendered.contains("unsupported because `value` has type `unknown`"));
    }

    #[test]
    fn check_reports_unknown_method_call() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::from("unknown"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: vec![typepython_binding::MethodCallSite {
                    owner_name: String::from("value"),
                    method: String::from("run"),
                    through_instance: false,
                    arg_count: 0,
                    arg_types: Vec::new(),
                    keyword_names: Vec::new(),
                }],
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4003"));
        assert!(rendered.contains("method call `value.run`"));
    }

    #[test]
    fn check_reports_unknown_direct_call_on_import() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.tpy"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::TypePython,
                declarations: vec![Declaration {
                    name: String::from("external"),
                    kind: DeclarationKind::Import,
                    detail: String::from("pkg.external"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("external"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4003"));
        assert!(rendered.contains("call to `external`"));
    }

    #[test]
    fn check_reports_missing_direct_member_access() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
                returns: Vec::new(),
                member_accesses: vec![typepython_binding::MemberAccessSite {
                    owner_name: String::from("Box"),
                    member: String::from("missing"),
                    through_instance: false,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4002"));
        assert!(rendered.contains("has no member `missing`"));
    }

    #[test]
    fn check_reports_direct_method_call_arity_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:int,y:int)->None"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: vec![typepython_binding::MethodCallSite {
                    owner_name: String::from("Box"),
                    method: String::from("run"),
                    through_instance: false,
                    arg_count: 1,
                    arg_types: vec![String::from("int")],
                    keyword_names: Vec::new(),
                }],
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("call to `Box.run`"));
    }

    #[test]
    fn check_reports_direct_constructor_arity_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__init__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:int,y:int)->None"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("Box"),
                    arg_count: 1,
                    arg_types: Vec::new(),
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("call to `Box`"));
    }

    #[test]
    fn check_reports_direct_constructor_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__init__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:int,y:str)->None"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("Box"),
                    arg_count: 2,
                    arg_types: vec![String::from("str"), String::from("int")],
                    keyword_names: Vec::new(),
                }],
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("call to `Box`"));
        assert!(rendered.contains("parameter expects `int`"));
    }

    #[test]
    fn check_reports_invalid_top_level_override_usage() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::new(),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("top_level"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: true,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4005"));
        assert!(rendered.contains("top_level"));
    }

    #[test]
    fn check_reports_member_override_without_base_member() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::new(),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Child"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: true,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4005"));
        assert!(rendered.contains("no direct base member was found"));
    }

    #[test]
    fn check_reports_incompatible_direct_override_signature() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::new(),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:int)->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Base"),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:str)->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Child"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4005"));
        assert!(rendered.contains("incompatible signature or annotation"));
    }

    #[test]
    fn check_reports_incompatible_imported_override_signature() {
        let result = check(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/app/base.py"),
                    module_key: String::from("app.base"),
                    module_kind: SourceKind::Python,
                    declarations: vec![
                        Declaration {
                            name: String::from("Base"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self,x:int)->int"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Base"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                },
                ModuleNode {
                    module_path: PathBuf::from("src/app/child.tpy"),
                    module_key: String::from("app.child"),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![
                        Declaration {
                            name: String::from("Base"),
                            kind: DeclarationKind::Import,
                            detail: String::from("app.base.Base"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Child"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Base"),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Base")],
                        },
                        Declaration {
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self,x:str)->int"),
                            value_type: None,
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Child"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 2,
                },
            ],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4005"));
        assert!(rendered.contains("incompatible signature or annotation"));
    }

    #[test]
    fn check_reports_incompatible_override_method_kind() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::new(),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(cls)->None"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Class),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Base"),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self, exc_type, exc_val, exc_tb)->None"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Child"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4005"));
        assert!(rendered.contains("incompatible signature or annotation"));
    }

    #[test]
    fn check_reports_missing_explicit_override_when_required() {
        let result = check_with_options(
            &ModuleGraph {
                nodes: vec![ModuleNode {
                    module_path: PathBuf::from("src/app/module.tpy"),
                    module_key: String::new(),
                    module_kind: SourceKind::TypePython,
                    declarations: vec![
                        Declaration {
                            name: String::from("Base"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Base"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Child"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Base")],
                        },
                        Declaration {
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::new(),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Child"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    method_calls: Vec::new(),
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
                    summary_fingerprint: 1,
                }],
            },
            true,
            true,
            DiagnosticLevel::Warning,
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4005"));
        assert!(rendered.contains("missing @override"));
    }

    #[test]
    fn check_reports_missing_explicit_override_when_required_for_imported_base() {
        let result = check_with_options(
            &ModuleGraph {
                nodes: vec![
                    ModuleNode {
                        module_path: PathBuf::from("src/app/base.py"),
                        module_key: String::from("app.base"),
                        module_kind: SourceKind::Python,
                        declarations: vec![
                            Declaration {
                                name: String::from("Base"),
                                kind: DeclarationKind::Class,
                                detail: String::new(),
                                value_type: None,
                                method_kind: None,
                                class_kind: Some(DeclarationOwnerKind::Class),
                                owner: None,
                                is_async: false,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_deprecated: false,
                                deprecation_message: None,
                                is_final: false,
                                is_class_var: false,
                                bases: Vec::new(),
                            },
                            Declaration {
                                name: String::from("run"),
                                kind: DeclarationKind::Function,
                                detail: String::from("(self)->None"),
                                value_type: None,
                                method_kind: Some(typepython_syntax::MethodKind::Instance),
                                class_kind: None,
                                owner: Some(DeclarationOwner {
                                    name: String::from("Base"),
                                    kind: DeclarationOwnerKind::Class,
                                }),
                                is_async: false,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_deprecated: false,
                                deprecation_message: None,
                                is_final: false,
                                is_class_var: false,
                                bases: Vec::new(),
                            },
                        ],
                        calls: Vec::new(),
                        method_calls: Vec::new(),
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
                        summary_fingerprint: 1,
                    },
                    ModuleNode {
                        module_path: PathBuf::from("src/app/child.tpy"),
                        module_key: String::from("app.child"),
                        module_kind: SourceKind::TypePython,
                        declarations: vec![
                            Declaration {
                                name: String::from("Base"),
                                kind: DeclarationKind::Import,
                                detail: String::from("app.base.Base"),
                                value_type: None,
                                method_kind: None,
                                class_kind: None,
                                owner: None,
                                is_async: false,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_deprecated: false,
                                deprecation_message: None,
                                is_final: false,
                                is_class_var: false,
                                bases: Vec::new(),
                            },
                            Declaration {
                                name: String::from("Child"),
                                kind: DeclarationKind::Class,
                                detail: String::from("Base"),
                                value_type: None,
                                method_kind: None,
                                class_kind: Some(DeclarationOwnerKind::Class),
                                owner: None,
                                is_async: false,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_deprecated: false,
                                deprecation_message: None,
                                is_final: false,
                                is_class_var: false,
                                bases: vec![String::from("Base")],
                            },
                            Declaration {
                                name: String::from("run"),
                                kind: DeclarationKind::Function,
                                detail: String::from("(self)->None"),
                                value_type: None,
                                method_kind: Some(typepython_syntax::MethodKind::Instance),
                                class_kind: None,
                                owner: Some(DeclarationOwner {
                                    name: String::from("Child"),
                                    kind: DeclarationOwnerKind::Class,
                                }),
                                is_async: false,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_deprecated: false,
                                deprecation_message: None,
                                is_final: false,
                                is_class_var: false,
                                bases: Vec::new(),
                            },
                        ],
                        calls: Vec::new(),
                        method_calls: Vec::new(),
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
                        summary_fingerprint: 2,
                    },
                ],
            },
            true,
            true,
            DiagnosticLevel::Warning,
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4005"));
        assert!(rendered.contains("missing @override"));
    }

    #[test]
    fn check_reports_classvar_outside_class_scope() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::new(),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("VALUE"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: true,
                    bases: Vec::new(),
                }],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("ClassVar binding `VALUE`"));
    }

    #[test]
    fn check_accepts_classvar_inside_class_scope() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::new(),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("cache"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: true,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_direct_method_call_result_return() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("get"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(box:Box)->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("box")),
                    value_method_name: Some(String::from("get")),
                    value_method_through_instance: false,
                    line: 2,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_direct_method_call_result_assignment() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("get"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("box"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Box"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("result"),
                        kind: DeclarationKind::Value,
                        detail: String::from("str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("box")),
                    value_method_name: Some(String::from("get")),
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_direct_method_call_result_assignment_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("get"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("box"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Box"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("result"),
                        kind: DeclarationKind::Value,
                        detail: String::from("int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("box")),
                    value_method_name: Some(String::from("get")),
                    value_method_through_instance: false,
                    owner_name: None,
                    owner_type_name: None,
                    line: 1,
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("assigns `str` where `result` expects `int`"));
    }

    #[test]
    fn check_reports_direct_method_call_result_return_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("get"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(box:Box)->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("box")),
                    value_method_name: Some(String::from("get")),
                    value_method_through_instance: false,
                    line: 2,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `str` where `build` expects `int`"));
    }

    #[test]
    fn check_accepts_direct_method_call_result_through_instance() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("get"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("make_box"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->Box"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("make_box")),
                    value_method_name: Some(String::from("get")),
                    value_method_through_instance: true,
                    line: 2,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_direct_method_call_result_through_instance_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("get"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("make_box"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->Box"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("make_box")),
                    value_method_name: Some(String::from("get")),
                    value_method_through_instance: true,
                    line: 2,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `str` where `build` expects `int`"));
    }

    #[test]
    fn check_accepts_for_loop_target_type_in_local_assignment() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(values:list[int])->None"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: vec![typepython_binding::ForSite {
                    target_name: String::from("item"),
                    target_names: Vec::new(),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    iter_type: Some(String::new()),
                    iter_is_awaited: false,
                    iter_callee: None,
                    iter_name: Some(String::from("values")),
                    iter_member_owner_name: None,
                    iter_member_name: None,
                    iter_member_through_instance: false,
                    iter_method_owner_name: None,
                    iter_method_name: None,
                    iter_method_through_instance: false,
                    line: 2,
                }],
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("item")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 3,
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_for_loop_target_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(values:list[int])->None"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: vec![typepython_binding::ForSite {
                    target_name: String::from("item"),
                    target_names: Vec::new(),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    iter_type: Some(String::new()),
                    iter_is_awaited: false,
                    iter_callee: None,
                    iter_name: Some(String::from("values")),
                    iter_member_owner_name: None,
                    iter_member_name: None,
                    iter_member_through_instance: false,
                    iter_method_owner_name: None,
                    iter_method_name: None,
                    iter_method_through_instance: false,
                    line: 2,
                }],
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: vec![typepython_binding::AssignmentSite {
                    name: String::from("result"),
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("item")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 3,
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("assigns `int` where local `result` expects `str`"));
    }

    #[test]
    fn check_accepts_tuple_for_loop_target_type_in_return() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(pairs:tuple[tuple[int, str]])->str"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("b")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 4,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: vec![typepython_binding::ForSite {
                    target_name: String::new(),
                    target_names: vec![String::from("a"), String::from("b")],
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    iter_type: Some(String::new()),
                    iter_is_awaited: false,
                    iter_callee: None,
                    iter_name: Some(String::from("pairs")),
                    iter_member_owner_name: None,
                    iter_member_name: None,
                    iter_member_through_instance: false,
                    iter_method_owner_name: None,
                    iter_method_name: None,
                    iter_method_through_instance: false,
                    line: 2,
                }],
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_sequence_for_loop_target_type_in_return() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(pairs:list[Sequence[int]])->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("b")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 4,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: vec![typepython_binding::ForSite {
                    target_name: String::new(),
                    target_names: vec![String::from("a"), String::from("b")],
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    iter_type: Some(String::new()),
                    iter_is_awaited: false,
                    iter_callee: None,
                    iter_name: Some(String::from("pairs")),
                    iter_member_owner_name: None,
                    iter_member_name: None,
                    iter_member_through_instance: false,
                    iter_method_owner_name: None,
                    iter_method_name: None,
                    iter_method_through_instance: false,
                    line: 2,
                }],
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_tuple_for_loop_target_type_mismatch() {
        let graph = ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(pairs:tuple[tuple[int, str]])->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("b")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 4,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: vec![typepython_binding::ForSite {
                    target_name: String::new(),
                    target_names: vec![String::from("a"), String::from("b")],
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    iter_type: Some(String::new()),
                    iter_is_awaited: false,
                    iter_callee: None,
                    iter_name: Some(String::from("pairs")),
                    iter_member_owner_name: None,
                    iter_member_name: None,
                    iter_member_through_instance: false,
                    iter_method_owner_name: None,
                    iter_method_name: None,
                    iter_method_through_instance: false,
                    line: 2,
                }],
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        };

        let result = check(&graph);

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `str` where `build` expects `int`"));
    }

    #[test]
    fn check_reports_tuple_for_loop_target_arity_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(pairs:tuple[tuple[int]])->None"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: vec![typepython_binding::ForSite {
                    target_name: String::new(),
                    target_names: vec![String::from("a"), String::from("b")],
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    iter_type: Some(String::new()),
                    iter_is_awaited: false,
                    iter_callee: None,
                    iter_name: Some(String::from("pairs")),
                    iter_member_owner_name: None,
                    iter_member_name: None,
                    iter_member_through_instance: false,
                    iter_method_owner_name: None,
                    iter_method_name: None,
                    iter_method_through_instance: false,
                    line: 2,
                }],
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("destructures `for` target `(a, b)` with 2 name(s) from tuple element type `tuple[int]` with 1 element(s)"));
    }

    #[test]
    fn check_accepts_with_target_type_in_return() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Manager"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__enter__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Manager"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__exit__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self, exc_type, exc_val, exc_tb)->None"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Manager"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(manager:Manager)->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 4,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: vec![typepython_binding::WithSite {
                    target_name: Some(String::from("value")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    context_type: Some(String::new()),
                    context_is_awaited: false,
                    context_callee: None,
                    context_name: Some(String::from("manager")),
                    context_member_owner_name: None,
                    context_member_name: None,
                    context_member_through_instance: false,
                    context_method_owner_name: None,
                    context_method_name: None,
                    context_method_through_instance: false,
                    line: 2,
                }],
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_with_target_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Manager"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__enter__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->int"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Manager"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(manager:Manager)->None"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: vec![typepython_binding::WithSite {
                    target_name: Some(String::from("value")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    context_type: Some(String::new()),
                    context_is_awaited: false,
                    context_callee: None,
                    context_name: Some(String::from("manager")),
                    context_member_owner_name: None,
                    context_member_name: None,
                    context_member_through_instance: false,
                    context_method_owner_name: None,
                    context_method_name: None,
                    context_method_through_instance: false,
                    line: 2,
                }],
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("lacks compatible `__enter__`/`__exit__` members"));
    }

    #[test]
    fn check_accepts_except_handler_binding_type() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->ValueError"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("e")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 5,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: vec![typepython_binding::ExceptHandlerSite {
                    exception_type: String::from("ValueError"),
                    binding_name: Some(String::from("e")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                    end_line: 5,
                }],
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_except_handler_binding_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->TypeError"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("e")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 5,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: vec![typepython_binding::ExceptHandlerSite {
                    exception_type: String::from("ValueError"),
                    binding_name: Some(String::from("e")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                    end_line: 5,
                }],
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `ValueError` where `build` expects `TypeError`"));
    }

    #[test]
    fn check_does_not_keep_except_binding_after_handler() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->ValueError"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("e")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 6,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: vec![typepython_binding::ExceptHandlerSite {
                    exception_type: String::from("ValueError"),
                    binding_name: Some(String::from("e")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                    end_line: 5,
                }],
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_tuple_except_handler_binding_type() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->Union[ValueError, TypeError]"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("e")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 5,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: vec![typepython_binding::ExceptHandlerSite {
                    exception_type: String::from("(ValueError, TypeError)"),
                    binding_name: Some(String::from("e")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                    end_line: 5,
                }],
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_tuple_except_handler_binding_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->ValueError"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("e")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 5,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: vec![typepython_binding::ExceptHandlerSite {
                    exception_type: String::from("(ValueError, TypeError)"),
                    binding_name: Some(String::from("e")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                    end_line: 5,
                }],
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(
            rendered.contains(
                "returns `Union[ValueError, TypeError]` where `build` expects `ValueError`"
            )
        );
    }

    #[test]
    fn check_accepts_bare_except_handler_binding_type() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->BaseException"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("e")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 5,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: vec![typepython_binding::ExceptHandlerSite {
                    exception_type: String::from("BaseException"),
                    binding_name: Some(String::from("e")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                    end_line: 5,
                }],
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_bare_except_handler_binding_type_mismatch() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->ValueError"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("e")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 5,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: vec![typepython_binding::ExceptHandlerSite {
                    exception_type: String::from("BaseException"),
                    binding_name: Some(String::from("e")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                    end_line: 5,
                }],
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `BaseException` where `build` expects `ValueError`"));
    }

    #[test]
    fn check_reports_non_exhaustive_sealed_match() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.tpy"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("Expr"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::SealedClass),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Add"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Expr")],
                    },
                    Declaration {
                        name: String::from("Mul"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Expr")],
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(expr:Expr)->None"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: vec![typepython_binding::MatchSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    subject_type: Some(String::new()),
                    subject_is_awaited: false,
                    subject_callee: None,
                    subject_name: Some(String::from("expr")),
                    subject_member_owner_name: None,
                    subject_member_name: None,
                    subject_member_through_instance: false,
                    subject_method_owner_name: None,
                    subject_method_name: None,
                    subject_method_through_instance: false,
                    cases: vec![typepython_binding::MatchCaseSite {
                        patterns: vec![typepython_binding::MatchPatternSite::Class(String::from(
                            "Add",
                        ))],
                        has_guard: false,
                        line: 3,
                    }],
                    line: 2,
                }],
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4009"));
        assert!(rendered.contains("missing subclasses: Mul"));
    }

    #[test]
    fn check_accepts_exhaustive_sealed_match_with_wildcard() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.tpy"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("Expr"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::SealedClass),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Add"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Expr")],
                    },
                    Declaration {
                        name: String::from("Mul"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Expr")],
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(expr:Expr)->None"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: vec![typepython_binding::MatchSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    subject_type: Some(String::new()),
                    subject_is_awaited: false,
                    subject_callee: None,
                    subject_name: Some(String::from("expr")),
                    subject_member_owner_name: None,
                    subject_member_name: None,
                    subject_member_through_instance: false,
                    subject_method_owner_name: None,
                    subject_method_name: None,
                    subject_method_through_instance: false,
                    cases: vec![
                        typepython_binding::MatchCaseSite {
                            patterns: vec![typepython_binding::MatchPatternSite::Class(
                                String::from("Add"),
                            )],
                            has_guard: false,
                            line: 3,
                        },
                        typepython_binding::MatchCaseSite {
                            patterns: vec![typepython_binding::MatchPatternSite::Wildcard],
                            has_guard: false,
                            line: 5,
                        },
                    ],
                    line: 2,
                }],
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_if_is_not_none_narrowing_for_return() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Optional[str])->str"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 3,
                }],
                yields: Vec::new(),
                if_guards: vec![typepython_binding::IfGuardSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_binding::GuardConditionSite::IsNone {
                        name: String::from("value"),
                        negated: true,
                    }),
                    line: 2,
                    true_start_line: 3,
                    true_end_line: 3,
                    false_start_line: None,
                    false_end_line: None,
                }],
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_assert_is_not_none_narrowing_for_return() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Optional[str])->str"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 3,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: vec![typepython_binding::AssertGuardSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_binding::GuardConditionSite::IsNone {
                        name: String::from("value"),
                        negated: true,
                    }),
                    line: 2,
                }],
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_isinstance_tuple_narrowing_for_return() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Union[str, bytes, int])->Union[str, bytes]"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 3,
                }],
                yields: Vec::new(),
                if_guards: vec![typepython_binding::IfGuardSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_binding::GuardConditionSite::IsInstance {
                        name: String::from("value"),
                        types: vec![String::from("str"), String::from("bytes")],
                    }),
                    line: 2,
                    true_start_line: 3,
                    true_end_line: 3,
                    false_start_line: None,
                    false_end_line: None,
                }],
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_typeguard_true_branch_narrowing() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("is_text"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(value:Union[str, int])->TypeGuard[str]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(value:Union[str, int])->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 3,
                }],
                yields: Vec::new(),
                if_guards: vec![typepython_binding::IfGuardSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_binding::GuardConditionSite::PredicateCall {
                        name: String::from("value"),
                        callee: String::from("is_text"),
                    }),
                    line: 2,
                    true_start_line: 3,
                    true_end_line: 3,
                    false_start_line: None,
                    false_end_line: None,
                }],
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_typeis_false_branch_narrowing() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("is_text"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(value:Union[str, int])->TypeIs[str]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(value:Union[str, int])->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 5,
                }],
                yields: Vec::new(),
                if_guards: vec![typepython_binding::IfGuardSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_binding::GuardConditionSite::PredicateCall {
                        name: String::from("value"),
                        callee: String::from("is_text"),
                    }),
                    line: 2,
                    true_start_line: 3,
                    true_end_line: 3,
                    false_start_line: Some(5),
                    false_end_line: Some(5),
                }],
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_typeis_post_if_fallthrough_narrowing() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("is_text"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(value:Union[str, int])->TypeIs[str]"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(value:Union[str, int])->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![
                    typepython_binding::ReturnSite {
                        owner_name: String::from("build"),
                        owner_type_name: None,
                        value_type: Some(String::from("int")),
                        is_awaited: false,
                        value_callee: None,
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        line: 3,
                    },
                    typepython_binding::ReturnSite {
                        owner_name: String::from("build"),
                        owner_type_name: None,
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: None,
                        value_name: Some(String::from("value")),
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        line: 4,
                    },
                ],
                yields: Vec::new(),
                if_guards: vec![typepython_binding::IfGuardSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_binding::GuardConditionSite::PredicateCall {
                        name: String::from("value"),
                        callee: String::from("is_text"),
                    }),
                    line: 2,
                    true_start_line: 3,
                    true_end_line: 3,
                    false_start_line: None,
                    false_end_line: None,
                }],
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_boolean_composition_narrowing() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:str | None)->str"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 3,
                }],
                yields: Vec::new(),
                if_guards: vec![typepython_binding::IfGuardSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_binding::GuardConditionSite::And(vec![
                        typepython_binding::GuardConditionSite::Not(Box::new(
                            typepython_binding::GuardConditionSite::IsNone {
                                name: String::from("value"),
                                negated: false,
                            },
                        )),
                        typepython_binding::GuardConditionSite::IsInstance {
                            name: String::from("value"),
                            types: vec![String::from("str")],
                        },
                    ])),
                    line: 2,
                    true_start_line: 3,
                    true_end_line: 3,
                    false_start_line: None,
                    false_end_line: None,
                }],
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_truthiness_narrowing_for_bool_optional() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(flag:Optional[Literal[True]])->Literal[True]"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("flag")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 3,
                }],
                yields: Vec::new(),
                if_guards: vec![typepython_binding::IfGuardSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_binding::GuardConditionSite::TruthyName {
                        name: String::from("flag"),
                    }),
                    line: 2,
                    true_start_line: 3,
                    true_end_line: 3,
                    false_start_line: None,
                    false_end_line: None,
                }],
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_does_not_over_narrow_truthiness_for_int_optional() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Optional[int])->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 3,
                }],
                yields: Vec::new(),
                if_guards: vec![typepython_binding::IfGuardSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_binding::GuardConditionSite::TruthyName {
                        name: String::from("value"),
                    }),
                    line: 2,
                    true_start_line: 3,
                    true_end_line: 3,
                    false_start_line: None,
                    false_end_line: None,
                }],
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `Optional[int]` where `build` expects `int`"));
    }

    #[test]
    fn check_invalidates_narrowing_after_augassign() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(value:Optional[int])->int"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 4,
                }],
                yields: Vec::new(),
                if_guards: vec![typepython_binding::IfGuardSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_binding::GuardConditionSite::IsNone {
                        name: String::from("value"),
                        negated: true,
                    }),
                    line: 2,
                    true_start_line: 3,
                    true_end_line: 4,
                    false_start_line: None,
                    false_end_line: None,
                }],
                asserts: Vec::new(),
                invalidations: vec![typepython_binding::InvalidationSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    names: vec![String::from("value")],
                    line: 3,
                }],
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("returns `Optional[int]` where `build` expects `int`"));
    }

    #[test]
    fn check_joins_branch_local_assignments_after_if() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("build"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(flag:bool)->Union[str, int]"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("result")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 5,
                }],
                yields: Vec::new(),
                if_guards: vec![typepython_binding::IfGuardSite {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_binding::GuardConditionSite::TruthyName {
                        name: String::from("flag"),
                    }),
                    line: 2,
                    true_start_line: 3,
                    true_end_line: 3,
                    false_start_line: Some(4),
                    false_end_line: Some(4),
                }],
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: vec![
                    typepython_binding::AssignmentSite {
                        name: String::from("result"),
                        annotation: None,
                        value_type: Some(String::from("str")),
                        is_awaited: false,
                        value_callee: None,
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: Some(String::from("build")),
                        owner_type_name: None,
                        line: 3,
                    },
                    typepython_binding::AssignmentSite {
                        name: String::from("result"),
                        annotation: None,
                        value_type: Some(String::from("int")),
                        is_awaited: false,
                        value_callee: None,
                        value_name: None,
                        value_member_owner_name: None,
                        value_member_name: None,
                        value_member_through_instance: false,
                        value_method_owner_name: None,
                        value_method_name: None,
                        value_method_through_instance: false,
                        owner_name: Some(String::from("build")),
                        owner_type_name: None,
                        line: 4,
                    },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_direct_recursive_type_alias() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.tpy"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::TypePython,
                declarations: vec![Declaration {
                    name: String::from("Tree"),
                    kind: DeclarationKind::TypeAlias,
                    detail: String::from("list[Tree]"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("type alias `Tree`"));
        assert!(rendered.contains("Tree -> Tree"));
    }

    #[test]
    fn check_reports_deprecated_import_and_call_when_enabled() {
        let result = check_with_options(
            &ModuleGraph {
                nodes: vec![
                    ModuleNode {
                        module_path: PathBuf::from("src/lib/deps.py"),
                        module_key: String::from("lib.deps"),
                        module_kind: SourceKind::Python,
                        declarations: vec![Declaration {
                            name: String::from("old"),
                            kind: DeclarationKind::Function,
                            detail: String::from("()->int"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: true,
                            deprecation_message: Some(String::from("use new instead")),
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        }],
                        calls: Vec::new(),
                        method_calls: Vec::new(),
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
                        summary_fingerprint: 1,
                    },
                    ModuleNode {
                        module_path: PathBuf::from("src/app/module.tpy"),
                        module_key: String::from("app.module"),
                        module_kind: SourceKind::TypePython,
                        declarations: vec![Declaration {
                            name: String::from("old"),
                            kind: DeclarationKind::Import,
                            detail: String::from("lib.deps.old"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        }],
                        calls: vec![typepython_binding::CallSite {
                            callee: String::from("old"),
                            arg_count: 0,
                            arg_types: Vec::new(),
                            keyword_names: Vec::new(),
                        }],
                        method_calls: Vec::new(),
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
                        summary_fingerprint: 2,
                    },
                ],
            },
            false,
            true,
            DiagnosticLevel::Warning,
        );

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4101"));
        assert!(rendered.contains("imports deprecated declaration `old`"));
        assert!(rendered.contains("calls deprecated declaration `old`"));
        assert!(rendered.contains("use new instead"));
    }

    #[test]
    fn check_ignores_deprecated_uses_when_configured() {
        let result = check_with_options(
            &ModuleGraph {
                nodes: vec![
                    ModuleNode {
                        module_path: PathBuf::from("src/lib/deps.py"),
                        module_key: String::from("lib.deps"),
                        module_kind: SourceKind::Python,
                        declarations: vec![Declaration {
                            name: String::from("old"),
                            kind: DeclarationKind::Function,
                            detail: String::from("()->int"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: true,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        }],
                        calls: Vec::new(),
                        method_calls: Vec::new(),
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
                        summary_fingerprint: 1,
                    },
                    ModuleNode {
                        module_path: PathBuf::from("src/app/module.tpy"),
                        module_key: String::from("app.module"),
                        module_kind: SourceKind::TypePython,
                        declarations: vec![Declaration {
                            name: String::from("old"),
                            kind: DeclarationKind::Import,
                            detail: String::from("lib.deps.old"),
                            value_type: None,
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        }],
                        calls: vec![typepython_binding::CallSite {
                            callee: String::from("old"),
                            arg_count: 0,
                            arg_types: Vec::new(),
                            keyword_names: Vec::new(),
                        }],
                        method_calls: Vec::new(),
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
                        summary_fingerprint: 2,
                    },
                ],
            },
            false,
            true,
            DiagnosticLevel::Ignore,
        );

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_mutual_recursive_type_aliases() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.tpy"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("Left"),
                        kind: DeclarationKind::TypeAlias,
                        detail: String::from("Right"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Right"),
                        kind: DeclarationKind::TypeAlias,
                        detail: String::from("Left"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
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
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("type alias `Left`"));
        assert!(rendered.contains("Left -> Right -> Left"));
    }

    #[test]
    fn check_accepts_self_return_through_inherited_method_call() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("clone"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->Self"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("SubBox"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Box")],
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(box:SubBox)->SubBox"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("box")),
                    value_method_name: Some(String::from("clone")),
                    value_method_through_instance: false,
                    line: 3,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_self_parameter_annotation_in_method_call() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("merge"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,other:Self)->Self"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
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
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: vec![typepython_binding::MethodCallSite {
                    owner_name: String::from("Box"),
                    method: String::from("merge"),
                    through_instance: true,
                    arg_count: 1,
                    arg_types: vec![String::from("Box")],
                    keyword_names: Vec::new(),
                }],
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_self_typed_attribute_access() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Node"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("next"),
                        kind: DeclarationKind::Value,
                        detail: String::from("Self"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: Some(typepython_binding::DeclarationOwner {
                            name: String::from("Node"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(node:Node)->Node"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("node")),
                    value_member_name: Some(String::from("next")),
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 3,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_enum_member_access_as_enum_type() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Enum"),
                        kind: DeclarationKind::Import,
                        detail: String::from("enum.Enum"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Color"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Enum")],
                    },
                    Declaration {
                        name: String::from("RED"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        value_type: Some(String::from("int")),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Color"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->Color"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("Color")),
                    value_member_name: Some(String::from("RED")),
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 2,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_with_without_target() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Manager"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__enter__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Manager"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__exit__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self, exc_type, exc_val, exc_tb)->None"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Manager"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(manager:Manager)->None"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: vec![typepython_binding::WithSite {
                    target_name: None,
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    context_type: Some(String::new()),
                    context_is_awaited: false,
                    context_callee: None,
                    context_name: Some(String::from("manager")),
                    context_member_owner_name: None,
                    context_member_name: None,
                    context_member_through_instance: false,
                    context_method_owner_name: None,
                    context_method_name: None,
                    context_method_through_instance: false,
                    line: 2,
                }],
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_multiple_with_items() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_key: String::from("app.module"),
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("A"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__enter__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->int"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("A"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__exit__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self, exc_type, exc_val, exc_tb)->None"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("A"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("B"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__enter__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("B"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("__exit__"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self, exc_type, exc_val, exc_tb)->None"),
                        value_type: None,
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("B"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("build"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(a:A,b:B)->str"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                member_accesses: Vec::new(),
                returns: vec![typepython_binding::ReturnSite {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("y")),
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: None,
                    value_method_name: None,
                    value_method_through_instance: false,
                    line: 4,
                }],
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: vec![
                    typepython_binding::WithSite {
                        target_name: Some(String::from("x")),
                        owner_name: Some(String::from("build")),
                        owner_type_name: None,
                        context_type: Some(String::new()),
                        context_is_awaited: false,
                        context_callee: None,
                        context_name: Some(String::from("a")),
                        context_member_owner_name: None,
                        context_member_name: None,
                        context_member_through_instance: false,
                        context_method_owner_name: None,
                        context_method_name: None,
                        context_method_through_instance: false,
                        line: 2,
                    },
                    typepython_binding::WithSite {
                        target_name: Some(String::from("y")),
                        owner_name: Some(String::from("build")),
                        owner_type_name: None,
                        context_type: Some(String::new()),
                        context_is_awaited: false,
                        context_callee: None,
                        context_name: Some(String::from("b")),
                        context_member_owner_name: None,
                        context_member_name: None,
                        context_member_through_instance: false,
                        context_method_owner_name: None,
                        context_method_name: None,
                        context_method_through_instance: false,
                        line: 2,
                    },
                ],
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 1,
                calls: Vec::new(),
                method_calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }
}

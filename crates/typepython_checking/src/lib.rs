//! Type-checking boundary for TypePython.

use std::collections::{BTreeMap, BTreeSet};

use typepython_binding::{Declaration, DeclarationKind, DeclarationOwnerKind};
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_graph::ModuleGraph;
use typepython_syntax::SourceKind;

/// Result of running the checker.
#[derive(Debug, Clone, Default)]
pub struct CheckResult {
    /// Diagnostics produced by the checker.
    pub diagnostics: DiagnosticReport,
}

/// Runs the placeholder checker over the module graph.
#[must_use]
pub fn check(graph: &ModuleGraph) -> CheckResult {
    check_with_options(graph, false)
}

#[must_use]
pub fn check_with_options(graph: &ModuleGraph, require_explicit_overrides: bool) -> CheckResult {
    let mut diagnostics = DiagnosticReport::default();

    for node in &graph.nodes {
        for resolution_diagnostic in unresolved_import_diagnostics(node, &graph.nodes) {
            diagnostics.push(resolution_diagnostic);
        }
        for call_diagnostic in direct_call_arity_diagnostics(node, &graph.nodes) {
            diagnostics.push(call_diagnostic);
        }
        for duplicate in duplicate_diagnostics(&node.module_path, node.module_kind, &node.declarations) {
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
    }

    CheckResult { diagnostics }
}

fn direct_call_arity_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    node.calls
        .iter()
        .filter_map(|call| {
            let target = if let Some(local) = node
                .declarations
                .iter()
                .find(|declaration| declaration.name == call.callee && declaration.owner.is_none() && declaration.kind == DeclarationKind::Function)
            {
                Some(local)
            } else {
                let import = node
                    .declarations
                    .iter()
                    .find(|declaration| declaration.kind == DeclarationKind::Import && declaration.name == call.callee)?;
                let (module_key, symbol_name) = import.detail.rsplit_once('.')?;
                let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
                target_node
                    .declarations
                    .iter()
                    .find(|declaration| declaration.name == symbol_name && declaration.owner.is_none() && declaration.kind == DeclarationKind::Function)
            }?;

            let expected = direct_param_count(&target.detail)?;
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
    if inner.is_empty() {
        Some(0)
    } else {
        Some(inner.split(',').count())
    }
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
                        nodes.iter().find(|candidate| candidate.module_key == module_key).map(|target| {
                            target.declarations.iter().any(|declaration| {
                                declaration.owner.is_none() && declaration.name == symbol_name
                            })
                        })
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

fn override_compatibility_diagnostics<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    let declarations = &node.declarations;
    let mut diagnostics = Vec::new();

    for class_declaration in declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Class && declaration.owner.is_none())
    {
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
                    if base_member.detail != member.detail
                        || base_member.method_kind != member.method_kind
                    {
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
                            && candidate.owner.as_ref().is_some_and(|candidate_owner| candidate_owner.name == base_decl.name)
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

    for class_declaration in declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Class && declaration.owner.is_none())
    {
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
                declaration.owner.as_ref().is_some_and(|owner| owner.name == class_declaration.name)
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
            for ((abstract_owner, member_name), member_kind) in abstract_member_index(&base_node.declarations) {
                if abstract_owner != base_decl.name {
                    continue;
                }

                let implemented = declarations.iter().any(|declaration| {
                    declaration.owner.as_ref().is_some_and(|owner| owner.name == class_declaration.name)
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
        .filter(|declaration| declaration.kind == DeclarationKind::Class && declaration.owner.is_none())
        .filter_map(|class_declaration| {
            let own_abstract = declarations.iter().any(|declaration| {
                declaration.owner.as_ref().is_some_and(|owner| owner.name == class_declaration.name)
                    && declaration.is_abstract_method
            });
            let inherited_abstract = class_declaration.bases.iter().any(|base| {
                let Some((base_node, base_decl)) = resolve_direct_base(nodes, node, base) else {
                    return false;
                };
                abstract_member_index(&base_node.declarations).iter().any(|((abstract_owner, member_name), member_kind)| {
                    abstract_owner == &base_decl.name
                        && !declarations.iter().any(|declaration| {
                            declaration.owner.as_ref().is_some_and(|owner| owner.name == class_declaration.name)
                                && declaration.name == *member_name
                                && declaration.kind == *member_kind
                                && !declaration.is_abstract_method
                        })
                })
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
                resolve_direct_base(nodes, node, &call.callee)
                    .and_then(|(base_node, declaration)| {
                        let own_abstract = base_node.declarations.iter().any(|declaration_member| {
                            declaration_member.owner.as_ref().is_some_and(|owner| owner.name == declaration.name)
                                && declaration_member.is_abstract_method
                        });
                        let inherited_abstract = declaration.bases.iter().any(|base| {
                            let Some((resolved_node, resolved_decl)) = resolve_direct_base(nodes, base_node, base) else {
                                return false;
                            };
                            abstract_member_index(&resolved_node.declarations).iter().any(|((abstract_owner, member_name), member_kind)| {
                                abstract_owner == &resolved_decl.name
                                    && !base_node.declarations.iter().any(|declaration_member| {
                                        declaration_member.owner.as_ref().is_some_and(|owner| owner.name == declaration.name)
                                            && declaration_member.name == *member_name
                                            && declaration_member.kind == *member_kind
                                            && !declaration_member.is_abstract_method
                                    })
                            })
                        });

                        (own_abstract || inherited_abstract).then_some(declaration.name.as_str())
                    })
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

fn abstract_member_index(declarations: &[Declaration]) -> BTreeMap<(String, String), DeclarationKind> {
    declarations
        .iter()
        .filter(|declaration| declaration.owner.is_some() && declaration.is_abstract_method)
        .filter_map(|declaration| {
            declaration.owner.as_ref().map(|owner| ((owner.name.clone(), declaration.name.clone()), declaration.kind))
        })
        .collect()
}

fn resolve_direct_base<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    base_name: &str,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    if let Some(local) = node
        .declarations
        .iter()
        .find(|declaration| declaration.name == base_name && declaration.owner.is_none() && declaration.kind == DeclarationKind::Class)
    {
        return Some((node, local));
    }

    let import = node
        .declarations
        .iter()
        .find(|declaration| declaration.kind == DeclarationKind::Import && declaration.name == base_name)?;
    let (module_key, symbol_name) = import.detail.rsplit_once('.')?;
    let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
    let target_decl = target_node
        .declarations
        .iter()
        .find(|declaration| declaration.name == symbol_name && declaration.owner.is_none() && declaration.kind == DeclarationKind::Class)?;
    Some((target_node, target_decl))
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
                        resolve_direct_base(nodes, node, base).is_some_and(|(base_node, base_decl)| {
                            base_node.declarations.iter().any(|candidate| {
                                candidate.name == declaration.name
                                    && candidate.owner.as_ref().is_some_and(|candidate_owner| candidate_owner.name == base_decl.name)
                            })
                        })
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

    for class_declaration in declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Class && declaration.owner.is_none())
    {
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
            if base_decl.class_kind != Some(DeclarationOwnerKind::Interface) {
                continue;
            }

            let interface_members: BTreeMap<_, _> = base_node
                .declarations
                .iter()
                .filter(|declaration| declaration.kind == DeclarationKind::Value || declaration.kind == DeclarationKind::Function)
                .filter_map(|declaration| {
                    let owner = declaration.owner.as_ref()?;
                    (owner.name == base_decl.name).then(|| {
                        ((owner.name.clone(), declaration.name.clone()), (declaration.kind, declaration.method_kind, declaration.detail.clone()))
                    })
                })
                .collect::<BTreeMap<_, _>>();

            for ((interface_name, member_name), (member_kind, member_method_kind, member_detail)) in &interface_members {
                if interface_name != &base_decl.name {
                    continue;
                }

                let implemented = declarations.iter().find(|declaration| {
                    declaration.owner.as_ref().is_some_and(|owner| owner.name == class_declaration.name)
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
            if let Some(diagnostic) = classvar_placement_diagnostic(
                module_path,
                owner_name.as_deref(),
                declaration,
            ) {
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
            } else if is_permitted_external_overload_group(module_kind, duplicate, &space_declarations) {
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

    let total_count = declarations
        .iter()
        .filter(|declaration| declaration.name == duplicate)
        .count();
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
            None => format!(
                "module `{}` reassigns Final binding `{duplicate}`",
                module_path.display()
            ),
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
        spaces
            .entry((key, owner_kind))
            .or_default()
            .push(declaration.clone());
    }

    spaces
        .into_iter()
        .map(|((owner_name, owner_kind), declarations)| (owner_name, owner_kind, declarations))
        .collect()
}

fn duplicate_message(module_path: &std::path::Path, owner_name: Option<&str>, duplicate: &str) -> String {
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

    if overload_count >= 1 && function_count == 1 && overload_count + function_count == kinds.len() {
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
        0 => overload_shape_message(module_path, owner_name, duplicate, "without a concrete implementation"),
        1 => return None,
        _ => overload_shape_message(module_path, owner_name, duplicate, "with more than one concrete implementation"),
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
    use std::path::PathBuf;
    use typepython_binding::{Declaration, DeclarationKind, DeclarationOwner, DeclarationOwnerKind};
    use typepython_graph::{ModuleGraph, ModuleNode};
    use typepython_syntax::SourceKind;

    #[test]
    fn check_reports_duplicate_module_symbols() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_key: String::new(),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(result.diagnostics.has_errors());
        assert!(rendered.contains("TPY4004"));
        assert!(rendered.contains("`User`"));
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
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4004"));
        assert!(rendered.contains("more than one concrete implementation"));
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
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Parser"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Parser"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                    is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: true,
                        is_class_var: false,
                    bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("MAX_SIZE"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                    is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                    bases: Vec::new(),
                    },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: true,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: true,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Derived"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Derived"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4006"));
        assert!(rendered.contains("overrides Final member `limit` from base `Base`"));
    }

    #[test]
    fn check_reports_subclassing_final_class() {
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: true,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                ],
                calls: Vec::new(),
                summary_fingerprint: 1,
            }],
        });

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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: true,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    }],
                    calls: Vec::new(),
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
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Child"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Base"),
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Base")],
                        },
                    ],
                    calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: true,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Child"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("SupportsClose"),
                            kind: DeclarationOwnerKind::Interface,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Widget"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("SupportsClose")],
                    },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Interface),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->int"),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("SupportsClose"),
                            kind: DeclarationOwnerKind::Interface,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Widget"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("SupportsClose")],
                    },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->str"),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Widget"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
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
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Interface),
                            owner: None,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("close"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->int"),
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("SupportsClose"),
                                kind: DeclarationOwnerKind::Interface,
                            }),
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
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
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Widget"),
                            kind: DeclarationKind::Class,
                            detail: String::from("SupportsClose"),
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("SupportsClose")],
                        },
                        Declaration {
                            name: String::from("close"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->str"),
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Widget"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: true,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: true,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("Base"),
                    arg_count: 0,
                }],
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
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self)->None"),
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Base"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_override: false,
                            is_abstract_method: true,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    }],
                    calls: vec![typepython_binding::CallSite {
                        callee: String::from("Base"),
                        arg_count: 0,
                    }],
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
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: Vec::new(),
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
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("build"),
                    arg_count: 1,
                }],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("expects 2 positional argument(s) but received 1"));
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
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: true,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Child"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: true,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:int)->int"),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Base"),
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self,x:str)->int"),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Child"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
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
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self,x:int)->int"),
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Base"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
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
                            method_kind: None,
                            class_kind: None,
                            owner: None,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Child"),
                            kind: DeclarationKind::Class,
                            detail: String::from("Base"),
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Base")],
                        },
                        Declaration {
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::from("(self,x:str)->int"),
                            method_kind: Some(typepython_syntax::MethodKind::Instance),
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Child"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(cls)->None"),
                        method_kind: Some(typepython_syntax::MethodKind::Class),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Child"),
                        kind: DeclarationKind::Class,
                        detail: String::from("Base"),
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                    Declaration {
                        name: String::from("run"),
                        kind: DeclarationKind::Function,
                        detail: String::from("(self)->None"),
                        method_kind: Some(typepython_syntax::MethodKind::Instance),
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Child"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
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
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::new(),
                            method_kind: None,
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Base"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                        Declaration {
                            name: String::from("Child"),
                            kind: DeclarationKind::Class,
                            detail: String::new(),
                            method_kind: None,
                            class_kind: Some(DeclarationOwnerKind::Class),
                            owner: None,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: vec![String::from("Base")],
                        },
                        Declaration {
                            name: String::from("run"),
                            kind: DeclarationKind::Function,
                            detail: String::new(),
                            method_kind: None,
                            class_kind: None,
                            owner: Some(DeclarationOwner {
                                name: String::from("Child"),
                                kind: DeclarationOwnerKind::Class,
                            }),
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_final: false,
                            is_class_var: false,
                            bases: Vec::new(),
                        },
                    ],
                    calls: Vec::new(),
                    summary_fingerprint: 1,
                }],
            },
            true,
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
                                method_kind: None,
                                class_kind: Some(DeclarationOwnerKind::Class),
                                owner: None,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_final: false,
                                is_class_var: false,
                                bases: Vec::new(),
                            },
                            Declaration {
                                name: String::from("run"),
                                kind: DeclarationKind::Function,
                                detail: String::from("(self)->None"),
                                method_kind: Some(typepython_syntax::MethodKind::Instance),
                                class_kind: None,
                                owner: Some(DeclarationOwner {
                                    name: String::from("Base"),
                                    kind: DeclarationOwnerKind::Class,
                                }),
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_final: false,
                                is_class_var: false,
                                bases: Vec::new(),
                            },
                        ],
                        calls: Vec::new(),
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
                                method_kind: None,
                                class_kind: None,
                                owner: None,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_final: false,
                                is_class_var: false,
                                bases: Vec::new(),
                            },
                            Declaration {
                                name: String::from("Child"),
                                kind: DeclarationKind::Class,
                                detail: String::from("Base"),
                                method_kind: None,
                                class_kind: Some(DeclarationOwnerKind::Class),
                                owner: None,
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_final: false,
                                is_class_var: false,
                                bases: vec![String::from("Base")],
                            },
                            Declaration {
                                name: String::from("run"),
                                kind: DeclarationKind::Function,
                                detail: String::from("(self)->None"),
                                method_kind: Some(typepython_syntax::MethodKind::Instance),
                                class_kind: None,
                                owner: Some(DeclarationOwner {
                                    name: String::from("Child"),
                                    kind: DeclarationOwnerKind::Class,
                                }),
                                is_override: false,
                                is_abstract_method: false,
                                is_final_decorator: false,
                                is_final: false,
                                is_class_var: false,
                                bases: Vec::new(),
                            },
                        ],
                        calls: Vec::new(),
                        summary_fingerprint: 2,
                    },
                ],
            },
            true,
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
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: true,
                    bases: Vec::new(),
                }],
                summary_fingerprint: 1,
                calls: Vec::new(),
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
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("cache"),
                        kind: DeclarationKind::Value,
                        detail: String::new(),
                        method_kind: None,
                        class_kind: None,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: true,
                        bases: Vec::new(),
                    },
                ],
                summary_fingerprint: 1,
                calls: Vec::new(),
            }],
        });

        assert!(result.diagnostics.is_empty());
    }
}

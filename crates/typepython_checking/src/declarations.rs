use super::*;

pub(super) fn override_compatibility_diagnostics<'a>(
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

pub(super) fn methods_are_compatible_for_override(
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
                || semantic_type_is_assignable(
                    node,
                    nodes,
                    &lower_type_text_or_name(&child.annotation),
                    &lower_type_text_or_name(&base.annotation),
                ))
    });
    if !params_compatible {
        return false;
    }

    let child_return = member.detail.split_once("->").map(|(_, right)| right.trim()).unwrap_or("");
    let base_return =
        base_member.detail.split_once("->").map(|(_, right)| right.trim()).unwrap_or("");
    child_return.is_empty()
        || base_return.is_empty()
        || semantic_type_is_assignable(
            node,
            nodes,
            &lower_type_text_or_name(base_return),
            &lower_type_text_or_name(child_return),
        )
}

pub(super) fn missing_override_diagnostics<'a>(
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

pub(super) fn final_decorator_diagnostics<'a>(
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

pub(super) fn abstract_member_diagnostics<'a>(
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

pub(super) fn abstract_instantiation_diagnostics<'a>(
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

pub(super) fn abstract_member_index(
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

pub(super) fn resolve_direct_base<'a>(
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

pub(super) fn sealed_match_exhaustiveness_diagnostics(
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

pub(super) fn enum_match_exhaustiveness_diagnostics(
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

pub(super) fn attach_match_case_suggestion(
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

pub(super) fn match_case_insertion_edit(
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

pub(super) fn leading_space_count(line: &str) -> usize {
    line.chars().take_while(|character| *character == ' ').count()
}

pub(super) fn resolve_match_subject_type(
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

pub(super) fn resolve_sealed_root<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    type_name: &str,
) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
    let mut visited = BTreeSet::new();
    resolve_sealed_root_with_visited(nodes, node, type_name, &mut visited)
}

pub(super) fn resolve_sealed_root_with_visited<'a>(
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

pub(super) fn collect_sealed_descendants(
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

pub(super) fn sealed_descends_from(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    root_name: &str,
) -> bool {
    let mut visited = BTreeSet::new();
    sealed_descends_from_with_visited(nodes, node, declaration, root_name, &mut visited)
}

pub(super) fn sealed_descends_from_with_visited(
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

pub(super) fn pattern_class_name(pattern: &typepython_binding::MatchPatternSite) -> Option<&str> {
    match pattern {
        typepython_binding::MatchPatternSite::Class(name) => Some(name.as_str()),
        _ => None,
    }
}

pub(super) fn enum_member_name_from_pattern(
    pattern: &typepython_binding::MatchPatternSite,
    enum_name: &str,
) -> Option<String> {
    let typepython_binding::MatchPatternSite::Literal(value) = pattern else {
        return None;
    };
    let (owner, member) = value.rsplit_once('.')?;
    (owner == enum_name).then(|| member.to_owned())
}

pub(super) fn is_interface_like_declaration(
    node: &typepython_graph::ModuleNode,
    declaration: &Declaration,
    nodes: &[typepython_graph::ModuleNode],
) -> bool {
    let mut visited = BTreeSet::new();
    is_interface_like_declaration_with_visited(node, declaration, nodes, &mut visited)
}

pub(super) fn is_interface_like_declaration_with_visited(
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

pub(super) fn override_diagnostics<'a>(
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

pub(super) fn final_override_diagnostics<'a>(
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

pub(super) fn interface_implementation_diagnostics<'a>(
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

pub(super) fn duplicate_diagnostics(
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

pub(super) fn classvar_placement_diagnostic(
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

pub(super) fn final_reassignment_diagnostic(
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

pub(super) fn declaration_spaces(
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

pub(super) fn duplicate_message(
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

pub(super) fn is_permitted_external_overload_group(
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

pub(super) fn invalid_duplicates(declarations: &[Declaration]) -> BTreeSet<&str> {
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

pub(super) fn is_invalid_duplicate_group(declarations: &[&Declaration]) -> bool {
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

pub(super) fn property_setter_compatibility_diagnostics(
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
            let getter_type = lower_type_text_or_name(getter.detail.split_once("->")?.1.trim());
            let setter_params = direct_param_types(&setter.detail)?;
            let setter_type = lower_type_text_or_name(setter_params.get(1)?);
            (getter_type != setter_type).then(|| {
                Diagnostic::error(
                    "TPY4001",
                    match owner_name {
                        Some(owner_name) => format!(
                            "type `{}` in module `{}` declares property `{}` with getter type `{}` but setter expects `{}`",
                            owner_name,
                            module_path.display(),
                            member_name,
                            render_semantic_type(&getter_type),
                            render_semantic_type(&setter_type),
                        ),
                        None => format!(
                            "module `{}` declares property `{}` with getter type `{}` but setter expects `{}`",
                            module_path.display(),
                            member_name,
                            render_semantic_type(&getter_type),
                            render_semantic_type(&setter_type),
                        ),
                    },
                )
            })
        })
        .collect()
}

pub(super) fn overload_shape_diagnostic(
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

pub(super) fn overload_shape_message(
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

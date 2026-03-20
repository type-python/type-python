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
    let mut diagnostics = DiagnosticReport::default();

    for node in &graph.nodes {
        for duplicate in duplicate_diagnostics(&node.module_path, node.module_kind, &node.declarations) {
            diagnostics.push(duplicate);
        }
        for override_violation in final_override_diagnostics(&node.module_path, &node.declarations) {
            diagnostics.push(override_violation);
        }
    }

    CheckResult { diagnostics }
}

fn final_override_diagnostics(
    module_path: &std::path::Path,
    declarations: &[Declaration],
) -> Vec<Diagnostic> {
    let class_final_members: BTreeMap<_, _> = declarations
        .iter()
        .filter(|declaration| declaration.owner.is_some() && declaration.is_final)
        .filter_map(|declaration| {
            declaration.owner.as_ref().map(|owner| ((owner.name.clone(), declaration.name.clone()), ()))
        })
        .collect();
    let mut diagnostics = Vec::new();

    for class_declaration in declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Class && declaration.owner.is_none())
    {
        for base in &class_declaration.bases {
            for member in declarations {
                let Some(owner) = member.owner.as_ref() else {
                    continue;
                };
                if owner.name != class_declaration.name {
                    continue;
                }
                if class_final_members.contains_key(&(base.clone(), member.name.clone())) {
                    diagnostics.push(Diagnostic::error(
                        "TPY4006",
                        format!(
                            "type `{}` in module `{}` overrides Final member `{}` from base `{}`",
                            class_declaration.name,
                            module_path.display(),
                            member.name,
                            base
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
    use super::check;
    use std::path::PathBuf;
    use typepython_binding::{Declaration, DeclarationKind, DeclarationOwner, DeclarationOwnerKind};
    use typepython_graph::{ModuleGraph, ModuleNode};
    use typepython_syntax::SourceKind;

    #[test]
    fn check_reports_duplicate_module_symbols() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
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
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                    name: String::from("UserId"),
                    kind: DeclarationKind::TypeAlias,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_accepts_overload_sets_with_one_implementation() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_overloads_without_concrete_implementation() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
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
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
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
                module_kind: SourceKind::Stub,
                declarations: vec![
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_duplicate_interface_members() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                    name: String::from("SupportsClose"),
                    kind: DeclarationKind::Class,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        owner: Some(DeclarationOwner {
                            name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                        name: String::from("close"),
                        kind: DeclarationKind::Function,
                        owner: Some(DeclarationOwner {
                            name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
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
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                    name: String::from("Parser"),
                    kind: DeclarationKind::Class,
                    owner: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                        owner: Some(DeclarationOwner {
                            name: String::from("Parser"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Function,
                        owner: Some(DeclarationOwner {
                            name: String::from("Parser"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                ],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_reports_final_reassignment_in_module_scope() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("MAX_SIZE"),
                        kind: DeclarationKind::Value,
                        owner: None,
                        is_final: true,
                        is_class_var: false,
                    bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("MAX_SIZE"),
                        kind: DeclarationKind::Value,
                        owner: None,
                        is_final: false,
                        is_class_var: false,
                    bases: Vec::new(),
                    },
                ],
                summary_fingerprint: 1,
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
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        owner: None,
                        is_final: false,
                        is_class_var: false,
                    bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_final: true,
                        is_class_var: false,
                    bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_final: false,
                        is_class_var: false,
                    bases: Vec::new(),
                    },
                ],
                summary_fingerprint: 1,
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
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Base"),
                        kind: DeclarationKind::Class,
                        owner: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        owner: Some(DeclarationOwner {
                            name: String::from("Base"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_final: true,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Derived"),
                        kind: DeclarationKind::Class,
                        owner: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Base")],
                    },
                    Declaration {
                        name: String::from("limit"),
                        kind: DeclarationKind::Value,
                        owner: Some(DeclarationOwner {
                            name: String::from("Derived"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                summary_fingerprint: 1,
            }],
        });

        let rendered = result.diagnostics.as_text();
        assert!(rendered.contains("TPY4006"));
        assert!(rendered.contains("overrides Final member `limit` from base `Base`"));
    }

    #[test]
    fn check_reports_classvar_outside_class_scope() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/module.py"),
                module_kind: SourceKind::Python,
                declarations: vec![Declaration {
                    name: String::from("VALUE"),
                    kind: DeclarationKind::Value,
                    owner: None,
                    is_final: false,
                    is_class_var: true,
                    bases: Vec::new(),
                }],
                summary_fingerprint: 1,
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
                module_kind: SourceKind::Python,
                declarations: vec![
                    Declaration {
                        name: String::from("Box"),
                        kind: DeclarationKind::Class,
                        owner: None,
                        is_final: false,
                        is_class_var: false,
                    bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("cache"),
                        kind: DeclarationKind::Value,
                        owner: Some(DeclarationOwner {
                            name: String::from("Box"),
                            kind: DeclarationOwnerKind::Class,
                        }),
                        is_final: false,
                        is_class_var: true,
                    bases: Vec::new(),
                    },
                ],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }
}

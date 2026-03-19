//! Type-checking boundary for TypePython.

use std::collections::{BTreeMap, BTreeSet};

use typepython_binding::{Declaration, DeclarationKind};
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
    }

    CheckResult { diagnostics }
}

fn duplicate_diagnostics(
    module_path: &std::path::Path,
    module_kind: SourceKind,
    declarations: &[Declaration],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for duplicate in invalid_duplicates(declarations) {
        if let Some(diagnostic) = overload_shape_diagnostic(module_path, module_kind, duplicate, declarations) {
            diagnostics.push(diagnostic);
        } else if is_permitted_external_overload_group(module_kind, duplicate, declarations) {
            continue;
        } else {
            diagnostics.push(Diagnostic::error(
                "TPY4004",
                format!(
                    "module `{}` declares `{duplicate}` more than once in the same declaration space",
                    module_path.display()
                ),
            ));
        }
    }

    diagnostics
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
    duplicate: &str,
    declarations: &[Declaration],
) -> Option<Diagnostic> {
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
        0 => format!(
            "module `{}` declares overloads for `{duplicate}` without a concrete implementation",
            module_path.display()
        ),
        1 => return None,
        _ => format!(
            "module `{}` declares overloads for `{duplicate}` with more than one concrete implementation",
            module_path.display()
        ),
    };

    Some(Diagnostic::error("TPY4004", message))
}

#[cfg(test)]
mod tests {
    use super::check;
    use std::path::PathBuf;
    use typepython_binding::{Declaration, DeclarationKind};
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
                    },
                    Declaration {
                        name: String::from("User"),
                        kind: DeclarationKind::Class,
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
                    },
                    Declaration {
                        name: String::from("User"),
                        kind: DeclarationKind::Class,
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
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Function,
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
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
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
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Function,
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Function,
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
                    },
                    Declaration {
                        name: String::from("parse"),
                        kind: DeclarationKind::Overload,
                    },
                ],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }
}

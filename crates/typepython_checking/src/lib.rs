//! Type-checking boundary for TypePython.

use std::collections::{BTreeMap, BTreeSet};

use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_graph::ModuleGraph;
use typepython_binding::{Declaration, DeclarationKind};

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
        for duplicate in invalid_duplicates(&node.declarations) {
            diagnostics.push(Diagnostic::error(
                "TPY4004",
                format!(
                    "module `{}` declares `{duplicate}` more than once in the same declaration space",
                    node.module_path.display()
                ),
            ));
        }
    }

    CheckResult { diagnostics }
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

#[cfg(test)]
mod tests {
    use super::check;
    use std::path::PathBuf;
    use typepython_binding::{Declaration, DeclarationKind};
    use typepython_graph::{ModuleGraph, ModuleNode};

    #[test]
    fn check_reports_duplicate_module_symbols() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
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
}

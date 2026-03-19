//! Type-checking boundary for TypePython.

use std::collections::BTreeSet;

use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_graph::ModuleGraph;

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
        let mut seen = BTreeSet::new();
        let mut duplicates = BTreeSet::new();

        for symbol in &node.symbols {
            if !seen.insert(symbol) {
                duplicates.insert(symbol);
            }
        }

        for duplicate in duplicates {
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

#[cfg(test)]
mod tests {
    use super::check;
    use std::path::PathBuf;
    use typepython_graph::{ModuleGraph, ModuleNode};

    #[test]
    fn check_reports_duplicate_module_symbols() {
        let result = check(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/app/__init__.tpy"),
                symbols: vec![String::from("User"), String::from("User")],
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
                symbols: vec![String::from("UserId"), String::from("User")],
                summary_fingerprint: 1,
            }],
        });

        assert!(result.diagnostics.is_empty());
    }
}

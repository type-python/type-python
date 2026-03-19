//! Type-checking boundary for TypePython.

use typepython_diagnostics::DiagnosticReport;
use typepython_graph::ModuleGraph;

/// Result of running the checker.
#[derive(Debug, Clone, Default)]
pub struct CheckResult {
    /// Diagnostics produced by the checker.
    pub diagnostics: DiagnosticReport,
}

/// Runs the placeholder checker over the module graph.
#[must_use]
pub fn check(_graph: &ModuleGraph) -> CheckResult {
    CheckResult { diagnostics: DiagnosticReport::default() }
}

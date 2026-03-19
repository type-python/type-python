//! Incremental build state boundary for TypePython.

use std::collections::BTreeMap;

use typepython_graph::ModuleGraph;

/// Fingerprint of one summary-bearing module.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Fingerprint {
    /// Stable module key.
    pub module_key: String,
    /// Placeholder fingerprint value.
    pub fingerprint: u64,
}

/// Current incremental snapshot.
#[derive(Debug, Clone, Default)]
pub struct IncrementalState {
    /// Tracked fingerprints by module key.
    pub fingerprints: BTreeMap<String, u64>,
}

/// Captures an incremental snapshot from the module graph.
#[must_use]
pub fn snapshot(graph: &ModuleGraph) -> IncrementalState {
    let fingerprints = graph
        .nodes
        .iter()
        .map(|node| (node.module_path.to_string_lossy().into_owned(), node.summary_fingerprint))
        .collect();

    IncrementalState { fingerprints }
}

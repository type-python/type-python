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

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct SnapshotDiff {
    pub added: Vec<Fingerprint>,
    pub removed: Vec<Fingerprint>,
    pub changed: Vec<Fingerprint>,
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

#[must_use]
pub fn diff(previous: &IncrementalState, current: &IncrementalState) -> SnapshotDiff {
    let mut snapshot_diff = SnapshotDiff::default();

    for (module_key, fingerprint) in &current.fingerprints {
        match previous.fingerprints.get(module_key) {
            None => snapshot_diff.added.push(Fingerprint {
                module_key: module_key.clone(),
                fingerprint: *fingerprint,
            }),
            Some(previous_fingerprint) if previous_fingerprint != fingerprint => {
                snapshot_diff.changed.push(Fingerprint {
                    module_key: module_key.clone(),
                    fingerprint: *fingerprint,
                });
            }
            Some(_) => {}
        }
    }

    for (module_key, fingerprint) in &previous.fingerprints {
        if !current.fingerprints.contains_key(module_key) {
            snapshot_diff.removed.push(Fingerprint {
                module_key: module_key.clone(),
                fingerprint: *fingerprint,
            });
        }
    }

    snapshot_diff
}

#[cfg(test)]
mod tests {
    use super::{Fingerprint, IncrementalState, SnapshotDiff, diff};
    use std::collections::BTreeMap;

    #[test]
    fn diff_reports_added_removed_and_changed_modules() {
        let previous = IncrementalState {
            fingerprints: BTreeMap::from([
                (String::from("pkg.a"), 10),
                (String::from("pkg.b"), 20),
            ]),
        };
        let current = IncrementalState {
            fingerprints: BTreeMap::from([
                (String::from("pkg.b"), 30),
                (String::from("pkg.c"), 40),
            ]),
        };

        let snapshot_diff = diff(&previous, &current);
        println!("{:?}", snapshot_diff);
        assert_eq!(
            snapshot_diff,
            SnapshotDiff {
                added: vec![Fingerprint {
                    module_key: String::from("pkg.c"),
                    fingerprint: 40,
                }],
                removed: vec![Fingerprint {
                    module_key: String::from("pkg.a"),
                    fingerprint: 10,
                }],
                changed: vec![Fingerprint {
                    module_key: String::from("pkg.b"),
                    fingerprint: 30,
                }],
            }
        );
    }

    #[test]
    fn diff_reports_no_changes_for_identical_snapshots() {
        let state = IncrementalState {
            fingerprints: BTreeMap::from([(String::from("pkg.a"), 10)]),
        };

        let snapshot_diff = diff(&state, &state);
        assert!(snapshot_diff.added.is_empty());
        assert!(snapshot_diff.removed.is_empty());
        assert!(snapshot_diff.changed.is_empty());
    }
}

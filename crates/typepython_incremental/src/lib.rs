//! Incremental build state boundary for TypePython.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use typepython_graph::ModuleGraph;

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Fingerprint of one summary-bearing module.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Fingerprint {
    /// Stable module key.
    pub module_key: String,
    /// Placeholder fingerprint value.
    pub fingerprint: u64,
}

/// Current incremental snapshot.
#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IncrementalState {
    /// Tracked fingerprints by module key.
    pub fingerprints: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
struct SnapshotFile {
    schema_version: u32,
    fingerprints: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SnapshotDecodeError {
    InvalidJson(String),
    IncompatibleSchemaVersion(u32),
}

impl std::fmt::Display for SnapshotDecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson(error) => {
                write!(formatter, "invalid incremental snapshot JSON: {error}")
            }
            Self::IncompatibleSchemaVersion(version) => write!(
                formatter,
                "incremental snapshot schema version {version} is incompatible with expected version {SNAPSHOT_SCHEMA_VERSION}"
            ),
        }
    }
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
            None => snapshot_diff
                .added
                .push(Fingerprint { module_key: module_key.clone(), fingerprint: *fingerprint }),
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
            snapshot_diff
                .removed
                .push(Fingerprint { module_key: module_key.clone(), fingerprint: *fingerprint });
        }
    }

    snapshot_diff
}

pub fn encode_snapshot(state: &IncrementalState) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&SnapshotFile {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        fingerprints: state.fingerprints.clone(),
    })
}

pub fn decode_snapshot(contents: &str) -> Result<IncrementalState, SnapshotDecodeError> {
    let snapshot: SnapshotFile = serde_json::from_str(contents)
        .map_err(|error| SnapshotDecodeError::InvalidJson(error.to_string()))?;
    if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(SnapshotDecodeError::IncompatibleSchemaVersion(snapshot.schema_version));
    }
    Ok(IncrementalState { fingerprints: snapshot.fingerprints })
}

#[cfg(test)]
mod tests {
    use super::{
        Fingerprint, IncrementalState, SNAPSHOT_SCHEMA_VERSION, SnapshotDecodeError, SnapshotDiff,
        decode_snapshot, diff, encode_snapshot,
    };
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
                added: vec![Fingerprint { module_key: String::from("pkg.c"), fingerprint: 40 }],
                removed: vec![Fingerprint { module_key: String::from("pkg.a"), fingerprint: 10 }],
                changed: vec![Fingerprint { module_key: String::from("pkg.b"), fingerprint: 30 }],
            }
        );
    }

    #[test]
    fn diff_reports_no_changes_for_identical_snapshots() {
        let state =
            IncrementalState { fingerprints: BTreeMap::from([(String::from("pkg.a"), 10)]) };

        let snapshot_diff = diff(&state, &state);
        assert!(snapshot_diff.added.is_empty());
        assert!(snapshot_diff.removed.is_empty());
        assert!(snapshot_diff.changed.is_empty());
    }

    #[test]
    fn encode_snapshot_includes_schema_version() {
        let rendered = encode_snapshot(&IncrementalState {
            fingerprints: BTreeMap::from([(String::from("pkg.a"), 10)]),
        })
        .expect("snapshot encoding should succeed");

        assert!(rendered.contains("schema_version"));
        assert!(rendered.contains(&SNAPSHOT_SCHEMA_VERSION.to_string()));
        assert!(rendered.contains("pkg.a"));
    }

    #[test]
    fn decode_snapshot_rejects_incompatible_schema_version() {
        let error = decode_snapshot("{\"schema_version\":999,\"fingerprints\":{}}")
            .expect_err("unexpected schema version should be rejected");

        assert_eq!(error, SnapshotDecodeError::IncompatibleSchemaVersion(999));
    }
}

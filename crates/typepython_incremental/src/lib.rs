//! Incremental build state boundary for TypePython.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use typepython_binding::{Declaration, DeclarationKind, DeclarationOwnerKind};
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
    #[serde(default)]
    pub summaries: Vec<PublicSummary>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
struct SnapshotFile {
    schema_version: u32,
    fingerprints: BTreeMap<String, u64>,
    #[serde(default)]
    summaries: Vec<PublicSummary>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PublicSummary {
    pub module: String,
    #[serde(rename = "isPackageEntry")]
    pub is_package_entry: bool,
    pub exports: Vec<SummaryExport>,
    pub imports: Vec<String>,
    #[serde(rename = "sealedRoots")]
    pub sealed_roots: Vec<SealedRootSummary>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SummaryExport {
    pub name: String,
    pub kind: String,
    #[serde(rename = "type")]
    pub type_repr: String,
    #[serde(rename = "typeParams")]
    pub type_params: Vec<String>,
    pub public: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SealedRootSummary {
    pub root: String,
    pub members: Vec<String>,
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
        .map(|node| (node.module_key.clone(), node.summary_fingerprint))
        .collect();

    let mut summaries = graph.nodes.iter().map(public_summary).collect::<Vec<_>>();
    summaries.sort_by(|left, right| left.module.cmp(&right.module));

    IncrementalState { fingerprints, summaries }
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
        summaries: state.summaries.clone(),
    })
}

pub fn decode_snapshot(contents: &str) -> Result<IncrementalState, SnapshotDecodeError> {
    let snapshot: SnapshotFile = serde_json::from_str(contents)
        .map_err(|error| SnapshotDecodeError::InvalidJson(error.to_string()))?;
    if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(SnapshotDecodeError::IncompatibleSchemaVersion(snapshot.schema_version));
    }
    Ok(IncrementalState { fingerprints: snapshot.fingerprints, summaries: snapshot.summaries })
}

fn public_summary(node: &typepython_graph::ModuleNode) -> PublicSummary {
    let top_level_declarations = node
        .declarations
        .iter()
        .filter(|declaration| declaration.owner.is_none())
        .collect::<Vec<_>>();

    let mut exports = top_level_declarations
        .iter()
        .map(|declaration| SummaryExport {
            name: declaration.name.clone(),
            kind: summary_kind(declaration),
            type_repr: summary_type_repr(declaration),
            type_params: Vec::new(),
            public: !declaration.name.starts_with('_'),
        })
        .collect::<Vec<_>>();
    exports.sort_by(|left, right| left.name.cmp(&right.name));

    let mut imports = top_level_declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Import)
        .map(|declaration| declaration.detail.clone())
        .collect::<Vec<_>>();
    imports.sort();
    imports.dedup();

    let mut sealed_roots = top_level_declarations
        .iter()
        .filter(|declaration| declaration.class_kind == Some(DeclarationOwnerKind::SealedClass))
        .map(|declaration| {
            let mut members = top_level_declarations
                .iter()
                .filter(|candidate| {
                    candidate.name != declaration.name
                        && candidate.bases.iter().any(|base| base == &declaration.name)
                })
                .map(|candidate| candidate.name.clone())
                .collect::<Vec<_>>();
            members.sort();
            SealedRootSummary { root: declaration.name.clone(), members }
        })
        .collect::<Vec<_>>();
    sealed_roots.sort_by(|left, right| left.root.cmp(&right.root));

    PublicSummary {
        module: node.module_key.clone(),
        is_package_entry: is_package_entry_path(&node.module_path),
        exports,
        imports,
        sealed_roots,
    }
}

fn summary_kind(declaration: &Declaration) -> String {
    match declaration.kind {
        DeclarationKind::TypeAlias => String::from("typealias"),
        DeclarationKind::Class => String::from("class"),
        DeclarationKind::Function => String::from("function"),
        DeclarationKind::Overload => String::from("overload"),
        DeclarationKind::Value => String::from("value"),
        DeclarationKind::Import => String::from("import"),
    }
}

fn summary_type_repr(declaration: &Declaration) -> String {
    match declaration.kind {
        DeclarationKind::Class => declaration.name.clone(),
        DeclarationKind::Value => declaration
            .value_type
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| declaration.detail.clone()),
        _ => {
            if declaration.detail.is_empty() {
                declaration.name.clone()
            } else {
                declaration.detail.clone()
            }
        }
    }
}

fn is_package_entry_path(path: &std::path::Path) -> bool {
    path.file_name().is_some_and(|name| {
        name == "__init__.py" || name == "__init__.pyi" || name == "__init__.tpy"
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Fingerprint, IncrementalState, PublicSummary, SNAPSHOT_SCHEMA_VERSION, SealedRootSummary,
        SnapshotDecodeError, SnapshotDiff, SummaryExport, decode_snapshot, diff, encode_snapshot,
        snapshot,
    };
    use std::{collections::BTreeMap, path::PathBuf};
    use typepython_binding::{Declaration, DeclarationKind, DeclarationOwnerKind};
    use typepython_graph::{ModuleGraph, ModuleNode};
    use typepython_syntax::SourceKind;

    #[test]
    fn diff_reports_added_removed_and_changed_modules() {
        let previous = IncrementalState {
            fingerprints: BTreeMap::from([
                (String::from("pkg.a"), 10),
                (String::from("pkg.b"), 20),
            ]),
            summaries: Vec::new(),
        };
        let current = IncrementalState {
            fingerprints: BTreeMap::from([
                (String::from("pkg.b"), 30),
                (String::from("pkg.c"), 40),
            ]),
            summaries: Vec::new(),
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
        let state = IncrementalState {
            fingerprints: BTreeMap::from([(String::from("pkg.a"), 10)]),
            summaries: Vec::new(),
        };

        let snapshot_diff = diff(&state, &state);
        assert!(snapshot_diff.added.is_empty());
        assert!(snapshot_diff.removed.is_empty());
        assert!(snapshot_diff.changed.is_empty());
    }

    #[test]
    fn encode_snapshot_includes_schema_version() {
        let rendered = encode_snapshot(&IncrementalState {
            fingerprints: BTreeMap::from([(String::from("pkg.a"), 10)]),
            summaries: vec![PublicSummary {
                module: String::from("pkg.a"),
                is_package_entry: false,
                exports: vec![SummaryExport {
                    name: String::from("Foo"),
                    kind: String::from("class"),
                    type_repr: String::from("Foo"),
                    type_params: Vec::new(),
                    public: true,
                }],
                imports: vec![String::from("pkg.base")],
                sealed_roots: vec![SealedRootSummary {
                    root: String::from("Expr"),
                    members: vec![String::from("Add"), String::from("Num")],
                }],
            }],
        })
        .expect("snapshot encoding should succeed");

        assert!(rendered.contains("schema_version"));
        assert!(rendered.contains(&SNAPSHOT_SCHEMA_VERSION.to_string()));
        assert!(rendered.contains("pkg.a"));
        assert!(rendered.contains("\"exports\""));
        assert!(rendered.contains("\"imports\""));
        assert!(rendered.contains("\"sealedRoots\""));
    }

    #[test]
    fn snapshot_uses_logical_module_keys() {
        let state = snapshot(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/pkg/a.tpy"),
                module_key: String::from("pkg.a"),
                module_kind: SourceKind::TypePython,
                declarations: vec![
                    Declaration {
                        name: String::from("Expr"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::SealedClass),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("Add"),
                        kind: DeclarationKind::Class,
                        detail: String::new(),
                        value_type: None,
                        method_kind: None,
                        class_kind: Some(DeclarationOwnerKind::Class),
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: vec![String::from("Expr")],
                    },
                    Declaration {
                        name: String::from("helper"),
                        kind: DeclarationKind::Function,
                        detail: String::from("()->int"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                    Declaration {
                        name: String::from("base"),
                        kind: DeclarationKind::Import,
                        detail: String::from("pkg.base"),
                        value_type: None,
                        method_kind: None,
                        class_kind: None,
                        owner: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        bases: Vec::new(),
                    },
                ],
                calls: Vec::new(),
                method_calls: Vec::new(),
                member_accesses: Vec::new(),
                returns: Vec::new(),
                yields: Vec::new(),
                if_guards: Vec::new(),
                asserts: Vec::new(),
                invalidations: Vec::new(),
                matches: Vec::new(),
                for_loops: Vec::new(),
                with_statements: Vec::new(),
                except_handlers: Vec::new(),
                assignments: Vec::new(),
                summary_fingerprint: 42,
            }],
        });

        assert_eq!(state.fingerprints, BTreeMap::from([(String::from("pkg.a"), 42)]));
        assert_eq!(
            state.summaries,
            vec![PublicSummary {
                module: String::from("pkg.a"),
                is_package_entry: false,
                exports: vec![
                    SummaryExport {
                        name: String::from("Add"),
                        kind: String::from("class"),
                        type_repr: String::from("Add"),
                        type_params: Vec::new(),
                        public: true,
                    },
                    SummaryExport {
                        name: String::from("Expr"),
                        kind: String::from("class"),
                        type_repr: String::from("Expr"),
                        type_params: Vec::new(),
                        public: true,
                    },
                    SummaryExport {
                        name: String::from("base"),
                        kind: String::from("import"),
                        type_repr: String::from("pkg.base"),
                        type_params: Vec::new(),
                        public: true,
                    },
                    SummaryExport {
                        name: String::from("helper"),
                        kind: String::from("function"),
                        type_repr: String::from("()->int"),
                        type_params: Vec::new(),
                        public: true,
                    },
                ],
                imports: vec![String::from("pkg.base")],
                sealed_roots: vec![SealedRootSummary {
                    root: String::from("Expr"),
                    members: vec![String::from("Add")],
                }],
            }]
        );
    }

    #[test]
    fn decode_snapshot_rejects_incompatible_schema_version() {
        let error = decode_snapshot("{\"schema_version\":999,\"fingerprints\":{}}")
            .expect_err("unexpected schema version should be rejected");

        assert_eq!(error, SnapshotDecodeError::IncompatibleSchemaVersion(999));
    }
}

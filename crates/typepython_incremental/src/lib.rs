//! Incremental build state boundary for TypePython.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use typepython_binding::{Declaration, DeclarationKind, DeclarationOwnerKind, GenericTypeParam};
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
    #[serde(default)]
    pub stdlib_snapshot: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
struct SnapshotFile {
    schema_version: u32,
    fingerprints: BTreeMap<String, u64>,
    #[serde(default)]
    summaries: Vec<PublicSummary>,
    #[serde(default)]
    stdlib_snapshot: Option<String>,
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
    pub type_params: Vec<SummaryTypeParam>,
    pub public: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SummaryTypeParam {
    pub name: String,
    pub bound: Option<String>,
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
    let mut summaries = graph.nodes.iter().map(public_summary).collect::<Vec<_>>();
    summaries.sort_by(|left, right| left.module.cmp(&right.module));

    let fingerprints = summaries
        .iter()
        .map(|summary| (summary.module.clone(), summary_fingerprint(summary)))
        .collect();

    IncrementalState { fingerprints, summaries, stdlib_snapshot: None }
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
        stdlib_snapshot: state.stdlib_snapshot.clone(),
    })
}

pub fn decode_snapshot(contents: &str) -> Result<IncrementalState, SnapshotDecodeError> {
    let snapshot: SnapshotFile = serde_json::from_str(contents)
        .map_err(|error| SnapshotDecodeError::InvalidJson(error.to_string()))?;
    if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(SnapshotDecodeError::IncompatibleSchemaVersion(snapshot.schema_version));
    }
    Ok(IncrementalState {
        fingerprints: snapshot.fingerprints,
        summaries: snapshot.summaries,
        stdlib_snapshot: snapshot.stdlib_snapshot,
    })
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
            type_params: declaration.type_params.iter().map(summary_type_param).collect(),
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

fn summary_type_param(type_param: &GenericTypeParam) -> SummaryTypeParam {
    SummaryTypeParam { name: type_param.name.clone(), bound: type_param.bound.clone() }
}

fn summary_fingerprint(summary: &PublicSummary) -> u64 {
    let Ok(serialized) = serde_json::to_vec(summary) else {
        return 0;
    };
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in serialized {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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
        SnapshotDecodeError, SnapshotDiff, SummaryExport, SummaryTypeParam, decode_snapshot, diff,
        encode_snapshot, snapshot,
    };
    use std::{collections::BTreeMap, path::PathBuf};
    use typepython_binding::{
        Declaration, DeclarationKind, DeclarationOwnerKind, GenericTypeParam, GenericTypeParamKind,
    };
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
            stdlib_snapshot: None,
        };
        let current = IncrementalState {
            fingerprints: BTreeMap::from([
                (String::from("pkg.b"), 30),
                (String::from("pkg.c"), 40),
            ]),
            summaries: Vec::new(),
            stdlib_snapshot: None,
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
            stdlib_snapshot: None,
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
                    type_params: vec![SummaryTypeParam {
                        name: String::from("T"),
                        bound: Some(String::from("SupportsClose")),
                    }],
                    public: true,
                }],
                imports: vec![String::from("pkg.base")],
                sealed_roots: vec![SealedRootSummary {
                    root: String::from("Expr"),
                    members: vec![String::from("Add"), String::from("Num")],
                }],
            }],
            stdlib_snapshot: Some(String::from("fnv1a64:demo")),
        })
        .expect("snapshot encoding should succeed");

        assert!(rendered.contains("schema_version"));
        assert!(rendered.contains(&SNAPSHOT_SCHEMA_VERSION.to_string()));
        assert!(rendered.contains("pkg.a"));
        assert!(rendered.contains("\"exports\""));
        assert!(rendered.contains("\"imports\""));
        assert!(rendered.contains("\"sealedRoots\""));
        assert!(rendered.contains("fnv1a64:demo"));
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
                        type_params: vec![GenericTypeParam {
                            kind: GenericTypeParamKind::TypeVar,
                            name: String::from("T"),
                            bound: Some(String::from("SupportsClose")),
                            constraints: Vec::new(),
                            default: None,
                        }],
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
                        type_params: Vec::new(),
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
                        type_params: vec![GenericTypeParam {
                            kind: GenericTypeParamKind::TypeVar,
                            name: String::from("T"),
                            bound: None,
                            constraints: Vec::new(),
                            default: None,
                        }],
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
                        type_params: Vec::new(),
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

        assert!(state.fingerprints.contains_key("pkg.a"));
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
                        type_params: vec![SummaryTypeParam {
                            name: String::from("T"),
                            bound: Some(String::from("SupportsClose")),
                        }],
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
                        type_params: vec![SummaryTypeParam {
                            name: String::from("T"),
                            bound: None,
                        }],
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
        assert_eq!(state.stdlib_snapshot, None);
    }

    #[test]
    fn snapshot_fingerprints_ignore_non_public_graph_noise() {
        let make_graph = |fingerprint| ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/pkg/a.tpy"),
                module_key: String::from("pkg.a"),
                module_kind: SourceKind::TypePython,
                declarations: vec![Declaration {
                    name: String::from("build"),
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
                    type_params: Vec::new(),
                }],
                calls: vec![typepython_binding::CallSite {
                    callee: String::from("helper"),
                    arg_count: 1,
                    arg_types: vec![String::from("int")],
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 3,
                }],
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
                summary_fingerprint: fingerprint,
            }],
        };

        let left = snapshot(&make_graph(1));
        let right = snapshot(&make_graph(99));

        assert_eq!(left.summaries, right.summaries);
        assert_eq!(left.fingerprints, right.fingerprints);
    }

    #[test]
    fn decode_snapshot_rejects_incompatible_schema_version() {
        let error = decode_snapshot("{\"schema_version\":999,\"fingerprints\":{}}")
            .expect_err("unexpected schema version should be rejected");

        assert_eq!(error, SnapshotDecodeError::IncompatibleSchemaVersion(999));
    }
}

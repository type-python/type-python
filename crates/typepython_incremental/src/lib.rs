//! Incremental build state boundary for TypePython.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use typepython_binding::{Declaration, DeclarationKind, DeclarationOwnerKind, GenericTypeParam};
use typepython_graph::ModuleGraph;

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 2;

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
    #[serde(rename = "solverFacts", default)]
    pub solver_facts: ModuleSolverFacts,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SummaryExport {
    pub name: String,
    pub kind: String,
    #[serde(rename = "type")]
    pub type_repr: String,
    #[serde(rename = "declarationSignature", default)]
    pub declaration_signature: Option<SummaryCallableSignature>,
    #[serde(rename = "exportedType", default)]
    pub exported_type: Option<String>,
    #[serde(rename = "typeParams")]
    pub type_params: Vec<SummaryTypeParam>,
    pub public: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct ModuleSolverFacts {
    #[serde(rename = "declarationFacts", default)]
    pub declaration_facts: Vec<SummaryDeclarationFact>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SummaryDeclarationFact {
    pub name: String,
    pub kind: String,
    #[serde(rename = "signature", default)]
    pub signature: Option<SummaryCallableSignature>,
    #[serde(rename = "typeExpr", default)]
    pub type_expr: Option<String>,
    #[serde(rename = "importTarget", default)]
    pub import_target: Option<String>,
    #[serde(default)]
    pub bases: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SummaryCallableSignature {
    pub params: Vec<SummarySignatureParam>,
    pub returns: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SummarySignatureParam {
    pub name: String,
    pub annotation: Option<String>,
    #[serde(rename = "hasDefault")]
    pub has_default: bool,
    #[serde(rename = "positionalOnly")]
    pub positional_only: bool,
    #[serde(rename = "keywordOnly")]
    pub keyword_only: bool,
    pub variadic: bool,
    #[serde(rename = "keywordVariadic")]
    pub keyword_variadic: bool,
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

/// Import dependency index for one module graph.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct ModuleDependencyIndex {
    pub imports_by_module: BTreeMap<String, BTreeSet<String>>,
    pub reverse_imports: BTreeMap<String, BTreeSet<String>>,
}

/// Captures an incremental snapshot from the module graph.
#[must_use]
pub fn snapshot(graph: &ModuleGraph) -> IncrementalState {
    let mut summaries = graph.nodes.iter().map(public_summary).collect::<Vec<_>>();
    snapshot_from_summaries(&mut summaries, None)
}

#[must_use]
pub fn snapshot_with_summaries(
    mut summaries: Vec<PublicSummary>,
    stdlib_snapshot: Option<String>,
) -> IncrementalState {
    snapshot_from_summaries(&mut summaries, stdlib_snapshot)
}

fn snapshot_from_summaries(
    summaries: &mut Vec<PublicSummary>,
    stdlib_snapshot: Option<String>,
) -> IncrementalState {
    summaries.sort_by(|left, right| left.module.cmp(&right.module));

    let fingerprints = summaries
        .iter()
        .map(|summary| (summary.module.clone(), summary_fingerprint(summary)))
        .collect();

    IncrementalState { fingerprints, summaries: summaries.clone(), stdlib_snapshot }
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

/// Builds a module dependency index from bound import declarations.
#[must_use]
pub fn dependency_index(graph: &ModuleGraph) -> ModuleDependencyIndex {
    let module_keys =
        graph.nodes.iter().map(|node| node.module_key.clone()).collect::<BTreeSet<_>>();
    let mut imports_by_module = graph
        .nodes
        .iter()
        .map(|node| (node.module_key.clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut reverse_imports = imports_by_module.clone();

    for node in &graph.nodes {
        let imports = node
            .declarations
            .iter()
            .filter(|declaration| declaration.owner.is_none())
            .filter(|declaration| declaration.kind == DeclarationKind::Import)
            .filter_map(|declaration| {
                let target = declaration
                    .import_target()
                    .map(|target| target.module_target.as_str())
                    .unwrap_or_else(|| declaration.detail.as_str());
                resolve_import_module_key(target, &module_keys)
            })
            .collect::<BTreeSet<_>>();
        for imported in &imports {
            reverse_imports.entry(imported.clone()).or_default().insert(node.module_key.clone());
        }
        imports_by_module.insert(node.module_key.clone(), imports);
    }

    ModuleDependencyIndex { imports_by_module, reverse_imports }
}

/// Collects the set of modules whose public summaries changed in a snapshot diff.
#[must_use]
pub fn snapshot_diff_modules(snapshot_diff: &SnapshotDiff) -> BTreeSet<String> {
    snapshot_diff
        .added
        .iter()
        .chain(snapshot_diff.removed.iter())
        .chain(snapshot_diff.changed.iter())
        .map(|fingerprint| fingerprint.module_key.clone())
        .collect()
}

/// Plans the module subset that must be rechecked after an incremental update.
#[must_use]
pub fn affected_modules(
    previous_index: Option<&ModuleDependencyIndex>,
    current_index: &ModuleDependencyIndex,
    direct_changes: &BTreeSet<String>,
    summary_changed_modules: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut affected = direct_changes.clone();
    for module_key in summary_changed_modules {
        if current_index.imports_by_module.contains_key(module_key) {
            affected.insert(module_key.clone());
        }
    }

    let mut queue = summary_changed_modules.iter().cloned().collect::<VecDeque<_>>();
    let mut visited = summary_changed_modules.clone();
    while let Some(module_key) = queue.pop_front() {
        for importer in reverse_importers(previous_index, current_index, &module_key) {
            if visited.insert(importer.clone()) {
                queue.push_back(importer.clone());
            }
            affected.insert(importer);
        }
    }

    affected
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
            declaration_signature: None,
            exported_type: None,
            type_params: declaration.type_params.iter().map(summary_type_param).collect(),
            public: !declaration.name.starts_with('_'),
        })
        .collect::<Vec<_>>();
    exports.sort_by(|left, right| left.name.cmp(&right.name));

    let mut imports = top_level_declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::Import)
        .map(|declaration| {
            declaration
                .import_target()
                .map(|target| target.raw_target.clone())
                .unwrap_or_else(|| declaration.detail.clone())
        })
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
        solver_facts: ModuleSolverFacts::default(),
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
            .or_else(|| declaration.value_annotation().map(|annotation| annotation.render()))
            .unwrap_or_else(|| declaration.detail.clone()),
        _ => {
            let structured = match declaration.kind {
                DeclarationKind::TypeAlias => {
                    declaration.type_alias_value().map(|value| value.render())
                }
                DeclarationKind::Function | DeclarationKind::Overload => {
                    declaration.callable_signature().map(|signature| signature.rendered())
                }
                DeclarationKind::Import => {
                    declaration.import_target().map(|target| target.raw_target.clone())
                }
                DeclarationKind::Class | DeclarationKind::Value => None,
            };
            let detail = structured.unwrap_or_else(|| declaration.detail.clone());
            if detail.is_empty() {
                declaration.name.clone()
            } else {
                detail
            }
        }
    }
}

fn summary_type_param(type_param: &GenericTypeParam) -> SummaryTypeParam {
    SummaryTypeParam { name: type_param.name.clone(), bound: type_param.rendered_bound() }
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

fn resolve_import_module_key(detail: &str, module_keys: &BTreeSet<String>) -> Option<String> {
    if module_keys.contains(detail) {
        return Some(detail.to_owned());
    }

    let mut current = detail;
    while let Some((parent, _)) = current.rsplit_once('.') {
        if module_keys.contains(parent) {
            return Some(parent.to_owned());
        }
        current = parent;
    }

    None
}

fn reverse_importers(
    previous_index: Option<&ModuleDependencyIndex>,
    current_index: &ModuleDependencyIndex,
    module_key: &str,
) -> BTreeSet<String> {
    let mut importers = current_index.reverse_imports.get(module_key).cloned().unwrap_or_default();
    if let Some(previous_index) = previous_index {
        importers
            .extend(previous_index.reverse_imports.get(module_key).cloned().unwrap_or_default());
    }
    importers
}

fn is_package_entry_path(path: &std::path::Path) -> bool {
    path.file_name().is_some_and(|name| {
        name == "__init__.py" || name == "__init__.pyi" || name == "__init__.tpy"
    })
}

#[cfg(test)]
mod tests {
    use super::{
        affected_modules, decode_snapshot, dependency_index, diff, encode_snapshot, snapshot,
        snapshot_diff_modules, Fingerprint, IncrementalState, ModuleSolverFacts, PublicSummary,
        SealedRootSummary, SnapshotDecodeError, SnapshotDiff, SummaryExport, SummaryTypeParam,
        SNAPSHOT_SCHEMA_VERSION,
    };
    use std::{
        collections::{BTreeMap, BTreeSet},
        path::PathBuf,
    };
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
                    declaration_signature: None,
                    exported_type: None,
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
                solver_facts: ModuleSolverFacts::default(),
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
        assert!(rendered.contains("\"solverFacts\""));
        assert!(rendered.contains("\"declarationSignature\""));
        assert!(rendered.contains("\"exportedType\""));
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
                        metadata: Default::default(),
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
                            bound_expr: None,
                            constraint_exprs: Vec::new(),
                            default_expr: None,
                        }],
                    },
                    Declaration {
                        metadata: Default::default(),
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
                        metadata: Default::default(),
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
                            bound_expr: None,
                            constraint_exprs: Vec::new(),
                            default_expr: None,
                        }],
                    },
                    Declaration {
                        metadata: Default::default(),
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
                        declaration_signature: None,
                        exported_type: None,
                        type_params: Vec::new(),
                        public: true,
                    },
                    SummaryExport {
                        name: String::from("Expr"),
                        kind: String::from("class"),
                        type_repr: String::from("Expr"),
                        declaration_signature: None,
                        exported_type: None,
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
                        declaration_signature: None,
                        exported_type: None,
                        type_params: Vec::new(),
                        public: true,
                    },
                    SummaryExport {
                        name: String::from("helper"),
                        kind: String::from("function"),
                        type_repr: String::from("()->int"),
                        declaration_signature: None,
                        exported_type: None,
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
                solver_facts: ModuleSolverFacts::default(),
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
                    metadata: Default::default(),
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

    #[test]
    fn diff_reports_only_additions_when_previous_is_empty() {
        let previous = IncrementalState::default();
        let current = IncrementalState {
            fingerprints: BTreeMap::from([
                (String::from("pkg.x"), 1),
                (String::from("pkg.y"), 2),
                (String::from("pkg.z"), 3),
            ]),
            summaries: Vec::new(),
            stdlib_snapshot: None,
        };

        let snapshot_diff = diff(&previous, &current);
        assert_eq!(snapshot_diff.added.len(), 3);
        assert!(snapshot_diff.removed.is_empty());
        assert!(snapshot_diff.changed.is_empty());
        assert_eq!(
            snapshot_diff.added,
            vec![
                Fingerprint { module_key: String::from("pkg.x"), fingerprint: 1 },
                Fingerprint { module_key: String::from("pkg.y"), fingerprint: 2 },
                Fingerprint { module_key: String::from("pkg.z"), fingerprint: 3 },
            ]
        );
    }

    #[test]
    fn diff_reports_only_removals_when_current_is_empty() {
        let previous = IncrementalState {
            fingerprints: BTreeMap::from([
                (String::from("pkg.a"), 10),
                (String::from("pkg.b"), 20),
            ]),
            summaries: Vec::new(),
            stdlib_snapshot: None,
        };
        let current = IncrementalState::default();

        let snapshot_diff = diff(&previous, &current);
        assert!(snapshot_diff.added.is_empty());
        assert!(snapshot_diff.changed.is_empty());
        assert_eq!(snapshot_diff.removed.len(), 2);
        assert_eq!(
            snapshot_diff.removed,
            vec![
                Fingerprint { module_key: String::from("pkg.a"), fingerprint: 10 },
                Fingerprint { module_key: String::from("pkg.b"), fingerprint: 20 },
            ]
        );
    }

    #[test]
    fn diff_handles_many_modules_correctly() {
        let previous = IncrementalState {
            fingerprints: BTreeMap::from([
                (String::from("mod.a"), 1),
                (String::from("mod.b"), 2),
                (String::from("mod.c"), 3),
                (String::from("mod.d"), 4),
                (String::from("mod.e"), 5),
                (String::from("mod.f"), 6),
                (String::from("mod.g"), 7),
                (String::from("mod.h"), 8),
                (String::from("mod.i"), 9),
                (String::from("mod.j"), 10),
                (String::from("mod.k"), 11),
            ]),
            summaries: Vec::new(),
            stdlib_snapshot: None,
        };
        let current = IncrementalState {
            fingerprints: BTreeMap::from([
                // mod.a unchanged
                (String::from("mod.a"), 1),
                // mod.b changed
                (String::from("mod.b"), 200),
                // mod.c changed
                (String::from("mod.c"), 300),
                // mod.d unchanged
                (String::from("mod.d"), 4),
                // mod.e through mod.g removed (absent)
                // mod.h unchanged
                (String::from("mod.h"), 8),
                // mod.i changed
                (String::from("mod.i"), 900),
                // mod.j unchanged
                (String::from("mod.j"), 10),
                // mod.k removed (absent)
                // new modules added
                (String::from("mod.l"), 12),
                (String::from("mod.m"), 13),
                (String::from("mod.n"), 14),
            ]),
            summaries: Vec::new(),
            stdlib_snapshot: None,
        };

        let snapshot_diff = diff(&previous, &current);
        assert_eq!(snapshot_diff.added.len(), 3);
        assert_eq!(snapshot_diff.removed.len(), 4);
        assert_eq!(snapshot_diff.changed.len(), 3);

        let added_keys: Vec<&str> =
            snapshot_diff.added.iter().map(|f| f.module_key.as_str()).collect();
        assert!(added_keys.contains(&"mod.l"));
        assert!(added_keys.contains(&"mod.m"));
        assert!(added_keys.contains(&"mod.n"));

        let removed_keys: Vec<&str> =
            snapshot_diff.removed.iter().map(|f| f.module_key.as_str()).collect();
        assert!(removed_keys.contains(&"mod.e"));
        assert!(removed_keys.contains(&"mod.f"));
        assert!(removed_keys.contains(&"mod.g"));
        assert!(removed_keys.contains(&"mod.k"));

        let changed_keys: Vec<&str> =
            snapshot_diff.changed.iter().map(|f| f.module_key.as_str()).collect();
        assert!(changed_keys.contains(&"mod.b"));
        assert!(changed_keys.contains(&"mod.c"));
        assert!(changed_keys.contains(&"mod.i"));
    }

    #[test]
    fn encode_decode_round_trip_preserves_all_fields() {
        let original = IncrementalState {
            fingerprints: BTreeMap::from([
                (String::from("pkg.alpha"), 111),
                (String::from("pkg.beta"), 222),
            ]),
            summaries: vec![
                PublicSummary {
                    module: String::from("pkg.alpha"),
                    is_package_entry: true,
                    exports: vec![SummaryExport {
                        name: String::from("Widget"),
                        kind: String::from("class"),
                        type_repr: String::from("Widget"),
                        declaration_signature: None,
                        exported_type: None,
                        type_params: vec![SummaryTypeParam {
                            name: String::from("T"),
                            bound: Some(String::from("Comparable")),
                        }],
                        public: true,
                    }],
                    imports: vec![String::from("pkg.base")],
                    sealed_roots: vec![SealedRootSummary {
                        root: String::from("Shape"),
                        members: vec![String::from("Circle"), String::from("Rect")],
                    }],
                    solver_facts: ModuleSolverFacts::default(),
                },
                PublicSummary {
                    module: String::from("pkg.beta"),
                    is_package_entry: false,
                    exports: vec![SummaryExport {
                        name: String::from("run"),
                        kind: String::from("function"),
                        type_repr: String::from("()->None"),
                        declaration_signature: None,
                        exported_type: None,
                        type_params: Vec::new(),
                        public: true,
                    }],
                    imports: Vec::new(),
                    sealed_roots: Vec::new(),
                    solver_facts: ModuleSolverFacts::default(),
                },
            ],
            stdlib_snapshot: Some(String::from("fnv1a64:stdlib_hash")),
        };

        let encoded = encode_snapshot(&original).expect("encoding should succeed");
        let decoded = decode_snapshot(&encoded).expect("decoding should succeed");

        assert_eq!(decoded.fingerprints, original.fingerprints);
        assert_eq!(decoded.summaries, original.summaries);
        assert_eq!(decoded.stdlib_snapshot, original.stdlib_snapshot);
        assert_eq!(decoded, original);
    }

    #[test]
    fn decode_snapshot_rejects_malformed_json() {
        let error = decode_snapshot("this is not json at all {{{{")
            .expect_err("malformed JSON should be rejected");
        match error {
            SnapshotDecodeError::InvalidJson(message) => {
                assert!(!message.is_empty(), "error message should not be empty");
            }
            other => panic!("expected InvalidJson, got {:?}", other),
        }
    }

    #[test]
    fn decode_snapshot_accepts_current_schema_version() {
        let json = format!(
            r#"{{"schema_version":{},"fingerprints":{{"mod.a":42}}}}"#,
            SNAPSHOT_SCHEMA_VERSION
        );
        let state = decode_snapshot(&json).expect("valid schema version should be accepted");
        assert_eq!(state.fingerprints.get("mod.a"), Some(&42));
    }

    #[test]
    fn snapshot_captures_package_entry_status() {
        let state = snapshot(&ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/pkg/__init__.tpy"),
                module_key: String::from("pkg"),
                module_kind: SourceKind::TypePython,
                declarations: vec![Declaration {
                    metadata: Default::default(),
                    name: String::from("VERSION"),
                    kind: DeclarationKind::Value,
                    detail: String::from("1.0"),
                    value_type: Some(String::from("str")),
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
                summary_fingerprint: 0,
            }],
        });

        assert_eq!(state.summaries.len(), 1);
        assert!(state.summaries[0].is_package_entry);
    }

    #[test]
    fn snapshot_fingerprint_changes_when_export_signature_changes() {
        let make_graph = |detail: &str| ModuleGraph {
            nodes: vec![ModuleNode {
                module_path: PathBuf::from("src/pkg/ops.tpy"),
                module_key: String::from("pkg.ops"),
                module_kind: SourceKind::TypePython,
                declarations: vec![Declaration {
                    metadata: Default::default(),
                    name: String::from("compute"),
                    kind: DeclarationKind::Function,
                    detail: String::from(detail),
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
                summary_fingerprint: 0,
            }],
        };

        let state_v1 = snapshot(&make_graph("(x: int)->int"));
        let state_v2 = snapshot(&make_graph("(x: int, y: int)->int"));

        let fp_v1 = state_v1.fingerprints.get("pkg.ops").expect("fingerprint should exist");
        let fp_v2 = state_v2.fingerprints.get("pkg.ops").expect("fingerprint should exist");
        assert_ne!(fp_v1, fp_v2, "fingerprint should change when export signature changes");
    }

    #[test]
    fn snapshot_with_multiple_modules_sorts_summaries_by_module_key() {
        let state = snapshot(&ModuleGraph {
            nodes: vec![
                ModuleNode {
                    module_path: PathBuf::from("src/pkg/zebra.tpy"),
                    module_key: String::from("pkg.zebra"),
                    module_kind: SourceKind::TypePython,
                    declarations: Vec::new(),
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
                    summary_fingerprint: 0,
                },
                ModuleNode {
                    module_path: PathBuf::from("src/pkg/alpha.tpy"),
                    module_key: String::from("pkg.alpha"),
                    module_kind: SourceKind::TypePython,
                    declarations: Vec::new(),
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
                    summary_fingerprint: 0,
                },
                ModuleNode {
                    module_path: PathBuf::from("src/pkg/mid.tpy"),
                    module_key: String::from("pkg.mid"),
                    module_kind: SourceKind::TypePython,
                    declarations: Vec::new(),
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
                    summary_fingerprint: 0,
                },
            ],
        });

        let module_keys: Vec<&str> = state.summaries.iter().map(|s| s.module.as_str()).collect();
        assert_eq!(module_keys, vec!["pkg.alpha", "pkg.mid", "pkg.zebra"]);
    }

    #[test]
    fn snapshot_diff_display_formats_error_messages() {
        let invalid_json_error = SnapshotDecodeError::InvalidJson(String::from("unexpected EOF"));
        let display_text = format!("{}", invalid_json_error);
        assert_eq!(display_text, "invalid incremental snapshot JSON: unexpected EOF");

        let version_error = SnapshotDecodeError::IncompatibleSchemaVersion(42);
        let display_text = format!("{}", version_error);
        assert!(display_text.contains("42"));
        assert!(display_text.contains(&SNAPSHOT_SCHEMA_VERSION.to_string()));
        assert_eq!(
            display_text,
            format!(
                "incremental snapshot schema version 42 is incompatible with expected version {}",
                SNAPSHOT_SCHEMA_VERSION
            )
        );
    }

    #[test]
    fn dependency_index_tracks_symbol_import_edges() {
        let graph = ModuleGraph {
            nodes: vec![
                module_node(
                    "pkg.base",
                    "src/pkg/base.tpy",
                    vec![import_declaration("shared", "pkg.shared.Value")],
                ),
                module_node(
                    "pkg.shared",
                    "src/pkg/shared.tpy",
                    vec![value_declaration("Value", "int")],
                ),
                module_node(
                    "pkg.consumer",
                    "src/pkg/consumer.tpy",
                    vec![
                        import_declaration("base", "pkg.base"),
                        import_declaration("value", "pkg.shared.Value"),
                    ],
                ),
            ],
        };

        let index = dependency_index(&graph);
        assert_eq!(
            index.imports_by_module.get("pkg.consumer"),
            Some(&BTreeSet::from([String::from("pkg.base"), String::from("pkg.shared")]))
        );
        assert_eq!(
            index.reverse_imports.get("pkg.shared"),
            Some(&BTreeSet::from([String::from("pkg.base"), String::from("pkg.consumer")]))
        );
    }

    #[test]
    fn affected_modules_rechecks_transitive_dependents_of_summary_changes() {
        let graph = ModuleGraph {
            nodes: vec![
                module_node("pkg.a", "src/pkg/a.tpy", vec![value_declaration("A", "int")]),
                module_node(
                    "pkg.b",
                    "src/pkg/b.tpy",
                    vec![import_declaration("a", "pkg.a"), value_declaration("B", "int")],
                ),
                module_node(
                    "pkg.c",
                    "src/pkg/c.tpy",
                    vec![import_declaration("b", "pkg.b"), value_declaration("C", "int")],
                ),
            ],
        };

        let affected = affected_modules(
            Some(&dependency_index(&graph)),
            &dependency_index(&graph),
            &BTreeSet::from([String::from("pkg.a")]),
            &BTreeSet::from([String::from("pkg.a")]),
        );

        assert_eq!(
            affected,
            BTreeSet::from([String::from("pkg.a"), String::from("pkg.b"), String::from("pkg.c"),])
        );
    }

    #[test]
    fn affected_modules_keeps_rechecks_local_for_implementation_only_changes() {
        let graph = ModuleGraph {
            nodes: vec![
                module_node("pkg.a", "src/pkg/a.tpy", vec![value_declaration("A", "int")]),
                module_node(
                    "pkg.b",
                    "src/pkg/b.tpy",
                    vec![import_declaration("a", "pkg.a"), value_declaration("B", "int")],
                ),
            ],
        };

        let affected = affected_modules(
            Some(&dependency_index(&graph)),
            &dependency_index(&graph),
            &BTreeSet::from([String::from("pkg.a")]),
            &BTreeSet::new(),
        );

        assert_eq!(affected, BTreeSet::from([String::from("pkg.a")]));
    }

    #[test]
    fn snapshot_diff_modules_collects_all_changed_keys() {
        let snapshot_diff = SnapshotDiff {
            added: vec![Fingerprint { module_key: String::from("pkg.new"), fingerprint: 1 }],
            removed: vec![Fingerprint { module_key: String::from("pkg.old"), fingerprint: 2 }],
            changed: vec![Fingerprint { module_key: String::from("pkg.same"), fingerprint: 3 }],
        };

        assert_eq!(
            snapshot_diff_modules(&snapshot_diff),
            BTreeSet::from([
                String::from("pkg.new"),
                String::from("pkg.old"),
                String::from("pkg.same"),
            ])
        );
    }

    fn module_node(
        module_key: &str,
        module_path: &str,
        declarations: Vec<Declaration>,
    ) -> ModuleNode {
        ModuleNode {
            module_path: PathBuf::from(module_path),
            module_key: String::from(module_key),
            module_kind: SourceKind::TypePython,
            declarations,
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
            summary_fingerprint: 0,
        }
    }

    fn import_declaration(name: &str, detail: &str) -> Declaration {
        Declaration {
            metadata: Default::default(),
            name: String::from(name),
            kind: DeclarationKind::Import,
            detail: String::from(detail),
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
        }
    }

    fn value_declaration(name: &str, value_type: &str) -> Declaration {
        Declaration {
            metadata: Default::default(),
            name: String::from(name),
            kind: DeclarationKind::Value,
            detail: String::from(value_type),
            value_type: Some(String::from(value_type)),
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
        }
    }
}

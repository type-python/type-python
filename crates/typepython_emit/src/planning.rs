use super::*;

/// Planned runtime and stub artifacts for one source module.
#[derive(Debug, Clone)]
pub struct EmitArtifact {
    /// Original source file.
    pub source_path: PathBuf,
    /// Planned `.py` output, if any.
    pub runtime_path: Option<PathBuf>,
    /// Planned `.pyi` output, if any.
    pub stub_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PlannedModuleSource {
    pub source_path: PathBuf,
    pub source_kind: SourceKind,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct RuntimeWriteSummary {
    pub runtime_files_written: usize,
    pub stub_files_written: usize,
    pub py_typed_written: usize,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct TypePythonStubContext {
    pub value_overrides: Vec<StubValueOverride>,
    pub callable_overrides: Vec<StubCallableOverride>,
    pub synthetic_methods: Vec<StubSyntheticMethod>,
    pub sealed_classes: Vec<StubSealedClass>,
    pub guarded_declaration_lines: BTreeSet<usize>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StubValueOverride {
    pub line: usize,
    pub annotation: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StubCallableOverride {
    pub line: usize,
    pub params: Vec<FunctionParam>,
    pub returns: Option<String>,
    pub use_async_syntax: bool,
    pub drop_non_builtin_decorators: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StubSyntheticMethod {
    pub class_line: usize,
    pub name: String,
    pub method_kind: MethodKind,
    pub params: Vec<FunctionParam>,
    pub returns: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StubSealedClass {
    pub line: usize,
    pub name: String,
    pub members: Vec<String>,
}

/// Generated stub flavor for inferred pass-through Python surfaces.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InferredStubMode {
    /// Internal cache-only stubs used as a typing surface for local `.py` files.
    Shadow,
    /// User-facing migration stubs with TODO markers for manual refinement.
    Migration,
}

/// Plans output paths for the provided modules.
#[must_use]
pub fn plan_emits(config: &ConfigHandle, modules: &[LoweredModule]) -> Vec<EmitArtifact> {
    let sources: Vec<_> = modules
        .iter()
        .map(|module| PlannedModuleSource {
            source_path: module.source_path.clone(),
            source_kind: module.source_kind,
        })
        .collect();
    plan_emits_for_sources(config, &sources)
}

/// Plans output paths for the provided source descriptors.
#[must_use]
pub fn plan_emits_for_sources(
    config: &ConfigHandle,
    sources: &[PlannedModuleSource],
) -> Vec<EmitArtifact> {
    let out_root = config.resolve_relative_path(&config.config.project.out_dir);
    let mut artifacts = Vec::new();
    let mut paired_by_module: BTreeMap<PathBuf, usize> = BTreeMap::new();

    for source in sources {
        let relative = relative_module_path(config, &source.source_path);
        let module_key = relative.with_extension("");

        match source.source_kind {
            SourceKind::TypePython => artifacts.push(EmitArtifact {
                source_path: source.source_path.clone(),
                runtime_path: Some(out_root.join(&relative).with_extension("py")),
                stub_path: config
                    .config
                    .emit
                    .emit_pyi
                    .then(|| out_root.join(relative).with_extension("pyi")),
            }),
            SourceKind::Python => {
                let runtime_path = out_root.join(relative.with_extension("py"));
                if let Some(index) = paired_by_module.get(&module_key).copied() {
                    artifacts[index].source_path = source.source_path.clone();
                    artifacts[index].runtime_path = Some(runtime_path);
                } else {
                    let index = artifacts.len();
                    artifacts.push(EmitArtifact {
                        source_path: source.source_path.clone(),
                        runtime_path: Some(runtime_path),
                        stub_path: None,
                    });
                    paired_by_module.insert(module_key, index);
                }
            }
            SourceKind::Stub => {
                let stub_path = out_root.join(relative.with_extension("pyi"));
                if let Some(index) = paired_by_module.get(&module_key).copied() {
                    artifacts[index].stub_path = Some(stub_path);
                } else {
                    let index = artifacts.len();
                    artifacts.push(EmitArtifact {
                        source_path: source.source_path.clone(),
                        runtime_path: None,
                        stub_path: Some(stub_path),
                    });
                    paired_by_module.insert(module_key, index);
                }
            }
        }
    }

    artifacts
}

fn relative_module_path(config: &ConfigHandle, source_path: &Path) -> PathBuf {
    let logical_root = config.resolve_relative_path(&config.config.project.root_dir);

    if let Ok(relative) = source_path.strip_prefix(logical_root) {
        return relative.to_path_buf();
    }

    source_path.file_name().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("unknown"))
}

//! Output planning boundary for TypePython.

use std::{collections::BTreeMap, fs, io, path::{Path, PathBuf}};

use typepython_config::ConfigHandle;
use typepython_lowering::LoweredModule;
use typepython_syntax::SourceKind;

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

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct RuntimeWriteSummary {
    pub runtime_files_written: usize,
    pub stub_files_written: usize,
    pub py_typed_written: usize,
}

/// Plans output paths for the provided modules.
#[must_use]
pub fn plan_emits(config: &ConfigHandle, modules: &[LoweredModule]) -> Vec<EmitArtifact> {
    modules
        .iter()
        .map(|module| {
            let relative = relative_module_path(config, &module.source_path);
            let out_root = config.resolve_relative_path(&config.config.project.out_dir);

            match module.source_kind {
                SourceKind::TypePython => EmitArtifact {
                    source_path: module.source_path.clone(),
                    runtime_path: Some(out_root.join(&relative).with_extension("py")),
                    stub_path: config
                        .config
                        .emit
                        .emit_pyi
                        .then(|| out_root.join(relative).with_extension("pyi")),
                },
                SourceKind::Python => EmitArtifact {
                    source_path: module.source_path.clone(),
                    runtime_path: Some(out_root.join(relative)),
                    stub_path: None,
                },
                SourceKind::Stub => EmitArtifact {
                    source_path: module.source_path.clone(),
                    runtime_path: None,
                    stub_path: Some(out_root.join(relative)),
                },
            }
        })
        .collect()
}

pub fn write_runtime_outputs(
    artifacts: &[EmitArtifact],
    modules: &[LoweredModule],
) -> Result<RuntimeWriteSummary, io::Error> {
    let modules_by_source: BTreeMap<_, _> =
        modules.iter().map(|module| (module.source_path.as_path(), module)).collect();
    let mut runtime_files_written = 0usize;
    let mut stub_files_written = 0usize;
    let mut package_roots = std::collections::BTreeSet::new();

    for artifact in artifacts {
        let Some(module) = modules_by_source.get(artifact.source_path.as_path()) else {
            continue;
        };

        if let Some(runtime_path) = &artifact.runtime_path {
            if let Some(parent) = runtime_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(runtime_path, &module.python_source)?;
            if runtime_path.file_name().is_some_and(|name| name == "__init__.py") {
                if let Some(parent) = runtime_path.parent() {
                    package_roots.insert(parent.to_path_buf());
                }
            }
            runtime_files_written += 1;
        }

        if let Some(stub_path) = &artifact.stub_path {
            if let Some(parent) = stub_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(stub_path, &module.python_source)?;
            stub_files_written += 1;
        }
    }

    let mut py_typed_written = 0usize;
    for package_root in package_roots {
        fs::write(package_root.join("py.typed"), "")?;
        py_typed_written += 1;
    }

    Ok(RuntimeWriteSummary {
        runtime_files_written,
        stub_files_written,
        py_typed_written,
    })
}

fn relative_module_path(config: &ConfigHandle, source_path: &Path) -> PathBuf {
    let logical_root = config.resolve_relative_path(&config.config.project.root_dir);

    if let Ok(relative) = source_path.strip_prefix(logical_root) {
        return relative.to_path_buf();
    }

    source_path.file_name().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("unknown"))
}

#[cfg(test)]
mod tests {
    use super::{EmitArtifact, RuntimeWriteSummary, write_runtime_outputs};
    use std::{
        env, fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };
    use typepython_lowering::{LoweredModule, SourceMapEntry};
    use typepython_syntax::SourceKind;

    #[test]
    fn write_runtime_outputs_emits_lowered_typepython_and_python_modules() {
        let temp_dir = temp_dir("write_runtime_outputs_emits_lowered_typepython_and_python_modules");
        let result = (|| {
            let modules = vec![
                LoweredModule {
                    source_path: PathBuf::from("src/app/__init__.tpy"),
                    source_kind: SourceKind::TypePython,
                    python_source: String::from("from typing import TypeAlias\nUserId: TypeAlias = int\n"),
                    source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                },
                LoweredModule {
                    source_path: PathBuf::from("src/app/helpers.py"),
                    source_kind: SourceKind::Python,
                    python_source: String::from("def helper():\n    return 1\n"),
                    source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                },
                LoweredModule {
                    source_path: PathBuf::from("src/app/helpers.pyi"),
                    source_kind: SourceKind::Stub,
                    python_source: String::from("def helper() -> int: ...\n"),
                    source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                },
            ];
            let artifacts = vec![
                EmitArtifact {
                    source_path: PathBuf::from("src/app/__init__.tpy"),
                    runtime_path: Some(temp_dir.join("build/app/__init__.py")),
                    stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
                },
                EmitArtifact {
                    source_path: PathBuf::from("src/app/helpers.py"),
                    runtime_path: Some(temp_dir.join("build/app/helpers.py")),
                    stub_path: None,
                },
                EmitArtifact {
                    source_path: PathBuf::from("src/app/helpers.pyi"),
                    runtime_path: None,
                    stub_path: Some(temp_dir.join("build/app/helpers.pyi")),
                },
            ];

            let summary = write_runtime_outputs(&artifacts, &modules).unwrap();
            let runtime_init = fs::read_to_string(temp_dir.join("build/app/__init__.py")).unwrap();
            let stub_init = fs::read_to_string(temp_dir.join("build/app/__init__.pyi")).unwrap();
            let runtime_helpers = fs::read_to_string(temp_dir.join("build/app/helpers.py")).unwrap();
            let stub_helpers = fs::read_to_string(temp_dir.join("build/app/helpers.pyi")).unwrap();
            let py_typed = fs::read_to_string(temp_dir.join("build/app/py.typed")).unwrap();

            (summary, runtime_init, stub_init, runtime_helpers, stub_helpers, py_typed)
        })();
        remove_temp_dir(&temp_dir);

        let (summary, runtime_init, stub_init, runtime_helpers, stub_helpers, py_typed) = result;
        assert_eq!(
            summary,
            RuntimeWriteSummary {
                runtime_files_written: 2,
                stub_files_written: 2,
                py_typed_written: 1,
            }
        );
        assert_eq!(runtime_init, "from typing import TypeAlias\nUserId: TypeAlias = int\n");
        assert_eq!(stub_init, "from typing import TypeAlias\nUserId: TypeAlias = int\n");
        assert_eq!(runtime_helpers, "def helper():\n    return 1\n");
        assert_eq!(stub_helpers, "def helper() -> int: ...\n");
        assert_eq!(py_typed, "");
    }

    fn temp_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let directory = env::temp_dir().join(format!("typepython-emit-{test_name}-{unique}"));
        fs::create_dir_all(&directory).expect("temp directory should be created");
        directory
    }

    fn remove_temp_dir(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).expect("temp directory should be removed");
        }
    }
}

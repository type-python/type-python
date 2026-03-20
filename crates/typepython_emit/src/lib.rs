//! Output planning boundary for TypePython.

use std::{collections::BTreeMap, fs, io, path::{Path, PathBuf}};

use ruff_python_ast::{Expr, Stmt};
use ruff_python_parser::parse_module;
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
            let stub_source = if module.source_kind == SourceKind::TypePython {
                rewrite_to_stub_source(&module.python_source)?
            } else {
                module.python_source.clone()
            };
            fs::write(stub_path, stub_source)?;
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

fn rewrite_to_stub_source(python: &str) -> Result<String, io::Error> {
    let parsed = parse_module(python).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unable to parse lowered Python for stub emission: {}", error.error),
        )
    })?;

    let mut edits = Vec::new();
    collect_stub_edits(python, parsed.suite(), &mut edits);
    edits.sort_by_key(|edit| edit.start_line);

    let lines: Vec<&str> = python.lines().collect();
    let mut output = Vec::new();
    let mut line = 1usize;
    let mut edits = edits.into_iter().peekable();

    while line <= lines.len() {
        if let Some(edit) = edits.peek() {
            if edit.start_line == line {
                if let Some(replacement) = &edit.replacement {
                    output.push(replacement.clone());
                }
                line = edit.end_line + 1;
                edits.next();
                continue;
            }
        }

        output.push(lines[line - 1].to_owned());
        line += 1;
    }

    let mut rewritten = output.join("\n");
    if python.ends_with('\n') {
        rewritten.push('\n');
    }
    Ok(rewritten)
}

#[derive(Debug)]
struct StubEdit {
    start_line: usize,
    end_line: usize,
    replacement: Option<String>,
}

fn collect_stub_edits(source: &str, suite: &[Stmt], edits: &mut Vec<StubEdit>) {
    let overloaded_names: std::collections::BTreeSet<_> = suite
        .iter()
        .filter_map(|statement| match statement {
            Stmt::FunctionDef(function) if function.decorator_list.iter().any(is_overload_decorator) => {
                Some(function.name.as_str().to_owned())
            }
            _ => None,
        })
        .collect();

    for statement in suite {
        match statement {
            Stmt::FunctionDef(function) => {
                let start_line = offset_to_line(source, function.name.range.start().to_usize());
                let end_offset = function.range.end().to_usize().saturating_sub(1);
                let end_line = offset_to_line(source, end_offset.max(function.range.start().to_usize()));
                let line = source.lines().nth(start_line - 1).unwrap_or("");
                edits.push(StubEdit {
                    start_line,
                    end_line,
                    replacement: if function.decorator_list.iter().any(is_overload_decorator) {
                        Some(rewrite_stub_signature_line(line))
                    } else if overloaded_names.contains(function.name.as_str()) {
                        None
                    } else {
                        Some(rewrite_stub_signature_line(line))
                    },
                });
            }
            Stmt::AnnAssign(assign) => {
                if let Some(replacement) = rewrite_stub_annotated_assignment_line(
                    source.lines().nth(offset_to_line(source, assign.range.start().to_usize()) - 1).unwrap_or(""),
                ) {
                    let start_line = offset_to_line(source, assign.range.start().to_usize());
                    edits.push(StubEdit {
                        start_line,
                        end_line: start_line,
                        replacement: Some(replacement),
                    });
                }
            }
            Stmt::ClassDef(class_def) => {
                if is_empty_stub_class_body(&class_def.body) {
                    let start_line = offset_to_line(source, class_def.name.range.start().to_usize());
                    let end_offset = class_def.range.end().to_usize().saturating_sub(1);
                    let end_line = offset_to_line(source, end_offset.max(class_def.range.start().to_usize()));
                    let line = source.lines().nth(start_line - 1).unwrap_or("");
                    edits.push(StubEdit {
                        start_line,
                        end_line,
                        replacement: Some(rewrite_stub_class_line(line)),
                    });
                } else {
                    collect_stub_edits(source, &class_def.body, edits)
                }
            }
            _ => {}
        }
    }
}

fn rewrite_stub_signature_line(line: &str) -> String {
    let trimmed = line.trim_end();
    if trimmed.contains(": ...") {
        trimmed.to_owned()
    } else if trimmed.ends_with(':') {
        format!("{trimmed} ...")
    } else {
        trimmed.to_owned()
    }
}

fn rewrite_stub_annotated_assignment_line(line: &str) -> Option<String> {
    if line.contains("TypeAlias =") {
        return None;
    }
    let (head, _) = line.split_once('=')?;
    Some(head.trim_end().to_owned())
}

fn rewrite_stub_class_line(line: &str) -> String {
    let trimmed = line.trim_end();
    if trimmed.contains(": ...") {
        trimmed.to_owned()
    } else if trimmed.ends_with(':') {
        format!("{trimmed} ...")
    } else {
        trimmed.to_owned()
    }
}

fn is_empty_stub_class_body(body: &[Stmt]) -> bool {
    body.iter().all(|statement| match statement {
        Stmt::Pass(_) => true,
        Stmt::Expr(expr) => matches!(expr.value.as_ref(), Expr::StringLiteral(_) | Expr::EllipsisLiteral(_)),
        _ => false,
    })
}

fn is_overload_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "overload",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "overload"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if name.id.as_str() == "typing")
        }
        _ => false,
    }
}

fn offset_to_line(source: &str, offset: usize) -> usize {
    let mut line = 1usize;

    for (index, character) in source.char_indices() {
        if index >= offset {
            break;
        }
        if character == '\n' {
            line += 1;
        }
    }

    line
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
                    python_source: String::from("from typing import TypeAlias\nUserId: TypeAlias = int\ncount: int = 1\n\ndef build_user() -> int:\n    return 1\n"),
                    source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                },
                LoweredModule {
                    source_path: PathBuf::from("src/app/helpers.py"),
                    source_kind: SourceKind::Python,
                    python_source: String::from("def helper():\n    return 1\n"),
                    source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                },
                LoweredModule {
                    source_path: PathBuf::from("src/app/parse.tpy"),
                    source_kind: SourceKind::TypePython,
                    python_source: String::from(
                        "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n\ndef parse(x):\n    return 0\n",
                    ),
                    source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
                },
                LoweredModule {
                    source_path: PathBuf::from("src/app/empty.tpy"),
                    source_kind: SourceKind::TypePython,
                    python_source: String::from("class Empty:\n    pass\n"),
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
                    source_path: PathBuf::from("src/app/parse.tpy"),
                    runtime_path: Some(temp_dir.join("build/app/parse.py")),
                    stub_path: Some(temp_dir.join("build/app/parse.pyi")),
                },
                EmitArtifact {
                    source_path: PathBuf::from("src/app/empty.tpy"),
                    runtime_path: Some(temp_dir.join("build/app/empty.py")),
                    stub_path: Some(temp_dir.join("build/app/empty.pyi")),
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
            let runtime_parse = fs::read_to_string(temp_dir.join("build/app/parse.py")).unwrap();
            let stub_parse = fs::read_to_string(temp_dir.join("build/app/parse.pyi")).unwrap();
            let runtime_empty = fs::read_to_string(temp_dir.join("build/app/empty.py")).unwrap();
            let stub_empty = fs::read_to_string(temp_dir.join("build/app/empty.pyi")).unwrap();
            let stub_helpers = fs::read_to_string(temp_dir.join("build/app/helpers.pyi")).unwrap();
            let py_typed = fs::read_to_string(temp_dir.join("build/app/py.typed")).unwrap();

            (summary, runtime_init, stub_init, runtime_helpers, runtime_parse, stub_parse, runtime_empty, stub_empty, stub_helpers, py_typed)
        })();
        remove_temp_dir(&temp_dir);

        let (summary, runtime_init, stub_init, runtime_helpers, runtime_parse, stub_parse, runtime_empty, stub_empty, stub_helpers, py_typed) = result;
        assert_eq!(
            summary,
            RuntimeWriteSummary {
                runtime_files_written: 4,
                stub_files_written: 4,
                py_typed_written: 1,
            }
        );
        assert_eq!(runtime_init, "from typing import TypeAlias\nUserId: TypeAlias = int\ncount: int = 1\n\ndef build_user() -> int:\n    return 1\n");
        assert_eq!(stub_init, "from typing import TypeAlias\nUserId: TypeAlias = int\ncount: int\n\ndef build_user() -> int: ...\n");
        assert_eq!(runtime_helpers, "def helper():\n    return 1\n");
        assert_eq!(runtime_parse, "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n\ndef parse(x):\n    return 0\n");
        assert_eq!(stub_parse, "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n\n");
        assert_eq!(runtime_empty, "class Empty:\n    pass\n");
        assert_eq!(stub_empty, "class Empty: ...\n");
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

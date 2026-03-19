//! Lowering boundary for TypePython.

use std::{collections::BTreeSet, path::PathBuf};

use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span};
use typepython_syntax::{SourceKind, SyntaxStatement, SyntaxTree};

/// A single source-map row.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SourceMapEntry {
    /// Original source line.
    pub original_line: usize,
    /// Lowered output line.
    pub lowered_line: usize,
}

/// Lowered representation consumed by later phases.
#[derive(Debug, Clone)]
pub struct LoweredModule {
    /// Original module path.
    pub source_path: PathBuf,
    /// Source kind of the module.
    pub source_kind: SourceKind,
    /// Lowered Python text.
    pub python_source: String,
    /// Placeholder source-map rows.
    pub source_map: Vec<SourceMapEntry>,
}

#[derive(Debug, Clone)]
pub struct LoweringResult {
    pub module: LoweredModule,
    pub diagnostics: DiagnosticReport,
}

/// Lowers a parsed module into its Python-facing form.
#[must_use]
pub fn lower(tree: &SyntaxTree) -> LoweringResult {
    let python_source = match tree.source.kind {
        SourceKind::TypePython => lower_typepython(tree),
        SourceKind::Python | SourceKind::Stub => tree.source.text.clone(),
    };
    let diagnostics = collect_lowering_diagnostics(tree);

    LoweringResult {
        module: LoweredModule {
            source_path: tree.source.path.clone(),
            source_kind: tree.source.kind,
            python_source,
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
        },
        diagnostics,
    }
}

fn lower_typepython(tree: &SyntaxTree) -> String {
    let unsafe_lines: BTreeSet<_> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::Unsafe(statement) => Some(statement.line),
            _ => None,
        })
        .collect();
    let type_aliases: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::TypeAlias(statement) if statement.type_params.is_empty() => {
                Some((statement.line, statement))
            }
            _ => None,
        })
        .collect();

    let mut lowered_lines = Vec::new();
    if !type_aliases.is_empty() && !has_typealias_import(&tree.source.text) {
        lowered_lines.push(String::from("from typing import TypeAlias"));
    }

    for (index, line) in tree.source.text.lines().enumerate() {
        let line_number = index + 1;
        if let Some(statement) = type_aliases.get(&line_number) {
            lowered_lines.push(rewrite_typealias_line(line, statement));
        } else if unsafe_lines.contains(&line_number) {
            lowered_lines.push(rewrite_unsafe_line(line));
        } else {
            lowered_lines.push(line.to_owned());
        }
    }

    let mut lowered = lowered_lines.join("\n");
    if tree.source.text.ends_with('\n') {
        lowered.push('\n');
    }
    lowered
}

fn rewrite_unsafe_line(line: &str) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    format!("{indentation}if True:")
}

fn rewrite_typealias_line(
    line: &str,
    statement: &typepython_syntax::TypeAliasStatement,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    format!(
        "{indentation}{}: TypeAlias = {}",
        statement.name, statement.value
    )
}

fn has_typealias_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import TypeAlias"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("TypeAlias"))
    })
}

fn collect_lowering_diagnostics(tree: &SyntaxTree) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();

    for statement in &tree.statements {
        match statement {
            SyntaxStatement::Unsafe(_) => {}
            SyntaxStatement::TypeAlias(statement) if statement.type_params.is_empty() => {}
            SyntaxStatement::TypeAlias(statement) => diagnostics.push(lowering_error(
                &tree.source.path,
                statement.line,
                "generic typealias",
            )),
            SyntaxStatement::Interface(statement) => diagnostics.push(lowering_error(
                &tree.source.path,
                statement.line,
                "interface",
            )),
            SyntaxStatement::DataClass(statement) => diagnostics.push(lowering_error(
                &tree.source.path,
                statement.line,
                "data class",
            )),
            SyntaxStatement::SealedClass(statement) => diagnostics.push(lowering_error(
                &tree.source.path,
                statement.line,
                "sealed class",
            )),
            SyntaxStatement::OverloadDef(statement) => diagnostics.push(lowering_error(
                &tree.source.path,
                statement.line,
                "overload def",
            )),
        }
    }

    diagnostics
}

fn lowering_error(path: &std::path::Path, line: usize, construct: &str) -> Diagnostic {
    Diagnostic::error(
        "TPY2002",
        format!("TypePython-only syntax `{construct}` is recognized but not lowerable yet"),
    )
    .with_span(Span::new(path.display().to_string(), line, 1, line, 1))
}

#[cfg(test)]
mod tests {
    use super::lower;
    use std::path::PathBuf;
    use typepython_diagnostics::DiagnosticReport;
    use typepython_syntax::{
        NamedBlockStatement, SourceFile, SourceKind, SyntaxStatement, SyntaxTree,
        TypeAliasStatement, UnsafeStatement,
    };

    #[test]
    fn lower_rewrites_top_level_unsafe_blocks() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("unsafe.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("unsafe:\n    x = 1\n"),
            },
            statements: vec![SyntaxStatement::Unsafe(UnsafeStatement { line: 1 })],
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "if True:\n    x = 1\n");
    }

    #[test]
    fn lower_rewrites_nested_unsafe_blocks_with_indentation() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("nested-unsafe.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("def update():\n    unsafe:\n        x = 1\n"),
            },
            statements: vec![SyntaxStatement::Unsafe(UnsafeStatement { line: 2 })],
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "def update():\n    if True:\n        x = 1\n");
    }

    #[test]
    fn lower_reports_unimplemented_typepython_constructs() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("typealias.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("typealias UserId = int\ninterface Service:\n"),
            },
            statements: vec![
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("UserId"),
                    type_params: Vec::new(),
                    value: String::from("int"),
                    line: 1,
                }),
                SyntaxStatement::Interface(NamedBlockStatement {
                    name: String::from("Service"),
                    type_params: Vec::new(),
                    line: 2,
                }),
            ],
            diagnostics: DiagnosticReport::default(),
        });

        let rendered = lowered.diagnostics.as_text();
        assert!(lowered.diagnostics.has_errors());
        assert!(rendered.contains("TPY2002"));
        assert!(rendered.contains("`interface`"));
    }

    #[test]
    fn lower_rewrites_non_generic_typealias_with_import() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("typealias.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("typealias UserId = int\n"),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("UserId"),
                type_params: Vec::new(),
                value: String::from("int"),
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "from typing import TypeAlias\nUserId: TypeAlias = int\n");
    }

    #[test]
    fn lower_still_blocks_generic_typealias() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("generic-typealias.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("typealias Pair[T] = tuple[T, T]\n"),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("Pair"),
                type_params: vec![typepython_syntax::TypeParam {
                    name: String::from("T"),
                    bound: None,
                }],
                value: String::from("tuple[T, T]"),
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        let rendered = lowered.diagnostics.as_text();
        assert!(lowered.diagnostics.has_errors());
        assert!(rendered.contains("TPY2002"));
        assert!(rendered.contains("`generic typealias`"));
    }
}

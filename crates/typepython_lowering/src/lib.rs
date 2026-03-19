//! Lowering boundary for TypePython.

use std::{collections::BTreeSet, path::PathBuf};

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

/// Lowers a parsed module into its Python-facing form.
#[must_use]
pub fn lower(tree: &SyntaxTree) -> LoweredModule {
    let python_source = match tree.source.kind {
        SourceKind::TypePython => lower_typepython(tree),
        SourceKind::Python | SourceKind::Stub => tree.source.text.clone(),
    };

    LoweredModule {
        source_path: tree.source.path.clone(),
        source_kind: tree.source.kind,
        python_source,
        source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
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

    let mut lowered_lines = Vec::new();
    for (index, line) in tree.source.text.lines().enumerate() {
        let line_number = index + 1;
        if unsafe_lines.contains(&line_number) {
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

#[cfg(test)]
mod tests {
    use super::lower;
    use std::path::PathBuf;
    use typepython_diagnostics::DiagnosticReport;
    use typepython_syntax::{SourceFile, SourceKind, SyntaxStatement, SyntaxTree, UnsafeStatement};

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

        println!("{}", lowered.python_source);
        assert_eq!(lowered.python_source, "if True:\n    x = 1\n");
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

        assert_eq!(lowered.python_source, "def update():\n    if True:\n        x = 1\n");
    }
}

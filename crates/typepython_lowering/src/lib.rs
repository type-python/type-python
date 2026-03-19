//! Lowering boundary for TypePython.

use std::path::PathBuf;

use typepython_syntax::{SourceKind, SyntaxTree};

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
    LoweredModule {
        source_path: tree.source.path.clone(),
        source_kind: tree.source.kind,
        python_source: tree.source.text.clone(),
        source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
    }
}

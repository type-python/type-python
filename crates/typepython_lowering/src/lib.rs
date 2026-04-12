//! Lowering boundary for TypePython.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use ruff_python_ast::{Expr, Stmt};
use ruff_python_parser::parse_module;
use ruff_text_size::Ranged;
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span, SuggestionApplicability};
use typepython_syntax::{SourceKind, SyntaxStatement, SyntaxTree};
use typepython_target::{
    EmitStyle, PythonTarget, RuntimeFeature, RuntimeTypingForm, RuntimeTypingSemantics,
};

mod core;
#[cfg(test)]
mod tests;
mod typeddict;

use core::*;
pub use core::*;
use typeddict::*;

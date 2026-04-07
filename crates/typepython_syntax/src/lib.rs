//! Source classification and parser boundary for TypePython.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use ruff_python_ast::{visitor, visitor::Visitor, Expr, Stmt, TypeParam as AstTypeParam};
use ruff_python_parser::parse_module;
use ruff_text_size::Ranged;
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span};

include!("syntax_parts/surface.rs");
include!("syntax_parts/metadata_collectors.rs");
include!("syntax_parts/parsing.rs");
include!("syntax_parts/formatting.rs");
include!("syntax_parts/type_expr.rs");
include!("syntax_parts/extraction.rs");

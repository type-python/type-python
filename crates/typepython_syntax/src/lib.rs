//! Source classification and parser boundary for TypePython.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use ruff_python_ast::{Expr, Stmt, TypeParam as AstTypeParam, visitor, visitor::Visitor};
use ruff_python_parser::parse_module;
use ruff_text_size::Ranged;
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span};

mod syntax_parts;

pub use syntax_parts::*;

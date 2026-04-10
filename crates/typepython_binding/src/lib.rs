//! Symbol binding boundary for TypePython.

use std::{collections::BTreeMap, path::PathBuf};

use typepython_syntax::{MethodKind, SourceKind, SyntaxStatement, SyntaxTree};

mod binding_impl;
#[cfg(test)]
mod tests;
mod types;

pub use binding_impl::bind;
use binding_impl::*;
pub use types::*;

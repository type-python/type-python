//! Symbol binding boundary for TypePython.

use std::path::PathBuf;

use typepython_lowering::LoweredModule;

/// Placeholder bound symbol table.
#[derive(Debug, Clone, Default)]
pub struct BindingTable {
    /// Module path for the symbol table.
    pub module_path: PathBuf,
    /// Collected symbol names.
    pub symbols: Vec<String>,
}

/// Binds a lowered module into a symbol table.
#[must_use]
pub fn bind(module: &LoweredModule) -> BindingTable {
    BindingTable { module_path: module.source_path.clone(), symbols: Vec::new() }
}

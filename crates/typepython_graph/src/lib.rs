//! Module graph and summary construction boundary for TypePython.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::PathBuf,
};

use typepython_binding::BindingTable;

/// Summary node for one module.
#[derive(Debug, Clone)]
pub struct ModuleNode {
    /// Module path on disk.
    pub module_path: PathBuf,
    pub symbols: Vec<String>,
    pub summary_fingerprint: u64,
}

/// Module graph placeholder.
#[derive(Debug, Clone, Default)]
pub struct ModuleGraph {
    /// Collected module nodes.
    pub nodes: Vec<ModuleNode>,
}

/// Builds a placeholder module graph from bound modules.
#[must_use]
pub fn build(bindings: &[BindingTable]) -> ModuleGraph {
    let nodes = bindings
        .iter()
        .map(|binding| ModuleNode {
            module_path: binding.module_path.clone(),
            symbols: binding.symbols.clone(),
            summary_fingerprint: hash_summary(binding),
        })
        .collect();

    ModuleGraph { nodes }
}

fn hash_summary(binding: &BindingTable) -> u64 {
    let mut hasher = DefaultHasher::new();
    binding.module_path.hash(&mut hasher);
    binding.symbols.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::build;
    use std::path::PathBuf;
    use typepython_binding::BindingTable;

    #[test]
    fn build_carries_bound_symbols_into_module_nodes() {
        let graph = build(&[BindingTable {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            symbols: vec![String::from("UserId"), String::from("User")],
        }]);

        assert_eq!(graph.nodes[0].symbols, vec!["UserId", "User"]);
    }

    #[test]
    fn build_changes_fingerprint_when_symbols_change() {
        let first = build(&[BindingTable {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            symbols: vec![String::from("UserId")],
        }]);
        let second = build(&[BindingTable {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            symbols: vec![String::from("UserId"), String::from("User")],
        }]);

        println!(
            "{} -> {}",
            first.nodes[0].summary_fingerprint,
            second.nodes[0].summary_fingerprint
        );
        assert_ne!(
            first.nodes[0].summary_fingerprint,
            second.nodes[0].summary_fingerprint
        );
    }
}

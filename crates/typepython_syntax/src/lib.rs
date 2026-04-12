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

#[cfg(test)]
mod baseline_tests {
    use std::{fs, path::PathBuf};

    #[test]
    fn parser_baseline_manifest_matches_current_dependency_versions() {
        let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let baseline = fs::read_to_string(crate_root.join("../../stdlib/BASELINE.toml"))
            .expect("baseline manifest should be readable");
        let cargo_toml =
            fs::read_to_string(crate_root.join("Cargo.toml")).expect("Cargo.toml should exist");

        for dependency in [
            "littrs-ruff-python-ast = \"0.6.2\"",
            "littrs-ruff-python-parser = \"0.6.2\"",
            "littrs-ruff-text-size = \"0.6.2\"",
        ] {
            assert!(
                cargo_toml.contains(dependency),
                "syntax Cargo.toml should contain parser dependency `{dependency}`",
            );
        }
        for baseline_line in [
            "littrs_ruff_python_ast = \"0.6.2\"",
            "littrs_ruff_python_parser = \"0.6.2\"",
            "littrs_ruff_text_size = \"0.6.2\"",
        ] {
            assert!(
                baseline.contains(baseline_line),
                "baseline manifest should contain `{baseline_line}`",
            );
        }
    }

    #[test]
    fn bundled_stdlib_baseline_confirms_python_314_markers() {
        let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let versions = fs::read_to_string(crate_root.join("../../stdlib/VERSIONS"))
            .expect("stdlib/VERSIONS should be readable");
        let typing = fs::read_to_string(crate_root.join("../../stdlib/typing.pyi"))
            .expect("stdlib/typing.pyi should be readable");
        let baseline = fs::read_to_string(crate_root.join("../../stdlib/BASELINE.toml"))
            .expect("baseline manifest should be readable");

        assert!(versions.contains("_interpqueues: 3.13-"));
        assert!(versions.contains("_zstd: 3.14-"));
        assert!(typing.contains("from annotationlib import Format"));
        assert!(typing.contains("\"get_protocol_members\", \"is_protocol\", \"NoDefault\", \"TypeIs\", \"ReadOnly\""));
        assert!(baseline.contains("target_range = \"3.10-3.14\""));
    }
}

from __future__ import annotations

import importlib.util
import pathlib
import re
import subprocess
import sys
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]


def read_text(relative_path: str) -> str:
    return (REPO_ROOT / relative_path).read_text(encoding="utf-8")


def workspace_crate_names() -> set[str]:
    cargo = read_text("Cargo.toml")
    members_block = re.search(
        r"^members\s*=\s*\[(.*?)^\]", cargo, flags=re.MULTILINE | re.DOTALL
    )
    if members_block is None:
        raise AssertionError("workspace members block was not found in Cargo.toml")
    members = re.findall(r'"([^"]+)"', members_block.group(1))
    return {pathlib.Path(member).name for member in members}


def string_assignment(relative_path: str, key: str) -> str:
    text = read_text(relative_path)
    match = re.search(
        rf'^\s*{re.escape(key)}\s*=\s*"([^"]+)"', text, flags=re.MULTILINE
    )
    if match is None:
        raise AssertionError(f"{key} was not found in {relative_path}")
    return match.group(1)


def load_script_module(relative_path: str, module_name: str):
    module_path = REPO_ROOT / relative_path
    spec = importlib.util.spec_from_file_location(module_name, module_path)
    if spec is None or spec.loader is None:
        raise AssertionError(f"unable to import {relative_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


def markdown_section(text: str, heading: str) -> str:
    pattern = rf"^##\s+{re.escape(heading)}\s*$\n(?P<body>.*?)(?=^##\s+|\Z)"
    match = re.search(pattern, text, flags=re.MULTILINE | re.DOTALL)
    if match is None:
        raise AssertionError(f"README section {heading!r} was not found")
    return match.group("body")


class RepoContractsTests(unittest.TestCase):
    def test_docs_cover_workspace_crates_without_hard_coded_totals(self) -> None:
        architecture = read_text("docs/architecture.md")
        contributing = read_text("docs/contributing.md")
        expected_crates = workspace_crate_names()

        architecture_sections = set(
            re.findall(
                r"^###\s+(typepython_[a-z_]+)$", architecture, flags=re.MULTILINE
            )
        )
        contributing_structure = set(
            re.findall(r"^\s+(typepython_[a-z_]+)\/", contributing, flags=re.MULTILINE)
        )

        self.assertEqual(architecture_sections, expected_crates)
        self.assertEqual(contributing_structure, expected_crates)
        self.assertNotRegex(architecture, r"containing\s+\d+\s+Rust crates")
        self.assertNotRegex(contributing, r"The\s+\d+\s+crates form")

    def test_readmes_agree_on_supported_target_range(self) -> None:
        supported_target_phrase = "Python 3.10 through 3.14"

        self.assertIn(supported_target_phrase, read_text("README.md"))
        self.assertIn(supported_target_phrase, read_text("README-PyPI.md"))

    def test_msrv_contract_is_consistent_and_verified(self) -> None:
        msrv = string_assignment("Cargo.toml", "rust-version")
        pinned_toolchain = string_assignment("rust-toolchain.toml", "channel")

        makefile = read_text("Makefile")
        readme = read_text("README.md")
        contributing = read_text("docs/contributing.md")
        getting_started = read_text("docs/getting-started.md")
        bootstrap = read_text("scripts/bootstrap-rust.sh")
        rust_workflow = read_text(".github/workflows/rust.yml")

        self.assertEqual(msrv, pinned_toolchain)
        self.assertIn(f"The workspace MSRV is Rust {msrv}.", readme)
        self.assertIn(f"workspace MSRV is {msrv}", contributing)
        self.assertIn(f"workspace MSRV: {msrv}", getting_started)
        self.assertIn(f'TOOLCHAIN="{pinned_toolchain}"', bootstrap)
        self.assertIn(f"MSRV ?= {msrv}", makefile)
        self.assertIn("msrv-check:", makefile)
        self.assertIn("$(CARGO) +$(MSRV) check --workspace", makefile)
        self.assertIn("msrv-check:", rust_workflow)
        self.assertIn(f"Install Rust {msrv}", rust_workflow)
        self.assertIn(
            f"rustup toolchain install {msrv} --profile minimal", rust_workflow
        )
        self.assertIn("make msrv-check", rust_workflow)
        self.assertIn("REQUESTED_VERSION", bootstrap)
        self.assertIn("bump_version.py", bootstrap)

    def test_bundled_stdlib_baseline_is_pinned_and_current(self) -> None:
        baseline = read_text("stdlib/BASELINE.toml")
        makefile = read_text("Makefile")
        rust_workflow = read_text(".github/workflows/rust.yml")

        self.assertRegex(baseline, r'typeshed_commit = "[0-9a-f]{40}"')
        self.assertIn('upstream_repository = "https://github.com/python/typeshed"', baseline)
        self.assertIn("stdlib-baseline-check:", makefile)
        self.assertIn("scripts/refresh_stdlib_stubs.py --check", rust_workflow)
        subprocess.run(
            [sys.executable, "scripts/refresh_stdlib_stubs.py", "--check"],
            cwd=REPO_ROOT,
            check=True,
        )

    def test_conformance_report_is_generated_and_checked(self) -> None:
        makefile = read_text("Makefile")
        rust_workflow = read_text(".github/workflows/rust.yml")
        report = read_text("docs/conformance-report.md")

        self.assertIn("conformance-check:", makefile)
        self.assertIn("scripts/conformance_report.py --check", rust_workflow)
        self.assertIn("TypePython Conformance Report", report)
        self.assertIn("Normative MUST Traceability", report)
        self.assertIn("language-spec-v1:L", report)
        self.assertNotIn("| Core v1 | MUST | missing |", report)
        subprocess.run(
            [sys.executable, "scripts/conformance_report.py", "--check"],
            cwd=REPO_ROOT,
            check=True,
        )

    def test_beta_scope_and_release_gate_are_documented(self) -> None:
        pyproject = read_text("pyproject.toml")
        package_version = string_assignment("pyproject.toml", "version")
        readme = read_text("README.md")
        pypi_readme = read_text("README-PyPI.md")
        faq = read_text("docs/faq.md")
        beta = read_text("docs/beta-readiness.md")
        workflow = read_text(".github/workflows/rust.yml")
        makefile = read_text("Makefile")

        self.assertIn("Development Status :: 4 - Beta", pyproject)
        self.assertIn("Core v1 Beta", readme)
        self.assertIn("Core v1 Beta", pypi_readme)
        self.assertIn(f"Core v1 Beta** (v{package_version})", readme)
        self.assertIn(f"Core v1 Beta** (v{package_version})", pypi_readme)
        self.assertIn("not a blanket production-ready claim", faq)
        self.assertIn("Stable Core v1 During Beta", beta)
        self.assertIn("Supported DX, Non-Stable", beta)
        self.assertIn("Experimental Opt-In", beta)
        self.assertIn("Roadmap / Prototype", beta)
        self.assertIn("beta-release-gate", workflow)
        self.assertIn("require-beta-release-gate", read_text(".github/workflows/publish.yml"))
        self.assertIn("beta-release-gate:", makefile)

    def test_feature_status_contracts_match_marketing_claims(self) -> None:
        readme = read_text("README.md")
        pypi_readme = read_text("README-PyPI.md")
        beta = read_text("docs/beta-readiness.md")
        feature_status = read_text("docs/feature-status.md")
        conformance = read_text("docs/conformance-report.md")

        self.assertIn("[TypePython Feature Status](feature-status.md)", beta)
        for status in (
            "Stable Core v1",
            "Supported DX",
            "Experimental opt-in",
            "Roadmap / prototype",
        ):
            self.assertIn(status, feature_status)

        self.assertIn(
            "| Sealed exhaustiveness | Core v1 | MUST | `cargo test -p typepython-checking sealed` |",
            conformance,
        )
        self.assertNotIn(
            "| Sealed exhaustiveness | DX v1 | SHOULD | missing |",
            conformance,
        )

        why_not = markdown_section(readme, "Why not just mypy / pyright / PEP 695?")
        self.assertIn("`sealed class` + compiler-proved exhaustiveness", why_not)
        self.assertNotIn("Framework shapes beyond", why_not)
        self.assertNotIn("Roadmap / prototype", why_not)
        self.assertNotIn("Experimental opt-in", why_not)

        self.assertIn("Core checker semantics", readme)
        self.assertIn("Core checker semantics", pypi_readme)
        self.assertIn("author-time checks", pypi_readme)

    def test_conformance_beta_scope_classification_is_precise(self) -> None:
        conformance_report = load_script_module(
            "scripts/conformance_report.py",
            "conformance_report_under_test",
        )

        conditional_rule = (
            "Conditional return syntax is an Experimental v1 feature. "
            "If an implementation enables it explicitly, it MUST follow the rules in this subsection."
        )
        self.assertEqual(
            conformance_report.beta_rule_status(
                conditional_rule,
                ("cargo test -p typepython-syntax",),
            ),
            "not-claimed-beta",
        )
        self.assertEqual(
            conformance_report.inferred_rule_tests(
                "A name MUST NOT shadow a hard keyword (see Section 7.7.2)."
            ),
            ("cargo test -p typepython-syntax",),
        )

    def test_diagnostic_coverage_report_is_generated_and_checked(self) -> None:
        makefile = read_text("Makefile")
        report = read_text("docs/diagnostic-test-coverage.md")

        self.assertIn("diagnostic-coverage-check:", makefile)
        self.assertIn("scripts/diagnostic_test_coverage.py --check", makefile)
        self.assertIn("Diagnostic Test Coverage", report)
        self.assertIn("TPY4001", report)
        self.assertIn("TPY4029", report)
        subprocess.run(
            [sys.executable, "scripts/diagnostic_test_coverage.py", "--check"],
            cwd=REPO_ROOT,
            check=True,
        )

    def test_strict_typing_config_options_are_wired_to_checker(self) -> None:
        checker = read_text("crates/typepython_checking/src/lib.rs")
        assignability = read_text(
            "crates/typepython_checking/src/type_system/assignability.rs"
        )
        semantic = read_text("crates/typepython_checking/src/semantic.rs")
        semantic_tests = read_text("crates/typepython_checking/src/tests/semantic.rs")
        pipeline = read_text("crates/typepython_cli/src/pipeline.rs")
        lsp_lifecycle = read_text("crates/typepython_lsp/src/workspace/lifecycle.rs")
        coverage = read_text("docs/diagnostic-test-coverage.md")

        self.assertIn("pub strict_nulls: bool", checker)
        self.assertIn("pub no_implicit_dynamic: bool", checker)
        self.assertIn("strict_nulls: config.strict_nulls", checker)
        self.assertIn("no_implicit_dynamic: config.no_implicit_dynamic", checker)
        self.assertIn("fn assignability_options(&self) -> AssignabilityOptions", checker)
        self.assertIn("implicit_dynamic_diagnostics(context, node)", checker)

        self.assertIn("pub(super) struct AssignabilityOptions", assignability)
        self.assertIn("pub(super) strict_nulls: bool", assignability)
        self.assertIn("if !options.strict_nulls", assignability)
        self.assertIn("semantic_type_is_assignable_with_options", assignability)

        self.assertIn("context.no_implicit_dynamic", semantic)
        self.assertIn('Diagnostic::error("TPY4029"', semantic)
        self.assertIn("strict_nulls: false", semantic_tests)
        self.assertIn("no_implicit_dynamic: true", semantic_tests)

        self.assertIn("strict_nulls: config.config.typing.strict_nulls", pipeline)
        self.assertIn(
            "no_implicit_dynamic: config.config.typing.no_implicit_dynamic",
            pipeline,
        )
        self.assertIn(
            "CheckerOptions::from_typing_config(&config.config.typing)",
            pipeline,
        )
        self.assertIn(
            "CheckerOptions::from_typing_config(&self.config.config.typing)",
            lsp_lifecycle,
        )
        self.assertIn("TPY4029", coverage)

    def test_unknown_assignability_contract_is_enforced(self) -> None:
        assignability = read_text(
            "crates/typepython_checking/src/type_system/assignability.rs"
        )
        semantic_tests = read_text("crates/typepython_checking/src/tests/semantic.rs")
        conformance_plan = read_text("docs/spec/conformance-and-test-plan-v1.md")
        conformance_report = read_text("docs/conformance-report.md")
        conformance_script = read_text("scripts/conformance_report.py")

        self.assertIn("let actual_is_unknown = is_unknown_semantic_type(&actual)", assignability)
        self.assertIn("if actual_is_unknown && is_any_semantic_type(&expected)", assignability)
        self.assertIn("is_top_receiving_semantic_type(&expected)", assignability)
        self.assertIn("is_top_escaping_semantic_type(&actual)", assignability)
        self.assertIn("fn is_dynamic_semantic_type", assignability)
        self.assertIn("fn is_unknown_semantic_type", assignability)
        self.assertNotIn("is_top_assignable_semantic_type", assignability)
        self.assertLess(
            assignability.index("let actual_is_unknown = is_unknown_semantic_type(&actual)"),
            assignability.index("if !actual_is_unknown"),
        )
        self.assertLess(
            assignability.index("if expanded == *stripped"),
            assignability.index("expand_semantic_type_aliases_in_provider_context"),
        )

        for test_name in (
            "check_accepts_assignment_into_unknown_boundary",
            "check_reports_unknown_assignment_to_concrete_type",
            "check_accepts_unknown_assignment_to_allowed_boundary_types",
            "check_reports_unknown_assignment_to_any",
            "check_reports_unknown_assignment_to_any_alias",
            "check_reports_unknown_call_argument_to_concrete_parameter",
            "check_reports_unknown_return_to_concrete_type",
            "semantic_assignability_treats_unknown_as_checked_boundary_not_any",
        ):
            self.assertIn(test_name, semantic_tests)

        self.assertIn("Unknown and dynamic boundary assignability", conformance_plan)
        self.assertIn("Unknown and dynamic boundary assignability", conformance_report)
        self.assertIn("cargo test -p typepython-checking unknown", conformance_report)
        self.assertIn(
            '"Unknown and dynamic boundary assignability": ("cargo test -p typepython-checking unknown",)',
            conformance_script,
        )

    def test_insta_snapshots_are_limited_to_emission_golden_outputs(self) -> None:
        allowed_prefixes = (
            "crates/typepython_emit/src/snapshots/",
            "crates/typepython_lowering/src/snapshots/",
        )
        snapshot_paths = sorted(
            path.relative_to(REPO_ROOT).as_posix() for path in REPO_ROOT.glob("**/*.snap")
        )

        self.assertTrue(snapshot_paths, "expected committed insta snapshots")
        disallowed = [
            path for path in snapshot_paths if not path.startswith(allowed_prefixes)
        ]
        self.assertEqual(disallowed, [])

    def test_migration_guide_covers_strict_baseline_workflow(self) -> None:
        migration_guide = read_text("docs/migration-guide.md")
        cli_reference = read_text("docs/cli-reference.md")

        self.assertIn("severity_overrides", migration_guide)
        self.assertIn("--no-new-diagnostics", migration_guide)
        self.assertIn("type: ignore[TPY4001]", migration_guide)
        self.assertIn("severity_overrides", cli_reference)
        self.assertIn("type: ignore[...]", cli_reference)


if __name__ == "__main__":
    unittest.main()

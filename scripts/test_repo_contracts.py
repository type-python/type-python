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
        self.assertIn("Semantic Sub-Rule Evidence", report)
        self.assertIn("unknown` is diagnosed as TPY4003", report)
        self.assertIn("check_reports_none_call_argument_when_strict_nulls_is_enabled", report)
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
        release_evidence = read_text("docs/release-evidence.md")
        workflow = read_text(".github/workflows/rust.yml")
        makefile = read_text("Makefile")

        self.assertIn("Development Status :: 4 - Beta", pyproject)
        self.assertIn("Core v1.0 RC", readme)
        self.assertIn("Core v1.0 RC", pypi_readme)
        self.assertIn(f"Core v1.0 RC** (v{package_version})", readme)
        self.assertIn(f"Core v1.0 RC** (v{package_version})", pypi_readme)
        self.assertIn("release-candidate status", beta)
        self.assertIn("Development Status :: 4 - Beta", beta)
        self.assertIn("not a blanket production-ready claim", faq)
        self.assertIn("Stable Core v1 During Beta", beta)
        self.assertIn("Core v1.0 RC Scope", beta)
        self.assertIn("Full v1.0 / DX Promotion", beta)
        self.assertIn("Core v1.0 RC", readme)
        self.assertIn("Core v1.0 RC", pypi_readme)
        self.assertIn("Supported DX, Non-Stable", beta)
        self.assertIn("Experimental Opt-In", beta)
        self.assertIn("Roadmap / Prototype", beta)
        self.assertIn("beta-release-gate", workflow)
        self.assertIn("require-beta-release-gate", read_text(".github/workflows/publish.yml"))
        self.assertIn("beta-release-gate:", makefile)
        self.assertIn("quickstart-smoke:", makefile)
        self.assertIn("quickstart-smoke", makefile.split("beta-release-gate:", 1)[1])
        self.assertIn("flagship-smoke:", makefile)
        self.assertIn("flagship-smoke", makefile.split("beta-release-gate:", 1)[1])
        self.assertIn("examples-smoke:", makefile)
        self.assertIn("examples-smoke", makefile.split("beta-release-gate:", 1)[1])
        self.assertIn("security-audit:", makefile)
        self.assertIn("$(CARGO) audit", makefile)
        self.assertIn("security-audit", makefile.split("beta-release-gate:", 1)[1])
        self.assertIn("security-audit:", workflow)
        self.assertIn("run: cargo audit", workflow)
        self.assertIn("security-audit", workflow.split("beta-release-gate:", 1)[1])
        self.assertIn("flagship_core_smoke.py", beta)
        self.assertIn("examples_smoke.py", beta)
        self.assertIn("run: cargo test --workspace", workflow)
        self.assertIn("[Release Evidence](release-evidence.md)", beta)
        self.assertIn("macOS Local RC Preflight", release_evidence)
        self.assertIn("downstream_checker_smoke.py", release_evidence)
        self.assertIn("make fuzz-smoke", release_evidence)
        self.assertIn("CI-only follow-up", release_evidence)

    def test_feature_status_contracts_match_marketing_claims(self) -> None:
        readme = read_text("README.md")
        pypi_readme = read_text("README-PyPI.md")
        beta = read_text("docs/beta-readiness.md")
        feature_status = read_text("docs/feature-status.md")
        experimental = read_text("docs/experimental-features.md")
        conformance = read_text("docs/conformance-report.md")

        self.assertIn("[TypePython Feature Status](feature-status.md)", beta)
        self.assertIn("[Experimental Feature Registry](experimental-features.md)", beta)
        self.assertIn("[Experimental Feature Registry](experimental-features.md)", feature_status)
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
        self.assertIn(
            "| Enum exhaustiveness | DX v1 | SHOULD | `cargo test -p typepython-checking check_reports_non_exhaustive_enum_match`",
            conformance,
        )
        self.assertNotIn(
            "| Enum exhaustiveness | DX v1 | SHOULD | missing |",
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

        for feature_id in (
            "runtime_validators",
            "conditional_returns",
            "infer_passthrough",
            "sync_async_dual_emit",
            "shape_transforms",
            "framework_adapters",
            "effect_rows",
            "taint",
            "validator_witnesses",
            "notebook_ingestion",
        ):
            self.assertIn(f"`{feature_id}`", experimental)
        self.assertIn("disabled by default", experimental)
        self.assertIn("explicit opt-in", experimental)
        self.assertIn("No primary README differentiator", experimental)
        self.assertIn("Promotion Checklist", experimental)

    def test_experimental_scope_is_guarded(self) -> None:
        readme = read_text("README.md")
        pypi_readme = read_text("README-PyPI.md")
        beta = read_text("docs/beta-readiness.md")
        diagnostics = read_text("docs/diagnostics.md")
        experimental = read_text("docs/experimental-features.md")
        author_time = read_text("docs/author-time-semantics.md")
        configuration = read_text("docs/configuration.md")
        artifact_spec = read_text("docs/spec/artifact-and-tooling-spec-v1.md")
        config = read_text("crates/typepython_config/src/lib.rs")

        for text in (readme, pypi_readme):
            self.assertIn("Experimental Feature Registry", text)
            self.assertIn("Experimental opt-in", text)
            self.assertIn("Roadmap / prototype", text)

        self.assertIn(
            "| [`research-roadmap-demo/`](examples/research-roadmap-demo/) | Roadmap / prototype slices only; not a Core v1 stability claim |",
            readme,
        )

        for code in ("TPY4026", "TPY4028"):
            section = diagnostics.split(f"#### {code}", maxsplit=1)[1].split("#### ", maxsplit=1)[0]
            self.assertIn("Roadmap / prototype diagnostic", section)
            self.assertIn("not part of Stable Core v1", section)

        self.assertIn("experimental scope contract", beta)
        self.assertIn("test_experimental_scope_is_guarded", beta)
        self.assertIn("validate_experimental_features", config)
        self.assertIn("accepted_features", config)
        self.assertIn("EXPERIMENTAL_RUNTIME_VALIDATORS", config)
        self.assertNotIn("\n[[boundaries]]\n", configuration)
        self.assertIn("# [[boundaries]]", configuration)
        self.assertIn(
            'requires `"runtime_validators"` in `[experimental].accepted_features`',
            artifact_spec,
        )

        self.assertIn("Experimental opt-in features share the same release contract", experimental)
        self.assertIn("Roadmap / prototype rows are tracked here", experimental)
        self.assertIn(
            "promotion requires adding or tightening explicit gates",
            " ".join(experimental.split()),
        )
        self.assertIn("P0 effect rows, P3 taint, and P4 validator", author_time)
        self.assertIn("not Stable Core v1 promises", author_time)

    def test_author_time_semantics_are_not_marketed_as_external_guarantees(self) -> None:
        readme = read_text("README.md")
        pypi_readme = read_text("README-PyPI.md")
        interop = read_text("docs/interop.md")
        beta = read_text("docs/beta-readiness.md")
        feature_status = read_text("docs/feature-status.md")
        author_time = read_text("docs/author-time-semantics.md")

        for text in (readme, pypi_readme):
            normalized = " ".join(text.split())
            self.assertIn("author-time TypeScript-class ergonomics", normalized)
            self.assertIn("emitted artifacts remain standard Python", normalized)
            self.assertIn(
                "do not transfer every TypePython-only constraint to downstream checkers",
                normalized,
            )
            self.assertIn("No required per-checker plugin for emitted output", normalized)
            self.assertIn("TypePython `.tpy` authoring", normalized)
            self.assertIn("external consumers do not automatically inherit", normalized)
            self.assertIn("TypePython-only", normalized)
            self.assertNotIn("**TypePython `.tpy`**", normalized)
            self.assertNotIn("No custom runtime. No per-checker plugin. No vendor lock-in.", normalized)

        self.assertIn("TypePython-only safety applies inside the author package", readme)
        self.assertIn(
            "They validate the standard published surface; they do not make ordinary",
            readme,
        )
        self.assertIn(
            "They validate the standard published surface; they do not make ordinary",
            pypi_readme,
        )

        self.assertIn("## Guarantee Levels", interop)
        self.assertIn("TypePython-checked author package", interop)
        self.assertIn("Default external consumer", interop)
        self.assertIn("TypePython-aware consumer", interop)
        self.assertIn("internal author-time safety plus portable external typing", interop)

        self.assertIn("ordinary downstream consumers receive portable Python typing", beta)
        self.assertIn("TypePython-aware sidecar, checker plugin, or consumer mode", beta)
        self.assertIn("author-package checker semantics", feature_status)
        self.assertIn("Public-facing claims should say", feature_status)
        self.assertIn("## External Consumer Model", author_time)
        self.assertIn("Standard consumers use the generated `.py`, `.pyi`, and `py.typed`", author_time)

    def test_downstream_interop_gate_is_documented(self) -> None:
        beta = read_text("docs/beta-readiness.md")
        contributing = read_text("docs/contributing.md")
        cli_reference = read_text("docs/cli-reference.md")
        readme = read_text("README.md")
        pypi_readme = read_text("README-PyPI.md")
        normalized_beta = " ".join(beta.split())

        self.assertIn("mypy strict", beta)
        self.assertIn("pyright strict", beta)
        self.assertIn("basedpyright strict", beta)
        self.assertIn("ty strict", beta)
        self.assertIn("Python 3.10 through 3.14 target lowering", beta)
        self.assertIn("typeshed-heavy imports", beta)
        self.assertIn("implicit namespace packages", beta)
        self.assertIn("partial-stub metadata", normalized_beta)
        self.assertIn("TypedDict-heavy SDK clients", beta)
        self.assertIn("overload-heavy APIs", beta)
        self.assertIn("ecosystem_corpus.baseline_categories", beta)
        self.assertIn("allowlist_expires", beta)

        for fixture_name in (
            "typeshed-heavy-package",
            "namespace-package",
            "pep561-partial-stub-package",
            "ecosystem-patterns-package",
            "sdk-client-package",
            "overload-heavy-package",
        ):
            self.assertIn(fixture_name, contributing)

        self.assertIn("mypy,pyright,basedpyright,ty", cli_reference)
        self.assertIn("basedpyright", readme)
        self.assertIn("basedpyright", pypi_readme)

    def test_industrial_performance_gate_is_documented(self) -> None:
        architecture = read_text("docs/architecture.md")
        benchmarks = read_text("docs/benchmarks.md")
        beta = read_text("docs/beta-readiness.md")
        normalized_beta = " ".join(beta.split())
        makefile = read_text("Makefile")
        workflow = read_text(".github/workflows/rust.yml")
        project = read_text("crates/typepython_project/src/lib.rs")
        lsp_bench = read_text("crates/typepython_lsp/benches/incremental.rs")

        self.assertIn("scripts/industrial_perf_smoke.py", benchmarks)
        self.assertIn("cold check", benchmarks)
        self.assertIn("warm check", benchmarks)
        self.assertIn("single-file implementation edit", benchmarks)
        self.assertIn("public surface edit", benchmarks)
        self.assertIn("peak RSS", benchmarks)
        self.assertIn("p95/p99", benchmarks)
        self.assertIn("make perf-smoke", benchmarks)
        self.assertIn("perf-smoke:", makefile)
        self.assertIn("beta-release-gate: fmt-check", makefile)
        self.assertIn("perf-smoke", makefile.split("beta-release-gate:", 1)[1])
        self.assertIn("scripts/test_industrial_perf_smoke.py", makefile)
        self.assertIn("industrial-performance-smoke:", workflow)
        self.assertIn("scripts/industrial_perf_smoke.py --json-out", workflow)
        self.assertIn("industrial-performance-smoke", workflow.split("beta-release-gate:", 1)[1])

        self.assertIn("lsp_incremental_impl_edit_session_512_modules", benchmarks)
        self.assertIn("lsp_incremental_public_edit_session_512_modules", benchmarks)
        self.assertIn("bench_incremental_implementation_edit_session_large", lsp_bench)
        self.assertIn("bench_incremental_public_edit_session_large", lsp_bench)

        self.assertIn("recursive file manifest", architecture)
        self.assertIn("content hash", architecture)
        self.assertIn("const SUPPORT_SOURCE_INDEX_VERSION: u32 = 2", project)
        self.assertIn("struct CachedSupportFile", project)
        self.assertIn("content_hash", project)

        self.assertIn("industrial performance evidence", beta)
        self.assertIn("cold check time", normalized_beta)
        self.assertIn("warm check time", normalized_beta)
        self.assertIn("single-file implementation edit latency", normalized_beta)
        self.assertIn("public surface edit latency", normalized_beta)
        self.assertIn("peak RSS", beta)
        self.assertIn("p95/p99", beta)

    def test_coverage_gate_enforces_lines_regions_functions_and_branches(self) -> None:
        makefile = read_text("Makefile")
        workflow = read_text(".github/workflows/rust.yml")
        contributing = read_text("docs/contributing.md")

        self.assertIn("COVERAGE_MIN_LINES ?= 65", makefile)
        self.assertIn("COVERAGE_MIN_REGIONS ?= 60", makefile)
        self.assertIn("COVERAGE_MIN_FUNCTIONS ?= 60", makefile)
        self.assertIn("COVERAGE_MIN_BRANCHES ?= 50", makefile)
        self.assertIn("llvm-cov --branch", makefile)
        self.assertIn("scripts/check_coverage.py", makefile)
        self.assertIn("--min-branches $(COVERAGE_MIN_BRANCHES)", makefile)
        self.assertIn("Install nightly Rust with coverage tools", workflow)
        self.assertIn("scripts/test_coverage_gate.py", workflow)
        self.assertIn("branch instrumentation", contributing)

    def test_python_package_host_matrix_matches_declared_support(self) -> None:
        pyproject = read_text("pyproject.toml")
        workflow = read_text(".github/workflows/rust.yml")
        expected_versions = ["3.9", "3.10", "3.11", "3.12", "3.13", "3.14"]

        self.assertIn('requires-python = ">=3.9"', pyproject)
        for version in expected_versions:
            self.assertIn(f'Programming Language :: Python :: {version}', pyproject)
        self.assertIn("python-package-hosts:", workflow)
        self.assertIn(
            'python-version: ["3.9", "3.10", "3.11", "3.12", "3.13", "3.14"]',
            workflow,
        )
        host_job = workflow.split("python-package-hosts:", 1)[1].split("msrv-check:", 1)[0]
        self.assertIn("python -m build --wheel", host_job)
        self.assertIn("python -m pip install --force-reinstall dist/*.whl", host_job)
        self.assertIn("python scripts/quickstart_smoke.py", host_job)
        release_gate = workflow.split("beta-release-gate:", 1)[1]
        self.assertIn("python-package-hosts", release_gate)

    def test_fuzz_matrix_covers_parser_roundtrip_lowering_and_checker(self) -> None:
        makefile = read_text("Makefile")
        rust_workflow = read_text(".github/workflows/rust.yml")
        security_workflow = read_text(".github/workflows/security.yml")
        parser_target = read_text("fuzz/fuzz_targets/parser.rs")
        type_expr_target = read_text("fuzz/fuzz_targets/type_expr.rs")
        checker_target = read_text("fuzz/fuzz_targets/checker.rs")

        expected = "parser type_expr lowering_stub checker"
        self.assertIn(f"FUZZ_TARGETS ?= {expected}", makefile)
        for workflow in (rust_workflow, security_workflow):
            self.assertIn("[parser, type_expr, lowering_stub, checker]", workflow)
        self.assertIn("String::from_utf8_lossy(data)", parser_target)
        self.assertNotIn("data[1..]", parser_target)
        self.assertIn("assert_eq!(reparsed.render(), rendered", type_expr_target)
        self.assertIn("let binding = bind(&tree)", checker_target)
        self.assertIn("let graph = build(&[binding])", checker_target)
        self.assertIn("let _ = check(&graph)", checker_target)

    def test_dx_and_lsp_stability_boundary_is_documented(self) -> None:
        dx = read_text("docs/dx-stability.md")
        beta = read_text("docs/beta-readiness.md")
        lsp = read_text("docs/lsp.md")
        readme = read_text("README.md")
        pypi_readme = read_text("README-PyPI.md")

        for expected in (
            "Supported DX, non-stable",
            "LSP capabilities",
            "JSON schema",
            "watch-mode tests",
            "formatting and code-action golden tests",
            "installable VS Code extension",
            "copy-paste configuration for Neovim, Helix, Sublime Text, and Emacs",
            "Logs and crash diagnostics",
        ):
            self.assertIn(expected, dx)

        self.assertIn("DX and LSP Stability", beta)
        self.assertIn("LSP capability shape, formatter behavior, code-action details", beta)
        normalized_dx = " ".join(dx.split())
        self.assertIn("Core v1.0 RC does not promote the Supported DX tier", normalized_dx)
        self.assertIn("A Core-only v1.0 RC may ship before this gate is complete", normalized_dx)
        self.assertIn("Supported Beta DX surface", lsp)
        self.assertIn("DX and LSP Stability", lsp)

        self.assertIn("supported DX stdio LSP server", readme)
        self.assertIn("Supported DX LSP server", pypi_readme)
        for text in (readme, pypi_readme):
            normalized = " ".join(text.split())
            self.assertIn("extension packaging", normalized)
            self.assertIn("Core v1.0 RC compatibility promise", normalized)

    def test_cli_json_schema_is_versioned_and_documented(self) -> None:
        main = read_text("crates/typepython_cli/src/main.rs")
        api_diff = read_text("crates/typepython_cli/src/api_diff.rs")
        adapter = read_text("crates/typepython_cli/src/adapter.rs")
        verification = read_text("crates/typepython_cli/src/verification.rs")
        migration = read_text("crates/typepython_cli/src/migration.rs")
        type_health = read_text("crates/typepython_cli/src/type_health.rs")
        cli_reference = read_text("docs/cli-reference.md")
        diagnostics = read_text("docs/diagnostics.md")
        json_schema = read_text("docs/json-output-schema.md")

        self.assertIn("pub(crate) const CLI_JSON_SCHEMA_VERSION: u32 = 1", main)
        for source in (main, api_diff, adapter, verification, migration, type_health):
            self.assertIn('"schema_version": CLI_JSON_SCHEMA_VERSION', source)

        self.assertIn("CLI JSON Output Schema", cli_reference)
        self.assertIn('"schema_version": 1', cli_reference)
        self.assertIn("CLI JSON Output Schema", diagnostics)
        self.assertIn("Current schema version: `1`", json_schema)
        self.assertIn("typepython check --format json", json_schema)
        self.assertIn("typepython adapter validate --format json", json_schema)
        self.assertIn("must not remove or change the meaning", json_schema)
        self.assertIn("1-based span coordinates", json_schema)

    def test_lsp_capabilities_and_logging_controls_are_stable(self) -> None:
        main = read_text("crates/typepython_cli/src/main.rs")
        lsp_tests = read_text("crates/typepython_lsp/src/tests.rs")
        lsp_docs = read_text("docs/lsp.md")
        cli_reference = read_text("docs/cli-reference.md")
        dx = read_text("docs/dx-stability.md")

        self.assertIn("fn tracing_filter() -> EnvFilter", main)
        self.assertIn('env::var("TYPEPYTHON_LOG")', main)
        self.assertIn('env::var_os("TYPEPYTHON_LOG_FILE")', main)
        self.assertIn("RUST_LOG", main)

        self.assertIn("handle_initialize_returns_required_capabilities", lsp_tests)
        self.assertIn('"completionProvider"', lsp_tests)
        self.assertIn('"executeCommandProvider"', lsp_tests)
        self.assertIn('"textDocumentSync"', lsp_tests)

        for text in (lsp_docs, cli_reference):
            self.assertIn("TYPEPYTHON_LOG", text)
            self.assertIn("TYPEPYTHON_LOG_FILE", text)
            self.assertIn("RUST_LOG", text)

        self.assertIn("capability drift", dx)

    def test_watch_rebuild_recovery_is_documented_and_tested(self) -> None:
        main = read_text("crates/typepython_cli/src/main.rs")
        migration_tests = read_text("crates/typepython_cli/src/tests/migration.rs")
        cli_reference = read_text("docs/cli-reference.md")
        dx = read_text("docs/dx-stability.md")

        self.assertIn("fn run_watch_rebuild(", main)
        self.assertIn("watch rebuild failed", main)
        self.assertIn("run_watch_rebuild_reloads_project_and_recovers_after_checker_failure", migration_tests)
        self.assertIn("Reloads project configuration", cli_reference)
        self.assertIn("Keeps watching after type-checking", cli_reference)
        self.assertIn("configuration reload", dx)
        self.assertIn("rebuild failure recovery", dx)

    def test_formatter_and_code_action_golden_coverage_is_documented(self) -> None:
        lsp_tests = read_text("crates/typepython_lsp/src/tests.rs")
        lsp_docs = read_text("docs/lsp.md")
        dx = read_text("docs/dx-stability.md")

        for test_name in (
            "formatting_returns_restored_typepython_source_edits",
            "formatting_reports_missing_explicit_formatter",
            "code_actions_offer_machine_applicable_return_suggestion",
            "code_actions_offer_project_workflow_commands",
            "code_action_returns_only_source_commands_when_no_quickfixes_apply",
        ):
            self.assertIn(test_name, lsp_tests)
            self.assertIn(test_name, lsp_docs)

        self.assertIn("formatting and code-action golden tests", dx)
        self.assertIn("diagnostic quick fixes and command IDs", dx)

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
            "CheckerOptions::from_config(&config.config)",
            pipeline,
        )
        self.assertIn(
            "CheckerOptions::from_config(&self.config.config)",
            lsp_lifecycle,
        )
        self.assertIn("TPY4029", coverage)

    def test_checker_api_default_boundary_is_documented(self) -> None:
        checker = read_text("crates/typepython_checking/src/lib.rs")
        architecture = read_text("docs/architecture.md")

        self.assertIn("pub fn core_project_default() -> Self", checker)
        self.assertIn("pub fn permissive_test_default() -> Self", checker)
        self.assertIn("Compatibility default for legacy tests and boolean helper APIs", checker)
        self.assertIn("Legacy compatibility helper", checker)
        self.assertIn("check_with_checker_options", checker)
        self.assertIn("check_with_binding_metadata_and_options", checker)

        self.assertIn("Checker option API boundary", architecture)
        self.assertIn("`CheckerOptions::default()` is the Core project default", architecture)
        self.assertIn(
            "`CheckerOptions::permissive_test_default()` is reserved for tests",
            architecture,
        )
        self.assertIn("Boolean-argument helpers", architecture)

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
        self.assertIn("check_reports_unknown_subscript_from_real_parse_pipeline", conformance_report)
        self.assertIn("check_reports_unknown_arithmetic_from_real_parse_pipeline", conformance_report)
        self.assertIn(
            '"Unknown and dynamic boundary assignability": ("cargo test -p typepython-checking unknown",)',
            conformance_script,
        )
        self.assertIn("SEMANTIC_SUBRULE_EVIDENCE", conformance_script)

    def test_canonical_type_relation_boundaries_are_enforced(self) -> None:
        architecture = read_text("docs/architecture.md")
        assignability = read_text(
            "crates/typepython_checking/src/type_system/assignability.rs"
        )

        self.assertIn("Canonical type model boundary", architecture)
        self.assertIn("`SemanticType` is the checker relationship shape", architecture)
        self.assertIn("pub(super) struct TypeRelationContext", assignability)
        self.assertIn(
            "semantic_invariant_type_matches_with_options(\n                    node,\n                    nodes,\n                    expected_arg,\n                    actual_arg,\n                    options,",
            assignability,
        )
        self.assertIn("join_semantic_type_candidates(actual_args.to_vec())", assignability)

        for forbidden in (
            "fn invariant_type_matches",
            "fn direct_type_matches",
            "render_semantic_type(expected_arg)",
            "render_semantic_type(actual_arg)",
            "actual_args.iter().map(render_semantic_type)",
        ):
            self.assertNotIn(forbidden, assignability)

        allowed_direct_assignability = {
            pathlib.Path("crates/typepython_checking/src/type_system/assignability.rs"),
            pathlib.Path("crates/typepython_checking/src/calls/reporting.rs"),
        }
        offenders: list[str] = []
        for path in (REPO_ROOT / "crates/typepython_checking/src").glob("**/*.rs"):
            relative_path = path.relative_to(REPO_ROOT)
            if "/tests/" in relative_path.as_posix():
                continue
            text = path.read_text(encoding="utf-8")
            if re.search(r"\bdirect_type_matches\s*\(", text):
                offenders.append(relative_path.as_posix())
                continue
            if (
                relative_path not in allowed_direct_assignability
                and re.search(r"\bdirect_type_is_assignable\s*\(", text)
            ):
                offenders.append(relative_path.as_posix())

        self.assertEqual(offenders, [])

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

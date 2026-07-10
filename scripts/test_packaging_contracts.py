from __future__ import annotations

import os
import pathlib
import re
import unittest
from unittest import mock

from typepython import _runner


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]


def read_text(relative_path: str) -> str:
    return (REPO_ROOT / relative_path).read_text(encoding="utf-8")


class PackagingContractTests(unittest.TestCase):
    def test_runner_distinguishes_installed_package_missing_binary(self) -> None:
        with (
            mock.patch.dict(os.environ, {}, clear=True),
            mock.patch.object(_runner, "_bundled_command", return_value=None),
            mock.patch.object(_runner, "_cargo_typepython_command", return_value=None),
            mock.patch.object(_runner, "_is_repo_checkout", return_value=False),
        ):
            with self.assertRaisesRegex(RuntimeError, "installed package"):
                _runner._command()

    def test_runner_distinguishes_checkout_missing_cargo(self) -> None:
        with (
            mock.patch.dict(os.environ, {}, clear=True),
            mock.patch.object(_runner, "_bundled_command", return_value=None),
            mock.patch.object(_runner, "_cargo_typepython_command", return_value=None),
            mock.patch.object(_runner, "_is_repo_checkout", return_value=True),
        ):
            with self.assertRaisesRegex(RuntimeError, "source checkout"):
                _runner._command()

    def test_runner_prefers_typepython_bin_override(self) -> None:
        with mock.patch.dict(os.environ, {"TYPEPYTHON_BIN": "/opt/typepython/bin/typepython"}):
            self.assertEqual(_runner._command(), ["/opt/typepython/bin/typepython"])

    def test_packaging_docs_and_build_contract_explain_wheel_strategy(self) -> None:
        pyproject = read_text("pyproject.toml")
        setup = read_text("setup.py")
        manifest = read_text("MANIFEST.in")
        makefile = read_text("Makefile")
        rust_workflow = read_text(".github/workflows/rust.yml")
        publish_workflow = read_text(".github/workflows/publish.yml")
        sdist_smoke = read_text("scripts/sdist_smoke.py")
        packaging = read_text("docs/packaging.md")
        getting_started = read_text("docs/getting-started.md")
        beta = read_text("docs/beta-readiness.md")
        readme = read_text("README.md")
        pypi_readme = read_text("README-PyPI.md")

        self.assertRegex(pyproject, r'build = "cp312-\*"')
        self.assertIn('return ("py3", "none", plat)', setup)
        self.assertIn("Rust 1.94.0", setup)
        self.assertIn("prebuilt type-python wheel", setup)
        self.assertIn("_copy_bundled_stdlib", setup)
        self.assertIn("typepython/stdlib/", packaging)
        self.assertIn('"py.typed"', pyproject)
        self.assertTrue((REPO_ROOT / "typepython" / "py.typed").is_file())
        for graft in ("graft crates", "graft stdlib", "graft templates"):
            self.assertIn(graft, manifest)

        self.assertIn("sdist-smoke:", makefile)
        self.assertIn("sdist-smoke", makefile.split("beta-release-gate:", 1)[1])
        self.assertIn("scripts/sdist_smoke.py dist/*.tar.gz", rust_workflow)
        self.assertIn("scripts/sdist_smoke.py dist/*.tar.gz", publish_workflow)
        self.assertIn('"crates/typepython_cli/src/main.rs"', sdist_smoke)
        self.assertIn('"stdlib/BASELINE.toml"', sdist_smoke)
        self.assertIn('"templates/typepython.toml"', sdist_smoke)
        self.assertIn('"typepython/py.typed"', sdist_smoke)
        self.assertIn('"--wheel"', sdist_smoke)
        self.assertIn("validate_sdist_contents(source_root)", sdist_smoke)
        self.assertIn('"quickstart_smoke.py"', sdist_smoke)
        self.assertIn("source-distribution smoke", " ".join(packaging.split()).lower())

        for text in (packaging, getting_started, beta, readme, pypi_readme):
            normalized = " ".join(text.split())
            self.assertIn("py3-none-<platform>", normalized)
            self.assertIn("Rust CLI", normalized)

        self.assertIn("without `cargo`", beta)
        self.assertIn("Runtime Launcher Resolution", packaging)
        self.assertIn("source checkout missing `cargo`", packaging)

    def test_packaging_doc_version_matches_project(self) -> None:
        pyproject = read_text("pyproject.toml")
        version = re.search(r'(?m)^version = "([^"]+)"$', pyproject)
        self.assertIsNotNone(version)
        self.assertIn(
            f"typepython-vscode-{version.group(1)}.vsix",
            read_text("editors/vscode/README.md"),
        )


if __name__ == "__main__":
    unittest.main()

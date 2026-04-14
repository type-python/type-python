from __future__ import annotations

import pathlib
import re
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


if __name__ == "__main__":
    unittest.main()

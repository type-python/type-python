#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = ROOT / "Cargo.toml"
PYPROJECT_TOML = ROOT / "pyproject.toml"
PACKAGE_INIT = ROOT / "typepython" / "__init__.py"
CARGO_LOCK = ROOT / "Cargo.lock"
README = ROOT / "README.md"
PYPI_README = ROOT / "README-PyPI.md"
LSP_DOC = ROOT / "docs" / "lsp.md"
VSCODE_PACKAGE = ROOT / "editors" / "vscode" / "package.json"
VSCODE_README = ROOT / "editors" / "vscode" / "README.md"
VERSION_PATTERN = r"\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?"


def replace_single(pattern: str, replacement: str, text: str, label: str) -> str:
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise SystemExit(f"unable to update {label}")
    return updated


def replace_literal(old: str, new: str, text: str, label: str) -> str:
    if old not in text:
        raise SystemExit(f"unable to find version {old!r} in {label}")
    return text.replace(old, new)


def replace_status_version(text: str, version: str, label: str) -> str:
    return replace_single(
        rf"\(v{VERSION_PATTERN}\)",
        f"(v{version})",
        text,
        f"{label} Core status version",
    )


def extract_workspace_members(text: str) -> list[str]:
    match = re.search(r"members = \[(.*?)\]", text, flags=re.DOTALL)
    if not match:
        raise SystemExit("unable to locate workspace members in Cargo.toml")
    return re.findall(r'"([^"]+)"', match.group(1))


def extract_package_name(text: str, path: Path) -> str:
    match = re.search(r'^name = "([^"]+)"$', text, flags=re.MULTILINE)
    if not match:
        raise SystemExit(f"unable to locate package.name in {path}")
    return match.group(1)


def extract_pyproject_version(text: str) -> str:
    match = re.search(r'^version = "([^"]+)"$', text, flags=re.MULTILINE)
    if not match:
        raise SystemExit("unable to locate [project].version in pyproject.toml")
    return match.group(1)


def workspace_package_names() -> list[str]:
    members = extract_workspace_members(CARGO_TOML.read_text())
    return [
        extract_package_name((ROOT / member / "Cargo.toml").read_text(), ROOT / member / "Cargo.toml")
        for member in members
    ]


def update_cargo_lock(text: str, package_names: list[str], new_version: str) -> str:
    lines = text.splitlines()
    current_name: str | None = None
    in_package = False
    touched = set()

    for index, line in enumerate(lines):
        if line == "[[package]]":
            current_name = None
            in_package = True
            continue
        if in_package and line.startswith("name = "):
            current_name = line.split('"')[1]
            continue
        if in_package and line.startswith("version = ") and current_name in package_names:
            lines[index] = f'version = "{new_version}"'
            touched.add(current_name)
            continue
        if in_package and line == "":
            in_package = False
            current_name = None

    missing = set(package_names) - touched
    if missing:
        raise SystemExit(f"unable to update Cargo.lock entries for: {', '.join(sorted(missing))}")
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description="Synchronize TypePython package versions")
    parser.add_argument("version", help="New semantic version, for example 0.0.8 or 1.0.0-rc.1")
    args = parser.parse_args()

    if not re.fullmatch(VERSION_PATTERN, args.version):
        raise SystemExit(
            f"invalid version {args.version!r}; expected semantic version like 0.0.8 or 1.0.0-rc.1"
        )

    pyproject_text = PYPROJECT_TOML.read_text()
    old_version = extract_pyproject_version(pyproject_text)

    cargo_text = CARGO_TOML.read_text()
    init_text = PACKAGE_INIT.read_text()
    lock_text = CARGO_LOCK.read_text()
    readme_text = README.read_text()
    pypi_readme_text = PYPI_README.read_text()
    lsp_doc_text = LSP_DOC.read_text()
    vscode_package_text = VSCODE_PACKAGE.read_text()
    vscode_readme_text = VSCODE_README.read_text()

    cargo_text = replace_single(
        r'^version = "[^"]+"$',
        f'version = "{args.version}"',
        cargo_text,
        "Cargo.toml workspace.package.version",
    )
    pyproject_text = replace_single(
        r'^version = "[^"]+"$',
        f'version = "{args.version}"',
        pyproject_text,
        "pyproject.toml [project].version",
    )
    init_text = replace_single(
        r'^__version__ = "[^"]+"$',
        f'__version__ = "{args.version}"',
        init_text,
        "typepython/__init__.py __version__",
    )
    lock_text = update_cargo_lock(lock_text, workspace_package_names(), args.version)
    readme_text = replace_status_version(readme_text, args.version, "README.md")
    pypi_readme_text = replace_status_version(
        pypi_readme_text, args.version, "README-PyPI.md"
    )
    lsp_doc_text = replace_literal(old_version, args.version, lsp_doc_text, "docs/lsp.md")
    vscode_package_text = replace_single(
        r'^  "version": "[^"]+",$',
        f'  "version": "{args.version}",',
        vscode_package_text,
        "editors/vscode/package.json version",
    )
    vscode_readme_text = replace_literal(
        old_version, args.version, vscode_readme_text, "editors/vscode/README.md"
    )

    CARGO_TOML.write_text(cargo_text)
    PYPROJECT_TOML.write_text(pyproject_text)
    PACKAGE_INIT.write_text(init_text)
    CARGO_LOCK.write_text(lock_text)
    README.write_text(readme_text)
    PYPI_README.write_text(pypi_readme_text)
    LSP_DOC.write_text(lsp_doc_text)
    VSCODE_PACKAGE.write_text(vscode_package_text)
    VSCODE_README.write_text(vscode_readme_text)

    print(f"updated version {old_version} -> {args.version}")


if __name__ == "__main__":
    main()

from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
import sys
from typing import Sequence


def _repo_root() -> pathlib.Path:
    return pathlib.Path(__file__).resolve().parent.parent


def _cargo_typepython_command() -> list[str] | None:
    repo_root = _repo_root()
    cargo_toml = repo_root / "Cargo.toml"
    if not cargo_toml.exists():
        return None
    cargo = shutil.which("cargo")
    if cargo is None:
        return None
    return [cargo, "run", "-p", "typepython-cli", "--"]


def _configured_command() -> list[str] | None:
    configured = os.environ.get("TYPEPYTHON_BIN")
    if not configured:
        return None
    return [configured]


def _command() -> list[str]:
    configured = _configured_command()
    if configured is not None:
        return configured
    cargo_command = _cargo_typepython_command()
    if cargo_command is not None:
        return cargo_command
    raise RuntimeError(
        "Unable to locate the TypePython Rust CLI. Set TYPEPYTHON_BIN or run from a repository checkout with cargo available."
    )


def main(argv: Sequence[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    command = [*_command(), *args]
    completed = subprocess.run(command, cwd=_repo_root(), check=False)
    return completed.returncode

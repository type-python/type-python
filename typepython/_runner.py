from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
import sys
from typing import Sequence


def _repo_root() -> pathlib.Path:
    return pathlib.Path(__file__).resolve().parent.parent


def _is_repo_checkout() -> bool:
    repo_root = _repo_root()
    return (
        repo_root.joinpath("Cargo.toml").is_file()
        and repo_root.joinpath("crates/typepython_cli/Cargo.toml").is_file()
    )


def _cargo_typepython_command() -> list[str] | None:
    if not _is_repo_checkout():
        return None
    cargo = shutil.which("cargo")
    if cargo is None:
        return None
    cargo_toml = _repo_root() / "Cargo.toml"
    return [cargo, "run", "--manifest-path", str(cargo_toml), "-p", "typepython-cli", "--"]


def _configured_command() -> list[str] | None:
    configured = os.environ.get("TYPEPYTHON_BIN")
    if not configured:
        return None
    return [configured]


def _bundled_command() -> list[str] | None:
    binary_name = "typepython.exe" if os.name == "nt" else "typepython"
    bundled = pathlib.Path(__file__).resolve().parent / "bin" / binary_name
    if not bundled.is_file():
        return None
    return [str(bundled)]


def _command() -> list[str]:
    configured = _configured_command()
    if configured is not None:
        return configured
    bundled = _bundled_command()
    if bundled is not None:
        return bundled
    cargo_command = _cargo_typepython_command()
    if cargo_command is not None:
        return cargo_command
    if _is_repo_checkout():
        raise RuntimeError(
            "Unable to locate the TypePython Rust CLI in this source checkout. "
            "Install Rust 1.94.0 with cargo via ./scripts/bootstrap-rust.sh, "
            "build the CLI with `cargo build --release -p typepython-cli`, "
            "or set TYPEPYTHON_BIN=/path/to/typepython."
        )
    raise RuntimeError(
        "Unable to locate the bundled TypePython Rust CLI in the installed package. "
        "Reinstall a supported platform wheel, or build from the source distribution "
        "with Rust 1.94.0 and cargo available. You can also set "
        "TYPEPYTHON_BIN=/path/to/typepython."
    )


def main(argv: Sequence[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    command = [*_command(), *args]
    completed = subprocess.run(command, check=False)
    return completed.returncode

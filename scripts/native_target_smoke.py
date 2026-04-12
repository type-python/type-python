from __future__ import annotations

import argparse
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile


def run(command: list[str], cwd: pathlib.Path | None = None) -> None:
    location = f" (cwd={cwd})" if cwd is not None else ""
    print(f"+ {' '.join(command)}{location}")
    subprocess.run(command, cwd=cwd, check=True)


def resolve_entrypoint() -> str:
    configured = os.environ.get("TYPEPYTHON_BIN")
    if configured:
        return configured

    scripts_dir = pathlib.Path(sys.executable).parent
    for candidate in ("typepython", "typepython.exe"):
        path = scripts_dir / candidate
        if path.is_file():
            return str(path)

    entrypoint = shutil.which("typepython")
    if entrypoint is not None:
        return entrypoint

    raise SystemExit(
        "typepython executable was not installed into PATH or the active Python scripts directory"
    )


def rewrite_target_python(config_path: pathlib.Path, target: str) -> None:
    original = config_path.read_text()
    rewritten, count = re.subn(
        r'(?m)^target_python = "[^"]+"$',
        f'target_python = "{target}"',
        original,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"unable to rewrite target_python in {config_path}")
    config_path.write_text(rewritten)


def write_native_source(source_path: pathlib.Path) -> None:
    source_path.write_text(
        """from typing import ReadOnly, TypeIs, TypedDict
from warnings import deprecated

type Pair[T = int] = tuple[T, T]

class Box[T = int]:
    value: T

    def __init__(self, value: T):
        self.value = value

    def clone(self) -> "Box[T]":
        return Box(self.value)

@deprecated("use first_pair")
def first_pair[T = int](pair: Pair[T] = (1, 2)) -> T:
    return pair[0]

class Config(TypedDict):
    flag: ReadOnly[bool]

def accepts(value: object) -> TypeIs[int]:
    return isinstance(value, int)

PAIR: Pair[int] = (1, 2)
BOX: Box[int] = Box(1)
CONFIG: Config = {"flag": True}
VALUE: int = first_pair(PAIR)
assert accepts(VALUE)
"""
    )


def assert_native_outputs(project_dir: pathlib.Path) -> None:
    runtime_path = project_dir / ".typepython" / "build" / "app" / "__init__.py"
    stub_path = project_dir / ".typepython" / "build" / "app" / "__init__.pyi"
    py_typed_path = project_dir / ".typepython" / "build" / "app" / "py.typed"

    for path in (runtime_path, stub_path, py_typed_path):
        if not path.is_file():
            raise SystemExit(f"missing expected native build output: {path}")

    runtime_source = runtime_path.read_text()
    stub_source = stub_path.read_text()

    for rendered in (
        "type Pair[T = int] = tuple[T, T]",
        "class Box[T = int]:",
        "def first_pair[T = int](pair: Pair[T] = (1, 2)) -> T:",
    ):
        if rendered not in runtime_source:
            raise SystemExit(f"native runtime output is missing expected syntax: {rendered}")

    if "TypeVar(" in runtime_source:
        raise SystemExit("native runtime output unexpectedly materialized TypeVar definitions")

    for rendered in (
        "type Pair[T = int] = tuple[T, T]",
        "def first_pair[T = int](pair: Pair[T] = (1, 2)) -> T: ...",
    ):
        if rendered not in stub_source:
            raise SystemExit(f"native stub output is missing expected syntax: {rendered}")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Build and verify a native-target TypePython project",
    )
    parser.add_argument(
        "--target-python",
        required=True,
        choices=("3.13", "3.14"),
        help="Native Python target version to exercise",
    )
    args = parser.parse_args()

    entrypoint = resolve_entrypoint()
    run([entrypoint, "--help"])

    with tempfile.TemporaryDirectory(prefix="typepython-native-smoke-") as tmp:
        root = pathlib.Path(tmp)
        project_dir = root / "native-project"

        run([entrypoint, "init", "--dir", "native-project"], cwd=root)
        rewrite_target_python(project_dir / "typepython.toml", args.target_python)
        write_native_source(project_dir / "src" / "app" / "__init__.tpy")

        run([entrypoint, "check", "--project", "."], cwd=project_dir)
        run([entrypoint, "build", "--project", "."], cwd=project_dir)
        assert_native_outputs(project_dir)
        run(
            [entrypoint, "verify", "--project", ".", "--unsafe-runtime-imports"],
            cwd=project_dir,
        )

    print(f"typepython native target smoke test passed for {args.target_python}")


if __name__ == "__main__":
    main()

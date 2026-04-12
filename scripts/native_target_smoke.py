from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

from typepython.annotation_compat import AnnotationFormat, get_annotations, supported_formats


def run(command: list[str], cwd: pathlib.Path | None = None) -> None:
    location = f" (cwd={cwd})" if cwd is not None else ""
    print(f"+ {' '.join(command)}{location}")
    subprocess.run(command, cwd=cwd, check=True)


def capture(command: list[str], cwd: pathlib.Path | None = None) -> str:
    location = f" (cwd={cwd})" if cwd is not None else ""
    print(f"+ {' '.join(command)}{location}")
    completed = subprocess.run(
        command,
        cwd=cwd,
        check=True,
        text=True,
        capture_output=True,
    )
    return completed.stdout


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
type Explosive = 1 / 0

class Scope:
    type Alias = Nested

    class Nested:
        pass

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


def assert_runtime_semantics(project_dir: pathlib.Path) -> None:
    build_root = project_dir / ".typepython" / "build"
    probe = """
import importlib
import json
import pathlib
import sys

build_root = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(build_root))
module = importlib.import_module("app")
from typepython.annotation_compat import AnnotationFormat, get_annotations, supported_formats

explosive_error = None
try:
    module.Explosive.__value__
except BaseException as error:
    explosive_error = type(error).__name__

payload = {
    "module_has_T": "T" in vars(module),
    "pair_type_name": type(module.Pair).__name__,
    "pair_type_module": type(module.Pair).__module__,
    "pair_type_params": [param.__name__ for param in module.Pair.__type_params__],
    "box_type_params": [param.__name__ for param in module.Box.__type_params__],
    "first_pair_type_params": [param.__name__ for param in module.first_pair.__type_params__],
    "pair_default_present": getattr(module.Pair.__type_params__[0], "__default__", None) is not None,
    "box_default_present": getattr(module.Box.__type_params__[0], "__default__", None) is not None,
    "function_default_present": getattr(module.first_pair.__type_params__[0], "__default__", None) is not None,
    "scope_alias_type_name": type(module.Scope.Alias).__name__,
    "scope_alias_resolves_nested": module.Scope.Alias.__value__ is module.Scope.Nested,
    "explosive_error": explosive_error,
}

if sys.version_info >= (3, 14):
    payload["annotationlib_box_has_value"] = "value" in get_annotations(
        module.Box, format=AnnotationFormat.VALUE
    )
    payload["annotationlib_module_has_pair"] = "PAIR" in get_annotations(
        module, format=AnnotationFormat.VALUE
    )
    payload["annotationlib_supports_string"] = supported_formats().string

print(json.dumps(payload))
"""
    rendered = capture([sys.executable, "-c", probe, str(build_root)])
    payload = json.loads(rendered)

    if payload["module_has_T"]:
        raise SystemExit("native runtime unexpectedly leaked a module-level `T` binding")
    if payload["pair_type_name"] != "TypeAliasType":
        raise SystemExit(
            "native runtime did not materialize a TypeAliasType for `Pair` "
            f"(got {payload['pair_type_module']}.{payload['pair_type_name']})"
        )
    if payload["scope_alias_type_name"] != "TypeAliasType":
        raise SystemExit(
            "class-scope native alias did not materialize as TypeAliasType "
            f"(got {payload['scope_alias_type_name']})"
        )
    if payload["pair_type_params"] != ["T"]:
        raise SystemExit(
            f"native runtime alias type parameters were not preserved: {payload['pair_type_params']}"
        )
    if payload["box_type_params"] != ["T"]:
        raise SystemExit(
            f"native runtime class type parameters were not preserved: {payload['box_type_params']}"
        )
    if payload["first_pair_type_params"] != ["T"]:
        raise SystemExit(
            "native runtime function type parameters were not preserved: "
            f"{payload['first_pair_type_params']}"
        )
    if not (
        payload["pair_default_present"]
        and payload["box_default_present"]
        and payload["function_default_present"]
    ):
        raise SystemExit(
            "native runtime did not preserve generic defaults on alias/class/function type parameters"
        )
    if not payload["scope_alias_resolves_nested"]:
        raise SystemExit(
            "class-scope native alias did not resolve names using annotation scope semantics"
        )
    if payload["explosive_error"] != "ZeroDivisionError":
        raise SystemExit(
            "native alias value was not lazily evaluated as expected "
            f"(got {payload['explosive_error']!r})"
        )
    if sys.version_info >= (3, 14):
        if not payload.get("annotationlib_box_has_value"):
            raise SystemExit("annotation compatibility layer did not expose Box annotations")
        if not payload.get("annotationlib_module_has_pair"):
            raise SystemExit(
                "annotation compatibility layer did not expose module-level annotations"
            )
        if not payload.get("annotationlib_supports_string"):
            raise SystemExit("annotation compatibility layer did not detect string-format support")


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
        assert_runtime_semantics(project_dir)
        run(
            [entrypoint, "verify", "--project", ".", "--unsafe-runtime-imports"],
            cwd=project_dir,
        )

    print(f"typepython native target smoke test passed for {args.target_python}")


if __name__ == "__main__":
    main()

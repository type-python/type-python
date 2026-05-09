from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "test-fixtures" / "downstream-checkers" / "flagship-core-package"
EXPECTED_STUB_FRAGMENTS = (
    "class PublicUser(TypedDict):",
    "class UserWithoutSecret(TypedDict):",
    "class ReadonlyUserRecord(TypedDict):",
    "class Renderable(Protocol):",
    "class LookupResult:",
    "class Found(LookupResult):",
    "class Missing(LookupResult):",
    "def clean_label(value: str) -> str: ...",
    "def apply_patch(record: UserRecord, patch: UserPatch) -> UserRecord: ...",
    "def describe_result(result: LookupResult) -> str: ...",
    "def render_all(items: list[Renderable]) -> list[str]: ...",
    "def freeze_record(record: UserRecord) -> ReadonlyUserRecord: ...",
)
FORBIDDEN_RUNTIME_FRAGMENTS = (
    "interface ",
    "sealed class",
    "typealias ",
)


def typepython_command() -> list[str]:
    return [sys.executable, "-m", "typepython"]


def run(command: list[str], cwd: pathlib.Path | None = None) -> None:
    location = f" (cwd={cwd})" if cwd is not None else ""
    print(f"+ {' '.join(command)}{location}")
    subprocess.run(command, cwd=cwd, check=True)


def assert_contains(path: pathlib.Path, fragments: tuple[str, ...]) -> None:
    text = path.read_text(encoding="utf-8")
    missing = [fragment for fragment in fragments if fragment not in text]
    if missing:
        joined = "; ".join(missing)
        raise SystemExit(f"{path} is missing expected fragment(s): {joined}")


def assert_not_contains(path: pathlib.Path, fragments: tuple[str, ...]) -> None:
    text = path.read_text(encoding="utf-8")
    leaked = [fragment for fragment in fragments if fragment in text]
    if leaked:
        joined = "; ".join(leaked)
        raise SystemExit(f"{path} contains forbidden TypePython-only fragment(s): {joined}")


def assert_flagship_outputs(project: pathlib.Path) -> None:
    build_root = project / ".typepython" / "build" / "app"
    runtime = build_root / "__init__.py"
    stub = build_root / "__init__.pyi"
    marker = build_root / "py.typed"
    for path in (runtime, stub, marker):
        if not path.is_file():
            raise SystemExit(f"missing flagship output artifact: {path}")
    assert_contains(stub, EXPECTED_STUB_FRAGMENTS)
    assert_not_contains(runtime, FORBIDDEN_RUNTIME_FRAGMENTS)


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="typepython-flagship-core-") as tmp:
        project = pathlib.Path(tmp) / "flagship-core-package"
        shutil.copytree(FIXTURE, project)
        command = typepython_command()
        run([*command, "check", "--project", str(project)])
        run([*command, "build", "--project", str(project)])
        run([*command, "verify", "--project", str(project)])
        assert_flagship_outputs(project)

    print("flagship core smoke test passed")


if __name__ == "__main__":
    main()

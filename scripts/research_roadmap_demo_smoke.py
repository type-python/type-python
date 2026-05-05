from __future__ import annotations

import json
import os
import pathlib
import subprocess


ROOT = pathlib.Path(__file__).resolve().parents[1]
PROJECT = ROOT / "examples" / "research-roadmap-demo"


def typepython_command() -> list[str]:
    if override := os.environ.get("TYPEPYTHON_BIN"):
        return [override]
    return ["cargo", "run", "-p", "typepython-cli", "--"]


def run(command: list[str]) -> None:
    print(f"+ {' '.join(command)}")
    subprocess.run(command, cwd=ROOT, check=True)


def assert_contains(path: pathlib.Path, needle: str) -> None:
    text = path.read_text(encoding="utf-8")
    if needle not in text:
        raise SystemExit(f"{path} does not contain expected text: {needle}")


def assert_not_contains(path: pathlib.Path, needle: str) -> None:
    text = path.read_text(encoding="utf-8")
    if needle in text:
        raise SystemExit(f"{path} leaked checker-only text: {needle}")


def assert_demo_outputs(project: pathlib.Path = PROJECT) -> None:
    runtime = project / ".typepython" / "build" / "app" / "__init__.py"
    stub = project / ".typepython" / "build" / "app" / "__init__.pyi"
    marker = project / ".typepython" / "build" / "app" / "py.typed"
    effects = project / ".typepython" / "cache" / "effects.json"

    for path in (runtime, stub, marker, effects):
        if not path.is_file():
            raise SystemExit(f"missing expected roadmap demo artifact: {path}")

    assert_contains(runtime, "class UserPatch(TypedDict):")
    assert_contains(runtime, "class PublicUser(TypedDict):")
    assert_contains(stub, "class UserPatch(TypedDict):")
    assert_contains(stub, "class PublicUser(TypedDict):")

    for checker_only in ("Tainted", "ValidatorWitness"):
        assert_not_contains(runtime, checker_only)
        assert_not_contains(stub, checker_only)

    payload = json.loads(effects.read_text(encoding="utf-8"))
    rendered = json.dumps(payload, sort_keys=True)
    for expected in ("load_user", "io.net", "handle", "taint.source", "taint.sanitize"):
        if expected not in rendered:
            raise SystemExit(f"effects sidecar is missing `{expected}`")


def main() -> None:
    base = typepython_command()
    run([*base, "check", "--project", str(PROJECT)])
    run([*base, "build", "--project", str(PROJECT)])
    assert_demo_outputs()
    print("research roadmap demo smoke test passed")


if __name__ == "__main__":
    main()

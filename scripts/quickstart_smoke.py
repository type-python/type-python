from __future__ import annotations

import hashlib
import importlib.metadata
import json
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


def assert_bundled_stdlib() -> None:
    distribution = importlib.metadata.distribution("type-python")
    root = pathlib.Path(distribution.locate_file("typepython/stdlib"))
    required = [
        root / "BASELINE.toml",
        root / "REFRESH_STATS.json",
        root / "VERSIONS",
        root / "builtins.pyi",
        root / "typing.pyi",
    ]
    missing = [path for path in required if not path.is_file()]
    if missing:
        formatted = ", ".join(str(path) for path in missing)
        raise SystemExit(f"installed wheel is missing bundled stdlib files: {formatted}")

    stats = json.loads((root / "REFRESH_STATS.json").read_text(encoding="utf-8"))
    digest = hashlib.sha256()
    files = [
        path
        for path in sorted(root.rglob("*"))
        if path.is_file()
        and path.name != ".DS_Store"
        and path.name not in {"BASELINE.toml", "REFRESH_STATS.json", "VERSIONS"}
    ]
    byte_count = 0
    for path in files:
        relative = path.relative_to(root).as_posix()
        contents = path.read_bytes()
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(contents)
        byte_count += len(contents)

    actual = (len(files), byte_count, digest.hexdigest())
    expected = (
        stats["stdlib_files"],
        stats["stdlib_bytes"],
        stats["stdlib_sha256"],
    )
    if actual != expected:
        raise SystemExit(
            "installed wheel bundled stdlib does not match REFRESH_STATS.json: "
            f"expected {expected}, got {actual}"
        )


def assert_cli_version(entrypoint: str) -> None:
    expected = importlib.metadata.version("type-python")
    result = subprocess.run(
        [entrypoint, "--version"],
        check=True,
        capture_output=True,
        text=True,
    )
    actual = result.stdout.strip()
    prefix = "typepython "
    actual_version = actual.removeprefix(prefix) if actual.startswith(prefix) else None
    if actual_version is None or version_identity(actual_version) != version_identity(expected):
        raise SystemExit(
            f"installed wheel CLI version mismatch: expected typepython {expected}, got {actual}"
        )


def version_identity(value: str) -> tuple[tuple[int, ...], str]:
    match = re.fullmatch(r"v?(\d+(?:\.\d+)*)(.*)", value.strip(), flags=re.IGNORECASE)
    if match is None:
        return ((), value.strip().lower())
    release = [int(part) for part in match.group(1).split(".")]
    while len(release) > 1 and release[-1] == 0:
        release.pop()
    suffix = re.sub(r"[-_.]", "", match.group(2).lower())
    for spelling, canonical in (
        ("preview", "rc"),
        ("alpha", "a"),
        ("beta", "b"),
        ("pre", "rc"),
    ):
        if suffix.startswith(spelling):
            suffix = canonical + suffix[len(spelling) :]
            break
    if suffix.isdigit():
        suffix = f"post{suffix}"
    return (tuple(release), suffix)


def main() -> None:
    assert_bundled_stdlib()
    entrypoint = resolve_entrypoint()
    assert_cli_version(entrypoint)
    run([entrypoint, "--help"])

    with tempfile.TemporaryDirectory(prefix="typepython-wheel-smoke-") as tmp:
        root = pathlib.Path(tmp)
        project_dir = root / "my-project"

        run([entrypoint, "init", "--dir", "my-project"], cwd=root)
        run([entrypoint, "check", "--project", "."], cwd=project_dir)
        run([entrypoint, "build", "--project", "."], cwd=project_dir)
        run([entrypoint, "verify", "--project", "."], cwd=project_dir)

        expected_files = [
            project_dir / ".typepython" / "build" / "app" / "__init__.py",
            project_dir / ".typepython" / "build" / "app" / "__init__.pyi",
            project_dir / ".typepython" / "build" / "app" / "py.typed",
        ]
        missing = [path for path in expected_files if not path.is_file()]
        if missing:
            formatted = ", ".join(str(path) for path in missing)
            raise SystemExit(f"missing expected build outputs: {formatted}")

    print("typepython wheel smoke test passed")


if __name__ == "__main__":
    main()

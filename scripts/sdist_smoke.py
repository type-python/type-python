from __future__ import annotations

import argparse
import os
import pathlib
import subprocess
import sys
import tarfile
import tempfile


ROOT = pathlib.Path(__file__).resolve().parent.parent

REQUIRED_SDIST_PATHS = (
    "Cargo.lock",
    "Cargo.toml",
    "pyproject.toml",
    "rust-toolchain.toml",
    "setup.py",
    "crates/typepython_cli/Cargo.toml",
    "crates/typepython_cli/src/main.rs",
    "stdlib/BASELINE.toml",
    "stdlib/REFRESH_STATS.json",
    "stdlib/VERSIONS",
    "stdlib/builtins.pyi",
    "stdlib/typing.pyi",
    "templates/src/app/__init__.tpy",
    "templates/typepython.toml",
    "typepython/__init__.py",
    "typepython/py.typed",
)


def run(command: list[str], *, cwd: pathlib.Path | None = None) -> None:
    location = f" (cwd={cwd})" if cwd is not None else ""
    print(f"+ {' '.join(command)}{location}")
    subprocess.run(command, cwd=cwd, check=True)


def extract_sdist(sdist: pathlib.Path, destination: pathlib.Path) -> pathlib.Path:
    if not sdist.is_file():
        raise SystemExit(f"source distribution does not exist: {sdist}")

    with tarfile.open(sdist, mode="r:gz") as archive:
        members = archive.getmembers()
        roots: set[str] = set()
        for member in members:
            path = pathlib.PurePosixPath(member.name)
            if path.is_absolute() or ".." in path.parts:
                raise SystemExit(f"unsafe source distribution path: {member.name}")
            if member.issym() or member.islnk() or member.isdev():
                raise SystemExit(f"unsupported source distribution entry: {member.name}")
            if path.parts:
                roots.add(path.parts[0])

        if len(roots) != 1:
            rendered = ", ".join(sorted(roots)) or "<none>"
            raise SystemExit(
                "source distribution must contain exactly one top-level directory; "
                f"found: {rendered}"
            )
        archive.extractall(destination)

    return destination / next(iter(roots))


def missing_required_paths(source_root: pathlib.Path) -> list[str]:
    return [
        relative
        for relative in REQUIRED_SDIST_PATHS
        if not source_root.joinpath(relative).is_file()
    ]


def validate_sdist_contents(source_root: pathlib.Path) -> None:
    missing = missing_required_paths(source_root)
    if missing:
        raise SystemExit(
            "source distribution is missing required build/runtime files: " + ", ".join(missing)
        )


def venv_python(venv: pathlib.Path) -> pathlib.Path:
    return venv / ("Scripts/python.exe" if os.name == "nt" else "bin/python")


def smoke_sdist(sdist: pathlib.Path) -> None:
    with tempfile.TemporaryDirectory(prefix="typepython-sdist-smoke-") as tmp:
        root = pathlib.Path(tmp)
        source_root = extract_sdist(sdist.resolve(), root / "source")
        validate_sdist_contents(source_root)

        wheelhouse = root / "wheelhouse"
        run(
            [
                sys.executable,
                "-m",
                "build",
                "--wheel",
                "--outdir",
                str(wheelhouse),
                str(source_root),
            ]
        )
        wheels = sorted(wheelhouse.glob("*.whl"))
        if len(wheels) != 1:
            raise SystemExit(
                "source distribution build must produce exactly one wheel; "
                f"found {len(wheels)}"
            )

        venv = root / "venv"
        run([sys.executable, "-m", "venv", str(venv)])
        python = venv_python(venv)
        run([str(python), "-m", "pip", "install", "--no-deps", "--force-reinstall", str(wheels[0])])
        run([str(python), str(ROOT / "scripts" / "quickstart_smoke.py")])


def main() -> None:
    parser = argparse.ArgumentParser(
        description=(
            "Validate a TypePython source distribution by rebuilding its wheel from the unpacked "
            "archive, installing it in an isolated environment, and running the wheel "
            "quickstart smoke."
        )
    )
    parser.add_argument("sdist", type=pathlib.Path)
    args = parser.parse_args()
    smoke_sdist(args.sdist)
    print("typepython source distribution smoke test passed")


if __name__ == "__main__":
    main()

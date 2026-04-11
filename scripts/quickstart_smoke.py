from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys
import tempfile


def run(command: list[str], cwd: pathlib.Path | None = None) -> None:
    location = f" (cwd={cwd})" if cwd is not None else ""
    print(f"+ {' '.join(command)}{location}")
    subprocess.run(command, cwd=cwd, check=True)


def resolve_entrypoint() -> str:
    entrypoint = shutil.which("typepython")
    if entrypoint is not None:
        return entrypoint

    scripts_dir = pathlib.Path(sys.executable).parent
    for candidate in ("typepython", "typepython.exe"):
        path = scripts_dir / candidate
        if path.is_file():
            return str(path)

    raise SystemExit("typepython executable was not installed into PATH or the active Python scripts directory")


def main() -> None:
    entrypoint = resolve_entrypoint()
    run([entrypoint, "--help"])

    with tempfile.TemporaryDirectory(prefix="typepython-wheel-smoke-") as tmp:
        root = pathlib.Path(tmp)
        project_dir = root / "my-project"

        run([sys.executable, "-m", "typepython", "init", "--dir", "my-project"], cwd=root)
        run([sys.executable, "-m", "typepython", "check", "--project", "."], cwd=project_dir)
        run([sys.executable, "-m", "typepython", "build", "--project", "."], cwd=project_dir)

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

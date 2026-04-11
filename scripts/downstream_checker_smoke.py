from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parent.parent
FIXTURE_ROOT = ROOT / "test-fixtures" / "downstream-checkers"


def run(command: list[str], cwd: pathlib.Path | None = None) -> None:
    location = f" (cwd={cwd})" if cwd is not None else ""
    print(f"+ {' '.join(command)}{location}")
    subprocess.run(command, cwd=cwd, check=True)


def require_command(name: str) -> str:
    resolved = shutil.which(name)
    if resolved is None:
        raise SystemExit(f"required command `{name}` was not found in PATH")
    return resolved


def check_fixture(fixture_name: str) -> None:
    source_dir = FIXTURE_ROOT / fixture_name
    if not source_dir.is_dir():
        raise SystemExit(f"missing checker smoke fixture: {source_dir}")

    with tempfile.TemporaryDirectory(prefix=f"typepython-checker-smoke-{fixture_name}-") as tmp:
        project_dir = pathlib.Path(tmp) / fixture_name
        shutil.copytree(source_dir, project_dir)

        run([sys.executable, "-m", "typepython", "build", "--project", str(project_dir)])

        build_dir = project_dir / ".typepython" / "build"
        run(["mypy", "--python-version", "3.10", str(build_dir)])
        run(["pyright", "--pythonversion", "3.10", str(build_dir)])


def main() -> None:
    require_command("mypy")
    require_command("pyright")

    for fixture_name in ("basic-package", "rich-package"):
        check_fixture(fixture_name)

    print("downstream checker smoke test passed")


if __name__ == "__main__":
    main()

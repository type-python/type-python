from __future__ import annotations

import dataclasses
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parent.parent
FIXTURE_ROOT = ROOT / "test-fixtures" / "downstream-checkers"
DEFAULT_CHECKERS = ("mypy", "pyright", "ty")


@dataclasses.dataclass(frozen=True)
class FixtureCase:
    name: str
    targets: tuple[str, ...]
    expected_stub_fragments: dict[str, tuple[str, ...]] | None = None


FIXTURES = {
    "basic-package": FixtureCase(name="basic-package", targets=("3.10",)),
    "rich-package": FixtureCase(name="rich-package", targets=("3.10", "3.12")),
    "compat-package": FixtureCase(
        name="compat-package",
        targets=("3.10", "3.11", "3.12"),
        expected_stub_fragments={
            "3.10": (
                "typing_extensions.Self",
                "@typing_extensions.override",
                "typing_extensions.ReadOnly[int]",
                "typing_extensions.TypeIs[int]",
            ),
            "3.11": (
                "typing.Self",
                "@typing_extensions.override",
                "typing_extensions.ReadOnly[int]",
                "typing_extensions.TypeIs[int]",
            ),
            "3.12": (
                "typing.Self",
                "@typing.override",
                "typing_extensions.ReadOnly[int]",
                "typing_extensions.TypeIs[int]",
            ),
        },
    ),
}


def run(command: list[str], cwd: pathlib.Path | None = None) -> None:
    location = f" (cwd={cwd})" if cwd is not None else ""
    print(f"+ {' '.join(command)}{location}")
    subprocess.run(command, cwd=cwd, check=True)


def require_command(name: str) -> str:
    resolved = shutil.which(name)
    if resolved is None:
        raise SystemExit(f"required command `{name}` was not found in PATH")
    return resolved


def env_csv(name: str, default: tuple[str, ...]) -> tuple[str, ...]:
    raw = os.environ.get(name)
    if raw is None:
        return default
    values = tuple(value.strip() for value in raw.split(",") if value.strip())
    if not values:
        raise SystemExit(f"{name} must name at least one entry when provided")
    return values


def rewrite_target_python(config_path: pathlib.Path, target: str) -> None:
    rendered = config_path.read_text(encoding="utf-8")
    rewritten, replacements = re.subn(
        r'(?m)^target_python = "[^"]+"$',
        f'target_python = "{target}"',
        rendered,
        count=1,
    )
    if replacements != 1:
        raise SystemExit(f"unable to rewrite target_python in {config_path}")
    config_path.write_text(rewritten, encoding="utf-8")


def assert_expected_stub_fragments(
    build_dir: pathlib.Path,
    target: str,
    expected_stub_fragments: dict[str, tuple[str, ...]],
) -> None:
    stub_path = build_dir / "app" / "__init__.pyi"
    rendered = stub_path.read_text(encoding="utf-8")
    missing = [fragment for fragment in expected_stub_fragments[target] if fragment not in rendered]
    if missing:
        joined = "; ".join(missing)
        raise SystemExit(
            f"compat stub check failed for target {target} in {stub_path}: missing {joined}"
        )


def checker_command(
    checker: str,
    target: str,
    build_dir: pathlib.Path,
) -> list[str]:
    if checker == "mypy":
        return ["mypy", "--python-version", target, str(build_dir)]
    if checker == "pyright":
        return ["pyright", "--pythonversion", target, str(build_dir)]
    if checker == "ty":
        command = ["ty", "check", "--no-progress"]
        python_override = os.environ.get("TYPEPYTHON_DOWNSTREAM_TY_PYTHON")
        if python_override:
            command.extend(["--python", python_override])
        command.extend(["--python-version", target, str(build_dir)])
        return command
    raise SystemExit(f"unsupported downstream checker `{checker}`")


def check_fixture(case: FixtureCase, checkers: tuple[str, ...]) -> None:
    source_dir = FIXTURE_ROOT / case.name
    if not source_dir.is_dir():
        raise SystemExit(f"missing checker smoke fixture: {source_dir}")

    for target in case.targets:
        with tempfile.TemporaryDirectory(
            prefix=f"typepython-checker-smoke-{case.name}-{target}-"
        ) as tmp:
            project_dir = pathlib.Path(tmp) / case.name
            shutil.copytree(source_dir, project_dir)
            rewrite_target_python(project_dir / "typepython.toml", target)

            run([sys.executable, "-m", "typepython", "build", "--project", str(project_dir)])

            build_dir = project_dir / ".typepython" / "build"
            if case.expected_stub_fragments is not None:
                assert_expected_stub_fragments(build_dir, target, case.expected_stub_fragments)

            for checker in checkers:
                run(checker_command(checker, target, build_dir), cwd=project_dir)


def main() -> None:
    checker_names = env_csv("TYPEPYTHON_DOWNSTREAM_CHECKERS", DEFAULT_CHECKERS)
    fixture_names = env_csv("TYPEPYTHON_DOWNSTREAM_FIXTURES", tuple(FIXTURES))
    for checker in checker_names:
        require_command(checker)
    for fixture_name in fixture_names:
        case = FIXTURES.get(fixture_name)
        if case is None:
            known = ", ".join(sorted(FIXTURES))
            raise SystemExit(
                f"unknown downstream checker fixture `{fixture_name}`; known fixtures: {known}"
            )
        check_fixture(case, checker_names)

    print("downstream checker smoke test passed")


if __name__ == "__main__":
    main()

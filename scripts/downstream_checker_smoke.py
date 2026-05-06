from __future__ import annotations

import dataclasses
import datetime as dt
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parent.parent
FIXTURE_ROOT = ROOT / "test-fixtures" / "downstream-checkers"
MATRIX_PATH = FIXTURE_ROOT / "matrix.json"
DEFAULT_CHECKERS = ("mypy", "pyright", "basedpyright", "ty")
DEFAULT_PROFILES = ("strict",)
DEFAULT_STUB_FRAGMENT_PATH = "app/__init__.pyi"


@dataclasses.dataclass(frozen=True)
class FixtureCase:
    name: str
    targets: tuple[str, ...]
    profiles: tuple[str, ...] = DEFAULT_PROFILES
    expect_checker_failure: bool = False
    expected_checker_failures: tuple[str, ...] = ()
    expected_failure_patterns: dict[str, tuple[str, ...]] | None = None
    allowlist_reason: str | None = None
    allowlist_expires: dt.date | None = None
    expected_stub_fragments: dict[str, dict[str, tuple[str, ...]]] | None = None


def parse_allowlist_expiry(name: str, raw_value: object) -> dt.date | None:
    if raw_value is None:
        return None
    if not isinstance(raw_value, str):
        raise SystemExit(f"fixture `{name}` allowlist_expires must be a YYYY-MM-DD string")
    try:
        return dt.date.fromisoformat(raw_value)
    except ValueError as exc:
        raise SystemExit(
            f"fixture `{name}` allowlist_expires must be a valid YYYY-MM-DD date"
        ) from exc


def validate_expected_failure_allowlist(
    name: str,
    expected_failures: tuple[str, ...],
    allowlist_reason: object,
    allowlist_expires: dt.date | None,
) -> str | None:
    if not expected_failures:
        return None
    if not isinstance(allowlist_reason, str) or not allowlist_reason.strip():
        raise SystemExit(f"fixture `{name}` expected failures require allowlist_reason")
    if allowlist_expires is None:
        raise SystemExit(f"fixture `{name}` expected failures require allowlist_expires")
    today = dt.date.today()
    if allowlist_expires < today:
        raise SystemExit(
            f"fixture `{name}` checker allowlist expired on {allowlist_expires.isoformat()}"
        )
    return allowlist_reason


def load_fixture_matrix(path: pathlib.Path = MATRIX_PATH) -> dict[str, FixtureCase]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    fixtures: dict[str, FixtureCase] = {}
    for raw_case in payload.get("fixtures", []):
        name = raw_case["name"]
        targets = tuple(raw_case["targets"])
        profiles = tuple(raw_case.get("profiles", DEFAULT_PROFILES))
        expected_stub_fragments = raw_case.get("expected_stub_fragments")
        expect_checker_failure = bool(raw_case.get("expect_checker_failure", False))
        expected_checker_failures = tuple(raw_case.get("expected_checker_failures", ()))
        expected_failure_patterns = raw_case.get("expected_failure_patterns")
        allowlist_expires = parse_allowlist_expiry(name, raw_case.get("allowlist_expires"))
        if expect_checker_failure and expected_checker_failures:
            raise SystemExit(
                f"fixture `{name}` cannot set both expect_checker_failure and expected_checker_failures"
            )
        if not targets:
            raise SystemExit(f"fixture `{name}` must declare at least one target")
        if not profiles:
            raise SystemExit(f"fixture `{name}` must declare at least one profile")
        expected_failure_ids = ("*",) if expect_checker_failure else expected_checker_failures
        allowlist_reason = validate_expected_failure_allowlist(
            name,
            expected_failure_ids,
            raw_case.get("allowlist_reason"),
            allowlist_expires,
        )
        fixtures[name] = FixtureCase(
            name=name,
            targets=targets,
            profiles=profiles,
            expect_checker_failure=expect_checker_failure,
            expected_checker_failures=expected_checker_failures,
            expected_failure_patterns=(
                None
                if expected_failure_patterns is None
                else {
                    key: tuple(patterns)
                    for key, patterns in expected_failure_patterns.items()
                }
            ),
            allowlist_reason=allowlist_reason,
            allowlist_expires=allowlist_expires,
            expected_stub_fragments=(
                normalize_expected_stub_fragments(name, expected_stub_fragments)
            ),
        )
    if not fixtures:
        raise SystemExit(f"downstream checker matrix is empty: {path}")
    return fixtures


def normalize_expected_stub_fragments(
    name: str,
    raw_fragments: object,
) -> dict[str, dict[str, tuple[str, ...]]] | None:
    if raw_fragments is None:
        return None
    if not isinstance(raw_fragments, dict):
        raise SystemExit(f"fixture `{name}` expected_stub_fragments must be an object")
    normalized: dict[str, dict[str, tuple[str, ...]]] = {}
    for target, target_fragments in raw_fragments.items():
        if not isinstance(target, str):
            raise SystemExit(f"fixture `{name}` expected_stub_fragments target must be a string")
        if isinstance(target_fragments, list):
            normalized[target] = {DEFAULT_STUB_FRAGMENT_PATH: tuple(target_fragments)}
            continue
        if not isinstance(target_fragments, dict):
            raise SystemExit(
                f"fixture `{name}` expected_stub_fragments for {target} must be a list or object"
            )
        normalized[target] = {
            path: tuple(fragments)
            for path, fragments in target_fragments.items()
        }
    return normalized


def run(command: list[str], cwd: pathlib.Path | None = None) -> None:
    location = f" (cwd={cwd})" if cwd is not None else ""
    print(f"+ {' '.join(command)}{location}")
    subprocess.run(command, cwd=cwd, check=True, env=command_env(command, cwd))


def run_expect_failure(
    command: list[str],
    cwd: pathlib.Path | None = None,
    expected_patterns: tuple[str, ...] = (),
) -> None:
    location = f" (cwd={cwd})" if cwd is not None else ""
    print(f"+ {' '.join(command)} # expected failure{location}")
    completed = subprocess.run(
        command,
        cwd=cwd,
        check=False,
        capture_output=True,
        text=True,
        env=command_env(command, cwd),
    )
    if completed.stdout:
        print(completed.stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="", file=sys.stderr)
    if completed.returncode == 0:
        raise SystemExit(
            f"expected downstream checker command to fail, but it succeeded: {' '.join(command)}"
        )
    rendered = f"{completed.stdout}\n{completed.stderr}"
    for pattern in expected_patterns:
        if re.search(pattern, rendered, flags=re.MULTILINE) is None:
            raise SystemExit(
                f"expected downstream checker failure to match /{pattern}/ for: {' '.join(command)}"
            )


def require_command(name: str) -> str:
    resolved = shutil.which(name)
    if resolved is None:
        raise SystemExit(f"required command `{name}` was not found in PATH")
    return resolved


def command_env(
    command: list[str],
    cwd: pathlib.Path | None = None,
) -> dict[str, str] | None:
    if not command or pathlib.Path(command[0]).name != "mypy":
        return None
    env = os.environ.copy()
    search_paths = [command[-1]]
    if cwd is not None:
        search_paths.extend(str(cwd / path) for path in checker_support_paths(cwd))
    existing = env.get("MYPYPATH")
    rendered = os.pathsep.join(search_paths)
    env["MYPYPATH"] = rendered if not existing else f"{rendered}{os.pathsep}{existing}"
    return env


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
    expected_stub_fragments: dict[str, dict[str, tuple[str, ...]]],
) -> None:
    for relative_stub_path, fragments in expected_stub_fragments[target].items():
        stub_path = build_dir / relative_stub_path
        rendered = stub_path.read_text(encoding="utf-8")
        missing = [fragment for fragment in fragments if fragment not in rendered]
        if missing:
            joined = "; ".join(missing)
            raise SystemExit(
                f"compat stub check failed for target {target} in {stub_path}: missing {joined}"
            )


def prepare_checker_build_dir(build_dir: pathlib.Path, project_dir: pathlib.Path) -> pathlib.Path:
    checker_build_dir = project_dir / "checker-build"
    if checker_build_dir.exists():
        shutil.rmtree(checker_build_dir)
    shutil.copytree(build_dir, checker_build_dir)
    return checker_build_dir


def checker_support_paths(project_dir: pathlib.Path) -> tuple[pathlib.Path, ...]:
    support_dir = project_dir / "checker-support"
    if not support_dir.exists():
        return ()
    if not support_dir.is_dir():
        raise SystemExit(f"checker support path is not a directory: {support_dir}")
    support_paths = [
        path.relative_to(project_dir)
        for path in sorted(support_dir.iterdir())
        if path.is_dir() and (path.name == "typings" or path.name.endswith("-stubs"))
    ]
    support_paths.append(support_dir.relative_to(project_dir))
    return tuple(support_paths)


def checker_command(
    checker: str,
    profile: str,
    target: str,
    build_dir: pathlib.Path,
) -> list[str]:
    if checker == "mypy":
        command = [
            "mypy",
            "--python-version",
            target,
            "--namespace-packages",
            "--explicit-package-bases",
        ]
        if profile == "strict":
            command.append("--strict")
        elif profile != "standard":
            raise SystemExit(f"unsupported mypy profile `{profile}`")
        command.append(str(build_dir))
        return command
    if checker in {"pyright", "basedpyright"}:
        if profile not in {"standard", "strict"}:
            raise SystemExit(f"unsupported {checker} profile `{profile}`")
        command = [checker, "--pythonversion", target, str(build_dir)]
        if checker == "pyright":
            command[1:1] = ["--level", "error"]
        return command
    if checker == "ty":
        command = ["ty", "check", "--no-progress"]
        if profile != "strict":
            raise SystemExit(f"unsupported ty profile `{profile}`")
        python_override = os.environ.get("TYPEPYTHON_DOWNSTREAM_TY_PYTHON")
        if python_override:
            command.extend(["--python", python_override])
        command.extend(["--python-version", target, str(build_dir)])
        return command
    raise SystemExit(f"unsupported downstream checker `{checker}`")


def write_pyright_config(project_dir: pathlib.Path, build_dir: pathlib.Path, profile: str) -> None:
    type_checking_mode = "strict" if profile == "strict" else "standard"
    extra_paths = [build_dir.name]
    extra_paths.extend(
        str(path)
        for path in checker_support_paths(project_dir)
        if not path.name.endswith("-stubs")
    )
    config = {
        "extraPaths": extra_paths,
        "include": [build_dir.name],
        "typeCheckingMode": type_checking_mode,
        "reportUnusedImport": "none",
    }
    if profile == "standard":
        config.update(
            {
                "reportMissingTypeStubs": "none",
                "reportUnknownVariableType": "none",
                "reportUnknownMemberType": "none",
                "reportUnknownArgumentType": "none",
                "reportUnknownParameterType": "none",
                "reportUnnecessaryCast": "none",
                "reportUnusedImport": "none",
            }
        )
    stub_path = project_dir / "checker-support" / "typings"
    if stub_path.is_dir():
        config["stubPath"] = str(stub_path.relative_to(project_dir))
    (project_dir / "pyrightconfig.json").write_text(
        json.dumps(config, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def expected_failure_patterns_for(
    case: FixtureCase,
    checker: str,
    profile: str,
) -> tuple[str, ...]:
    if case.expected_failure_patterns is None:
        return ()
    invocation_id = f"{checker}:{profile}"
    patterns = (
        case.expected_failure_patterns.get(invocation_id)
        or case.expected_failure_patterns.get(checker)
        or case.expected_failure_patterns.get("*")
        or ()
    )
    return tuple(patterns)


def checker_failure_expected(
    case: FixtureCase,
    checker: str,
    profile: str,
    active_checkers: tuple[str, ...],
) -> bool:
    if case.expect_checker_failure:
        return checker in active_checkers
    expected = set(case.expected_checker_failures)
    return checker in expected or f"{checker}:{profile}" in expected


def sync_checker_consumer(
    consumer_path: pathlib.Path,
    build_consumer_path: pathlib.Path,
) -> None:
    if consumer_path.exists():
        shutil.copy2(consumer_path, build_consumer_path)
    elif build_consumer_path.exists():
        build_consumer_path.unlink()


def check_fixture(case: FixtureCase, checkers: tuple[str, ...]) -> None:
    source_dir = FIXTURE_ROOT / case.name
    if not source_dir.is_dir():
        raise SystemExit(f"missing checker smoke fixture: {source_dir}")

    for target in case.targets:
        for profile in case.profiles:
            with tempfile.TemporaryDirectory(
                prefix=f"typepython-checker-smoke-{case.name}-{target}-{profile}-"
            ) as tmp:
                project_dir = pathlib.Path(tmp) / case.name
                shutil.copytree(source_dir, project_dir)
                rewrite_target_python(project_dir / "typepython.toml", target)

                run([sys.executable, "-m", "typepython", "build", "--project", str(project_dir)])

                build_dir = project_dir / ".typepython" / "build"
                consumer_path = source_dir / "checker-consumer.py"
                checker_build_dir = prepare_checker_build_dir(build_dir, project_dir)
                build_consumer_path = checker_build_dir / "checker_consumer.py"
                has_expected_failure = (
                    case.expect_checker_failure or bool(case.expected_checker_failures)
                )
                if has_expected_failure and not consumer_path.exists():
                    raise SystemExit(
                        f"negative downstream checker fixture `{case.name}` is missing {consumer_path}"
                    )
                if case.expected_stub_fragments is not None:
                    assert_expected_stub_fragments(build_dir, target, case.expected_stub_fragments)

                for checker in checkers:
                    if checker in {"pyright", "basedpyright"}:
                        write_pyright_config(project_dir, checker_build_dir, profile)
                    command = checker_command(checker, profile, target, checker_build_dir)
                    sync_checker_consumer(consumer_path, build_consumer_path)
                    if checker_failure_expected(case, checker, profile, checkers):
                        run_expect_failure(
                            command,
                            cwd=project_dir,
                            expected_patterns=expected_failure_patterns_for(case, checker, profile),
                        )
                    else:
                        run(command, cwd=project_dir)


def main() -> None:
    fixtures = load_fixture_matrix()
    checker_names = env_csv("TYPEPYTHON_DOWNSTREAM_CHECKERS", DEFAULT_CHECKERS)
    requested_profiles = env_csv("TYPEPYTHON_DOWNSTREAM_PROFILES", ())
    fixture_names = env_csv("TYPEPYTHON_DOWNSTREAM_FIXTURES", tuple(fixtures))
    for checker in checker_names:
        require_command(checker)
    for fixture_name in fixture_names:
        case = fixtures.get(fixture_name)
        if case is None:
            known = ", ".join(sorted(fixtures))
            raise SystemExit(
                f"unknown downstream checker fixture `{fixture_name}`; known fixtures: {known}"
            )
        if requested_profiles:
            case = dataclasses.replace(case, profiles=requested_profiles)
        check_fixture(case, checker_names)

    print("downstream checker smoke test passed")


if __name__ == "__main__":
    main()

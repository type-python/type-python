from __future__ import annotations

import argparse
import dataclasses
import datetime as dt
import json
import os
import pathlib
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any


ROOT = pathlib.Path(__file__).resolve().parent.parent


@dataclasses.dataclass(frozen=True)
class WorkspaceOptions:
    modules: int
    external_stubs: int
    target_python: str


@dataclasses.dataclass(frozen=True)
class TimedStep:
    label: str
    command: list[str]
    seconds: float
    return_code: int
    peak_rss_bytes: int | None
    stdout_bytes: int
    stderr_bytes: int


def toml_string(value: str | pathlib.Path) -> str:
    return json.dumps(str(value))


def create_workspace(root: pathlib.Path, options: WorkspaceOptions) -> pathlib.Path:
    if options.modules < 1:
        raise ValueError("--modules must be at least 1")
    if options.external_stubs < 0:
        raise ValueError("--external-stubs must not be negative")

    project = root / "industrial-workspace"
    src = project / "src" / "app"
    typestubs = project / "typestubs"
    src.mkdir(parents=True, exist_ok=True)
    typestubs.mkdir(parents=True, exist_ok=True)

    write_probe(project / "bin" / "python-probe", options.target_python)
    write_config(project, options)
    write_application_modules(src, options.modules)
    write_external_stubs(typestubs, options.external_stubs)
    return project


def write_probe(path: pathlib.Path, target_python: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "\n".join(
            [
                "#!/usr/bin/env python3",
                "from __future__ import annotations",
                "import json",
                "import sys",
                "script = sys.argv[2] if len(sys.argv) > 2 else ''",
                "if 'version_info' in script:",
                f"    print({target_python!r})",
                "else:",
                "    print(json.dumps([]))",
                "",
            ]
        ),
        encoding="utf-8",
    )
    try:
        path.chmod(path.stat().st_mode | 0o755)
    except OSError:
        pass


def write_config(project: pathlib.Path, options: WorkspaceOptions) -> None:
    probe = project / "bin" / "python-probe"
    project.joinpath("typepython.toml").write_text(
        "\n".join(
            [
                "[project]",
                'src = ["src"]',
                'include = ["src/**/*.tpy"]',
                'exclude = [".typepython/**"]',
                'root_dir = "src"',
                'out_dir = ".typepython/build"',
                'cache_dir = ".typepython/cache"',
                f"target_python = {toml_string(options.target_python)}",
                "",
                "[resolution]",
                'type_roots = ["typestubs"]',
                f"python_executable = {toml_string(probe)}",
                "",
                "[emit]",
                "emit_pyi = true",
                "emit_pyc = false",
                "write_py_typed = true",
                "no_emit_on_error = true",
                "",
                "[typing]",
                'profile = "application"',
                "strict = true",
                "strict_nulls = true",
                'imports = "unknown"',
                "require_known_public_types = false",
                "",
                "[watch]",
                "debounce_ms = 80",
                "",
            ]
        ),
        encoding="utf-8",
    )


def write_application_modules(src: pathlib.Path, module_count: int) -> None:
    src.joinpath("__init__.tpy").write_text("pass\n", encoding="utf-8")
    for index in range(module_count):
        path = src / f"mod_{index:04}.tpy"
        imports: list[str] = []
        if index > 0:
            imports.append(
                f"from app.mod_{index - 1:04} import value_{index - 1:04}"
            )
        imported_value = "0" if index == 0 else f"value_{index - 1:04}()"
        path.write_text(
            "\n".join(
                [
                    *imports,
                    "",
                    f"ALIAS_{index:04}: int = {index}",
                    "",
                    f"def value_{index:04}() -> int:",
                    f"    return {imported_value}",
                    "",
                    f"def consume_{index:04}(values: list[int]) -> int:",
                    "    return value_{:04}()".format(index),
                    "",
                ]
            ),
            encoding="utf-8",
        )


def write_external_stubs(root: pathlib.Path, stub_count: int) -> None:
    namespace_root = root / "namespace_pkg"
    for index in range(stub_count):
        package = root / f"vendor_{index:04}"
        package.mkdir(parents=True, exist_ok=True)
        package.joinpath("__init__.pyi").write_text(
            "\n".join(
                [
                    f"class Record{index:04}:",
                    "    id: int",
                    "    name: str",
                    "",
                    f"def load_{index:04}() -> Record{index:04}: ...",
                    "",
                ]
            ),
            encoding="utf-8",
        )

        service = namespace_root / f"service_{index:04}"
        service.mkdir(parents=True, exist_ok=True)
        service.joinpath("__init__.pyi").write_text(
            f"def call_{index:04}(payload: bytes) -> bytes: ...\n",
            encoding="utf-8",
        )

    partial_stub_root = root / "framework-stubs"
    partial_stub_root.mkdir(parents=True, exist_ok=True)
    partial_stub_root.joinpath("py.typed").write_text("partial\n", encoding="utf-8")
    partial_package = partial_stub_root / "framework"
    partial_package.mkdir(parents=True, exist_ok=True)
    partial_package.joinpath("__init__.pyi").write_text(
        "def configured() -> bool: ...\n",
        encoding="utf-8",
    )

    runtime_package = root / "framework"
    runtime_package.mkdir(parents=True, exist_ok=True)
    runtime_package.joinpath("extra.py").write_text(
        "def runtime_only():\n    return True\n",
        encoding="utf-8",
    )


def implementation_edit(project: pathlib.Path, module_count: int) -> pathlib.Path:
    path = project / "src" / "app" / f"mod_{module_count - 1:04}.tpy"
    with path.open("a", encoding="utf-8") as handle:
        handle.write(f"\n_private_marker_{module_count - 1:04}: int = {module_count}\n")
    return path


def public_surface_edit(project: pathlib.Path) -> pathlib.Path:
    path = project / "src" / "app" / "mod_0000.tpy"
    with path.open("a", encoding="utf-8") as handle:
        handle.write("\ndef added_public_surface() -> int:\n    return value_0000()\n")
    return path


def resolve_typepython_command() -> list[str]:
    raw_command = os.environ.get("TYPEPYTHON_CMD")
    if raw_command:
        return shlex.split(raw_command)

    raw_binary = os.environ.get("TYPEPYTHON_BIN")
    if raw_binary:
        return [raw_binary]

    debug_binary = ROOT / "target" / "debug" / ("typepython.exe" if os.name == "nt" else "typepython")
    if debug_binary.is_file():
        return [str(debug_binary)]

    path_binary = shutil.which("typepython")
    if path_binary is not None:
        return [path_binary]

    return ["cargo", "run", "-q", "-p", "typepython-cli", "--"]


def typepython_invocation(base_command: list[str], command: str, project: pathlib.Path) -> list[str]:
    return [*base_command, command, "--project", str(project), "--format", "json"]


def run_timed(label: str, command: list[str], cwd: pathlib.Path) -> TimedStep:
    timed_command = platform_time_command(command)
    started = time.perf_counter()
    completed = subprocess.run(
        timed_command,
        cwd=cwd,
        check=False,
        capture_output=True,
        text=False,
    )
    elapsed = time.perf_counter() - started
    peak_rss = parse_peak_rss(timed_command, completed.stderr)

    stderr = strip_time_output(timed_command, completed.stderr)
    if completed.returncode != 0:
        sys.stdout.buffer.write(completed.stdout)
        sys.stderr.buffer.write(stderr)
        raise SystemExit(
            f"{label} failed with exit code {completed.returncode}: {' '.join(command)}"
        )

    return TimedStep(
        label=label,
        command=command,
        seconds=elapsed,
        return_code=completed.returncode,
        peak_rss_bytes=peak_rss,
        stdout_bytes=len(completed.stdout),
        stderr_bytes=len(stderr),
    )


def platform_time_command(command: list[str]) -> list[str]:
    if not pathlib.Path("/usr/bin/time").is_file():
        return command
    if sys.platform == "darwin":
        return ["/usr/bin/time", "-l", *command]
    if os.name == "posix":
        return ["/usr/bin/time", "-v", *command]
    return command


def parse_peak_rss(timed_command: list[str], stderr: bytes) -> int | None:
    if not timed_command[:1] == ["/usr/bin/time"]:
        return None
    rendered = stderr.decode("utf-8", errors="replace")
    if len(timed_command) > 1 and timed_command[1] == "-l":
        for line in rendered.splitlines():
            stripped = line.strip()
            if stripped.endswith("maximum resident set size"):
                value = stripped.split()[0]
                return int(value)
        return None

    marker = "Maximum resident set size (kbytes):"
    for line in rendered.splitlines():
        if marker in line:
            value = line.split(marker, 1)[1].strip()
            return int(value) * 1024
    return None


def strip_time_output(timed_command: list[str], stderr: bytes) -> bytes:
    if not timed_command[:1] == ["/usr/bin/time"]:
        return stderr
    rendered = stderr.decode("utf-8", errors="replace")
    kept: list[str] = []
    for line in rendered.splitlines():
        stripped = line.strip()
        if "maximum resident set size" in line:
            continue
        if "Maximum resident set size (kbytes):" in line:
            continue
        if sys.platform == "darwin" and is_macos_time_output_line(stripped):
            continue
        if line.startswith("\t") and os.name == "posix":
            continue
        kept.append(line)
    if not kept:
        return b""
    return ("\n".join(kept) + "\n").encode("utf-8")


def is_macos_time_output_line(stripped: str) -> bool:
    return any(
        stripped.endswith(suffix)
        for suffix in (
            "real",
            "user",
            "sys",
            "average shared memory size",
            "average unshared data size",
            "average unshared stack size",
            "page reclaims",
            "page faults",
            "swaps",
            "block input operations",
            "block output operations",
            "messages sent",
            "messages received",
            "signals received",
            "voluntary context switches",
            "involuntary context switches",
            "instructions retired",
            "cycles elapsed",
            "peak memory footprint",
        )
    )


def run_smoke(project: pathlib.Path, options: WorkspaceOptions, command: str) -> list[TimedStep]:
    base_command = resolve_typepython_command()
    steps = [
        ("cold_check", typepython_invocation(base_command, command, project)),
        ("warm_check", typepython_invocation(base_command, command, project)),
    ]
    implementation_edit(project, options.modules)
    steps.append(
        (
            "single_file_implementation_edit",
            typepython_invocation(base_command, command, project),
        )
    )
    public_surface_edit(project)
    steps.append(
        (
            "public_surface_edit",
            typepython_invocation(base_command, command, project),
        )
    )
    return [run_timed(label, invocation, ROOT) for label, invocation in steps]


def render_summary(payload: dict[str, Any]) -> None:
    print(f"workspace: {payload['workspace']}")
    print(
        "fixture: "
        f"{payload['modules']} modules, "
        f"{payload['external_stubs']} external stub packages, "
        f"Python {payload['target_python']}"
    )
    for step in payload["steps"]:
        rss = step["peak_rss_bytes"]
        rss_text = "n/a" if rss is None else f"{rss / 1024 / 1024:.1f} MiB"
        print(f"{step['label']}: {step['seconds']:.3f}s, peak RSS {rss_text}")


def build_payload(
    project: pathlib.Path,
    options: WorkspaceOptions,
    command: str,
    steps: list[TimedStep],
) -> dict[str, Any]:
    return {
        "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "workspace": str(project),
        "modules": options.modules,
        "external_stubs": options.external_stubs,
        "target_python": options.target_python,
        "command": command,
        "typepython_command": resolve_typepython_command(),
        "steps": [dataclasses.asdict(step) for step in steps],
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate a large TypePython workspace and measure cold/warm/edit check time."
    )
    parser.add_argument("--workspace", type=pathlib.Path)
    parser.add_argument("--modules", type=int, default=512)
    parser.add_argument("--external-stubs", type=int, default=128)
    parser.add_argument("--target-python", default="3.12")
    parser.add_argument("--command", choices=("check", "build"), default="check")
    parser.add_argument("--json-out", type=pathlib.Path)
    parser.add_argument("--skip-run", action="store_true")
    parser.add_argument("--keep-workspace", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    options = WorkspaceOptions(
        modules=args.modules,
        external_stubs=args.external_stubs,
        target_python=args.target_python,
    )

    temp_root: pathlib.Path | None = None
    if args.workspace is None:
        temp_root = pathlib.Path(tempfile.mkdtemp(prefix="typepython-industrial-perf-"))
        root = temp_root
    else:
        root = args.workspace
        if root.exists():
            shutil.rmtree(root)
        root.mkdir(parents=True)

    try:
        project = create_workspace(root, options)
        steps = [] if args.skip_run else run_smoke(project, options, args.command)
        payload = build_payload(project, options, args.command, steps)
        if args.json_out is not None:
            args.json_out.parent.mkdir(parents=True, exist_ok=True)
            args.json_out.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
        render_summary(payload)
        if args.keep_workspace or args.workspace is not None:
            print(f"kept workspace: {project}")
    finally:
        if temp_root is not None and not args.keep_workspace:
            shutil.rmtree(temp_root, ignore_errors=True)


if __name__ == "__main__":
    main()

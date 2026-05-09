from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
EXAMPLES_ROOT = ROOT / "examples"
EXAMPLE_NAMES = (
    "hello-world",
    "todo-app",
    "shapes",
    "http-client",
    "config-loader",
    "event-system",
    "showcase",
)
EXPECTED_STUB_FRAGMENTS: dict[str, tuple[tuple[str, tuple[str, ...]], ...]] = {
    "hello-world": (
        ("__init__.pyi", ("def greet(name: str) -> str: ...",)),
    ),
    "todo-app": (
        (
            "__init__.pyi",
            (
                "class Todo:",
                "class TodoList:",
                "class CreateTodoRequest(TypedDict):",
                "def complete(todos: list[Todo], title: str) -> str: ...",
                "def filter_todos(todos: list[Todo], by: str | None) -> list[Todo]: ...",
            ),
        ),
    ),
    "shapes": (
        (
            "__init__.pyi",
            (
                "class Drawable(Protocol):",
                "# tpy:sealed Shape",
                "class Circle(Shape):",
                "def area(shape: Shape) -> float: ...",
                "def largest(shapes: list[T]) -> T | None: ...",
            ),
        ),
    ),
    "http-client": (
        (
            "__init__.pyi",
            (
                "class Serializable(Protocol):",
                "class ApiClient(Generic[T]):",
                "def parse_response(response: HttpResponse, strict: bool = False) -> str | None: ...",
                "def send_batch(client: ApiClient[T], items: list[T], path: str) -> list[int]: ...",
            ),
        ),
    ),
    "config-loader": (
        (
            "__init__.pyi",
            (
                "class DatabaseConfig(TypedDict):",
                "class AppConfig(TypedDict):",
                "def load_raw_config(path: str) -> object: ...",
                "def load_config(path: str) -> AppConfig | None: ...",
            ),
        ),
    ),
    "event-system": (
        (
            "__init__.pyi",
            (
                "# tpy:sealed Event",
                "class EventHandler(Protocol):",
                "Callback: TypeAlias = str",
                "class EventLog(Generic[E]):",
                "def process_events(events: list[Event]) -> list[str]: ...",
            ),
        ),
    ),
    "showcase": (
        (
            "__init__.pyi",
            (
                "UserId: TypeAlias = int",
                "def first(items: list[",
                "def eval_expression(expr: str) -> JsonValue: ...",
                "def save_user(repo: Repository[",
            ),
        ),
        (
            "models.pyi",
            (
                "class PartialUserRecord(TypedDict):",
                "name: NotRequired[str]",
                "class UserSummary(TypedDict):",
            ),
        ),
        ("expr.pyi", ("# tpy:sealed Expr", "def evaluate(expr: Expr) -> int: ...")),
        (
            "services.pyi",
            (
                "class Serializable(Protocol):",
                "class Repository(Generic[",
                "def save(self, data: str) -> bool: ...",
            ),
        ),
    ),
}
TYPEPYTHON_ONLY_PREFIXES = ("interface ", "sealed class ", "data class ", "typealias ")


def typepython_command() -> list[str]:
    return [sys.executable, "-m", "typepython"]


def run(command: list[str]) -> None:
    print(f"+ {' '.join(command)}")
    subprocess.run(command, check=True)


def copy_example(name: str, destination_root: pathlib.Path) -> pathlib.Path:
    source = EXAMPLES_ROOT / name
    destination = destination_root / name
    shutil.copytree(
        source,
        destination,
        ignore=shutil.ignore_patterns(".typepython", "__pycache__"),
    )
    return destination


def assert_contains(path: pathlib.Path, fragments: tuple[str, ...]) -> None:
    text = path.read_text(encoding="utf-8")
    missing = [fragment for fragment in fragments if fragment not in text]
    if missing:
        joined = "; ".join(missing)
        raise SystemExit(f"{path} is missing expected fragment(s): {joined}")


def assert_runtime_erases_typepython_syntax(path: pathlib.Path) -> None:
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("#"):
            continue
        if stripped.startswith(TYPEPYTHON_ONLY_PREFIXES):
            raise SystemExit(f"{path}:{line_number} still contains TypePython-only syntax")


def assert_example_outputs(project: pathlib.Path, name: str) -> None:
    build_root = project / ".typepython" / "build" / "app"
    marker = build_root / "py.typed"
    if not marker.is_file():
        raise SystemExit(f"{name} is missing py.typed marker: {marker}")

    for relative, fragments in EXPECTED_STUB_FRAGMENTS[name]:
        stub_path = build_root / relative
        if not stub_path.is_file():
            raise SystemExit(f"{name} is missing expected stub artifact: {stub_path}")
        assert_contains(stub_path, fragments)

    runtime_files = sorted(build_root.glob("*.py"))
    if not runtime_files:
        raise SystemExit(f"{name} did not emit runtime Python files under {build_root}")
    for runtime_path in runtime_files:
        assert_runtime_erases_typepython_syntax(runtime_path)


def run_example_smoke(name: str, workspace: pathlib.Path) -> None:
    project = copy_example(name, workspace)
    command = typepython_command()
    run([*command, "check", "--project", str(project)])
    run([*command, "build", "--project", str(project)])
    run([*command, "verify", "--project", str(project)])
    assert_example_outputs(project, name)


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="typepython-examples-smoke-") as tmp:
        workspace = pathlib.Path(tmp)
        for name in EXAMPLE_NAMES:
            run_example_smoke(name, workspace)

    print("examples smoke test passed")


if __name__ == "__main__":
    main()

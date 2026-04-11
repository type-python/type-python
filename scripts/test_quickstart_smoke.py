from __future__ import annotations

import importlib.util
import pathlib
import tempfile
import unittest
from unittest import mock


SCRIPT_PATH = pathlib.Path(__file__).with_name("quickstart_smoke.py")
SPEC = importlib.util.spec_from_file_location("quickstart_smoke", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
quickstart_smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(quickstart_smoke)


class QuickstartSmokeTests(unittest.TestCase):
    def test_resolve_entrypoint_prefers_active_python_scripts_dir(self) -> None:
        with tempfile.TemporaryDirectory(prefix="quickstart-smoke-test-") as tmp:
            root = pathlib.Path(tmp)
            scripts_dir = root / "bin"
            scripts_dir.mkdir()
            installed_entrypoint = scripts_dir / "typepython"
            installed_entrypoint.write_text("", encoding="utf-8")

            path_dir = root / "path-bin"
            path_dir.mkdir()
            shadowed_entrypoint = path_dir / "typepython"
            shadowed_entrypoint.write_text("", encoding="utf-8")

            with (
                mock.patch.object(
                    quickstart_smoke.sys, "executable", str(scripts_dir / "python")
                ),
                mock.patch.object(
                    quickstart_smoke.shutil,
                    "which",
                    return_value=str(shadowed_entrypoint),
                ),
            ):
                resolved = quickstart_smoke.resolve_entrypoint()

        self.assertEqual(resolved, str(installed_entrypoint))

    def test_main_uses_resolved_entrypoint_for_full_smoke_flow(self) -> None:
        commands: list[tuple[list[str], pathlib.Path | None]] = []
        entrypoint = "/fake/typepython"

        def fake_run(command: list[str], cwd: pathlib.Path | None = None) -> None:
            commands.append((command, cwd))
            if cwd is None:
                return
            if command == [entrypoint, "init", "--dir", "my-project"]:
                (cwd / "my-project").mkdir()
                return
            if command == [entrypoint, "build", "--project", "."]:
                build_root = cwd / ".typepython" / "build" / "app"
                build_root.mkdir(parents=True)
                for filename in ("__init__.py", "__init__.pyi", "py.typed"):
                    (build_root / filename).write_text("", encoding="utf-8")

        with (
            mock.patch.object(
                quickstart_smoke, "resolve_entrypoint", return_value=entrypoint
            ),
            mock.patch.object(quickstart_smoke, "run", side_effect=fake_run),
        ):
            quickstart_smoke.main()

        self.assertEqual(
            [command for command, _ in commands],
            [
                [entrypoint, "--help"],
                [entrypoint, "init", "--dir", "my-project"],
                [entrypoint, "check", "--project", "."],
                [entrypoint, "build", "--project", "."],
            ],
        )


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import importlib.util
import json
import os
import pathlib
import tempfile
import unittest
from unittest import mock


SCRIPT_PATH = pathlib.Path(__file__).with_name("native_target_smoke.py")
SPEC = importlib.util.spec_from_file_location("native_target_smoke", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
native_target_smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(native_target_smoke)


class NativeTargetSmokeTests(unittest.TestCase):
    def test_rewrite_target_python_updates_single_target_line(self) -> None:
        with tempfile.TemporaryDirectory(prefix="native-target-smoke-test-") as tmp:
            config_path = pathlib.Path(tmp) / "typepython.toml"
            config_path.write_text(
                '[project]\nsrc = ["src"]\ntarget_python = "3.10"\n',
                encoding="utf-8",
            )

            native_target_smoke.rewrite_target_python(config_path, "3.14")

            rendered = config_path.read_text(encoding="utf-8")

        self.assertIn('target_python = "3.14"', rendered)
        self.assertNotIn('target_python = "3.10"', rendered)

    def test_rewrite_python_executable_updates_single_resolution_line(self) -> None:
        with tempfile.TemporaryDirectory(prefix="native-target-smoke-test-") as tmp:
            config_path = pathlib.Path(tmp) / "typepython.toml"
            config_path.write_text(
                '[project]\nsrc = ["src"]\n\n[resolution]\npython_executable = null\n',
                encoding="utf-8",
            )

            native_target_smoke.rewrite_python_executable(
                config_path,
                "/tmp/python with spaces/bin/python",
            )

            rendered = config_path.read_text(encoding="utf-8")

        self.assertIn(
            'python_executable = "/tmp/python with spaces/bin/python"',
            rendered,
        )
        self.assertNotIn("python_executable = null", rendered)

    def test_main_runs_native_build_and_verify_flow(self) -> None:
        commands: list[tuple[list[str], pathlib.Path | None]] = []
        entrypoint = "/fake/typepython"
        asserted_outputs: list[pathlib.Path] = []
        asserted_runtime: list[pathlib.Path] = []
        written_sources: list[pathlib.Path] = []

        def fake_run(command: list[str], cwd: pathlib.Path | None = None) -> None:
            commands.append((command, cwd))
            if cwd is None:
                return
            if command == [entrypoint, "init", "--dir", "native-project"]:
                project_dir = cwd / "native-project"
                (project_dir / "src" / "app").mkdir(parents=True)
                (project_dir / "typepython.toml").write_text(
                    '[project]\nsrc = ["src"]\ntarget_python = "3.10"\n\n'
                    "[resolution]\npython_executable = null\n",
                    encoding="utf-8",
                )

        def fake_write_native_source(path: pathlib.Path) -> None:
            written_sources.append(path)
            path.write_text("type Pair[T = int] = tuple[T, T]\n", encoding="utf-8")

        def fake_assert_outputs(project_dir: pathlib.Path) -> None:
            asserted_outputs.append(project_dir)

        def fake_assert_runtime(project_dir: pathlib.Path) -> None:
            asserted_runtime.append(project_dir)

        with (
            mock.patch.object(
                native_target_smoke, "resolve_entrypoint", return_value=entrypoint
            ),
            mock.patch.object(native_target_smoke, "run", side_effect=fake_run),
            mock.patch.object(
                native_target_smoke,
                "write_native_source",
                side_effect=fake_write_native_source,
            ),
            mock.patch.object(
                native_target_smoke,
                "assert_native_outputs",
                side_effect=fake_assert_outputs,
            ),
            mock.patch.object(
                native_target_smoke,
                "assert_runtime_semantics",
                side_effect=fake_assert_runtime,
            ),
            mock.patch(
                "sys.argv",
                ["native_target_smoke.py", "--target-python", "3.13"],
            ),
        ):
            native_target_smoke.main()

        self.assertEqual(
            [command for command, _ in commands],
            [
                [entrypoint, "--help"],
                [entrypoint, "init", "--dir", "native-project"],
                [entrypoint, "check", "--project", "."],
                [entrypoint, "build", "--project", "."],
                [entrypoint, "verify", "--project", ".", "--unsafe-runtime-imports"],
            ],
        )
        self.assertEqual(len(written_sources), 1)
        self.assertEqual(len(asserted_outputs), 1)
        self.assertEqual(len(asserted_runtime), 1)
        self.assertEqual(asserted_outputs[0], asserted_runtime[0])

    def test_child_environment_does_not_recurse_through_typepython_bin(self) -> None:
        with mock.patch.dict(os.environ, {"TYPEPYTHON_BIN": "/tmp/typepython-wrapper"}):
            child_env = native_target_smoke.child_environment()

        self.assertNotIn("TYPEPYTHON_BIN", child_env)

    def test_framework_annotation_audit_summary_detects_expected_consumers(self) -> None:
        payload = native_target_smoke.framework_annotation_audit_summary()

        self.assertEqual(
            set(payload["framework_annotation_consumers"]),
            {
                "fastapi.Depends",
                "fastapi.route_decorator",
                "pydantic.BaseModel",
                "pydantic.Field",
            },
        )
        self.assertTrue(payload["framework_annotation_safe"])

    def test_runtime_probe_is_self_contained(self) -> None:
        captured_probe: list[str] = []

        def fake_capture(command: list[str], cwd: pathlib.Path | None = None) -> str:
            self.assertIsNone(cwd)
            self.assertEqual(command[1], "-c")
            captured_probe.append(command[2])
            return json.dumps(
                {
                    "module_has_T": False,
                    "pair_type_name": "TypeAliasType",
                    "pair_type_module": "typing",
                    "pair_type_params": ["T"],
                    "box_type_params": ["T"],
                    "first_pair_type_params": ["T"],
                    "pair_default_present": True,
                    "box_default_present": True,
                    "function_default_present": True,
                    "scope_alias_type_name": "TypeAliasType",
                    "scope_alias_resolves_nested": True,
                    "explosive_error": "ZeroDivisionError",
                    "framework_annotation_consumers": [
                        "fastapi.Depends",
                        "fastapi.route_decorator",
                        "pydantic.BaseModel",
                        "pydantic.Field",
                    ],
                    "framework_annotation_safe": True,
                }
            )

        with (
            tempfile.TemporaryDirectory(prefix="native-target-smoke-test-") as tmp,
            mock.patch.object(native_target_smoke, "capture", side_effect=fake_capture),
        ):
            native_target_smoke.assert_runtime_semantics(pathlib.Path(tmp))

        self.assertEqual(len(captured_probe), 1)
        self.assertIn("audit_source(framework_audit_source)", captured_probe[0])
        self.assertNotIn("framework_annotation_audit_summary", captured_probe[0])


if __name__ == "__main__":
    unittest.main()

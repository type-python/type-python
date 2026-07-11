from __future__ import annotations

import hashlib
import importlib.util
import json
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
    def test_assert_bundled_stdlib_validates_installed_snapshot(self) -> None:
        with tempfile.TemporaryDirectory(prefix="quickstart-stdlib-test-") as tmp:
            root = pathlib.Path(tmp) / "typepython" / "stdlib"
            root.mkdir(parents=True)
            for relative in ("BASELINE.toml", "VERSIONS", "builtins.pyi", "typing.pyi"):
                (root / relative).write_text("# fixture\n", encoding="utf-8")

            files = [root / "builtins.pyi", root / "typing.pyi"]
            digest = hashlib.sha256()
            byte_count = 0
            for path in files:
                contents = path.read_bytes()
                digest.update(path.relative_to(root).as_posix().encode("utf-8"))
                digest.update(b"\0")
                digest.update(contents)
                byte_count += len(contents)
            (root / "REFRESH_STATS.json").write_text(
                json.dumps(
                    {
                        "stdlib_files": len(files),
                        "stdlib_bytes": byte_count,
                        "stdlib_sha256": digest.hexdigest(),
                    }
                ),
                encoding="utf-8",
            )
            distribution = mock.Mock()
            distribution.locate_file.return_value = root
            with mock.patch.object(
                quickstart_smoke.importlib.metadata,
                "distribution",
                return_value=distribution,
            ):
                quickstart_smoke.assert_bundled_stdlib()

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

    def test_assert_cli_version_rejects_stale_binary(self) -> None:
        result = mock.Mock(stdout="typepython 0.9.0\n")
        with (
            mock.patch.object(
                quickstart_smoke.importlib.metadata,
                "version",
                return_value="1.0.0-rc.1",
            ),
            mock.patch.object(quickstart_smoke.subprocess, "run", return_value=result),
        ):
            with self.assertRaisesRegex(SystemExit, "CLI version mismatch"):
                quickstart_smoke.assert_cli_version("/fake/typepython")

    def test_assert_cli_version_accepts_pep440_normalized_prerelease(self) -> None:
        result = mock.Mock(stdout="typepython 1.0.0-rc.1\n")
        with (
            mock.patch.object(
                quickstart_smoke.importlib.metadata,
                "version",
                return_value="1.0.0rc1",
            ),
            mock.patch.object(quickstart_smoke.subprocess, "run", return_value=result),
        ):
            quickstart_smoke.assert_cli_version("/fake/typepython")

    def test_assert_cli_version_rejects_unexpected_output_shape(self) -> None:
        result = mock.Mock(stdout="version 1.0.0rc1\n")
        with (
            mock.patch.object(
                quickstart_smoke.importlib.metadata,
                "version",
                return_value="1.0.0rc1",
            ),
            mock.patch.object(quickstart_smoke.subprocess, "run", return_value=result),
        ):
            with self.assertRaisesRegex(SystemExit, "CLI version mismatch"):
                quickstart_smoke.assert_cli_version("/fake/typepython")

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
            mock.patch.object(quickstart_smoke, "assert_bundled_stdlib") as assert_stdlib,
            mock.patch.object(
                quickstart_smoke, "resolve_entrypoint", return_value=entrypoint
            ),
            mock.patch.object(quickstart_smoke, "assert_cli_version") as assert_version,
            mock.patch.object(quickstart_smoke, "run", side_effect=fake_run),
        ):
            quickstart_smoke.main()

        assert_stdlib.assert_called_once_with()
        assert_version.assert_called_once_with(entrypoint)

        self.assertEqual(
            [command for command, _ in commands],
            [
                [entrypoint, "--help"],
                [entrypoint, "init", "--dir", "my-project"],
                [entrypoint, "check", "--project", "."],
                [entrypoint, "build", "--project", "."],
                [entrypoint, "verify", "--project", "."],
            ],
        )


if __name__ == "__main__":
    unittest.main()

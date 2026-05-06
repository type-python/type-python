from __future__ import annotations

import unittest
import pathlib
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import downstream_checker_smoke


class DownstreamCheckerMatrixTests(unittest.TestCase):
    def test_matrix_lists_existing_fixtures_and_targets(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()

        self.assertIn("basic-package", matrix)
        self.assertIn("rich-package", matrix)
        self.assertIn("compat-package", matrix)
        self.assertIn("pydantic-like-package", matrix)
        self.assertIn("fastapi-like-package", matrix)
        self.assertIn("negative-consumer-package", matrix)
        self.assertIn("standard-typing-package", matrix)
        self.assertIn("toy-task-package", matrix)
        self.assertIn("dual-emit-package", matrix)
        for case in matrix.values():
            fixture_dir = downstream_checker_smoke.FIXTURE_ROOT / case.name
            self.assertTrue(fixture_dir.is_dir(), fixture_dir)
            self.assertGreater(len(case.targets), 0)
            self.assertGreater(len(case.profiles), 0)

    def test_negative_fixtures_are_explicitly_marked(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()

        negative_cases = [
            case
            for case in matrix.values()
            if case.expect_checker_failure or case.expected_checker_failures
        ]
        self.assertGreater(len(negative_cases), 0)
        for case in negative_cases:
            self.assertTrue(case.name.startswith("negative-"))
            consumer_path = downstream_checker_smoke.FIXTURE_ROOT / case.name / "checker-consumer.py"
            self.assertTrue(consumer_path.exists(), consumer_path)

    def test_expected_checker_disagreements_have_reason_and_expiry(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()
        payload = downstream_checker_smoke.MATRIX_PATH.read_text(encoding="utf-8")
        import json

        matrix_payload = json.loads(payload)
        for raw_case in matrix_payload["fixtures"]:
            if raw_case.get("expect_checker_failure") or raw_case.get("expected_checker_failures"):
                case = matrix[raw_case["name"]]
                self.assertRegex(raw_case.get("allowlist_reason", ""), r"\S")
                self.assertRegex(raw_case.get("allowlist_expires", ""), r"^20\d{2}-\d{2}-\d{2}$")
                self.assertIsNotNone(case.allowlist_reason)
                self.assertIsNotNone(case.allowlist_expires)

    def test_expired_expected_checker_disagreements_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            matrix_path = pathlib.Path(tmp) / "matrix.json"
            matrix_path.write_text(
                """
                {
                  "fixtures": [
                    {
                      "name": "negative-expired",
                      "targets": ["3.12"],
                      "expect_checker_failure": true,
                      "allowlist_reason": "expired test fixture",
                      "allowlist_expires": "2000-01-01"
                    }
                  ]
                }
                """,
                encoding="utf-8",
            )

            with self.assertRaises(SystemExit):
                downstream_checker_smoke.load_fixture_matrix(matrix_path)

    def test_checker_commands_include_strict_profiles(self) -> None:
        build_dir = pathlib.Path("checker-build")

        self.assertIn("basedpyright", downstream_checker_smoke.DEFAULT_CHECKERS)
        self.assertIn(
            "--strict",
            downstream_checker_smoke.checker_command("mypy", "strict", "3.12", build_dir),
        )
        self.assertIn(
            "--pythonversion",
            downstream_checker_smoke.checker_command("pyright", "strict", "3.12", build_dir),
        )
        self.assertIn(
            "basedpyright",
            downstream_checker_smoke.checker_command("basedpyright", "strict", "3.12", build_dir),
        )

    def test_expected_stub_fragments_are_keyed_by_declared_target(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()

        for case in matrix.values():
            if case.expected_stub_fragments is None:
                continue
            self.assertEqual(set(case.expected_stub_fragments), set(case.targets))

    def test_checker_build_dir_uses_visible_copy(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            project_dir = pathlib.Path(tmp) / "project"
            build_dir = project_dir / ".typepython" / "build"
            package_dir = build_dir / "app"
            package_dir.mkdir(parents=True)
            (package_dir / "__init__.pyi").write_text(
                "def parse_count(value: str) -> int: ...\n",
                encoding="utf-8",
            )

            checker_build_dir = downstream_checker_smoke.prepare_checker_build_dir(
                build_dir,
                project_dir,
            )

            self.assertEqual(checker_build_dir, project_dir / "checker-build")
            self.assertTrue((checker_build_dir / "app" / "__init__.pyi").exists())
            self.assertFalse(checker_build_dir.relative_to(project_dir).parts[0].startswith("."))


if __name__ == "__main__":
    unittest.main()

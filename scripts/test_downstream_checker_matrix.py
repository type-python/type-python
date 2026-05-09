from __future__ import annotations

import unittest
import json
import os
import pathlib
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import downstream_checker_smoke


SUPPORTED_TARGETS = {"3.10", "3.11", "3.12", "3.13", "3.14"}


class DownstreamCheckerMatrixTests(unittest.TestCase):
    def test_matrix_lists_existing_fixtures_and_targets(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()

        self.assertIn("basic-package", matrix)
        self.assertIn("rich-package", matrix)
        self.assertIn("flagship-core-package", matrix)
        self.assertIn("compat-package", matrix)
        self.assertIn("pydantic-like-package", matrix)
        self.assertIn("fastapi-like-package", matrix)
        self.assertIn("negative-consumer-package", matrix)
        self.assertIn("standard-typing-package", matrix)
        self.assertIn("toy-task-package", matrix)
        self.assertIn("dual-emit-package", matrix)
        self.assertIn("typeshed-heavy-package", matrix)
        self.assertIn("namespace-package", matrix)
        self.assertIn("pep561-partial-stub-package", matrix)
        self.assertIn("ecosystem-patterns-package", matrix)
        self.assertIn("sdk-client-package", matrix)
        self.assertIn("overload-heavy-package", matrix)
        for case in matrix.values():
            fixture_dir = downstream_checker_smoke.FIXTURE_ROOT / case.name
            self.assertTrue(fixture_dir.is_dir(), fixture_dir)
            self.assertGreater(len(case.targets), 0)
            self.assertGreater(len(case.profiles), 0)

    def test_ecosystem_corpus_categories_are_mapped_to_fixtures(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()
        payload = json.loads(downstream_checker_smoke.MATRIX_PATH.read_text(encoding="utf-8"))
        corpus = payload.get("ecosystem_corpus", {}).get("baseline_categories", {})

        expected_categories = {
            "attrs_dataclasses_heavy",
            "pydantic_v2_heavy",
            "fastapi_route_structure",
            "sqlalchemy_typing_heavy",
            "protocol_paramspec_heavy",
            "typeddict_sdk_client",
            "flagship_core_package",
            "namespace_package",
            "partial_stub_package",
            "large_py_typed_package",
            "overload_heavy_package",
        }
        self.assertEqual(set(corpus), expected_categories)
        for category, fixture_names in corpus.items():
            self.assertGreater(len(fixture_names), 0, category)
            for fixture_name in fixture_names:
                self.assertIn(fixture_name, matrix, category)

    def test_matrix_has_release_target_coverage(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()

        self.assertEqual(set(matrix["basic-package"].targets), SUPPORTED_TARGETS)
        self.assertEqual(set(matrix["standard-typing-package"].targets), SUPPORTED_TARGETS)
        self.assertEqual(set(matrix["compat-package"].targets), SUPPORTED_TARGETS)

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

        matrix_payload = json.loads(payload)
        for raw_case in matrix_payload["fixtures"]:
            if raw_case.get("expect_checker_failure") or raw_case.get("expected_checker_failures"):
                case = matrix[raw_case["name"]]
                self.assertRegex(raw_case.get("allowlist_reason", ""), r"\S")
                self.assertRegex(raw_case.get("allowlist_expires", ""), r"^20\d{2}-\d{2}-\d{2}$")
                self.assertIsNotNone(case.allowlist_reason)
                self.assertIsNotNone(case.allowlist_expires)

    def test_partial_expected_failures_still_run_consumer_for_other_checkers(self) -> None:
        case = downstream_checker_smoke.FixtureCase(
            name="negative-synthetic-package",
            targets=("3.12",),
            expected_checker_failures=("mypy",),
            allowlist_reason="synthetic checker disagreement",
        )
        self.assertFalse(
            downstream_checker_smoke.checker_failure_expected(
                case,
                "pyright",
                "strict",
                downstream_checker_smoke.DEFAULT_CHECKERS,
            )
        )

        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            consumer_path = root / "checker-consumer.py"
            build_consumer_path = root / "checker-build" / "checker_consumer.py"
            build_consumer_path.parent.mkdir()
            consumer_path.write_text("from app import StrictUser\n", encoding="utf-8")

            downstream_checker_smoke.sync_checker_consumer(
                consumer_path,
                build_consumer_path,
            )

            self.assertEqual(
                build_consumer_path.read_text(encoding="utf-8"),
                "from app import StrictUser\n",
            )

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
            "--explicit-package-bases",
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
        self.assertIn(
            "--extra-search-path",
            downstream_checker_smoke.checker_command("ty", "strict", "3.12", build_dir),
        )

    def test_ty_command_includes_checker_root_for_namespace_packages(self) -> None:
        build_dir = pathlib.Path("/tmp/checker-build")

        command = downstream_checker_smoke.checker_command(
            "ty",
            "strict",
            "3.12",
            build_dir,
        )

        extra_search_path_index = command.index("--extra-search-path")
        self.assertEqual(command[extra_search_path_index + 1], str(build_dir))
        self.assertEqual(command[-1], str(build_dir))

    def test_ty_command_includes_checker_support_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            project_dir = pathlib.Path(tmp)
            build_dir = project_dir / "checker-build"
            (project_dir / "checker-support" / "typings").mkdir(parents=True)
            (project_dir / "checker-support" / "vendor-stubs").mkdir()
            build_dir.mkdir()

            command = downstream_checker_smoke.checker_command(
                "ty",
                "strict",
                "3.12",
                build_dir,
            )

            search_paths = [
                pathlib.Path(command[index + 1])
                for index, value in enumerate(command)
                if value == "--extra-search-path"
            ]
            self.assertEqual(
                search_paths,
                [
                    build_dir,
                    project_dir / "checker-support" / "typings",
                    project_dir / "checker-support" / "vendor-stubs",
                    project_dir / "checker-support",
                ],
            )

    def test_pyright_strict_config_preserves_strict_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            project_dir = pathlib.Path(tmp)
            build_dir = project_dir / "checker-build"
            build_dir.mkdir()

            downstream_checker_smoke.write_pyright_config(project_dir, build_dir, "strict")
            config = json.loads((project_dir / "pyrightconfig.json").read_text(encoding="utf-8"))

            self.assertEqual(config["typeCheckingMode"], "strict")
            self.assertEqual(config["extraPaths"], ["checker-build"])
            self.assertEqual(config["reportUnusedImport"], "none")
            for strict_setting in (
                "reportMissingTypeStubs",
                "reportUnknownVariableType",
                "reportUnknownMemberType",
                "reportUnknownArgumentType",
                "reportUnknownParameterType",
            ):
                self.assertNotIn(strict_setting, config)

    def test_pyright_standard_config_focuses_on_type_portability(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            project_dir = pathlib.Path(tmp)
            build_dir = project_dir / "checker-build"
            build_dir.mkdir()

            downstream_checker_smoke.write_pyright_config(project_dir, build_dir, "standard")
            config = json.loads((project_dir / "pyrightconfig.json").read_text(encoding="utf-8"))

            self.assertEqual(config["typeCheckingMode"], "standard")
            self.assertEqual(config["extraPaths"], ["checker-build"])
            self.assertEqual(config["reportUnknownVariableType"], "none")
            self.assertEqual(config["reportUnnecessaryCast"], "none")

    def test_pyright_config_uses_checker_stub_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            project_dir = pathlib.Path(tmp)
            build_dir = project_dir / "checker-build"
            (project_dir / "checker-support" / "typings").mkdir(parents=True)
            build_dir.mkdir()

            downstream_checker_smoke.write_pyright_config(project_dir, build_dir, "strict")
            config = json.loads((project_dir / "pyrightconfig.json").read_text(encoding="utf-8"))

            self.assertEqual(config["stubPath"], "checker-support/typings")
            self.assertIn("checker-support/typings", config["extraPaths"])

    def test_mypy_command_env_includes_checker_root(self) -> None:
        command = downstream_checker_smoke.checker_command(
            "mypy",
            "strict",
            "3.12",
            pathlib.Path("/tmp/checker-build"),
        )

        env = downstream_checker_smoke.command_env(command)

        self.assertIsNotNone(env)
        assert env is not None
        self.assertIn("/tmp/checker-build", env["MYPYPATH"].split(os.pathsep))

    def test_mypy_command_env_includes_checker_support(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            project_dir = pathlib.Path(tmp)
            (project_dir / "checker-support" / "vendor-stubs").mkdir(parents=True)
            command = downstream_checker_smoke.checker_command(
                "mypy",
                "strict",
                "3.12",
                project_dir / "checker-build",
            )

            env = downstream_checker_smoke.command_env(command, project_dir)

            self.assertIsNotNone(env)
            assert env is not None
            self.assertIn(
                str(project_dir / "checker-support" / "vendor-stubs"),
                env["MYPYPATH"].split(os.pathsep),
            )

    def test_expected_stub_fragments_are_keyed_by_declared_target(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()

        for case in matrix.values():
            if case.expected_stub_fragments is None:
                continue
            self.assertEqual(set(case.expected_stub_fragments), set(case.targets))

    def test_expected_stub_fragments_can_target_non_app_modules(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()
        fragments = matrix["namespace-package"].expected_stub_fragments

        self.assertIsNotNone(fragments)
        assert fragments is not None
        self.assertIn("acme/widgets/__init__.pyi", fragments["3.12"])

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

    def test_checker_support_paths_include_stub_packages_first(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            project_dir = pathlib.Path(tmp) / "fixture"
            support_dir = project_dir / "checker-support"
            (support_dir / "typings").mkdir(parents=True)
            (support_dir / "vendor-stubs").mkdir(parents=True)
            (support_dir / "vendor").mkdir()

            paths = downstream_checker_smoke.checker_support_paths(project_dir)

            self.assertEqual(
                paths,
                (
                    pathlib.Path("checker-support/typings"),
                    pathlib.Path("checker-support/vendor-stubs"),
                    pathlib.Path("checker-support"),
                ),
            )


if __name__ == "__main__":
    unittest.main()

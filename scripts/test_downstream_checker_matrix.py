from __future__ import annotations

import unittest
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import downstream_checker_smoke


class DownstreamCheckerMatrixTests(unittest.TestCase):
    def test_matrix_lists_existing_fixtures_and_targets(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()

        self.assertIn("basic-package", matrix)
        self.assertIn("compat-package", matrix)
        self.assertIn("negative-consumer-package", matrix)
        self.assertIn("standard-typing-package", matrix)
        for case in matrix.values():
            fixture_dir = downstream_checker_smoke.FIXTURE_ROOT / case.name
            self.assertTrue(fixture_dir.is_dir(), fixture_dir)
            self.assertGreater(len(case.targets), 0)

    def test_negative_fixtures_are_explicitly_marked(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()

        negative_cases = [case for case in matrix.values() if case.expect_checker_failure]
        self.assertGreater(len(negative_cases), 0)
        for case in negative_cases:
            self.assertTrue(case.name.startswith("negative-"))
            consumer_path = downstream_checker_smoke.FIXTURE_ROOT / case.name / "checker-consumer.py"
            self.assertTrue(consumer_path.exists(), consumer_path)

    def test_expected_stub_fragments_are_keyed_by_declared_target(self) -> None:
        matrix = downstream_checker_smoke.load_fixture_matrix()

        for case in matrix.values():
            if case.expected_stub_fragments is None:
                continue
            self.assertEqual(set(case.expected_stub_fragments), set(case.targets))


if __name__ == "__main__":
    unittest.main()

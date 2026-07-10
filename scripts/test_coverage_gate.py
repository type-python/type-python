from __future__ import annotations

import unittest

from scripts.check_coverage import branch_coverage_percent, lcov_branch_totals


class CoverageGateTests(unittest.TestCase):
    def test_branch_totals_sum_all_lcov_records(self) -> None:
        contents = "TN:\nSF:first.rs\nBRF:10\nBRH:7\nend_of_record\nSF:second.rs\nBRF:5\nBRH:2\n"

        self.assertEqual(lcov_branch_totals(contents), (9, 15))
        self.assertEqual(branch_coverage_percent(contents), 60.0)

    def test_missing_branch_instrumentation_fails_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "no branch records"):
            branch_coverage_percent("TN:\nSF:line-only.rs\nLF:10\nLH:10\n")

    def test_invalid_branch_totals_are_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "exceeds"):
            branch_coverage_percent("BRF:2\nBRH:3\n")


if __name__ == "__main__":
    unittest.main()

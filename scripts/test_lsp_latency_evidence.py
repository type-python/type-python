from __future__ import annotations

import json
import pathlib
import tempfile
import unittest

from scripts import lsp_latency_evidence


class LspLatencyEvidenceTests(unittest.TestCase):
    def write_samples(
        self,
        root: pathlib.Path,
        benchmark: str,
        *,
        iterations: list[float],
        times: list[float],
    ) -> None:
        sample_path = root / benchmark / "new" / "sample.json"
        sample_path.parent.mkdir(parents=True, exist_ok=True)
        sample_path.write_text(
            json.dumps({"sampling_mode": "Flat", "iters": iterations, "times": times}),
            encoding="utf-8",
        )

    def test_build_evidence_calculates_per_iteration_percentiles(self) -> None:
        with tempfile.TemporaryDirectory(prefix="typepython-lsp-latency-test-") as tmp:
            root = pathlib.Path(tmp)
            for benchmark in lsp_latency_evidence.BENCHMARKS:
                self.write_samples(
                    root,
                    benchmark,
                    iterations=[1, 1, 1, 1],
                    times=[4_000_000, 2_000_000, 1_000_000, 3_000_000],
                )

            payload = lsp_latency_evidence.build_evidence(root, min_samples=4)

        self.assertEqual(payload["schema_version"], 1)
        self.assertEqual(payload["fixture_modules"], 512)
        for summary in payload["benchmarks"].values():
            self.assertEqual(summary["samples"], 4)
            self.assertEqual(summary["min_ms"], 1.0)
            self.assertEqual(summary["median_ms"], 2.0)
            self.assertEqual(summary["p95_ms"], 4.0)
            self.assertEqual(summary["p99_ms"], 4.0)
            self.assertEqual(summary["max_ms"], 4.0)

    def test_load_samples_rejects_short_or_invalid_measurements(self) -> None:
        with tempfile.TemporaryDirectory(prefix="typepython-lsp-latency-test-") as tmp:
            root = pathlib.Path(tmp)
            benchmark = lsp_latency_evidence.BENCHMARKS[0]
            self.write_samples(root, benchmark, iterations=[1], times=[1])
            with self.assertRaisesRegex(ValueError, "expected at least 2"):
                lsp_latency_evidence.load_iteration_nanoseconds(
                    root,
                    benchmark,
                    min_samples=2,
                )

            self.write_samples(root, benchmark, iterations=[0, 1], times=[1, 1])
            with self.assertRaisesRegex(ValueError, "invalid iterations"):
                lsp_latency_evidence.load_iteration_nanoseconds(
                    root,
                    benchmark,
                    min_samples=2,
                )

            self.write_samples(root, benchmark, iterations=[2, 2], times=[2, 2])
            with self.assertRaisesRegex(ValueError, "one session per sample"):
                lsp_latency_evidence.load_iteration_nanoseconds(
                    root,
                    benchmark,
                    min_samples=2,
                )

    def test_missing_benchmark_sample_is_a_hard_failure(self) -> None:
        with tempfile.TemporaryDirectory(prefix="typepython-lsp-latency-test-") as tmp:
            with self.assertRaisesRegex(ValueError, "missing Criterion sample data"):
                lsp_latency_evidence.build_evidence(pathlib.Path(tmp), min_samples=2)


if __name__ == "__main__":
    unittest.main()

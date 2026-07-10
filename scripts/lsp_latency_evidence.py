from __future__ import annotations

import argparse
import datetime as dt
import json
import math
import pathlib
from typing import Any


BENCHMARKS = (
    "lsp_incremental_impl_edit_session_512_modules",
    "lsp_incremental_public_edit_session_512_modules",
)


def percentile_nearest_rank(samples: list[float], percentile: float) -> float:
    if not samples:
        raise ValueError("latency samples must not be empty")
    if not 0 < percentile <= 100:
        raise ValueError("percentile must be in the interval (0, 100]")
    ordered = sorted(samples)
    rank = max(0, math.ceil(percentile / 100 * len(ordered)) - 1)
    return ordered[rank]


def load_iteration_nanoseconds(
    criterion_root: pathlib.Path,
    benchmark: str,
    *,
    min_samples: int,
) -> list[float]:
    sample_path = criterion_root / benchmark / "new" / "sample.json"
    try:
        payload = json.loads(sample_path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise ValueError(f"missing Criterion sample data: {sample_path}") from error
    except json.JSONDecodeError as error:
        raise ValueError(f"invalid Criterion sample JSON in {sample_path}: {error}") from error

    iterations = payload.get("iters")
    times = payload.get("times")
    if payload.get("sampling_mode") != "Flat":
        raise ValueError(f"Criterion sample data in {sample_path} must use Flat sampling")
    if not isinstance(iterations, list) or not isinstance(times, list):
        raise ValueError(f"Criterion sample data in {sample_path} must contain iters and times")
    if len(iterations) != len(times):
        raise ValueError(f"Criterion sample data in {sample_path} has mismatched sample arrays")
    if len(times) < min_samples:
        raise ValueError(
            f"Criterion sample data in {sample_path} has {len(times)} samples; "
            f"expected at least {min_samples}"
        )

    samples: list[float] = []
    for index, (iteration_count, total_nanoseconds) in enumerate(zip(iterations, times)):
        if not isinstance(iteration_count, (int, float)) or iteration_count <= 0:
            raise ValueError(f"Criterion sample {index} in {sample_path} has invalid iterations")
        if iteration_count != 1:
            raise ValueError(
                f"Criterion sample {index} in {sample_path} averages {iteration_count} sessions; "
                "p95/p99 evidence requires one session per sample"
            )
        if not isinstance(total_nanoseconds, (int, float)) or total_nanoseconds < 0:
            raise ValueError(f"Criterion sample {index} in {sample_path} has invalid time")
        samples.append(float(total_nanoseconds))
    return samples


def summarize_milliseconds(samples_ns: list[float]) -> dict[str, float | int]:
    return {
        "samples": len(samples_ns),
        "min_ms": min(samples_ns) / 1_000_000,
        "median_ms": percentile_nearest_rank(samples_ns, 50) / 1_000_000,
        "p95_ms": percentile_nearest_rank(samples_ns, 95) / 1_000_000,
        "p99_ms": percentile_nearest_rank(samples_ns, 99) / 1_000_000,
        "max_ms": max(samples_ns) / 1_000_000,
    }


def build_evidence(
    criterion_root: pathlib.Path,
    *,
    min_samples: int,
) -> dict[str, Any]:
    if min_samples < 2:
        raise ValueError("minimum sample count must be at least 2")
    benchmarks = {
        benchmark: summarize_milliseconds(
            load_iteration_nanoseconds(
                criterion_root,
                benchmark,
                min_samples=min_samples,
            )
        )
        for benchmark in BENCHMARKS
    }
    return {
        "schema_version": 1,
        "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "fixture_modules": 512,
        "criterion_root": str(criterion_root),
        "benchmarks": benchmarks,
    }


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Calculate p95/p99 evidence from the 512-module LSP Criterion samples."
    )
    parser.add_argument("--criterion-root", type=pathlib.Path, default="target/criterion")
    parser.add_argument("--min-samples", type=int, default=20)
    parser.add_argument("--json-out", type=pathlib.Path, required=True)
    args = parser.parse_args()

    try:
        payload = build_evidence(args.criterion_root, min_samples=args.min_samples)
    except ValueError as error:
        raise SystemExit(str(error)) from error
    args.json_out.parent.mkdir(parents=True, exist_ok=True)
    args.json_out.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(payload, indent=2))


if __name__ == "__main__":
    main()

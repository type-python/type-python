from __future__ import annotations

import argparse
import pathlib


def lcov_branch_totals(contents: str) -> tuple[int, int]:
    found = 0
    hit = 0
    for line in contents.splitlines():
        if line.startswith("BRF:"):
            found += int(line.removeprefix("BRF:"))
        elif line.startswith("BRH:"):
            hit += int(line.removeprefix("BRH:"))
    return hit, found


def branch_coverage_percent(contents: str) -> float:
    hit, found = lcov_branch_totals(contents)
    if found == 0:
        raise ValueError("LCOV report has no branch records; run cargo llvm-cov with --branch")
    if hit > found:
        raise ValueError(f"LCOV branch totals are invalid: {hit} hit exceeds {found} found")
    return hit * 100.0 / found


def main() -> None:
    parser = argparse.ArgumentParser(description="Enforce branch coverage from an LCOV report.")
    parser.add_argument("report", type=pathlib.Path)
    parser.add_argument("--min-branches", type=float, required=True)
    args = parser.parse_args()

    if not 0.0 <= args.min_branches <= 100.0:
        raise SystemExit("--min-branches must be between 0 and 100")
    try:
        percent = branch_coverage_percent(args.report.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise SystemExit(str(error)) from error
    print(f"branch coverage: {percent:.2f}% (minimum {args.min_branches:.2f}%)")
    if percent + 1e-9 < args.min_branches:
        raise SystemExit(
            f"branch coverage {percent:.2f}% is below required {args.min_branches:.2f}%"
        )


if __name__ == "__main__":
    main()

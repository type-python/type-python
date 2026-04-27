from __future__ import annotations

import argparse
import dataclasses
import json
import pathlib
import re
from collections.abc import Iterable


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
DIAGNOSTICS_PATH = REPO_ROOT / "docs/diagnostics.md"
REPORT_PATH = REPO_ROOT / "docs/diagnostic-test-coverage.md"
CODE_RE = re.compile(r"TPY\d{4}")
REFERENCE_ROW_RE = re.compile(
    r"^\|\s*`(?P<code>TPY\d{4})`\s*\|\s*(?P<severity>[^|]+?)\s*\|\s*(?P<description>[^|]+?)\s*\|"
)


@dataclasses.dataclass(frozen=True)
class DiagnosticCoverage:
    code: str
    severity: str
    description: str
    implementation_files: tuple[str, ...]
    test_files: tuple[str, ...]

    @property
    def status(self) -> str:
        if self.implementation_files and self.test_files:
            return "covered"
        if self.implementation_files:
            return "needs-test"
        return "reserved"


def markdown_cell(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


def diagnostic_reference() -> list[tuple[str, str, str]]:
    rows: list[tuple[str, str, str]] = []
    for line in DIAGNOSTICS_PATH.read_text(encoding="utf-8").splitlines():
        match = REFERENCE_ROW_RE.match(line)
        if match is not None:
            rows.append(
                (
                    match.group("code"),
                    match.group("severity").strip(),
                    match.group("description").strip(),
                )
            )
    return rows


def rust_files() -> Iterable[pathlib.Path]:
    yield from sorted((REPO_ROOT / "crates").glob("**/*.rs"))


def is_test_file(path: pathlib.Path) -> bool:
    relative = path.relative_to(REPO_ROOT).as_posix()
    return "/tests/" in relative or path.name == "tests.rs" or path.parent.name == "tests"


def files_mentioning(code: str, *, tests: bool) -> tuple[str, ...]:
    files: list[str] = []
    for path in rust_files():
        if is_test_file(path) != tests:
            continue
        text = path.read_text(encoding="utf-8", errors="ignore")
        if code in text:
            files.append(path.relative_to(REPO_ROOT).as_posix())
    return tuple(files)


def coverage_rows() -> list[DiagnosticCoverage]:
    return [
        DiagnosticCoverage(
            code=code,
            severity=severity,
            description=description,
            implementation_files=files_mentioning(code, tests=False),
            test_files=files_mentioning(code, tests=True),
        )
        for code, severity, description in diagnostic_reference()
    ]


def render_markdown(rows: Iterable[DiagnosticCoverage]) -> str:
    rows = list(rows)
    covered = sum(1 for row in rows if row.status == "covered")
    needs_test = sum(1 for row in rows if row.status == "needs-test")
    reserved = sum(1 for row in rows if row.status == "reserved")
    lines = [
        "# Diagnostic Test Coverage",
        "",
        "This generated report maps every documented `TPYxxxx` diagnostic code to implementation files and Rust test files that mention the code. It is a traceability audit: `needs-test` means the code is emitted by implementation code but no Rust test currently names that diagnostic code directly.",
        "",
        f"Tracked codes: {len(rows)} ({covered} covered, {needs_test} need tests, {reserved} reserved).",
        "",
        "| Code | Severity | Status | Description | Implementation evidence | Test evidence |",
        "| ---- | -------- | ------ | ----------- | ----------------------- | ------------- |",
    ]
    for row in rows:
        implementation = "<br>".join(f"`{path}`" for path in row.implementation_files) or "missing"
        tests = "<br>".join(f"`{path}`" for path in row.test_files) or "missing"
        lines.append(
            f"| `{row.code}` | {markdown_cell(row.severity)} | {row.status} | {markdown_cell(row.description)} | {implementation} | {tests} |"
        )
    lines.append("")
    return "\n".join(lines)


def render_json(rows: Iterable[DiagnosticCoverage]) -> str:
    return json.dumps(
        [dataclasses.asdict(row) | {"status": row.status} for row in rows],
        indent=2,
        sort_keys=True,
    ) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate or check diagnostic test coverage.")
    parser.add_argument("--format", choices=["markdown", "json"], default="markdown")
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    rows = coverage_rows()
    rendered = render_json(rows) if args.format == "json" else render_markdown(rows)
    if args.write:
        REPORT_PATH.write_text(rendered, encoding="utf-8")
        return 0
    if args.check:
        expected = REPORT_PATH.read_text(encoding="utf-8")
        if expected != rendered:
            raise SystemExit(
                "docs/diagnostic-test-coverage.md is stale; run scripts/diagnostic_test_coverage.py --write"
            )
        return 0
    print(rendered, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

from __future__ import annotations

import argparse
import dataclasses
import json
import pathlib
import re
from collections.abc import Iterable


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
PLAN_PATH = REPO_ROOT / "docs/spec/conformance-and-test-plan-v1.md"
REPORT_PATH = REPO_ROOT / "docs/conformance-report.md"
SPEC_PATHS = (
    REPO_ROOT / "docs/spec/language-spec-v1.md",
    REPO_ROOT / "docs/spec/artifact-and-tooling-spec-v1.md",
    REPO_ROOT / "docs/spec/conformance-and-test-plan-v1.md",
    REPO_ROOT / "docs/spec/implementation-notes-v1.md",
)
FEATURE_ROW_RE = re.compile(r"^\|\s*(?P<feature>[^|]+?)\s*\|\s*(?P<tier>[^|]+?)\s*\|\s*(?P<status>MUST|SHOULD|MAY)\s*\|")
NORMATIVE_RE = re.compile(r"\bMUST(?:\s+NOT)?\b")


@dataclasses.dataclass(frozen=True)
class FeatureClaim:
    feature: str
    tier: str
    status: str
    tests: tuple[str, ...]


@dataclasses.dataclass(frozen=True)
class NormativeRule:
    rule_id: str
    document: str
    line: int
    requirement: str
    status: str
    tests: tuple[str, ...]


TEST_EVIDENCE: dict[str, tuple[str, ...]] = {
    "`.tpy` parsing for Core syntax": ("cargo test -p typepython-syntax",),
    "`.py` emission": ("cargo test -p typepython-lowering", "cargo test -p typepython-cli tests::pipeline"),
    "`.pyi` emission": ("cargo test -p typepython-emit", "cargo test -p typepython-cli tests::pipeline"),
    "`typealias`": ("cargo test -p typepython-lowering typealias",),
    "`interface`": ("cargo test -p typepython-lowering interface",),
    "`data class`": ("cargo test -p typepython-lowering data_class",),
    "`sealed class` with same-module closure": ("cargo test -p typepython-checking sealed",),
    "`overload def`": ("cargo test -p typepython-checking overload",),
    "`unsafe:`": ("cargo test -p typepython-checking unsafe",),
    ".tpy parsing for Core syntax": ("cargo test -p typepython-syntax",),
    ".py emission": ("cargo test -p typepython-lowering", "cargo test -p typepython-cli tests::pipeline"),
    ".pyi emission": ("cargo test -p typepython-emit", "cargo test -p typepython-cli tests::pipeline"),
    "typealias": ("cargo test -p typepython-lowering typealias",),
    "interface": ("cargo test -p typepython-lowering interface",),
    "data class": ("cargo test -p typepython-lowering data_class",),
    "sealed class with same-module closure": ("cargo test -p typepython-checking sealed",),
    "overload def": ("cargo test -p typepython-checking overload",),
    "unsafe:": ("cargo test -p typepython-checking unsafe",),
    "Generics with single upper bound": ("cargo test -p typepython-checking generics",),
    "Type-parameter defaults and constraint lists": ("cargo test -p typepython-checking advanced_generics",),
    "`ParamSpec` authoring including source-authored `P.args` / `P.kwargs` forwarding": ("cargo test -p typepython-checking paramspec",),
    "ParamSpec authoring including source-authored `P.args` / `P.kwargs` forwarding": ("cargo test -p typepython-checking paramspec",),
    "Recursive type aliases": ("cargo test -p typepython-checking recursive",),
    "Unions and literals": ("cargo test -p typepython-checking literal",),
    "Local inference": ("cargo test -p typepython-checking inference",),
    "Widened literal and container inference": ("cargo test -p typepython-checking widened",),
    "`Self` and receiver typing": ("cargo test -p typepython-checking receiver",),
    "Self and receiver typing": ("cargo test -p typepython-checking receiver",),
    "Callable compatibility and overload specificity": ("cargo test -p typepython-checking calls",),
    "Typed callable decorator transforms (callable-to-callable)": ("cargo test -p typepython-checking decorator",),
    "`TypedDict` literal checking in contextual positions": ("cargo test -p typepython-checking typed_dict",),
    "`TypedDict` `closed=` / `extra_items=` semantics": ("cargo test -p typepython-checking typed_dict",),
    "`Annotated`, `ClassVar`, `Required`, `NotRequired`, and `ReadOnly` in their supported positions": ("cargo test -p typepython-checking wrappers",),
    "`NewType` declarations and nominal compatibility": ("cargo test -p typepython-checking newtype",),
    "NewType declarations and nominal compatibility": ("cargo test -p typepython-checking newtype",),
    "Narrowing (`is None`, `isinstance`, `TypeGuard`/`TypeIs`, `assert`, `match`, boolean composition)": ("cargo test -p typepython-checking narrowing",),
    "Builtin decorator typing (`@property`, `@classmethod`, `@staticmethod`, `@final`, `@override`, `@deprecated`)": ("cargo test -p typepython-checking decorator",),
    "`dataclass_transform`-based dataclass-like framework typing": ("cargo test -p typepython-checking dataclass",),
    "Lambda parameter annotation sugar": ("cargo test -p typepython-syntax lambda",),
    "Authored async semantics in `.tpy` (`async def`, `await`, `async for`, `async with`, `yield`, `yield from`)": ("cargo test -p typepython-checking async",),
    "`with` statement typing": ("cargo test -p typepython-checking with",),
    "`for` loop and comprehension typing": ("cargo test -p typepython-checking for_loop",),
    "`try`/`except` exception variable typing": ("cargo test -p typepython-checking except",),
    "with statement typing": ("cargo test -p typepython-checking with",),
    "for loop and comprehension typing": ("cargo test -p typepython-checking for_loop",),
    "try`/`except` exception variable typing": ("cargo test -p typepython-checking except",),
    "Enum type support and enum member typing": ("cargo test -p typepython-checking enum",),
    "`Final` binding enforcement": ("cargo test -p typepython-checking final",),
    "Final binding enforcement": ("cargo test -p typepython-checking final",),
    "Abstract class and `@abstractmethod` checking": ("cargo test -p typepython-checking abstract",),
    "Implicit namespace packages / PEP 420 project modeling": ("cargo test -p typepython-cli collect_source_paths",),
    "PEP 561 typed-package and partial-stub resolution": ("cargo test -p typepython-cli external_resolution",),
    "`typing` / `typing_extensions` semantic equivalence for supported constructs": ("cargo test -p typepython-target",),
    "Target-version compatibility matrix for emitted typing constructs": ("cargo test -p typepython-cli target",),
    "Untyped import fallback (`unknown`/`dynamic`)": ("cargo test -p typepython-checking imports",),
    "Deterministic diagnostics": ("cargo test -p typepython-diagnostics",),
    "Cache invalidation": ("cargo test -p typepython-incremental",),
    "`TypedDict` utility transforms (`Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, `Required_`)": ("cargo test -p typepython-checking typed_dict",),
    "Public API completeness enforcement when configured": ("cargo test -p typepython-cli public_surface",),
    "Packaging artifact consistency rules for typed publication": ("cargo test -p typepython-cli tests::verification",),
    "`typepython verify` library publishability checks": ("cargo test -p typepython-cli tests::verification",),
}

RULE_EVIDENCE_PATTERNS: tuple[tuple[re.Pattern[str], tuple[str, ...]], ...] = tuple(
    (re.compile(pattern, re.IGNORECASE), tests)
    for pattern, tests in [
        (r"\.tpy|parser|type-expression grammar|syntax", TEST_EVIDENCE["`.tpy` parsing for Core syntax"]),
        (r"\.pyi|stub emitter|public API surface", TEST_EVIDENCE["`.pyi` emission"]),
        (r"\.py\b|runtime behavior|execution order|runtime semantics", TEST_EVIDENCE["`.py` emission"]),
        (r"typealias|type alias", TEST_EVIDENCE["`typealias`"]),
        (r"interface|Protocol", TEST_EVIDENCE["`interface`"]),
        (r"data class|dataclass_transform|dataclass", TEST_EVIDENCE["`data class`"]),
        (r"sealed", TEST_EVIDENCE["`sealed class` with same-module closure"]),
        (r"overload", TEST_EVIDENCE["`overload def`"]),
        (r"unsafe", TEST_EVIDENCE["`unsafe:`"]),
        (r"ParamSpec|Concatenate", TEST_EVIDENCE["ParamSpec authoring including source-authored `P.args` / `P.kwargs` forwarding"]),
        (r"recursive", TEST_EVIDENCE["Recursive type aliases"]),
        (r"Literal|union", TEST_EVIDENCE["Unions and literals"]),
        (r"inference|infer", TEST_EVIDENCE["Local inference"]),
        (r"Self|receiver", TEST_EVIDENCE["Self and receiver typing"]),
        (r"Callable|call-site", TEST_EVIDENCE["Callable compatibility and overload specificity"]),
        (r"TypedDict|ReadOnly|Required|NotRequired", TEST_EVIDENCE["`TypedDict` utility transforms (`Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, `Required_`)"]),
        (r"Annotated|ClassVar", TEST_EVIDENCE["`Annotated`, `ClassVar`, `Required`, `NotRequired`, and `ReadOnly` in their supported positions"]),
        (r"NewType", TEST_EVIDENCE["NewType declarations and nominal compatibility"]),
        (r"narrow|TypeGuard|TypeIs|assert|match", TEST_EVIDENCE["Narrowing (`is None`, `isinstance`, `TypeGuard`/`TypeIs`, `assert`, `match`, boolean composition)"]),
        (r"property|classmethod|staticmethod|final|override|deprecated", TEST_EVIDENCE["Builtin decorator typing (`@property`, `@classmethod`, `@staticmethod`, `@final`, `@override`, `@deprecated`)"]),
        (r"async|await|yield", TEST_EVIDENCE["Authored async semantics in `.tpy` (`async def`, `await`, `async for`, `async with`, `yield`, `yield from`)"]),
        (r"with statement|context manager", TEST_EVIDENCE["`with` statement typing"]),
        (r"for loop|comprehension", TEST_EVIDENCE["`for` loop and comprehension typing"]),
        (r"except|exception", TEST_EVIDENCE["`try`/`except` exception variable typing"]),
        (r"enum", TEST_EVIDENCE["Enum type support and enum member typing"]),
        (r"Final", TEST_EVIDENCE["`Final` binding enforcement"]),
        (r"abstract", TEST_EVIDENCE["Abstract class and `@abstractmethod` checking"]),
        (r"namespace package|module discovery|logical module", TEST_EVIDENCE["Implicit namespace packages / PEP 420 project modeling"]),
        (r"PEP 561|py\.typed|stub package|typed-package|partial-stub", TEST_EVIDENCE["PEP 561 typed-package and partial-stub resolution"]),
        (r"typing_extensions|typing semantic|target_python|target version", TEST_EVIDENCE["Target-version compatibility matrix for emitted typing constructs"]),
        (r"unknown|dynamic|untyped import", TEST_EVIDENCE["Untyped import fallback (`unknown`/`dynamic`)"]),
        (r"diagnostic|TPY\d+|severity", TEST_EVIDENCE["Deterministic diagnostics"]),
        (r"cache|incremental|invalidation|rechecking|summary", TEST_EVIDENCE["Cache invalidation"]),
        (r"verify|wheel|sdist|artifact|publication", TEST_EVIDENCE["`typepython verify` library publishability checks"]),
    ]
)


def feature_claims() -> list[FeatureClaim]:
    claims: list[FeatureClaim] = []
    for line in PLAN_PATH.read_text(encoding="utf-8").splitlines():
        match = FEATURE_ROW_RE.match(line)
        if match is None:
            continue
        feature = re.sub(r"\s+", " ", match.group("feature").strip())
        tier = match.group("tier").strip()
        status = match.group("status").strip()
        claims.append(FeatureClaim(feature, tier, status, TEST_EVIDENCE.get(feature, ())))
    return claims


def markdown_cell(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


def normalize_requirement(line: str) -> str:
    line = line.strip()
    line = re.sub(r"^[-*]\s+", "", line)
    line = re.sub(r"^\d+\.\s+", "", line)
    line = re.sub(r"\s+", " ", line)
    return line


def inferred_rule_tests(requirement: str) -> tuple[str, ...]:
    tests: list[str] = []
    for pattern, pattern_tests in RULE_EVIDENCE_PATTERNS:
        if pattern.search(requirement):
            tests.extend(pattern_tests)
    return tuple(dict.fromkeys(tests))


def normative_rules() -> list[NormativeRule]:
    rules: list[NormativeRule] = []
    for path in SPEC_PATHS:
        document = path.relative_to(REPO_ROOT).as_posix()
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            if NORMATIVE_RE.search(line) is None:
                continue
            requirement = normalize_requirement(line)
            tests = inferred_rule_tests(requirement)
            status = "mapped" if tests else "needs-mapping"
            rules.append(
                NormativeRule(
                    rule_id=f"{path.stem}:L{line_number}",
                    document=document,
                    line=line_number,
                    requirement=requirement,
                    status=status,
                    tests=tests,
                )
            )
    return rules


def render_markdown(claims: Iterable[FeatureClaim], rules: Iterable[NormativeRule]) -> str:
    claims = list(claims)
    rules = list(rules)
    mapped_rules = sum(1 for rule in rules if rule.tests)
    lines = [
        "# TypePython Conformance Report",
        "",
        "This report maps the feature matrix in `docs/spec/conformance-and-test-plan-v1.md` to test evidence commands. It is intentionally conservative: missing evidence is reported as `missing`, not inferred from nearby tests.",
        "",
        "It also extracts normative `MUST` and `MUST NOT` rules from `docs/spec/` and maps each rule to the nearest concrete test family when one can be inferred. Rules without evidence remain visible as `needs-mapping` so conformance gaps cannot disappear from review.",
        "",
        f"Feature claims: {len(claims)}. Normative rules: {len(rules)} ({mapped_rules} mapped, {len(rules) - mapped_rules} need mapping).",
        "",
        "## Feature Matrix Evidence",
        "",
        "| Feature | Tier | Requirement | Evidence |",
        "| ------- | ---- | ----------- | -------- |",
    ]
    for claim in claims:
        evidence = "<br>".join(f"`{test}`" for test in claim.tests) if claim.tests else "missing"
        lines.append(f"| {markdown_cell(claim.feature)} | {claim.tier} | {claim.status} | {evidence} |")
    lines.extend([
        "",
        "## Normative MUST Traceability",
        "",
        "| Rule | Source | Status | Requirement | Evidence |",
        "| ---- | ------ | ------ | ----------- | -------- |",
    ])
    for rule in rules:
        evidence = "<br>".join(f"`{test}`" for test in rule.tests) if rule.tests else "missing"
        source = f"`{rule.document}:{rule.line}`"
        lines.append(
            f"| `{rule.rule_id}` | {source} | {rule.status} | {markdown_cell(rule.requirement)} | {evidence} |"
        )
    lines.append("")
    return "\n".join(lines)


def render_json(claims: Iterable[FeatureClaim], rules: Iterable[NormativeRule]) -> str:
    payload = {
        "feature_claims": [dataclasses.asdict(claim) for claim in claims],
        "normative_rules": [dataclasses.asdict(rule) for rule in rules],
    }
    return json.dumps(payload, indent=2, sort_keys=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate or check TypePython conformance mapping.")
    parser.add_argument("--format", choices=["markdown", "json"], default="markdown")
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    claims = feature_claims()
    rules = normative_rules()
    rendered = render_json(claims, rules) if args.format == "json" else render_markdown(claims, rules)
    if args.write:
        REPORT_PATH.write_text(rendered, encoding="utf-8")
        return 0
    if args.check:
        expected = REPORT_PATH.read_text(encoding="utf-8")
        if expected != rendered:
            raise SystemExit("docs/conformance-report.md is stale; run scripts/conformance_report.py --write")
        missing_must = [claim.feature for claim in claims if claim.status == "MUST" and not claim.tests]
        if missing_must:
            joined = ", ".join(missing_must)
            raise SystemExit(f"missing conformance evidence for MUST feature(s): {joined}")
        if not rules:
            raise SystemExit("no normative MUST rules found in docs/spec")
        return 0
    print(rendered, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

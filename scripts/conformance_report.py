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
FEATURE_ROW_RE = re.compile(r"^\|\s*(?P<feature>[^|]+?)\s*\|\s*(?P<tier>[^|]+?)\s*\|\s*(?P<status>MUST|SHOULD|MAY)\s*\|")


@dataclasses.dataclass(frozen=True)
class FeatureClaim:
    feature: str
    tier: str
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


def render_markdown(claims: Iterable[FeatureClaim]) -> str:
    lines = [
        "# TypePython Conformance Report",
        "",
        "This report maps the feature matrix in `docs/spec/conformance-and-test-plan-v1.md` to test evidence commands. It is intentionally conservative: missing evidence is reported as `missing`, not inferred from nearby tests.",
        "",
        "| Feature | Tier | Requirement | Evidence |",
        "| ------- | ---- | ----------- | -------- |",
    ]
    for claim in claims:
        evidence = "<br>".join(f"`{test}`" for test in claim.tests) if claim.tests else "missing"
        lines.append(f"| {claim.feature} | {claim.tier} | {claim.status} | {evidence} |")
    lines.append("")
    return "\n".join(lines)


def render_json(claims: Iterable[FeatureClaim]) -> str:
    return json.dumps([dataclasses.asdict(claim) for claim in claims], indent=2, sort_keys=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate or check TypePython conformance mapping.")
    parser.add_argument("--format", choices=["markdown", "json"], default="markdown")
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    claims = feature_claims()
    rendered = render_json(claims) if args.format == "json" else render_markdown(claims)
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
        return 0
    print(rendered, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

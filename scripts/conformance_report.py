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


@dataclasses.dataclass(frozen=True)
class SemanticSubRule:
    area: str
    rule: str
    tests: tuple[str, ...]


BETA_SCOPE_NOTES: dict[str, str] = {
    "mapped": "Evidence command(s) cover this rule for the Core v1 Beta claim.",
    "meta-rule": "Spec governance or terminology rule; non-blocking for Core v1 Beta runtime/tool behavior.",
    "not-claimed-beta": "Not part of the externally visible Core v1 Beta compatibility claim.",
}


SEMANTIC_SUBRULE_EVIDENCE: tuple[SemanticSubRule, ...] = (
    SemanticSubRule(
        "unknown",
        "member access on `unknown` is diagnosed as TPY4003",
        ("cargo test -p typepython-checking check_reports_unknown_member_access",),
    ),
    SemanticSubRule(
        "unknown",
        "direct call of `unknown` is diagnosed as TPY4003",
        (
            "cargo test -p typepython-checking check_reports_unknown_direct_call_keyword",
            "cargo test -p typepython-checking check_reports_unknown_direct_call_on_import",
        ),
    ),
    SemanticSubRule(
        "unknown",
        "method call on `unknown` is diagnosed as TPY4003",
        ("cargo test -p typepython-checking check_reports_unknown_method_call",),
    ),
    SemanticSubRule(
        "unknown",
        "subscript/indexing on `unknown` is diagnosed as TPY4003",
        ("cargo test -p typepython-checking check_reports_unknown_subscript_from_real_parse_pipeline",),
    ),
    SemanticSubRule(
        "unknown",
        "binary arithmetic with either operand `unknown` is diagnosed as TPY4003",
        ("cargo test -p typepython-checking check_reports_unknown_arithmetic_from_real_parse_pipeline",),
    ),
    SemanticSubRule(
        "unknown",
        "`unknown` cannot flow into concrete parameters or returns without narrowing",
        (
            "cargo test -p typepython-checking check_reports_unknown_call_argument_to_concrete_parameter",
            "cargo test -p typepython-checking check_reports_unknown_return_to_concrete_type",
        ),
    ),
    SemanticSubRule(
        "unknown",
        "`unknown` assignment into concrete or `Any` targets is rejected",
        (
            "cargo test -p typepython-checking check_reports_unknown_assignment_to_concrete_type",
            "cargo test -p typepython-checking check_reports_unknown_assignment_to_any",
        ),
    ),
    SemanticSubRule(
        "unknown",
        "explicit casts make subsequent supported operations legal",
        (
            "cargo test -p typepython-checking check_accepts_unknown_after_explicit_cast",
            "cargo test -p typepython-checking check_accepts_unknown_subscript_and_arithmetic_after_explicit_cast",
        ),
    ),
    SemanticSubRule(
        "no_implicit_dynamic",
        "unannotated function and method parameters are diagnosed when enabled",
        (
            "cargo test -p typepython-checking check_reports_implicit_dynamic_function_and_method_params_when_enabled",
        ),
    ),
    SemanticSubRule(
        "no_implicit_dynamic",
        "explicit `dynamic` remains accepted when the option is enabled",
        ("cargo test -p typepython-checking check_accepts_explicit_dynamic_when_no_implicit_dynamic_is_enabled",),
    ),
    SemanticSubRule(
        "no_implicit_dynamic",
        "uncontextualized lambda parameters are diagnosed",
        (
            "cargo test -p typepython-checking check_reports_uncontextualized_lambda_param_when_no_implicit_dynamic_is_enabled",
        ),
    ),
    SemanticSubRule(
        "no_implicit_dynamic",
        "contextual callable lambda parameters are accepted",
        (
            "cargo test -p typepython-checking check_accepts_contextual_lambda_param_when_no_implicit_dynamic_is_enabled",
        ),
    ),
    SemanticSubRule(
        "strict_nulls",
        "`None` assignment into non-optional targets is diagnosed when enabled",
        ("cargo test -p typepython-checking check_reports_none_assignment_when_strict_nulls_is_enabled",),
    ),
    SemanticSubRule(
        "strict_nulls",
        "`None` call arguments for non-optional parameters are diagnosed when enabled",
        ("cargo test -p typepython-checking check_reports_none_call_argument_when_strict_nulls_is_enabled",),
    ),
    SemanticSubRule(
        "strict_nulls",
        "`None` returns for non-optional return types are diagnosed when enabled",
        ("cargo test -p typepython-checking check_reports_none_return_when_strict_nulls_is_enabled",),
    ),
    SemanticSubRule(
        "strict_nulls",
        "compatibility mode allows selected `None` flows when strict nulls are disabled",
        ("cargo test -p typepython-checking strict_nulls",),
    ),
    SemanticSubRule(
        "sealed",
        "non-exhaustive sealed matches name missing subclasses",
        ("cargo test -p typepython-checking check_reports_non_exhaustive_sealed_match",),
    ),
    SemanticSubRule(
        "sealed",
        "sealed matches with explicit coverage or wildcard are accepted",
        ("cargo test -p typepython-checking check_accepts_exhaustive_sealed_match_with_wildcard",),
    ),
    SemanticSubRule(
        "enum",
        "non-exhaustive enum matches name missing members",
        (
            "cargo test -p typepython-checking check_reports_non_exhaustive_enum_match",
            "cargo test -p typepython-checking check_reports_non_exhaustive_enum_match_missing_member",
        ),
    ),
    SemanticSubRule(
        "enum",
        "exhaustive enum matches are accepted",
        ("cargo test -p typepython-checking check_accepts_enum_exhaustive_match",),
    ),
    SemanticSubRule(
        "TypedDict",
        "contextual literals reject missing keys, unknown keys, and value mismatches",
        (
            "cargo test -p typepython-checking check_reports_missing_required_typed_dict_key",
            "cargo test -p typepython-checking check_reports_unknown_typed_dict_key",
            "cargo test -p typepython-checking check_reports_incompatible_typed_dict_value",
        ),
    ),
    SemanticSubRule(
        "TypedDict",
        "`Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, and `Required_` lower to standard stubs",
        ("cargo test -p typepython-lowering transform",),
    ),
    SemanticSubRule(
        "public API emit",
        "typed publication and verify failures cover missing or divergent artifacts",
        ("cargo test -p typepython-cli tests::verification",),
    ),
    SemanticSubRule(
        "downstream compatibility",
        "emitted artifacts are consumed by the downstream checker matrix",
        ("python3 scripts/downstream_checker_smoke.py",),
    ),
)


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
    "Unknown and dynamic boundary assignability": ("cargo test -p typepython-checking unknown",),
    "No implicit dynamic fallback": ("cargo test -p typepython-checking no_implicit_dynamic",),
    "Closed configuration schema": ("cargo test -p typepython-config rejects_unknown",),
    "Strict null compatibility modes": ("cargo test -p typepython-checking strict_nulls",),
    "Deterministic diagnostics": ("cargo test -p typepython-diagnostics",),
    "Cache invalidation": ("cargo test -p typepython-incremental",),
    "Materialized output reuse": ("cargo test -p typepython-cli materialized",),
    "`TypedDict` utility transforms (`Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, `Required_`)": ("cargo test -p typepython-checking typed_dict",),
    "Public API completeness enforcement when configured": ("cargo test -p typepython-cli public_surface",),
    "Packaging artifact consistency rules for typed publication": ("cargo test -p typepython-cli tests::verification",),
    "`typepython verify` library publishability checks": ("cargo test -p typepython-cli tests::verification",),
    "Sealed exhaustiveness": ("cargo test -p typepython-checking sealed",),
    "Runtime validator emission for selected data-class trust boundaries": (
        "cargo test -p typepython-emit write_runtime_outputs",
        "cargo test -p typepython-emit write_runtime_outputs_honors_named_validation_boundary_kinds",
        "cargo test -p typepython-config loads_all_supported_typepython_toml_configuration_fields",
    ),
    "`typepython lsp`": ("cargo test -p typepython-lsp",),
    "typepython lsp": ("cargo test -p typepython-lsp",),
    "`typepython migrate --report`": ("cargo test -p typepython-cli build_migration_report",),
    "typepython migrate --report": ("cargo test -p typepython-cli build_migration_report",),
    "Stable JSON diagnostic output": ("cargo test -p typepython-diagnostics", "cargo test -p typepython-cli"),
    "`typepython migrate` stub-generation workflows that do not affect authoritative public surfaces": ("cargo test -p typepython-cli emit_migration_stubs",),
    "typepython migrate stub-generation workflows that do not affect authoritative public surfaces": ("cargo test -p typepython-cli emit_migration_stubs",),
    "Optional `.pyc` generation": ("cargo test -p typepython-cli run_verify_bootstraps_bytecode_after_clean_when_emit_pyc_is_enabled",),
}

VALIDATION_CHECKS: tuple[str, ...] = (
    "cargo fmt --check",
    "cargo test -p typepython-emit write_runtime_outputs_honors_named_validation_boundary_kinds",
    "cargo test -p typepython-config loads_all_supported_typepython_toml_configuration_fields",
    "python3 scripts/conformance_report.py --check",
    "cargo test -p typepython-lsp code_actions_offer",
    "cargo test -p typepython-emit write_runtime_outputs",
    "cargo test -p typepython-cli run_pipeline_invalidates_cache_when_runtime_validators_change",
    "python3 scripts/diagnostic_test_coverage.py --check",
    "python3 -m unittest scripts.test_repo_contracts scripts.test_annotation_compat scripts.test_downstream_checker_matrix",
)

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
        (r"typing_extensions|typing semantic|target_python|target version|target-version|compatibility matrix|emit_style|typeshed snapshot|standard-library type source", TEST_EVIDENCE["Target-version compatibility matrix for emitted typing constructs"]),
        (r"no_implicit_dynamic|fallback to `dynamic`|fallback to dynamic", TEST_EVIDENCE["No implicit dynamic fallback"]),
        (r"unknown config keys|configuration schema is closed|unknown top-level tables|unknown keys inside a recognized table", TEST_EVIDENCE["Closed configuration schema"]),
        (r"unknown.*assign|assign.*unknown|dynamic.*assign|assign.*dynamic|operations involving `unknown`|member access.*unknown", TEST_EVIDENCE["Unknown and dynamic boundary assignability"]),
        (r"untyped import|imports =|imported values", TEST_EVIDENCE["Untyped import fallback (`unknown`/`dynamic`)"]),
        (r"strict_nulls|strict null|assignment-compatible with any type except|ordinary member access.*None", TEST_EVIDENCE["Strict null compatibility modes"]),
        (r"diagnostic|compile errors|TPY\d+|severity", TEST_EVIDENCE["Deterministic diagnostics"]),
        (r"JSON|json", ("cargo test -p typepython-diagnostics", "cargo test -p typepython-cli")),
        (r"materialized build outputs|output-affecting config|emitted artifact plan", TEST_EVIDENCE["Materialized output reuse"]),
        (r"cache|incremental|invalidation|rechecking|summary", TEST_EVIDENCE["Cache invalidation"]),
        (r"verify|wheel|sdist|artifact|publication", TEST_EVIDENCE["`typepython verify` library publishability checks"]),
        (r"normal Python interpreter|mandatory TypePython runtime|no mandatory TypePython runtime", TEST_EVIDENCE["`.py` emission"]),
        (r"Generics with single upper bound", TEST_EVIDENCE["Generics with single upper bound"]),
        (r"runtime validator|validation boundary|boundaries|__tpy_validate__", TEST_EVIDENCE["Runtime validator emission for selected data-class trust boundaries"]),
        (r"default.*type parameter|type parameter.*default|following type parameter|Type-parameter defaults and constraint lists", TEST_EVIDENCE["Type-parameter defaults and constraint lists"]),
        (r"assignable|assignment|RHS type|value type", ("cargo test -p typepython-checking assignments", "cargo test -p typepython-checking typed_dict")),
        (r"Optionality|T\?|None guard|None`, `None|short-circuit", TEST_EVIDENCE["Narrowing (`is None`, `isinstance`, `TypeGuard`/`TypeIs`, `assert`, `match`, boolean composition)"]),
        (r"Transform|Partial|Pick|Omit|Readonly|compile-time-only type operators", TEST_EVIDENCE["`TypedDict` utility transforms (`Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, `Required_`)"]),
        (r"Widening|widening", TEST_EVIDENCE["Widened literal and container inference"]),
        (r"simple name|duplicate|same body|declare both", ("cargo test -p typepython-binding", "cargo test -p typepython-checking")),
        (r"hard keyword|soft keyword", TEST_EVIDENCE["`.tpy` parsing for Core syntax"]),
        (r"synthesis|dataclass-like|constructor", TEST_EVIDENCE["`dataclass_transform`-based dataclass-like framework typing"]),
        (
            r"call expression|call site|call-site|callable type|source callable|target callable|concrete signature is applicable|applicable signature|keyword argument|starred argument|argument list|applicable overload|overload",
            TEST_EVIDENCE["Callable compatibility and overload specificity"],
        ),
        (r"indexing by a statically known undeclared key", TEST_EVIDENCE["`TypedDict` literal checking in contextual positions"]),
        (r"import|resolution|type_roots|stub package|partial stub", ("cargo test -p typepython-cli external_resolution", "cargo test -p typepython-checking imports")),
        (r"project path|search upward|src`|profile names|out_dir|cache_dir|exclude|ignored", ("cargo test -p typepython-config", "cargo test -p typepython-cli tests::pipeline")),
        (r"public contract|executable bodies|Function bodies|Stub declarations", TEST_EVIDENCE["`.pyi` emission"]),
        (r"Lowering MUST produce|lowerer MUST|lowering map|mapping segment|lowering metadata|deterministic serialization", TEST_EVIDENCE["`.py` emission"]),
        (r"accept the grammar above|index signature shorthand|__getitem__|__setitem__", ("cargo test -p typepython-syntax", "cargo test -p typepython-checking")),
        (r"Lambda parameter annotation sugar", TEST_EVIDENCE["Lambda parameter annotation sugar"]),
        (r"`with` statement typing|with statement typing", TEST_EVIDENCE["with statement typing"]),
        (r"Public API completeness enforcement", TEST_EVIDENCE["Public API completeness enforcement when configured"]),
        (r"require_known_public_types|public surface is not type-complete", TEST_EVIDENCE["Public API completeness enforcement when configured"]),
        (r"concrete codes|reserve the following concrete codes", TEST_EVIDENCE["Deterministic diagnostics"]),
        (r"public summary|isPackageEntry|exports|standardized export|serialized in lexicographic|direct_changes|build up to date|rebuild or re-emit", TEST_EVIDENCE["Cache invalidation"]),
        (r"It MUST at minimum|Runtime verification in v1|byte-for-byte equivalence", TEST_EVIDENCE["`typepython verify` library publishability checks"]),
        (r"multiple compatible spellings|same choice for equivalent declarations", TEST_EVIDENCE["Target-version compatibility matrix for emitted typing constructs"]),
        (r"Core v1 MUST support|Core v1 supports|MUST support", ("cargo test -p typepython-syntax", "cargo test -p typepython-checking")),
    ]
)

META_RULE_PATTERNS: tuple[re.Pattern[str], ...] = tuple(
    re.compile(pattern, re.IGNORECASE)
    for pattern in [
        r"RFC 2119|words \*\*MUST|MUST indicates|MUST NOT indicates",
        r"implementation-defined|host-defined|document the dependency",
        r"scope|terminology|governance|specification error",
        r"v1-conformant|Core v1 conformance|test suite MUST|externally visible `MUST` rule",
    ]
)

NOT_CLAIMED_BETA_PATTERNS: tuple[re.Pattern[str], ...] = tuple(
    re.compile(pattern, re.IGNORECASE)
    for pattern in [
        r"Experimental v1|experimental feature|prototype|not part of the Beta|outside conformance",
        r"runtime validator|generated validator|validator generation|The validator MUST|conditional return|case arm pattern|infer_passthrough",
        r"LSP support is a DX v1 feature|watch|migration|migrate|formatter|formatting|cache internal|snapshot schema",
        r"add fields.*deterministic ordering.*hashing|versioned extension fields",
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


def beta_rule_status(requirement: str, tests: tuple[str, ...]) -> str:
    if any(pattern.search(requirement) for pattern in META_RULE_PATTERNS):
        return "meta-rule"
    if any(pattern.search(requirement) for pattern in NOT_CLAIMED_BETA_PATTERNS):
        return "not-claimed-beta"
    if tests:
        return "mapped"
    return "needs-mapping"


def normative_rules() -> list[NormativeRule]:
    rules: list[NormativeRule] = []
    for path in SPEC_PATHS:
        document = path.relative_to(REPO_ROOT).as_posix()
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            if NORMATIVE_RE.search(line) is None:
                continue
            requirement = normalize_requirement(line)
            tests = inferred_rule_tests(requirement)
            status = beta_rule_status(requirement, tests)
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
    mapped_rules = sum(1 for rule in rules if rule.status == "mapped")
    needs_mapping = sum(1 for rule in rules if rule.status == "needs-mapping")
    non_blocking = sum(1 for rule in rules if rule.status in {"meta-rule", "not-claimed-beta"})
    lines = [
        "# TypePython Conformance Report",
        "",
        "This report maps the feature matrix in `docs/spec/conformance-and-test-plan-v1.md` to test evidence commands. It is intentionally conservative: missing evidence is reported as `missing`, not inferred from nearby tests.",
        "",
        "It also extracts normative `MUST` and `MUST NOT` rules from `docs/spec/` and maps each externally visible Core v1 Beta rule to the nearest concrete test family when one can be inferred. Rules outside the Core v1 Beta compatibility claim remain visible as `meta-rule` or `not-claimed-beta` rather than disappearing from review.",
        "",
        f"Feature claims: {len(claims)}. Normative rules: {len(rules)} ({mapped_rules} mapped, {non_blocking} non-blocking for Core v1 Beta, {needs_mapping} need mapping).",
        "",
        "## Beta Scope Summary",
        "",
        "Core v1 is the Beta compatibility claim. DX v1 and Experimental v1 features may ship in the package, but their UX details, adapter manifests, runtime validators, migration heuristics, conditional returns, pass-through inference, and cache internals are not compatibility-stable unless a later document explicitly promotes them.",
        "",
        "| Status | Meaning |",
        "| ------ | ------- |",
    ]
    lines.extend(
        f"| `{status}` | {markdown_cell(note)} |" for status, note in BETA_SCOPE_NOTES.items()
    )
    lines.extend([
        "",
        "## Feature Matrix Evidence",
        "",
        "| Feature | Tier | Requirement | Evidence |",
        "| ------- | ---- | ----------- | -------- |",
    ])
    for claim in claims:
        evidence = "<br>".join(f"`{test}`" for test in claim.tests) if claim.tests else "missing"
        lines.append(f"| {markdown_cell(claim.feature)} | {claim.tier} | {claim.status} | {evidence} |")
    lines.extend([
        "",
        "## Semantic Sub-Rule Evidence",
        "",
        "This table expands broad Core v1 feature claims into the specific semantic cases that are easiest to over-map accidentally. Each row points to a concrete test filter or smoke command rather than only a broad crate-level family.",
        "",
        "| Area | Sub-rule | Evidence |",
        "| ---- | -------- | -------- |",
    ])
    for subrule in SEMANTIC_SUBRULE_EVIDENCE:
        evidence = "<br>".join(f"`{test}`" for test in subrule.tests) if subrule.tests else "missing"
        lines.append(
            f"| {markdown_cell(subrule.area)} | {markdown_cell(subrule.rule)} | {evidence} |"
        )
    lines.extend([
        "",
        "## Current Validation Evidence",
        "",
        "These commands are the exact post-remediation checks used to validate the current strategic roadmap closure:",
        "",
    ])
    lines.extend(f"- `{check}`" for check in VALIDATION_CHECKS)
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
        "semantic_subrules": [
            dataclasses.asdict(subrule) for subrule in SEMANTIC_SUBRULE_EVIDENCE
        ],
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
        missing_subrules = [
            f"{subrule.area}: {subrule.rule}"
            for subrule in SEMANTIC_SUBRULE_EVIDENCE
            if not subrule.tests
        ]
        if missing_subrules:
            joined = ", ".join(missing_subrules)
            raise SystemExit(f"missing semantic sub-rule evidence: {joined}")
        needs_mapping = [rule.rule_id for rule in rules if rule.status == "needs-mapping"]
        if needs_mapping:
            joined = ", ".join(needs_mapping)
            raise SystemExit(f"normative MUST rule(s) need mapping: {joined}")
        if not rules:
            raise SystemExit("no normative MUST rules found in docs/spec")
        return 0
    print(rendered, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

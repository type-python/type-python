# Maintainability Plan

This page records engineering debt that is real but not a Core v1.0 RC blocker. It is meant to keep
post-RC refactors explicit, scoped, and testable.

## Current Hotspots

| Area | Current shape | Risk |
| ---- | ------------- | ---- |
| Checker file size | `semantic.rs` 3468 LOC, `assignments.rs` 2387 LOC, `calls/call_diagnostics.rs` 1979 LOC | Review friction, merge conflicts, and slower onboarding around the highest-risk semantic code. |
| Syntax extraction passes | `syntax_parts/extraction/` keeps separate passes for calls, control flow, expression metadata, guards, lambdas, and syntax extensions | Multiple full AST walks are simple and correct today, but large modules pay repeated traversal cost and new metadata can be added inconsistently. |

These are maintenance risks rather than correctness failures. The current RC gate still requires
formatting, clippy, unit tests, conformance coverage, downstream-checker smoke, example smoke,
incremental recovery tests, and release-evidence checks before tagging.

## Checker Split Plan

The checker should be split along existing semantic ownership boundaries, not by moving code into
generic utility modules.

1. Move declaration-to-semantic conversion and cache helpers out of `semantic.rs` into a focused
   `semantic/facts.rs` or equivalent module. The public surface should stay behind the current
   checker APIs until callers need a narrower boundary.
2. Move flow/narrowing helpers into a module that owns guard environments, `isinstance` handling,
   match exhaustiveness inputs, and persistent local narrowing rules.
3. Keep generic solving in `generic_solver.rs`; do not interleave solver internals with diagnostic
   rendering while splitting call code.
4. Split `assignments.rs` by responsibility: local/value assignment, attribute assignment, destructuring,
   TypedDict mutation, and contextual inference. Each moved slice should carry its adjacent tests or
   add a targeted regression before the move.
5. Split `calls/call_diagnostics.rs` only after direct-call candidate resolution and diagnostic
   rendering are cleanly named. The target shape is candidate construction, applicability, specificity,
   and rendering as separate modules.

Acceptance criteria for each checker split:

- no public API expansion unless a downstream crate needs it
- `cargo test -p typepython-checking` passes after every slice
- `cargo test -p typepython-cli` passes when a slice touches CLI-visible diagnostics
- diagnostic code identity and message intent remain stable unless the commit explicitly updates docs
- benchmark noise is checked with the checker Criterion suite before and after larger moves

## Extraction Visitor Plan

The syntax extraction crate can keep today's separate extractors through Core v1.0 RC. A unified
visitor is worth doing when it removes repeated traversal without blurring ownership.

Target design:

- One AST walk collects an `ExtractionBundle` containing declaration-adjacent syntax metadata, calls,
  method calls, member accesses, returns, yields, guards, asserts, matches, for-loops, with-statements,
  assignments, lambdas, and unsafe sites.
- Existing extractor functions remain as compatibility adapters during migration so binding code does
  not need a large synchronized rewrite.
- The visitor must preserve source-order stability for all site vectors. Snapshot and downstream
  diagnostics depend on deterministic ordering.
- Syntax-extension validation remains explicit. The unified visitor should not make TypePython-only
  soft-keyword handling harder to audit.
- Large-module benchmarks should compare old and new traversal counts before the old adapters are
  deleted.

Migration order:

1. Introduce the visitor and populate one low-risk metadata family, such as unsafe operation sites.
2. Add parity tests that compare the visitor output with the existing extractor output for representative
   `.tpy`, `.py`, and `.pyi` fixtures.
3. Move calls and member accesses next, because checker and LSP consumers depend on their ordering.
4. Move control-flow and narrowing sites last, because they have the most diagnostic blast radius.
5. Delete old independent walkers only after all parity tests and downstream CLI smoke remain green.

## Release Posture

Core v1.0 RC can ship with these hotspots documented because the current risk is maintainability, not
an unbounded user-visible contract. The follow-up requirement is to avoid adding major new checker
features or syntax metadata families on top of the current large-file and multi-pass shape without first
splitting the relevant owner.

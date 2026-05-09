# TypePython Feature Status

TypePython ships several surfaces in one package, but not every shipped surface has the same
compatibility promise. User-facing claims, conformance reports, release notes, and roadmap examples
should use the status vocabulary below. Shipped-but-unstable features are tracked in the
[Experimental Feature Registry](experimental-features.md).

## Status Vocabulary

| Status | Meaning | May appear as a primary README differentiator? |
| ------ | ------- | ---------------------------------------------- |
| Stable Core v1 | The source syntax or configuration, author-package checker semantics, diagnostic code identity, emitted `.py` / `.pyi` shape, and interop boundary are part of the Core v1 Beta compatibility promise. | Yes, when author-time scope and external boundary are visible. |
| Supported DX, non-stable | The feature is implemented and useful, but command UX, JSON shape, heuristics, or editor behavior may change during Beta. | Only as toolchain/DX, with the non-stable status visible. |
| Experimental opt-in | The feature is outside Core v1 conformance, requires an explicit opt-in flag or config setting, and must not change Core v1 behavior when disabled. | No; document it in Experimental sections only. |
| Roadmap / prototype | The feature is a design direction, RFC slice, adapter prototype, or example fixture without a compatibility promise. | No; it must not be presented as a completed advantage over mypy, pyright, or PEP 695. |

## Stable Core v1 Examples

| Feature | Stable promise | Output / interop boundary |
| ------- | -------------- | ------------------------- |
| `sealed class` closure and sealed-match exhaustiveness | Same-module sealed roots are checked by the TypePython checker, and incomplete supported `match` coverage reports stable diagnostic `TPY4009`. | Emitted `.py` / `.pyi` stay standard Python. External checkers see a normal class hierarchy and do not preserve sealed exhaustiveness. |
| `unknown` | Values typed as `unknown` must be narrowed before ordinary use in TypePython checking. | `unknown` lowers to standard `object` in emitted stubs. |
| `unsafe:` | Unsafe operations can be fenced and audited by TypePython diagnostics. | The block is lowered to ordinary Python with no TypePython runtime requirement. |
| `TypedDict` transforms | Supported transforms such as `Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, and `Required_` expand deterministically. | Emitted stubs contain standard `TypedDict` forms. |
| `interface`, `data class`, `typealias`, and source-authored generics | Source forms and lowering rules are part of Core v1. | Emitted output uses standard `Protocol`, `dataclass`, `TypeAlias`, `TypeVar`, and target-compatible typing constructs. |

Stable Core v1 checker semantics are guarantees for the package while it is checked by TypePython. They are not a promise that ordinary downstream `.py` users inherit TypePython-only facts from the emitted `.pyi` surface. Public-facing claims should say "author-time" or "TypePython-checked package" when they describe sealed exhaustiveness, `unknown` strictness, or `unsafe:` fences.

## Core v1.0 RC Boundary

When the package is labeled Core v1.0 RC, only the Stable Core v1 row is promoted to release-candidate status. Supported DX, Experimental opt-in, and Roadmap / prototype features keep their existing labels even if they are included in the same wheel. Release notes must say "Core v1.0 RC" rather than unqualified "v1.0 RC" unless the DX gate in `docs/dx-stability.md` is complete.

## Supported DX, Non-Stable Examples

The following features are usable in Beta builds, but their UX and machine-readable output are not
compatibility-stable:

- `typepython watch`
- `typepython lsp` editor details, command names, and code-action shapes
- `typepython compat`, `api-diff`, and `type-health` scoring details
- migration reports, baselines, and adoption dashboards
- checker portability profiles and type-budget heuristics

## Experimental Opt-In Examples

Experimental features must be disabled by default for ordinary Core v1 builds. The canonical feature
ids, gates, and promotion requirements live in the
[Experimental Feature Registry](experimental-features.md):

- runtime validator emission
- conditional return syntax
- pass-through `.py` inference and shadow-stub generation
- sync/async dual emit paths
- shape projections beyond `TypedDict`

## Roadmap / Prototype Examples

These items are useful for research, fixtures, or ecosystem design, but they are not compatibility
claims:

- framework adapter manifests and SDK details
- effect/capability rows beyond the stable `unsafe:` fence
- taint facts and sanitizer/source/sink vocabularies
- validator witnesses
- notebook ingestion
- future type-level algebra and shape algebra work

When a roadmap, prototype, or experimental item graduates, the promoting change must update this
document, `docs/experimental-features.md`, `docs/beta-readiness.md`, the conformance feature matrix
when applicable, and any README claims in the same commit series.

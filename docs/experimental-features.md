# TypePython Experimental Feature Registry

This registry is the canonical list of shipped-but-unstable features. A feature listed here is not
part of the Core v1 compatibility promise unless a later promotion change updates this registry,
`docs/feature-status.md`, `docs/beta-readiness.md`, the conformance matrix when applicable, and any
README or PyPI claims in the same commit series.

Experimental opt-in features share the same release contract:

- disabled by default for ordinary Core v1 projects
- explicit opt-in before they can affect parsing, checking, lowering, emit, or diagnostics
- no change to Core v1 program acceptance when the feature is disabled
- No primary README differentiator unless and until the feature is promoted
- no compatibility-stable JSON, LSP, runtime, or emitted-surface contract beyond the documented
  narrow slice

Roadmap / prototype rows are tracked here so they stay out of Core v1 product claims. They may appear
in research fixtures, RFCs, or explicitly labeled diagnostics, but they are disabled for ordinary
Core projects unless `[experimental].accepted_features` names the feature id. Promotion still
requires tightening those gates before the slice can become a compatibility-stable feature.

## Registry

| Feature id | Status | Current gate | Core v1 impact while disabled | Promotion requirements |
| ---------- | ------ | ------------ | ----------------------------- | ---------------------- |
| `runtime_validators` | Experimental opt-in | `[experimental].accepted_features = ["runtime_validators"]` plus `[emit].runtime_validators = true` or `[[boundaries]]` | No `__tpy_validate__` methods are emitted, and public `.pyi` output remains unchanged. | Replace source-text scanning with semantic boundary metadata, prove every supported field annotation is checked or rejected, make error shape stable, and add downstream/runtime conformance fixtures. |
| `conditional_returns` | Experimental opt-in | `[experimental].accepted_features = ["conditional_returns"]` plus `[typing].conditional_returns = true` | Conditional-return syntax is rejected by the Core parser path. | Complete syntax, checking, lowering, diagnostics, and conformance rules for all supported control-flow forms. |
| `infer_passthrough` | Experimental opt-in | `[experimental].accepted_features = ["infer_passthrough"]` plus `[typing].infer_passthrough = true` | `.py` sources stay pass-through and do not synthesize shadow stubs. | Prove inference cache invalidation, public-surface stability, and checker-neutral stub generation across large real-world fixtures. |
| `sync_async_dual_emit` | Experimental opt-in | Explicit sync/async mapping inputs and downstream-checker fixture coverage | Ordinary emit does not synthesize paired sync/async APIs. | Stabilize mapping schema, conflict diagnostics, incremental cache keys, and downstream checker compatibility. |
| `shape_transforms` | Experimental opt-in | `[experimental].accepted_features = ["shape_transforms"]` plus `[experimental].shape_transforms = true` for non-`TypedDict` shape sources | `TypedDict` transforms remain available; non-`TypedDict` projections fail closed. | Stabilize internal `Shape` model, class/protocol/framework field provenance, and emitted checker-neutral stubs. |
| `framework_adapters` | Roadmap / prototype | Prototype adapter manifests and fixture-only SDK details | No default emitted output depends on framework adapter manifests. | Publish a versioned SDK, registry format, validation rules, and cross-checker fixture matrix. |
| `effect_rows` | Roadmap / prototype | `[experimental].accepted_features = ["effect_rows"]` | No effect-row diagnostics, LSP effect hovers, or effect sidecar facts are produced. Core v1 programs rely only on `unsafe:` fences. | Define row algebra, framework capability provenance, diagnostics, and sidecar or plugin story before marketing as stable. |
| `taint` | Roadmap / prototype | `[experimental].accepted_features = ["taint"]` | No taint source/sink/sanitizer diagnostics or taint effect facts are produced. | Define interprocedural scope, sanitizer soundness, context algebra, and promotion tests. |
| `validator_witnesses` | Roadmap / prototype | `[experimental].accepted_features = ["validator_witnesses"]` | No Core v1 narrowing depends on validator witnesses. Trusted witnesses do not narrow `unknown` while disabled. | Define witness syntax, runtime/static trust model, diagnostics, and erasure rules. |
| `notebook_ingestion` | Roadmap / prototype | Script and future workflow experiments | Core v1 CLI behavior is unchanged. | Stabilize notebook source mapping, diagnostics, cache invalidation, and output policy. |

## Promotion Checklist

A promotion change must include all of the following in the same commit series:

- registry row status update and narrower or removed experimental gate
- `docs/feature-status.md` and `docs/beta-readiness.md` updates
- conformance report or diagnostic coverage updates when the feature has normative behavior
- README and `README-PyPI.md` wording that reflects the new scope without implying external
  downstream consumers inherit TypePython-only facts
- tests proving disabled-by-default behavior still preserves Core v1 acceptance and output

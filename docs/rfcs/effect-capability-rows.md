# RFC: Effect and Capability Rows Slice

**Status:** internal P0 vertical slice  
**Scope:** checker and LSP author-time facts; emitted `.py` and `.pyi` remain standard

## Summary

TypePython now has an internal effect model with `EffectKind`, `EffectRow`, `EffectSummary`, `EffectSource`, and `CapabilityScope`. The first capability scope is `unsafe:`: unsafe operation sites discovered inside an `unsafe:` block materialize a local author-time grant for the `unsafe` effect while preserving the existing lowering behavior that erases the syntax to standard Python.

## Authoring surface

The MVP intentionally uses decorators and existing syntax rather than new effect syntax:

- `@effect_pure` declares a function pure.
- `@effect("io.net")` / `@tpy.effect("io.net")` declares an explicit effect label; known labels map to concrete rows and unknown labels remain declared effect atoms.
- `@effect_io_fs`, `@effect_io_net`, `@effect_io_proc`, `@effect_time`, `@effect_random`, `@effect_runtime_validation`, and `@effect_taint_sanitize` declare concrete effect rows.
- lifecycle decorators such as `@must_use`, `@must_await`, `@must_close`, and `@must_consume` map into resource effect facts.
- `@source`, `@sink`, `@sanitizer`, and `@trusted_validator` feed the taint and boundary-validator vertical slices through the same effect vocabulary.
- framework adapter providers with function-decorator capabilities such as `effect_io_net`, `effect_time`, `taint_source`, or `validator_witness` inject the same effect facts into callables decorated by that provider.

## Diagnostics and LSP

Strict checking reports `TPY4026` when a function consumes an effectful result without declaring a covering effect row or isolating that effect. Capability scopes are consulted before reporting; an `unsafe:` scope grants the local `unsafe` row for calls on that line, while other effect rows still require a declaration or refactor. Diagnostic notes include the source decorator, framework capability, lifecycle fact, inferred callee, or standard-library catalog entry.

Public summaries persist explicit and inferred effect rows as solver facts. Importers can therefore diagnose pure functions that call an effectful function imported from another TypePython module without reopening the provider source, including provider functions whose row was inferred from their own direct effectful calls. Lifecycle decorators are part of that same summary channel: an imported `@must_use` function can still trigger ignored-result diagnostics, and an imported `@must_close` / `@must_consume` factory can still trigger resource diagnostics. CLI cache persistence also writes `.typepython/cache/effects.json`, a compact sidecar for tools that want effect/capability facts without decoding the full incremental snapshot.

The checker also ships a small standard-library effect catalog for stable side-effect surfaces such as `time`, `random`, `secrets`, `os`, `subprocess`, and `urllib.request`. Imported stdlib functions and module methods seed local inference, so a pure function that calls `time()` or `random.random()` receives the same uncovered-row diagnostic as one calling a user-authored `@effect(...)` function.

LSP hover displays a callable's effect summary next to its signature, including decorator-declared effects, framework adapter effect capabilities, lifecycle/resource facts such as `resource.lifecycle`, locally inferred rows, stdlib-derived rows, and rows inferred through imported TypePython callables. This keeps editor explanations aligned with the checker path instead of showing effect metadata only for explicitly decorated functions.

## Portability boundary

Effect and capability facts are TypePython author-time metadata. They do not require a runtime library and do not place TypePython-only constructs in emitted `.py` or `.pyi`.

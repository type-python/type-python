# RFC: Effect and Capability Rows Slice

**Status:** internal P0 vertical slice  
**Strategic source:** `docs/research-roadmap-assessment.zh.md` P0 Effect / Capability Rows  
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

Strict checking reports `TPY4026` when a function consumes an effectful result without declaring a covering effect row or isolating that effect. Capability scopes are consulted before reporting; an `unsafe:` scope grants the local `unsafe` row for calls on that line, while other effect rows still require a declaration or refactor. Diagnostic notes include the source decorator, framework capability, or lifecycle fact.

Public summaries persist effect rows as solver facts. Importers can therefore diagnose pure functions that call an effectful function imported from another TypePython module without reopening the provider source.

LSP hover displays a callable's effect summary next to its signature so users can understand why a diagnostic was produced without inspecting checker internals.

## Portability boundary

Effect and capability facts are TypePython author-time metadata. They do not require a runtime library and do not place TypePython-only constructs in emitted `.py` or `.pyi`.

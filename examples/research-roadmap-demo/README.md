# Research roadmap demo slice

This example documents the implemented P0-P4 research slices as a user-facing demo set:

- P0: `@effect("io.net")` and explicit pure/effect boundaries feed checker diagnostics and LSP hover/code actions while emitting standard Python.
- P1: field-bearing sources flow through the shared Shape substrate, with shape fingerprints recorded in incremental summaries.
- P2: restricted type-level aliases such as `TypeIf`, `KeyOf`, `RequiredKeys`, `OptionalKeys`, `Pick`, `Omit`, and supported `MapValues` forms reduce before emit or fail closed with `TPY4027`.
- P3: request sources, HTML sinks, and sanitizer decorators are shown through the checker-facing taint slice; framework capability-backed coverage lives in the crate tests.
- P4: `ValidatorWitness[T, Trust]` narrows `unknown` only through explicit trust markers such as `TrustedValidator` or generated equivalents.

The focused crate tests exercise the executable form of this demo; this checked-in example gives users a stable map from the roadmap language to the current authoring surfaces.

# Research roadmap demo slice

This example documents the implemented P0-P4 research slices as a user-facing demo set:

- P0: this project accepts `"effect_rows"` so `@effect("io.net")` and composed caller rows such as `@effect("taint.sanitize")` feed checker diagnostics and LSP hover/code actions while emitting standard Python.
- P1: field-bearing sources flow through the shared Shape substrate. This project accepts `"shape_transforms"` in `[experimental].accepted_features` and enables `[experimental].shape_transforms = true` so `Partial[UserModel]` and `Pick[UserModel, ...]` materialize from a TypePython `data class`, while ordinary `TypedDict` key-set aliases keep working without non-standard output.
- P2: restricted type-level aliases such as `TypeIf`, `KeyOf`, `RequiredKeys`, `OptionalKeys`, `Pick`, `Omit`, and supported `MapValues` forms reduce before emit or fail closed with `TPY4027`.
- P3: request sources, HTML sinks, and sanitizer decorators are shown as syntax in this demo; the stricter `"taint"` checker slice remains covered by crate tests until the demo opts into that contract.
- P4: this project accepts `"validator_witnesses"` so `ValidatorWitness[T, Trust]` narrows `unknown` only through explicit trust metadata such as `Literal["trusted"]`, `Literal["generated"]`, or generated-validator decorator metadata.

Run it from the repository root:

```bash
cargo run -p typepython-cli -- check --project examples/research-roadmap-demo
cargo run -p typepython-cli -- build --project examples/research-roadmap-demo
```

The focused crate tests exercise the deeper edge cases; this checked-in example gives users a stable map from the roadmap language to the current authoring surfaces.

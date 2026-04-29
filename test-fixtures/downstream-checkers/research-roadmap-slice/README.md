# Research roadmap downstream fixture

This fixture records the downstream-checker expectations for the emitted roadmap slice, with a focus on the standard-library-facing P1/P2 output surfaces:

- emitted stubs must not contain TypePython-only effect rows, taint qualifiers, `ValidatorWitness`, or restricted type-level functions;
- shape projections and supported `MapValues` reductions should appear as standard `TypedDict`, `Literal`, `Optional`, and `ReadOnly` surfaces;
- checker-only features such as effect rows, taint metadata, and validator witnesses must stay out of emitted stubs and must not require a runtime TypePython package.

The fixture is intentionally limited to emitted-output verification because the current P0/P3/P4 slices are author-time checker/LSP behavior rather than downstream-stub runtime features.

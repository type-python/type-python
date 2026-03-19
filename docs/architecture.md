# TypePython Rust Workspace

This repository is initialized as a virtual Cargo workspace whose crate layout
matches the normative compiler phase split in Appendix G of
`TypePython_Spec.md`.

## Crate Map

- `typepython_cli`: user-facing `typepython` binary and command wiring
- `typepython_config`: project discovery and config loading for
  `typepython.toml` and `[tool.typepython]`
- `typepython_diagnostics`: shared diagnostic model and text/JSON rendering
- `typepython_syntax`: source-kind detection and parser entrypoint boundary
- `typepython_lowering`: lowered Python form and source-map boundary
- `typepython_binding`: symbol creation boundary
- `typepython_graph`: module graph and summary boundary
- `typepython_checking`: type-checking boundary
- `typepython_emit`: output planning boundary
- `typepython_incremental`: cache fingerprint boundary
- `typepython_lsp`: future language-server boundary

## Milestone Alignment

- Milestone 0: workspace, toolchain pin, config loader, command skeletons
- Milestone 1: parser and lowering move into `typepython_syntax` and
  `typepython_lowering`
- Milestone 2: binder, graph, summaries, and checking grow in their dedicated
  crates
- Milestone 3+: emit, verify, watch, incremental, and LSP deepen without
  collapsing crate boundaries

## Engineering Choices

- The repository uses a virtual Cargo workspace so profiles, shared dependency
  versions, and lint policy live at the workspace root.
- The toolchain is pinned in `rust-toolchain.toml` to keep local development
  and CI on the same Rust release and component set.
- Formatting, linting, and tests are standardized through `Makefile` targets
  and a GitHub Actions workflow.
- The CLI implements Milestone 0 behavior: configuration discovery, command
  routing, basic source enumeration, and pipeline placeholders.

## Official References

- Cargo workspaces:
  https://doc.rust-lang.org/cargo/reference/workspaces.html
- Cargo profiles:
  https://doc.rust-lang.org/cargo/reference/profiles.html
- `cargo test`:
  https://doc.rust-lang.org/cargo/commands/cargo-test.html
- rustup toolchain pinning:
  https://rust-lang.github.io/rustup/overrides.html
- Clippy:
  https://rust-lang.github.io/rust-clippy/
- rustfmt:
  https://rust-lang.github.io/rustfmt/
- Rust API Guidelines:
  https://rust-lang.github.io/api-guidelines/checklist.html

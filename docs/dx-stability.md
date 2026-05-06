# DX and LSP Stability

TypePython separates Core authoring semantics from editor and workflow DX. During
the Core v1 Beta line, the compiler can expose useful LSP, watch, formatting,
code-action, and packaging helpers without treating every editor-facing detail as
a stable v1.0 product promise.

## Stability Classes

| Surface | Beta status | v1.0 requirement |
| ------- | ----------- | ---------------- |
| `typepython lsp` stdio transport | Supported DX, non-stable | Stable startup contract, logging contract, and capability snapshot |
| LSP capabilities | Supported DX, non-stable | Stable `initialize` response unless a documented version bump explains the change |
| Diagnostics in editors | Supported DX, non-stable | Stable diagnostic code identity plus stable machine-readable diagnostic JSON schema |
| `textDocument/hover`, definition, references, completion, rename | Supported DX, non-stable | Stable request/response behavior for documented methods |
| Formatting | Supported DX, non-stable | Golden tests for `.tpy`, `.py`, `.pyi`, configured formatter errors, and no-op formatting |
| Code actions | Supported DX, non-stable | Golden tests for diagnostic quick fixes and command IDs |
| `typepython watch` | Supported DX, non-stable | Stable debounce, rebuild failure recovery, delete/rename handling, and JSON output schema |
| Editor extensions and snippets | Supported DX, non-stable | Installable VS Code extension plus copy-paste Neovim, Helix, Sublime, and Emacs setup |
| Logs and crash diagnostics | Supported DX, non-stable | Documented environment variables, file logging, and non-silent LSP request failures |

## v1.0 DX Gate

A v1.0 release candidate must include evidence for:

- an installable VS Code extension or packaged VSIX artifact;
- copy-paste configuration for Neovim, Helix, Sublime Text, and Emacs;
- an exact LSP `initialize` capability snapshot test;
- stable `--format json` schema versioning for project-oriented commands;
- watch-mode tests for initial build, debounced rebuild, rebuild failure recovery, and file deletion;
- formatting and code-action golden tests for representative `.tpy` edits;
- documented logging controls for CLI, watch, and LSP;
- packaging smoke proving installed wheels run without `cargo` on supported platforms.

Until those items are complete, README and release notes should describe these
surfaces as supported Beta DX rather than stable v1.0 commitments.

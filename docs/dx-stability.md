# DX and LSP Stability

TypePython separates Core authoring semantics from editor and workflow DX. During
the Core v1 Beta line and the first Core v1.0 RC, the compiler can expose useful
LSP, watch, formatting, code-action, and packaging helpers without treating every
editor-facing detail as a stable v1.0 product promise.

The first v1.0 release candidate is a **Core v1.0 RC** unless release notes
explicitly say otherwise. Core v1.0 RC does not promote the Supported DX tier:
`typepython lsp`, `typepython watch`, editor extensions, formatter integration,
code actions, and migration/adoption workflows remain Supported DX, non-stable
until the DX gate below is complete.

## Stability Classes

| Surface | Beta status | v1.0 requirement |
| ------- | ----------- | ---------------- |
| `typepython lsp` stdio transport | Supported DX, non-stable | Stable startup contract, logging contract, and capability snapshot |
| LSP capabilities | Supported DX, non-stable | Stable `initialize` response unless a documented version bump explains the change |
| Diagnostics in editors | Supported DX, non-stable | Stable diagnostic code identity plus stable machine-readable diagnostic JSON schema |
| `textDocument/hover`, definition, references, completion, rename | Supported DX, non-stable | Stable request/response behavior for documented methods |
| Formatting | Supported DX, non-stable | Golden tests for `.tpy`, `.py`, `.pyi`, configured formatter errors, and no-op formatting |
| Code actions | Supported DX, non-stable | Golden tests for diagnostic quick fixes and command IDs |
| `typepython watch` | Supported DX, non-stable | Stable debounce, configuration reload, rebuild failure recovery, delete/rename handling, and JSON output schema |
| Editor extensions and snippets | Supported DX, non-stable | Installable VS Code extension plus copy-paste Neovim, Helix, Sublime, and Emacs setup |
| Logs and crash diagnostics | Supported DX, non-stable | Documented environment variables, file logging, and non-silent LSP request failures |

## v1.0 DX Gate

A product-wide v1.0 release candidate that claims stable DX must include evidence for:

- an installable VS Code extension or packaged VSIX artifact;
- copy-paste configuration for Neovim, Helix, Sublime Text, and Emacs;
- an exact LSP `initialize` capability snapshot test that fails on accidental
  capability drift;
- stable `--format json` schema versioning for project-oriented commands;
- watch-mode tests for initial build, debounced rebuild, rebuild failure recovery, and file deletion;
- formatting and code-action golden tests for representative `.tpy` edits;
- documented logging controls for CLI, watch, and LSP;
- packaging smoke proving installed wheels run without `cargo` on supported platforms.

Until those items are complete, README and release notes should describe these
surfaces as supported Beta DX rather than stable v1.0 commitments. A Core-only
v1.0 RC may ship before this gate is complete, but it must keep the DX exclusion
visible in the README, PyPI description, and release notes.

## Core RC VS Code Packaging Evidence

The source-installable VS Code extension under `editors/vscode/` is release-gate evidence for the
Supported DX tier, not part of the Stable Core surface. `scripts/test_editor_integrations.py` checks
that the manifest launches `typepython lsp` over stdio, that `.tpy` registration and Python document
attachment remain wired, and that the documented VSIX filename matches the package version.

That evidence does not require Marketplace publication, does not require uploading a VSIX artifact
from the Core RC workflow, and does not block Core v1.0 RC when the compiler, emitted artifacts, and
Core verification gates are green. Marketplace release and polished editor onboarding remain DX gate
items for a later product-wide v1.0 claim.

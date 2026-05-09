# TypePython VS Code Extension

This directory contains the official source-installable VS Code language client
for TypePython. It is intentionally thin: the extension starts the bundled
`typepython lsp` server, registers `.tpy` files, attaches to VS Code Python
documents for `.py` / `.pyi` project files, exposes a restart command, and routes
server output to VS Code output channels. It restarts the language server when
TypePython settings, workspace folders, `typepython.toml`, or `[tool.typepython]`
configuration changes.

## Install from Source

```sh
cd editors/vscode
npm install
npm run package
code --install-extension typepython-vscode-1.0.0-rc.1.vsix
```

## Settings

| Setting | Default | Meaning |
| ------- | ------- | ------- |
| `typepython.binaryPath` | `""` | Path to the `typepython` executable. Empty means `TYPEPYTHON_BIN` or `typepython` on `PATH`. |
| `typepython.projectPath` | `""` | Project directory passed to `typepython lsp --project`. Empty means the first workspace folder with TypePython config, then the first workspace folder. |
| `typepython.trace.server` | `"off"` | LSP message tracing: `off`, `messages`, or `verbose`. |

The extension is a Supported Beta DX surface. Extension settings, commands, and
packaging details may change before the DX v1.0 gate in
[`docs/dx-stability.md`](../../docs/dx-stability.md).

Source-installable extension checks and local VSIX packaging are release evidence,
but the local VSIX packaging check does not block Core v1.0 RC. Marketplace
publication and polished onboarding remain part of the later DX v1.0 gate.

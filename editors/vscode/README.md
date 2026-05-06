# TypePython VS Code Extension

This directory contains the official source-installable VS Code language client
for TypePython. It is intentionally thin: the extension starts the bundled
`typepython lsp` server, registers `.tpy` files, attaches to VS Code Python
documents for `.py` / `.pyi` project files, exposes a restart command, and routes
server output to VS Code output channels.

## Install from Source

```sh
cd editors/vscode
npm install
npm run package
code --install-extension typepython-vscode-0.4.0.vsix
```

## Settings

| Setting | Default | Meaning |
| ------- | ------- | ------- |
| `typepython.binaryPath` | `""` | Path to the `typepython` executable. Empty means `TYPEPYTHON_BIN` or `typepython` on `PATH`. |
| `typepython.projectPath` | `""` | Project directory passed to `typepython lsp --project`. Empty means the first workspace folder. |
| `typepython.trace.server` | `"off"` | LSP message tracing: `off`, `messages`, or `verbose`. |

The extension is a Supported Beta DX surface. Extension settings, commands, and
packaging details may change before the DX v1.0 gate in
[`docs/dx-stability.md`](../../docs/dx-stability.md).

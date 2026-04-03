# LSP Integration

TypePython includes a built-in Language Server Protocol (LSP) server that provides real-time type checking and code intelligence in your editor.

## Starting the LSP Server

```bash
typepython lsp --project /path/to/project
```

The server communicates via **stdio** using the JSON-RPC 2.0 protocol -- the standard transport for LSP.

## Capabilities

| Feature           | LSP Method                        | Description                               |
| ----------------- | --------------------------------- | ----------------------------------------- |
| Diagnostics       | `textDocument/publishDiagnostics` | Real-time type errors and warnings        |
| Hover             | `textDocument/hover`              | Type information at cursor position       |
| Go to Definition  | `textDocument/definition`         | Jump to symbol definition                 |
| Find References   | `textDocument/references`         | Find all usages of a symbol               |
| Formatting        | `textDocument/formatting`         | Format the current document               |
| Signature Help    | `textDocument/signatureHelp`      | Active call signature and parameter index |
| Document Symbols  | `textDocument/documentSymbol`     | Outline symbols in the current file       |
| Workspace Symbols | `workspace/symbol`                | Search declarations across the workspace  |
| Rename            | `textDocument/rename`             | Rename symbol across entire project       |
| Code Actions      | `textDocument/codeAction`         | Quick fixes from diagnostic suggestions   |
| Completion        | `textDocument/completion`         | Autocomplete (triggered on `.`)           |

### Document Synchronization

The server uses **incremental text sync** (`textDocumentSync.change: 2`): clients can send either full-document replacements or ranged edits, and the server applies them to the in-memory overlay before incremental analysis reruns.

### Completion

Completions are triggered by the `.` character and include:

- Module member completions
- Attribute completions based on inferred type
- Method completions

### Code Actions

Code actions are generated from diagnostic suggestions. For example:

- Add `| None` to return type
- Add `@override` decorator
- Add missing `match` cases
- Fix TypedDict key typos

### Formatting

Document formatting is exposed via `textDocument/formatting`.

- `.tpy` files are normalized to formatter-friendly Python, run through the backend, then restored back to TypePython syntax
- `.py` and `.pyi` files are formatted directly
- The server auto-detects `ruff format` and `black`
- You can override the backend with `[format].command` in configuration

Example configuration:

```toml
[format]
command = ["python3", "{workspace_root}/tools/format_stdin.py", "{file}"]
line_length = 1000
```

### Hover

Hover displays the inferred type of the symbol at the cursor:

```
x: int
---
def greet(name: str) -> str
---
class User (data class)
  name: str
  age: int
```

## Editor Setup

The snippets below are generic LSP client configurations. TypePython does not currently ship an official VS Code, Neovim, Helix, Sublime, or Emacs plugin.

### VS Code

Configure a generic LSP client extension (for example one built on `vscode-languageclient`) to launch `typepython lsp`:

```json
{
  "languageServerExample.trace.server": "verbose",
  "languageServerExample.serverPath": "typepython",
  "languageServerExample.serverArgs": ["lsp", "--project", "${workspaceFolder}"]
}
```

**File association** -- add `.tpy` files to Python language mode or create a custom language:

```json
{
  "files.associations": {
    "*.tpy": "python"
  }
}
```

### Neovim (nvim-lspconfig)

Add to your Neovim configuration:

```lua
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

if not configs.typepython then
  configs.typepython = {
    default_config = {
      cmd = { 'typepython', 'lsp', '--project', '.' },
      filetypes = { 'python', 'typepython' },
      root_dir = function(fname)
        return lspconfig.util.root_pattern('typepython.toml', 'pyproject.toml')(fname)
      end,
      settings = {},
    },
  }
end

lspconfig.typepython.setup({})
```

Register the `.tpy` filetype:

```lua
vim.filetype.add({
  extension = {
    tpy = 'typepython',
  },
})
```

### Helix

Add to `~/.config/helix/languages.toml`:

```toml
[[language]]
name = "typepython"
scope = "source.typepython"
injection-regex = "typepython"
file-types = ["tpy"]
language-servers = ["typepython-lsp"]
indent = { tab-width = 4, unit = "    " }
grammar = "python"

[language-server.typepython-lsp]
command = "typepython"
args = ["lsp", "--project", "."]
```

### Sublime Text (LSP package)

Add to `LSP.sublime-settings`:

```json
{
  "clients": {
    "typepython": {
      "enabled": true,
      "command": ["typepython", "lsp", "--project", "."],
      "selector": "source.python",
      "schemes": ["file"]
    }
  }
}
```

### Emacs (lsp-mode)

```elisp
(with-eval-after-load 'lsp-mode
  (add-to-list 'lsp-language-id-configuration '(typepython-mode . "typepython"))

  (lsp-register-client
   (make-lsp-client
    :new-connection (lsp-stdio-connection '("typepython" "lsp" "--project" "."))
    :activation-fn (lsp-activate-on "typepython" "python")
    :server-id 'typepython-lsp)))
```

### Generic LSP Client

Any editor with LSP support can use TypePython. Configure:

| Setting      | Value                                 |
| ------------ | ------------------------------------- |
| Command      | `typepython lsp --project <path>`     |
| Transport    | stdio                                 |
| File types   | `.tpy` (and optionally `.py`, `.pyi`) |
| Root pattern | `typepython.toml` or `pyproject.toml` |

## Architecture

The LSP server maintains:

1. **Document overlays** -- in-memory copies of open files (unsaved changes)
2. **Incremental workspace cache** -- cached project/support syntax trees, bindings, module graph, and checker diagnostics
3. **Query indexes** -- incrementally maintained document, occurrence, and module-node maps for editor lookups

On each document change:

1. The overlay is updated with the new document text
2. Only the changed project document is reparsed/rebound
3. Support modules are added to or removed from the active workspace set incrementally if imports changed
4. The module graph snapshot and reverse-import index are updated
5. The checker reruns only for the changed modules plus dependents whose public summaries changed
6. Query indexes are refreshed only for the affected modules using cached document indexes
7. Diagnostics are pushed to the editor

## Troubleshooting

### Server doesn't start

- Ensure the `typepython` binary is on your `PATH` or set `TYPEPYTHON_BIN`
- Check that a `typepython.toml` or `pyproject.toml` with `[tool.typepython]` exists in the project
- Run `typepython lsp --project .` manually to see error output

### No diagnostics appearing

- Verify the file is within a configured `src` directory
- Check `include`/`exclude` patterns in configuration
- Confirm the file has a `.tpy`, `.py`, or `.pyi` extension

### Slow response times

- Large projects may benefit from narrower `include` patterns
- Exclude generated directories (`.typepython/`, `dist/`, `node_modules/`)
- The `watch.debounce_ms` setting can reduce recompilation frequency

### Formatting returns an error

- Ensure either `ruff` or `black` is installed, or configure `[format].command`
- If you use a relative path inside `[format].command`, it is resolved from the workspace root
- `TPY6003` indicates formatter startup, availability, or execution failure

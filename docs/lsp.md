# LSP Integration

TypePython includes a built-in Language Server Protocol (LSP) server that provides real-time type checking and code intelligence in your editor.

## Starting the LSP Server

```bash
typepython lsp --project /path/to/project
```

The server communicates via **stdio** using the JSON-RPC 2.0 protocol -- the standard transport for LSP.

## Capabilities

| Feature | LSP Method | Description |
|---|---|---|
| Diagnostics | `textDocument/publishDiagnostics` | Real-time type errors and warnings |
| Hover | `textDocument/hover` | Type information at cursor position |
| Go to Definition | `textDocument/definition` | Jump to symbol definition |
| Find References | `textDocument/references` | Find all usages of a symbol |
| Rename | `textDocument/rename` | Rename symbol across entire project |
| Code Actions | `textDocument/codeAction` | Quick fixes from diagnostic suggestions |
| Completion | `textDocument/completion` | Autocomplete (triggered on `.`) |

### Document Synchronization

The server uses **full-text sync** (`textDocumentSync: 1`): the entire document content is sent on each change. This ensures consistency with the TypePython compilation model.

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

| Setting | Value |
|---|---|
| Command | `typepython lsp --project <path>` |
| Transport | stdio |
| File types | `.tpy` (and optionally `.py`, `.pyi`) |
| Root pattern | `typepython.toml` or `pyproject.toml` |

## Architecture

The LSP server maintains:

1. **Document overlays** -- in-memory copies of open files (unsaved changes)
2. **Cached workspace** -- full compiled module graph
3. **Symbol tables** -- occurrence maps for definition/reference lookup

On each document change:
1. The overlay is updated with the new document text
2. The full workspace is recompiled (parse -> bind -> graph -> check)
3. Diagnostics are pushed to the editor
4. Symbol tables are rebuilt for navigation

## Limitations

- **Full-text sync**: the server receives the entire document on each change (no incremental text sync)
- **Full recompilation**: the server recompiles the entire workspace on each change (incremental compilation is used for the CLI but not yet exposed in the LSP)
- **No signature help**: `textDocument/signatureHelp` is not yet implemented
- **No document symbols**: `textDocument/documentSymbol` is not yet implemented
- **No workspace symbols**: `workspace/symbol` is not yet implemented
- **No formatting**: use an external formatter (e.g., ruff, black) for code formatting

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

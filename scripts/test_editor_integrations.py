from __future__ import annotations

import json
import pathlib
import re
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
VSCODE_ROOT = REPO_ROOT / "editors" / "vscode"


def read_text(relative_path: str) -> str:
    return (REPO_ROOT / relative_path).read_text(encoding="utf-8")


def project_version() -> str:
    pyproject = read_text("pyproject.toml")
    match = re.search(r'(?m)^version = "([^"]+)"$', pyproject)
    if match is None:
        raise AssertionError("pyproject.toml version was not found")
    return match.group(1)


class EditorIntegrationTests(unittest.TestCase):
    def test_vscode_extension_manifest_launches_typepython_lsp(self) -> None:
        package = json.loads(VSCODE_ROOT.joinpath("package.json").read_text(encoding="utf-8"))

        self.assertEqual(package["name"], "typepython-vscode")
        self.assertEqual(package["version"], project_version())
        self.assertEqual(package["main"], "./extension.js")
        self.assertIn("vscode-languageclient", package["dependencies"])
        self.assertIn("@vscode/vsce", package["devDependencies"])

        language = package["contributes"]["languages"][0]
        self.assertEqual(language["id"], "typepython")
        self.assertIn(".tpy", language["extensions"])
        self.assertEqual(language["configuration"], "./language-configuration.json")
        self.assertIn("onLanguage:python", package["activationEvents"])

        properties = package["contributes"]["configuration"]["properties"]
        self.assertIn("typepython.binaryPath", properties)
        self.assertIn("typepython.projectPath", properties)
        self.assertIn("typepython.trace.server", properties)

        commands = {
            command["command"]
            for command in package["contributes"]["commands"]
        }
        self.assertIn("typepython.restartServer", commands)

    def test_vscode_extension_client_uses_stdio_lsp_and_env_override(self) -> None:
        extension = VSCODE_ROOT.joinpath("extension.js").read_text(encoding="utf-8")

        self.assertIn("LanguageClient", extension)
        self.assertIn("TransportKind.stdio", extension)
        self.assertIn("Trace", extension)
        self.assertIn("Trace.Off", extension)
        self.assertIn("Trace.Messages", extension)
        self.assertIn("Trace.Verbose", extension)
        self.assertIn('args: ["lsp", "--project", workspaceProjectPath()]', extension)
        self.assertIn('{ scheme: "file", language: "python" }', extension)
        self.assertIn("TYPEPYTHON_BIN", extension)
        self.assertIn("configuredProjectPath", extension)
        self.assertIn("projectConfigWorkspaceFolder", extension)
        self.assertIn("typepython.restartServer", extension)
        self.assertIn("stopServer", extension)
        self.assertIn("restartServer", extension)
        self.assertIn("let restartQueue = Promise.resolve();", extension)
        self.assertIn("restartQueue = restartQueue", extension)
        self.assertIn(".then(() => startServer(context, explicit))", extension)
        self.assertIn("return restartQueue", extension)
        self.assertIn("await restartServer(context, true);", extension)
        self.assertIn("await restartServer(context);", extension)
        self.assertIn("let traceOutputChannel;", extension)
        self.assertIn("if (!traceOutputChannel)", extension)
        self.assertIn(
            'traceOutputChannel = vscode.window.createOutputChannel("TypePython Trace")',
            extension,
        )
        self.assertEqual(
            extension.count('vscode.window.createOutputChannel("TypePython Trace")'),
            1,
        )
        self.assertIn("let documentFileWatcher;", extension)
        self.assertIn(
            'documentFileWatcher = vscode.workspace.createFileSystemWatcher("**/*.{tpy,py,pyi}")',
            extension,
        )
        self.assertEqual(
            extension.count('vscode.workspace.createFileSystemWatcher("**/*.{tpy,py,pyi}")'),
            1,
        )
        self.assertNotIn("context.subscriptions.push(languageClient)", extension)
        self.assertIn("createFileSystemWatcher", extension)
        self.assertIn("onDidCreate", extension)
        self.assertIn("onDidDelete", extension)
        self.assertIn("onDidChangeConfiguration", extension)
        self.assertIn("onDidChangeWorkspaceFolders", extension)
        self.assertIn("hasTypePythonProjectConfig", extension)
        self.assertIn("typepython.toml", extension)
        self.assertIn(r"\[tool\.typepython", extension)
        self.assertIn("shouldStartServer", extension)
        self.assertIn("TypePython project config not found", extension)

    def test_lsp_docs_reference_official_extension_and_generic_editors(self) -> None:
        lsp = read_text("docs/lsp.md")
        readme = read_text("README.md")
        pypi_readme = read_text("README-PyPI.md")

        self.assertIn("editors/vscode", lsp)
        self.assertIn("code --install-extension typepython-vscode-0.4.0.vsix", lsp)
        self.assertIn("attaches the TypePython LSP client to VS Code", lsp)
        self.assertIn("first workspace folder with `typepython.toml`", lsp)
        self.assertIn("setting changes, workspace-folder changes", lsp)
        for editor in ("Neovim", "Helix", "Sublime Text", "Emacs"):
            self.assertIn(editor, lsp)
            self.assertIn(editor, readme)
            self.assertIn(editor, pypi_readme)

        self.assertIn("source-installable VS Code extension", readme)
        self.assertIn("source-installable VS Code extension", pypi_readme)


if __name__ == "__main__":
    unittest.main()

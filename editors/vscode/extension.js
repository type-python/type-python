const vscode = require("vscode");
const fs = require("fs");
const path = require("path");
const { LanguageClient, TransportKind, Trace } = require("vscode-languageclient/node");

let client;

function extensionConfig() {
  return vscode.workspace.getConfiguration("typepython");
}

function workspaceProjectPath() {
  const configured = extensionConfig().get("projectPath");
  if (configured && configured.trim() !== "") {
    return configured;
  }
  const folder = vscode.workspace.workspaceFolders?.[0];
  return folder ? folder.uri.fsPath : ".";
}

function hasConfiguredProjectPath() {
  const configured = extensionConfig().get("projectPath");
  return Boolean(configured && configured.trim() !== "");
}

function hasTypePythonProjectConfig(projectPath) {
  if (fs.existsSync(path.join(projectPath, "typepython.toml"))) {
    return true;
  }
  const pyprojectPath = path.join(projectPath, "pyproject.toml");
  if (!fs.existsSync(pyprojectPath)) {
    return false;
  }
  try {
    const pyproject = fs.readFileSync(pyprojectPath, "utf8");
    return /^\s*\[tool\.typepython(?:\]|\.)/m.test(pyproject);
  } catch (_) {
    return false;
  }
}

function shouldStartServer() {
  return hasConfiguredProjectPath() || hasTypePythonProjectConfig(workspaceProjectPath());
}

function binaryPath() {
  const configured = extensionConfig().get("binaryPath");
  if (configured && configured.trim() !== "") {
    return configured;
  }
  return process.env.TYPEPYTHON_BIN || "typepython";
}

function serverTrace() {
  const configured = extensionConfig().get("trace.server");
  switch (configured) {
    case "messages":
      return Trace.Messages;
    case "verbose":
      return Trace.Verbose;
    default:
      return Trace.Off;
  }
}

function buildClient(context) {
  const serverOptions = {
    command: binaryPath(),
    args: ["lsp", "--project", workspaceProjectPath()],
    transport: TransportKind.stdio,
    options: {
      env: {
        ...process.env
      }
    }
  };

  const clientOptions = {
    documentSelector: [
      { scheme: "file", language: "typepython" }
    ],
    outputChannelName: "TypePython",
    traceOutputChannel: vscode.window.createOutputChannel("TypePython Trace"),
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.{tpy,py,pyi}")
    }
  };

  const languageClient = new LanguageClient(
    "typepython",
    "TypePython",
    serverOptions,
    clientOptions
  );
  languageClient.setTrace(serverTrace());
  context.subscriptions.push(languageClient);
  return languageClient;
}

async function startServer(context, explicit = false) {
  if (!shouldStartServer()) {
    if (explicit) {
      vscode.window.showWarningMessage(
        "TypePython project config not found. Add typepython.toml, [tool.typepython], or set typepython.projectPath."
      );
    }
    return;
  }
  if (client) {
    await client.stop();
  }
  client = buildClient(context);
  await client.start();
}

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("typepython.restartServer", async () => {
      await startServer(context, true);
    })
  );
  await startServer(context);
}

function deactivate() {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

module.exports = {
  activate,
  deactivate
};

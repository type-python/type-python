const vscode = require("vscode");
const fs = require("fs");
const path = require("path");
const { LanguageClient, TransportKind, Trace } = require("vscode-languageclient/node");

let client;
let traceOutputChannel;
let documentFileWatcher;
let restartQueue = Promise.resolve();

function extensionConfig() {
  return vscode.workspace.getConfiguration("typepython");
}

function configuredProjectPath() {
  const configured = extensionConfig().get("projectPath");
  if (configured && configured.trim() !== "") {
    return configured.trim();
  }
  return undefined;
}

function hasConfiguredProjectPath() {
  return configuredProjectPath() !== undefined;
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

function projectConfigWorkspaceFolder() {
  return vscode.workspace.workspaceFolders?.find((folder) =>
    hasTypePythonProjectConfig(folder.uri.fsPath)
  );
}

function workspaceProjectPath() {
  const configured = configuredProjectPath();
  if (configured !== undefined) {
    return configured;
  }
  const folder = projectConfigWorkspaceFolder() || vscode.workspace.workspaceFolders?.[0];
  return folder ? folder.uri.fsPath : ".";
}

function shouldStartServer() {
  return hasConfiguredProjectPath() || projectConfigWorkspaceFolder() !== undefined;
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

function traceChannel(context) {
  if (!traceOutputChannel) {
    traceOutputChannel = vscode.window.createOutputChannel("TypePython Trace");
    context.subscriptions.push(traceOutputChannel);
  }
  return traceOutputChannel;
}

function lspDocumentFileWatcher(context) {
  if (!documentFileWatcher) {
    documentFileWatcher = vscode.workspace.createFileSystemWatcher("**/*.{tpy,py,pyi}");
    context.subscriptions.push(documentFileWatcher);
  }
  return documentFileWatcher;
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
      { scheme: "file", language: "typepython" },
      { scheme: "file", language: "python" }
    ],
    outputChannelName: "TypePython",
    traceOutputChannel: traceChannel(context),
    synchronize: {
      fileEvents: lspDocumentFileWatcher(context)
    }
  };

  const languageClient = new LanguageClient(
    "typepython",
    "TypePython",
    serverOptions,
    clientOptions
  );
  languageClient.setTrace(serverTrace());
  return languageClient;
}

async function stopServer() {
  const runningClient = client;
  client = undefined;
  if (!runningClient) {
    return;
  }
  await runningClient.stop();
}

async function startServer(context, explicit = false) {
  if (!shouldStartServer()) {
    await stopServer();
    if (explicit) {
      vscode.window.showWarningMessage(
        "TypePython project config not found. Add typepython.toml, [tool.typepython], or set typepython.projectPath."
      );
    }
    return;
  }
  await stopServer();
  const nextClient = buildClient(context);
  client = nextClient;
  try {
    await nextClient.start();
  } catch (error) {
    if (client === nextClient) {
      client = undefined;
    }
    throw error;
  }
}

function reportServerStartError(error) {
  const message = error instanceof Error ? error.message : String(error);
  vscode.window.showErrorMessage(`TypePython language server failed to start: ${message}`);
}

function restartServer(context, explicit = false) {
  restartQueue = restartQueue
    .catch(() => undefined)
    .then(() => startServer(context, explicit))
    .catch(reportServerStartError);
  return restartQueue;
}

function registerProjectConfigWatchers(context) {
  const restart = () => restartServer(context);
  for (const pattern of ["**/typepython.toml", "**/pyproject.toml"]) {
    const watcher = vscode.workspace.createFileSystemWatcher(pattern);
    context.subscriptions.push(
      watcher,
      watcher.onDidCreate(restart),
      watcher.onDidChange(restart),
      watcher.onDidDelete(restart)
    );
  }
}

function registerConfigurationWatchers(context) {
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("typepython")) {
        restartServer(context);
      }
    }),
    vscode.workspace.onDidChangeWorkspaceFolders(() => {
      restartServer(context);
    })
  );
}

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("typepython.restartServer", async () => {
      await restartServer(context, true);
    })
  );
  registerProjectConfigWatchers(context);
  registerConfigurationWatchers(context);
  await restartServer(context);
}

function deactivate() {
  return restartQueue
    .catch(() => undefined)
    .then(() => stopServer())
    .then(() => {
      if (documentFileWatcher) {
        documentFileWatcher.dispose();
        documentFileWatcher = undefined;
      }
      if (traceOutputChannel) {
        traceOutputChannel.dispose();
        traceOutputChannel = undefined;
      }
    });
}

module.exports = {
  activate,
  deactivate
};

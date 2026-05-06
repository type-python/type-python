const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

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

function binaryPath() {
  const configured = extensionConfig().get("binaryPath");
  if (configured && configured.trim() !== "") {
    return configured;
  }
  return process.env.TYPEPYTHON_BIN || "typepython";
}

function serverTrace() {
  const configured = extensionConfig().get("trace.server");
  return configured || "off";
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

async function startServer(context) {
  if (client) {
    await client.stop();
  }
  client = buildClient(context);
  await client.start();
}

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("typepython.restartServer", async () => {
      await startServer(context);
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

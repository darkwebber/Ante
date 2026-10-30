// Starts the "Ante" language server (`antec lsp`) for `.ante` files:
// errors as you type, hover, go to definition, references, rename,
// completion and formatting. Without `antec`, highlighting, snippets and
// indentation still work; the extension just says how to get the rest.

const vscode = require("vscode");
const { findAntec } = require("./server-path");

const SETUP_URL = "https://github.com/darkwebber/Ante/blob/main/docs/TOOLING.md#language-server-antec-lsp";
const QUIET_KEY = "ante.missingAntec.quiet";

let client;

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("ante.restartServer", () => restart(context)),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("ante.antecPath") || e.affectsConfiguration("ante.lsp.enabled")) {
        restart(context);
      }
    }),
  );
  await start(context);
}

async function start(context) {
  const config = vscode.workspace.getConfiguration("ante");
  if (!config.get("lsp.enabled", true)) return;
  const setting = config.get("antecPath", "antec");
  const folder = vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders[0];
  const command = findAntec(setting, { base: folder && folder.uri.fsPath });
  if (!command) {
    // Not awaited: activation shouldn't wait on the user.
    missing(context, setting);
    return;
  }
  const { LanguageClient } = require("vscode-languageclient/node");
  const server = { command, args: ["lsp"] };
  client = new LanguageClient("ante", '"Ante" language server', server, {
    documentSelector: [
      { scheme: "file", language: "ante" },
      { scheme: "untitled", language: "ante" },
    ],
  });
  try {
    await client.start();
  } catch (err) {
    client = undefined;
    vscode.window.showWarningMessage(
      `"Ante" language server start avvaledhu (${command}): ${err && err.message ? err.message : err}. Highlighting matrame pani chesthundhi.`,
    );
  }
}

async function missing(context, setting) {
  if (context.globalState.get(QUIET_KEY)) return;
  const settings = "Setting maarchu";
  const help = "Setup ela?";
  const quiet = "Malli cheppaku";
  const pick = await vscode.window.showInformationMessage(
    `\`${setting}\` dorakaledhu, kaabatti ippatiki highlighting matrame 🙂. Errors, hover, go to definition kosam \`antec\` install chesi \`ante.antecPath\` set cheyyandi.`,
    settings,
    help,
    quiet,
  );
  if (pick === settings) {
    vscode.commands.executeCommand("workbench.action.openSettings", "ante.antecPath");
  } else if (pick === help) {
    vscode.env.openExternal(vscode.Uri.parse(SETUP_URL));
  } else if (pick === quiet) {
    await context.globalState.update(QUIET_KEY, true);
  }
}

async function stop() {
  const c = client;
  client = undefined;
  if (c) {
    try {
      await c.stop();
    } catch {
      // Already gone.
    }
  }
}

async function restart(context) {
  await stop();
  await start(context);
}

function deactivate() {
  return stop();
}

module.exports = { activate, deactivate };

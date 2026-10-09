import * as vscode from "vscode";
import { LanguageClient, type LanguageClientOptions, type ServerOptions } from "vscode-languageclient/node";
import { SERVER, downloadLatest, expandHome, findOnPath, isFile, newestDownloaded } from "./binary";

/** Languages this extension defines; the server also colors these (semantic tokens). */
const OWN_LANGUAGES = ["klipper", "gcode"];
/**
 * Language ids of other Klipper extensions (dannymcgee.klipper). The server
 * adds hover and go-to-definition to their files, but leaves colors to them.
 */
const OTHER_LANGUAGES = ["klipper-cfg", "klipper-gcode"];

let client: LanguageClient | undefined;
let output: vscode.OutputChannel;

function settings() {
  return vscode.workspace.getConfiguration("klipper");
}

async function resolveServer(context: vscode.ExtensionContext): Promise<string> {
  const configured = settings().get<string>("server.path", "").trim();
  if (configured) {
    const p = expandHome(configured);
    if (!isFile(p)) throw new Error(`klipper.server.path is not a file: ${p}`);
    return p;
  }
  const onPath = findOnPath();
  if (onPath) return onPath;

  const storage = context.globalStorageUri.fsPath;
  try {
    return await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: "Klipper: fetching klipper-ls" },
      () => downloadLatest(storage, (m) => output.appendLine(m)),
    );
  } catch (error) {
    const earlier = newestDownloaded(storage);
    if (earlier) {
      output.appendLine(`Using previously downloaded server (${error})`);
      return earlier;
    }
    throw error;
  }
}

/** Only non-empty settings are sent, so the server's own defaults apply otherwise. */
function initializationOptions() {
  const c = settings();
  const options: Record<string, string | boolean> = {
    downloadDocs: c.get<boolean>("downloadDocs", true),
    diagnostics: c.get<boolean>("diagnostics", true),
  };
  for (const key of ["klipperDocs", "klipperConfig", "marlinDocs"]) {
    const value = c.get<string>(key, "").trim();
    if (value) options[key] = expandHome(value);
  }
  return options;
}

async function start(context: vscode.ExtensionContext): Promise<void> {
  let command: string;
  try {
    command = await resolveServer(context);
  } catch (error) {
    output.appendLine(String(error));
    const choice = await vscode.window.showErrorMessage(
      `Klipper: could not start ${SERVER}. ${error instanceof Error ? error.message : error}`,
      "Show Output",
    );
    if (choice) output.show();
    return;
  }
  output.appendLine(`Starting ${command}`);

  const serverOptions: ServerOptions = { command, args: [] };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [...OWN_LANGUAGES, ...OTHER_LANGUAGES].map((language) => ({ scheme: "file", language })),
    middleware: {
      // Another extension's grammar owns the colors for its languages; two
      // sets of semantic colors on top of each other would fight.
      provideDocumentSemanticTokens: (doc, token, next) => (OWN_LANGUAGES.includes(doc.languageId) ? next(doc, token) : undefined),
      provideDocumentRangeSemanticTokens: (doc, range, token, next) =>
        OWN_LANGUAGES.includes(doc.languageId) ? next(doc, range, token) : undefined,
    },
    initializationOptions: initializationOptions(),
    outputChannel: output,
  };
  client = new LanguageClient("klipper", "Klipper", serverOptions, clientOptions);
  await client.start();
}

async function stop(): Promise<void> {
  const running = client;
  client = undefined;
  await running?.stop();
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  output = vscode.window.createOutputChannel("Klipper");
  const restart = async () => {
    await stop();
    await start(context);
  };
  context.subscriptions.push(
    output,
    vscode.commands.registerCommand("klipper.restartServer", restart),
    vscode.commands.registerCommand("klipper.showOutput", () => output.show()),
    // The server reads its options once at startup, so a settings change means a restart.
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("klipper") && !e.affectsConfiguration("klipper.trace")) void restart();
    }),
  );
  await start(context);
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}

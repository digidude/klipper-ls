import * as assert from "node:assert/strict";
import * as path from "node:path";
import * as vscode from "vscode";

const fixtures = path.resolve(__dirname, "../../test/fixtures");

async function until<T>(what: string, probe: () => Promise<T | undefined>, ms = 30000): Promise<T> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    const value = await probe();
    if (value !== undefined) return value;
    await new Promise((r) => setTimeout(r, 300));
  }
  throw new Error(`timed out waiting for ${what}`);
}

const text = (hovers: vscode.Hover[]) =>
  hovers
    .flatMap((h) => h.contents)
    .map((c) => (typeof c === "string" ? c : c.value))
    .join("\n");

export async function run(): Promise<void> {
  const bin = process.env.KLIPPER_LS_BIN;
  assert.ok(bin, "set KLIPPER_LS_BIN to a klipper-ls binary");
  const config = vscode.workspace.getConfiguration("klipper");
  await config.update("server.path", bin, vscode.ConfigurationTarget.Global);
  if (process.env.KLIPPER_DOCS) await config.update("klipperDocs", process.env.KLIPPER_DOCS, vscode.ConfigurationTarget.Global);
  else await config.update("downloadDocs", false, vscode.ConfigurationTarget.Global);

  const doc = await vscode.workspace.openTextDocument(path.join(fixtures, "printer.cfg"));
  await vscode.window.showTextDocument(doc);
  assert.equal(doc.languageId, "klipper");

  const at = (needle: string, delta = 1) => {
    const offset = doc.getText().indexOf(needle);
    assert.ok(offset >= 0, `${needle} not in fixture`);
    return doc.positionAt(offset + delta);
  };

  // Your macro: hover (once the server is up) and definition across [include].
  const macro = await until("macro hover", async () => {
    const hovers = await vscode.commands.executeCommand<vscode.Hover[]>("vscode.executeHoverProvider", doc.uri, at("HEAT_SOAK"));
    return hovers?.length ? text(hovers) : undefined;
  });
  assert.match(macro, /HEAT_SOAK/);

  const defs = await vscode.commands.executeCommand<(vscode.Location | vscode.LocationLink)[]>(
    "vscode.executeDefinitionProvider", doc.uri, at("HEAT_SOAK"));
  assert.ok(defs.length > 0, "no definition");
  const target = "targetUri" in defs[0] ? defs[0].targetUri : defs[0].uri;
  assert.equal(path.basename(target.fsPath), "macros.cfg");

  // A config option, answered from Klipper's docs.
  if (process.env.KLIPPER_DOCS) {
    const builtin = await vscode.commands.executeCommand<vscode.Hover[]>("vscode.executeHoverProvider", doc.uri, at("M140"));
    assert.match(text(builtin), /M140/);
    const option = await vscode.commands.executeCommand<vscode.Hover[]>("vscode.executeHoverProvider", doc.uri, at("sensor_pin"));
    assert.ok(text(option).length > 0, "no hover for sensor_pin");
  }
  console.log("e2e ok");
}

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
  // Semantic tokens: highlighting comes from the server's parse, in config...
  const legend = await vscode.commands.executeCommand<vscode.SemanticTokensLegend>(
    "vscode.provideDocumentSemanticTokensLegend", doc.uri);
  assert.ok(legend?.tokenTypes.includes("function"), "no semantic token legend");
  const decode = (document: vscode.TextDocument, tokens: vscode.SemanticTokens) => {
    const out: string[] = [];
    let line = 0, col = 0;
    for (let i = 0; i < tokens.data.length; i += 5) {
      line += tokens.data[i];
      col = tokens.data[i] === 0 ? col + tokens.data[i + 1] : tokens.data[i + 1];
      const text = document.lineAt(line).text.substr(col, tokens.data[i + 2]);
      out.push(`${text}:${legend.tokenTypes[tokens.data[i + 3]]}`);
    }
    return out;
  };
  const cfgTokens = await vscode.commands.executeCommand<vscode.SemanticTokens>("vscode.provideDocumentSemanticTokens", doc.uri);
  const cfg = decode(doc, cfgTokens);
  for (const expected of ["heater_bed:type", "PRINT_START:function", "M140:keyword", "HEAT_SOAK:function", "BED:variable"]) {
    assert.ok(cfg.includes(expected), `config token ${expected} missing from ${cfg.join(" ")}`);
  }

  // ...and in .gcode, where only the requested lines are looked at.
  const gdoc = await vscode.workspace.openTextDocument(path.join(fixtures, "sample.gcode"));
  assert.equal(gdoc.languageId, "gcode");
  const gcode = await until("gcode tokens", async () => {
    const t = await vscode.commands.executeCommand<vscode.SemanticTokens>(
      "vscode.provideDocumentRangeSemanticTokens", gdoc.uri, new vscode.Range(1, 0, 3, 0));
    return t?.data.length ? decode(gdoc, t) : undefined;
  });
  assert.ok(gcode.includes("G1:keyword") && gcode.includes("X:parameter"), gcode.join(" "));
  assert.ok(!gcode.includes("PRINT_START:function"), "line outside the range was highlighted");
  console.log("e2e ok");
}

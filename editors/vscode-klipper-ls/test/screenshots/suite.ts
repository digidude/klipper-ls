import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";

const out = process.env.SHOTS_OUT!;
const title = process.env.SHOTS_TITLE!;
const swift = path.resolve(__dirname, "../../test/screenshots/window-id.swift");
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

interface Shot {
  name: string;
  file: string;
  /** The cursor goes one character into the first match of `at`. */
  at: string;
  /** What to show: the hover, or a peeked definition. */
  show: "hover" | "peek";
  /** A phrase that must appear in the hover, so a shot never captures an empty box. */
  expect?: string;
}

const SHOTS: Shot[] = [
  { name: "hover-macro", file: "printer.cfg", at: "HEAT_SOAK MINUTES", show: "hover", expect: "Wait for the bed" },
  { name: "peek-definition", file: "printer.cfg", at: "HEAT_SOAK MINUTES", show: "peek" },
  { name: "hover-status-field", file: "printer.cfg", at: "homed_axes !=", show: "hover", expect: "homed" },
  { name: "hover-config-option", file: "printer.cfg", at: "rotation_distance", show: "hover", expect: "Distance" },
  { name: "hover-gcode-ignored-parameter", file: "sample.gcode", at: "M140 S60", show: "hover", expect: "Ignored by Klipper" },
  { name: "hover-gcode-unknown-code", file: "sample.gcode", at: "M900", show: "hover", expect: "Unknown command" },
];

function windowId(): string {
  try {
    return execFileSync("swift", [swift, title], { encoding: "utf8" }).trim();
  } catch {
    throw new Error(
      "Could not find the VS Code window. Give the app running this Screen Recording permission " +
        "(System Settings > Privacy & Security > Screen & System Audio Recording), then run again.",
    );
  }
}

function capture(file: string) {
  const id = windowId();
  try {
    execFileSync("screencapture", ["-x", "-o", `-l${id}`, file]);
  } catch {
    throw new Error("screencapture failed; check Screen Recording permission");
  }
  if (!fs.existsSync(file) || fs.statSync(file).size < 5000) throw new Error(`empty screenshot: ${file}`);
}

async function hoverText(uri: vscode.Uri, pos: vscode.Position): Promise<string> {
  const hovers = (await vscode.commands.executeCommand<vscode.Hover[]>("vscode.executeHoverProvider", uri, pos)) ?? [];
  return hovers
    .flatMap((h) => h.contents)
    .map((c) => (typeof c === "string" ? c : c.value))
    .join("\n");
}

export async function run(): Promise<void> {
  fs.mkdirSync(out, { recursive: true });
  const root = vscode.workspace.workspaceFolders![0].uri.fsPath;
  for (const cmd of ["workbench.action.closeSidebar", "workbench.action.closePanel", "workbench.action.closeAuxiliaryBar"]) {
    await vscode.commands.executeCommand(cmd);
  }

  for (const shot of SHOTS) {
    const doc = await vscode.workspace.openTextDocument(path.join(root, shot.file));
    const editor = await vscode.window.showTextDocument(doc, { preview: false });
    const pos = doc.positionAt(doc.getText().indexOf(shot.at) + 1);
    editor.selection = new vscode.Selection(pos, pos);
    editor.revealRange(new vscode.Range(pos, pos), vscode.TextEditorRevealType.InCenter);

    // The server answers once its docs are loaded; wait for a real answer.
    let text = "";
    for (let i = 0; i < 100 && !text; i++) {
      text = await hoverText(doc.uri, pos);
      if (!text) await sleep(300);
    }
    if (shot.expect && !text.includes(shot.expect)) throw new Error(`${shot.name}: hover lacks "${shot.expect}":\n${text}`);

    await vscode.commands.executeCommand(shot.show === "hover" ? "editor.action.showHover" : "editor.action.peekDefinition");
    await sleep(1500);
    if (!process.env.SHOTS_DRY) capture(path.join(out, `${shot.name}.png`));
    await vscode.commands.executeCommand("closeReferenceSearch");
    await vscode.commands.executeCommand("editor.action.hideHover");
    console.log(`captured ${shot.name}`);
  }
}

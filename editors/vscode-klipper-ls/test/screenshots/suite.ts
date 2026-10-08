import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";

const out = process.env.SHOTS_OUT!;
const title = process.env.SHOTS_TITLE!;
const hideNotifications = async () => {
  await vscode.commands.executeCommand("notifications.clearAll");
  await vscode.commands.executeCommand("notifications.hideToasts");
};
const swift = path.resolve(__dirname, "../../test/screenshots/window-id.swift");
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

interface Shot {
  name: string;
  file: string;
  /** The cursor goes one character into the first match of `at`. */
  at: string;
  /** Points of height to keep below the title bar; the rest is empty editor. */
  keep: number;
  /** Points of width to keep. */
  width: number;
  /** What to show: the hover, or a peeked definition. */
  show: "hover" | "peek";
  /** A phrase that must appear in the hover, so a shot never captures an empty box. */
  expect?: string;
}

const SHOTS: Shot[] = [
  { name: "hover-macro", file: "printer.cfg", at: "HEAT_SOAK MINUTES", keep: 380, width: 1200, show: "hover", expect: "Wait for the bed" },
  { name: "peek-definition", file: "printer.cfg", at: "HEAT_SOAK MINUTES", keep: 650, width: 1440, show: "peek" },
  { name: "hover-status-field", file: "printer.cfg", at: "homed_axes !=", keep: 280, width: 1200, show: "hover", expect: "homed" },
  { name: "hover-config-option", file: "printer.cfg", at: "rotation_distance", keep: 380, width: 1200, show: "hover", expect: "Distance" },
  { name: "hover-gcode-ignored-parameter", file: "sample.gcode", at: "M140 S60", keep: 430, width: 1200, show: "hover", expect: "Ignored by Klipper" },
  { name: "hover-gcode-unknown-code", file: "sample.gcode", at: "M500", keep: 380, width: 1200, show: "hover", expect: "Unknown command" },
];

/** The window's CoreGraphics id and its width in points. */
function findWindow(): { id: string; widthPoints: number } {
  try {
    const [id, width] = execFileSync("swift", [swift, title], { encoding: "utf8" }).trim().split(" ");
    return { id, widthPoints: Number(width) };
  } catch {
    throw new Error(
      "Could not find the VS Code window. Give the app running this Screen Recording permission " +
        "(System Settings > Privacy & Security > Screen & System Audio Recording), then run again.",
    );
  }
}

const pixelWidth = (file: string) =>
  Number(/pixelWidth: (\d+)/.exec(execFileSync("sips", ["-g", "pixelWidth", file], { encoding: "utf8" }))![1]);

async function capture(file: string, keep: number, width: number) {
  // Window lookups and captures occasionally fail right after a window repaint.
  let last: unknown;
  for (let attempt = 0; attempt < 4; attempt++) {
    try {
      const window = findWindow();
      execFileSync("screencapture", ["-x", "-o", `-l${window.id}`, file]);
      if (fs.existsSync(file) && fs.statSync(file).size > 5000) {
        // Sizes are in points; the capture is 1x or 2x depending on the display.
        const scale = pixelWidth(file) / window.widthPoints;
        const px = (points: number) => String(Math.round(points * scale));
        // Drop the title bar ("Extension Development Host" in a test run) and the empty editor below.
        execFileSync("swift", [path.resolve(__dirname, "../../test/screenshots/crop.swift"), file, px(32), px(keep), px(width)]);
        return;
      }
    } catch (error) {
      last = error;
    }
    await sleep(800);
  }
  throw new Error(`could not capture ${file} (check Screen Recording permission): ${last}`);
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
    // Near the top, so the hover opens below the line instead of covering the code above it.
    editor.revealRange(new vscode.Range(pos, pos), vscode.TextEditorRevealType.AtTop);

    // The server answers once its docs are loaded; wait for a real answer.
    let text = "";
    for (let i = 0; i < 100 && !text; i++) {
      text = await hoverText(doc.uri, pos);
      if (!text) await sleep(300);
    }
    if (shot.expect && !text.includes(shot.expect)) throw new Error(`${shot.name}: hover lacks "${shot.expect}":\n${text}`);

    await hideNotifications();
    await sleep(500);
    await vscode.commands.executeCommand(shot.show === "hover" ? "editor.action.showHover" : "editor.action.peekDefinition");
    await sleep(1500);
    if (!process.env.SHOTS_DRY) await capture(path.join(out, `${shot.name}.png`), shot.keep, shot.width);
    await vscode.commands.executeCommand("closeReferenceSearch");
    await vscode.commands.executeCommand("editor.action.hideHover");
    console.log(`captured ${shot.name}`);
  }
}

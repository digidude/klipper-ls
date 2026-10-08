// Takes the README screenshots: a real VS Code with klipper-ls and
// dannymcgee.klipper installed side by side (his grammar colors the text, klipper-ls
// supplies hover and go-to-definition). macOS only; needs Screen Recording
// permission for the terminal or app running it.
//
//   KLIPPER_LS_BIN=../../target/debug/klipper-ls KLIPPER_DOCS=../../klipper/docs \
//     npm run screenshots
//
// KLIPPER_OTHER_EXTENSION defaults to the newest ~/.vscode/extensions/dannymcgee.klipper-*.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { runTests } from "@vscode/test-electron";

function otherExtension(): string {
  if (process.env.KLIPPER_OTHER_EXTENSION) return process.env.KLIPPER_OTHER_EXTENSION;
  const dir = path.join(os.homedir(), ".vscode/extensions");
  const found = fs.readdirSync(dir).filter((n) => n.startsWith("dannymcgee.klipper-")).sort();
  if (!found.length) throw new Error("install dannymcgee.klipper first (or set KLIPPER_OTHER_EXTENSION)");
  return path.join(dir, found[found.length - 1]);
}

async function main() {
  const root = path.resolve(__dirname, "../..");
  const bin = path.resolve(process.env.KLIPPER_LS_BIN ?? "");
  const docs = process.env.KLIPPER_DOCS ? path.resolve(process.env.KLIPPER_DOCS) : "";
  if (!fs.existsSync(bin)) throw new Error("set KLIPPER_LS_BIN to a klipper-ls binary");

  // A scratch copy, so the settings written below never touch the repo.
  // A short, fixed path: the peek-definition title shows it in the image.
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "klipper-shots-"));
  const workspace = "/tmp/printer_config";
  fs.rmSync(workspace, { recursive: true, force: true });
  fs.cpSync(path.join(root, "test/screenshots/workspace"), workspace, { recursive: true });
  fs.mkdirSync(path.join(workspace, ".vscode"));
  fs.writeFileSync(
    path.join(workspace, ".vscode/settings.json"),
    JSON.stringify(
      {
        "files.associations": { "*.cfg": "klipper-cfg", "*.gcode": "klipper-gcode" },
        "workbench.colorTheme": "Default Dark Modern",
        "workbench.startupEditor": "none",
        "workbench.tips.enabled": false,
        "editor.minimap.enabled": false,
        "editor.hover.above": false,
        "editor.fontSize": 13,
        "editor.lineHeight": 22,
        "editor.glyphMargin": false,
        "editor.folding": false,
        "breadcrumbs.enabled": false,
        "window.commandCenter": false,
        "window.title": "${activeEditorShort} \u2014 ${rootName}",
        "workbench.layoutControl.enabled": false,
        "chat.disableAIFeatures": true,
        "update.mode": "none",
        "extensions.ignoreRecommendations": true,
        "klipper.server.path": bin,
        ...(docs ? { "klipper.klipperDocs": docs } : {}),
      },
      null,
      2,
    ),
  );

  // A fixed, smaller window (points): less empty editor in every image.
  const globalStorage = path.join(work, "user-data/User/globalStorage");
  fs.mkdirSync(globalStorage, { recursive: true });
  fs.writeFileSync(
    path.join(globalStorage, "storage.json"),
    JSON.stringify({ windowsState: { lastActiveWindow: { folder: `file://${workspace}`, uiState: { mode: 1, x: 80, y: 60, width: 1100, height: 660 } } } }),
  );

  await runTests({
    extensionDevelopmentPath: root,
    extensionTestsPath: path.resolve(__dirname, "suite"),
    launchArgs: [
      workspace,
      // An empty extensions folder: only the two extensions under development load.
      `--extensions-dir=${path.join(work, "extensions")}`,
      "--disable-workspace-trust",
      `--extensionDevelopmentPath=${otherExtension()}`,
      `--user-data-dir=${path.join(work, "user-data")}`,
    ],
    extensionTestsEnv: { SHOTS_OUT: path.resolve(root, "../../docs/screenshots"), SHOTS_TITLE: "printer_config" },
  });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});

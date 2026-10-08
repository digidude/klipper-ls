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
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "klipper-shots-"));
  const workspace = path.join(work, "printer_config");
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
        "editor.fontSize": 15,
        "editor.lineHeight": 24,
        "editor.glyphMargin": false,
        "editor.folding": false,
        "breadcrumbs.enabled": false,
        "window.commandCenter": false,
        "klipper.server.path": bin,
        ...(docs ? { "klipper.klipperDocs": docs } : {}),
      },
      null,
      2,
    ),
  );

  await runTests({
    extensionDevelopmentPath: root,
    extensionTestsPath: path.resolve(__dirname, "suite"),
    launchArgs: [
      workspace,
      "--disable-extensions",
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

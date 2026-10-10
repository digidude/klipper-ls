// Takes the README screenshots: a real VS Code with klipper-ls and Klipper
// Syntax (the syntax/ submodule) loaded side by side: its grammar colors the
// text, klipper-ls supplies hover, go-to-definition and diagnostics. macOS
// only; needs Screen Recording permission for the terminal or app running it.
//
//   KLIPPER_LS_BIN=../../target/debug/klipper-ls KLIPPER_DOCS=../../klipper/docs \
//     npm run screenshots
//
// Captures go through the window's DevTools port (SHOTS_NATIVE=1 uses screencapture
// instead and needs Screen Recording permission). KLIPPER_OTHER_EXTENSION
// defaults to syntax/vscode. Point it at
// ~/.vscode/extensions/dannymcgee.klipper-* to compare, and set KLIPPER_THEME
// to try another colour theme.
import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { runTests } from "@vscode/test-electron";

function otherExtension(): string {
  if (process.env.KLIPPER_OTHER_EXTENSION) return process.env.KLIPPER_OTHER_EXTENSION;
  const bundled = path.resolve(__dirname, "../../../../syntax/vscode");
  if (!fs.existsSync(path.join(bundled, "package.json"))) {
    throw new Error("run `git submodule update --init` (syntax/vscode), or set KLIPPER_OTHER_EXTENSION");
  }
  return bundled;
}

async function main() {
  const root = path.resolve(__dirname, "../..");
  const port = Number(process.env.SHOTS_CDP_PORT ?? 9333);
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
        "files.associations": { "*.cfg": "klipper", "*.gcode": "gcode" },
        "workbench.colorTheme": process.env.KLIPPER_THEME ?? "Default Dark Modern",
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
      // Captured over DevTools: no Screen Recording permission needed.
      `--remote-debugging-port=${port}`,
    ],
    extensionTestsEnv: {
      SHOTS_OUT: path.resolve(root, "../../docs/screenshots"),
      SHOTS_TITLE: "printer_config",
      SHOTS_CDP_PORT: String(port),
      SHOTS_FRAMES: path.join(work, "frames"),
      ...(process.env.SHOTS_NATIVE ? { SHOTS_NATIVE: "1" } : {}),
    },
  });

  // The hero GIF is assembled from the frames the suite wrote.
  const frames = path.join(work, "frames");
  if (fs.existsSync(path.join(frames, "frames.json"))) {
    const script = path.resolve(root, "test/screenshots/make-gif.py");
    const gif = path.resolve(root, "../../docs/screenshots/hero.gif");
    try {
      console.log(execFileSync("uv", ["run", "--with", "pillow", "python", script, frames, gif], { encoding: "utf8" }).trim());
    } catch (error) {
      console.warn(`hero.gif not built (needs uv): uv run --with pillow python ${script} ${frames} ${gif}\n${error}`);
    }
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});

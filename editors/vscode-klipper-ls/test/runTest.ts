// Launches a real VS Code with the extension loaded and runs suite/index.
//   KLIPPER_LS_BIN=/path/to/klipper-ls [KLIPPER_DOCS=/path/to/klipper/docs] npm run test:e2e
import * as path from "node:path";
import { runTests } from "@vscode/test-electron";

async function main() {
  const root = path.resolve(__dirname, "..");
  await runTests({
    extensionDevelopmentPath: root,
    extensionTestsPath: path.resolve(__dirname, "suite/index"),
    launchArgs: [path.join(root, "test/fixtures"), "--disable-extensions"],
  });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});

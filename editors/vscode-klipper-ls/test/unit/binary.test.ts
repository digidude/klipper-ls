import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { findOnPath, newestDownloaded, releaseTarget } from "../../src/binary.ts";

test("release targets match the klipper-ls release workflow", () => {
  assert.equal(releaseTarget("darwin", "arm64"), "aarch64-apple-darwin");
  assert.equal(releaseTarget("darwin", "x64"), "x86_64-apple-darwin");
  assert.equal(releaseTarget("linux", "x64"), "x86_64-unknown-linux-gnu");
  assert.equal(releaseTarget("linux", "arm64"), "aarch64-unknown-linux-gnu");
  assert.equal(releaseTarget("win32", "x64"), undefined);
});

test("findOnPath checks PATH, then Cargo's bin", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "kls-"));
  const cargo = path.join(dir, "cargo/bin");
  fs.mkdirSync(cargo, { recursive: true });
  assert.equal(findOnPath({ PATH: "/nonexistent", CARGO_HOME: path.join(dir, "cargo") }), undefined);
  fs.writeFileSync(path.join(cargo, "klipper-ls"), "");
  assert.equal(findOnPath({ PATH: "/nonexistent", CARGO_HOME: path.join(dir, "cargo") }), path.join(cargo, "klipper-ls"));
});

test("newestDownloaded sorts versions numerically", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "kls-"));
  for (const v of ["0.9.0", "0.10.0"]) {
    fs.mkdirSync(path.join(dir, `klipper-ls-${v}`));
    fs.writeFileSync(path.join(dir, `klipper-ls-${v}`, "klipper-ls"), "");
  }
  assert.equal(newestDownloaded(dir), path.join(dir, "klipper-ls-0.10.0", "klipper-ls"));
  assert.equal(newestDownloaded(path.join(dir, "missing")), undefined);
});

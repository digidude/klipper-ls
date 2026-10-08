// Finding (or fetching) the klipper-ls executable. Kept free of `vscode`
// imports so the pure parts can be unit-tested with plain node.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { execFile } from "node:child_process";

export const SERVER = "klipper-ls";
export const RELEASES_API = "https://api.github.com/repos/digidude/klipper-ls/releases/latest";

/** The Rust target triple the klipper-ls release workflow builds for this machine. */
export function releaseTarget(platform = process.platform, arch = process.arch): string | undefined {
  const table: Record<string, string> = {
    "darwin-arm64": "aarch64-apple-darwin",
    "darwin-x64": "x86_64-apple-darwin",
    "linux-x64": "x86_64-unknown-linux-gnu",
    "linux-arm64": "aarch64-unknown-linux-gnu",
  };
  return table[`${platform}-${arch}`];
}

export function expandHome(p: string): string {
  return p === "~" || p.startsWith("~/") ? path.join(os.homedir(), p.slice(1)) : p;
}

export function isFile(p: string): boolean {
  try {
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
}

/**
 * `klipper-ls` on PATH, plus Cargo's bin folder: VS Code launched from the
 * Dock/Finder doesn't inherit the shell's PATH, so `cargo install` would
 * otherwise appear not to work.
 */
export function findOnPath(env: NodeJS.ProcessEnv = process.env): string | undefined {
  const dirs = (env.PATH ?? "").split(path.delimiter).filter(Boolean);
  dirs.push(path.join(env.CARGO_HOME ?? path.join(os.homedir(), ".cargo"), "bin"));
  for (const dir of dirs) {
    const candidate = path.join(dir, SERVER);
    if (isFile(candidate)) return candidate;
  }
  return undefined;
}

/** Newest `klipper-ls-<version>/klipper-ls` under `dir`, from an earlier download. */
export function newestDownloaded(dir: string): string | undefined {
  let names: string[];
  try {
    names = fs.readdirSync(dir);
  } catch {
    return undefined;
  }
  const found = names
    .filter((n) => n.startsWith(`${SERVER}-`) && isFile(path.join(dir, n, SERVER)))
    .sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
  const last = found.pop();
  return last && path.join(dir, last, SERVER);
}

interface Release {
  tag_name: string;
  assets: { name: string; browser_download_url: string }[];
}

function untar(archive: string, dest: string): Promise<void> {
  return new Promise((resolve, reject) =>
    execFile("tar", ["-xzf", archive, "-C", dest], (err) => (err ? reject(err) : resolve())),
  );
}

/** Download the latest release's binary for this platform into `storageDir`. */
export async function downloadLatest(storageDir: string, log: (m: string) => void): Promise<string> {
  const target = releaseTarget();
  if (!target) {
    throw new Error(
      `No prebuilt ${SERVER} for ${process.platform}/${process.arch}. ` +
        `Build it with "cargo install --git https://github.com/digidude/klipper-ls" or set klipper.server.path.`,
    );
  }
  const headers = { "User-Agent": "vscode-klipper-ls", Accept: "application/vnd.github+json" };
  const res = await fetch(RELEASES_API, { headers });
  if (!res.ok) throw new Error(`GitHub releases lookup failed: HTTP ${res.status}`);
  const release = (await res.json()) as Release;
  const assetName = `${SERVER}-${target}.tar.gz`;
  const asset = release.assets.find((a) => a.name === assetName);
  if (!asset) throw new Error(`Release ${release.tag_name} has no ${assetName}`);

  const dir = path.join(storageDir, `${SERVER}-${release.tag_name.replace(/^v/, "")}`);
  const binary = path.join(dir, SERVER);
  if (isFile(binary)) return binary;

  log(`Downloading ${assetName} (${release.tag_name})`);
  const download = await fetch(asset.browser_download_url, { headers: { "User-Agent": headers["User-Agent"] } });
  if (!download.ok) throw new Error(`Downloading ${assetName} failed: HTTP ${download.status}`);
  fs.mkdirSync(dir, { recursive: true });
  const archive = path.join(dir, assetName);
  fs.writeFileSync(archive, Buffer.from(await download.arrayBuffer()));
  await untar(archive, dir);
  fs.rmSync(archive);
  fs.chmodSync(binary, 0o755);
  // Drop older versions.
  for (const name of fs.readdirSync(storageDir)) {
    if (name.startsWith(`${SERVER}-`) && path.join(storageDir, name) !== dir) {
      fs.rmSync(path.join(storageDir, name), { recursive: true, force: true });
    }
  }
  return binary;
}

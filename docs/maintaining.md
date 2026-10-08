# Maintaining klipper-ls

A runbook for whoever holds the keys. Layout and day-to-day development are in [CONTRIBUTING.md](../CONTRIBUTING.md); who maintains what is in [MAINTAINERS.md](../MAINTAINERS.md).

## One thing to know first

The server and both editor extensions share a small contract. Break it and the editors silently stop working, so change it in one commit:

- **Release asset names:** `klipper-ls-<target>.tar.gz` for the four targets (`aarch64`/`x86_64` on `apple-darwin` and `unknown-linux-gnu`). Both extensions download these by name from the latest release.
- **Language ids:** `klipper` and `gcode` (hover code fences use the name "Klipper"). The VS Code extension also attaches to `klipper-cfg` and `klipper-gcode` (the dannymcgee.klipper extension). The server treats any id ending in `gcode` as G-code.
- **`initializationOptions`:** `klipperDocs`, `klipperConfig`, `marlinDocs`, `downloadDocs`.
- **Grammar revision:** `editors/zed-klipper-ls/extension.toml` pins the grammar by commit, with `path = "grammar"`.

## Setup

- Rust stable, plus `rustup target add wasm32-wasip2` for the Zed shim.
- Node 22 for the grammar and the VS Code extension.
- A shallow Klipper checkout for the real-data tests (never commit it, it is GPL):
  `git clone --depth 1 https://github.com/Klipper3d/klipper.git klipper`
- Optional: real configs in `test-data/` (also gitignored). See CONTRIBUTING.md.

## Before any release

```sh
cargo test --locked && cargo clippy --locked -- -D warnings
(cd grammar && npx tree-sitter test)
(cd editors/zed-klipper-ls && cargo check --target wasm32-wasip2)
(cd editors/vscode-klipper-ls && npm ci && npm run typecheck && npm run test:unit)
# against a real config and real slicer output (see CONTRIBUTING.md for the variables)
KLIPPER_CONFIG=... KLIPPER_DOCS=klipper/docs cargo test coverage -- --ignored --nocapture
GCODE_DIR=...      KLIPPER_DOCS=klipper/docs cargo test gcode_coverage -- --ignored --nocapture
```

The expected baseline: every command and config key has a hover, except keys that only a plugin defines (such as `[beacon]`) and `toolhead.estimated_print_time`, which is undocumented upstream. The VS Code end-to-end test (`npm run test:e2e`, needs `KLIPPER_LS_BIN`) launches a real VS Code.

## Cutting a release

One tag releases everything, and the versions move together.

1. Bump the version in:
   - `Cargo.toml` (then run `cargo build` so `Cargo.lock` follows),
   - `editors/zed-klipper-ls/Cargo.toml` and `extension.toml`,
   - `editors/vscode-klipper-ls/package.json`.
2. If `grammar/` changed since the last release, push it first, then set `rev` in `extension.toml` to the pushed commit.
3. Commit, push, wait for CI to pass.
4. `git tag vX.Y.Z && git push origin vX.Y.Z`
5. The `release` workflow builds the four binaries and the `.vsix` (stamped from the tag) and attaches them to one GitHub release.

**Check the release.** Download the assets, run the macOS one with `--version`, confirm that `file` reports the right architecture for each, and check the `.vsix`. Then run the VS Code end-to-end test with the released binary. Anonymous downloads only work on a public repo, so check them after any visibility change.

## Regenerating screenshots

`npm run screenshots` in `editors/vscode-klipper-ls`. macOS only. It drives a real VS Code with dannymcgee.klipper installed so every box shown comes from klipper-ls. It needs Screen Recording permission and, to size the window, Accessibility permission for the app running it. `SHOTS_DRY=1` checks every hover without capturing. The images show short excerpts of Klipper's and Marlin's docs; keep them short, and do not paste doc text into repo text files.

## Keeping up with Klipper and Marlin

Most of the recurring work is here.

- Klipper's docs vary in format (`#### NAME`, ``#### `NAME` ``, `- \`CMD …\`: text`, shared bullets). A format change shows up as the coverage test reporting a command without a hover.
- Commands that the server reports as "Klipper doesn't run this" depend on scanning `klippy/` for handlers (`def cmd_X`, `register_command('X'`). If Klipper changes how it registers commands, this is where it breaks.
- `printer.*` hover reads `Status_Reference.md`, which is organised by topic, not object.
- Marlin's reference is read from its docs repo (YAML headers). The download is cached under the user's cache folder.
- The Jinja filter list in `editors/zed-klipper-ls/languages/klipper/highlights.scm` follows the Jinja version Klipper ships (2.11). Update it if that changes.
- Highlighting: the server runs that same `highlights.scm` for semantic tokens (`src/highlight.rs`), so a capture renamed there must be updated in `map_capture`.

## Rules that protect the licence

- Never vendor, embed or compile in Klipper or Marlin docs.
- Test fixtures that look like docs keep the structure but use their own wording. Check for verbatim overlap before committing.
- Keep dependencies permissive (MIT / Apache-2.0).

## Fragile spots

- `grammar/src/scanner.c` is deliberately stateless; each call re-verifies the whole shape. Keep it that way.
- `lsp-server`: `drop(connection)` before `io_threads.join()`, or shutdown hangs.
- The unauthenticated GitHub API allows 60 requests an hour per address, which is the limit for each user's first download of the binary.

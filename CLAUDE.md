# CLAUDE.md

`klipper-ls`: an editor-neutral language server for Klipper config and G-code, plus the tree-sitter grammar it parses with. Clients live in other repos: [zed-klipper](https://github.com/digidude/zed-klipper) and [vscode-klipper-ls](https://github.com/digidude/vscode-klipper-ls). README is user-facing; CONTRIBUTING.md has the layout and workflows; this file is the working context.

## Commands

```sh
cargo test && cargo clippy                       # keep clippy at zero warnings
cargo install --path .                           # then restart the language server in the editor
python3 scripts/probe.py <file> "needle" ...     # LSP round-trip; hovers 1 char into each needle
cd grammar && npx tree-sitter generate && npx tree-sitter test
cargo test coverage -- --ignored --nocapture     # real-data checks; env vars in CONTRIBUTING.md
```

Node: this machine's nvm lives in `~/.local/share/nvm`; `nvm use 22.11.0` before `npx`.

## Decisions and why

- **License stays MIT.** Klipper's and Marlin's docs are GPL-3.0; they are read at runtime (local checkout or a one-time download into `~/Library/Caches/klipper-ls/`) and never vendored, embedded or compiled in. Don't add doc text to the repo. Relicensing is a one-way door. Marlin's docs as a runtime source was reconfirmed 2026-10-06; don't remove or make it opt-in unprompted.
- **Release asset names are a contract** (`klipper-ls-<target>.tar.gz`): both client packages download them.
- **Jinja is parsed in the grammar, not injected.** Klipper uses `{expr}` and G-code and Jinja share lines.
- **Newlines go through the external scanner** (`_newline` / `_continuation` / `_jinja_newline`); EOF emits a zero-width `_newline` only when valid; no rule may consist of only a newline.
- **The scanner also lexes** option keys, `NAME=` parameter names, `X10`/`S{..}` params, and pins; each call re-verifies the whole shape, so it stays stateless.
- **Hover layering:** user macros, then Klipper docs (what actually runs), then Marlin per-parameter detail with Klipper-ignored params folded into one line. "Klipper won't run this" is asserted only when `klippy/` source was scanned; docs-only wording hedges ("most likely").
- **`printer.*` hover** reads `Status_Reference.md`, organised by topic, not object. "Not documented" is a fact about the doc, not Klipper.
- **Targets are byte ranges** (`features::Target`), so `.cfg` (syntax tree) and `.gcode` (line under cursor, `gcode.rs`) share hover/definition.
- **.gcode never gets a whole-file parse:** incremental sync + single-line lookup; tested on a 32 MB / 1.2M-line file.
- **The macro index is rebuilt per request** from `printer.cfg` following `[include]`s (≈1 ms), not cached.

## Gotchas

- tree-sitter lexing: precedence beats match length (then length, string-over-regex, rule order); numbers use `token(prec(1, …))`.
- `lsp-server`: `drop(connection)` before `io_threads.join()`, or shutdown hangs.
- `while let Some(x) = mutex.lock().unwrap().pop()` holds the lock for the whole body; take it in a closure (see `marlin::download`).
- `assert!` messages are format strings: escape `{` as `{{`.
- Klipper doc formats vary (`#### NAME`, ``#### `NAME` ``, `- \`CMD …\`: text`, shared bullets); coverage tests catch regressions.
- Test fixtures that look like docs: keep structure, own wording; check verbatim overlap before commit.

## Verify against real data

Unit tests use small fixtures; correctness claims come from real files. After grammar or server changes, parse a large set of real configs (0 errors) and run the two coverage tests. Real configs and slicer output live in the sibling `zed-klipper/test-data/` (gitignored, third-party GPL files); the Pi's Moonraker is read-only (GET only).

## Commits

Use the GitHub noreply address (`131036+digidude@users.noreply.github.com`, set in this repo's git config), never a personal email.

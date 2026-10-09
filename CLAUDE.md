# CLAUDE.md

Monorepo (since 2026-10-08, was three repos): `klipper-ls`, an editor-neutral language server for Klipper config and G-code, at the root; the two editor clients in `editors/vscode-klipper-ls` and `editors/zed-klipper-ls` (both are published as **Klipper Language Server**); and, as the submodule `syntax/`, the separate repo klipper-syntax (the tree-sitter grammar this server parses with, `highlights.scm`, and the syntax-only extensions **Klipper Syntax** for VS Code and **Klipper** for Zed). The two products are a pair: syntax makes it look right, the server makes it know things. Neither requires the other. README is user-facing; CONTRIBUTING.md has the layout and workflows; this file is the working context.

## Commands

```sh
cargo test && cargo clippy                       # keep clippy at zero warnings
cargo install --path .                           # then restart the language server in the editor
python3 scripts/probe.py <file> "needle" ...     # LSP round-trip; hovers 1 char into each needle
cd syntax/grammar && npx tree-sitter generate && npx tree-sitter test   # grammar work happens in the klipper-syntax repo; bump the submodule pin here after
cargo test coverage -- --ignored --nocapture     # real-data checks; env vars in CONTRIBUTING.md
```

Node: this machine's nvm lives in `~/.local/share/nvm`; `export NVM_DIR=$HOME/.local/share/nvm; source $NVM_DIR/nvm.sh; nvm use 22.11.0` before `npx`/`npm`.

Editors (details in CONTRIBUTING.md): VS Code `cd editors/vscode-klipper-ls && npm run test:unit` and `test:e2e` (needs `KLIPPER_LS_BIN=../../target/debug/klipper-ls`); Zed `cargo check --target wasm32-wasip2` in `editors/zed-klipper-ls`, then **zed: rebuild dev extension**.

## Decisions and why

- **License stays MIT.** Klipper's and Marlin's docs are GPL-3.0; they are read at runtime (local checkout or a one-time download into `~/Library/Caches/klipper-ls/`) and never vendored, embedded or compiled in. Don't add doc text to the repo. Relicensing is a one-way door. Marlin's docs as a runtime source was reconfirmed 2026-10-06; don't remove or make it opt-in unprompted.
- **README screenshots show short excerpts of Klipper's and Marlin's docs** (a sentence or two per hover, in `docs/screenshots/`). Dan accepted that use and risk on 2026-10-08; don't add more or longer excerpts, and don't quote doc text in repo text files. Regenerate with `npm run screenshots` in `editors/vscode-klipper-ls`.
- **Release asset names are a contract** (`klipper-ls-<target>.tar.gz`): both client packages download them.
- **Jinja is parsed in the grammar, not injected.** Klipper uses `{expr}` and G-code and Jinja share lines.
- **Newlines go through the external scanner** (`_newline` / `_continuation` / `_jinja_newline`); EOF emits a zero-width `_newline` only when valid; no rule may consist of only a newline.
- **The scanner also lexes** option keys, `NAME=` parameter names, `X10`/`S{..}` params, and pins; each call re-verifies the whole shape, so it stays stateless.
- **Hover layering:** user macros, then Klipper docs (what actually runs), then Marlin per-parameter detail with Klipper-ignored params folded into one line. "Klipper won't run this" is asserted only when `klippy/` source was scanned; docs-only wording hedges ("most likely").
- **`printer.*` hover** reads `Status_Reference.md`, organised by topic, not object. "Not documented" is a fact about the doc, not Klipper.
- **Semantic tokens reuse Zed's `highlights.scm`** (`include_str!` of `syntax/zed/languages/klipper/highlights.scm`), so one file decides what gets a color everywhere. Captures map to LSP token types in `highlight::map_capture` (punctuation is left to the editor). "Later pattern wins" is implemented by painting patterns onto a byte map in pattern order. `.gcode` is never parsed whole: a range request wraps the requested lines (max 3000) in a `[gcode_macro x]`/`gcode:` body, parses that and maps back; a *full* request for `.gcode` is answered empty. Changing a capture name in highlights.scm means updating the mapping.
- **Targets are byte ranges** (`features::Target`), so `.cfg` (syntax tree) and `.gcode` (line under cursor, `gcode.rs`) share hover/definition.
- **.gcode never gets a whole-file parse:** incremental sync + single-line lookup; tested on a 32 MB / 1.2M-line file.
- **The macro index is rebuilt per request** from `printer.cfg` following `[include]`s (≈1 ms), not cached.

## Editors

- **Lockstep versions, one tag.** `v*` builds the four binaries and the `.vsix` into one release; both clients rely on the release having `klipper-ls-<target>.tar.gz` assets. The `.vsix` is stamped from the tag; bump Cargo.toml, the Zed `Cargo.toml`/`extension.toml` and the VS Code `package.json` by hand first.
- **The contract between server and clients** is the asset names, the language ids (`klipper`, `gcode`; hover code fences say "Klipper"), the init option names and the `syntax/` pin. The same ids are also defined by klipper-syntax (VS Code merges them), so renaming one is a change in both repos.
- **The grammar and queries are not in this repo.** `syntax/` is a submodule of klipper-syntax. Zed pins the grammar by commit in *that* repo's `zed/extension.toml`; this repo's Zed extension is server-only and attaches to the "Klipper" language that extension defines (Zed has no extension dependencies, so users install both). In VS Code this extension depends on nothing and defines the `klipper`/`gcode` language ids itself (merged with a syntax extension's, if present).
- **Zed queries:** the later pattern wins; many themes (e.g. JetBrains Dark) don't define `variable.parameter` or `function.builtin`; prefer `attribute`, `keyword`, `function`, `type`, `constant`, `string.special`. The palette is deliberately small (trimmed 2026-10-07); `syntax/docs/highlighting.md` is the token-to-color guide, update it with any `highlights.scm` change (in the klipper-syntax repo).
- **Other Klipper extensions:** the client also attaches to dannymcgee.klipper's language ids (`klipper-cfg`, `klipper-gcode`) for hover/definition only; semantic tokens are suppressed there (middleware) so two sets of colors don't fight. `activationEvents` must list those ids, because our extension only auto-activates for the languages it defines itself (found by the screenshot dry run). The server treats any language id ending in `gcode` as G-code.
- **VS Code:** `src/binary.ts` stays free of `vscode` imports (unit tests run under plain node). Highlighting is the server's semantic tokens (`src/highlight.rs`), with a TextMate grammar from whichever syntax extension is installed (Klipper Syntax, or dannymcgee.klipper) only as a fallback/first paint; there is deliberately no extension dependency, so the server works with any of them; `configurationDefaults` turn semantic highlighting on for `klipper`/`gcode`. A settings change restarts the client because the server reads options once.

## Gotchas

- tree-sitter lexing: precedence beats match length (then length, string-over-regex, rule order); numbers use `token(prec(1, …))`.
- `lsp-server`: `drop(connection)` before `io_threads.join()`, or shutdown hangs.
- `while let Some(x) = mutex.lock().unwrap().pop()` holds the lock for the whole body; take it in a closure (see `marlin::download`).
- `assert!` messages are format strings: escape `{` as `{{`.
- Klipper doc formats vary (`#### NAME`, ``#### `NAME` ``, `- \`CMD …\`: text`, shared bullets); coverage tests catch regressions.
- Test fixtures that look like docs: keep structure, own wording; check verbatim overlap before commit.

## Verify against real data

Unit tests use small fixtures; correctness claims come from real files. After grammar or server changes (the grammar via the `syntax/` submodule), parse a large set of real configs (0 errors) and run the two coverage tests. Real configs and slicer output live in `test-data/`, and a shallow Klipper checkout in `klipper/` (both gitignored, third-party GPL, never commit; the test data's gcodes are 666 MB pulled from the printer); the Pi's Moonraker is read-only (GET only).

## Commits

Use the GitHub noreply address (`131036+digidude@users.noreply.github.com`, set in this repo's git config), never a personal email.

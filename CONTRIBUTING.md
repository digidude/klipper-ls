# Contributing

## Layout

```
editors/vscode-klipper-ls/  VS Code extension (TypeScript): starts the server, TextMate grammar
editors/zed-klipper-ls/     Zed extension: wasm shim, Zed queries, docs/highlighting.md
Cargo.toml, build.rs        build.rs compiles grammar/src into the binary
src/knowledge/klipper.rs    Klipper's G-Codes.md / Config_Reference.md, and handlers in klippy/
src/knowledge/status.rs     Klipper's status reference (printer.* fields)
src/knowledge/marlin.rs     Marlin's G-code reference (YAML headers), download
src/knowledge/mod.rs        layering: Klipper first, Marlin underneath, what Klipper ignores
src/index.rs                your macros, following [include]s from printer.cfg
src/features.rs             what's under the cursor -> hover / definition
src/gcode.rs                the same for .gcode files, one line at a time
scripts/probe.py            talk to the server from a terminal (no editor needed)
grammar/                    tree-sitter-klipper
  grammar.js                the grammar
  src/scanner.c             external scanner: line continuations, option keys, G-code params
  src/parser.c              generated, committed
  test/corpus/              parser tests
```

## Server

```sh
cargo test && cargo clippy                  # keep clippy at zero warnings
cargo install --path .                      # then restart the language server in your editor
python3 scripts/probe.py ~/printer_data/config/printer.cfg "M140" "PRINT_START"
```

`probe.py` hovers one character into each needle and prints the hover and definition. `PROBE_OPTIONS='{"klipperConfig": "..."}'` passes initialization options.

Unit tests use small fixtures; check changes against real files. Clone Klipper (GPL-3.0, gitignored, never commit it) and point the ignored tests at your own config:

```sh
git clone --depth 1 https://github.com/Klipper3d/klipper.git klipper

# Which commands, keys and printer.* references in a config have no docs?
KLIPPER_CONFIG=~/printer_data/config/printer.cfg KLIPPER_DOCS=klipper/docs \
  cargo test coverage -- --ignored --nocapture

# Every command in a folder of slicer output: hover source, or what Klipper skips
GCODE_DIR=~/printer_data/gcodes KLIPPER_DOCS=klipper/docs \
  cargo test gcode_coverage -- --ignored --nocapture

# Download (if needed) and check Marlin's reference
cargo test marlin_download -- --ignored --nocapture
```

Test fixtures that look like Klipper's docs keep the structure but use their own wording. Don't paste upstream text into the repo.

## Grammar

Needs Node.js.

```sh
cd grammar
npm install
npx tree-sitter generate          # after editing grammar.js; commit the regenerated src/parser.c
npx tree-sitter test
npx tree-sitter parse --quiet --stat path/to/*.cfg    # expect 0 errors
```

Editors that use the grammar pin it by commit: after a grammar change, the Zed extension's `extension.toml` `rev` needs bumping.

## How it's parsed (and why)

Klipper reads config with Python's `configparser`. A value continues on every following **indented** line, and blank lines and full-line comments don't end it, even at column 0. That rule depends on the *next* line's indentation, which a context-free lexer can't see. So line breaks go through the external scanner, which emits one of three tokens:

| Token | When |
|---|---|
| `_newline` | next content line starts at column 0 (new option or section) |
| `_continuation` | next content line is indented (same value continues) |
| `_jinja_newline` | line break inside an open `{% ... %}` / `{ ... }` (treated as whitespace) |

The scanner also lexes option keys, so a `gcode`/`*_gcode` key can switch its value to the G-code+Jinja sub-grammar, and G-code parameter names, because telling `MSG=hi` (a parameter) from `hi` (a plain word in an `M118` message) needs lookahead the regex lexer doesn't have.

Jinja is parsed inside this grammar rather than injected, because Klipper uses `{ }` rather than `{{ }}` and G-code and Jinja share lines (`M140 S{BED}`).

## Editors

Both clients are thin; the server does the work. They share a contract with it: the release asset names, the language ids (`klipper`, `gcode`; hover code fences use the name "Klipper"), the `initializationOptions` names, and the grammar `rev`. Change those in one commit.

### VS Code (`editors/vscode-klipper-ls`)

Needs Node (22).

```sh
cd editors/vscode-klipper-ls && npm install
npm run build && npm run test:unit
KLIPPER_LS_BIN=../../target/debug/klipper-ls KLIPPER_DOCS=../../klipper/docs npm run test:e2e   # real VS Code, ~300 MB first run
```

F5 ("Run Extension") debugs it. `src/binary.ts` must stay free of `vscode` imports so the unit tests run under plain node. The server reads options once at startup, so a settings change restarts the client.

### Zed (`editors/zed-klipper-ls`)

Install it with **zed: install dev extension** and pick `editors/zed-klipper-ls`. Query, `config.toml` and shim changes need only **zed: rebuild dev extension**. Validate queries against real files:

```sh
cd grammar && npx tree-sitter query ../editors/zed-klipper-ls/languages/klipper/highlights.scm path/to/printer.cfg
```

Zed builds the grammar from the `rev` in `editors/zed-klipper-ls/extension.toml`, so a grammar change reaches Zed only by commit: commit and push the grammar change, put the new SHA in `rev`, commit, then rebuild the dev extension. To iterate without pushing, temporarily point the grammar `repository` at `file:///…/klipper-ls` (never commit that). Update `docs/highlighting.md` with any `highlights.scm` change.

Machine-specific Zed settings go in a gitignored `.zed/settings.json` at the repo root, e.g. `{ "lsp": { "klipper-ls": { "initialization_options": { "klipperConfig": "…/printer.cfg" } } } }`.

## Test data

`test-data/` (gitignored: real configs include third-party GPL files) feeds the coverage tests:

```sh
git clone https://github.com/digidude/voron.git /tmp/voron
mkdir -p test-data/voron && cp -R /tmp/voron/printer_data test-data/voron/
```

Slicer output isn't in that repo. Copy a printer's `~/printer_data/gcodes` into `test-data/<printer>/printer_data/gcodes/` (Moonraker's read-only file API works: `GET /server/files/list?root=gcodes`, then `GET /server/files/gcodes/<path>`). Next to `printer_data/config/`, `.gcode` files find that config automatically.

## Releasing

One tag releases everything (lockstep versions):

```sh
git tag v0.3.0 && git push origin v0.3.0
```

The `release` workflow builds macOS and Linux archives named `klipper-ls-<target>.tar.gz` and a `klipper-ls-<version>.vsix`, and attaches them to one release. Both clients download the archive names, so don't rename them. Bump the versions in `Cargo.toml`, `editors/zed-klipper-ls/{Cargo.toml,extension.toml}` and `editors/vscode-klipper-ls/package.json` first (the workflow stamps the `.vsix` from the tag).

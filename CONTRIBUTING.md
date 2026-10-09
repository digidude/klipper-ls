# Contributing

Contributions are accepted under the project's MIT licence (inbound = outbound). Never paste Klipper or Marlin documentation into the repo; see [docs/maintaining.md](docs/maintaining.md) for the rules and the release process, and [MAINTAINERS.md](MAINTAINERS.md) for who looks after what.

## Layout

```
editors/vscode-klipper-ls/  VS Code extension (TypeScript): starts the server
editors/zed-klipper-ls/     Zed extension: the wasm shim that starts the server
syntax/                     submodule: github.com/digidude/klipper-syntax (grammar, queries, both syntax extensions)
Cargo.toml, build.rs        build.rs compiles syntax/grammar/src into the binary
src/knowledge/klipper.rs    Klipper's G-Codes.md / Config_Reference.md, and handlers in klippy/
src/knowledge/status.rs     Klipper's status reference (printer.* fields)
src/knowledge/marlin.rs     Marlin's G-code reference (YAML headers), download
src/knowledge/mod.rs        layering: Klipper first, Marlin underneath, what Klipper ignores
src/index.rs                your macros, following [include]s from printer.cfg
src/features.rs             what's under the cursor -> hover / definition
src/gcode.rs                the same for .gcode files, one line at a time
src/highlight.rs            semantic tokens: runs Zed's highlights.scm and maps captures to LSP token types
scripts/probe.py            talk to the server from a terminal (no editor needed)
syntax/grammar/             tree-sitter-klipper (submodule, see below)
  grammar.js                the grammar
  src/scanner.c             external scanner: line continuations, option keys, G-code params
  src/parser.c              generated, committed
  test/corpus/              parser tests
syntax/zed/languages/klipper/highlights.scm   what gets a color, here and in Zed
```

`syntax/` is the [klipper-syntax](https://github.com/digidude/klipper-syntax) repository as a submodule: `git clone --recurse-submodules`, or `git submodule update --init` in an existing checkout. It owns the grammar, the highlight queries and the two syntax-only extensions (Zed **Klipper**, VS Code **Klipper Syntax**); this repo owns the server and its editor clients, **Klipper Language Server** for each. The two are meant to be installed together.

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

The grammar lives in `syntax/grammar` (a submodule), so a grammar or query change is made in [klipper-syntax](https://github.com/digidude/klipper-syntax): see its CONTRIBUTING for generating, testing and parsing real configs there. Then bump the submodule here:

```sh
cd syntax && git pull origin main && cd ..
cargo test && cargo clippy           # the server compiles and queries what the submodule now holds
git add syntax                       # commit the new pin
```

`build.rs` compiles `syntax/grammar/src` into the binary and `src/highlight.rs` `include_str!`s `syntax/zed/languages/klipper/highlights.scm`, so the server and the editors always agree on the parse. A renamed capture in `highlights.scm` also means updating `highlight::map_capture`.

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

Both clients are thin; the server does the work. They share a contract with it: the release asset names, the language ids (`klipper`, `gcode`; hover code fences use the name "Klipper"), the `initializationOptions` names, and the grammar pin (`syntax/`). Both repositories define the language ids (VS Code merges the definitions), so renaming one is a change in both.

### VS Code (`editors/vscode-klipper-ls`)

Needs Node (22).

```sh
cd editors/vscode-klipper-ls && npm install
npm run build && npm run test:unit
KLIPPER_LS_BIN=../../target/debug/klipper-ls KLIPPER_DOCS=../../klipper/docs npm run test:e2e   # real VS Code, ~300 MB first run
```

`KLIPPER_OTHER_EXTENSION=<path to dannymcgee.klipper>` adds a check of klipper-ls on that extension's languages. `npm run screenshots` regenerates the README screenshots in a real VS Code with both extensions installed (macOS; needs Screen Recording permission; `SHOTS_DRY=1` checks every hover without capturing). F5 ("Run Extension") debugs it. `src/binary.ts` must stay free of `vscode` imports so the unit tests run under plain node. The server reads options once at startup, so a settings change restarts the client.

### Zed (`editors/zed-klipper-ls`)

Install it with **zed: install dev extension** and pick `editors/zed-klipper-ls`. Shim changes need only **zed: rebuild dev extension**. Validate queries against real files:

```sh
cd syntax/grammar && npx tree-sitter query ../zed/languages/klipper/highlights.scm path/to/printer.cfg
```

This extension only starts the server. The Klipper language itself (grammar, queries, colors) comes from the **Klipper** extension built from `syntax/zed`: install it too (dev extension, folder `syntax/zed`), and see klipper-syntax's CONTRIBUTING for its grammar pin.

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

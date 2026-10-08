# Klipper Language Server for VS Code

Hover docs and go-to-definition for [Klipper](https://www.klipper3d.org/) `printer.cfg`, your macros, and sliced `.gcode`, powered by [klipper-ls](https://github.com/digidude/klipper-ls). Syntax highlighting for config files, including the G-code and Jinja2 inside `[gcode_macro]` blocks.

> Early release: feedback wanted. [Open an issue](https://github.com/digidude/klipper-ls/issues) with the file that confused it.

## Features

- **Commands** (`M140`, `QUAD_GANTRY_LEVEL`): Klipper's documentation, and the config section the command needs. Standard G/M codes also show Marlin's per-parameter reference, with the parameters Klipper ignores folded into one line.
- **Parameters** (`S` in `M109 S215`): whether Klipper uses it. Slicer start code written for Marlin can silently do nothing on Klipper; this shows where.
- **Codes Klipper doesn't run:** a warning that Klipper replies `Unknown command` and skips the line.
- **Your macros:** description, the `params.*` it reads with defaults, its variables, and where it's defined.
- **Sections and options** (`[heater_bed]`, `rotation_distance`): the entry in Klipper's config reference.
- **Status fields in templates** (`printer.toolhead.position`): the entry in Klipper's status reference.
- **Go to definition** (F12 / Cmd-click): macro call → `[gcode_macro]` across `[include]`d files; `[include macros/*.cfg]` → the files.
- **Large slicer files are fine:** hovers read only the line under the cursor.

## Install

Until it's on the Marketplace, install the `.vsix` from the [latest release](https://github.com/digidude/klipper-ls/releases/latest):

```sh
code --install-extension klipper-ls-<version>.vsix
```

The extension finds the language server in this order:

1. the `klipper.server.path` setting,
2. `klipper-ls` on your `PATH` (or in `~/.cargo/bin`),
3. a download of the [latest klipper-ls release](https://github.com/digidude/klipper-ls/releases/latest), kept in the extension's storage (macOS and Linux, x86_64 and arm64).

On Windows, build the server (`cargo install --git https://github.com/digidude/klipper-ls`) and set `klipper.server.path`.

## Files it claims

`*.cfg` opens as **Klipper** and `*.gcode` / `*.gco` as **G-code**. If another Klipper or INI extension already owns `.cfg`, pick the language from the status bar or set:

```jsonc
"files.associations": { "printer.cfg": "klipper", "*.cfg": "klipper" }
```

This extension adds no highlighting for `.gcode`; hovers still work, and any G-code extension that uses the language id `gcode` gives you colors.

## Settings

| Setting | Meaning |
|---|---|
| `klipper.server.path` | Path to a `klipper-ls` binary. |
| `klipper.klipperDocs` | Klipper's `docs` folder. Default: found next to your config, then downloaded once. |
| `klipper.klipperConfig` | `printer.cfg` (or its folder), used to find your macros from `.gcode` files. |
| `klipper.marlinDocs` | A Marlin documentation checkout, or its `_gcode` folder. Default: downloaded once. |
| `klipper.downloadDocs` | `false` = never use the network for docs. |

Changing a setting restarts the server. Commands: **Klipper: Restart Language Server**, **Klipper: Show Server Output**.

## Where hover text comes from

Klipper's and Marlin's docs (both GPL-3.0) are read at runtime, from a local copy if one is found, else downloaded once into a cache. Nothing from them is bundled here. A local `klipper/docs` that matches the version you run gives the most accurate answers. Details are in the [klipper-ls README](https://github.com/digidude/klipper-ls#where-hover-text-comes-from).

## Known limitations

- Plugins outside Klipper (Beacon, led_effect, …) have no docs to show.
- No diagnostics, completion or rename yet.
- Highlighting is a TextMate grammar: it treats a macro body as a block until the next column-0 line, but can't tell every G-code detail the way the tree-sitter grammar in klipper-ls does.

## Development

```sh
cd editors/vscode-klipper-ls
npm install
npm run build
npm run test:unit
KLIPPER_LS_BIN=../../target/debug/klipper-ls \
  KLIPPER_DOCS=../../klipper/docs npm run test:e2e   # launches a real VS Code
```

Press F5 in VS Code ("Run Extension") to debug. `npm run package` builds the `.vsix`.

## License

MIT.

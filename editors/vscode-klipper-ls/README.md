# Klipper Language Server for VS Code

> **Pairs with [Klipper Syntax](https://github.com/digidude/klipper-syntax)**, which supplies a TextMate grammar and editing configuration. It is optional: this extension works on its own (colors then come from the server's semantic tokens) and with other Klipper syntax extensions such as dannymcgee.klipper. Klipper Syntax is how your files *look*; this is what makes them *know* things.

Hover docs, go-to-definition and highlighting for [Klipper](https://www.klipper3d.org/) `printer.cfg`, your macros and sliced `.gcode`, powered by [klipper-ls](https://github.com/digidude/klipper-ls). Hover any command, option, macro or `printer.*` field to see what it does in Klipper, and jump from a macro call to its definition, including the G-code and Jinja2 inside `[gcode_macro]` blocks.

> **Early release: feedback and corner cases wanted.** klipper-ls is new and has only been run on a handful of real setups. I'm looking for the files it handles badly: a hover that is wrong or missing, a macro it can't find, odd highlighting, a slow file. [Open an issue](https://github.com/digidude/klipper-ls/issues/new/choose); the forms ask for what's needed. Please remove secrets and personal details from anything you paste.

## Features

- **Commands** (`M140`, `QUAD_GANTRY_LEVEL`): Klipper's documentation, and the config section the command needs. Standard G/M codes also show Marlin's per-parameter reference, with the parameters Klipper ignores folded into one line.
- **Parameters** (`S` in `M109 S215`): whether Klipper uses it. Slicer start code written for Marlin can silently do nothing on Klipper; this shows where.
- **Codes Klipper doesn't run:** a warning that Klipper replies `Unknown command` and skips the line.
- **Your macros:** description, the `params.*` it reads with defaults, its variables, and where it's defined.
- **Sections and options** (`[heater_bed]`, `rotation_distance`): the entry in Klipper's config reference.
- **Status fields in templates** (`printer.toolhead.position`): the entry in Klipper's status reference.
- **Go to definition** (F12 / Cmd-click): macro call → `[gcode_macro]` across `[include]`d files; `[include macros/*.cfg]` → the files.
- **Highlighting from the real parse tree** (LSP semantic tokens): sections, options, pins, and the G-code and Jinja inside macros. The colors come from your theme. `.gcode` files get it too, for the lines on screen.
- **Large slicer files are fine:** hovers and highlighting read only the lines they need.

## Screenshots

Taken in VS Code with [dannymcgee.klipper](https://marketplace.visualstudio.com/items?itemName=dannymcgee.klipper) supplying the colors; everything in the boxes is `klipper-ls`.

**Your macros**: description, parameters with defaults, and where it's defined.

![Hover on a macro call showing its description, parameters and definition](https://github.com/digidude/klipper-ls/raw/main/docs/screenshots/hover-macro.png)

**Go to definition**, here peeked across an `[include]`d file.

![Peek definition of a macro in another file](https://github.com/digidude/klipper-ls/raw/main/docs/screenshots/peek-definition.png)

**Status fields in templates**: `printer.*` explained from Klipper's status reference.

![Hover on printer.toolhead.homed_axes](https://github.com/digidude/klipper-ls/raw/main/docs/screenshots/hover-status-field.png)

**Config options**: the entry from Klipper's config reference, with its section.

![Hover on rotation_distance in a stepper section](https://github.com/digidude/klipper-ls/raw/main/docs/screenshots/hover-config-option.png)

**G-code in sliced files**: which parameters Klipper ignores.

![Hover on M140 showing the Marlin parameter reference and the parameter Klipper ignores](https://github.com/digidude/klipper-ls/raw/main/docs/screenshots/hover-gcode-ignored-parameter.png)

**Codes Klipper doesn't run**: a warning that it replies `Unknown command` and skips the line.

![Hover on M500 explaining that Klipper skips it](https://github.com/digidude/klipper-ls/raw/main/docs/screenshots/hover-gcode-unknown-code.png)

## Install

Requires VS Code 1.90 or newer.

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

`.gcode` files are highlighted by the server too (semantic tokens, visible lines only).

## Using it with another Klipper extension

klipper-ls works alongside [dannymcgee.klipper](https://marketplace.visualstudio.com/items?itemName=dannymcgee.klipper), an alternative to Klipper Syntax with snippets and extras. If you prefer its colors, keep both installed and tell VS Code to open your files with its language:

```jsonc
"files.associations": { "*.cfg": "klipper-cfg", "*.gcode": "klipper-gcode" }
```

klipper-ls then attaches to those files and adds hover and go-to-definition. It leaves the colors to the other extension: semantic tokens are only sent for this extension's own `klipper` and `gcode` languages, so the two never fight. `klipper-config` (aeresov.klipper-config) isn't supported this way yet.

## Settings

| Setting | Meaning |
|---|---|
| `klipper.server.path` | Path to a `klipper-ls` binary. |
| `klipper.klipperDocs` | Klipper's `docs` folder. Default: found next to your config, then downloaded once. |
| `klipper.klipperConfig` | `printer.cfg` (or its folder), used to find your macros from `.gcode` files. |
| `klipper.marlinDocs` | A Marlin documentation checkout, or its `_gcode` folder. Default: downloaded once. |
| `klipper.downloadDocs` | `false` = never use the network for docs. |
| `klipper.trace.server` | `off`, `messages` or `verbose`: log the LSP traffic in the Klipper output channel. |

Changing a setting restarts the server. Commands: **Klipper: Restart Language Server**, **Klipper: Show Server Output**.

## Where hover text comes from

Klipper's and Marlin's docs (both GPL-3.0) are read at runtime, from a local copy if one is found, else downloaded once into a cache. Nothing from them is bundled here. A local `klipper/docs` that matches the version you run gives the most accurate answers. Details are in the [klipper-ls README](https://github.com/digidude/klipper-ls#where-hover-text-comes-from).

## Troubleshooting

- **No hover at all.** Check the language in the status bar says **Klipper** (or the other extension's language, see above). If an INI or another Klipper extension owns `.cfg`, set `files.associations`. Then run **Klipper: Show Server Output**.
- **Hovers are empty the first time.** The server downloads Klipper's docs once. It needs network access, or point `klipper.klipperDocs` at a local `klipper/docs` folder.
- **A macro isn't found from a `.gcode` file.** Set `klipper.klipperConfig` to your `printer.cfg` or its folder.
- **Still stuck?** [Open an issue](https://github.com/digidude/klipper-ls/issues/new/choose) and include the server output.

## Known limitations

- Plugins outside Klipper (Beacon, led_effect, …) have no docs to show.
- No diagnostics, completion or rename yet.
- Colors come from your theme's mapping of semantic token types (function, keyword, parameter, …). Themes that disable semantic highlighting fall back to a simpler built-in TextMate grammar, which is less precise. The extension turns semantic highlighting on for `klipper` and `gcode`; if you override `editor.semanticTokenColorCustomizations` you can recolor them.
- Punctuation (`=`, `:`, brackets) keeps the TextMate color; the server doesn't tokenize it.

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

# klipper-ls

A language server for [Klipper](https://www.klipper3d.org/) printer config. It speaks LSP over stdio, so any editor can use it. It answers two questions while you edit `printer.cfg`, your macros, or sliced `.gcode`: *what does this do in Klipper?* and *where is it defined?*

> Early release: feedback wanted. Open an issue with the file that confused it.

| Editor | Package |
|---|---|
| VS Code | [`editors/vscode-klipper-ls`](editors/vscode-klipper-ls) |
| Zed | [`editors/zed-klipper-ls`](editors/zed-klipper-ls) |
| Anything else with LSP support (Neovim, Helix, Emacs, …) | run `klipper-ls`; see [Other editors](#other-editors) |

## Features

These work in Klipper config and in sliced `.gcode` files:

- **Commands** (`M140`, `QUAD_GANTRY_LEVEL`): Klipper's documentation, and the config section the command needs. Standard G/M codes also show Marlin's per-parameter reference, with the parameters Klipper ignores folded into one line.
- **Parameters** (`S` in `M109 S215`): whether Klipper uses it. Slicer start code written for Marlin can silently do nothing on Klipper; this shows where.
- **Codes Klipper doesn't run:** a warning that Klipper replies `Unknown command` and skips the line.
- **Your macros** (`PRINT_START`): the description, the `params.*` it reads with their defaults, its variables, and where it's defined.
- **Sections and options** (`[heater_bed]`, `rotation_distance`): the matching entry in Klipper's config reference.
- **Status fields in templates** (`printer.toolhead.position`, `printer['heater_generic chamber'].target`): the matching entry in Klipper's status reference. `printer["gcode_macro X"].var` shows the variable's initial value, and flags a variable the macro doesn't define.
- **Go to definition:** from a macro call to its `[gcode_macro]`, across `[include]`d files; from `[include macros/*.cfg]` to the files; from built-ins into Klipper's docs.

- **Highlighting** (semantic tokens): config files and macros, including the G-code and Jinja inside `[gcode_macro]`, colored from the real parse tree, the same as in Zed. In `.gcode` files only the lines on screen are looked at.

Large slicer files are fine: hovers and highlighting read only the lines they need, so a 32 MB file answers instantly.

## Install

Download the archive for your platform from the [latest release](https://github.com/digidude/klipper-ls/releases/latest) (macOS and Linux, x86_64 and aarch64), unpack it, and put `klipper-ls` on your `PATH`. Or build it with [Rust](https://rustup.rs):

```sh
cargo install --git https://github.com/digidude/klipper-ls
```

The VS Code and Zed packages download the release binary for you.

## Configuration

Everything works without configuration. The server reads these `initializationOptions`:

```jsonc
{
  "klipperDocs": "~/klipper/docs",           // Klipper's docs folder
  "klipperConfig": "~/printer_data/config",  // printer.cfg (or its folder), used for .gcode files
  "marlinDocs": "~/src/MarlinDocumentation", // a checkout, or its _gcode folder
  "downloadDocs": true                       // false = never use the network
}
```

### Where hover text comes from

- **Klipper's docs** (`G-Codes.md`, `Config_Reference.md`, `Status_Reference.md`). Found in this order:
  1. `klipperDocs`.
  2. A `klipper/docs` folder next to your config or in a parent folder.
  3. `~/klipper/docs`.
  4. A one-time download into `~/Library/Caches/klipper-ls/docs` (or `$XDG_CACHE_HOME/klipper-ls`).

  A local copy matches the Klipper version you run. If it includes Klipper's source (`klippy/`), the "Klipper doesn't run this" warning is definite; with docs alone it says "most likely".
- **Marlin's G-code reference**, for per-parameter detail on standard codes. Taken from `marlinDocs`, or downloaded once (about 250 small files, in the background).

Delete a cache folder to refresh it. With `downloadDocs: false` and no local docs, hovers are empty.

### Which macros it sees

It starts at the nearest `printer.cfg` and follows its `[include]`s, the same way Klipper does, so stale backup files don't show up as duplicates. Unsaved edits count.

For a `.gcode` file, it looks for `printer.cfg`, `config/printer.cfg` or `printer_data/config/printer.cfg` in each parent folder, then `~/printer_data/config/printer.cfg`. Set `klipperConfig` to point anywhere else.

## Other editors

The server handles `textDocument/hover` and `textDocument/definition` with incremental sync. Language ids it expects are `klipper` for config and `gcode` for G-code. Neovim, for example:

```lua
vim.lsp.config("klipper-ls", {
  cmd = { "klipper-ls" },
  filetypes = { "klipper", "gcode" },
  root_markers = { "printer.cfg" },
})
vim.lsp.enable("klipper-ls")
```

The server sends semantic tokens, so a client that enables them (Neovim does by default) gets highlighting without tree-sitter. The tree-sitter grammar in [`grammar/`](grammar) can also be used on its own.

## Known limitations

- Plugins outside Klipper (Beacon, led_effect, …) aren't in Klipper's docs, so their options and commands get no hover.
- Status fields that Klipper's reference doesn't list (such as `toolhead.estimated_print_time`) are reported as undocumented.
- Settings are read once at startup; restart the server after changing them.
- No diagnostics, completion or rename yet.
- Semantic token colors depend on your editor theme. Punctuation (`=`, `:`, brackets) isn't tokenized; the editor's own grammar, if any, colors it.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Credits and license

MIT. Hover text comes from [Klipper's docs](https://github.com/Klipper3d/klipper/tree/master/docs) and [Marlin's G-code reference](https://marlinfw.org/meta/gcode/). Both are GPL-3.0; they are read at runtime and never bundled with this project.

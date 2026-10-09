# Klipper for Zed

[Klipper](https://www.klipper3d.org/) support for [Zed](https://zed.dev): syntax highlighting for `printer.cfg` and its includes, including the G-code and Jinja2 inside `[gcode_macro]` blocks, plus hover docs and go-to-definition from [`klipper-ls`](https://github.com/digidude/klipper-ls), a separate language server this extension starts for you. (VS Code users: [`editors/vscode-klipper-ls`](https://github.com/digidude/klipper-ls/tree/main/editors/vscode-klipper-ls).)

This extension supplies the **colors and editing** (queries, outline, brackets, indents). `klipper-ls` supplies the **hover and go-to-definition**.

> **Early release: feedback and corner cases wanted.** klipper-ls is new and has only been run on a handful of real setups. I'm looking for the files it handles badly: a hover that is wrong or missing, a macro it can't find, odd highlighting, a slow file. [Open an issue](https://github.com/digidude/klipper-ls/issues/new/choose); the forms ask for what's needed. Please remove secrets and personal details from anything you paste.

## Features

### Highlighting and editing

- **Config files:** sections, options, numbers, booleans, paths, multi-line values and the `#*#` SAVE_CONFIG block.
- **Pins:** one color for the whole pin, with the `^`/`!`/`~` modifiers picked out (`^!EBBCan:PB6`). Covers STM32/AVR, LPC176x (`P1.24`), RP2040/host (`gpio0_27`), board aliases (`EXP1_5`) and `chip:virtual_endstop`.
- **G-code in macros:** in `gcode:` and every `*_gcode:` option, commands, `KEY=value` and `X10` / `S{BED}` parameters are highlighted.
- **Klipper's Jinja2 dialect:** `{expr}` (single braces), `{% … %}` and `{# … #}`, parsed as real expressions. `params`, `printer` and `action_*()` get their own colors. Only Jinja's real filters get the function color, so a typo like `|integer` stays plain.
- **Color guide:** [docs/highlighting.md](docs/highlighting.md) maps every token to its capture and typical color.
- **Outline panel:** one entry per section, with `variable_*` options nested under each macro.
- **Editing:** `#` comments, bracket matching, auto-indent after `gcode:` and `{% if %}` / `{% for %}`, and Vim text objects (`af`/`if` for a `gcode:` option, `ac`/`ic` for a section).

### Hover and go-to-definition

From `klipper-ls`, in Klipper config and in sliced `.gcode` files: Klipper's docs for commands, sections, options and `printer.*` status fields; which parameters Klipper ignores (and codes it won't run); your own macros with their `params.*` and variables; and go-to-definition across `[include]`s. The [klipper-ls README](https://github.com/digidude/klipper-ls#features) has the full list and says where the text comes from.

## Installation

This extension isn't in Zed's extension registry yet, so install it as a dev extension. You need [Rust via rustup](https://rustup.rs).

1. Clone [klipper-ls](https://github.com/digidude/klipper-ls); the extension is in `editors/zed-klipper-ls`.
2. *(Optional)* Build the language server yourself:
   ```sh
   cargo install --git https://github.com/digidude/klipper-ls
   ```
   This puts `klipper-ls` in Cargo's bin folder (`~/.cargo/bin`, or `$CARGO_HOME/bin`), which needs to be on your `PATH`. Without it, the extension downloads a prebuilt server for macOS or Linux from the latest [klipper-ls release](https://github.com/digidude/klipper-ls/releases/latest) on first start. A `klipper-ls` on your `PATH`, or `lsp.klipper-ls.binary.path`, takes priority over the download.
3. In Zed, open the command palette, run **zed: install dev extension**, and pick the `editors/zed-klipper-ls` folder.
4. *(Optional)* For hovers in `.gcode` files, install the **G-code** extension from Zed's extension list. `klipper-ls` attaches to that language.

`*.cfg` files open as Klipper. Zed's INI extension also claims `.cfg`, and when two extensions claim the same suffix Zed can pick either. If your `.cfg` files still open as INI, tell Zed which one you want in `settings.json`:

```jsonc
"file_types": { "Klipper": ["cfg"] }
```

If an older setting maps `cfg` to INI, remove it.

To update, pull, run the `cargo install` again (if you built the server), then run **zed: rebuild dev extension** and **editor: restart language server**.

## Configuration

Everything works without configuration. These optional settings go in Zed's `settings.json`:

```jsonc
"lsp": {
  "klipper-ls": {
    "binary": { "path": "/path/to/klipper-ls" },   // default: klipper-ls on PATH
    "initialization_options": {
      "klipperDocs": "~/klipper/docs",             // Klipper's docs folder
      "klipperConfig": "~/printer_data/config",    // printer.cfg (or its folder), used for .gcode files
      "marlinDocs": "~/src/MarlinDocumentation",   // a checkout, or its _gcode folder
      "downloadDocs": true                         // false = never use the network
    }
  }
}
```

Settings changes take effect after **editor: restart language server**.

Where hover text comes from, how macros are found, and what each option means: see the [klipper-ls README](https://github.com/digidude/klipper-ls#configuration).

## Known limitations

- Plugins outside Klipper (Beacon, led_effect, …) aren't in Klipper's docs, so their options and commands get no hover.
- The G-code extension also claims CNC formats (`.nc`, `.tap`), where Klipper/Marlin hovers don't apply.
- Status fields that Klipper's reference doesn't list (such as `toolhead.estimated_print_time`) are reported as undocumented.
- Jinja `{% raw %}` isn't special-cased, and `{% for x in y if cond %}` parses as a conditional expression; highlighting is unaffected.
- KlipperScreen and Moonraker `.conf` files aren't claimed. They use the same format, so you can add them with `file_types`.

## Contributing

See [CONTRIBUTING.md](https://github.com/digidude/klipper-ls/blob/main/CONTRIBUTING.md). The server and the tree-sitter grammar live in [klipper-ls](https://github.com/digidude/klipper-ls).

## Credits and license

MIT. Several ideas (pin structure, filter list, paths) come from [dannymcgee/vscode-klipper](https://github.com/dannymcgee/vscode-klipper) (MIT). Hover text comes from [Klipper's docs](https://github.com/Klipper3d/klipper/tree/master/docs) and [Marlin's G-code reference](https://marlinfw.org/meta/gcode/). Both are GPL-3.0; they are read at runtime by klipper-ls and never bundled with this extension.

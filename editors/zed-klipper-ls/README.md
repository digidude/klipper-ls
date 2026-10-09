# Klipper Language Server for Zed

Hover docs, go-to-definition and macro awareness for [Klipper](https://www.klipper3d.org/) `printer.cfg` and sliced `.gcode`, from [`klipper-ls`](https://github.com/digidude/klipper-ls), a language server this extension starts for you.

> **Pairs with [Klipper Syntax](https://github.com/digidude/klipper-syntax)** (listed in Zed as **Klipper**). That extension defines the Klipper language and colors it; this one attaches the server to it. **Install both.** Zed can't declare a dependency between extensions, so without the Klipper extension this one has nothing to attach to.

> **Early release: feedback and corner cases wanted.** klipper-ls is new and has only been run on a handful of real setups. I'm looking for the files it handles badly: a hover that is wrong or missing, a macro it can't find, odd highlighting, a slow file. [Open an issue](https://github.com/digidude/klipper-ls/issues/new/choose); the forms ask for what's needed. Please remove secrets and personal details from anything you paste.

## Features

From `klipper-ls`, in Klipper config and in sliced `.gcode` files: Klipper's docs for commands, sections, options and `printer.*` status fields; which parameters Klipper ignores (and codes it won't run); your own macros with their `params.*` and variables; and go-to-definition across `[include]`s. The [klipper-ls README](https://github.com/digidude/klipper-ls#features) has the full list and says where the text comes from.

Colors, the outline panel, brackets, indentation and text objects come from the Klipper extension, not this one.

## Installation

This extension isn't in Zed's extension registry yet, so install it as a dev extension. You need [Rust via rustup](https://rustup.rs).

1. Install **Klipper** from Zed's extension list (or as a dev extension from [klipper-syntax](https://github.com/digidude/klipper-syntax), folder `zed`).
2. Clone [klipper-ls](https://github.com/digidude/klipper-ls); this extension is in `editors/zed-klipper-ls`.
3. *(Optional)* Build the language server yourself:
   ```sh
   cargo install --git https://github.com/digidude/klipper-ls
   ```
   This puts `klipper-ls` in Cargo's bin folder (`~/.cargo/bin`, or `$CARGO_HOME/bin`), which needs to be on your `PATH`. Without it, the extension downloads a prebuilt server for macOS or Linux from the latest [klipper-ls release](https://github.com/digidude/klipper-ls/releases/latest) on first start. A `klipper-ls` on your `PATH`, or `lsp.klipper-ls.binary.path`, takes priority over the download.
4. In Zed, open the command palette, run **zed: install dev extension**, and pick the `editors/zed-klipper-ls` folder.
5. *(Optional)* For hovers in `.gcode` files, install the **G-code** extension from Zed's extension list. `klipper-ls` attaches to that language.

`*.cfg` files open as Klipper. Zed's INI extension also claims `.cfg`; see the Klipper extension's README if your files open as INI.

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
      "downloadDocs": true,                        // false = never use the network
      "diagnostics": true                          // false = no squiggles, only hovers
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

## Contributing

See [CONTRIBUTING.md](https://github.com/digidude/klipper-ls/blob/main/CONTRIBUTING.md). The grammar and highlighting live in [klipper-syntax](https://github.com/digidude/klipper-syntax).

## Credits and license

MIT. Hover text comes from [Klipper's docs](https://github.com/Klipper3d/klipper/tree/master/docs) and [Marlin's G-code reference](https://marlinfw.org/meta/gcode/). Both are GPL-3.0; they are read at runtime by klipper-ls and never bundled with this extension.

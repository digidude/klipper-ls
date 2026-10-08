x# Highlighting: token-to-color guide

What each piece of a Klipper config gets colored, and why. Colors come from your Zed theme, not from this extension: the extension only tags each token with a **capture name** (`@keyword`, `@string`, …) and the theme picks the color. The "One Dark" column is the typical result in Zed's default theme; other themes differ.

## The idea

The palette follows the same conventions as Zed's Rust, Python and TOML support, so Klipper files look familiar next to your other code:

- **Strong color goes to a few categories:** keywords, functions/macros, types (section headers), strings and constants.
- **Structure stays quiet:** plain variables, `=` / `:` and punctuation sit near the foreground color.
- **Config lines read like TOML/INI; macro bodies read like Python.** `[section]`, `key:` and the value come first. G-code and Jinja get more color only inside `gcode:` blocks.
- **One idea, one color.** A pin like `^!EBBCan:PB6` is a single constant, not three colors.

## Config sections and options

| Token | Example | Capture | One Dark (approx.) | Like… |
|---|---|---|---|---|
| Comment | `# 400 for 0.9° motors` | `@comment` | gray | `# …` in Python |
| SAVE_CONFIG block | `#*# [bed_mesh default]` | `@comment.doc` → falls back to `@comment` | gray | `///` doc comments in Rust |
| Section type | `[stepper_x]`, `[tmc2209 …]`, `[include …]` | `@type` | teal | `[table]` in TOML, a struct name in Rust |
| Section name | `[tmc2209 stepper_x]` | *(none)* | foreground | |
| Macro name | `[gcode_macro PRINT_START]` | `@function` | blue | `def print_start` in Python |
| Brackets | `[` `]` | `@punctuation.bracket` | foreground | |
| Option key | `rotation_distance:` | `@property` | red/pink | a TOML key, a Rust struct field |
| Macro variable | `variable_z_hot_offset:` | `@variable.special` | orange | `self` in Python: special to templates |
| `:` / `=` after a key | `step_pin: PF13` | `@punctuation.delimiter` | foreground | `=` in TOML |
| Text value | `description: Heat, level…` | `@string` | green | a string literal |
| Number | `40`, `0.04`, `-2` | `@number` | orange | |
| Boolean / None | `true`, `False`, `None` | `@constant` | yellow/orange | `True` / `None` in Python |
| Pin | `PF13`, `EBBCan:PB6`, `probe:z_virtual_endstop` | `@constant` | yellow/orange | an enum variant or ALL_CAPS constant |
| Pin modifier | the `^` `!` `~` in `^!PF14` | `@operator` | teal | `!` / `&` in Rust |
| File path | `serial: /dev/serial/by-id/…` | `@string.special` | orange/green | |
| `rename_existing:` value | `rename_existing: G28` | `@function` | blue | it names a command |

## G-code (inside `gcode:` and `*_gcode:`)

| Token | Example | Capture | One Dark (approx.) | Like… |
|---|---|---|---|---|
| Standard code | `G28`, `M104`, `T0` | `@keyword` | purple | built-in statements like `return` |
| Macro / extended command | `PRINT_START`, `SET_GCODE_VARIABLE` | `@function` | blue | a function call |
| Parameter name | `TARGET` in `TARGET=100`; the `X` in `X10` and the `S` in `S{BED}` | `@attribute` | blue/teal | a keyword argument |
| `=` in a parameter | `TARGET=100` | `@punctuation.delimiter` | foreground | |
| Parameter value | `100`, `"text"` | `@number` / `@string` | orange / green | |

## Jinja2 templates (`{ … }`, `{% … %}`, `{# … #}`)

| Token | Example | Capture | One Dark (approx.) | Like… |
|---|---|---|---|---|
| Delimiters | `{` `}` `{%` `%}` | `@punctuation.special` | red/orange | `{}` in a Python f-string |
| Statement keyword | `set`, `if`, `for`, `in`, `and`, `not` | `@keyword` | purple | Python keywords |
| Variable | `BED`, `has_chamber` | `@variable` | foreground | a Python local |
| Template context | `params`, `rawparams`, `printer` | `@variable.special` | orange | `self` in Python |
| Attribute / key | `params.BED`, `.toolhead` | `@property` | red/pink | a field access |
| Function call | `range(3)`, `macro()` | `@function` | blue | |
| Klipper/Jinja built-in | `action_respond_info()`, `range()` | `@function.builtin` → usually `@function` | blue | `print()` in Python |
| Known filter or test | `\|int`, `\|default(0)`, `is defined` | `@function` | blue | |
| Unknown filter or test | `\|integer` (a typo) | `@variable` | foreground | stays plain so mistakes stand out |
| Keyword argument | `round(precision=2)` | `@variable.parameter` | foreground or orange | `f(x=1)` in Python |
| Math / comparison | `+ - * / == < ~` and `\|` | `@operator` | teal | |
| `=` in `set` / kwargs | `{% set X = 1 %}` | `@punctuation.delimiter` | foreground | |
| Template comment | `{# … #}` | `@comment` | gray | |

## Theme notes

- Captures with a dot fall back to their parent when a theme doesn't define them: `@comment.doc` → `@comment`, `@function.builtin` → `@function`, `@variable.special` → `@variable`.
- Many themes (e.g. JetBrains Dark) leave `@variable.parameter` and `@function.builtin` undefined, so those render plain or in the parent color. The queries rely on them only where that fallback is acceptable.
- To retune a color yourself, override the capture in Zed's `settings.json`:

  ```json
  "experimental.theme_overrides": {
    "syntax": {
      "constant": { "color": "#d19a66" },
      "attribute": { "color": "#61afef" }
    }
  }
  ```

Captures are defined in [`languages/klipper/highlights.scm`](../languages/klipper/highlights.scm). When the queries change, update this table in the same commit.

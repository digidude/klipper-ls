; ---------------------------------------------------------------------------
; Comments
; ---------------------------------------------------------------------------

(comment) @comment

; SAVE_CONFIG block written by Klipper
((comment) @comment.doc
  (#match? @comment.doc "^#\\*#"))

(jinja_comment) @comment

; ---------------------------------------------------------------------------
; Sections
; ---------------------------------------------------------------------------

(section_header
  [
    "["
    "]"
  ] @punctuation.bracket)

; [stepper_x], [tmc2209 stepper_x]: the type carries the color, the name
; stays plain. Macro names are the exception: they match their call sites.
(section_type) @type

(section_header
  type: (section_type) @_type
  name: (section_name) @function
  (#match? @_type "^(gcode_macro|delayed_gcode)$"))

; ---------------------------------------------------------------------------
; Options and plain values
; ---------------------------------------------------------------------------

(key) @property

((key) @variable.special
  (#match? @variable.special "^variable_"))

(option
  [
    ":"
    "="
  ] @punctuation.delimiter)

(gcode_option
  [
    ":"
    "="
  ] @punctuation.delimiter)

(value
  (text) @string)

(value
  "," @punctuation.delimiter)

(value
  [
    "["
    "]"
    "{"
    "}"
    "("
    ")"
  ] @punctuation.bracket)

; ^!EBBCan:PB6 -> one constant, with the ^/!/~ modifiers as operators
(pin) @constant

(pin_modifier) @operator

(path) @string.special

; rename_existing: G28 names a command
(option
  key: (key) @_key
  value: (value
    (text) @function)
  (#eq? @_key "rename_existing"))

(number) @number

(boolean) @constant

(none) @constant

(string) @string

; ---------------------------------------------------------------------------
; G-code
; ---------------------------------------------------------------------------

(command) @function

; G0/G28/M104/T0: standard codes vs. macros. (function.builtin would fall back
; to function in many themes, so use keyword for a visible difference.)
((command) @keyword
  (#match? @keyword "^[GgMmTt][0-9]+$"))

; KEY=value reads like an attribute on the command. (variable.parameter falls
; back to plain text in many themes.)
(parameter_name) @attribute

; `=` is assignment punctuation, as in TOML/INI; real math stays operator
(parameter
  "=" @punctuation.delimiter)

(gcode_string
  "\"" @string)

(string_content) @string

; ---------------------------------------------------------------------------
; Jinja
; ---------------------------------------------------------------------------

(jinja_expression
  [
    "{"
    "}"
  ] @punctuation.special)

(jinja_statement
  [
    "{%"
    "{%-"
    "%}"
    "-%}"
  ] @punctuation.special)

[
  "set"
  "if"
  "elif"
  "for"
  "in"
  "recursive"
  "macro"
  "call"
  "filter"
  "do"
  "with"
  "is"
  "not"
  "and"
  "or"
] @keyword

(keyword_statement) @keyword

(conditional_expression
  [
    "if"
    "else"
  ] @keyword)

(identifier) @variable

; Klipper's template context
((identifier) @variable.special
  (#match? @variable.special "^(params|rawparams|printer)$"))

(property_identifier) @property

(call
  function: (identifier) @function)

(call
  function: (attribute
    attribute: (property_identifier) @function))

; Klipper's template functions and Jinja's globals
(call
  function: (identifier) @function.builtin
  (#match? @function.builtin "^(action_emergency_stop|action_respond_info|action_raise_error|action_call_remote_method|range|dict|lipsum|cycler|joiner|namespace)$"))

; Filters and tests: Klipper adds none of its own, so only Jinja's built-ins
; (2.11, as shipped with Klipper) exist. Known ones get the function color;
; anything else stays plain, so a typo like `|integer` stands out in any theme.
(filter_name) @variable

((filter_name) @function
  (#match? @function "^(abs|attr|batch|capitalize|center|count|d|default|dictsort|e|escape|filesizeformat|first|float|forceescape|format|groupby|indent|int|join|last|length|list|lower|map|max|min|pprint|random|reject|rejectattr|replace|reverse|round|safe|select|selectattr|slice|sort|string|striptags|sum|title|tojson|trim|truncate|unique|upper|urlencode|urlize|wordcount|wordwrap|xmlattr)$"))

(test_name) @variable

((test_name) @function
  (#match? @function "^(boolean|callable|defined|divisibleby|eq|equalto|escaped|even|false|float|ge|greaterthan|gt|in|integer|iterable|le|lessthan|lower|lt|mapping|ne|none|number|odd|sameas|sequence|string|true|undefined|upper)$"))

(macro_statement
  name: (identifier) @function)

(keyword_argument
  name: (identifier) @variable.parameter)

(pair
  key: (string) @property)

[
  "=="
  "!="
  "<"
  ">"
  "<="
  ">="
  "+"
  "-"
  "*"
  "/"
  "//"
  "%"
  "**"
  "~"
  "|"
] @operator

(set_statement
  "=" @punctuation.delimiter)

(keyword_argument
  "=" @punctuation.delimiter)

[
  "."
  ","
  ":"
] @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
] @punctuation.bracket

(dict
  [
    "{"
    "}"
  ] @punctuation.bracket)

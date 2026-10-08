; Anchors for decrease_indent_patterns in config.toml: `{% endif %}` lines up
; with the nearest `{% if %}` / `{% elif %}` above it, and so on.
(jinja_statement (if_statement)) @start.if
(jinja_statement (elif_statement)) @start.elif
(jinja_statement (for_statement)) @start.for
(jinja_statement (macro_statement)) @start.macro

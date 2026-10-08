; [gcode_macro PRINT_START] -> "gcode_macro PRINT_START"
(section
  header: (section_header
    type: (section_type) @context
    name: (section_name) @name)) @item

; [printer] -> "printer"
(section
  header: (section_header
    type: (section_type) @name
    !name)) @item

; Macro variables nest under their macro.
(option
  key: (key) @name
  (#match? @name "^variable_")) @item

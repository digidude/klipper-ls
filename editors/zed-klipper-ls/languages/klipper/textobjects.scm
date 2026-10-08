; Vim mode: `ac`/`ic` select a whole section / its options,
; `af`/`if` a whole gcode: option / just its template body.
(section
  header: (_)
  [
    (option)
    (gcode_option)
    (comment)
  ]* @class.inside) @class.around

(gcode_option
  value: (gcode_block) @function.inside) @function.around

(comment)+ @comment.around

(jinja_comment) @comment.around

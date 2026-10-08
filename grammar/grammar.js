/**
 * @file Klipper printer configuration (printer.cfg) with Jinja2 G-code macros
 * @license MIT
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

// Jinja operator precedence, loosely following jinja2/parser.py.
// Filters and tests bind tighter than any binary operator: `a + b|int` is `a + (b|int)`.
const PREC = {
  conditional: 1,
  or: 2,
  and: 3,
  not: 4,
  compare: 5,
  add: 6,
  concat: 7,
  mul: 8,
  unary: 9,
  pow: 10,
  filter: 11,
  postfix: 12,
};

const commaSep1 = (rule) => seq(rule, repeat(seq(',', rule)));
const commaSep = (rule) => optional(commaSep1(rule));

module.exports = grammar({
  name: 'klipper',

  // Newlines are never extras: the scanner turns every line break into one of
  // _newline (next line starts at column 0), _continuation (next line is
  // indented, so it belongs to the current value) or _jinja_newline (a line
  // break inside an open {% ... %} or { ... } that spans lines).
  externals: ($) => [
    $._newline,
    $._continuation,
    $._jinja_newline,
    $.key,
    $._template_key,
    $.parameter_name,
    $._parameter_letter,
    $.pin_modifier,
    $.pin_chip,
    $.pin_name,
    $._error_sentinel,
  ],

  extras: ($) => [/[ \t\r\f]/, $.comment, $._jinja_newline],

  conflicts: ($) => [],

  rules: {
    source_file: ($) =>
      seq(
        optional($._newline),
        repeat($._comment_line),
        repeat($.section),
      ),

    // Klipper (via configparser) only treats `#`/`;` as a comment at the start
    // of a line or after whitespace. `#*#` lines are the SAVE_CONFIG block.
    comment: (_) => token(seq(/[#;]/, /.*/)),

    _comment_line: ($) => seq($.comment, $._newline),

    // ------------------------------------------------------------------
    // Sections and options
    // ------------------------------------------------------------------

    section: ($) =>
      seq(
        field('header', $.section_header),
        $._newline,
        repeat(choice($.option, $.gcode_option, $._comment_line)),
      ),

    section_header: ($) =>
      seq(
        '[',
        field('type', $.section_type),
        optional(field('name', $.section_name)),
        ']',
      ),

    section_type: (_) => /[A-Za-z0-9_]+/,
    section_name: (_) => /[^\]\s][^\]\n]*/,

    option: ($) =>
      seq(
        field('key', $.key),
        $._separator,
        optional(field('value', $.value)),
        $._newline,
      ),

    // `gcode:` and `*_gcode:` options hold Jinja2 templates that render to G-code.
    gcode_option: ($) =>
      seq(
        field('key', alias($._template_key, $.key)),
        $._separator,
        optional(field('value', $.gcode_block)),
        $._newline,
      ),

    _separator: (_) => choice(':', '='),

    value: ($) =>
      choice(
        seq(
          $._value_line,
          repeat(seq($._continuation, optional($._value_line))),
        ),
        repeat1(seq($._continuation, optional($._value_line))),
      ),

    _value_line: ($) => repeat1($._value_item),

    _value_item: ($) =>
      choice(
        alias($._signed_number, $.number),
        $.boolean,
        $.none,
        $.string,
        $.pin,
        $.path,
        $.text,
        ',',
        '[',
        ']',
        '{',
        '}',
        '(',
        ')',
      ),

    // MCU pin, e.g. `^!EBBCan:PB6`: pullup/invert/pulldown modifiers, the MCU
    // it lives on, then the pin. The scanner only emits these tokens when the
    // whole thing looks like a pin, so ordinary words in values stay `text`.
    pin: ($) =>
      seq(
        optional(field('modifier', $.pin_modifier)),
        optional(seq(field('chip', $.pin_chip), ':')),
        field('name', $.pin_name),
      ),

    // /dev/serial/by-id/..., ~/printer_data/config/variables.cfg
    path: (_) => token(prec(1, /(~|\.\.?)?\/[^\s,"'\[\]{}()]*/)),

    text: (_) => /[^\s,"'\[\]{}()#;][^\s,\[\]{}()]*/,

    // ------------------------------------------------------------------
    // G-code templates
    // ------------------------------------------------------------------

    gcode_block: ($) =>
      choice(
        seq(
          $.gcode_line,
          repeat(seq($._continuation, optional($.gcode_line))),
        ),
        repeat1(seq($._continuation, optional($.gcode_line))),
      ),

    // A line is any leading Jinja (`{% if %}`, `{x}`), then a command and its
    // arguments. A line can also be pure Jinja, or start with KEY=value when
    // the command name itself comes from a template (`{cmd} KEY=1`).
    gcode_line: ($) =>
      choice(
        seq(repeat1($._jinja), optional($._gcode_tail)),
        $._gcode_tail,
      ),

    _gcode_tail: ($) =>
      choice(
        seq(field('command', $.command), repeat($._argument)),
        seq($.parameter, repeat($._argument)),
      ),

    command: (_) => /[A-Za-z_][A-Za-z0-9_]*/,

    _argument: ($) =>
      choice(
        $.parameter,
        $.letter_parameter,
        alias($._signed_number, $.number),
        $.gcode_string,
        $.word,
        $._jinja,
      ),

    // KEY=value (Klipper extended commands)
    parameter: ($) =>
      prec.right(
        seq(
          field('name', $.parameter_name),
          '=',
          optional(
            field(
              'value',
              choice(
                alias($._signed_number, $.number),
                $.boolean,
                $.gcode_string,
                $.word,
                $.jinja_expression,
              ),
            ),
          ),
        ),
      ),

    // X10 / S{BED} (traditional G-code)
    letter_parameter: ($) =>
      seq(
        field('name', alias($._parameter_letter, $.parameter_name)),
        field(
          'value',
          choice(alias($._signed_number, $.number), $.jinja_expression),
        ),
      ),

    // Templates render before Klipper sees the line, so `{...}` inside a
    // G-code string is still Jinja: VALUE="'{params.MODE}'".
    gcode_string: ($) =>
      seq(
        '"',
        repeat(
          choice(
            alias(token.immediate(/[^"{\n]+/), $.string_content),
            $.jinja_expression,
            $.jinja_statement,
          ),
        ),
        token.immediate('"'),
      ),

    word: (_) => /[^\s{}#;"=][^\s{}"]*/,

    // Wins ties with `word`/`text`, so `ARG=1` and `rotation_distance: 40`
    // are numbers rather than bare words.
    _signed_number: (_) =>
      token(prec(1, /[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?/)),

    // ------------------------------------------------------------------
    // Jinja2 (Klipper uses `{ }` for expressions, not `{{ }}`)
    // ------------------------------------------------------------------

    _jinja: ($) =>
      choice($.jinja_expression, $.jinja_statement, $.jinja_comment),

    jinja_comment: (_) => token(seq('{#', /([^#]|#[^}])*/, '#}')),

    jinja_expression: ($) => seq('{', $._expression, '}'),

    jinja_statement: ($) =>
      seq(
        choice('{%', '{%-'),
        optional($._statement),
        choice('%}', '-%}'),
      ),

    _statement: ($) =>
      choice(
        $.set_statement,
        $.if_statement,
        $.elif_statement,
        $.for_statement,
        $.macro_statement,
        $.call_statement,
        $.filter_statement,
        $.do_statement,
        $.with_statement,
        alias(
          choice(
            'else',
            'endif',
            'endfor',
            'endset',
            'endmacro',
            'endcall',
            'endfilter',
            'endwith',
            'break',
            'continue',
          ),
          $.keyword_statement,
        ),
      ),

    set_statement: ($) =>
      seq(
        'set',
        field('target', commaSep1($._expression)),
        optional(seq('=', field('value', $._expression))),
      ),

    if_statement: ($) => seq('if', field('condition', $._expression)),
    elif_statement: ($) => seq('elif', field('condition', $._expression)),

    for_statement: ($) =>
      seq(
        'for',
        field('target', commaSep1($.identifier)),
        'in',
        field('iterable', $._expression),
        optional('recursive'),
      ),

    macro_statement: ($) =>
      seq('macro', field('name', $.identifier), $.parameter_list),

    parameter_list: ($) =>
      seq(
        '(',
        commaSep(
          choice(
            $.identifier,
            seq($.identifier, '=', $._expression),
          ),
        ),
        ')',
      ),

    call_statement: ($) =>
      seq('call', $._expression),

    filter_statement: ($) =>
      seq('filter', $.identifier, optional($.argument_list)),

    do_statement: ($) => seq('do', $._expression),

    with_statement: ($) =>
      seq(
        'with',
        commaSep(seq($.identifier, '=', $._expression)),
      ),

    _expression: ($) =>
      choice(
        $.identifier,
        alias($._unsigned_number, $.number),
        $.string,
        $.concatenated_string,
        $.boolean,
        $.none,
        $.list,
        $.dict,
        $.tuple,
        $.parenthesized_expression,
        $.attribute,
        $.subscript,
        $.call,
        $.filter,
        $.test,
        $.unary_expression,
        $.binary_expression,
        $.conditional_expression,
      ),

    identifier: (_) => /[A-Za-z_][A-Za-z0-9_]*/,

    _unsigned_number: (_) => /(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?/,

    // A string may continue onto indented lines (Klipper joins them before
    // Jinja runs), but never past a column-0 line, so an unclosed quote can't
    // swallow the rest of the file.
    string: (_) =>
      token(
        choice(
          seq('"', repeat(choice(/[^"\\\n]/, /\\./, /\r?\n[ \t]+/)), '"'),
          seq("'", repeat(choice(/[^'\\\n]/, /\\./, /\r?\n[ \t]+/)), "'"),
        ),
      ),

    // Jinja joins adjacent literals, handy for multi-line messages.
    concatenated_string: ($) => seq($.string, repeat1($.string)),

    boolean: (_) => choice('true', 'false', 'True', 'False'),
    none: (_) => choice('none', 'None'),

    list: ($) => seq('[', commaSep($._expression), optional(','), ']'),

    dict: ($) => seq('{', commaSep($.pair), optional(','), '}'),
    pair: ($) =>
      seq(field('key', $._expression), ':', field('value', $._expression)),

    tuple: ($) =>
      seq(
        '(',
        optional(
          seq($._expression, ',', commaSep($._expression), optional(',')),
        ),
        ')',
      ),

    parenthesized_expression: ($) => seq('(', $._expression, ')'),

    attribute: ($) =>
      prec(
        PREC.postfix,
        seq(
          field('object', $._expression),
          '.',
          field('attribute', alias($.identifier, $.property_identifier)),
        ),
      ),

    subscript: ($) =>
      prec(
        PREC.postfix,
        seq(
          field('value', $._expression),
          '[',
          field('subscript', choice($._expression, $.slice)),
          ']',
        ),
      ),

    slice: ($) =>
      seq(
        optional($._expression),
        ':',
        optional($._expression),
        optional(seq(':', optional($._expression))),
      ),

    call: ($) =>
      prec(
        PREC.postfix,
        seq(field('function', $._expression), field('arguments', $.argument_list)),
      ),

    argument_list: ($) =>
      seq(
        '(',
        commaSep(choice($._expression, $.keyword_argument)),
        optional(','),
        ')',
      ),

    keyword_argument: ($) =>
      seq(field('name', $.identifier), '=', field('value', $._expression)),

    // params.X|default(0)|int
    filter: ($) =>
      prec.right(
        PREC.filter,
        seq(
          field('value', $._expression),
          '|',
          field('name', alias($.identifier, $.filter_name)),
          optional(field('arguments', $.argument_list)),
        ),
      ),

    // params.X is defined
    test: ($) =>
      prec.right(
        PREC.filter,
        seq(
          field('value', $._expression),
          'is',
          optional('not'),
          field('name', alias(choice($.identifier, 'in'), $.test_name)),
          // `is divisibleby(3)` or the paren-less `is in ['a', 'b']`
          optional(
            field(
              'arguments',
              choice(
                $.argument_list,
                $.list,
                $.string,
                alias($._unsigned_number, $.number),
              ),
            ),
          ),
        ),
      ),

    unary_expression: ($) =>
      choice(
        prec(PREC.not, seq(field('operator', 'not'), field('argument', $._expression))),
        prec(PREC.unary, seq(field('operator', choice('-', '+')), field('argument', $._expression))),
      ),

    binary_expression: ($) => {
      const table = [
        [prec.left, PREC.or, 'or'],
        [prec.left, PREC.and, 'and'],
        [prec.left, PREC.compare, choice('==', '!=', '<', '>', '<=', '>=', 'in', seq('not', 'in'))],
        [prec.left, PREC.add, choice('+', '-')],
        [prec.left, PREC.concat, '~'],
        [prec.left, PREC.mul, choice('*', '/', '//', '%')],
        [prec.right, PREC.pow, '**'],
      ];
      return choice(
        ...table.map(([fn, p, op]) =>
          // @ts-ignore
          fn(p, seq(
            field('left', $._expression),
            // @ts-ignore
            field('operator', op),
            field('right', $._expression),
          )),
        ),
      );
    },

    conditional_expression: ($) =>
      prec.right(
        PREC.conditional,
        seq(
          field('consequence', $._expression),
          'if',
          field('condition', $._expression),
          optional(seq('else', field('alternative', $._expression))),
        ),
      ),
  },
});

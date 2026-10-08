// External scanner for tree-sitter-klipper.
//
// Klipper's config is configparser-flavoured: a value continues onto every
// following line that is indented, and blank lines / full-line comments
// (even at column 0) don't end it. That depends on the *next* line's
// indentation, which a context-free lexer can't see, so every line break goes
// through here.
//
// The scanner also lexes three things that need one character of lookahead
// beyond the token itself:
//   * option keys, so `gcode` / `*_gcode` can switch the value to a G-code
//     template
//   * `NAME=` parameter names (the `=` is checked but not consumed)
//   * `X10` / `S{...}` letter parameters (only when followed by a number or
//     a Jinja expression, so words in M118 messages stay plain words)
//   * MCU pins like `^!EBBCan:PB6`, split into modifier / chip / name, but
//     only when the whole run of characters is a pin
//
// The scanner is stateless: everything it needs is in the lookahead.

#include "tree_sitter/parser.h"

#include <string.h>
#include <wctype.h>

enum TokenType {
  NEWLINE,
  CONTINUATION,
  JINJA_NEWLINE,
  KEY,
  TEMPLATE_KEY,
  PARAMETER_NAME,
  PARAMETER_LETTER,
  PIN_MODIFIER,
  PIN_CHIP,
  PIN_NAME,
  ERROR_SENTINEL,
};

void *tree_sitter_klipper_external_scanner_create(void) { return NULL; }
void tree_sitter_klipper_external_scanner_destroy(void *payload) {}
unsigned tree_sitter_klipper_external_scanner_serialize(void *payload, char *buffer) { return 0; }
void tree_sitter_klipper_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {}

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }
static inline void skip(TSLexer *lexer) { lexer->advance(lexer, true); }

static inline bool is_hspace(int32_t c) { return c == ' ' || c == '\t' || c == '\r' || c == '\f'; }
static inline bool is_comment_start(int32_t c) { return c == '#' || c == ';'; }
static inline bool is_ascii_alpha(int32_t c) { return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z'); }
static inline bool is_ascii_digit(int32_t c) { return c >= '0' && c <= '9'; }
static inline bool is_ident_char(int32_t c) { return is_ascii_alpha(c) || is_ascii_digit(c) || c == '_'; }
static inline bool is_key_char(int32_t c) { return is_ident_char(c) || c == '-' || c == '.'; }

// Consume indentation; return its width (\r doesn't count).
static unsigned consume_indent(TSLexer *lexer) {
  unsigned indent = 0;
  while (is_hspace(lexer->lookahead)) {
    if (lexer->lookahead != '\r') indent++;
    advance(lexer);
  }
  return indent;
}

static void consume_rest_of_line(TSLexer *lexer) {
  while (lexer->lookahead != '\n' && !lexer->eof(lexer)) advance(lexer);
}

// Lookahead is at '\n'. Consumes the newline plus any blank lines, then
// decides whether the next content line continues the current value.
//
// Returns true (continuation) when that line is indented. The token then ends
// after its indentation. A column-0 comment doesn't end a value in
// configparser, so for those we peek past the comment block (without moving
// the token end) and decide on whatever follows.
static bool scan_line_break(TSLexer *lexer) {
  advance(lexer);
  lexer->mark_end(lexer);

  for (;;) {
    unsigned indent = consume_indent(lexer);
    if (lexer->lookahead == '\n') {
      advance(lexer);
      lexer->mark_end(lexer);
      continue;
    }
    if (lexer->eof(lexer)) return false;
    if (indent > 0) {
      lexer->mark_end(lexer);
      return true;
    }
    if (!is_comment_start(lexer->lookahead)) return false;

    // Column-0 comment block: look through it.
    for (;;) {
      consume_rest_of_line(lexer);
      if (lexer->eof(lexer)) return false;
      advance(lexer);
      indent = consume_indent(lexer);
      if (lexer->lookahead == '\n' || is_comment_start(lexer->lookahead)) continue;
      if (lexer->eof(lexer)) return false;
      return indent > 0;
    }
  }
}

static bool scan_key(TSLexer *lexer, const bool *valid_symbols) {
  char buf[64];
  unsigned len = 0;

  if (!is_key_char(lexer->lookahead)) return false;
  while (is_key_char(lexer->lookahead)) {
    if (len < sizeof(buf) - 1) buf[len++] = (char)towlower(lexer->lookahead);
    advance(lexer);
  }
  buf[len] = '\0';
  lexer->mark_end(lexer);

  // configparser lowercases option names, so `GCODE:` counts too.
  bool is_template = strcmp(buf, "gcode") == 0 ||
                     (len > 6 && strcmp(buf + len - 6, "_gcode") == 0);

  if (is_template && valid_symbols[TEMPLATE_KEY]) {
    lexer->result_symbol = TEMPLATE_KEY;
    return true;
  }
  if (valid_symbols[KEY]) {
    lexer->result_symbol = KEY;
    return true;
  }
  return false;
}

static inline bool is_letter_param_end(TSLexer *lexer) {
  int32_t c = lexer->lookahead;
  return lexer->eof(lexer) || c == ' ' || c == '\t' || c == '\r' || c == '\n' ||
         c == '{' || c == '}' || c == '#' || c == ';';
}

static bool scan_parameter(TSLexer *lexer, const bool *valid_symbols) {
  int32_t first = lexer->lookahead;
  if (!is_ascii_alpha(first) && first != '_') return false;
  advance(lexer);

  if (valid_symbols[PARAMETER_LETTER] && is_ascii_alpha(first)) {
    lexer->mark_end(lexer);

    // S{BED}
    if (lexer->lookahead == '{') {
      lexer->result_symbol = PARAMETER_LETTER;
      return true;
    }

    // X10, Z-0.5, E.8
    bool only_digits = true;
    bool has_number = false;
    if (lexer->lookahead == '-' || lexer->lookahead == '+') {
      only_digits = false;
      advance(lexer);
    }
    while (is_ascii_digit(lexer->lookahead) || lexer->lookahead == '.') {
      if (lexer->lookahead == '.') only_digits = false;
      has_number = true;
      advance(lexer);
    }
    if (has_number && is_letter_param_end(lexer)) {
      lexer->result_symbol = PARAMETER_LETTER;
      return true;
    }
    // Could still be an identifier like `T0_TEMP=`, as long as we only ate
    // identifier characters.
    if (!only_digits) return false;
  }

  if (!valid_symbols[PARAMETER_NAME]) return false;
  while (is_ident_char(lexer->lookahead)) advance(lexer);
  lexer->mark_end(lexer);
  if (lexer->lookahead != '=') return false;
  // `==` would be a comparison, not an assignment.
  advance(lexer);
  if (lexer->lookahead == '=') return false;
  lexer->result_symbol = PARAMETER_NAME;
  return true;
}

// ---------------------------------------------------------------------------
// Pins
// ---------------------------------------------------------------------------

static inline bool is_pin_modifier(int32_t c) { return c == '!' || c == '^' || c == '~'; }

static inline bool is_pin_end(TSLexer *lexer) {
  int32_t c = lexer->lookahead;
  return lexer->eof(lexer) || c == ' ' || c == '\t' || c == '\r' || c == '\n' ||
         c == ',' || c == ')' || c == ']' || c == '}' || c == '#' || c == ';';
}

// Reads [A-Za-z0-9_.]+ into buf. Returns false if empty or too long.
static bool read_pin_word(TSLexer *lexer, char *buf, unsigned size) {
  unsigned len = 0;
  while (is_ident_char(lexer->lookahead) || lexer->lookahead == '.') {
    if (len + 1 >= size) return false;
    buf[len++] = (char)lexer->lookahead;
    advance(lexer);
  }
  buf[len] = '\0';
  return len > 0;
}

// Consumes min..max digits from *s.
static bool eat_digits(const char **s, unsigned min, unsigned max) {
  unsigned n = 0;
  while (n < max && is_ascii_digit(**s)) {
    (*s)++;
    n++;
  }
  return n >= min;
}

static bool has_prefix(const char *s, const char *prefix) {
  return strncmp(s, prefix, strlen(prefix)) == 0;
}

static bool has_suffix(const char *s, const char *suffix) {
  size_t ls = strlen(s), lx = strlen(suffix);
  return ls >= lx && strcmp(s + ls - lx, suffix) == 0;
}

// Pin names across the MCUs Klipper supports:
//   PA1 .. PL7       STM32 / SAM / AVR ports
//   P1.24            LPC176x
//   gpio17, gpio0_27 RP2040 / Linux host / BeagleBone
//   ar5, analog3     Arduino aliases
//   EXP1_5           board connector aliases ([board_pins])
//   PIN_12           SX1509 expander
//   z_virtual_endstop, virtual_endstop (after a chip prefix)
static bool is_pin_name(const char *s, bool has_chip) {
  const char *p = s;
  if (p[0] == 'P' && p[1] >= 'A' && p[1] <= 'Z') {
    p += 2;
    if (eat_digits(&p, 1, 2) && *p == '\0') return true;
  }
  p = s;
  if (p[0] == 'P' && is_ascii_digit(p[1]) && p[2] == '.') {
    p += 3;
    if (eat_digits(&p, 1, 2) && *p == '\0') return true;
  }
  p = s;
  if (has_prefix(p, "gpio")) {
    p += 4;
    if (eat_digits(&p, 1, 3)) {
      if (*p == '_') {
        p++;
        if (!eat_digits(&p, 1, 3)) return false;
      }
      if (*p == '\0') return true;
    }
  }
  p = s;
  if (has_prefix(p, "analog") || has_prefix(p, "ar")) {
    p += has_prefix(p, "analog") ? 6 : 2;
    if (eat_digits(&p, 1, 3) && *p == '\0') return true;
  }
  p = s;
  if (has_prefix(p, "EXP")) {
    p += 3;
    if (eat_digits(&p, 1, 1) && *p == '_') {
      p++;
      if (eat_digits(&p, 1, 2) && *p == '\0') return true;
    }
  }
  p = s;
  if (has_prefix(p, "PIN_")) {
    p += 4;
    if (eat_digits(&p, 1, 2) && *p == '\0') return true;
  }
  return has_chip && has_suffix(s, "virtual_endstop");
}

static bool is_identifier(const char *s) {
  if (!(is_ascii_alpha(s[0]) || s[0] == '_')) return false;
  for (const char *p = s; *p; p++) {
    if (!is_ident_char(*p)) return false;
  }
  return true;
}

// Each call emits one piece (modifier, chip or name), but always checks that
// the rest of the pin follows. That keeps the scanner stateless.
static bool scan_pin(TSLexer *lexer, const bool *valid_symbols) {
  bool modifier = false;
  if (valid_symbols[PIN_MODIFIER] && is_pin_modifier(lexer->lookahead)) {
    while (is_pin_modifier(lexer->lookahead)) advance(lexer);
    lexer->mark_end(lexer);
    modifier = true;
  }

  char first[64];
  if (!read_pin_word(lexer, first, sizeof(first))) return false;

  if (lexer->lookahead == ':') {
    if (!is_identifier(first)) return false;
    if (!modifier) lexer->mark_end(lexer);
    advance(lexer);
    char second[64];
    if (!read_pin_word(lexer, second, sizeof(second))) return false;
    if (!is_pin_name(second, true) || !is_pin_end(lexer)) return false;
    if (modifier) {
      lexer->result_symbol = PIN_MODIFIER;
      return true;
    }
    if (!valid_symbols[PIN_CHIP]) return false;
    lexer->result_symbol = PIN_CHIP;
    return true;
  }

  // Only PIN_NAME is valid right after `chip:`, so that's how we know a
  // chip-only name like `virtual_endstop` is allowed here.
  bool after_chip = !valid_symbols[PIN_CHIP] && !valid_symbols[PIN_MODIFIER];
  if (!is_pin_name(first, after_chip) || !is_pin_end(lexer)) return false;
  if (modifier) {
    lexer->result_symbol = PIN_MODIFIER;
    return true;
  }
  if (!valid_symbols[PIN_NAME]) return false;
  lexer->mark_end(lexer);
  lexer->result_symbol = PIN_NAME;
  return true;
}

bool tree_sitter_klipper_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  // During error recovery every symbol is valid. Only produce line breaks then,
  // so recovery resynchronises on line boundaries.
  bool error_recovery = valid_symbols[ERROR_SENTINEL];
  bool line_end_valid = valid_symbols[NEWLINE] || valid_symbols[CONTINUATION];

  while (is_hspace(lexer->lookahead)) skip(lexer);

  if (lexer->lookahead == '\n') {
    if (error_recovery || line_end_valid) {
      bool continues = scan_line_break(lexer);
      if (error_recovery) {
        lexer->result_symbol = continues ? CONTINUATION : NEWLINE;
      } else if (continues && valid_symbols[CONTINUATION]) {
        lexer->result_symbol = CONTINUATION;
      } else if (valid_symbols[NEWLINE]) {
        lexer->result_symbol = NEWLINE;
      } else {
        lexer->result_symbol = CONTINUATION;
      }
      return true;
    }
    // Inside an unfinished {% ... %} / { ... }: the break is whitespace, but
    // only if the template carries on in an indented line.
    if (valid_symbols[JINJA_NEWLINE] && scan_line_break(lexer)) {
      lexer->result_symbol = JINJA_NEWLINE;
      return true;
    }
    return false;
  }

  if (lexer->eof(lexer)) {
    // A file that doesn't end in a newline still ends its last line.
    if (valid_symbols[NEWLINE] && !error_recovery) {
      lexer->result_symbol = NEWLINE;
      return true;
    }
    return false;
  }

  if (error_recovery) return false;

  if (valid_symbols[KEY] || valid_symbols[TEMPLATE_KEY]) {
    return scan_key(lexer, valid_symbols);
  }
  if (valid_symbols[PARAMETER_NAME] || valid_symbols[PARAMETER_LETTER]) {
    return scan_parameter(lexer, valid_symbols);
  }
  if (valid_symbols[PIN_MODIFIER] || valid_symbols[PIN_CHIP] || valid_symbols[PIN_NAME]) {
    return scan_pin(lexer, valid_symbols);
  }
  return false;
}

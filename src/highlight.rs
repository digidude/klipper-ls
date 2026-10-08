//! Semantic tokens: syntax highlighting for any LSP editor, from the same
//! tree-sitter queries Zed uses.
//!
//! The Zed extension's `highlights.scm` is the single source of truth for
//! *what* gets a color. Here each capture name is translated to an LSP token
//! type, so editors without tree-sitter (VS Code) match Zed.
//!
//! Tree-sitter's rule is "the later pattern wins". Patterns are painted onto
//! a byte map in pattern order, so a later capture overwrites an earlier one
//! over the same bytes even when the nodes differ in size.
//!
//! `.gcode` files are never parsed whole. A range request wraps just the
//! requested lines in a macro body (`[gcode_macro x]` / `gcode:`), parses
//! that, and maps the tokens back.

use std::ops::Range;
use std::sync::LazyLock;

use lsp_types::{SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend};
use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

use crate::position::LineIndex;
use crate::syntax;

/// Shared with the Zed extension on purpose; see the module comment.
const HIGHLIGHTS: &str = include_str!("../editors/zed-klipper-ls/languages/klipper/highlights.scm");

/// Cap on one range request, so a client that asks for a whole 1M-line file
/// still gets an answer in milliseconds.
const MAX_GCODE_LINES: usize = 3000;

const TYPES: [SemanticTokenType; 11] = [
    SemanticTokenType::COMMENT,
    SemanticTokenType::TYPE,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::OPERATOR,
];
const MODIFIERS: [SemanticTokenModifier; 2] =
    [SemanticTokenModifier::DEFAULT_LIBRARY, SemanticTokenModifier::DOCUMENTATION];

const DEFAULT_LIBRARY: u32 = 1 << 0;
const DOCUMENTATION: u32 = 1 << 1;

pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend { token_types: TYPES.to_vec(), token_modifiers: MODIFIERS.to_vec() }
}

fn type_index(t: SemanticTokenType) -> u32 {
    TYPES.iter().position(|x| *x == t).expect("type is in the legend") as u32
}

/// What a Zed capture becomes. Punctuation and unknown captures are `None`:
/// they paint "no token", which still overrides an earlier pattern.
fn map_capture(name: &str) -> Option<(u32, u32)> {
    let (t, m) = match name {
        "comment" => (SemanticTokenType::COMMENT, 0),
        "comment.doc" => (SemanticTokenType::COMMENT, DOCUMENTATION),
        "type" => (SemanticTokenType::TYPE, 0),
        "function" => (SemanticTokenType::FUNCTION, 0),
        "function.builtin" => (SemanticTokenType::FUNCTION, DEFAULT_LIBRARY),
        "property" => (SemanticTokenType::PROPERTY, 0),
        "variable" => (SemanticTokenType::VARIABLE, 0),
        "variable.special" => (SemanticTokenType::VARIABLE, DEFAULT_LIBRARY),
        "variable.parameter" | "attribute" => (SemanticTokenType::PARAMETER, 0),
        "keyword" => (SemanticTokenType::KEYWORD, 0),
        "string" | "string.special" => (SemanticTokenType::STRING, 0),
        "number" => (SemanticTokenType::NUMBER, 0),
        "constant" => (SemanticTokenType::ENUM_MEMBER, 0),
        "operator" => (SemanticTokenType::OPERATOR, 0),
        _ => return None,
    };
    Some((type_index(t), m))
}

struct Highlighter {
    query: Query,
    /// Per capture index; `None` = punctuation / unmapped, `Skip` = `@_helper`.
    captures: Vec<Capture>,
}

#[derive(Clone, Copy)]
enum Capture {
    Skip,
    Clear,
    Token(u32, u32),
}

static HIGHLIGHTER: LazyLock<Highlighter> = LazyLock::new(|| {
    let query = Query::new(&syntax::language(), HIGHLIGHTS).expect("highlights.scm matches the grammar");
    let captures = query
        .capture_names()
        .iter()
        .map(|name| {
            if name.starts_with('_') {
                Capture::Skip
            } else {
                map_capture(name).map_or(Capture::Clear, |(t, m)| Capture::Token(t, m))
            }
        })
        .collect();
    Highlighter { query, captures }
});

/// A colored span on one line, in bytes of the text it was computed from.
#[derive(Debug, PartialEq)]
struct Span {
    start: usize,
    end: usize,
    token_type: u32,
    modifiers: u32,
}

/// Paint `tree` over `bytes` (a sub-range of the text), later patterns on top.
fn spans(tree: &Tree, text: &str, bytes: Range<usize>) -> Vec<Span> {
    let h = &*HIGHLIGHTER;
    let mut found: Vec<(usize, usize, usize, Capture)> = Vec::new(); // pattern, start, end, capture
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(bytes.clone());
    let mut matches = cursor.matches(&h.query, tree.root_node(), text.as_bytes());
    while let Some(m) = matches.next() {
        for c in m.captures {
            let capture = h.captures[c.index as usize];
            if matches!(capture, Capture::Skip) {
                continue;
            }
            let r = c.node.byte_range();
            found.push((m.pattern_index, r.start.max(bytes.start), r.end.min(bytes.end), capture));
        }
    }
    found.sort_by_key(|f| f.0); // stable: ties keep document order

    // 0 = unpainted/cleared; otherwise token_type + 1 and the modifiers.
    let mut paint: Vec<(u32, u32)> = vec![(0, 0); bytes.len()];
    for (_, start, end, capture) in found {
        if start >= end {
            continue;
        }
        let value = match capture {
            Capture::Token(t, m) => (t + 1, m),
            _ => (0, 0),
        };
        paint[start - bytes.start..end - bytes.start].fill(value);
    }

    // Runs of equal paint, split at line ends and trimmed of whitespace.
    let mut out = Vec::new();
    let raw = text.as_bytes();
    let mut i = 0;
    while i < paint.len() {
        let value = paint[i];
        let mut j = i + 1;
        while j < paint.len() && paint[j] == value && raw[bytes.start + j - 1] != b'\n' {
            j += 1;
        }
        if value.0 != 0 {
            let (mut s, mut e) = (bytes.start + i, bytes.start + j);
            while s < e && raw[s].is_ascii_whitespace() {
                s += 1;
            }
            while e > s && raw[e - 1].is_ascii_whitespace() {
                e -= 1;
            }
            if s < e {
                out.push(Span { start: s, end: e, token_type: value.0 - 1, modifiers: value.1 });
            }
        }
        i = j;
    }
    out
}

/// LSP's relative encoding. `line_offset`/`col_offset` shift tokens computed
/// on wrapped text back to the real document.
fn encode(spans: &[Span], text: &str, lines: &LineIndex, line_base: u32, col_cut: u32) -> Vec<SemanticToken> {
    let mut data = Vec::with_capacity(spans.len());
    let (mut prev_line, mut prev_col) = (0, 0);
    for s in spans {
        let pos = lines.position(text, s.start);
        if pos.character < col_cut || pos.line < line_base {
            continue;
        }
        let (line, col) = (pos.line - line_base, pos.character - col_cut);
        let length = text[s.start..s.end].encode_utf16().count() as u32;
        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 { col - prev_col } else { col };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: s.token_type,
            token_modifiers_bitset: s.modifiers,
        });
        (prev_line, prev_col) = (line, col);
    }
    data
}

/// Tokens for a parsed config, optionally limited to a line range.
pub fn config_tokens(tree: &Tree, text: &str, lines: &LineIndex, line_range: Option<Range<u32>>) -> Vec<SemanticToken> {
    let bytes = match line_range {
        Some(r) => {
            let start = lines.offset(text, lsp_types::Position::new(r.start, 0));
            let end = lines.offset(text, lsp_types::Position::new(r.end, 0));
            start..end.max(start)
        }
        None => 0..text.len(),
    };
    let spans = spans(tree, text, bytes);
    encode(&spans, text, lines, 0, 0)
}

/// Tokens for some lines of a `.gcode` file, without touching the rest.
pub fn gcode_tokens(text: &str, lines: &LineIndex, line_range: Range<u32>) -> Vec<SemanticToken> {
    const HEADER: &str = "[gcode_macro x]\ngcode:\n";
    const INDENT: &str = "  ";

    let first = line_range.start;
    let last = line_range.end.min(first.saturating_add(MAX_GCODE_LINES as u32));
    let from = lines.offset(text, lsp_types::Position::new(first, 0));
    let to = lines.offset(text, lsp_types::Position::new(last, 0)).max(from);

    let mut wrapped = String::with_capacity(HEADER.len() + (to - from) + 4 * (last - first) as usize);
    wrapped.push_str(HEADER);
    for line in text[from..to].split_inclusive('\n') {
        wrapped.push_str(INDENT);
        wrapped.push_str(line.trim_end_matches(['\n', '\r']));
        wrapped.push('\n');
    }

    let tree = syntax::parse(&wrapped);
    let wrapped_lines = LineIndex::new(&wrapped);
    let spans = spans(&tree, &wrapped, 0..wrapped.len());
    // Two header lines come first; the indent is two columns. Tokens are
    // relative to the first requested line, so shift line numbers by `first`.
    let mut data = encode(&spans, &wrapped, &wrapped_lines, 2, INDENT.len() as u32);
    if let Some(head) = data.first_mut() {
        head.delta_line += first;
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (text, token type name, modifiers) for every token, decoded.
    fn decode(text: &str, data: &[SemanticToken]) -> Vec<(String, String, u32)> {
        let lines: Vec<&str> = text.split('\n').collect();
        let (mut line, mut col) = (0usize, 0u32);
        data.iter()
            .map(|t| {
                line += t.delta_line as usize;
                col = if t.delta_line == 0 { col + t.delta_start } else { t.delta_start };
                let s: String = lines[line].encode_utf16().skip(col as usize).take(t.length as usize).collect::<Vec<_>>()
                    .pipe(|u| String::from_utf16_lossy(&u));
                (s, TYPES[t.token_type as usize].as_str().to_string(), t.token_modifiers_bitset)
            })
            .collect()
    }

    trait Pipe: Sized {
        fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R { f(self) }
    }
    impl<T> Pipe for T {}

    fn config(text: &str) -> Vec<(String, String, u32)> {
        let tree = syntax::parse(text);
        decode(text, &config_tokens(&tree, text, &LineIndex::new(text), None))
    }

    fn has(tokens: &[(String, String, u32)], text: &str, ty: &str) -> bool {
        tokens.iter().any(|(t, k, _)| t == text && k == ty)
    }

    #[test]
    fn highlights_sections_options_and_values() {
        let t = config("# note\n[heater_bed]\nmax_temp: 120\nsensor_pin: ^!PA3\n");
        assert!(has(&t, "# note", "comment"));
        assert!(has(&t, "heater_bed", "type"));
        assert!(has(&t, "max_temp", "property"));
        assert!(has(&t, "120", "number"));
        assert!(has(&t, "PA3", "enumMember"), "{t:?}");
    }

    #[test]
    fn highlights_macro_bodies_like_zed() {
        let src = "[gcode_macro PRINT_START]\ngcode:\n  {% set BED = params.BED|default(60)|float %}\n  M140 S{BED}\n  HEAT_SOAK TIME=5\n";
        let t = config(src);
        assert!(has(&t, "PRINT_START", "function"), "macro name: {t:?}");
        assert!(has(&t, "set", "keyword"));
        assert!(t.iter().any(|(s, k, m)| s == "params" && k == "variable" && m & DEFAULT_LIBRARY != 0));
        assert!(has(&t, "default", "function"));
        assert!(has(&t, "M140", "keyword"), "standard G/M code: {t:?}");
        assert!(has(&t, "HEAT_SOAK", "function"));
        assert!(has(&t, "TIME", "parameter"));
    }

    #[test]
    fn later_pattern_wins() {
        // `variable_x` is a @property key, then re-captured as @variable.special.
        let t = config("[gcode_macro A]\nvariable_x: 1\ngcode:\n  G28\n");
        assert!(t.iter().any(|(s, k, m)| s == "variable_x" && k == "variable" && m & DEFAULT_LIBRARY != 0), "{t:?}");
        assert!(!has(&t, "variable_x", "property"));
    }

    #[test]
    fn counts_utf16_columns() {
        let t = config("[a]\n# 60°C max\nb: 2\n");
        assert!(has(&t, "# 60°C max", "comment"));
        assert!(has(&t, "2", "number"));
    }

    #[test]
    fn gcode_ranges_map_back_to_real_lines() {
        let text = "; header\nG28\nG1 X10.5 Y20 ; move\nM104 S0\nM109 S215\n";
        let lines = LineIndex::new(text);
        // Lines 2..4: only the middle two lines of the file.
        let t = decode(text, &gcode_tokens(text, &lines, 2..4));
        assert!(has(&t, "G1", "keyword"), "{t:?}");
        assert!(has(&t, "X", "parameter"));
        assert!(has(&t, "10.5", "number"));
        assert!(has(&t, "; move", "comment"));
        assert!(has(&t, "M104", "keyword"));
        assert!(!has(&t, "G28", "keyword"), "line 1 is outside the range");
        assert!(!has(&t, "M109", "keyword"), "line 4 is outside the range");
    }

    #[test]
    fn highlights_file_matches_the_legend() {
        // Every mapped capture resolves to a legend entry (panics otherwise).
        for name in HIGHLIGHTER.query.capture_names() {
            let _ = map_capture(name);
        }
    }
}

#[cfg(test)]
mod real_data {
    use super::*;
    use std::time::Instant;

    /// `GCODE_DIR=test-data/voron/printer_data/gcodes cargo test highlight_gcode_ranges -- --ignored --nocapture`
    #[test]
    #[ignore = "needs GCODE_DIR with real slicer output"]
    fn highlight_gcode_ranges() {
        let dir = std::env::var("GCODE_DIR").expect("set GCODE_DIR");
        let mut files: Vec<_> = walk(std::path::Path::new(&dir));
        files.sort_by_key(|p| std::fs::metadata(p).map(|m| std::cmp::Reverse(m.len())).ok());
        let (mut windows, mut tokens, mut slowest) = (0, 0, 0u128);
        for path in files.iter().take(5) {
            let text = std::fs::read_to_string(path).unwrap();
            let lines = LineIndex::new(&text);
            let count = text.matches('\n').count() as u32;
            for start in [0, count / 3, count / 2, count.saturating_sub(3000)] {
                let t = Instant::now();
                let data = gcode_tokens(&text, &lines, start..start + 60);
                let bigger = gcode_tokens(&text, &lines, start..start + 3000);
                slowest = slowest.max(t.elapsed().as_millis());
                assert!(!data.is_empty() && !bigger.is_empty(), "{} @ {start}", path.display());
                windows += 1;
                tokens += bigger.len();
            }
        }
        println!("{windows} windows, {tokens} tokens, slowest {slowest} ms (60+3000-line windows)");
    }

    /// `KLIPPER_CONFIG_DIR=test-data/voron cargo test highlight_configs -- --ignored --nocapture`
    #[test]
    #[ignore = "needs KLIPPER_CONFIG_DIR with real configs"]
    fn highlight_configs() {
        let dir = std::env::var("KLIPPER_CONFIG_DIR").expect("set KLIPPER_CONFIG_DIR");
        let (mut files, mut tokens) = (0, 0);
        for path in walk(std::path::Path::new(&dir)).into_iter().filter(|p| p.extension().is_some_and(|e| e == "cfg")) {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let tree = syntax::parse(&text);
            let data = config_tokens(&tree, &text, &LineIndex::new(&text), None);
            assert!(!data.is_empty() || text.trim().is_empty(), "{}", path.display());
            files += 1;
            tokens += data.len();
        }
        println!("{files} configs, {tokens} tokens");
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() { out.extend(walk(&p)) } else { out.push(p) }
        }
        out
    }
}

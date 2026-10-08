//! LSP positions count UTF-16 code units per line; tree-sitter counts bytes.
//! Klipper configs are nearly always ASCII, where the two agree, but a single
//! `°` in a comment would otherwise shift every hover on that line.

use lsp_types::{Position, Range};

pub struct LineIndex {
    /// Byte offset where each line starts.
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self { starts }
    }

    /// LSP position -> byte offset (clamped to the line / document end).
    pub fn offset(&self, text: &str, position: Position) -> usize {
        let line = position.line as usize;
        let Some(&start) = self.starts.get(line) else {
            return text.len();
        };
        let end = self.starts.get(line + 1).copied().unwrap_or(text.len());
        let mut units = 0;
        for (i, ch) in text[start..end].char_indices() {
            if units >= position.character {
                return start + i;
            }
            units += ch.len_utf16() as u32;
        }
        end
    }

    /// Byte offset -> LSP position. `offset` must be on a char boundary,
    /// which tree-sitter node boundaries always are.
    pub fn position(&self, text: &str, offset: usize) -> Position {
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        let start = self.starts[line];
        Position {
            line: line as u32,
            character: text[start..offset].encode_utf16().count() as u32,
        }
    }

    pub fn range(&self, text: &str, start: usize, end: usize) -> Range {
        Range {
            start: self.position(text, start),
            end: self.position(text, end),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_non_ascii() {
        let text = "a: 1\n# 60°C max\nb: 2\n";
        let lines = LineIndex::new(text);
        // 'm' of "max": byte 13 on line 1 ('°' is 2 bytes, 1 UTF-16 unit)
        let offset = text.find("max").unwrap();
        let position = lines.position(text, offset);
        assert_eq!(position, Position::new(1, 7));
        assert_eq!(lines.offset(text, position), offset);
    }

    #[test]
    fn clamps_past_the_end() {
        let text = "a: 1\n";
        let lines = LineIndex::new(text);
        assert_eq!(lines.offset(text, Position::new(9, 0)), text.len());
        assert_eq!(lines.offset(text, Position::new(0, 99)), text.len());
    }
}

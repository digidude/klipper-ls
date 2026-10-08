//! `.gcode` files. Slicer output can run to millions of lines, so there is no
//! whole-file parse: a hover looks only at the line under the cursor, the way
//! Klipper itself reads it.
//!
//! - `;` starts a comment.
//! - The first word is the command (`G1`, `M109`, `PRINT_START`).
//! - Standard codes take letter parameters (`X10`, `S215`), including the
//!   packed form `G1X10Y20`. Extended commands take `NAME=value`.
//! - `M117`/`M118`/`M23` take free text, so their words aren't parameters.

use crate::features::{Kind, Target};
use crate::knowledge::is_standard_code;

const FREE_TEXT_COMMANDS: [&str; 3] = ["M117", "M118", "M23"];

pub fn target_at(text: &str, offset: usize) -> Option<Target> {
    let offset = offset.min(text.len());
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    let line = &text[line_start..line_end];
    let code = &line[..line.find(';').unwrap_or(line.len())];
    let cursor = offset - line_start;
    if cursor >= code.len() {
        return None;
    }

    let words = words_with_offsets(code);
    let &(first_at, first) = words.first()?;
    let command_len = command_len(first);
    let command = first[..command_len].to_ascii_uppercase();
    let at = |start: usize, end: usize, kind: Kind| {
        Some(Target { kind, start: line_start + start, end: line_start + end })
    };

    let &(word_at, word) = words.iter().find(|(at, w)| cursor >= *at && cursor < at + w.len())?;

    // The command itself, or a parameter packed onto it (`G1X10`).
    if word_at == first_at {
        let in_word = cursor - first_at;
        if in_word < command_len {
            return at(first_at, first_at + command_len, Kind::Command { name: command });
        }
        let (start, len) = packed_param_at(&first[command_len..], in_word - command_len)?;
        let name = first[command_len + start..][..1].to_ascii_uppercase();
        let start = first_at + command_len + start;
        return at(start, start + len, Kind::Parameter { command, name });
    }

    if FREE_TEXT_COMMANDS.contains(&command.as_str()) {
        return None;
    }

    // NAME=value; on the value of MACRO=, the value names a macro.
    if let Some((name, value)) = word.split_once('=') {
        let value_at = word_at + name.len() + 1;
        if name.eq_ignore_ascii_case("MACRO") && cursor >= value_at && !value.is_empty() {
            let kind = Kind::Macro { name: value.to_ascii_uppercase(), is_definition: false };
            return at(value_at, value_at + value.len(), kind);
        }
        let kind = Kind::Parameter { command, name: name.to_ascii_uppercase() };
        return at(word_at, word_at + name.len(), kind);
    }

    // X10, S215, E.5
    if is_standard_code(&command) && word.starts_with(|c: char| c.is_ascii_alphabetic()) {
        let name = word[..1].to_ascii_uppercase();
        return at(word_at, word_at + word.len(), Kind::Parameter { command, name });
    }
    None
}

fn words_with_offsets(code: &str) -> Vec<(usize, &str)> {
    let mut words = Vec::new();
    let mut start = None;
    for (i, c) in code.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                words.push((s, &code[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        words.push((s, &code[s..]));
    }
    words
}

/// `G1`, `M109`, `G29.1`: a letter and a number (stopping at the next
/// letter, so `G1X10` is `G1`). Any other first word is a whole command name.
fn command_len(word: &str) -> usize {
    let bytes = word.as_bytes();
    let is_code = bytes.len() > 1 && bytes[0].is_ascii_alphabetic() && bytes[1].is_ascii_digit();
    if !is_code {
        return word.len();
    }
    1 + word[1..]
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(word.len() - 1)
}

/// In `X10Y20.5`, the (start, len) of the letter+number containing `at`.
fn packed_param_at(rest: &str, at: usize) -> Option<(usize, usize)> {
    let mut start = 0;
    for (i, c) in rest.char_indices().skip(1) {
        if c.is_ascii_alphabetic() {
            if at < i {
                break;
            }
            start = i;
        }
    }
    let len = rest[start + 1..]
        .find(|c: char| c.is_ascii_alphabetic())
        .map_or(rest.len() - start, |n| n + 1);
    rest[start..].starts_with(|c: char| c.is_ascii_alphabetic()).then_some((start, len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::describe;

    fn kind(line: &str, needle: &str) -> Option<String> {
        let offset = line.find(needle).expect("needle") ;
        target_at(line, offset).map(|t| describe(&t.kind))
    }

    #[test]
    fn commands_and_parameters() {
        let line = "M109 S215 R180 ; wait";
        assert_eq!(kind(line, "M109").as_deref(), Some("command M109"));
        assert_eq!(kind(line, "S215").as_deref(), Some("param M109.S"));
        assert_eq!(kind(line, "R180").as_deref(), Some("param M109.R"));
        assert_eq!(kind(line, "wait"), None, "comments have no targets");
    }

    #[test]
    fn extended_commands_and_macros() {
        let text = "G28\nPRINT_START BED=60 HOTEND=215\nSET_GCODE_VARIABLE MACRO=Foo VARIABLE=x VALUE=1\n";
        assert_eq!(kind(text, "PRINT_START").as_deref(), Some("command PRINT_START"));
        assert_eq!(kind(text, "BED=").as_deref(), Some("param PRINT_START.BED"));
        assert_eq!(kind(text, "Foo").as_deref(), Some("macro FOO"));
        assert_eq!(kind(text, "MACRO=").as_deref(), Some("param SET_GCODE_VARIABLE.MACRO"));
    }

    #[test]
    fn packed_parameters() {
        let line = "G1X10.5Y20E-0.2";
        assert_eq!(kind(line, "G1").as_deref(), Some("command G1"));
        assert_eq!(kind(line, "X10").as_deref(), Some("param G1.X"));
        assert_eq!(kind(line, "Y20").as_deref(), Some("param G1.Y"));
        assert_eq!(kind(line, "0.2").as_deref(), Some("param G1.E"));
        let target = target_at(line, line.find("Y20").unwrap()).unwrap();
        assert_eq!(&line[target.start..target.end], "Y20");
    }

    #[test]
    fn free_text_is_not_parameters() {
        let line = "M117 Layer 3 of 120";
        assert_eq!(kind(line, "Layer"), None);
        assert_eq!(kind(line, "M117").as_deref(), Some("command M117"));
    }
}

#[cfg(test)]
mod real_gcode {
    use std::collections::{BTreeMap, HashMap};
    use std::path::{Path, PathBuf};

    use crate::features::{Context, hover_markdown};
    use crate::knowledge::{KlipperDocs, Sources, marlin::MarlinDocs};
    use crate::{index, knowledge};

    /// Every distinct command in real slicer output: does it get a hover, and
    /// which ones would Klipper skip?
    /// `GCODE_DIR=... KLIPPER_DOCS=... cargo test gcode_coverage -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn gcode_coverage() {
        let dir = PathBuf::from(std::env::var("GCODE_DIR").expect("set GCODE_DIR"));
        let klipper = KlipperDocs::load(Path::new(&std::env::var("KLIPPER_DOCS").unwrap())).unwrap();
        let marlin = MarlinDocs::load(&knowledge::cache_root().unwrap().join("marlin")).unwrap();
        let sources = Sources { klipper: Some(&klipper), marlin: Some(&marlin) };

        let mut uses: BTreeMap<String, usize> = BTreeMap::new();
        let mut first_file = None;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "gcode") {
                continue;
            }
            first_file.get_or_insert(path.clone());
            let text = std::fs::read_to_string(&path).unwrap();
            for line in text.lines() {
                let code = line.split(';').next().unwrap_or("");
                if let Some(word) = code.split_whitespace().next() {
                    let name = word[..super::command_len(word)].to_ascii_uppercase();
                    *uses.entry(name).or_default() += 1;
                }
            }
        }

        let file = first_file.unwrap();
        let printer_cfg = index::printer_cfg_for(&file, None);
        println!("macros from: {printer_cfg:?}");
        let index = printer_cfg.map(|p| index::build_from(&p, None, &HashMap::new())).unwrap_or_default();
        let ctx = Context { path: &file, index: &index, sources };

        for (name, count) in &uses {
            let target = crate::features::Target {
                kind: crate::features::Kind::Command { name: name.clone() },
                start: 0,
                end: 0,
            };
            let status = match hover_markdown(&ctx, &target) {
                None => "NO HOVER".to_string(),
                Some(md) if md.contains("not handled by Klipper") => "skipped by Klipper".to_string(),
                Some(md) => {
                    let mut from = Vec::new();
                    if md.contains("· macro") { from.push("macro") }
                    if klipper.command(name).is_some() { from.push("klipper") }
                    if md.contains("Marlin reference") { from.push("marlin") }
                    from.join("+")
                }
            };
            println!("{count:>8}  {name:<28} {status}");
        }
    }
}

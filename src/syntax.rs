//! The tree-sitter-klipper grammar (compiled from grammar/ by build.rs) and
//! small helpers for walking its trees.

use tree_sitter::{Language, Node, Parser, Tree};
use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_klipper() -> *const ();
}

pub fn language() -> Language {
    // SAFETY: tree_sitter_klipper is the generated, statically linked
    // language function; it takes no arguments and returns a static pointer.
    let language_fn = unsafe { LanguageFn::from_raw(tree_sitter_klipper) };
    language_fn.into()
}

pub fn parse(text: &str) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&language())
        .expect("grammar ABI matches the tree-sitter crate");
    // Only returns None on cancellation or timeout, neither of which we set.
    parser.parse(text, None).expect("parse without a timeout")
}

pub fn text<'a>(node: Node, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("").trim()
}

pub fn ancestor<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut current = node.parent();
    while let Some(n) = current {
        if n.kind() == kind {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

pub fn field_text<'a>(node: Node, field: &str, source: &'a str) -> Option<&'a str> {
    node.child_by_field_name(field).map(|n| text(n, source))
}

/// `[type name]` of a section node.
pub fn section_header<'a>(section: Node, source: &'a str) -> Option<(&'a str, Option<&'a str>)> {
    let header = section.child_by_field_name("header")?;
    let ty = field_text(header, "type", source)?;
    Some((ty, field_text(header, "name", source)))
}

/// Collapse a (possibly multi-line) config value onto one line.
pub fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_macro() {
        let source = "[gcode_macro A]\ngcode:\n  G28\n";
        let tree = parse(source);
        let root = tree.root_node();
        assert!(!root.has_error());
        let section = root.named_child(0).unwrap();
        assert_eq!(section_header(section, source), Some(("gcode_macro", Some("A"))));
    }
}

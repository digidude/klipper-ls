//! The user's own macros, found the way Klipper finds them: start at
//! `printer.cfg` and follow `[include]`s (globs relative to the including
//! file). Old backups like `printer-20240601.cfg` sitting in the same folder
//! are never included, so they don't show up as duplicate definitions.
//!
//! The index is rebuilt per request. Parsing a whole config is ~1 ms, which
//! is cheaper than getting cache invalidation right.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use lsp_types::Range;
use tree_sitter::Node;

use crate::position::LineIndex;
use crate::syntax::{self, collapse, field_text, section_header};

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub default: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MacroDef {
    pub name: String,
    pub path: PathBuf,
    /// The `[gcode_macro NAME]` header.
    pub range: Range,
    pub description: Option<String>,
    pub rename_existing: Option<String>,
    /// `params.X` references in the template, with `|default(...)` values.
    pub params: Vec<Param>,
    /// `variable_x: value`, without the prefix.
    pub variables: Vec<(String, String)>,
}

/// A `[type name]` header, as written.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionRef {
    pub ty: String,
    pub name: Option<String>,
}

impl SectionRef {
    /// The printer object this section creates: `heater_generic chamber`.
    pub fn object_name(&self) -> String {
        match &self.name {
            Some(name) => format!("{} {}", self.ty, name.split_whitespace().collect::<Vec<_>>().join(" ")),
            None => self.ty.clone(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Index {
    pub root: PathBuf,
    pub macros: HashMap<String, Vec<MacroDef>>,
    /// Every section header in printer.cfg and what it includes.
    pub sections: Vec<SectionRef>,
    /// An `[include]` of a single file that doesn't exist here (typically
    /// `/home/pi/...`). Whatever it defines is unknown, so checks that
    /// depend on the full config must not assert anything.
    pub unresolved_includes: bool,
}

impl Index {
    pub fn macros(&self, name: &str) -> &[MacroDef] {
        self.macros.get(&name.to_ascii_uppercase()).map_or(&[], Vec::as_slice)
    }
}

/// Nearest folder at or above `file` with a printer.cfg.
pub fn config_root(file: &Path) -> PathBuf {
    let dir = file.parent().unwrap_or(file);
    dir.ancestors()
        .find(|d| d.join("printer.cfg").is_file())
        .unwrap_or(dir)
        .to_path_buf()
}

fn normalize(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The `printer.cfg` whose macros a `.gcode` file can call. G-code usually
/// lives apart from the config, so: an explicit setting (file or folder)
/// wins; otherwise look in each parent folder for `printer.cfg`,
/// `config/printer.cfg` or `printer_data/config/printer.cfg` (Moonraker keeps
/// `printer_data/gcodes` next to `printer_data/config`), then
/// `~/printer_data/config/printer.cfg`.
pub fn printer_cfg_for(file: &Path, explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        let candidate = if path.is_dir() { path.join("printer.cfg") } else { path.to_path_buf() };
        return candidate.is_file().then_some(candidate);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    file.parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .chain(home.as_deref())
        .flat_map(|dir| {
            ["printer.cfg", "config/printer.cfg", "printer_data/config/printer.cfg"].map(|p| dir.join(p))
        })
        .find(|candidate| candidate.is_file())
}

/// Index for a Klipper config file: its nearest `printer.cfg` and everything
/// that includes, plus the file itself in case nothing includes it.
/// `open` holds unsaved editor buffers, keyed by normalized path.
pub fn build(file: &Path, open: &HashMap<PathBuf, &str>) -> Index {
    let root = config_root(file);
    build_from(&root.join("printer.cfg"), Some(file), open)
}

/// Index starting at `printer_cfg` (and optionally one more file).
pub fn build_from(printer_cfg: &Path, also: Option<&Path>, open: &HashMap<PathBuf, &str>) -> Index {
    let root = printer_cfg.parent().unwrap_or(Path::new("/")).to_path_buf();
    let mut index = Index {
        root,
        ..Index::default()
    };

    let mut queue: VecDeque<PathBuf> = std::iter::once(printer_cfg.to_path_buf())
        .chain(also.map(Path::to_path_buf))
        .collect();
    let mut seen = HashSet::new();

    while let Some(path) = queue.pop_front() {
        let key = normalize(&path);
        if !seen.insert(key.clone()) {
            continue;
        }
        let text: Cow<str> = match open.get(&key) {
            Some(text) => Cow::Borrowed(*text),
            None => match fs::read_to_string(&path) {
                Ok(text) => Cow::Owned(text),
                Err(_) => continue,
            },
        };

        let tree = syntax::parse(&text);
        let lines = LineIndex::new(&text);
        let root_node = tree.root_node();
        let mut cursor = root_node.walk();
        for section in root_node.named_children(&mut cursor) {
            if section.kind() != "section" {
                continue;
            }
            if let Some((ty, name)) = section_header(section, &text) {
                index.sections.push(SectionRef { ty: ty.to_string(), name: name.map(str::to_string) });
            }
            match section_header(section, &text) {
                Some(("include", Some(pattern))) => {
                    let files = resolve_include(&path, pattern);
                    if files.is_empty() && !pattern.contains(['*', '?', '[']) {
                        index.unresolved_includes = true;
                    }
                    queue.extend(files);
                }
                Some(("gcode_macro", Some(_))) => {
                    if let Some(def) = parse_macro(section, &text, &path, &lines) {
                        index.macros.entry(def.name.clone()).or_default().push(def);
                    }
                }
                _ => {}
            }
        }
    }
    index
}

/// Files matched by an `[include ...]` pattern. Absolute paths that only
/// exist on the printer (`/home/pi/...`) simply match nothing.
pub fn resolve_include(from: &Path, pattern: &str) -> Vec<PathBuf> {
    let base = from.parent().unwrap_or(Path::new("."));
    let full = if Path::new(pattern).is_absolute() {
        PathBuf::from(pattern)
    } else {
        base.join(pattern)
    };
    let full_str = full.to_string_lossy();
    if full_str.contains(['*', '?', '[']) {
        let mut matches: Vec<PathBuf> = glob::glob(&full_str)
            .map(|paths| paths.filter_map(Result::ok).collect())
            .unwrap_or_default();
        matches.sort();
        matches
    } else if full.is_file() {
        vec![full]
    } else {
        Vec::new()
    }
}

fn parse_macro(section: Node, text: &str, path: &Path, lines: &LineIndex) -> Option<MacroDef> {
    let header = section.child_by_field_name("header")?;
    let name = field_text(header, "name", text)?.to_ascii_uppercase();
    let mut def = MacroDef {
        name,
        path: path.to_path_buf(),
        range: lines.range(text, header.start_byte(), header.end_byte()),
        description: None,
        rename_existing: None,
        params: Vec::new(),
        variables: Vec::new(),
    };

    let mut cursor = section.walk();
    for option in section.named_children(&mut cursor) {
        let Some(key) = field_text(option, "key", text) else { continue };
        let key = key.to_ascii_lowercase();
        let value = option.child_by_field_name("value");
        match (option.kind(), key.as_str()) {
            ("gcode_option", "gcode") => {
                if let Some(block) = value {
                    collect_params(block, text, &mut def.params);
                }
            }
            ("option", _) => {
                let value = value.map(|v| collapse(syntax::text(v, text))).unwrap_or_default();
                if key == "description" {
                    def.description = Some(value);
                } else if key == "rename_existing" {
                    def.rename_existing = Some(value);
                } else if let Some(variable) = key.strip_prefix("variable_") {
                    def.variables.push((variable.to_string(), value));
                }
            }
            _ => {}
        }
    }
    Some(def)
}

fn collect_params(node: Node, text: &str, params: &mut Vec<Param>) {
    if let Some(name) = param_reference(node, text) {
        let default = default_argument(node, text);
        match params.iter_mut().find(|p| p.name == name) {
            Some(existing) if existing.default.is_none() => existing.default = default,
            Some(_) => {}
            None => params.push(Param { name, default }),
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_params(child, text, params);
    }
}

/// `params.BED` or `params["BED"]` -> `BED`.
fn param_reference(node: Node, text: &str) -> Option<String> {
    let (object, name) = match node.kind() {
        "attribute" => (
            node.child_by_field_name("object")?,
            field_text(node, "attribute", text)?.to_string(),
        ),
        "subscript" => {
            let key = node.child_by_field_name("subscript")?;
            if key.kind() != "string" {
                return None;
            }
            let quoted = syntax::text(key, text);
            (node.child_by_field_name("value")?, quoted[1..quoted.len() - 1].to_string())
        }
        _ => return None,
    };
    (object.kind() == "identifier" && syntax::text(object, text) == "params")
        .then(|| name.to_ascii_uppercase())
}

/// The `100` in `params.BED|default(100)`.
fn default_argument(node: Node, text: &str) -> Option<String> {
    let filter = node.parent().filter(|p| p.kind() == "filter")?;
    if filter.child_by_field_name("value") != Some(node) {
        return None;
    }
    if !matches!(field_text(filter, "name", text)?, "default" | "d") {
        return None;
    }
    let arguments = filter.child_by_field_name("arguments")?;
    Some(syntax::text(arguments.named_child(0)?, text).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("klipper-ls-test-{}", std::process::id()))
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        for (name, content) in files {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        dir
    }

    #[test]
    fn follows_includes_and_reads_macros() {
        let dir = temp_config("includes", &[
            ("printer.cfg", "[include macros/*.cfg]\n[include /home/pi/missing.cfg]\n"),
            (
                "macros/start.cfg",
                "[gcode_macro print_start]\n\
                 description: Heat and level\n\
                 variable_soak: 10\n\
                 gcode:\n\
                 \x20 {% set BED = params.BED|default(100)|int %}\n\
                 \x20 {% if params.SOAK is defined %}{% endif %}\n\
                 \x20 M140 S{params[\"BED\"]}\n",
            ),
            ("printer-20240101.cfg", "[gcode_macro PRINT_START]\ngcode:\n  G28\n"),
        ]);
        let index = build(&dir.join("printer.cfg"), &HashMap::new());

        let defs = index.macros("PRINT_START");
        assert_eq!(defs.len(), 1, "backups are not included");
        let def = &defs[0];
        assert_eq!(def.description.as_deref(), Some("Heat and level"));
        assert_eq!(def.variables, vec![("soak".to_string(), "10".to_string())]);
        assert_eq!(
            def.params,
            vec![
                Param { name: "BED".into(), default: Some("100".into()) },
                Param { name: "SOAK".into(), default: None },
            ]
        );
        assert_eq!(def.range.start.line, 0);
    }

    #[test]
    fn finds_printer_cfg_from_a_gcode_file() {
        let dir = temp_config("gcode", &[
            ("printer_data/config/printer.cfg", "[gcode_macro PRINT_START]\ngcode:\n  G28\n"),
            ("printer_data/gcodes/part.gcode", "PRINT_START\n"),
        ]);
        let gcode = dir.join("printer_data/gcodes/part.gcode");
        let found = printer_cfg_for(&gcode, None).unwrap();
        assert_eq!(found, dir.join("printer_data/config/printer.cfg"));
        let index = build_from(&found, None, &HashMap::new());
        assert_eq!(index.macros("print_start").len(), 1);

        // An explicit folder setting wins.
        let explicit = dir.join("printer_data/config");
        assert_eq!(printer_cfg_for(&gcode, Some(&explicit)), Some(explicit.join("printer.cfg")));
    }

    #[test]
    fn unsaved_buffers_win_over_disk() {
        let dir = temp_config("unsaved", &[("printer.cfg", "[gcode_macro OLD]\ngcode:\n  G28\n")]);
        let path = dir.join("printer.cfg");
        let open = HashMap::from([(normalize(&path), "[gcode_macro NEW]\ngcode:\n  G28\n")]);
        let index = build(&path, &open);
        assert!(index.macros("OLD").is_empty());
        assert_eq!(index.macros("new").len(), 1);
    }
}

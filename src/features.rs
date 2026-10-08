//! Hover and go-to-definition. Both start by asking "what is under the
//! cursor?" and then look it up in the user's macros ([`Index`]) and the
//! reference docs ([`Sources`]).
//!
//! A [`Target`] is plain data (a kind plus a byte range), so the same lookup
//! serves Klipper config, where targets come from the syntax tree
//! ([`target_at`]), and `.gcode` files, where they come from the single line
//! under the cursor ([`crate::gcode::target_at`]).

use std::path::{Path, PathBuf};

use lsp_types::{Location, Position, Range, Url};
use tree_sitter::{Node, Tree};

use crate::index::{Index, MacroDef, resolve_include};
use crate::knowledge::{SEPARATOR, Sources, code_block, framed};
use crate::syntax::{ancestor, field_text, section_header, text};

const MAX_VALUE_LEN: usize = 60;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// `M140`, `PRINT_START`
    Command { name: String },
    /// `BED` in `PRINT_START BED=60`, `S` in `M140 S60`
    Parameter { command: String, name: String },
    /// A macro named in data (`SET_GCODE_VARIABLE MACRO=X`,
    /// `printer["gcode_macro X"]`) or in its own `[gcode_macro X]` header.
    Macro { name: String, is_definition: bool },
    /// `stepper_x` in `[stepper_x]`
    Section { ty: String },
    /// `rotation_distance` under `[stepper_x]`
    Option { ty: String, key: String },
    /// `macros/*.cfg` in `[include macros/*.cfg]`
    Include { pattern: String },
    /// `printer.toolhead.position` and `printer["heater_generic c"].target`:
    /// the object (`toolhead`, `heater_generic c`) and the fields after it up
    /// to the cursor; empty when the cursor is on the object. `*` stands for
    /// a computed subscript (`position[axis]`).
    Status { object: String, path: Vec<String> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub kind: Kind,
    /// Byte range of the text the hover applies to.
    pub start: usize,
    pub end: usize,
}

/// What's under the cursor in a Klipper config file.
pub fn target_at(tree: &Tree, source: &str, offset: usize) -> Option<Target> {
    let node = tree.root_node().descendant_for_byte_range(offset, offset)?;
    let upper = || text(node, source).to_ascii_uppercase();
    let kind = match node.kind() {
        "command" => Kind::Command { name: upper() },
        "parameter_name" => {
            let line = ancestor(node, "gcode_line")?;
            let command = field_text(line, "command", source)?.to_ascii_uppercase();
            Kind::Parameter { command, name: upper() }
        }
        "word" => {
            let parameter = node.parent().filter(|p| p.kind() == "parameter")?;
            let name = field_text(parameter, "name", source)?;
            if !name.eq_ignore_ascii_case("MACRO") {
                return None;
            }
            Kind::Macro { name: upper(), is_definition: false }
        }
        "string" => {
            let inner = unquote(text(node, source))?;
            match inner.strip_prefix("gcode_macro ") {
                Some(name) => Kind::Macro { name: name.trim().to_ascii_uppercase(), is_definition: false },
                None => status_kind(node.parent().filter(|p| p.kind() == "subscript")?, source)?,
            }
        }
        "property_identifier" => status_kind(node.parent()?, source)?,
        "key" => {
            let section = ancestor(node, "section")?;
            let (ty, _) = section_header(section, source)?;
            Kind::Option {
                ty: ty.to_ascii_lowercase(),
                key: text(node, source).to_ascii_lowercase(),
            }
        }
        "section_type" => Kind::Section { ty: text(node, source).to_ascii_lowercase() },
        "section_name" => {
            let header = node.parent()?;
            let ty = field_text(header, "type", source)?.to_ascii_lowercase();
            match ty.as_str() {
                "include" => Kind::Include { pattern: text(node, source).to_string() },
                "gcode_macro" => Kind::Macro { name: upper(), is_definition: true },
                _ => Kind::Section { ty },
            }
        }
        _ => return None,
    };
    Some(Target { kind, start: node.start_byte(), end: node.end_byte() })
}

fn unquote(quoted: &str) -> Option<&str> {
    quoted.get(1..quoted.len().saturating_sub(1))
}

/// `printer.a["b c"].d` read back from the `attribute` / `subscript` node
/// whose last segment is under the cursor.
fn status_kind(node: Node, source: &str) -> Option<Kind> {
    let chain = status_chain(node, source)?;
    let (root, rest) = chain.split_first()?;
    let (object, path) = rest.split_first()?;
    (root == "printer" && object != "*").then(|| Kind::Status { object: object.clone(), path: path.to_vec() })
}

fn status_chain(node: Node, source: &str) -> Option<Vec<String>> {
    let child = |field| node.child_by_field_name(field);
    match node.kind() {
        "identifier" => Some(vec![text(node, source).to_string()]),
        "attribute" => {
            let mut chain = status_chain(child("object")?, source)?;
            chain.push(text(child("attribute")?, source).to_string());
            Some(chain)
        }
        "subscript" => {
            let mut chain = status_chain(child("value")?, source)?;
            let key = child("subscript")?;
            let segment = match key.kind() {
                "string" => unquote(text(key, source))?.to_string(),
                _ => "*".to_string(),
            };
            chain.push(segment);
            Some(chain)
        }
        _ => None,
    }
}

pub struct Context<'a> {
    /// The file being edited, for resolving relative `[include]`s.
    pub path: &'a Path,
    pub index: &'a Index,
    pub sources: Sources<'a>,
}

// ---------------------------------------------------------------------------
// Hover
// ---------------------------------------------------------------------------

pub fn hover_markdown(ctx: &Context, target: &Target) -> Option<String> {
    let klipper = ctx.sources.klipper;
    let parts: Vec<String> = match &target.kind {
        Kind::Command { name } => {
            let macros = ctx.index.macros(name);
            let mut parts: Vec<String> = macros.iter().map(|m| render_macro(m, &ctx.index.root)).collect();
            parts.extend(ctx.sources.command(name, !macros.is_empty()));
            parts
        }
        Kind::Parameter { command, name } => {
            let macros = ctx.index.macros(command);
            if macros.is_empty() {
                ctx.sources.parameter(command, name)
            } else {
                macros.iter().map(|m| render_parameter(m, name, &ctx.index.root)).collect()
            }
        }
        Kind::Macro { name, .. } => ctx
            .index
            .macros(name)
            .iter()
            .map(|m| render_macro(m, &ctx.index.root))
            .collect(),
        Kind::Section { ty } => klipper
            .and_then(|d| d.section(ty))
            .map(|e| e.markdown.clone())
            .into_iter()
            .collect(),
        Kind::Option { ty, key } => klipper
            .map(|d| d.option(ty, key).iter().map(|o| o.markdown()).collect())
            .unwrap_or_default(),
        Kind::Include { pattern } => {
            let files = resolve_include(ctx.path, pattern);
            let base = ctx.path.parent().unwrap_or(Path::new(""));
            let list = files
                .iter()
                .map(|f| format!("- `{}`", f.strip_prefix(base).unwrap_or(f).display()))
                .collect::<Vec<_>>()
                .join("\n");
            let body = match files.len() {
                0 => "Matches no files here (paths like `/home/pi/...` only exist on the printer).".to_string(),
                _ => list,
            };
            vec![framed(code_block(Some("Klipper"), &format!("[include {pattern}]")), &body)]
        }
        Kind::Status { object, path } => status_hover(ctx, object, path),
    };
    (!parts.is_empty()).then(|| parts.join(SEPARATOR))
}

/// `printer.toolhead.position`, `printer["heater_generic c"].target`
fn status_display(object: &str, path: &[String]) -> String {
    let mut shown = String::from("printer");
    for segment in std::iter::once(object).chain(path.iter().map(String::as_str)) {
        match segment {
            "*" => shown.push_str("[…]"),
            s if s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => shown.push_str(&format!(".{s}")),
            s => shown.push_str(&format!("[\"{s}\"]")),
        }
    }
    shown
}

/// The macro behind `printer["gcode_macro X"]`, if the object is one.
fn status_macro<'a>(ctx: &'a Context, object: &str) -> Option<&'a [MacroDef]> {
    let (ty, name) = object.split_once(' ')?;
    ty.eq_ignore_ascii_case("gcode_macro")
        .then(|| ctx.index.macros(&name.trim().to_ascii_uppercase()))
        .filter(|m| !m.is_empty())
}

/// `printer.configfile.settings.<section>[.<option>]` -> (section type, option)
fn configfile_setting<'p>(object: &str, path: &'p [String]) -> Option<(String, Option<&'p str>)> {
    if !object.eq_ignore_ascii_case("configfile") {
        return None;
    }
    match path {
        [kind, section, rest @ ..] if matches!(kind.as_str(), "settings" | "config") && rest.len() <= 1 => {
            let ty = section.split_whitespace().next()?.to_ascii_lowercase();
            Some((ty, rest.first().map(String::as_str)))
        }
        _ => None,
    }
}

/// User macro variables first (for `gcode_macro` objects), then
/// Status_Reference.md, then for `configfile.settings.x.y` the config docs
/// of the setting itself.
fn status_hover(ctx: &Context, object: &str, path: &[String]) -> Vec<String> {
    let shown = status_display(object, path);
    let mut parts = Vec::new();
    let macros = status_macro(ctx, object);
    if let (Some(macros), [variable]) = (macros, path) {
        for m in macros {
            let body = match m.variables.iter().find(|(name, _)| name.eq_ignore_ascii_case(variable)) {
                Some((_, value)) => format!("Variable of `{}`. Initial value: `{}`", m.name, shorten(value)),
                None => format!("`{}` defines no `variable_{variable}`.", m.name),
            };
            let md = format!(
                "{}{SEPARATOR}{body}{SEPARATOR}{}",
                code_block(None, &shown),
                location_note(m, &ctx.index.root)
            );
            parts.push(md);
        }
    }
    let Some(klipper) = ctx.sources.klipper else { return parts };
    if let Some(status) = klipper.status().filter(|_| macros.is_none()) {
        let sections = status.sections(object);
        if path.is_empty() {
            for section in sections {
                let fields = section.field_names().iter().map(|f| format!("`{f}`")).collect::<Vec<_>>().join(", ");
                let body = format!("{}\n\n**Fields:** {fields}", section.intro);
                parts.push(framed(code_block(None, &shown), &body));
            }
        } else {
            let fields = status.fields(object, path);
            for (_, field) in &fields {
                parts.push(framed(code_block(None, &shown), &field.markdown));
            }
            if let Some(k) = status.undocumented(object, path).filter(|_| !sections.is_empty()) {
                let field = &path[k];
                let under = match k {
                    0 => String::new(),
                    _ => format!(" under `{}`", path[..k].join(".")),
                };
                let headings = sections.iter().map(|s| format!("`{}`", s.heading)).collect::<Vec<_>>().join(", ");
                let body = format!("Status_Reference.md documents no field `{field}`{under} for {headings}.");
                parts.push(framed(code_block(None, &shown), &body));
            }
        }
    }
    match configfile_setting(object, path) {
        Some((ty, None)) => parts.extend(klipper.section(&ty).map(|e| e.markdown.clone())),
        Some((ty, Some(key))) => parts.extend(klipper.option(&ty, key).iter().map(|o| o.markdown())),
        None => {}
    }
    parts
}

fn shorten(value: &str) -> String {
    if value.chars().count() > MAX_VALUE_LEN {
        let cut: String = value.chars().take(MAX_VALUE_LEN - 1).collect();
        format!("{cut}…")
    } else {
        value.to_string()
    }
}

fn location_note(def: &MacroDef, root: &Path) -> String {
    let shown = def.path.strip_prefix(root).unwrap_or(&def.path);
    format!("*{}:{}*", shown.display(), def.range.start.line + 1)
}

/// Header: the `[gcode_macro NAME]` line, as in the editor. Body:
/// description, override note, parameters, variables. Footer: where it's
/// defined.
fn render_macro(def: &MacroDef, root: &Path) -> String {
    let mut sections: Vec<String> = Vec::new();
    if let Some(description) = &def.description {
        sections.push(description.clone());
    }
    if let Some(renamed) = &def.rename_existing {
        sections.push(format!("Overrides a built-in command, which stays available as `{renamed}`."));
    }
    if !def.params.is_empty() {
        let items = def.params.iter().map(|p| match &p.default {
            Some(d) => format!("- `{}` (default `{}`)", p.name, shorten(d)),
            None => format!("- `{}`", p.name),
        });
        sections.push(format!("**Parameters**\n{}", items.collect::<Vec<_>>().join("\n")));
    }
    if !def.variables.is_empty() {
        let items = def.variables.iter().map(|(name, value)| format!("- `{name}` = `{}`", shorten(value)));
        sections.push(format!("**Variables**\n{}", items.collect::<Vec<_>>().join("\n")));
    }
    let mut md = code_block(Some("Klipper"), &format!("[gcode_macro {}]", def.name));
    if !sections.is_empty() {
        md.push_str(SEPARATOR);
        md.push_str(&sections.join("\n\n"));
    }
    md.push_str(SEPARATOR);
    md.push_str(&location_note(def, root));
    md
}

/// Header: the macro call with this parameter. Body: description or a
/// warning. Footer: where the macro is defined.
fn render_parameter(def: &MacroDef, name: &str, root: &Path) -> String {
    let param = def.params.iter().find(|p| p.name == name);
    let usage = match param.and_then(|p| p.default.as_deref()) {
        Some(d) => format!("{} {name}={}", def.name, shorten(d)),
        None => format!("{} {name}=", def.name),
    };
    let mut body: Vec<String> = Vec::new();
    match param {
        Some(p) if p.default.is_some() => body.push("Default shown; used when the call omits it.".to_string()),
        Some(_) => {}
        None => body.push("The macro never reads this parameter.".to_string()),
    }
    if let Some(description) = &def.description {
        body.push(description.clone());
    }
    let mut md = code_block(None, &usage);
    if !body.is_empty() {
        md.push_str(SEPARATOR);
        md.push_str(&body.join("\n\n"));
    }
    md.push_str(SEPARATOR);
    md.push_str(&location_note(def, root));
    md
}

// ---------------------------------------------------------------------------
// Go to definition
// ---------------------------------------------------------------------------

fn file_location(path: &Path, line: u32) -> Option<Location> {
    let position = Position::new(line, 0);
    Some(Location::new(Url::from_file_path(path).ok()?, Range::new(position, position)))
}

fn macro_locations(defs: &[MacroDef]) -> Vec<Location> {
    defs.iter()
        .filter_map(|d| Some(Location::new(Url::from_file_path(&d.path).ok()?, d.range)))
        .collect()
}

/// Macros jump to their `[gcode_macro]`. Built-in commands, sections and
/// options jump into the reference docs (Klipper's, else Marlin's).
pub fn definition(ctx: &Context, target: &Target) -> Vec<Location> {
    let klipper = ctx.sources.klipper;
    match &target.kind {
        Kind::Command { name } | Kind::Parameter { command: name, .. } => {
            let macros = ctx.index.macros(name);
            if !macros.is_empty() {
                return macro_locations(macros);
            }
            ctx.sources
                .command_location(name)
                .and_then(|(path, line)| file_location(&path, line))
                .into_iter()
                .collect()
        }
        // On the header itself there's nowhere to go.
        Kind::Macro { is_definition: true, .. } => Vec::new(),
        Kind::Macro { name, .. } => macro_locations(ctx.index.macros(name)),
        Kind::Section { ty } => klipper
            .and_then(|d| d.section(ty))
            .and_then(|e| file_location(&e.path, e.line))
            .into_iter()
            .collect(),
        Kind::Option { ty, key } => klipper
            .map(|d| {
                d.option(ty, key)
                    .iter()
                    .filter_map(|o| file_location(&o.path, o.line))
                    .collect()
            })
            .unwrap_or_default(),
        Kind::Include { pattern } => resolve_include(ctx.path, pattern)
            .iter()
            .filter_map(|p: &PathBuf| file_location(p, 0))
            .collect(),
        Kind::Status { object, path } => {
            if let Some(macros) = status_macro(ctx, object) {
                return macro_locations(macros);
            }
            let Some(klipper) = klipper else { return Vec::new() };
            let config = match configfile_setting(object, path) {
                Some((ty, None)) => klipper.section(&ty).and_then(|e| file_location(&e.path, e.line)).into_iter().collect(),
                Some((ty, Some(key))) => klipper.option(&ty, key).iter().filter_map(|o| file_location(&o.path, o.line)).collect(),
                None => Vec::new(),
            };
            if !config.is_empty() {
                return config;
            }
            let Some(status) = klipper.status() else { return Vec::new() };
            if path.is_empty() {
                status.sections(object).iter().filter_map(|s| file_location(&s.path, s.line)).collect()
            } else {
                status.fields(object, path).iter().filter_map(|(s, f)| file_location(&s.path, f.line)).collect()
            }
        }
    }
}

#[cfg(test)]
pub fn describe(kind: &Kind) -> String {
    match kind {
        Kind::Command { name } => format!("command {name}"),
        Kind::Parameter { command, name } => format!("param {command}.{name}"),
        Kind::Macro { name, .. } => format!("macro {name}"),
        Kind::Section { ty } => format!("section {ty}"),
        Kind::Option { ty, key } => format!("option {ty}.{key}"),
        Kind::Include { pattern } => format!("include {pattern}"),
        Kind::Status { object, path } => format!("status {}", status_display(object, path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::parse;

    fn target_kind(source: &str, needle: &str) -> Option<String> {
        let tree = parse(source);
        let offset = source.find(needle).unwrap() + 1;
        target_at(&tree, source, offset).map(|t| describe(&t.kind))
    }

    const SOURCE: &str = "\
[include macros/*.cfg]
[stepper_x]
rotation_distance: 40
[gcode_macro print_start]
gcode:
  m140 S{params.BED}
  _MARK MSG=hi
  SET_GCODE_VARIABLE MACRO=other VARIABLE=x VALUE=1
  {% set v = printer[\"gcode_macro other\"].x %}
  {% set t = printer['heater_generic chamber'].target + printer.toolhead.axis_maximum.x %}
  {% set p = printer.toolhead.position[axis].real + printer[name].temperature + params.Z %}
";

    #[test]
    fn finds_targets() {
        let k = |needle| target_kind(SOURCE, needle);
        assert_eq!(k("macros/").as_deref(), Some("include macros/*.cfg"));
        assert_eq!(k("tepper_x").as_deref(), Some("section stepper_x"));
        assert_eq!(k("rotation").as_deref(), Some("option stepper_x.rotation_distance"));
        assert_eq!(k("rint_start").as_deref(), Some("macro PRINT_START"));
        assert_eq!(k("m140").as_deref(), Some("command M140"));
        assert_eq!(k("S{params").as_deref(), None, "S is one char; +1 lands on '{{'");
        assert_eq!(k("MSG").as_deref(), Some("param _MARK.MSG"));
        assert_eq!(k("other VAR").as_deref(), Some("macro OTHER"));
        assert_eq!(k("\"gcode_macro").as_deref(), Some("macro OTHER"));
        assert_eq!(k("{params").as_deref(), None);
        assert_eq!(k("other\"].x").as_deref(), Some("macro OTHER"));
        assert_eq!(k(".x %}").as_deref(), Some("status printer[\"gcode_macro other\"].x"));
        assert_eq!(k("'heater").as_deref(), Some("status printer[\"heater_generic chamber\"]"));
        assert_eq!(k("target +").as_deref(), Some("status printer[\"heater_generic chamber\"].target"));
        assert_eq!(k("toolhead.axis").as_deref(), Some("status printer.toolhead"));
        assert_eq!(k("s_maximum").as_deref(), Some("status printer.toolhead.axis_maximum"));
        assert_eq!(k(".x %}\n  {% set p").as_deref(), Some("status printer.toolhead.axis_maximum.x"));
        assert_eq!(k("real").as_deref(), Some("status printer.toolhead.position[…].real"));
        assert_eq!(k("temperature +").as_deref(), None, "computed object");
        assert_eq!(k(".Z %}").as_deref(), None, "not printer.*");
    }
}

#[cfg(test)]
mod real_config {
    use std::collections::{BTreeMap, HashMap};
    use std::path::Path;

    use super::*;
    use crate::knowledge::KlipperDocs;
    use crate::{index, syntax::parse};

    /// Which commands in a real config get no hover?
    /// `KLIPPER_CONFIG=.../printer.cfg KLIPPER_DOCS=.../docs cargo test coverage -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn coverage() {
        let printer = std::env::var("KLIPPER_CONFIG").expect("set KLIPPER_CONFIG");
        let docs = KlipperDocs::load(Path::new(&std::env::var("KLIPPER_DOCS").unwrap())).unwrap();
        let sources = Sources { klipper: Some(&docs), marlin: None };
        let printer = Path::new(&printer);
        let index = index::build(printer, &HashMap::new());

        let mut files = vec![printer.to_path_buf()];
        let source = std::fs::read_to_string(printer).unwrap();
        let tree = parse(&source);
        let mut cursor = tree.root_node().walk();
        for section in tree.root_node().named_children(&mut cursor) {
            if let Some(("include", Some(pattern))) = crate::syntax::section_header(section, &source) {
                files.extend(index::resolve_include(printer, pattern));
            }
        }
        files.sort();
        files.dedup();

        let mut missing: BTreeMap<String, usize> = BTreeMap::new();
        let mut total = 0;
        let mut keys_missing: BTreeMap<String, usize> = BTreeMap::new();
        let mut keys_total = 0;
        let mut status_missing: BTreeMap<String, usize> = BTreeMap::new();
        let mut status_total = 0;
        for file in &files {
            let source = std::fs::read_to_string(file).unwrap();
            let tree = parse(&source);
            let ctx = Context { path: file, index: &index, sources };
            let mut stack = vec![tree.root_node()];
            while let Some(node) = stack.pop() {
                if matches!(node.kind(), "key" | "command") {
                    let target = target_at(&tree, &source, node.start_byte());
                    if let Some(target) = target.filter(|t| hover_markdown(&ctx, t).is_none()) {
                        match target.kind {
                            Kind::Option { ty, key } => *keys_missing.entry(format!("[{ty}] {key}")).or_default() += 1,
                            Kind::Command { name } => *missing.entry(name).or_default() += 1,
                            _ => {}
                        }
                    }
                    if node.kind() == "key" { keys_total += 1 } else { total += 1 }
                }
                if matches!(node.kind(), "property_identifier" | "string")
                    && let Some(target) = target_at(&tree, &source, node.start_byte())
                    && let Kind::Status { .. } = &target.kind
                {
                    status_total += 1;
                    let hover = hover_markdown(&ctx, &target).unwrap_or_default();
                    if hover.is_empty() || hover.contains("documents no field") {
                        let Kind::Status { object, path } = &target.kind else { unreachable!() };
                        *status_missing.entry(status_display(object, path)).or_default() += 1;
                    }
                }
                let mut cursor = node.walk();
                stack.extend(node.named_children(&mut cursor));
            }
        }
        let missed: usize = missing.values().sum();
        println!("{} of {total} command uses have hover; missing: {missing:?}", total - missed);
        let keys_missed: usize = keys_missing.values().sum();
        println!("{} of {keys_total} config keys have hover; missing:", keys_total - keys_missed);
        for key in keys_missing.keys() {
            println!("  {key}");
        }
        let status_missed: usize = status_missing.values().sum();
        println!("{} of {status_total} printer.* references have docs; missing:", status_total - status_missed);
        for (reference, count) in &status_missing {
            println!("  {reference} ({count})");
        }
    }
}

//! Klipper's own reference docs, parsed for hover text.
//!
//! The source of truth is the `docs/` folder of a Klipper checkout:
//! `G-Codes.md` for commands, `Config_Reference.md` for sections and
//! options, and `Status_Reference.md` for `printer.*` fields ([`super::status`]). A local checkout is preferred because it matches the Klipper
//! version actually running; otherwise the files are downloaded once from
//! GitHub and cached.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::status::{self, StatusDocs};
use super::{SEPARATOR, code_block};

const GCODES: &str = "G-Codes.md";
const CONFIG_REFERENCE: &str = "Config_Reference.md";
const UPSTREAM: &str = "https://raw.githubusercontent.com/Klipper3d/klipper/master/docs";

#[derive(Debug, Clone)]
pub struct DocEntry {
    /// Hover text, already formatted as markdown.
    pub markdown: String,
    pub path: PathBuf,
    /// Zero-based line of the entry's heading, for go-to-definition.
    pub line: u32,
    /// Parameters named in the command's syntax line (`M109 [T<index>]
    /// S<temperature>` -> T, S). `None` when the docs give no syntax, so
    /// "Klipper ignores this parameter" is only claimed when we know.
    pub params: Option<Vec<String>>,
    /// The syntax line(s) as written in the docs, for the hover's code header.
    pub signature: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OptionDoc {
    /// As written in the reference, e.g. `variable_<name>`.
    pub name: String,
    pub section: String,
    pub heading: String,
    /// `Some` for optional settings (`#name: default`), `None` for required ones.
    pub default: Option<String>,
    pub description: String,
    pub path: PathBuf,
    pub line: u32,
}

impl OptionDoc {
    fn matches(&self, key: &str) -> bool {
        match self.name.split_once('<') {
            // variable_<name> matches any variable_xxx
            Some((prefix, rest)) => {
                let suffix = rest.split_once('>').map_or("", |(_, s)| s);
                key.starts_with(prefix) && key.ends_with(suffix) && key.len() > prefix.len()
            }
            None => self.name == key,
        }
    }

    /// `See the "extruder" section ...` points at where the real text lives.
    fn see_also(&self) -> Option<&str> {
        let rest = self.description.split_once("See the \"")?.1;
        let (section, tail) = rest.split_once('"')?;
        tail.trim_start().starts_with("section").then_some(section)
    }

    /// Header: the option as it sits in its section. Body: where it
    /// comes from when that isn't the obvious section, whether it's
    /// required, and the description.
    pub fn markdown(&self) -> String {
        let line = match &self.default {
            Some(d) if !d.is_empty() => format!("{}: {d}", self.name),
            _ => format!("{}:", self.name),
        };
        let mut body: Vec<String> = Vec::new();
        // `[stepper]` is just the docs' name for the section in the header.
        if !self.heading.starts_with('[') {
            body.push(format!("*{}*", self.heading));
        }
        if self.default.is_none() {
            body.push("Required.".to_string());
        }
        if !self.description.is_empty() {
            body.push(self.description.clone());
        }
        framed(code_block(Some("Klipper"), &format!("[{}]\n{line}", self.section)), &body.join("\n\n"))
    }
}

/// A code header, then (when there is one) the body, as `rust-analyzer`
/// lays out its hovers. Relative doc links are made absolute.
pub fn framed(header: String, body: &str) -> String {
    match body.trim() {
        "" => header,
        body => absolute_links(&format!("{header}{SEPARATOR}{body}")),
    }
}

/// What Klipper does with a command, as far as we can tell.
#[derive(Debug, Clone, PartialEq)]
pub enum Support {
    /// Has an entry in G-Codes.md.
    Documented,
    /// No docs entry, but Klipper's source registers a handler (G21, M110).
    SourceOnly { path: PathBuf, line: u32 },
    /// Neither. `source_checked` says whether we had Klipper's source to
    /// look in; without it, "unsupported" is only a likely guess.
    Unknown { source_checked: bool },
}

#[derive(Debug, Default)]
pub struct Docs {
    commands: HashMap<String, DocEntry>,
    sections: HashMap<String, DocEntry>,
    options: HashMap<String, Vec<OptionDoc>>,
    /// Sections whose intro says they share another section's settings,
    /// by naming it in quotes (heater_bed pointing at "extruder").
    inherits: HashMap<String, String>,
    /// Command handlers found in Klipper's source (`klippy/`), when the docs
    /// come from a full checkout: name -> (file, line).
    handlers: Option<HashMap<String, (PathBuf, u32)>>,
    /// Pin chips (`register_chip('probe', ...)`) and printer objects
    /// (`add_object('toolhead', ...)`) that the source registers under a
    /// literal name, when it was scanned. Names built at runtime
    /// (`"%s_%s" % ...`) aren't here; the config's own sections supply those.
    source_names: Option<SourceNames>,
    /// Optional: older downloaded caches don't have it.
    status: Option<StatusDocs>,
}

impl Docs {
    pub fn load(dir: &Path) -> std::io::Result<Self> {
        let mut docs = Docs::default();
        let gcodes = dir.join(GCODES);
        let reference = dir.join(CONFIG_REFERENCE);
        docs.parse_gcodes(&fs::read_to_string(&gcodes)?, &gcodes);
        docs.parse_config_reference(&fs::read_to_string(&reference)?, &reference);
        let klippy = dir.parent().map(|checkout| checkout.join("klippy"));
        if let Some(scan) = klippy.filter(|k| k.is_dir()).map(|k| scan_source(&k)) {
            docs.handlers = Some(scan.handlers);
            docs.source_names = Some(scan.names);
        }
        let status = dir.join(status::FILE);
        docs.status = fs::read_to_string(&status).ok().map(|text| StatusDocs::parse(&text, &status));
        Ok(docs)
    }

    pub fn support(&self, name: &str) -> Support {
        let name = name.to_ascii_uppercase();
        if self.commands.contains_key(&name) {
            return Support::Documented;
        }
        match &self.handlers {
            Some(handlers) => match handlers.get(&name) {
                Some((path, line)) => Support::SourceOnly { path: path.clone(), line: *line },
                None => Support::Unknown { source_checked: true },
            },
            None => Support::Unknown { source_checked: false },
        }
    }

    /// Whether Klipper's source was scanned, which is what lets us say
    /// "Klipper doesn't know this" instead of "the docs don't mention it".
    pub fn source_scanned(&self) -> bool {
        self.handlers.is_some()
    }

    /// Whether Klipper's source has a module that loads sections of type
    /// `ty`. None without the source.
    pub fn source_module(&self, ty: &str) -> Option<bool> {
        self.source_names.as_ref().map(|n| n.modules.contains(ty))
    }

    /// Chip names the source registers under a literal name.
    pub fn source_chips(&self) -> impl Iterator<Item = &str> {
        self.source_names.iter().flat_map(|n| n.chips.iter().map(String::as_str))
    }

    /// Printer objects the source registers under a literal name.
    pub fn source_objects(&self) -> impl Iterator<Item = &str> {
        self.source_names.iter().flat_map(|n| n.objects.iter().map(String::as_str))
    }

    #[cfg(test)]
    pub fn with_source_names(mut self, chips: &[&str], objects: &[&str]) -> Self {
        self.source_names = Some(SourceNames {
            chips: chips.iter().map(|n| n.to_string()).collect(),
            objects: objects.iter().map(|n| n.to_string()).collect(),
            modules: HashSet::new(),
        });
        self
    }

    #[cfg(test)]
    pub fn with_status(mut self, text: &str) -> Self {
        self.status = Some(StatusDocs::parse(text, Path::new(status::FILE)));
        self
    }

    #[cfg(test)]
    pub fn with_handlers(mut self, names: &[&str]) -> Self {
        self.handlers = Some(names.iter().map(|n| (n.to_string(), (PathBuf::from("gcode_move.py"), 165))).collect());
        self
    }

    #[cfg(test)]
    pub fn from_strings(gcodes: &str, reference: &str, path: &Path) -> Self {
        let mut docs = Docs::default();
        docs.parse_gcodes(gcodes, path);
        docs.parse_config_reference(reference, path);
        docs
    }

    pub fn status(&self) -> Option<&StatusDocs> {
        self.status.as_ref()
    }

    pub fn command(&self, name: &str) -> Option<&DocEntry> {
        self.commands.get(&name.to_ascii_uppercase())
    }

    pub fn section(&self, ty: &str) -> Option<&DocEntry> {
        section_candidates(ty).find_map(|t| self.sections.get(&t))
    }

    /// Docs for `key` in a section of type `ty`, following "See the X
    /// section" references so `[heater_bed] control` explains itself.
    pub fn option(&self, ty: &str, key: &str) -> Vec<&OptionDoc> {
        let key = key.to_ascii_lowercase();
        let lookup = |ty: &str| {
            section_candidates(ty).find_map(|t| {
                let matches: Vec<_> = self.options.get(&t)?.iter().filter(|o| o.matches(&key)).collect();
                (!matches.is_empty()).then_some(matches)
            })
        };
        let inherited = || {
            let parent = section_candidates(ty).find_map(|t| self.inherits.get(&t))?;
            lookup(parent)
        };
        let Some(found) = lookup(ty).or_else(inherited) else {
            return Vec::new();
        };

        // Kinematics variants repeat options; keep the ones that say something,
        // once each.
        let mut result: Vec<&OptionDoc> = Vec::new();
        for option in &found {
            let duplicate = result.iter().any(|r| r.description == option.description);
            if !option.description.is_empty() && !duplicate {
                result.push(option);
            }
        }
        if result.is_empty() {
            result.extend(found.first());
        }
        for option in found {
            if let Some(other) = option.see_also()
                && other != option.section {
                    result.extend(
                        self.options
                            .get(other)
                            .into_iter()
                            .flatten()
                            .filter(|o| o.matches(&key)),
                    );
                }
        }
        result
    }

    // -----------------------------------------------------------------------
    // G-Codes.md
    // -----------------------------------------------------------------------

    fn parse_gcodes(&mut self, text: &str, path: &Path) {
        let lines: Vec<&str> = text.lines().collect();
        // `### [bed_mesh]`: the config section a command needs
        let mut provider: Option<&str> = None;
        let mut in_standard_list = false;
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            if let Some(heading) = line.strip_prefix("## ") {
                in_standard_list = heading.trim() == "G-Code commands";
                provider = None;
                i += 1;
            } else if let Some(heading) = line.strip_prefix("### ") {
                in_standard_list = false;
                provider = Some(heading.trim());
                i += 1;
            } else if let Some(heading) = line.strip_prefix("#### ") {
                // Some headings wrap the name in backticks: #### `EXCLUDE_OBJECT_START`
                let heading = heading.replace('`', "");
                let heading = heading.as_str();
                let start = i;
                let (body, next) = take_until_heading(&lines, i + 1);
                i = next;

                let mut md = format!("**{}**", heading.trim());
                if let Some(p) = provider {
                    md.push_str(&format!(" · needs `{p}`"));
                }
                if !body.is_empty() {
                    md.push_str("\n\n");
                    md.push_str(&body);
                }
                let md = absolute_links(&md);
                for name in command_names(heading) {
                    let params = syntax_params(&code_spans(&body), &name);
                    let signature = signature(&code_spans(&body), &name);
                    self.commands.entry(name).or_insert_with(|| DocEntry {
                        markdown: md.clone(),
                        path: path.to_path_buf(),
                        line: start as u32,
                        params,
                        signature,
                    });
                }
            } else if in_standard_list && line.starts_with("- ") {
                let start = i;
                let (head, notes, next) = take_bullet(&lines, i);
                i = next;
                self.add_standard_gcode(&head, &notes, path, start);
            } else if provider.is_some() && line.starts_with("- `") {
                // Some sections list commands as bullets instead of headings:
                // "- `M118 <message>`: echo the message ..."
                let start = i;
                let (head, notes, next) = take_bullet(&lines, i);
                i = next;
                self.add_bullet_command(&head, &notes, provider, path, start);
            } else if provider.is_some() && line.starts_with("- ") && line.contains(": `") {
                // ... or in the standard-list style:
                // "- Set build percentage: `M73 P<percent>`" under [display_status]
                let start = i;
                let (head, notes, next) = take_bullet(&lines, i);
                i = next;
                self.add_titled_bullet(&head, &notes, provider, path, start);
            } else {
                i += 1;
            }
        }
    }

    /// `- Set bed temperature: \`M140 [S<temperature>]\`` and friends.
    fn add_standard_gcode(&mut self, head: &str, notes: &[String], path: &Path, line: usize) {
        let (title, syntax) = head.split_once(": ").unwrap_or((head, ""));
        for code in standard_codes(head) {
            // "Move (G0 or G1): `G1 ...`": G0 shares G1's syntax.
            let spans = code_spans(head);
            let signature = signature(&spans, &code)
                .or_else(|| standard_codes(head).iter().find_map(|c| signature(&spans, c)));
            let mut md = format!("**{code}** · {title}");
            // A bare syntax span is the hover's code header already.
            let shown_above = signature.as_ref().is_some_and(|sig| syntax.trim() == format!("`{sig}`"));
            if !syntax.is_empty() && !shown_above {
                md.push_str(&format!("\n\n{syntax}"));
            }
            for note in notes {
                md.push_str(&format!("\n\n{note}"));
            }
            md.push_str("\n\nStandard G-code; see the [RepRap G-code docs](https://reprap.org/wiki/G-code).");
            let params = syntax_params(&spans, &code)
                .or_else(|| standard_codes(head).iter().find_map(|c| syntax_params(&spans, c)));
            self.commands.entry(code).or_insert_with(|| DocEntry {
                markdown: md,
                path: path.to_path_buf(),
                line: line as u32,
                params,
                signature,
            });
        }
    }

    fn add_titled_bullet(
        &mut self,
        head: &str,
        notes: &[String],
        provider: Option<&str>,
        path: &Path,
        line: usize,
    ) {
        let Some((title, syntax)) = head.split_once(": ") else { return };
        for span in code_spans(syntax) {
            let Some(name) = command_names(span.split_whitespace().next().unwrap_or("")).pop() else {
                continue;
            };
            let mut md = format!("**{name}** · {title}");
            if let Some(p) = provider {
                md.push_str(&format!(" · needs `{p}`"));
            }
            md.push_str(&format!("\n\n{syntax}"));
            for note in notes {
                md.push_str(&format!("\n\n{note}"));
            }
            let params = syntax_params(&code_spans(syntax), &name);
            let signature = signature(&code_spans(syntax), &name);
            self.commands.entry(name).or_insert_with(|| DocEntry {
                markdown: absolute_links(&md),
                path: path.to_path_buf(),
                line: line as u32,
                params,
                signature,
            });
        }
    }

    fn add_bullet_command(
        &mut self,
        head: &str,
        notes: &[String],
        provider: Option<&str>,
        path: &Path,
        line: usize,
    ) {
        let Some((syntax, rest)) = head.strip_prefix('`').and_then(|h| h.split_once('`')) else {
            return;
        };
        if !rest.starts_with(':') {
            return;
        }
        let Some(name) = command_names(syntax.split_whitespace().next().unwrap_or("")).pop() else {
            return;
        };
        let mut md = format!("**{name}**");
        if let Some(p) = provider {
            md.push_str(&format!(" · needs `{p}`"));
        }
        let params = syntax_params(&code_spans(head), &name);
        let signature = signature(&code_spans(head), &name);
        // The syntax span is the hover's code header; keep only its description.
        let shown_above = signature.as_ref().is_some_and(|sig| sig.lines().any(|l| l == syntax));
        match rest.strip_prefix(':').map(str::trim).filter(|d| shown_above && !d.is_empty()) {
            Some(description) => {
                md.push_str(&format!("\n\n{description}"));
                for note in notes {
                    md.push_str(&format!("\n- {note}"));
                }
            }
            None => {
                md.push_str(&format!("\n\n- {head}"));
                for note in notes {
                    md.push_str(&format!("\n  - {note}"));
                }
            }
        }
        self.commands.entry(name).or_insert_with(|| DocEntry {
            markdown: absolute_links(&md),
            path: path.to_path_buf(),
            line: line as u32,
            params,
            signature,
        });
    }

    // -----------------------------------------------------------------------
    // Config_Reference.md
    // -----------------------------------------------------------------------

    fn parse_config_reference(&mut self, text: &str, path: &Path) {
        let lines: Vec<&str> = text.lines().collect();
        let mut heading = String::new();
        let mut heading_line = 0;
        let mut prose: Vec<&str> = Vec::new();
        let mut before_first_fence = true;
        let mut section_registered = false;
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            if line.starts_with("## ") || line.starts_with("### ") {
                heading = line.trim_start_matches('#').trim().to_string();
                heading_line = i;
                prose.clear();
                before_first_fence = true;
                section_registered = false;
                i += 1;
                continue;
            }
            if line.trim_start().starts_with("```") {
                let mut end = i + 1;
                while end < lines.len() && !lines[end].trim_start().starts_with("```") {
                    end += 1;
                }
                let block = &lines[i + 1..end.min(lines.len())];
                if let Some(ty) = self.parse_reference_block(block, i + 1, &heading, path)
                    && !section_registered && heading.starts_with('[') {
                        section_registered = true;
                        let intro = prose.join("\n").trim().to_string();
                        if let Some(parent) = quoted_section(&intro)
                            && parent != ty {
                                self.inherits.insert(ty.clone(), parent.to_string());
                            }
                        self.sections.entry(ty.clone()).or_insert_with(|| DocEntry {
                            markdown: framed(code_block(Some("Klipper"), &heading), &intro),
                            path: path.to_path_buf(),
                            line: heading_line as u32,
                            params: None,
                            signature: None,
                        });
                    }
                before_first_fence = false;
                i = end + 1;
                continue;
            }
            if before_first_fence {
                prose.push(line);
            }
            i += 1;
        }
    }

    /// Parses one fenced example config. Returns the first section type in it
    /// (`[stepper_x]` -> `stepper_x`), or None for blocks that aren't config
    /// (ASCII diagrams and the like).
    fn parse_reference_block(
        &mut self,
        block: &[&str],
        first_line: usize,
        heading: &str,
        path: &Path,
    ) -> Option<String> {
        let first_section = section_type_of(block.iter().find(|l| !l.trim().is_empty())?)?;
        let mut section = first_section.clone();
        // Options listed back to back share the description that follows them.
        let mut pending: Vec<OptionDoc> = Vec::new();
        let mut description: Vec<&str> = Vec::new();

        let flush = |pending: &mut Vec<OptionDoc>, description: &mut Vec<&str>, docs: &mut Docs| {
            let text = description.join("\n");
            for mut option in pending.drain(..) {
                option.description = text.clone();
                docs.options.entry(option.section.clone()).or_default().push(option);
            }
            description.clear();
        };

        for (offset, line) in block.iter().enumerate() {
            if let Some(ty) = section_type_of(line) {
                flush(&mut pending, &mut description, self);
                section = ty;
            } else if let Some((name, value, optional)) = option_line(line) {
                if !description.is_empty() {
                    flush(&mut pending, &mut description, self);
                }
                let option = OptionDoc {
                    name: name.to_ascii_lowercase(),
                    section: section.clone(),
                    heading: heading.to_string(),
                    default: optional.then(|| value.to_string()),
                    description: String::new(),
                    path: path.to_path_buf(),
                    line: (first_line + offset) as u32,
                };
                // `kinematics: cartesian` in a kinematics example is a fixed
                // value, not an option waiting for the next description.
                if !optional && !value.is_empty() {
                    flush(&mut pending, &mut description, self);
                    self.options.entry(option.section.clone()).or_default().push(option);
                } else {
                    pending.push(option);
                }
            } else if let Some(text) = line.strip_prefix('#') {
                description.push(text.trim());
            }
        }
        flush(&mut pending, &mut description, self);
        Some(first_section)
    }
}

#[derive(Debug, Default)]
struct SourceNames {
    chips: HashSet<String>,
    objects: HashSet<String>,
    /// Section types Klipper can load: `klippy/extras/<type>.py` (or a
    /// package of that name) and the modules directly under `klippy/`.
    modules: HashSet<String>,
}

struct SourceScan {
    handlers: HashMap<String, (PathBuf, u32)>,
    names: SourceNames,
}

/// What Klipper's Python source registers. Commands come either by name
/// (`register_command('M73', ...)`) or from looping over a list and looking
/// up `cmd_<NAME>` methods, so both spellings count.
fn scan_source(klippy: &Path) -> SourceScan {
    let mut handlers = HashMap::new();
    let mut names = SourceNames::default();
    let mut stack = vec![klippy.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "py") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else { continue };
            for (number, line) in text.lines().enumerate() {
                for name in handler_names(line) {
                    handlers.entry(name).or_insert_with(|| (path.clone(), number as u32));
                }
                names.chips.extend(literal_argument(line, "register_chip("));
                names.objects.extend(literal_argument(line, "add_object("));
            }
        }
    }
    for dir in [klippy.to_path_buf(), klippy.join("extras")] {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let stem = path.file_stem().and_then(|s| s.to_str());
            let is_module = path.extension().is_some_and(|e| e == "py") || path.join("__init__.py").is_file();
            if let (Some(stem), true) = (stem, is_module) {
                names.modules.insert(stem.to_string());
            }
        }
    }
    SourceScan { handlers, names }
}

/// The string literal that is the first argument of `call`, when it is the
/// whole argument: `'probe'` yes, `"%s_%s" % x` and `'heater ' + name` no.
fn literal_argument(line: &str, call: &str) -> Option<String> {
    let rest = line.split_once(call)?.1.trim_start();
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let body = &rest[1..];
    let end = body.find(quote)?;
    let literal = &body[..end];
    let after = body[end + 1..].trim_start();
    let whole = after.is_empty() || after.starts_with([',', ')']);
    (whole && !literal.is_empty() && !literal.contains(['%', '{', ' '])).then(|| literal.to_string())
}

fn handler_names(line: &str) -> Vec<String> {
    let is_name = |n: &str| {
        n.starts_with(|c: char| c.is_ascii_uppercase())
            && n.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    };
    let mut names = Vec::new();
    if let Some(rest) = line.trim_start().strip_prefix("def cmd_") {
        let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if is_name(&name) && !name.ends_with("_HELP") {
            names.push(name);
        }
    }
    for marker in ["register_command(", "register_mux_command("] {
        if let Some((_, rest)) = line.split_once(marker) {
            let rest = rest.trim_start().trim_start_matches(['\'', '"']);
            let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if is_name(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// Inline code spans: the text between backtick pairs.
pub(super) fn code_spans(text: &str) -> Vec<&str> {
    text.split('`').skip(1).step_by(2).collect()
}

/// The syntax spans that start with `command`, one per line, for the hover's
/// code header: `M109 [T<index>] S<temperature>`.
fn signature(spans: &[&str], command: &str) -> Option<String> {
    let mut lines: Vec<&str> = Vec::new();
    for span in spans {
        let first = span.split_whitespace().next();
        if first.is_some_and(|w| w.eq_ignore_ascii_case(command)) && !lines.contains(span) {
            lines.push(span);
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Parameter names from the syntax spans that start with `command`:
/// `M109 [T<index>] S<temperature>` -> [T, S];
/// `TEMPERATURE_WAIT SENSOR=<config_name> [MINIMUM=<target>]` -> [SENSOR, MINIMUM];
/// `G28 [X] [Y] [Z]` -> [X, Y, Z].
fn syntax_params(spans: &[&str], command: &str) -> Option<Vec<String>> {
    let mut found_syntax = false;
    let mut params: Vec<String> = Vec::new();
    for span in spans {
        let mut words = span.split_whitespace();
        if !words.next().is_some_and(|w| w.eq_ignore_ascii_case(command)) {
            continue;
        }
        found_syntax = true;
        for word in words {
            let word = word.trim_start_matches(['[', '{', '(']);
            let name: String = word
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            let rest = &word[name.len()..];
            let well_formed = name.starts_with(|c: char| c.is_ascii_uppercase())
                && (rest.is_empty() || rest.starts_with(['<', '=', ']', '}', ')']));
            if well_formed && !params.contains(&name) {
                params.push(name);
            }
        }
    }
    found_syntax.then_some(params)
}

/// The docs link to each other relatively (`(Bed_Mesh.md#calibration)`),
/// which goes nowhere in a hover. Point them at klipper3d.org instead.
pub(super) fn absolute_links(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(start) = rest.find("](") {
        let (before, after) = rest.split_at(start + 2);
        out.push_str(before);
        let end = after.find(')').unwrap_or(after.len());
        let target = &after[..end];
        let (file, anchor) = target.split_once('#').map_or((target, None), |(f, a)| (f, Some(a)));
        let relative_doc = file.ends_with(".md") && !file.contains("://");
        if relative_doc || (file.is_empty() && anchor.is_some()) {
            out.push_str("https://www.klipper3d.org/");
            out.push_str(&file.replace(".md", ".html"));
            if let Some(anchor) = anchor {
                out.push('#');
                out.push_str(anchor);
            }
        } else {
            out.push_str(target);
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// `... described in the "extruder" section` -> `extruder`
fn quoted_section(text: &str) -> Option<&str> {
    let end = text.find("\" section")?;
    let start = text[..end].rfind('"')? + 1;
    let name = &text[start..end];
    name.chars().all(|c| c.is_ascii_lowercase() || c == '_').then_some(name)
}

/// Lines from `start` up to the next markdown heading (ignoring `#` inside
/// code fences), trimmed and joined.
fn take_until_heading(lines: &[&str], start: usize) -> (String, usize) {
    let mut i = start;
    let mut in_fence = false;
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence && line.starts_with('#') {
            break;
        }
        i += 1;
    }
    (lines[start..i].join("\n").trim().to_string(), i)
}

/// A top-level bullet, with wrapped lines joined and `  - Note:` sub-bullets
/// returned separately.
fn take_bullet(lines: &[&str], start: usize) -> (String, Vec<String>, usize) {
    let mut head = lines[start][2..].trim().to_string();
    let mut notes: Vec<String> = Vec::new();
    let mut i = start + 1;
    while i < lines.len() && lines[i].starts_with("  ") {
        let line = lines[i].trim();
        if let Some(note) = line.strip_prefix("- ") {
            notes.push(note.to_string());
        } else if let Some(last) = notes.last_mut() {
            last.push(' ');
            last.push_str(line);
        } else {
            head.push(' ');
            head.push_str(line);
        }
        i += 1;
    }
    (head, notes, i)
}

/// `#### SET_PIN` -> ["SET_PIN"]; tolerates `A / B` style headings.
fn command_names(heading: &str) -> Vec<String> {
    heading
        .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
        .filter(|n| !n.is_empty())
        .filter(|n| n.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
        .map(str::to_string)
        .collect()
}

/// Every G/M code mentioned in a bullet: "Move (G0 or G1): `G1 ...`".
fn standard_codes(head: &str) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    for word in head.split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut chars = word.chars();
        let is_code = matches!(chars.next(), Some('G' | 'M'))
            && word.len() > 1
            && chars.all(|c| c.is_ascii_digit());
        if is_code && !codes.iter().any(|c| c == word) {
            codes.push(word.to_string());
        }
    }
    codes
}

/// `[stepper_x]` / `[mcu my_extra_mcu]` -> the section type.
fn section_type_of(line: &str) -> Option<String> {
    let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
    let ty = inner.split_whitespace().next()?;
    ty.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
        .then(|| ty.to_ascii_lowercase())
}

/// `#name: default` (optional) or `name: example` (required). Names may be
/// written with capitals (`initial_RED`); Klipper lowercases them.
fn option_line(line: &str) -> Option<(&str, &str, bool)> {
    let (optional, rest) = match line.strip_prefix('#') {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    let (name, value) = rest.split_once(':')?;
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '<' | '>'));
    valid.then(|| (name, value.trim(), optional))
}

/// `stepper_z1` has no entry of its own for most options; fall back to
/// `stepper_z`, then `stepper_x` (where the reference documents them),
/// then `stepper`. `extruder1` falls back to `extruder`.
fn section_candidates(ty: &str) -> impl Iterator<Item = String> {
    let ty = ty.to_ascii_lowercase();
    let mut candidates = vec![ty.clone()];
    let base = ty.trim_end_matches(|c: char| c.is_ascii_digit());
    if base != ty && !base.is_empty() {
        candidates.push(base.to_string());
    }
    if let Some((prefix, axis)) = base.rsplit_once('_')
        && axis.len() == 1 {
            candidates.push(format!("{prefix}_x"));
            candidates.push(prefix.to_string());
        }
    candidates.into_iter()
}

// ---------------------------------------------------------------------------
// Finding the docs
// ---------------------------------------------------------------------------

fn has_docs(dir: &Path) -> bool {
    dir.join(GCODES).is_file() && dir.join(CONFIG_REFERENCE).is_file()
}

/// An explicit setting wins; otherwise look for `klipper/docs` (or a Klipper
/// checkout's own `docs`) next to the config or any of its parents, then
/// `~/klipper/docs` (where KIAUH installs it on the printer).
pub fn find_local(explicit: Option<&Path>, start_dirs: &[PathBuf]) -> Option<PathBuf> {
    if let Some(dir) = explicit {
        return has_docs(dir).then(|| dir.to_path_buf());
    }
    for start in start_dirs {
        for dir in start.ancestors() {
            for candidate in [dir.join("klipper/docs"), dir.join("docs")] {
                if has_docs(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    home().map(|h| h.join("klipper/docs")).filter(|d| has_docs(d))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn cache_dir() -> Option<PathBuf> {
    Some(super::cache_root()?.join("docs"))
}

/// Downloads the reference files from Klipper's master branch into `dir`,
/// unless they're already there. Delete the folder to refresh.
///
/// Status_Reference.md is optional: caches made before it was fetched get it
/// on first use, tried once per run so a failure never costs a request per
/// hover.
///
/// This runs on the thread that serves every request, and ureq has no
/// timeouts by default, so each fetch gets one: a stalled network costs one
/// slow hover, not a hung server. The optional file gets a shorter one, since
/// it's paid by caches that already work offline.
pub fn download(dir: &Path) -> Result<PathBuf, String> {
    static TRIED_STATUS: AtomicBool = AtomicBool::new(false);
    if !has_docs(dir) {
        fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        for file in [GCODES, CONFIG_REFERENCE] {
            fetch(dir, file, Duration::from_secs(30))?;
        }
    }
    if !dir.join(status::FILE).is_file()
        && !TRIED_STATUS.swap(true, Ordering::Relaxed)
        && let Err(e) = fetch(dir, status::FILE, Duration::from_secs(5))
    {
        eprintln!("klipper-ls: {e}; no hover for printer.* fields");
    }
    Ok(dir.to_path_buf())
}

fn fetch(dir: &Path, file: &str, timeout: Duration) -> Result<(), String> {
    let url = format!("{UPSTREAM}/{file}");
    let body = ureq::get(&url)
        .config()
        .timeout_global(Some(timeout))
        .build()
        .call()
        .and_then(|mut response| response.body_mut().read_to_string())
        .map_err(|e| format!("downloading {url}: {e}"))?;
    fs::write(dir.join(file), body).map_err(|e| format!("writing {file}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_arguments_are_only_whole_literals() {
        assert_eq!(literal_argument("ppins.register_chip('probe', self)", "register_chip(").as_deref(), Some("probe"));
        assert_eq!(literal_argument("register_chip(\"%s_%s\" % (a, b), self)", "register_chip("), None);
        assert_eq!(literal_argument("add_object('heater ' + name, h)", "add_object("), None);
        assert_eq!(literal_argument("add_object('toolhead', t)", "add_object(").as_deref(), Some("toolhead"));
    }

    const GCODES_MD: &str = "\
# G-Codes

## G-Code commands

The fixture handles these standard commands:
- Linear move (G0 or G1): `G1 [X<pos>] [E<pos>] [F<speed>]`
- Release the steppers: `M18` or `M84`
- Heat the extruder and block: `M109 [T<index>] S<temperature>`
  - Note: returns only once the target is reached

## Additional Commands

### [bed_mesh]

#### BED_MESH_CALIBRATE
`BED_MESH_CALIBRATE [PROFILE=<name>]`: Runs a probing pass over
the bed.

```
# not a heading
```

#### BED_MESH_OUTPUT
`BED_MESH_OUTPUT`: Output the mesh.
";

    const REFERENCE_MD: &str = "\
## Common kinematic settings

### [stepper]

Stepper motor definitions.

```
[stepper_x]
step_pin:
#   Pin that steps the motor.
#full_steps_per_rotation: 200
#   Full steps per turn.
```

### Cartesian Kinematics

```
[printer]
kinematics: cartesian
max_z_velocity:
#   Speed ceiling for the Z axis.
```

### [extruder]

```
[extruder]
control:
pid_Kp:
#   Heater control.
#max_power: 1.0
#   The maximum power.
```

### [heater_bed]

Heated bed. Shares its heater options with the \"extruder\" section.

```
[heater_bed]
heater_pin:
control:
#   See the \"extruder\" section.
```

### [gcode_macro]

```
[gcode_macro my_cmd]
#variable_<name>:
#   Any option may start with \"variable_\"; each becomes a macro variable.
```
";

    fn docs() -> Docs {
        let mut docs = Docs::default();
        docs.parse_gcodes(GCODES_MD, Path::new("G-Codes.md"));
        docs.parse_config_reference(REFERENCE_MD, Path::new("Config_Reference.md"));
        docs
    }

    #[test]
    fn relative_links_become_absolute() {
        assert_eq!(
            absolute_links("See [guide](Bed_Mesh.md#calibration) and [x](https://a.b/c.md)."),
            "See [guide](https://www.klipper3d.org/Bed_Mesh.html#calibration) and [x](https://a.b/c.md)."
        );
    }

    #[test]
    fn syntax_lines_give_parameter_names() {
        let docs = docs();
        assert_eq!(docs.command("G1").unwrap().params.as_deref(), Some(&["X", "E", "F"].map(String::from)[..]));
        assert_eq!(docs.command("G0").unwrap().params, docs.command("G1").unwrap().params, "G0 shares G1's syntax");
        assert_eq!(docs.command("M109").unwrap().params.as_deref(), Some(&["T", "S"].map(String::from)[..]));
        assert_eq!(docs.command("M84").unwrap().params.as_deref(), Some(&[][..]));
        assert_eq!(
            docs.command("BED_MESH_CALIBRATE").unwrap().params.as_deref(),
            Some(&["PROFILE"].map(String::from)[..])
        );
    }

    #[test]
    fn finds_handler_names_in_python() {
        assert_eq!(handler_names("    def cmd_G21(self, gcmd):"), vec!["G21"]);
        assert_eq!(handler_names("        gcode.register_command('M73', self.cmd_M73)"), vec!["M73"]);
        assert_eq!(handler_names("    def cmd_default(self, gcmd):"), Vec::<String>::new());
        assert_eq!(handler_names("    def cmd_SET_PIN(self, gcmd):"), vec!["SET_PIN"]);
    }

    #[test]
    fn backticked_headings_and_titled_bullets() {
        let docs = Docs::from_strings(
            "### [display_status]\n\n- Set build percentage: `M73 P<percent>`\n\n\
             ### [exclude_object]\n\n#### `EXCLUDE_OBJECT_START`\n`EXCLUDE_OBJECT_START NAME=object_name`: Starts.\n",
            "",
            Path::new("G-Codes.md"),
        );
        let m73 = docs.command("M73").unwrap();
        assert!(m73.markdown.contains("Set build percentage") && m73.markdown.contains("needs `[display_status]`"));
        assert_eq!(m73.params.as_deref(), Some(&["P".to_string()][..]));
        let start = docs.command("EXCLUDE_OBJECT_START").unwrap();
        assert_eq!(start.params.as_deref(), Some(&["NAME".to_string()][..]));
    }

    #[test]
    fn standard_gcodes() {
        let docs = docs();
        let g0 = docs.command("g0").unwrap();
        assert!(g0.markdown.contains("Linear move (G0 or G1)"));
        assert!(docs.command("M84").unwrap().markdown.contains("Release the steppers"));
        let m109 = docs.command("M109").unwrap();
        assert!(m109.markdown.contains("returns only once the target is reached"));
    }

    #[test]
    fn extended_commands_keep_fenced_lines_and_provider() {
        let docs = docs();
        let calibrate = docs.command("BED_MESH_CALIBRATE").unwrap();
        assert!(calibrate.markdown.contains("needs `[bed_mesh]`"));
        assert!(calibrate.markdown.contains("# not a heading"));
        assert!(docs.command("BED_MESH_OUTPUT").is_some());
    }

    #[test]
    fn options_with_defaults_and_axis_fallback() {
        let docs = docs();
        let steps = docs.option("stepper_z1", "full_steps_per_rotation");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].default.as_deref(), Some("200"));
        let step_pin = docs.option("stepper_y", "step_pin");
        assert_eq!(step_pin[0].default, None);
        assert!(step_pin[0].description.contains("Pin that steps the motor"));
    }

    #[test]
    fn example_values_dont_take_the_next_description() {
        let docs = docs();
        let kinematics = docs.option("printer", "kinematics");
        assert_eq!(kinematics.len(), 1);
        assert!(kinematics[0].description.is_empty());
        let z = docs.option("printer", "max_z_velocity");
        assert!(z[0].description.contains("Speed ceiling for the Z axis"));
    }

    #[test]
    fn see_the_other_section() {
        let docs = docs();
        let control = docs.option("heater_bed", "control");
        assert_eq!(control.len(), 2);
        assert_eq!(control[1].section, "extruder");
        assert!(control[1].description.contains("Heater control"));
    }

    #[test]
    fn inherits_settings_named_in_the_intro() {
        let docs = docs();
        // heater_bed's intro points at extruder; pid_Kp is only listed there.
        let max_power = docs.option("heater_bed", "max_power");
        assert_eq!(max_power.len(), 1);
        assert_eq!(max_power[0].section, "extruder");
        assert_eq!(max_power[0].default.as_deref(), Some("1.0"));
    }

    #[test]
    fn wildcard_options_and_section_intro() {
        let docs = docs();
        assert_eq!(docs.option("gcode_macro", "variable_speed").len(), 1);
        assert!(docs.option("gcode_macro", "variable_").is_empty());
        let bed = docs.section("heater_bed").unwrap();
        assert!(bed.markdown.contains("Heated bed"));
    }
}

#[cfg(test)]
mod real_docs {
    /// `KLIPPER_DOCS=/path/to/klipper/docs cargo test real_docs -- --ignored`
    #[test]
    #[ignore]
    fn loads_real_docs() {
        let dir = std::env::var("KLIPPER_DOCS").expect("set KLIPPER_DOCS");
        let docs = super::Docs::load(std::path::Path::new(&dir)).unwrap();
        for name in ["M140", "G28", "QUAD_GANTRY_LEVEL", "TEMPERATURE_WAIT", "M118", "RESPOND"] {
            assert!(docs.command(name).is_some(), "{name}");
        }
        assert!(!docs.option("stepper_y", "rotation_distance").is_empty());
    }
}

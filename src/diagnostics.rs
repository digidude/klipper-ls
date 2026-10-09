//! Diagnostics: facts about a config that can be checked without running
//! Klipper.
//!
//! Every check here comes from something we can read: Jinja 2.11's closed
//! set of filters and tests, the sections the config actually defines, and
//! (when Klipper's source was scanned) what the source registers. The
//! severity says how sure that makes us:
//!
//! - **Error**: the config is complete as far as we can see (every include
//!   resolved, every section type known to the docs) and Klipper's source
//!   was scanned, so Klipper itself would reject this.
//! - **Warning**: same, but only the docs were available.
//! - **Hint**: a section we don't know (a plugin) or an include that only
//!   exists on the printer could supply what looks missing, so we only
//!   point at it.
//!
//! Jinja filters and tests are always a warning: a newer Jinja than the one
//! Klipper pins adds a few, and we can't see which one runs on the printer.

use std::collections::HashSet;

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};
use tree_sitter::{Node, Tree};

use crate::index::Index;
use crate::knowledge::{KlipperDocs, Support};
use crate::position::LineIndex;
use crate::syntax::{field_text, text};

const SOURCE: &str = "klipper-ls";

/// Jinja 2.11, the version Klipper pins. Kept in step with the filter list in
/// `highlights.scm` (a test checks it).
pub const FILTERS: &[&str] = &[
    "abs", "attr", "batch", "capitalize", "center", "count", "d", "default", "dictsort", "e", "escape",
    "filesizeformat", "first", "float", "forceescape", "format", "groupby", "indent", "int", "join", "last",
    "length", "list", "lower", "map", "max", "min", "pprint", "random", "reject", "rejectattr", "replace",
    "reverse", "round", "safe", "select", "selectattr", "slice", "sort", "string", "striptags", "sum", "title",
    "tojson", "trim", "truncate", "unique", "upper", "urlencode", "urlize", "wordcount", "wordwrap", "xmlattr",
];

pub const TESTS: &[&str] = &[
    "boolean", "callable", "defined", "divisibleby", "eq", "equalto", "escaped", "even", "false", "float", "ge",
    "greaterthan", "gt", "in", "integer", "iterable", "le", "lessthan", "lower", "lt", "mapping", "ne", "none",
    "number", "odd", "sameas", "sequence", "string", "true", "undefined", "upper",
];

/// Names people reach for from Python or other template languages.
const FILTER_ALIASES: &[(&str, &str)] = &[
    ("integer", "int"),
    ("toint", "int"),
    ("tofloat", "float"),
    ("str", "string"),
    ("tostring", "string"),
    ("len", "length"),
    ("lowercase", "lower"),
    ("uppercase", "upper"),
    ("bool", "boolean"),
];

/// Section types that register a pin chip named after the section
/// (`[tmc2209 stepper_x]` -> `tmc2209_stepper_x`) or fixed (`probe`). This is
/// what Klipper's source does; it is only the part of it we can state
/// without scanning, and `register_chip('literal', ...)` calls found in a
/// scanned source are added on top.
const PROBE_SECTIONS: &[&str] = &["probe", "bltouch", "smart_effector", "probe_eddy_current", "load_cell_probe"];

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
enum Confidence {
    /// A section type or include we can't see into.
    Uncertain,
    /// Complete config, docs only.
    Likely,
    /// Complete config and Klipper's source scanned.
    Certain,
}

impl Confidence {
    fn severity(self) -> DiagnosticSeverity {
        match self {
            Confidence::Certain => DiagnosticSeverity::ERROR,
            Confidence::Likely => DiagnosticSeverity::WARNING,
            Confidence::Uncertain => DiagnosticSeverity::HINT,
        }
    }
}

struct Checks<'a> {
    text: &'a str,
    lines: &'a LineIndex,
    index: &'a Index,
    docs: Option<&'a KlipperDocs>,
    confidence: Option<Confidence>,
    chips: HashSet<String>,
    objects: HashSet<String>,
    renamed: HashSet<String>,
    out: Vec<Diagnostic>,
}

/// Diagnostics for one parsed config file. `index` is the whole config the
/// file belongs to; `docs` may be missing, in which case only the checks
/// that don't need it run.
pub fn config_diagnostics(
    tree: &Tree,
    text: &str,
    lines: &LineIndex,
    index: &Index,
    docs: Option<&KlipperDocs>,
) -> Vec<Diagnostic> {
    let confidence = docs.map(|d| confidence(index, d));
    let mut checks = Checks {
        text,
        lines,
        index,
        docs,
        confidence,
        chips: config_chips(index, docs),
        objects: index.sections.iter().map(|s| s.object_name()).collect(),
        renamed: index
            .macros
            .values()
            .flatten()
            .filter_map(|m| m.rename_existing.as_ref())
            .map(|r| r.to_ascii_uppercase())
            .collect(),
        out: Vec::new(),
    };
    checks.walk(tree.root_node());
    checks.out
}

fn confidence(index: &Index, docs: &KlipperDocs) -> Confidence {
    // A section type is known if the reference documents it or, with the
    // source scanned, Klipper has a module that loads it. (`[display_status]`
    // has no options, so the reference has no entry for it.)
    let known = |ty: &str| {
        let ty = ty.to_ascii_lowercase();
        ty == "include" || docs.section(&ty).is_some() || docs.source_module(&ty) == Some(true)
    };
    if index.unresolved_includes || !index.sections.iter().all(|s| known(&s.ty)) {
        Confidence::Uncertain
    } else if docs.source_scanned() {
        Confidence::Certain
    } else {
        Confidence::Likely
    }
}

/// Pin chips this config defines, by Klipper's own naming.
fn config_chips(index: &Index, docs: Option<&KlipperDocs>) -> HashSet<String> {
    let mut chips: HashSet<String> = HashSet::from(["mcu".to_string()]);
    for section in &index.sections {
        let ty = section.ty.as_str();
        let words: Vec<&str> = section.name.as_deref().unwrap_or("").split_whitespace().collect();
        match ty {
            "mcu" => {
                chips.insert(words.first().map_or("mcu", |w| w).to_string());
            }
            "multi_pin" | "replicape" => {
                chips.insert(ty.to_string());
            }
            "adc_scaled" | "ads1x1x" | "ads1115" => chips.extend(words.first().map(|w| w.to_string())),
            "sx1509" => chips.extend(words.first().map(|w| format!("sx1509_{w}"))),
            _ if PROBE_SECTIONS.contains(&ty) => {
                chips.insert("probe".to_string());
            }
            _ if ty.starts_with("tmc") => {
                chips.extend(words.last().map(|last| format!("{ty}_{last}")));
            }
            _ => {}
        }
    }
    chips.extend(docs.into_iter().flat_map(|d| d.source_chips().map(str::to_string)));
    chips
}

/// `step_pin`, `pins`, `pin`: the options whose value is a pin.
fn is_pin_option(key: &str) -> bool {
    key == "pin" || key == "pins" || key.ends_with("_pin") || key.ends_with("_pins")
}

impl Checks<'_> {
    fn walk(&mut self, node: Node) {
        match node.kind() {
            "option" => self.pin_chips(node),
            "filter_name" => self.jinja_name(node, "filter", FILTERS),
            "test_name" => self.jinja_name(node, "test", TESTS),
            "command" => self.command(node),
            "attribute" | "subscript" => self.printer_reference(node),
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child);
        }
    }

    fn push(&mut self, node: Node, severity: DiagnosticSeverity, code: &str, message: String) {
        self.out.push(Diagnostic {
            range: self.lines.range(self.text, node.start_byte(), node.end_byte()),
            severity: Some(severity),
            code: Some(NumberOrString::String(code.to_string())),
            source: Some(SOURCE.to_string()),
            message,
            ..Diagnostic::default()
        });
    }

    // ---- Jinja filters and tests ------------------------------------------

    fn jinja_name(&mut self, node: Node, what: &str, known: &[&str]) {
        let name = text(node, self.text);
        if known.contains(&name) {
            return;
        }
        let suggestion = match what {
            "filter" => suggest(name, FILTERS, FILTER_ALIASES),
            _ => suggest(name, TESTS, &[]),
        };
        let hint = suggestion.map(|s| format!(" Did you mean `{s}`?")).unwrap_or_default();
        self.push(
            node,
            DiagnosticSeverity::WARNING,
            if what == "filter" { "unknown-filter" } else { "unknown-test" },
            format!(
                "`{name}` is not one of the Jinja 2.11 {what}s Klipper's templates can use; Klipper adds none of its own.{hint}"
            ),
        );
    }

    // ---- Pin chips ---------------------------------------------------------

    fn pin_chips(&mut self, option: Node) {
        let Some(confidence) = self.confidence else { return };
        let Some(key) = field_text(option, "key", self.text) else { return };
        if !is_pin_option(&key.to_ascii_lowercase()) {
            return;
        }
        let Some(value) = option.child_by_field_name("value") else { return };
        let mut pins = Vec::new();
        collect_kind(value, "pin", &mut pins);
        for pin in pins {
            let Some(chip) = pin.child_by_field_name("chip") else { continue };
            let name = text(chip, self.text);
            if self.chips.contains(name) {
                continue;
            }
            let near = suggest(name, &self.chips.iter().map(String::as_str).collect::<Vec<_>>(), &[]);
            let hint = near.map(|s| format!(" Did you mean `{s}`?")).unwrap_or_default();
            let tail = match confidence {
                Confidence::Uncertain => {
                    " Something this tool can't see into (a plugin, or an include that only exists on the printer) may register it."
                }
                _ => "",
            };
            self.push(
                chip,
                confidence.severity(),
                "unknown-pin-chip",
                format!("No `[mcu]`, driver or probe in this config provides the pin chip `{name}`.{hint}{tail}"),
            );
        }
    }

    // ---- Commands ----------------------------------------------------------

    fn command(&mut self, node: Node) {
        let (Some(docs), Some(confidence)) = (self.docs, self.confidence) else { return };
        // `G9{ 0 if ABSOLUTE else 1 }`: the name is built by the template.
        if self.text[node.end_byte()..].starts_with('{') {
            return;
        }
        let name = text(node, self.text).to_ascii_uppercase();
        // T0, T1, ...: registered per extruder at runtime.
        let is_tool = name.strip_prefix('T').is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if is_tool || !self.index.macros(&name).is_empty() || self.renamed.contains(&name) {
            return;
        }
        // Without the source, "the docs don't mention it" proves nothing.
        if !matches!(docs.support(&name), Support::Unknown { source_checked: true }) {
            return;
        }
        let severity = match confidence {
            Confidence::Uncertain => DiagnosticSeverity::HINT,
            _ => DiagnosticSeverity::WARNING,
        };
        self.push(
            node,
            severity,
            "unknown-command",
            format!(
                "`{name}` is not a command Klipper registers and no `[gcode_macro {name}]` here defines it; Klipper will answer \"Unknown command\"."
            ),
        );
    }

    // ---- printer.<object> --------------------------------------------------

    /// `printer.heater_generic_x` / `printer["heater_generic x"]`, and the
    /// variable after `printer["gcode_macro X"]`.
    fn printer_reference(&mut self, node: Node) {
        let (object_field, segment_field) = match node.kind() {
            "attribute" => ("object", "attribute"),
            _ => ("value", "subscript"),
        };
        let is_printer = node
            .child_by_field_name(object_field)
            .is_some_and(|o| o.kind() == "identifier" && text(o, self.text) == "printer");
        if !is_printer {
            return;
        }
        let Some(segment) = node.child_by_field_name(segment_field) else { return };
        let object = match segment.kind() {
            "property_identifier" => text(segment, self.text).to_string(),
            "string" => match unquote(text(segment, self.text)) {
                Some(s) => s.to_string(),
                None => return,
            },
            _ => return,
        };
        self.object_exists(node, segment, &object);
        self.macro_variable(node, &object);
    }

    fn object_exists(&mut self, reference: Node, segment: Node, object: &str) {
        let (Some(docs), Some(confidence)) = (self.docs, self.confidence) else { return };
        let normalized = object.split_whitespace().collect::<Vec<_>>().join(" ");
        if self.objects.contains(&normalized) {
            return;
        }
        // Looking an object up is not an error: Klipper gives an undefined
        // value, and only reading *through* it raises. So an optional macro
        // guarded with `|default(...)`, `is defined` or `'x' in printer` is
        // fine, and only a dereference is worth flagging.
        if !self.dereferenced(reference) || self.guarded(&normalized) {
            return;
        }
        // An object with a name part exists only if a section defines it;
        // for a bare name the docs and the source also know built-ins
        // (`toolhead`, `gcode_move`, `configfile`).
        if !normalized.contains(' ') {
            let documented = docs.status().is_some_and(|s| !s.sections(&normalized).is_empty());
            let registered = docs.source_objects().any(|o| o == normalized);
            if documented || registered {
                return;
            }
            // Without either list we can't tell a built-in from a typo.
            if docs.status().is_none() && !docs.source_scanned() {
                return;
            }
        }
        // Names are case-sensitive in Klipper. If only the case differs,
        // say so, but don't call it an error we can't be sure of.
        let case_match = self.objects.iter().find(|o| o.eq_ignore_ascii_case(&normalized));
        let (severity, message) = match case_match {
            Some(real) => (
                DiagnosticSeverity::WARNING,
                format!("`{normalized}` doesn't match the section `[{real}]` exactly; object names are case-sensitive."),
            ),
            None => (
                confidence.severity(),
                format!(
                    "No section in this config creates a printer object `{normalized}`, so reading from it fails when this template runs. Guard an optional object with `is defined`."
                ),
            ),
        };
        self.push(segment, severity, "unknown-printer-object", message);
    }

    /// Is `reference` (`printer.x`) the object of a further `.field` / `[key]`?
    fn dereferenced(&self, reference: Node) -> bool {
        reference.parent().is_some_and(|p| {
            let object_field = match p.kind() {
                "attribute" => "object",
                "subscript" => "value",
                _ => return false,
            };
            p.child_by_field_name(object_field) == Some(reference)
        })
    }

    /// Does the file test for this object somewhere (`'x' in printer`,
    /// `printer.x is defined`)? Then a read elsewhere is presumably inside
    /// that guard, which we don't track.
    fn guarded(&self, object: &str) -> bool {
        ['\'', '"'].iter().any(|q| self.text.contains(&format!("{q}{object}{q} in printer")))
            || self.text.contains(&format!("{object}\"] is defined"))
            || self.text.contains(&format!("{object}'] is defined"))
    }

    fn macro_variable(&mut self, object_node: Node, object: &str) {
        let Some((ty, name)) = object.split_once(' ') else { return };
        if ty != "gcode_macro" {
            return;
        }
        let macros = self.index.macros(&name.trim().to_ascii_uppercase());
        if macros.is_empty() {
            return;
        }
        let Some(parent) = object_node.parent() else { return };
        let (object_field, segment_field) = match parent.kind() {
            "attribute" => ("object", "attribute"),
            "subscript" => ("value", "subscript"),
            _ => return,
        };
        if parent.child_by_field_name(object_field) != Some(object_node) {
            return;
        }
        let Some(field) = parent.child_by_field_name(segment_field) else { return };
        let variable = match field.kind() {
            "property_identifier" => text(field, self.text).to_string(),
            "string" => match unquote(text(field, self.text)) {
                Some(s) => s.to_string(),
                None => return,
            },
            _ => return,
        };
        let defined = macros
            .iter()
            .any(|m| m.variables.iter().any(|(v, _)| v.eq_ignore_ascii_case(&variable)));
        // `.contact_temp|default(150)` and `.x is defined` expect it to be missing.
        let expected_missing = parent.parent().is_some_and(|p| match p.kind() {
            "test" => true,
            "filter" => {
                p.child_by_field_name("value") == Some(parent)
                    && matches!(field_text(p, "name", self.text), Some("default" | "d"))
            }
            _ => false,
        });
        if !defined && !expected_missing {
            self.push(
                field,
                DiagnosticSeverity::WARNING,
                "unknown-macro-variable",
                format!(
                    "`{}` defines no `variable_{variable}`; the template sees an undefined value.",
                    macros[0].name
                ),
            );
        }
    }
}

fn collect_kind<'t>(node: Node<'t>, kind: &str, out: &mut Vec<Node<'t>>) {
    if node.kind() == kind {
        out.push(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_kind(child, kind, out);
    }
}

fn unquote(quoted: &str) -> Option<&str> {
    quoted.get(1..quoted.len().checked_sub(1)?)
}

/// The closest name to `name` among `known`, if it is close: a known alias,
/// a case difference, or within two edits.
fn suggest(name: &str, known: &[&str], aliases: &[(&str, &str)]) -> Option<String> {
    if let Some((_, to)) = aliases.iter().find(|(from, _)| *from == name) {
        return Some((*to).to_string());
    }
    if let Some(same) = known.iter().find(|k| k.eq_ignore_ascii_case(name)) {
        return Some((*same).to_string());
    }
    known
        .iter()
        .map(|k| (edit_distance(name, k), k))
        .filter(|(d, k)| *d <= 2 && *d < k.len().min(name.len()))
        .min_by_key(|(d, k)| (*d, **k))
        .map(|(_, k)| (*k).to_string())
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let current = row[j];
            row[j] = (row[j] + 1).min(row[j - 1] + 1).min(prev + usize::from(a[i - 1] != b[j - 1]));
            prev = current;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index;
    use crate::syntax;
    use std::collections::HashMap;
    use std::path::Path;

    const CONFIG_REFERENCE: &str = "# Configuration reference\n\n\
        ### [mcu]\n\nMicrocontroller.\n\n```\n[mcu]\nserial:\n```\n\n\
        ### [stepper_x]\n\nA stepper.\n\n```\n[stepper_x]\nstep_pin:\nendstop_pin:\n```\n\n\
        ### [tmc2209]\n\nA driver.\n\n```\n[tmc2209 stepper_x]\n```\n\n\
        ### [probe]\n\nA probe.\n\n```\n[probe]\npin:\n```\n\n\
        ### [output_pin]\n\nA pin.\n\n```\n[output_pin my_pin]\npin:\n```\n\n\
        ### [gcode_macro]\n\nA macro.\n\n```\n[gcode_macro my_macro]\ngcode:\n```\n\n\
        ### [heater_generic]\n\nA heater.\n\n```\n[heater_generic my_heater]\n```\n";

    const STATUS: &str = "# Status reference\n\n## toolhead\n\n\
        The following information is available in the `toolhead` object:\n\
        - `position`: the position.\n";

    fn docs() -> KlipperDocs {
        KlipperDocs::from_strings("# G-Codes\n\n#### G28\n\nHome.\n\n#### SET_PIN\n\nSet a pin.\n", CONFIG_REFERENCE, Path::new("x.md"))
            .with_handlers(&["G28", "SET_PIN", "RESPOND"])
            .with_status(STATUS)
            .with_source_names(&[], &["gcode_move"])
    }

    fn run(config: &str, docs: Option<&KlipperDocs>) -> Vec<(String, String)> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("klipper-ls-diag-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("printer.cfg");
        std::fs::write(&path, config).unwrap();
        let index = index::build(&path, &HashMap::new());
        let tree = syntax::parse(config);
        let lines = LineIndex::new(config);
        config_diagnostics(&tree, config, &lines, &index, docs)
            .into_iter()
            .map(|d| {
                let code = match d.code {
                    Some(NumberOrString::String(c)) => c,
                    _ => String::new(),
                };
                let severity = match d.severity {
                    Some(DiagnosticSeverity::ERROR) => "error",
                    Some(DiagnosticSeverity::WARNING) => "warning",
                    _ => "hint",
                };
                (code, severity.to_string())
            })
            .collect()
    }

    fn codes(found: &[(String, String)]) -> Vec<&str> {
        found.iter().map(|(c, _)| c.as_str()).collect()
    }

    #[test]
    fn jinja_filter_and_test_lists_match_the_highlight_queries() {
        let queries = include_str!("../syntax/zed/languages/klipper/highlights.scm");
        // ((filter_name) @function
        //   (#match? @function "^(abs|attr|...)$"))
        let list = |capture: &str| -> Vec<String> {
            let from = queries.find(&format!("(({capture}) @function")).expect("query present");
            let rest = &queries[from..];
            let open = rest.find("\"^(").unwrap() + 3;
            let close = rest[open..].find(")$\"").unwrap();
            rest[open..open + close].split('|').map(str::to_string).collect()
        };
        assert_eq!(list("filter_name"), FILTERS, "FILTERS drifted from highlights.scm");
        assert_eq!(list("test_name"), TESTS, "TESTS drifted from highlights.scm");
    }

    #[test]
    fn flags_unknown_filters_and_tests_with_a_suggestion() {
        let found = run(
            "[gcode_macro A]\ngcode:\n  {% set x = params.T|integer %}\n  {% if x is definde %}{% endif %}\n  {% if x|int is defined %}{% endif %}\n",
            None,
        );
        assert_eq!(codes(&found), ["unknown-filter", "unknown-test"]);
    }

    #[test]
    fn suggests_the_obvious_name() {
        assert_eq!(suggest("integer", FILTERS, FILTER_ALIASES).as_deref(), Some("int"));
        assert_eq!(suggest("defind", TESTS, &[]).as_deref(), Some("defined"));
        assert_eq!(suggest("frobnicate", FILTERS, &[]), None);
    }

    #[test]
    fn pin_chips_resolve_against_the_config() {
        let config = "[mcu]\nserial: /dev/x\n[mcu EBBCan]\ncanbus_uuid: 1\n\
                      [stepper_x]\nstep_pin: PF13\nendstop_pin: ^!EBBCan:PB6\n\
                      [tmc2209 stepper_x]\n[probe]\npin: probe:z_virtual_endstop\n\
                      [output_pin a]\npin: tmc2209_stepper_x:virtual_endstop\n";
        assert_eq!(run(config, Some(&docs())), []);

        let typo = config.replace("^!EBBCan:PB6", "^!EBBCna:PB6");
        let found = run(&typo, Some(&docs()));
        assert_eq!(found, [("unknown-pin-chip".to_string(), "error".to_string())]);
    }

    #[test]
    fn chip_checks_only_look_at_pin_options() {
        let config = "[mcu]\n[gcode_macro A]\ndescription: ask nobody:PB6 about it\ngcode:\n  G28\n";
        assert_eq!(run(config, Some(&docs())), []);
    }

    #[test]
    fn unknown_section_types_soften_to_hints() {
        let config = "[mcu]\n[beacon]\nserial: x\n[stepper_x]\nendstop_pin: beacon:z_virtual_endstop\n";
        let found = run(config, Some(&docs()));
        assert_eq!(found, [("unknown-pin-chip".to_string(), "hint".to_string())]);
    }

    #[test]
    fn missing_printer_objects_and_macro_variables() {
        let config = "[mcu]\n[heater_generic chamber]\n[gcode_macro M]\nvariable_depth: 1\ngcode:\n\
                      \x20 {% set a = printer.toolhead.position %}\n\
                      \x20 {% set b = printer[\"heater_generic chamber\"].target %}\n\
                      \x20 {% set c = printer[\"heater_generic chambre\"].target %}\n\
                      \x20 {% set d = printer.gcode_move.speed %}\n\
                      \x20 {% set e = printer[\"gcode_macro M\"].depth %}\n\
                      \x20 {% set f = printer[\"gcode_macro M\"].width %}\n\
                      \x20 {% set g = printer.heatr_bed.temperature %}\n";
        let found = run(config, Some(&docs()));
        assert_eq!(
            found,
            [
                ("unknown-printer-object".to_string(), "error".to_string()),
                ("unknown-macro-variable".to_string(), "warning".to_string()),
                ("unknown-printer-object".to_string(), "error".to_string()),
            ]
        );
    }

    #[test]
    fn optional_objects_behind_a_guard_are_fine() {
        let config = "[mcu]\n[gcode_macro M]\ngcode:\n\
                      \x20 {% set a = printer['gcode_macro OPTIONAL']|default({}) %}\n\
                      \x20 {% if printer['gcode_macro OTHER'] is defined %}{% endif %}\n\
                      \x20 {% if 'gcode_macro THIRD' in printer %}{{ printer['gcode_macro THIRD'].x }}{% endif %}\n\
                      \x20 {% set b = printer['gcode_macro MISSING'].x %}\n";
        let found = run(config, Some(&docs()));
        assert_eq!(found, [("unknown-printer-object".to_string(), "error".to_string())]);
    }

    #[test]
    fn a_defaulted_macro_variable_is_expected_to_be_missing() {
        let config = "[mcu]\n[gcode_macro M]\nvariable_a: 1\ngcode:\n\
                      \x20 {% set x = printer[\"gcode_macro M\"].b|default(5) %}\n\
                      \x20 {% if printer[\"gcode_macro M\"].c is defined %}{% endif %}\n\
                      \x20 {% set y = printer[\"gcode_macro M\"].d %}\n";
        let found = run(config, Some(&docs()));
        assert_eq!(found, [("unknown-macro-variable".to_string(), "warning".to_string())]);
    }

    #[test]
    fn unknown_commands_need_the_source_scan() {
        let config = "[mcu]\n[gcode_macro A]\nrename_existing: G28.1\ngcode:\n  G28\n  G28.1\n  M500\n  A\n  T0\n  SET_PIN PIN=a VALUE=1\n";
        let found = run(config, Some(&docs()));
        assert_eq!(codes(&found), ["unknown-command"]);

        // Docs only: the docs not mentioning M500 doesn't prove anything.
        let no_source = KlipperDocs::from_strings("# G-Codes\n\n#### G28\n\nHome.\n", CONFIG_REFERENCE, Path::new("x.md"));
        assert_eq!(run(config, Some(&no_source)), []);
    }

    #[test]
    fn nothing_docs_dependent_runs_without_docs() {
        let config = "[mcu]\n[stepper_x]\nstep_pin: bogus:PA1\n[gcode_macro A]\ngcode:\n  M500\n  {% set x = printer.nothing %}\n";
        assert_eq!(run(config, None), []);
    }
}

/// Run against real files, to see what the checks say on configs that work:
///
/// ```sh
/// KLIPPER_CONFIG=~/printer_data/config/printer.cfg KLIPPER_DOCS=klipper/docs \
///   cargo test diagnostics_coverage -- --ignored --nocapture
/// ```
///
/// Every line printed is a claim that Klipper would object. On a config that
/// runs, each one should be a real finding or a bug here.
#[cfg(test)]
mod real_data {
    use super::*;
    use crate::{index, syntax};
    use std::collections::{BTreeMap, HashMap};
    use std::path::PathBuf;

    #[test]
    #[ignore = "needs KLIPPER_CONFIG and KLIPPER_DOCS"]
    fn diagnostics_coverage() {
        let (Some(config), Some(docs_dir)) = (std::env::var_os("KLIPPER_CONFIG"), std::env::var_os("KLIPPER_DOCS"))
        else {
            eprintln!("set KLIPPER_CONFIG and KLIPPER_DOCS");
            return;
        };
        let config = PathBuf::from(config);
        let docs = KlipperDocs::load(&PathBuf::from(docs_dir)).expect("docs load");
        let index = index::build(&config, &HashMap::new());
        let unknown_types: std::collections::BTreeSet<_> = index
            .sections
            .iter()
            .filter(|s| {
                let ty = s.ty.to_ascii_lowercase();
                ty != "include" && docs.section(&ty).is_none() && docs.source_module(&ty) != Some(true)
            })
            .map(|s| s.ty.clone())
            .collect();
        eprintln!(
            "{} sections, includes resolved: {}, source scanned: {}, section types the docs don't know: {:?}",
            index.sections.len(),
            !index.unresolved_includes,
            docs.source_scanned(),
            unknown_types
        );

        let root = config.parent().unwrap();
        let mut totals: BTreeMap<String, usize> = BTreeMap::new();
        let mut files = vec![config.clone()];
        files.extend(glob::glob(&format!("{}/**/*.cfg", root.display())).unwrap().filter_map(Result::ok));
        files.sort();
        files.dedup();
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let tree = syntax::parse(&text);
            let lines = LineIndex::new(&text);
            // As the server does: printer.cfg and its includes, plus this file.
            let index = index::build_from(&config, Some(&file), &HashMap::new());
            for d in config_diagnostics(&tree, &text, &lines, &index, Some(&docs)) {
                let code = match &d.code {
                    Some(NumberOrString::String(c)) => c.clone(),
                    _ => String::new(),
                };
                let severity = match d.severity {
                    Some(DiagnosticSeverity::ERROR) => "ERROR",
                    Some(DiagnosticSeverity::WARNING) => "warning",
                    _ => "hint",
                };
                *totals.entry(format!("{severity} {code}")).or_default() += 1;
                let at = text.lines().nth(d.range.start.line as usize).unwrap_or("").trim();
                eprintln!(
                    "{severity:7} {code:24} {}:{}  {at}",
                    file.strip_prefix(root).unwrap_or(&file).display(),
                    d.range.start.line + 1
                );
            }
        }
        eprintln!("\n{totals:#?}");
    }
}

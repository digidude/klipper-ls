//! Klipper's `Status_Reference.md`: the `printer.<object>.<field>` values a
//! macro can read.
//!
//! The doc is organised by topic, not strictly by object name: `## heater`
//! covers `extruder`, `heater_bed` and `heater_generic`, `## fan` covers
//! `heater_fan` and `controller_fan`. So each section's intro is read for the
//! object types it names (`[heater_bed](...)`, `` `[neopixel led_name]` ``,
//! `` `toolhead` ``), skipping conditions such as "(this object is available
//! if a [virtual_sdcard] section is defined)".

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::klipper::{absolute_links, code_spans};

pub const FILE: &str = "Status_Reference.md";

#[derive(Debug, Clone)]
pub struct Field {
    /// Each documented path, split into segments:
    /// `settings.<section>.<option>` -> [settings, <section>, <option>].
    /// `<...>` segments match anything.
    pub paths: Vec<Vec<String>>,
    /// The bullet, as markdown.
    pub markdown: String,
    pub line: u32,
}

impl Field {
    /// Does any documented path agree with `path` (the segments after the
    /// object) as far as both go? `position` documents `position.x`;
    /// `info.total_layer` answers a hover on `info`.
    fn matches(&self, path: &[String]) -> bool {
        self.paths.iter().any(|spec| {
            spec.iter()
                .zip(path)
                .all(|(s, p)| s.starts_with('<') || p == "*" || s.eq_ignore_ascii_case(p))
        })
    }
}

#[derive(Debug, Clone)]
pub struct Section {
    pub heading: String,
    pub intro: String,
    pub fields: Vec<Field>,
    pub path: PathBuf,
    pub line: u32,
    /// `- all items from [load_cell](...)`
    includes: Vec<String>,
}

impl Section {
    pub fn field_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for field in &self.fields {
            for spec in &field.paths {
                if let Some(first) = spec.first().filter(|f| !names.contains(&f.as_str())) {
                    names.push(first);
                }
            }
        }
        names
    }
}

#[derive(Debug, Default)]
pub struct StatusDocs {
    sections: Vec<Section>,
    /// Object type (`toolhead`, `heater_generic`, `tmc`) -> sections.
    by_type: HashMap<String, Vec<usize>>,
}

impl StatusDocs {
    pub fn parse(text: &str, path: &Path) -> Self {
        let lines: Vec<&str> = text.lines().collect();
        let mut docs = StatusDocs::default();
        let mut i = lines.iter().position(|l| l.starts_with("## ")).unwrap_or(lines.len());

        while i < lines.len() {
            let heading = lines[i].trim_start_matches('#').trim().to_string();
            let start = i;
            i += 1;
            let mut intro: Vec<&str> = Vec::new();
            let mut fields: Vec<Field> = Vec::new();
            let mut includes: Vec<String> = Vec::new();
            let mut in_fence = false;
            while i < lines.len() && !lines[i].starts_with("## ") {
                let line = lines[i];
                if line.trim_start().starts_with("```") {
                    in_fence = !in_fence;
                } else if !in_fence && line.starts_with("- ") {
                    let (bullet, next) = take_bullet(&lines, i);
                    if let Some(rest) = bullet.strip_prefix("all items from [") {
                        includes.extend(rest.split(']').next().map(str::to_ascii_lowercase));
                    } else {
                        let paths: Vec<Vec<String>> = field_specs(&bullet).iter().map(|s| segments(s)).collect();
                        if !paths.is_empty() {
                            fields.push(Field { paths, markdown: links(&bullet), line: i as u32 });
                        }
                    }
                    i = next;
                    continue;
                } else if !in_fence && fields.is_empty() && includes.is_empty() {
                    intro.push(line);
                }
                i += 1;
            }

            let types = object_types(&heading, &intro.join(" "));
            if types.is_empty() || (fields.is_empty() && includes.is_empty()) {
                continue;
            }
            let index = docs.sections.len();
            for ty in types {
                docs.by_type.entry(ty).or_default().push(index);
            }
            docs.sections.push(Section {
                heading,
                intro: links(intro.join("\n").trim()),
                fields,
                path: path.to_path_buf(),
                line: start as u32,
                includes,
            });
        }
        docs
    }

    /// Sections documenting an object, named as a macro would:
    /// `toolhead`, `extruder1`, `heater_generic chamber`, `tmc2209 stepper_x`.
    pub fn sections(&self, object: &str) -> Vec<&Section> {
        let ty = object.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        let base = ty.trim_end_matches(|c: char| c.is_ascii_digit());
        let Some(found) = self.by_type.get(&ty).or_else(|| self.by_type.get(base)) else {
            return Vec::new();
        };
        let mut result: Vec<usize> = found.clone();
        // One level of "all items from [x]" is all the doc uses.
        for &i in found {
            for include in &self.sections[i].includes {
                for &j in self.by_type.get(include).into_iter().flatten() {
                    if !result.contains(&j) {
                        result.push(j);
                    }
                }
            }
        }
        result.into_iter().map(|i| &self.sections[i]).collect()
    }

    /// Fields of `object` that agree with `path`, the segments after the
    /// object up to the one under the cursor (`*` for a computed subscript).
    pub fn fields(&self, object: &str, path: &[String]) -> Vec<(&Section, &Field)> {
        self.sections(object)
            .into_iter()
            .flat_map(|s| s.fields.iter().filter(|f| f.matches(path)).map(move |f| (s, f)))
            .collect()
    }

    /// Where `path` leaves the doc: the index of its first segment that no
    /// field documents (`info.bogus` -> 1, so the hover names `bogus`, not
    /// `info`), or None when every segment is documented.
    pub fn undocumented(&self, object: &str, path: &[String]) -> Option<usize> {
        (0..path.len()).find(|&k| self.fields(object, &path[..=k]).is_empty())
    }
}

/// A top-level bullet with its wrapped lines joined and sub-bullets kept,
/// ending at a blank line, the next bullet or a heading.
fn take_bullet(lines: &[&str], start: usize) -> (String, usize) {
    let mut text = lines[start][2..].trim().to_string();
    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() || line.starts_with("- ") || line.starts_with('#') || trimmed.starts_with("```") {
            break;
        }
        if trimmed.starts_with("- ") {
            text.push_str(&format!("\n  {trimmed}"));
        } else {
            text.push(' ');
            text.push_str(trimmed);
        }
        i += 1;
    }
    (text, i)
}

/// The field names a bullet documents: the code spans before its colon
/// (`` `axis_minimum`, `axis_maximum`: ... ``), or for prose bullets ("For
/// Delta printers the `cone_start_z` is ...") the first plain name in one.
fn field_specs(bullet: &str) -> Vec<&str> {
    let mut specs = Vec::new();
    let mut rest = bullet;
    loop {
        rest = rest.trim_start_matches([' ', ',']);
        rest = rest.strip_prefix("and ").unwrap_or(rest);
        let Some((spec, tail)) = rest.strip_prefix('`').and_then(|r| r.split_once('`')) else {
            break;
        };
        specs.push(spec);
        rest = tail;
    }
    if rest.starts_with(':') && !specs.is_empty() {
        return specs;
    }
    code_spans(bullet).into_iter().filter(|s| is_name(s)).take(1).collect()
}

/// `last_query["<endstop>"]` -> [last_query, <endstop>];
/// `printer["servo <config_name>"].value` -> [value].
fn segments(spec: &str) -> Vec<String> {
    let spec = match spec.strip_prefix("printer[") {
        Some(rest) => rest.split_once("].").map_or(rest, |(_, field)| field),
        None => spec,
    };
    spec.replace(['[', ']', '"', '\''], ".")
        .split('.')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_name(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Object types a section applies to: its heading when that is a name, plus
/// the sections its intro names, except inside "(... if ...)" conditions.
fn object_types(heading: &str, intro: &str) -> Vec<String> {
    let mut types: Vec<String> = Vec::new();
    let mut add = |name: &str| {
        let name = name.trim_matches(['[', ']']).split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        if is_name(&name) && !types.contains(&name) {
            types.push(name);
        }
    };
    add(heading);
    let intro = drop_conditions(&drop_link_targets(&intro.split_whitespace().collect::<Vec<_>>().join(" ")));
    for span in code_spans(&intro) {
        add(span);
    }
    for piece in intro.split('[').skip(1) {
        add(piece.split(']').next().unwrap_or(""));
    }
    types
}

/// `[heater_bed](Config_Reference.md#heater_bed)` -> `[heater_bed]`
fn drop_link_targets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        out.push_str(&rest[..=start]);
        rest = &rest[start + 2..];
        rest = rest.split_once(')').map_or("", |(_, after)| after);
    }
    out.push_str(rest);
    out
}

/// Removes parentheticals that state a condition, such as "(only present
/// if a [display] section exists)". Keeps ones that add examples or
/// scope, such as "(also [extruder] objects)" and "(eg, `[tmc2208 stepper_x]`)".
fn drop_conditions(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        let Some(close) = rest[open..].find(')').map(|c| open + c) else { break };
        out.push_str(&rest[..open]);
        let inner = &rest[open..=close];
        if !inner.contains(" if ") {
            out.push_str(inner);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// In-page links (`(#accessing-coordinates)`) belong to this doc.
fn links(markdown: &str) -> String {
    absolute_links(&markdown.replace("](#", &format!("]({FILE}#")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Same shape as Klipper's Status_Reference.md (headings, intros naming
    // objects, conditions, links, bullets, sub-bullets), in our own words:
    // the real doc is GPL and never goes into the repo.
    const STATUS_MD: &str = "\
# Status fixture

Read by [templates](Command_Templates.md).

## configfile

Fields of the `configfile` object
(present on every printer):
- `settings.<section>.<option>`: One parsed setting.
- `save_config_pending`: Whether SAVE_CONFIG has work to do.

## extruder_stepper

Fields of extruder_stepper objects (and of
[extruder](Config_Reference.md#extruder) objects too):
- `pressure_advance`: Pressure advance in use.

## heater

Common to every heater: [heater_generic](Config_Reference.md#heater_generic),
but also [heater_bed](Config_Reference.md#heater_bed) and
[extruder](Config_Reference.md#extruder):
- `temperature`: Latest reading.
- `target`: Requested temperature.

## load_cell

Fields of every `[load_cell name]`:
- `tare_counts`: Counts at zero load.

## load_cell_probe

The probing flavour of a load cell, `[load_cell_probe]`, has:
- all items from [load_cell](Status_Reference.md#load_cell)
- `endstop_tare_counts`: Counts at zero load while probing.

## print_stats

Fields of the `print_stats` object
(it exists only if a
[virtual_sdcard](Config_Reference.md#virtual_sdcard) section is
configured):
- `filename`, `total_duration`, `print_duration`: Job details.
- `info.total_layer`: Layer count from the slicer.
- `info.current_layer`: Layer in progress.

## query_endstops

Fields of the `query_endstops` object:
- `last_query[\"<endstop>\"]`: Whether that endstop was
  triggered at the last query.

## servo

Applies to [servo my_servo](Config_Reference.md#servo) sections:
- `printer[\"servo <config_name>\"].value`: Latest PWM setting.

## tmc drivers

Applies to [TMC driver](Config_Reference.md#tmc-stepper-driver-configuration)
sections (for instance `[tmc2209 stepper_y]`):
- `run_current`: Run current in use.

## toolhead

Fields of the `toolhead` object:
- `position`: Where the toolhead was last sent, given
  as a [coordinate](#accessing-coordinates).
- On a delta the `cone_start_z` marks the top of the cone.
- `results[\"<screw>\"]`: One entry per screw with:
  - `z`: Height found there.

## Accessing Coordinates

Some fields hold a \"coordinate\".
";

    fn docs() -> StatusDocs {
        StatusDocs::parse(STATUS_MD, Path::new(FILE))
    }

    fn path(p: &str) -> Vec<String> {
        p.split('.').map(str::to_string).collect()
    }

    fn headings(docs: &StatusDocs, object: &str, p: &str) -> Vec<String> {
        docs.fields(object, &path(p)).iter().map(|(s, _)| s.heading.clone()).collect()
    }

    #[test]
    fn objects_named_in_the_intro() {
        let docs = docs();
        assert_eq!(headings(&docs, "heater_generic chamber", "temperature"), ["heater"]);
        assert_eq!(headings(&docs, "extruder1", "temperature"), ["heater"]);
        let extruder: Vec<_> = docs.sections("extruder").iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(extruder, ["extruder_stepper", "heater"]);
        assert_eq!(headings(&docs, "tmc2209 stepper_x", "run_current"), ["tmc drivers"]);
        assert_eq!(headings(&docs, "servo my_servo", "value"), ["servo"]);
        assert!(docs.sections("virtual_sdcard").is_empty(), "named only in an availability condition");
        assert!(docs.sections("accessing").is_empty());
    }

    #[test]
    fn field_paths() {
        let docs = docs();
        assert_eq!(docs.fields("print_stats", &path("total_duration")).len(), 1);
        assert_eq!(docs.fields("print_stats", &path("info")).len(), 2);
        assert_eq!(docs.fields("print_stats", &path("info.current_layer")).len(), 1);
        assert_eq!(docs.fields("configfile", &path("settings.extruder.max_temp")).len(), 1);
        assert_eq!(docs.fields("query_endstops", &path("last_query.x")).len(), 1);
        assert_eq!(docs.fields("toolhead", &path("position.x")).len(), 1);
        assert_eq!(docs.fields("toolhead", &path("cone_start_z")).len(), 1);
        assert_eq!(docs.fields("toolhead", &path("results.*.z")).len(), 1);
        assert!(docs.fields("toolhead", &path("positon")).is_empty());
        assert_eq!(headings(&docs, "load_cell_probe", "tare_counts"), ["load_cell"]);
    }

    #[test]
    fn first_undocumented_segment() {
        let docs = docs();
        assert_eq!(docs.undocumented("print_stats", &path("info.bogus")), Some(1));
        assert_eq!(docs.undocumented("print_stats", &path("bogus.total_layer")), Some(0));
        assert_eq!(docs.undocumented("toolhead", &path("positon")), Some(0));
        assert_eq!(docs.undocumented("print_stats", &path("info.total_layer")), None);
        assert_eq!(docs.undocumented("toolhead", &path("results.*.z")), None);
    }

    #[test]
    fn links_and_sub_bullets() {
        let docs = docs();
        let position = &docs.fields("toolhead", &path("position"))[0].1.markdown;
        assert!(position.contains("(https://www.klipper3d.org/Status_Reference.html#accessing-coordinates)"), "{position}");
        let results = &docs.fields("toolhead", &path("results"))[0].1.markdown;
        assert!(results.ends_with("with:\n  - `z`: Height found there."), "{results}");
    }
}

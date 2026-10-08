//! What the server knows about commands, sections and options, independent
//! of where the cursor is or what kind of file it's in.
//!
//! Two sources, layered:
//! - [`klipper`]: Klipper's own docs. Ground truth for what actually runs.
//!   [`status`] covers the `printer.*` fields macros read.
//! - [`marlin`]: Marlin's reference, for per-parameter detail on standard
//!   G/M codes, annotated with what Klipper ignores.
//!
//! Both are read from disk at runtime and never compiled in.

pub mod klipper;
pub mod marlin;
pub mod status;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::thread;

pub use klipper::{DocEntry, Docs as KlipperDocs, Support, framed};
use marlin::{MarlinDoc, MarlinDocs};

/// `~/Library/Caches/klipper-ls` (macOS) or `$XDG_CACHE_HOME/klipper-ls`.
pub fn cache_root() -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let base = match std::env::var_os("XDG_CACHE_HOME") {
        Some(dir) => PathBuf::from(dir),
        None if cfg!(target_os = "macos") => home()?.join("Library/Caches"),
        None => home()?.join(".cache"),
    };
    Some(base.join("klipper-ls"))
}

/// Marlin's docs, loaded on a background thread so a first-time download
/// (~250 files) never blocks a hover. Until it's ready, hovers simply show
/// Klipper's text alone.
#[derive(Clone, Default)]
pub struct MarlinSlot(Arc<OnceLock<Option<Arc<MarlinDocs>>>>);

impl MarlinSlot {
    pub fn start(explicit: Option<PathBuf>, download: bool) -> Self {
        let slot = MarlinSlot::default();
        let cell = slot.0.clone();
        thread::spawn(move || {
            let _ = cell.set(load_marlin(explicit, download));
        });
        slot
    }

    pub fn get(&self) -> Option<Arc<MarlinDocs>> {
        self.0.get().cloned().flatten()
    }
}

fn load_marlin(explicit: Option<PathBuf>, download: bool) -> Option<Arc<MarlinDocs>> {
    let dir = match explicit {
        // A MarlinDocumentation checkout, or its _gcode folder directly.
        Some(dir) if dir.join("_gcode").is_dir() => dir.join("_gcode"),
        Some(dir) => dir,
        None => {
            let cache = cache_root()?.join("marlin");
            if !marlin::is_cached(&cache) {
                if !download {
                    return None;
                }
                eprintln!("klipper-ls: downloading Marlin's G-code reference into {}", cache.display());
                if let Err(e) = marlin::download(&cache) {
                    eprintln!("klipper-ls: {e}; hovers will show Klipper's docs only");
                    return None;
                }
            }
            cache
        }
    };
    match MarlinDocs::load(&dir) {
        Ok(docs) => {
            eprintln!("klipper-ls: using Marlin's G-code reference from {} ({} codes)", dir.display(), docs.len());
            Some(Arc::new(docs))
        }
        Err(e) => {
            eprintln!("klipper-ls: reading Marlin docs in {}: {e}", dir.display());
            None
        }
    }
}

/// G0, M109, T1: the codes Marlin documents and Klipper may or may not handle.
pub fn is_standard_code(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('G' | 'M' | 'T' | 'g' | 'm' | 't'))
        && name.len() > 1
        && chars.all(|c| c.is_ascii_digit() || c == '.')
}

const MAX_MARLIN_PARAMS: usize = 12;

/// Between the hover's code header, body and footer.
pub const SEPARATOR: &str = "\n\n---\n\n";

/// A fenced block for a hover's header. `language` is a Zed language name
/// (case-insensitive); `None` gives plain monospace. Only name languages this
/// extension defines: "G-code" comes from a third-party grammar that mis-lexes
/// docs-style syntax (`[S<temperature>]`, `END` inside `HOTEND`).
pub fn code_block(language: Option<&str>, code: &str) -> String {
    format!("```{}\n{code}\n```", language.unwrap_or(""))
}

/// The sources available for one request.
#[derive(Clone, Copy, Default)]
pub struct Sources<'a> {
    pub klipper: Option<&'a KlipperDocs>,
    pub marlin: Option<&'a MarlinDocs>,
}

impl<'a> Sources<'a> {
    /// Klipper's entry first (it's what will run), then for standard codes
    /// Marlin's parameter detail. A standard code Klipper doesn't document,
    /// and no macro defines, gets a warning: Klipper would skip it.
    pub fn command(&self, name: &str, defined_by_macro: bool) -> Vec<String> {
        let mut parts = Vec::new();
        let klipper = self.klipper.and_then(|k| k.command(name));
        if let Some(entry) = klipper {
            parts.push(command_markdown(entry));
        }
        let name = name.to_ascii_uppercase();
        let support = self.klipper.map(|k| k.support(&name));
        if let Some(Support::SourceOnly { path, line }) = &support {
            let file = path.file_name().map_or(String::new(), |f| f.to_string_lossy().into_owned());
            parts.push(format!(
                "**{name}** · handled by Klipper, not in its docs\n\n\
                 Klipper's source has a handler for it (`{file}:{}`), but G-Codes.md doesn't describe it.",
                line + 1
            ));
        }
        if is_standard_code(&name) {
            match support {
                Some(Support::Unknown { source_checked: true }) if !defined_by_macro => parts.push(format!(
                    "**{name}** · not handled by Klipper\n\n\
                     Klipper replies `Unknown command` and skips this line; a print carries on. \
                     A `[gcode_macro {name}]` can implement it."
                )),
                Some(Support::Unknown { source_checked: false }) if !defined_by_macro => parts.push(format!(
                    "**{name}** · not in Klipper's docs\n\n\
                     Klipper most likely replies `Unknown command` and skips this line (a print carries on). \
                     With a local Klipper checkout the server can check Klipper's source to be sure."
                )),
                _ => {}
            }
            if let Some(doc) = self.marlin.and_then(|m| m.get(&name)) {
                let klipper_params = klipper.and_then(|e| e.params.as_deref());
                parts.push(render_marlin(doc, klipper_params, klipper.is_some()));
            }
        }
        parts
    }

    /// One parameter of a built-in command. For standard codes: whether
    /// Klipper uses it, then Marlin's description. For Klipper's extended
    /// commands the command's own entry documents its parameters.
    pub fn parameter(&self, command: &str, param: &str) -> Vec<String> {
        let klipper = self.klipper.and_then(|k| k.command(command));
        if !is_standard_code(command) {
            return klipper.map(command_markdown).into_iter().collect();
        }
        let command = command.to_ascii_uppercase();
        let param = param.to_ascii_uppercase();
        let marlin = self.marlin.and_then(|m| m.get(&command));
        let marlin_param = marlin.and_then(|doc| doc.params.iter().find(|p| p.tag.eq_ignore_ascii_case(&param)));

        let mut md = format!("**{param}**");
        let support = self.klipper.map(|k| k.support(&command));
        match (klipper, klipper.and_then(|e| e.params.as_deref())) {
            (None, _) if support == Some(Support::Unknown { source_checked: true }) => {
                md.push_str("\n\nKlipper doesn't handle this command at all.");
            }
            (Some(_), Some(known)) if known.iter().any(|k| k.eq_ignore_ascii_case(&param)) => {
                md.push_str("\n\nUsed by Klipper.");
            }
            (Some(_), Some(known)) => {
                let list = known.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ");
                let takes = if known.is_empty() { "no parameters".to_string() } else { list };
                md.push_str(&format!("\n\n**Ignored by Klipper**, whose `{command}` takes {takes}."));
            }
            _ => {}
        }
        if let Some(p) = marlin_param {
            md.push_str(&format!("\n\nMarlin: {}", p.description));
            if let Some(value) = &p.value {
                md.push_str(&format!(" (`{value}`)"));
            }
        }
        if let Some(entry) = klipper {
            md.push_str(&format!("{SEPARATOR}{}", entry.markdown));
        }
        // Header: the command's syntax, with the parameter under the cursor
        // in context.
        let header = klipper
            .and_then(|e| e.signature.as_deref())
            .map_or_else(|| format!("{command} {param}"), str::to_string);
        vec![format!("{}{SEPARATOR}{md}", code_block(None, &header))]
    }

    /// Where go-to-definition should land for a command with no macro.
    pub fn command_location(&self, name: &str) -> Option<(PathBuf, u32)> {
        if let Some(entry) = self.klipper.and_then(|k| k.command(name)) {
            return Some((entry.path.clone(), entry.line));
        }
        if let Some(Support::SourceOnly { path, line }) = self.klipper.map(|k| k.support(name)) {
            return Some((path, line));
        }
        let doc = self.marlin.filter(|_| is_standard_code(name))?.get(name)?;
        Some((doc.path.clone(), 0))
    }
}

/// A Klipper command entry under its syntax line, when the docs give one.
fn command_markdown(entry: &DocEntry) -> String {
    match &entry.signature {
        Some(sig) => format!("{}{SEPARATOR}{}", code_block(None, sig), entry.markdown),
        None => entry.markdown.clone(),
    }
}

fn render_marlin(doc: &MarlinDoc, klipper_params: Option<&[String]>, klipper_handles: bool) -> String {
    let codes = doc.codes.join(", ");
    let mut md = format!("**Marlin reference** · {} ({codes})", doc.title);
    if !doc.brief.is_empty() {
        md.push_str(&format!("\n\n{}", doc.brief));
    }
    // When Klipper runs the command, list what it uses in full and fold what
    // it ignores (G1's A/B/C/U/V/W axes, laser power...) into one line.
    let ignored_by_klipper = |tag: &str| {
        klipper_handles && klipper_params.is_some_and(|known| !known.iter().any(|k| k.eq_ignore_ascii_case(tag)))
    };
    let (ignored, shown): (Vec<_>, Vec<_>) = doc.params.iter().partition(|p| ignored_by_klipper(&p.tag));
    if !shown.is_empty() {
        md.push_str("\n\n**Parameters**");
        for p in shown.iter().take(MAX_MARLIN_PARAMS) {
            md.push_str(&format!("\n- `{}`", p.tag));
            if let Some(value) = &p.value {
                md.push_str(&format!(" `{value}`"));
            }
            if !p.description.is_empty() {
                md.push_str(&format!(" — {}", p.description));
            }
        }
        if shown.len() > MAX_MARLIN_PARAMS {
            md.push_str(&format!("\n- …and {} more", shown.len() - MAX_MARLIN_PARAMS));
        }
    }
    if !ignored.is_empty() {
        let tags = ignored.iter().map(|p| format!("`{}`", p.tag)).collect::<Vec<_>>().join(", ");
        md.push_str(&format!("\n\nIgnored by Klipper: {tags}"));
    }
    md.push_str(&format!("\n\n[marlinfw.org]({})", doc.url));
    md
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Each call writes its own folder (tests run in parallel threads, so a
    /// shared one could be read mid-rewrite) and removes it once loaded:
    /// `load` reads every file into memory.
    fn marlin() -> MarlinDocs {
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("klipper-ls-marlin-{}-{call}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("M109.md"),
            "---\ntitle: Heat and Wait\nbrief: Wait.\ncodes: [ M109 ]\nparameters:\n\
             - tag: S\n  description: Target.\n- tag: R\n  description: Cool too.\n---\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("M900.md"),
            "---\ntitle: Linear Advance Factor\nbrief: K.\ncodes: [ M900 ]\nparameters:\n- tag: K\n  description: K factor.\n---\n",
        )
        .unwrap();
        let docs = MarlinDocs::load(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        docs
    }

    fn klipper() -> KlipperDocs {
        KlipperDocs::from_strings(
            "## G-Code commands\n\n- Heat the extruder and block: `M109 [T<index>] S<temperature>`\n",
            "",
            Path::new("G-Codes.md"),
        )
    }

    #[test]
    fn marks_parameters_klipper_ignores() {
        let (k, m) = (klipper(), marlin());
        let sources = Sources { klipper: Some(&k), marlin: Some(&m) };
        let parts = sources.command("M109", false);
        assert_eq!(parts.len(), 2);
        assert!(parts[1].contains("`S` — Target."));
        assert!(!parts[1].contains("`R` — Cool too."), "ignored params are folded");
        assert!(parts[1].contains("Ignored by Klipper: `R`"));

        let r = sources.parameter("M109", "r").join("");
        assert!(r.contains("**Ignored by Klipper**, whose `M109` takes `T`, `S`"));
        let s = sources.parameter("M109", "S").join("");
        assert!(s.contains("Used by Klipper."));
    }

    #[test]
    fn warns_about_codes_klipper_skips() {
        let m = marlin();
        // With Klipper's source to check, the verdict is definite.
        let k = klipper().with_handlers(&["M109", "G21"]);
        let sources = Sources { klipper: Some(&k), marlin: Some(&m) };
        let parts = sources.command("M900", false);
        assert!(parts[0].contains("not handled by Klipper"), "{parts:?}");
        assert!(parts[1].contains("Linear Advance Factor"));
        // A macro takes over, so no warning.
        assert!(!sources.command("M900", true).join("").contains("not handled"));
        // Handled in source but undocumented.
        assert!(sources.command("G21", false)[0].contains("handled by Klipper, not in its docs"));
        assert_eq!(sources.command_location("G21"), Some((PathBuf::from("gcode_move.py"), 165)));

        // Docs only (downloaded, no source): hedge.
        let k = klipper();
        let docs_only = Sources { klipper: Some(&k), marlin: Some(&m) };
        let parts = docs_only.command("M900", false);
        assert!(parts[0].contains("not in Klipper's docs") && parts[0].contains("most likely"));
        // Without Klipper's docs at all, claim nothing.
        let marlin_only = Sources { klipper: None, marlin: Some(&m) };
        assert!(!marlin_only.command("M900", false).join("").contains("Klipper replies"));
    }

    #[test]
    fn standard_codes() {
        assert!(is_standard_code("G1") && is_standard_code("m109") && is_standard_code("T0"));
        assert!(!is_standard_code("G") && !is_standard_code("GCODE") && !is_standard_code("TEMPERATURE_WAIT"));
    }
}

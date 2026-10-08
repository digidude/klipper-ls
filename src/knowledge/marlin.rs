//! Marlin's G-code reference, for parameter-level detail on standard G/M codes.
//!
//! Source: github.com/MarlinFirmware/MarlinDocumentation (GPL-3.0). Each
//! `_gcode/*.md` file starts with a YAML header listing the codes it covers,
//! a one-line summary and every parameter with its type and description.
//! Like Klipper's docs, it is read from disk at runtime (a local checkout or a
//! one-time download into the cache) and never compiled into the binary.
//!
//! Marlin describes Marlin. Klipper implements a subset of these codes and
//! ignores parameters it doesn't know, so the hover layers this under
//! Klipper's own entry and marks what Klipper ignores (see `super`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use yaml_rust2::{Yaml, YamlLoader};

const TREE_API: &str =
    "https://api.github.com/repos/MarlinFirmware/MarlinDocumentation/git/trees/master?recursive=1";
const RAW: &str = "https://raw.githubusercontent.com/MarlinFirmware/MarlinDocumentation/master";
const SITE: &str = "https://marlinfw.org";
/// Written last, so a half-finished download is never mistaken for a cache.
const COMPLETE: &str = ".complete";
const DOWNLOAD_THREADS: usize = 8;

#[derive(Debug, Clone)]
pub struct MarlinParam {
    pub tag: String,
    /// `temp: float`
    pub value: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct MarlinDoc {
    pub codes: Vec<String>,
    pub title: String,
    pub brief: String,
    pub params: Vec<MarlinParam>,
    /// Page on marlinfw.org.
    pub url: String,
    /// Local file, for go-to-definition.
    pub path: PathBuf,
}

#[derive(Debug, Default)]
pub struct MarlinDocs {
    by_code: HashMap<String, Arc<MarlinDoc>>,
}

impl MarlinDocs {
    /// Loads every `*.md` in `dir` (the `_gcode` folder).
    pub fn load(dir: &Path) -> std::io::Result<Self> {
        let mut docs = MarlinDocs::default();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "md") {
                let text = fs::read_to_string(&path)?;
                if let Some(doc) = parse(&text, &path) {
                    let doc = Arc::new(doc);
                    for code in &doc.codes {
                        docs.by_code.insert(code.clone(), doc.clone());
                    }
                }
            }
        }
        Ok(docs)
    }

    pub fn get(&self, code: &str) -> Option<&MarlinDoc> {
        self.by_code.get(&code.to_ascii_uppercase()).map(Arc::as_ref)
    }

    pub fn len(&self) -> usize {
        self.by_code.len()
    }
}

/// Marlin's text is written for its website: `<br/>` line breaks and
/// site-relative links.
fn clean(text: &str) -> String {
    text.replace("<br/>", " ")
        .replace("<br>", " ")
        .replace("](//", "](https://")
        .replace("](/", &format!("]({SITE}/"))
        .trim()
        .to_string()
}

/// Parameter descriptions become list items, where a blank line would end
/// the list; flatten them onto one line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn as_text(yaml: &Yaml) -> Option<String> {
    match yaml {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

pub fn parse(text: &str, path: &Path) -> Option<MarlinDoc> {
    let rest = text.strip_prefix("---")?;
    let header = &rest[..rest.find("\n---")?];
    let yaml = YamlLoader::load_from_str(header).ok()?.into_iter().next()?;

    let codes: Vec<String> = yaml["codes"]
        .as_vec()?
        .iter()
        .filter_map(as_text)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if codes.is_empty() {
        return None;
    }

    let params = yaml["parameters"]
        .as_vec()
        .map(|list| {
            list.iter()
                .filter_map(|p| {
                    let tag = as_text(&p["tag"])?;
                    let value = p["values"].as_vec().and_then(|values| {
                        let v = values.first()?;
                        match (as_text(&v["tag"]), as_text(&v["type"])) {
                            (Some(t), Some(ty)) => Some(format!("{t}: {ty}")),
                            (Some(t), None) => Some(t),
                            (None, Some(ty)) => Some(ty),
                            (None, None) => None,
                        }
                    });
                    Some(MarlinParam {
                        tag,
                        value,
                        description: as_text(&p["description"]).map(|d| one_line(&clean(&d))).unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let stem = path.file_stem()?.to_string_lossy();
    Some(MarlinDoc {
        codes,
        title: as_text(&yaml["title"]).unwrap_or_default(),
        brief: as_text(&yaml["brief"]).map(|b| clean(&b)).unwrap_or_default(),
        params,
        url: format!("{SITE}/docs/gcode/{stem}.html"),
        path: path.to_path_buf(),
    })
}

pub fn is_cached(dir: &Path) -> bool {
    dir.join(COMPLETE).is_file()
}

fn get(url: &str) -> Result<String, String> {
    ureq::get(url)
        .header("User-Agent", "klipper-ls")
        .call()
        .and_then(|mut response| response.body_mut().read_to_string())
        .map_err(|e| format!("{url}: {e}"))
}

/// Fetches `_gcode/*.md` (about 250 small files, ~360 KB) into `dir`.
/// One GitHub API call lists them; the files come from raw.githubusercontent.com
/// on a few threads. Delete `dir` to refresh.
pub fn download(dir: &Path) -> Result<(), String> {
    if is_cached(dir) {
        return Ok(());
    }
    fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;

    let listing: serde_json::Value =
        serde_json::from_str(&get(TREE_API)?).map_err(|e| format!("listing Marlin docs: {e}"))?;
    let files: Vec<String> = listing["tree"]
        .as_array()
        .ok_or("listing Marlin docs: unexpected response")?
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .filter(|p| p.starts_with("_gcode/") && p.ends_with(".md"))
        .map(str::to_string)
        .collect();
    if files.is_empty() {
        return Err("listing Marlin docs: no _gcode files found".into());
    }

    let queue = Arc::new(Mutex::new(files));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let workers: Vec<_> = (0..DOWNLOAD_THREADS)
        .map(|_| {
            let (queue, errors, dir) = (queue.clone(), errors.clone(), dir.to_path_buf());
            thread::spawn(move || {
                // Take the next file inside a closure so the mutex guard is
                // dropped before the download. Written inline in the
                // `while let`, the guard would live through the loop body
                // and serialize all eight threads.
                let next_file = || queue.lock().unwrap().pop();
                while let Some(file) = next_file() {
                    let name = file.trim_start_matches("_gcode/");
                    let result = get(&format!("{RAW}/{file}"))
                        .and_then(|body| fs::write(dir.join(name), body).map_err(|e| e.to_string()));
                    if let Err(e) = result {
                        errors.lock().unwrap().push(e);
                    }
                }
            })
        })
        .collect();
    for worker in workers {
        let _ = worker.join();
    }

    let errors = errors.lock().unwrap();
    if let Some(first) = errors.first() {
        return Err(format!("{} Marlin doc downloads failed, e.g. {first}", errors.len()));
    }
    fs::write(dir.join(COMPLETE), "").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const M109: &str = "---
tag: m0109
title: Heat and Wait
brief: Heat the nozzle and block until it gets there
codes: [ M109 ]
related: [ M104 ]

notes:
- To heat without blocking, use [`M104`](/docs/gcode/M104.html).

parameters:

- tag: S
  optional: true
  description: 'Goal (heating only).<br/>With `AUTOTEMP` this also sets the lower bound.'
  values:
  - tag: temp
    type: float

- tag: R
  optional: true
  description: Goal, whichever direction.
  values:
  - tag: temp
    type: float
---

The body.
";

    #[test]
    fn parses_the_yaml_header() {
        let doc = parse(M109, Path::new("/x/_gcode/M109.md")).unwrap();
        assert_eq!(doc.codes, vec!["M109"]);
        assert_eq!(doc.title, "Heat and Wait");
        assert_eq!(doc.url, "https://marlinfw.org/docs/gcode/M109.html");
        assert_eq!(doc.params.len(), 2);
        assert_eq!(doc.params[0].tag, "S");
        assert_eq!(doc.params[0].value.as_deref(), Some("temp: float"));
        assert!(doc.params[0].description.contains("only). With `AUTOTEMP`"));
    }

    #[test]
    fn links_and_paragraphs_are_hover_safe() {
        assert_eq!(
            clean("See [a](/docs/gcode/M104.html) and [b](//linuxcnc.org/x)."),
            "See [a](https://marlinfw.org/docs/gcode/M104.html) and [b](https://linuxcnc.org/x)."
        );
        assert_eq!(one_line("Rate.\n\nBy default  it is mm/min."), "Rate. By default it is mm/min.");
    }

    #[test]
    fn files_without_a_header_are_skipped() {
        assert!(parse("# just markdown", Path::new("x.md")).is_none());
    }
}

#[cfg(test)]
mod real_docs {
    /// Downloads (if needed) into the real cache and parses everything.
    /// `cargo test marlin_download -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn marlin_download() {
        let dir = crate::knowledge::cache_root().unwrap().join("marlin");
        let started = std::time::Instant::now();
        super::download(&dir).unwrap();
        let docs = super::MarlinDocs::load(&dir).unwrap();
        println!("{} codes from {} in {:?}", docs.len(), dir.display(), started.elapsed());
        for code in ["G0", "G1", "G28", "M104", "M109", "M140", "M190", "M106", "M204", "M900", "M73", "M486", "T0"] {
            let doc = docs.get(code).unwrap_or_else(|| panic!("{code}"));
            assert!(!doc.title.is_empty(), "{code}");
        }
        let unparsed = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
            .filter(|e| super::parse(&std::fs::read_to_string(e.path()).unwrap(), &e.path()).is_none())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        println!("files without a usable header: {unparsed:?}");
    }
}

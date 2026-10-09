//! Request/notification handling and per-session state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use lsp_server::{ErrorCode, Notification, Request, Response};
use lsp_types::notification::PublishDiagnostics;
use lsp_types::PublishDiagnosticsParams;
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
};
use lsp_types::request::{
    GotoDefinition, HoverRequest, Request as _, SemanticTokensFullRequest, SemanticTokensRangeRequest,
};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    InitializeParams, MarkupContent, MarkupKind, SemanticTokens, SemanticTokensParams,
    SemanticTokensRangeParams, SemanticTokensRangeResult, SemanticTokensResult,
    TextDocumentContentChangeEvent, TextDocumentPositionParams, Url,
};
use serde_json::Value;
use tree_sitter::Tree;

use crate::diagnostics;
use crate::features::{self, Context, Target};
use crate::gcode;
use crate::highlight;
use crate::index::{self, Index};
use crate::knowledge::marlin::MarlinDocs;
use crate::knowledge::{KlipperDocs, MarlinSlot, Sources, klipper};
use crate::position::LineIndex;
use crate::syntax;

#[derive(Debug, Clone, Copy, PartialEq)]
enum DocKind {
    /// printer.cfg and friends: parsed with the tree-sitter grammar.
    Klipper,
    /// Slicer output: no parse, line-at-a-time lookups.
    Gcode,
}

impl DocKind {
    fn detect(language_id: &str, uri: &Url) -> Self {
        let gcode_extension = Path::new(uri.path())
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "gcode" | "gco" | "g"));
        // "gcode" here, "klipper-gcode" from the dannymcgee.klipper extension.
        if language_id.ends_with("gcode") || (!language_id.starts_with("klipper") && gcode_extension) {
            DocKind::Gcode
        } else {
            DocKind::Klipper
        }
    }
}

struct Document {
    kind: DocKind,
    text: String,
    lines: LineIndex,
    tree: Option<Tree>,
}

impl Document {
    fn new(kind: DocKind, text: String) -> Self {
        let mut doc = Self { kind, lines: LineIndex::new(&text), text, tree: None };
        doc.reparse();
        doc
    }

    fn reparse(&mut self) {
        if self.kind == DocKind::Klipper {
            self.tree = Some(syntax::parse(&self.text));
        }
    }

    /// Incremental sync: the editor sends only what changed, which matters
    /// for 50 MB G-code files.
    fn apply(&mut self, changes: Vec<TextDocumentContentChangeEvent>) {
        for change in changes {
            match change.range {
                Some(range) => {
                    let start = self.lines.offset(&self.text, range.start);
                    let end = self.lines.offset(&self.text, range.end).max(start);
                    self.text.replace_range(start..end, &change.text);
                }
                None => self.text = change.text,
            }
            self.lines = LineIndex::new(&self.text);
        }
        self.reparse();
    }

    fn target_at(&self, offset: usize) -> Option<Target> {
        match (&self.tree, self.kind) {
            (Some(tree), DocKind::Klipper) => features::target_at(tree, &self.text, offset),
            _ => gcode::target_at(&self.text, offset),
        }
    }
}

pub struct Server {
    documents: HashMap<Url, Document>,
    workspace_roots: Vec<PathBuf>,
    /// `initialization_options.klipperDocs`
    klipper_docs_dir: Option<PathBuf>,
    /// `initialization_options.klipperConfig`: printer.cfg for .gcode files
    klipper_config: Option<PathBuf>,
    /// `initialization_options.downloadDocs` (default true)
    download_docs: bool,
    /// `initialization_options.diagnostics` (default true)
    diagnostics: bool,
    loaded_klipper_docs: HashMap<PathBuf, Rc<KlipperDocs>>,
    /// Set after a failed download so we don't retry on every hover.
    klipper_docs_unavailable: bool,
    marlin: MarlinSlot,
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

impl Server {
    pub fn new(params: InitializeParams) -> Self {
        let options = params.initialization_options.unwrap_or(Value::Null);
        let path_option = |name: &str| options.get(name).and_then(Value::as_str).map(expand_home);
        let download_docs = options
            .get("downloadDocs")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let diagnostics = options.get("diagnostics").and_then(Value::as_bool).unwrap_or(true);

        let mut workspace_roots: Vec<PathBuf> = params
            .workspace_folders
            .into_iter()
            .flatten()
            .filter_map(|f| f.uri.to_file_path().ok())
            .collect();
        #[allow(deprecated)] // root_uri is the fallback for single-folder clients
        if workspace_roots.is_empty() {
            workspace_roots.extend(params.root_uri.and_then(|u| u.to_file_path().ok()));
        }

        Self {
            documents: HashMap::new(),
            workspace_roots,
            klipper_docs_dir: path_option("klipperDocs"),
            klipper_config: path_option("klipperConfig"),
            download_docs,
            diagnostics,
            loaded_klipper_docs: HashMap::new(),
            klipper_docs_unavailable: false,
            marlin: MarlinSlot::start(path_option("marlinDocs"), download_docs),
        }
    }

    /// Returns the notifications to send back (diagnostics).
    pub fn handle_notification(&mut self, notification: Notification) -> Vec<Notification> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                if let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(notification.params) {
                    let doc = p.text_document;
                    let kind = DocKind::detect(&doc.language_id, &doc.uri);
                    self.documents.insert(doc.uri, Document::new(kind, doc.text));
                    return self.publish_diagnostics();
                }
            }
            DidChangeTextDocument::METHOD => {
                if let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(notification.params)
                    && let Some(doc) = self.documents.get_mut(&p.text_document.uri)
                {
                    doc.apply(p.content_changes);
                    return self.publish_diagnostics();
                }
            }
            DidCloseTextDocument::METHOD => {
                if let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(notification.params) {
                    let closed = self.documents.remove(&p.text_document.uri).is_some_and(|d| d.kind == DocKind::Klipper);
                    let mut out = Vec::new();
                    if closed && self.diagnostics {
                        // Clear what we published, so a closed file doesn't keep its squiggles in the Problems list.
                        out.push(Self::diagnostics_notification(p.text_document.uri, Vec::new()));
                    }
                    return out;
                }
            }
            _ => {}
        }
        Vec::new()
    }

    fn diagnostics_notification(uri: Url, diagnostics: Vec<lsp_types::Diagnostic>) -> Notification {
        Notification::new(
            PublishDiagnostics::METHOD.to_string(),
            PublishDiagnosticsParams::new(uri, diagnostics, None),
        )
    }

    /// Diagnostics for every open config file: an edit in one file can change
    /// what is wrong in another (a macro that vanished, a section removed).
    fn publish_diagnostics(&mut self) -> Vec<Notification> {
        if !self.diagnostics {
            return Vec::new();
        }
        let uris: Vec<Url> = self
            .documents
            .iter()
            .filter(|(_, d)| d.kind == DocKind::Klipper)
            .map(|(u, _)| u.clone())
            .collect();
        let mut out = Vec::new();
        for uri in uris {
            let Ok(path) = uri.to_file_path() else { continue };
            let index = index::build(&path, &self.open_texts());
            let near: Vec<PathBuf> = path.parent().map(Path::to_path_buf).into_iter().collect();
            // Never download docs just to check a file: the first hover does that.
            let docs = self.klipper_docs(&near, false);
            let Some(doc) = self.documents.get(&uri) else { continue };
            let Some(tree) = &doc.tree else { continue };
            let found = diagnostics::config_diagnostics(tree, &doc.text, &doc.lines, &index, docs.as_deref());
            out.push(Self::diagnostics_notification(uri, found));
        }
        out
    }

    pub fn handle_request(&mut self, request: Request) -> Response {
        let id = request.id.clone();
        let result = match request.method.as_str() {
            HoverRequest::METHOD => serde_json::from_value::<HoverParams>(request.params)
                .map(|p| serde_json::to_value(self.hover(p.text_document_position_params))),
            GotoDefinition::METHOD => serde_json::from_value::<GotoDefinitionParams>(request.params)
                .map(|p| serde_json::to_value(self.definition(p.text_document_position_params))),
            SemanticTokensFullRequest::METHOD => serde_json::from_value::<SemanticTokensParams>(request.params)
                .map(|p| serde_json::to_value(self.semantic_tokens(&p.text_document.uri, None).map(SemanticTokensResult::Tokens))),
            SemanticTokensRangeRequest::METHOD => serde_json::from_value::<SemanticTokensRangeParams>(request.params).map(|p| {
                let lines = p.range.start.line..p.range.end.line + 1;
                serde_json::to_value(
                    self.semantic_tokens(&p.text_document.uri, Some(lines)).map(SemanticTokensRangeResult::Tokens),
                )
            }),
            method => {
                return Response::new_err(
                    id,
                    ErrorCode::MethodNotFound as i32,
                    format!("klipper-ls does not handle {method}"),
                );
            }
        };
        match result {
            Ok(Ok(value)) => Response::new_ok(id, value),
            Ok(Err(e)) | Err(e) => Response::new_err(id, ErrorCode::InvalidParams as i32, e.to_string()),
        }
    }

    fn hover(&mut self, params: TextDocumentPositionParams) -> Option<Hover> {
        let request = self.prepare(&params)?;
        let document = self.documents.get(&params.text_document.uri)?;
        let target = document.target_at(request.offset)?;
        let markdown = features::hover_markdown(&request.context(), &target)?;
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: Some(document.lines.range(&document.text, target.start, target.end)),
        })
    }

    /// Highlighting. Config files are parsed already; `.gcode` is only ever
    /// looked at a range of lines at a time, so a full request for one is
    /// answered empty and the client's range requests do the work.
    fn semantic_tokens(&self, uri: &Url, lines: Option<std::ops::Range<u32>>) -> Option<SemanticTokens> {
        let doc = self.documents.get(uri)?;
        let data = match (doc.kind, &doc.tree) {
            (DocKind::Klipper, Some(tree)) => highlight::config_tokens(tree, &doc.text, &doc.lines, lines),
            (DocKind::Gcode, _) => match lines {
                Some(range) => highlight::gcode_tokens(&doc.text, &doc.lines, range),
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        Some(SemanticTokens { result_id: None, data })
    }

    fn definition(&mut self, params: TextDocumentPositionParams) -> Option<GotoDefinitionResponse> {
        let request = self.prepare(&params)?;
        let document = self.documents.get(&params.text_document.uri)?;
        let target = document.target_at(request.offset)?;
        let locations = features::definition(&request.context(), &target);
        (!locations.is_empty()).then_some(GotoDefinitionResponse::Array(locations))
    }

    /// Everything a hover or definition needs, gathered up front so the
    /// lookups themselves only borrow immutably.
    fn prepare(&mut self, params: &TextDocumentPositionParams) -> Option<Prepared> {
        let uri = &params.text_document.uri;
        let path = uri.to_file_path().ok()?;
        let document = self.documents.get(uri)?;
        let offset = document.lines.offset(&document.text, params.position);
        let kind = document.kind;

        let printer_cfg = match kind {
            DocKind::Gcode => index::printer_cfg_for(&path, self.klipper_config.as_deref()),
            DocKind::Klipper => None,
        };
        let index = match (&printer_cfg, kind) {
            (Some(cfg), _) => index::build_from(cfg, None, &self.open_texts()),
            (None, DocKind::Klipper) => index::build(&path, &self.open_texts()),
            (None, DocKind::Gcode) => Index::default(),
        };
        let mut doc_search: Vec<PathBuf> = path.parent().map(Path::to_path_buf).into_iter().collect();
        doc_search.extend(printer_cfg.as_deref().and_then(Path::parent).map(Path::to_path_buf));
        let klipper_docs = self.klipper_docs(&doc_search, true);
        let marlin = self.marlin.get();
        Some(Prepared { path, offset, index, klipper_docs, marlin })
    }

    fn open_texts(&self) -> HashMap<PathBuf, &str> {
        self.documents
            .iter()
            .filter(|(_, doc)| doc.kind == DocKind::Klipper)
            .filter_map(|(uri, doc)| {
                let path = uri.to_file_path().ok()?;
                Some((path.canonicalize().unwrap_or(path), doc.text.as_str()))
            })
            .collect()
    }

    fn klipper_docs(&mut self, near: &[PathBuf], may_download: bool) -> Option<Rc<KlipperDocs>> {
        let mut starts = near.to_vec();
        starts.extend(self.workspace_roots.iter().cloned());

        let dir = match klipper::find_local(self.klipper_docs_dir.as_deref(), &starts) {
            Some(dir) => dir,
            None if self.klipper_docs_dir.is_some() => {
                self.warn_once(&format!(
                    "klipperDocs is set, but {} has no G-Codes.md / Config_Reference.md",
                    self.klipper_docs_dir.as_ref().unwrap().display()
                ));
                return None;
            }
            None if may_download && self.download_docs && !self.klipper_docs_unavailable => {
                let cache = klipper::cache_dir()?;
                match klipper::download(&cache) {
                    Ok(dir) => dir,
                    Err(e) => {
                        self.warn_once(&format!("couldn't get Klipper docs: {e}"));
                        return None;
                    }
                }
            }
            None => return None,
        };

        if let Some(docs) = self.loaded_klipper_docs.get(&dir) {
            return Some(docs.clone());
        }
        match KlipperDocs::load(&dir) {
            Ok(docs) => {
                eprintln!("klipper-ls: using Klipper docs from {}", dir.display());
                let docs = Rc::new(docs);
                self.loaded_klipper_docs.insert(dir, docs.clone());
                Some(docs)
            }
            Err(e) => {
                self.warn_once(&format!("reading Klipper docs in {}: {e}", dir.display()));
                None
            }
        }
    }

    fn warn_once(&mut self, message: &str) {
        if !self.klipper_docs_unavailable {
            eprintln!("klipper-ls: {message}; hover will only show your own macros");
            self.klipper_docs_unavailable = true;
        }
    }
}

struct Prepared {
    path: PathBuf,
    offset: usize,
    index: Index,
    klipper_docs: Option<Rc<KlipperDocs>>,
    marlin: Option<Arc<MarlinDocs>>,
}

impl Prepared {
    fn context(&self) -> Context<'_> {
        Context {
            path: &self.path,
            index: &self.index,
            sources: Sources {
                klipper: self.klipper_docs.as_deref(),
                marlin: self.marlin.as_deref(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_document_kinds_across_languages() {
        let uri = |p: &str| Url::parse(&format!("file:///tmp/{p}")).unwrap();
        assert_eq!(DocKind::detect("gcode", &uri("a.gcode")), DocKind::Gcode);
        assert_eq!(DocKind::detect("klipper-gcode", &uri("a.gcode")), DocKind::Gcode);
        assert_eq!(DocKind::detect("klipper", &uri("printer.cfg")), DocKind::Klipper);
        assert_eq!(DocKind::detect("klipper-cfg", &uri("printer.cfg")), DocKind::Klipper);
        assert_eq!(DocKind::detect("plaintext", &uri("a.gco")), DocKind::Gcode);
    }
}

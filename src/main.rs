//! klipper-ls: a language server for Klipper printer config.
//!
//! - Hover a command (`M140`, `QUAD_GANTRY_LEVEL`) for Klipper's own docs,
//!   or one of your macros for its description, parameters and variables.
//! - Hover a section or option for its entry in Klipper's config reference.
//! - Go to definition jumps from a macro call to its `[gcode_macro]`, from
//!   `[include]` to the file, and from built-ins into the reference docs.
//! - The same works in `.gcode` files, where standard codes also get Marlin's
//!   per-parameter reference, marked with what Klipper ignores.
//!
//! It speaks LSP over stdio and logs to stderr.

mod features;
mod gcode;
mod highlight;
mod index;
mod knowledge;
mod position;
mod server;
mod syntax;

use std::error::Error;

use lsp_server::{Connection, Message};
use lsp_types::{
    HoverProviderCapability, InitializeParams, OneOf, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensServerCapabilities, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
};

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    if std::env::args().any(|a| a == "--version") {
        println!("klipper-ls {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let (connection, io_threads) = Connection::stdio();
    let capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::INCREMENTAL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
            legend: highlight::legend(),
            range: Some(true),
            full: Some(SemanticTokensFullOptions::Bool(true)),
            ..SemanticTokensOptions::default()
        })),
        ..ServerCapabilities::default()
    })?;
    let params: InitializeParams = serde_json::from_value(connection.initialize(capabilities)?)?;
    let mut server = server::Server::new(params);

    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                let response = server.handle_request(request);
                connection.sender.send(Message::Response(response))?;
            }
            Message::Notification(notification) => server.handle_notification(notification),
            Message::Response(_) => {}
        }
    }
    // The writer thread runs until every sender is gone; joining while we
    // still hold one would wait forever.
    drop(connection);
    io_threads.join()?;
    Ok(())
}

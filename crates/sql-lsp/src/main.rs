use lsp_server::Connection;
use lsp_types::{
    InitializeResult, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind,
};
use serde_json::json;

mod formatter;
mod handlers;
mod ir;
mod linter;
mod parser;

fn main() {
    let (connection, io_threads) = Connection::stdio();

    let (initialize_id, _initialize_params) = connection
        .initialize_start()
        .expect("failed to receive initialize request");

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        document_formatting_provider: Some(lsp_types::OneOf::Left(true)),
        ..Default::default()
    };

    let initialize_result = InitializeResult {
        capabilities,
        server_info: Some(ServerInfo {
            name: "sql-tools".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
    };

    connection
        .initialize_finish(initialize_id, json!(initialize_result))
        .expect("failed to send initialize response");

    handlers::run(&connection);

    io_threads.join().expect("io threads failed");
}

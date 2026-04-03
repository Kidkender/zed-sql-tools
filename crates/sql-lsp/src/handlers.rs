use lsp_server::{Connection, Message, Notification, Response};
use lsp_types::{DiagnosticSeverity, PublishDiagnosticsParams, Uri};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type DocumentStore = Arc<Mutex<HashMap<Uri, String>>>;

pub fn run(connection: &Connection) {
    let documents: DocumentStore = Arc::new(Mutex::new(HashMap::new()));

    let (lint_tx, lint_rx) = std::sync::mpsc::channel::<(Uri, String)>();
    let lsp_sender = connection.sender.clone();
    std::thread::spawn(move || lint_worker(lint_rx, lsp_sender));

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req).expect("shutdown failed") {
                    return;
                }
                let resp = handle_request(req, &documents);
                connection.sender.send(Message::Response(resp)).ok();
            }
            Message::Notification(notif) => {
                handle_notification(notif, &documents, &lint_tx);
            }
            Message::Response(_) => {}
        }
    }
}

// debounces lint requests so we don't run on every keystroke
fn lint_worker(
    rx: std::sync::mpsc::Receiver<(Uri, String)>,
    sender: crossbeam_channel::Sender<Message>,
) {
    let debounce = Duration::from_millis(300);
    let mut pending: Option<(Uri, String)> = None;
    let mut deadline: Option<Instant> = None;

    loop {
        let timeout = deadline
            .map(|d| d.saturating_duration_since(Instant::now()).max(Duration::ZERO))
            .unwrap_or(Duration::from_secs(60));

        match rx.recv_timeout(timeout) {
            Ok((uri, text)) => {
                pending = Some((uri, text));
                deadline = Some(Instant::now() + debounce);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some((uri, text)) = pending.take() {
                    let lsp_diags: Vec<lsp_types::Diagnostic> = crate::linter::lint_sql(&text)
                        .into_iter()
                        .map(|d| lsp_types::Diagnostic {
                            range: lsp_types::Range {
                                start: lsp_types::Position { line: d.line, character: d.col },
                                end: lsp_types::Position { line: d.line, character: d.col + 1 },
                            },
                            severity: Some(if d.is_error {
                                DiagnosticSeverity::ERROR
                            } else {
                                DiagnosticSeverity::WARNING
                            }),
                            message: d.message,
                            source: Some("sql-tools".to_string()),
                            ..Default::default()
                        })
                        .collect();

                    let notif = lsp_server::Notification::new(
                        "textDocument/publishDiagnostics".to_string(),
                        PublishDiagnosticsParams { uri, diagnostics: lsp_diags, version: None },
                    );
                    sender.send(Message::Notification(notif)).ok();
                    deadline = None;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn handle_request(req: lsp_server::Request, documents: &DocumentStore) -> Response {
    match req.method.as_str() {
        "textDocument/formatting" => handle_formatting(req, documents),
        _ => Response::new_err(
            req.id,
            lsp_server::ErrorCode::MethodNotFound as i32,
            format!("method not implemented: {}", req.method),
        ),
    }
}

fn handle_formatting(req: lsp_server::Request, documents: &DocumentStore) -> Response {
    let params: lsp_types::DocumentFormattingParams = match serde_json::from_value(req.params) {
        Ok(p) => p,
        Err(e) => {
            return Response::new_err(
                req.id,
                lsp_server::ErrorCode::InvalidParams as i32,
                e.to_string(),
            )
        }
    };

    let source = {
        let store = documents.lock().unwrap();
        store.get(&params.text_document.uri).cloned().unwrap_or_default()
    };

    if source.trim().is_empty() {
        return Response::new_ok(req.id, serde_json::json!(null));
    }

    let formatted = crate::formatter::format_sql(&source);

    if formatted == source {
        return Response::new_ok(req.id, serde_json::json!(null));
    }

    let end_line = source.lines().count().saturating_sub(1) as u32;
    let end_char = source.lines().last().map(|l| l.len()).unwrap_or(0) as u32;

    let edit = lsp_types::TextEdit {
        range: lsp_types::Range {
            start: lsp_types::Position { line: 0, character: 0 },
            end: lsp_types::Position { line: end_line, character: end_char },
        },
        new_text: formatted,
    };

    Response::new_ok(req.id, serde_json::json!([edit]))
}

fn handle_notification(
    notif: Notification,
    documents: &DocumentStore,
    lint_tx: &std::sync::mpsc::Sender<(Uri, String)>,
) {
    match notif.method.as_str() {
        "textDocument/didOpen" => {
            if let Ok(params) =
                serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(notif.params)
            {
                let uri = params.text_document.uri.clone();
                let text = params.text_document.text.clone();
                documents.lock().unwrap().insert(uri.clone(), text.clone());
                lint_tx.send((uri, text)).ok();
            }
        }
        "textDocument/didChange" => {
            if let Ok(params) =
                serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(notif.params)
            {
                if let Some(change) = params.content_changes.into_iter().last() {
                    let uri = params.text_document.uri.clone();
                    let text = change.text.clone();
                    documents.lock().unwrap().insert(uri.clone(), text.clone());
                    lint_tx.send((uri, text)).ok();
                }
            }
        }
        _ => {}
    }
}

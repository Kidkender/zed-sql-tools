// End-to-end LSP integration test: spawns sql-lsp as a subprocess and
// communicates with it using the JSON-RPC / LSP wire protocol.
//
// Run with:  cargo run -p sql-lsp --bin lsp-test
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn send(stdin: &mut impl Write, msg: &str) {
    let header = format!("Content-Length: {}\r\n\r\n", msg.len());
    stdin.write_all(header.as_bytes()).unwrap();
    stdin.write_all(msg.as_bytes()).unwrap();
    stdin.flush().unwrap();
}

fn recv(reader: &mut impl BufRead) -> String {
    // Read headers until blank line
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(val) = line.strip_prefix("Content-Length: ") {
            content_length = val.parse().unwrap();
        }
    }
    let mut body = vec![0u8; content_length];
    std::io::Read::read_exact(reader, &mut body).unwrap();
    String::from_utf8(body).unwrap()
}

fn main() {
    // Accept binary path as first argument; default to sibling sql-lsp(.exe)
    let binary = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let exe_name = if cfg!(windows) { "sql-lsp.exe" } else { "sql-lsp" };
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join(exe_name)
        });

    let mut child = Command::new(&binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {}", binary.display(), e));

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // ── initialize ────────────────────────────────────────────────────────────
    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}"#,
    );
    let resp = recv(&mut reader);
    assert!(resp.contains("\"result\""), "initialize failed:\n{}", resp);
    println!("✓ initialize");

    // ── initialized notification ──────────────────────────────────────────────
    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
    );

    // ── formatting: short SELECT → single line ────────────────────────────────
    let uri = "file:///test.sql";
    let sql = "select * from users where id = 1";

    send(
        &mut stdin,
        &format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{uri}","languageId":"sql","version":1,"text":{text}}}}}}}"#,
            uri = uri,
            text = serde_json::to_string(sql).unwrap()
        ),
    );

    send(
        &mut stdin,
        &format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/formatting","params":{{"textDocument":{{"uri":"{uri}"}},"options":{{"tabSize":4,"insertSpaces":true}}}}}}"#,
            uri = uri
        ),
    );
    let resp = recv(&mut reader);
    assert!(resp.contains("SELECT * FROM users WHERE id = 1"), "short SELECT wrong:\n{}", resp);
    println!("✓ format: short SELECT → single line");

    // ── formatting: multi-column SELECT → indented ────────────────────────────
    let sql2 = "select id, name, email from users";
    send(
        &mut stdin,
        &format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{uri}","version":2}},"contentChanges":[{{"text":{text}}}]}}}}"#,
            uri = uri,
            text = serde_json::to_string(sql2).unwrap()
        ),
    );
    send(
        &mut stdin,
        &format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"textDocument/formatting","params":{{"textDocument":{{"uri":"{uri}"}},"options":{{"tabSize":4,"insertSpaces":true}}}}}}"#,
            uri = uri
        ),
    );
    let resp = recv(&mut reader);
    assert!(resp.contains("SELECT\\n    id"), "multi-col SELECT wrong:\n{}", resp);
    println!("✓ format: multi-column SELECT → indented");

    // ── formatting: UPDATE always multiline ───────────────────────────────────
    let sql3 = "update users set name = 'x' where id = 1";
    send(
        &mut stdin,
        &format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{uri}","version":3}},"contentChanges":[{{"text":{text}}}]}}}}"#,
            uri = uri,
            text = serde_json::to_string(sql3).unwrap()
        ),
    );
    send(
        &mut stdin,
        &format!(
            r#"{{"jsonrpc":"2.0","id":4,"method":"textDocument/formatting","params":{{"textDocument":{{"uri":"{uri}"}},"options":{{"tabSize":4,"insertSpaces":true}}}}}}"#,
            uri = uri
        ),
    );
    let resp = recv(&mut reader);
    assert!(resp.contains("UPDATE users\\nSET"), "UPDATE not multiline:\n{}", resp);
    println!("✓ format: UPDATE always multiline");

    // ── linting: UPDATE without WHERE → warning ───────────────────────────────
    let sql4 = "update users set name = 'x'";
    send(
        &mut stdin,
        &format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{uri}","version":4}},"contentChanges":[{{"text":{text}}}]}}}}"#,
            uri = uri,
            text = serde_json::to_string(sql4).unwrap()
        ),
    );
    // Diagnostics are pushed via publishDiagnostics notification — read it
    let notif = recv(&mut reader);
    assert!(
        notif.contains("publishDiagnostics"),
        "expected publishDiagnostics:\n{}",
        notif
    );
    assert!(
        notif.contains("warning") || notif.contains("2"),  // DiagnosticSeverity::WARNING = 2
        "expected warning for UPDATE without WHERE:\n{}",
        notif
    );
    println!("✓ lint: UPDATE without WHERE → warning");

    // ── linting: syntax error → error diagnostic ──────────────────────────────
    let sql5 = "SELECT FROM";
    send(
        &mut stdin,
        &format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{uri}","version":5}},"contentChanges":[{{"text":{text}}}]}}}}"#,
            uri = uri,
            text = serde_json::to_string(sql5).unwrap()
        ),
    );
    let notif = recv(&mut reader);
    assert!(
        notif.contains("publishDiagnostics"),
        "expected publishDiagnostics:\n{}",
        notif
    );
    println!("✓ lint: syntax error → error diagnostic");

    // ── shutdown ──────────────────────────────────────────────────────────────
    // lsp-server's handle_shutdown may block waiting for "exit" before
    // returning the shutdown response, so we send both together then
    // close stdin (EOF) to let the server's IO reader thread unblock.
    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":null}"#,
    );
    send(&mut stdin, r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    drop(stdin);
    child.wait().unwrap();
    println!("✓ shutdown");

    println!("\nAll LSP integration tests passed.");
}

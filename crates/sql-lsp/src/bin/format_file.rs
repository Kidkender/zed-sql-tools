/// Run format_sql on a file and print the result to stdout.
/// Usage: cargo run -p sql-lsp --bin format-file -- path/to/file.sql
fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: format-file <file.sql>");
        std::process::exit(1)
    });

    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Cannot read {}: {}", path, e);
        std::process::exit(1)
    });

    let formatted = sql_lsp::formatter::format_sql(&source);
    print!("{}", formatted);
}

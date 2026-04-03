use tree_sitter::Parser;

fn main() {
    let sql = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: ast_dump <sql>");
        std::process::exit(1)
    });

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_sequel::LANGUAGE.into())
        .expect("failed to load SQL grammar");

    let tree = parser.parse(&sql, None).expect("parse failed");
    let root = tree.root_node();

    println!("Input: {:?}", sql);
    println!("Has errors: {}", root.has_error());
    println!();
    print_node(&root, sql.as_bytes(), 0);
}

fn print_node(node: &tree_sitter::Node, source: &[u8], depth: usize) {
    let indent = "  ".repeat(depth);
    let text = node.utf8_text(source).unwrap_or("<invalid utf8>");

    let display_text = if text.len() > 40 {
        format!("{}...", &text[..40])
    } else {
        text.to_string()
    };

    println!(
        "{}{} [{}..{}] {:?}{}",
        indent,
        node.kind(),
        node.start_position().row + 1,
        node.end_position().row + 1,
        display_text,
        if node.is_error() { " ⚠ ERROR" } else { "" },
    );

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        print_node(&child, source, depth + 1);
    }
}

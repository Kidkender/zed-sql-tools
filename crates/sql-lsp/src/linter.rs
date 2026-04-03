use tree_sitter::Node;

pub struct Diagnostic {
    pub line: u32,
    pub col: u32,
    pub message: String,
    pub is_error: bool,
}

pub fn lint_sql(source: &str) -> Vec<Diagnostic> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_sequel::LANGUAGE.into())
        .expect("failed to load SQL grammar");

    let tree = parser.parse(source, None).expect("parse failed");
    let mut diags = vec![];
    walk(&tree.root_node(), source.as_bytes(), &mut diags);
    diags
}

fn walk(node: &Node, src: &[u8], diags: &mut Vec<Diagnostic>) {
    if node.is_error() {
        let raw = node.utf8_text(src).unwrap_or("").trim().to_string();
        let msg = if raw.is_empty() {
            "Syntax error".to_string()
        } else {
            format!("Syntax error near '{}'", raw)
        };
        diags.push(Diagnostic {
            line: node.start_position().row as u32,
            col: node.start_position().column as u32,
            message: msg,
            is_error: true,
        });
        // don't recurse into ERROR nodes, children are garbage
        return;
    }

    match node.kind() {
        "update" => {
            let mut cur = node.walk();
            let has_where = node.children(&mut cur).any(|c| c.kind() == "where");
            if !has_where {
                diags.push(Diagnostic {
                    line: node.start_position().row as u32,
                    col: node.start_position().column as u32,
                    message: "UPDATE without WHERE clause — this will update all rows".to_string(),
                    is_error: false,
                });
            }
        }

        // SELECT FROM users → grammar parses `FROM users` as the expression.
        // catch it by checking if the expression text starts with a clause keyword.
        "select_expression" => {
            let expr = node.utf8_text(src).unwrap_or("").trim().to_uppercase();
            let clause_keywords = ["FROM ", "WHERE ", "GROUP ", "ORDER ", "HAVING ", "LIMIT "];
            if clause_keywords.iter().any(|kw| expr.starts_with(kw)) {
                diags.push(Diagnostic {
                    line: node.start_position().row as u32,
                    col: node.start_position().column as u32,
                    message: "Missing column list in SELECT".to_string(),
                    is_error: true,
                });
            }
        }

        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(&child, src, diags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_where_is_error() {
        let diags = lint_sql("SELECT * FROM users WHERE");
        assert!(!diags.is_empty(), "should detect bare WHERE");
        assert!(diags.iter().any(|d| d.is_error));
    }

    #[test]
    fn test_update_without_where_is_warning() {
        let diags = lint_sql("UPDATE users SET name = 'x'");
        assert!(!diags.is_empty());
        let w = diags.iter().find(|d| !d.is_error).unwrap();
        assert!(w.message.contains("WHERE"));
    }

    #[test]
    fn test_update_with_where_no_warning() {
        let diags = lint_sql("UPDATE users SET name = 'x' WHERE id = 1");
        let warnings: Vec<_> = diags.iter().filter(|d| !d.is_error).collect();
        assert!(warnings.is_empty(), "no warning expected: {:?}", warnings.iter().map(|d| &d.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_select_missing_columns() {
        let diags = lint_sql("SELECT FROM users");
        assert!(!diags.is_empty(), "should detect missing columns");
        assert!(diags.iter().any(|d| d.message.contains("column")));
    }

    #[test]
    fn test_valid_select_no_errors() {
        let diags = lint_sql("SELECT id, name FROM users WHERE id = 1");
        assert!(diags.is_empty(), "should be clean: {:?}", diags.iter().map(|d| &d.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_valid_insert_no_errors() {
        let diags = lint_sql("INSERT INTO users (name) VALUES ('test')");
        assert!(diags.is_empty());
    }
}

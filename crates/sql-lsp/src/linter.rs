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
        // Grammar limitation: `LIMIT $n OFFSET $n` with parameters is not parsed
        // by tree-sitter-sequel and produces a false positive ERROR node. Suppress
        // it — the SQL is valid; only the grammar can't represent it.
        let mut cur = node.walk();
        let starts_with_limit = node
            .children(&mut cur)
            .next()
            .map(|c| c.kind() == "keyword_limit")
            .unwrap_or(false);
        if starts_with_limit {
            return;
        }

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

        // DELETE FROM users → `where` is inside the `from` child, not `delete` directly.
        "statement" => {
            let mut cur = node.walk();
            let children: Vec<_> = node.children(&mut cur).collect();
            let is_delete = children.iter().any(|c| c.kind() == "delete");
            if is_delete {
                let has_where = children.iter().any(|c| {
                    if c.kind() == "from" {
                        let mut cur2 = c.walk();
                        c.children(&mut cur2).any(|gc| gc.kind() == "where")
                    } else {
                        false
                    }
                });
                if !has_where {
                    diags.push(Diagnostic {
                        line: node.start_position().row as u32,
                        col: node.start_position().column as u32,
                        message: "DELETE without WHERE clause — this will delete all rows"
                            .to_string(),
                        is_error: false,
                    });
                }
            }
        }

        // SELECT * → warn to use explicit columns
        "all_fields" => {
            diags.push(Diagnostic {
                line: node.start_position().row as u32,
                col: node.start_position().column as u32,
                message: "Avoid SELECT *, specify columns explicitly".to_string(),
                is_error: false,
            });
        }

        // LIMIT without ORDER BY → pagination gives inconsistent results
        "from" => {
            let mut cur = node.walk();
            let children: Vec<_> = node.children(&mut cur).collect();
            let has_limit = children.iter().any(|c| c.kind() == "limit");
            let has_order = children.iter().any(|c| c.kind() == "order_by");
            if has_limit && !has_order {
                let limit_node = children.iter().find(|c| c.kind() == "limit").unwrap();
                diags.push(Diagnostic {
                    line: limit_node.start_position().row as u32,
                    col: limit_node.start_position().column as u32,
                    message: "LIMIT without ORDER BY may produce inconsistent results".to_string(),
                    is_error: false,
                });
            }
        }

        // Subquery in FROM without alias → some DBs require it, always confusing
        "relation" => {
            let mut cur = node.walk();
            let children: Vec<_> = node.children(&mut cur).collect();
            let has_subquery = children.iter().any(|c| c.kind() == "subquery");
            let has_alias = children.iter().any(|c| c.kind() == "identifier");
            if has_subquery && !has_alias {
                diags.push(Diagnostic {
                    line: node.start_position().row as u32,
                    col: node.start_position().column as u32,
                    message: "Subquery in FROM must have an alias (AS name)".to_string(),
                    is_error: false,
                });
            }
        }

        // `col = NULL` → should be `col IS NULL`
        // `col IN ()` → empty IN list always false
        // `literal = literal` → always true/false, likely debug code left in
        "binary_expression" => {
            let mut cur = node.walk();
            let children: Vec<_> = node.children(&mut cur).collect();

            let has_eq = children.iter().any(|c| c.kind() == "=");
            let has_null_rhs = children.iter().any(|c| {
                if c.kind() == "literal" {
                    let mut cur2 = c.walk();
                    c.children(&mut cur2).any(|gc| gc.kind() == "keyword_null")
                } else {
                    false
                }
            });
            if has_eq && has_null_rhs {
                diags.push(Diagnostic {
                    line: node.start_position().row as u32,
                    col: node.start_position().column as u32,
                    message: "Use IS NULL / IS NOT NULL instead of = NULL".to_string(),
                    is_error: true,
                });
            }

            let has_in = children.iter().any(|c| c.kind() == "keyword_in");
            let has_empty_list = children.iter().any(|c| {
                if c.kind() == "list" {
                    // list with only `(` and `)` → empty
                    let mut cur2 = c.walk();
                    let items: Vec<_> = c.children(&mut cur2).collect();
                    items.len() == 2
                        && items[0].kind() == "("
                        && items[1].kind() == ")"
                } else {
                    false
                }
            });
            if has_in && has_empty_list {
                diags.push(Diagnostic {
                    line: node.start_position().row as u32,
                    col: node.start_position().column as u32,
                    message: "IN () with empty list — condition is always false".to_string(),
                    is_error: true,
                });
            }

            // `1 = 1`, `'x' = 'x'` → literal compared to literal, likely debug code
            let literal_children: Vec<_> = children.iter().filter(|c| c.kind() == "literal").collect();
            let has_eq_op = children.iter().any(|c| c.kind() == "=");
            if has_eq_op && literal_children.len() == 2 {
                let lhs = literal_children[0].utf8_text(src).unwrap_or("").trim();
                let rhs = literal_children[1].utf8_text(src).unwrap_or("").trim();
                if lhs.eq_ignore_ascii_case(rhs) {
                    diags.push(Diagnostic {
                        line: node.start_position().row as u32,
                        col: node.start_position().column as u32,
                        message: format!(
                            "Condition '{} = {}' is always true — likely debug code",
                            lhs, rhs
                        ),
                        is_error: false,
                    });
                }
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

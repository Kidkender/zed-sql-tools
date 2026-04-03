// SQL formatter implementing smart formatting rules:
//   RULE 1: <= 80 chars, no join/subquery, simple where → single line
//   RULE 2: multiple columns → SELECT with each column indented on its own line
//   RULE 3: simple WHERE (no AND/OR) → inline
//   RULE 4: complex WHERE (AND/OR) → WHERE on its own line, conditions indented
//   RULE 5: UPDATE always multiline
//   RULE 6: INSERT single tuple + short → single line; otherwise multiline
//   RULE 7: subquery → always multiline
//   RULE 8: JOIN → always multiline
//
// Node layout (DerekStride grammar, commit 4a99c73):
//   program  → comment | statement | ";"
//   statement → select + from | update | insert
//   from     → relation, where, order_by, group_by
//   update   → relation, assignment(s), where
use tree_sitter::Node;

const COMPACT_THRESHOLD: usize = 80;

pub fn format_sql(source: &str) -> String {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_sequel::LANGUAGE.into())
        .expect("failed to load SQL grammar");

    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();
    let src = source.as_bytes();

    let mut items: Vec<(String, bool)> = vec![];
    let mut cursor = root.walk();
    let children: Vec<_> = root.children(&mut cursor).collect();

    let mut i = 0;
    while i < children.len() {
        let child = &children[i];
        match child.kind() {
            "statement" => {
                let rendered = format_statement(child, src);
                if !rendered.trim().is_empty() {
                    let has_semi = children
                        .get(i + 1)
                        .map(|n| n.kind() == ";")
                        .unwrap_or(false);
                    items.push((rendered, has_semi));
                }
            }
            "comment" => items.push((text(child, src), false)),
            ";" => {}
            _ => {}
        }
        i += 1;
    }

    let mut out = String::new();
    for (idx, (content, has_semi)) in items.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
            if !content.starts_with("--") {
                out.push('\n');
            }
        }
        out.push_str(content);
        if *has_semi {
            out.push(';');
        }
    }
    out
}

fn format_statement(node: &Node, src: &[u8]) -> String {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();

    for child in &children {
        match child.kind() {
            "select" | "from" => return format_select_stmt(&children, src),
            "update" => return format_update_stmt(child, src),
            "insert" => return format_insert_stmt(child, src),
            _ => {}
        }
    }

    text(node, src)
}

fn format_select_stmt(siblings: &[Node], src: &[u8]) -> String {
    let select_node = siblings.iter().find(|n| n.kind() == "select");
    let from_node = siblings.iter().find(|n| n.kind() == "from");

    // collect columns
    let mut columns: Vec<String> = vec![];
    if let Some(sel) = select_node {
        let mut c = sel.walk();
        for child in sel.children(&mut c) {
            if child.kind() == "select_expression" {
                let mut tc = child.walk();
                for t in child.children(&mut tc) {
                    if t.kind() == "term" {
                        columns.push(text(&t, src));
                    }
                }
                if columns.is_empty() {
                    columns.push(text(&child, src));
                }
            }
        }
    }
    if columns.is_empty() {
        columns.push("*".to_string());
    }

    // collect FROM parts
    let mut table = String::new();
    let mut joins: Vec<String> = vec![];
    let mut where_condition: Option<String> = None;
    let mut order_by_items: Vec<String> = vec![];
    let mut group_by_items: Vec<String> = vec![];

    if let Some(frm) = from_node {
        let mut c = frm.walk();
        for child in frm.children(&mut c) {
            match child.kind() {
                "keyword_from" => {}
                "relation" => table = text(&child, src),
                "where" => {
                    let mut parts: Vec<String> = vec![];
                    let mut wc = child.walk();
                    for wchild in child.children(&mut wc) {
                        if wchild.kind() != "keyword_where" {
                            parts.push(text(&wchild, src));
                        }
                    }
                    where_condition = Some(parts.join(" ").trim().to_string());
                }
                "order_by" => {
                    let mut oc = child.walk();
                    for ochild in child.children(&mut oc) {
                        if ochild.kind() == "order_target" {
                            order_by_items.push(text(&ochild, src));
                        }
                    }
                }
                "group_by" => {
                    let mut gc = child.walk();
                    for gchild in child.children(&mut gc) {
                        match gchild.kind() {
                            "keyword_group" | "keyword_by" => {}
                            _ => group_by_items.push(text(&gchild, src)),
                        }
                    }
                }
                k if k.to_lowercase().contains("join") => {
                    joins.push(text(&child, src));
                }
                _ => {}
            }
        }
    }

    // RULE 2: multiple columns or function call → indented multiline SELECT
    let multi_column =
        columns.len() > 1 || columns.first().map(|c| c.contains('(')).unwrap_or(false);

    // RULE 4: complex WHERE detection
    let complex_where = where_condition
        .as_deref()
        .map(is_complex_condition)
        .unwrap_or(false);

    // RULE 8: JOIN always multiline; RULE 7: subquery always multiline
    let has_join = !joins.is_empty();

    // build SELECT clause
    let select_clause = if multi_column {
        format!("SELECT\n    {}", columns.join(",\n    "))
    } else {
        format!("SELECT {}", columns.join(", "))
    };

    let from_clause = format!("FROM {}", table);

    let where_clause = where_condition.as_ref().map(|w| format_where_clause(w));

    let group_clause = if !group_by_items.is_empty() {
        Some(format!("GROUP BY {}", group_by_items.join(", ")))
    } else {
        None
    };

    let order_clause = if !order_by_items.is_empty() {
        Some(format!("ORDER BY {}", order_by_items.join(", ")))
    } else {
        None
    };

    let mut parts: Vec<String> = vec![select_clause, from_clause];
    parts.extend(joins.iter().cloned());
    if let Some(w) = where_clause {
        parts.push(w);
    }
    if let Some(g) = group_clause {
        parts.push(g);
    }
    if let Some(o) = order_clause {
        parts.push(o);
    }

    // RULE 1: single-line only when query is simple and short
    let single_line_eligible = !multi_column && !has_join && !complex_where;
    let singleline = parts.join(" ");

    if single_line_eligible && singleline.len() <= COMPACT_THRESHOLD {
        singleline
    } else {
        parts.join("\n")
    }
}

// RULE 3/4: format WHERE clause — inline if simple, indented if AND/OR
fn format_where_clause(condition: &str) -> String {
    if !is_complex_condition(condition) {
        return format!("WHERE {}", condition);
    }

    // Replace " AND " / " OR " with newline + indent (case-insensitive, char-by-char)
    let chars: Vec<char> = condition.chars().collect();
    let upper: Vec<char> = condition.to_uppercase().chars().collect();
    let mut out = String::with_capacity(condition.len() + 32);
    let mut i = 0;

    while i < chars.len() {
        if i + 5 <= chars.len() && upper[i..i + 5].iter().collect::<String>() == " AND " {
            out.push_str("\n    AND ");
            i += 5;
        } else if i + 4 <= chars.len() && upper[i..i + 4].iter().collect::<String>() == " OR " {
            out.push_str("\n    OR ");
            i += 4;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    format!("WHERE\n    {}", out.trim())
}

fn is_complex_condition(condition: &str) -> bool {
    let upper = condition.to_uppercase();
    upper.contains(" AND ") || upper.contains(" OR ")
}

// RULE 5: UPDATE always multiline
fn format_update_stmt(node: &Node, src: &[u8]) -> String {
    let mut table = String::new();
    let mut assignments: Vec<String> = vec![];
    let mut where_condition: Option<String> = None;

    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            "keyword_update" | "keyword_set" => {}
            "relation" => table = text(&child, src),
            "assignment" => assignments.push(text(&child, src)),
            "where" => {
                let mut parts: Vec<String> = vec![];
                let mut wc = child.walk();
                for wchild in child.children(&mut wc) {
                    if wchild.kind() != "keyword_where" {
                        parts.push(text(&wchild, src));
                    }
                }
                where_condition = Some(parts.join(" ").trim().to_string());
            }
            _ => {}
        }
    }

    let mut out = format!("UPDATE {}\nSET {}", table, assignments.join(", "));
    if let Some(w) = where_condition {
        out.push('\n');
        out.push_str(&format_where_clause(&w));
    }
    out
}

// RULE 6: INSERT — single tuple + short → single line; otherwise multiline
fn format_insert_stmt(node: &Node, src: &[u8]) -> String {
    let mut table = String::new();
    let mut lists: Vec<String> = vec![];

    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            "keyword_insert" | "keyword_into" | "keyword_values" => {}
            "object_reference" => table = text(&child, src),
            "list" => lists.push(text(&child, src)),
            _ => {}
        }
    }

    let cols = lists.first().cloned().unwrap_or_default();
    let vals = lists.get(1).cloned().unwrap_or_default();

    let singleline = if cols.is_empty() {
        format!("INSERT INTO {} VALUES {}", table, vals)
    } else {
        format!("INSERT INTO {} {} VALUES {}", table, cols, vals)
    };

    if singleline.len() <= COMPACT_THRESHOLD {
        singleline
    } else if cols.is_empty() {
        format!("INSERT INTO {}\nVALUES {}", table, vals)
    } else {
        format!("INSERT INTO {} {}\nVALUES {}", table, cols, vals)
    }
}

fn text(node: &Node, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // RULE 1: short + simple → single line
    #[test]
    fn test_select_star() {
        let out = format_sql("select * from users");
        assert_eq!(out, "SELECT * FROM users");
    }

    // RULE 2: multiple columns → indented multiline SELECT
    #[test]
    fn test_select_columns_where() {
        let out = format_sql("select id, name from users where id = 1");
        assert_eq!(out, "SELECT\n    id,\n    name\nFROM users\nWHERE id = 1");
    }

    // RULE 1: single column, short → single line
    #[test]
    fn test_select_order_by() {
        let out = format_sql("select id from users order by name");
        assert_eq!(out, "SELECT id FROM users ORDER BY name");
    }

    // RULE 5: UPDATE always multiline; RULE 3: simple WHERE inline
    #[test]
    fn test_update_with_where() {
        let out = format_sql("update users set name = 'test' where id = 1");
        assert_eq!(out, "UPDATE users\nSET name = 'test'\nWHERE id = 1");
    }

    // RULE 5: UPDATE always multiline
    #[test]
    fn test_update_without_where() {
        let out = format_sql("update users set name = 'test'");
        assert_eq!(out, "UPDATE users\nSET name = 'test'");
    }

    // RULE 6: short INSERT → single line
    #[test]
    fn test_insert() {
        let out = format_sql("insert into users (name) values ('test')");
        assert_eq!(out, "INSERT INTO users (name) VALUES ('test')");
    }

    #[test]
    fn test_already_uppercase() {
        let out = format_sql("SELECT * FROM users");
        assert_eq!(out, "SELECT * FROM users");
    }

    // RULE 2: 4 columns → multiline with indent
    #[test]
    fn test_multi_column_select() {
        let out = format_sql("select id, name, email, created_at from users");
        assert_eq!(
            out,
            "SELECT\n    id,\n    name,\n    email,\n    created_at\nFROM users"
        );
    }

    // RULE 4: WHERE with AND → indented conditions
    #[test]
    fn test_complex_where() {
        let out = format_sql("select * from users where id = 1 and status = 'active'");
        assert!(
            out.contains("WHERE\n    "),
            "expected indented WHERE: {}",
            out
        );
        assert!(out.contains("AND"), "expected AND: {}", out);
    }

    // RULE 1: single column short → stays 1 line
    #[test]
    fn test_group_by() {
        let out = format_sql("select id from users group by id");
        assert_eq!(out, "SELECT id FROM users GROUP BY id");
    }

    #[test]
    fn test_group_by_and_order_by() {
        let out = format_sql("select id from users group by id order by id");
        assert_eq!(out, "SELECT id FROM users GROUP BY id ORDER BY id");
    }

    #[test]
    fn test_literal_not_uppercased() {
        let out = format_sql("select * from users where name = 'hello world'");
        assert!(
            out.contains("'hello world'"),
            "literal must be preserved: {}",
            out
        );
    }

    #[test]
    fn test_comment_preserved() {
        let out = format_sql("-- find all users\nselect * from users");
        assert!(
            out.starts_with("-- find all users"),
            "comment lost: {}",
            out
        );
        assert!(out.contains("SELECT *"), "statement lost: {}", out);
    }

    #[test]
    fn test_multi_statement() {
        let out = format_sql("select * from users; select id from orders;");
        assert!(out.contains("SELECT *"), "first statement lost: {}", out);
        assert!(out.contains("SELECT id"), "second statement lost: {}", out);
        assert!(out.contains(';'), "semicolons should be preserved: {}", out);
    }
}

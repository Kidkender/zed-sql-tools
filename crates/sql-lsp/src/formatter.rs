// SQL formatter: source → parse → walk AST → Vec<SqlIr> → string
//
// Node layout (DerekStride grammar, commit 4a99c73):
//   program → comment | statement | ";"
//   statement → select + from  (SELECT queries)
//   statement → update         (UPDATE)
//   statement → insert         (INSERT)
//   `from` wraps: relation, where, order_by, group_by
//   `update` wraps: relation, assignment(s), where
use tree_sitter::Node;

use crate::ir::{render, SqlIr};

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
                let rendered = render(&format_statement(child, src));
                if !rendered.trim().is_empty() {
                    let has_semi = children.get(i + 1).map(|n| n.kind() == ";").unwrap_or(false);
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

fn format_statement(node: &Node, src: &[u8]) -> Vec<SqlIr> {
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

    vec![SqlIr::Text(text(node, src))]
}

fn format_select_stmt(siblings: &[Node], src: &[u8]) -> Vec<SqlIr> {
    let mut ir = vec![];

    let select_node = siblings.iter().find(|n| n.kind() == "select");
    let from_node = siblings.iter().find(|n| n.kind() == "from");

    ir.push(SqlIr::Keyword("SELECT".to_string()));
    if let Some(sel) = select_node {
        let mut c = sel.walk();
        for child in sel.children(&mut c) {
            if child.kind() == "select_expression" {
                ir.push(SqlIr::Space);
                ir.extend(format_select_expression(&child, src));
            }
        }
    }

    if let Some(frm) = from_node {
        ir.push(SqlIr::Newline);
        ir.push(SqlIr::Keyword("FROM".to_string()));

        let mut c = frm.walk();
        for child in frm.children(&mut c) {
            match child.kind() {
                "keyword_from" => {}
                "relation" => {
                    ir.push(SqlIr::Space);
                    ir.push(SqlIr::Text(text(&child, src)));
                }
                "where" => ir.extend(format_where(&child, src)),
                "order_by" => ir.extend(format_order_by(&child, src)),
                "group_by" => ir.extend(format_group_by(&child, src)),
                _ => {}
            }
        }
    }

    ir
}

fn format_select_expression(node: &Node, src: &[u8]) -> Vec<SqlIr> {
    // collect term children — handles both `*` and `id, name`
    let mut terms: Vec<String> = vec![];
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() == "term" {
            terms.push(text(&child, src));
        }
    }

    if terms.is_empty() {
        vec![SqlIr::Text(text(node, src))]
    } else {
        vec![SqlIr::Text(terms.join(", "))]
    }
}

fn format_where(node: &Node, src: &[u8]) -> Vec<SqlIr> {
    let mut ir = vec![SqlIr::Newline, SqlIr::Keyword("WHERE".to_string())];
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() != "keyword_where" {
            ir.push(SqlIr::Space);
            ir.push(SqlIr::Text(text(&child, src)));
        }
    }
    ir
}

fn format_order_by(node: &Node, src: &[u8]) -> Vec<SqlIr> {
    let mut ir = vec![SqlIr::Newline, SqlIr::Keyword("ORDER BY".to_string())];
    let mut targets: Vec<String> = vec![];
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() == "order_target" {
            targets.push(text(&child, src));
        }
    }
    if !targets.is_empty() {
        ir.push(SqlIr::Space);
        ir.push(SqlIr::Text(targets.join(", ")));
    }
    ir
}

fn format_group_by(node: &Node, src: &[u8]) -> Vec<SqlIr> {
    // group_by has `field` children, not `order_target` like order_by
    let mut ir = vec![SqlIr::Newline, SqlIr::Keyword("GROUP BY".to_string())];
    let mut targets: Vec<String> = vec![];
    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            "keyword_group" | "keyword_by" => {}
            _ => targets.push(text(&child, src)),
        }
    }
    if !targets.is_empty() {
        ir.push(SqlIr::Space);
        ir.push(SqlIr::Text(targets.join(", ")));
    }
    ir
}

fn format_update_stmt(node: &Node, src: &[u8]) -> Vec<SqlIr> {
    let mut table = String::new();
    let mut assignments: Vec<String> = vec![];
    let mut where_node: Option<Node> = None;

    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            "keyword_update" | "keyword_set" => {}
            "relation" => table = text(&child, src),
            "assignment" => assignments.push(text(&child, src)),
            "where" => where_node = Some(child),
            _ => {}
        }
    }

    let mut ir = vec![
        SqlIr::Keyword("UPDATE".to_string()),
        SqlIr::Space,
        SqlIr::Text(table),
        SqlIr::Newline,
        SqlIr::Keyword("SET".to_string()),
        SqlIr::Space,
        SqlIr::Text(assignments.join(", ")),
    ];

    if let Some(w) = &where_node {
        ir.extend(format_where(w, src));
    }

    ir
}

fn format_insert_stmt(node: &Node, src: &[u8]) -> Vec<SqlIr> {
    let mut table = String::new();
    let mut lists: Vec<String> = vec![]; // [columns, values]

    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            "keyword_insert" | "keyword_into" | "keyword_values" => {}
            "object_reference" => table = text(&child, src),
            "list" => lists.push(text(&child, src)),
            _ => {}
        }
    }

    let mut ir = vec![
        SqlIr::Keyword("INSERT INTO".to_string()),
        SqlIr::Space,
        SqlIr::Text(table),
    ];
    if let Some(cols) = lists.first() {
        ir.push(SqlIr::Space);
        ir.push(SqlIr::Text(cols.clone()));
    }
    ir.push(SqlIr::Newline);
    ir.push(SqlIr::Keyword("VALUES".to_string()));
    if let Some(vals) = lists.get(1) {
        ir.push(SqlIr::Space);
        ir.push(SqlIr::Text(vals.clone()));
    }

    ir
}

fn text(node: &Node, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_star() {
        let out = format_sql("select * from users");
        assert_eq!(out, "SELECT *\nFROM users");
    }

    #[test]
    fn test_select_columns_where() {
        let out = format_sql("select id, name from users where id = 1");
        assert_eq!(out, "SELECT id, name\nFROM users\nWHERE id = 1");
    }

    #[test]
    fn test_select_order_by() {
        let out = format_sql("select id from users order by name");
        assert_eq!(out, "SELECT id\nFROM users\nORDER BY name");
    }

    #[test]
    fn test_update_with_where() {
        let out = format_sql("update users set name = 'test' where id = 1");
        assert_eq!(out, "UPDATE users\nSET name = 'test'\nWHERE id = 1");
    }

    #[test]
    fn test_update_without_where() {
        let out = format_sql("update users set name = 'test'");
        assert_eq!(out, "UPDATE users\nSET name = 'test'");
    }

    #[test]
    fn test_insert() {
        let out = format_sql("insert into users (name) values ('test')");
        assert_eq!(out, "INSERT INTO users (name)\nVALUES ('test')");
    }

    #[test]
    fn test_already_uppercase() {
        let out = format_sql("SELECT * FROM users");
        assert_eq!(out, "SELECT *\nFROM users");
    }

    #[test]
    fn test_literal_not_uppercased() {
        let out = format_sql("select * from users where name = 'hello world'");
        assert!(out.contains("'hello world'"), "literal must be preserved: {}", out);
    }

    #[test]
    fn test_group_by() {
        let out = format_sql("select id from users group by id");
        assert_eq!(out, "SELECT id\nFROM users\nGROUP BY id");
    }

    #[test]
    fn test_group_by_and_order_by() {
        let out = format_sql("select id from users group by id order by id");
        assert_eq!(out, "SELECT id\nFROM users\nGROUP BY id\nORDER BY id");
    }

    #[test]
    fn test_comment_preserved() {
        let out = format_sql("-- find all users\nselect * from users");
        assert!(out.starts_with("-- find all users"), "comment lost: {}", out);
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

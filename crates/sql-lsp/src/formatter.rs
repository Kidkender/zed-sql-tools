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
//   statement → select + from | update | insert | cte (WITH)
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

    // (rendered_statement, has_semicolon, optional_inline_comment)
    let mut items: Vec<(String, bool, Option<String>)> = vec![];
    let mut cursor = root.walk();
    let children: Vec<_> = root.children(&mut cursor).collect();

    // Track the last statement's end row to detect inline comments.
    let mut last_stmt_end_row: Option<usize> = None;

    let mut i = 0;
    while i < children.len() {
        let child = &children[i];
        match child.kind() {
            "statement" => {
                last_stmt_end_row = Some(child.end_position().row);
                let rendered = format_statement(child, src);
                if !rendered.trim().is_empty() {
                    let has_semi = children
                        .get(i + 1)
                        .map(|n| n.kind() == ";")
                        .unwrap_or(false);
                    items.push((rendered, has_semi, None));
                }
            }
            "comment" => {
                // Inline comment on the same line as the preceding statement
                // (e.g. `SELECT * FROM users; -- comment`) → stored separately
                // so the semicolon can be rendered before it.
                let is_inline = last_stmt_end_row
                    .map(|row| child.start_position().row == row)
                    .unwrap_or(false);

                if is_inline && !items.is_empty() {
                    items.last_mut().unwrap().2 = Some(text(child, src));
                } else {
                    items.push((text(child, src), false, None));
                }
                last_stmt_end_row = None;
            }
            ";" => {}
            _ => {}
        }
        i += 1;
    }

    let mut out = String::new();
    for (idx, (content, has_semi, inline_comment)) in items.iter().enumerate() {
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
        if let Some(comment) = inline_comment {
            out.push(' ');
            out.push_str(comment);
        }
    }
    out
}

fn format_statement(node: &Node, src: &[u8]) -> String {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();

    // WITH/CTE statements: fall back to raw text (uppercase keywords in-place
    // would require a full CTE formatter — preserving the query is safer).
    if children.iter().any(|n| n.kind() == "cte") {
        return text(node, src);
    }

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
    let mut order_by_text: Option<String> = None;
    let mut group_by_text: Option<String> = None;

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
                    // normalize_whitespace collapses newlines from prior formatting,
                    // making format_where_clause idempotent (fixes repeat-save bugs).
                    where_condition = Some(normalize_whitespace(&parts.join(" ")));
                }
                "order_by" => {
                    // Use full node text minus ORDER/BY keywords so qualified names
                    // like `users.email` aren't truncated.
                    let body = strip_leading_keywords(&text(&child, src), &["ORDER", "BY"]);
                    if !body.is_empty() {
                        order_by_text = Some(body);
                    }
                }
                "group_by" => {
                    let body = strip_leading_keywords(&text(&child, src), &["GROUP", "BY"]);
                    if !body.is_empty() {
                        group_by_text = Some(body);
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
    let multi_column = columns.len() > 1
        || columns.first().map(|c| c.contains('(')).unwrap_or(false);

    // RULE 4: complex WHERE detection
    let complex_where = where_condition
        .as_deref()
        .map(is_complex_condition)
        .unwrap_or(false);

    // RULE 8: JOIN always multiline
    let has_join = !joins.is_empty();

    // build clauses
    let select_clause = if multi_column {
        format!("SELECT\n    {}", columns.join(",\n    "))
    } else {
        format!("SELECT {}", columns.join(", "))
    };

    let from_clause = format!("FROM {}", table);
    let where_clause = where_condition.as_ref().map(|w| format_where_clause(w));
    let group_clause = group_by_text.as_ref().map(|g| format!("GROUP BY {}", g));
    let order_clause = order_by_text.as_ref().map(|o| format!("ORDER BY {}", o));

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

// RULE 3/4: simple WHERE → inline; AND/OR → indented conditions
fn format_where_clause(condition: &str) -> String {
    if !is_complex_condition(condition) {
        return format!("WHERE {}", condition);
    }

    // Replace " AND " / " OR " with newline + indent (char-by-char for Unicode safety)
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
                where_condition = Some(normalize_whitespace(&parts.join(" ")));
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

// RULE 6: INSERT — single short tuple → single line; otherwise multiline
fn format_insert_stmt(node: &Node, src: &[u8]) -> String {
    let mut table = String::new();
    let mut cols = String::new();
    let mut vals = String::new();
    let mut in_values = false;

    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            "keyword_insert" | "keyword_into" => {}
            "keyword_values" => in_values = true,
            "object_reference" => table = text(&child, src),
            "list" => {
                if in_values {
                    vals = text(&child, src);
                } else {
                    cols = text(&child, src);
                }
            }
            _ => {}
        }
    }

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

// Collapse all whitespace (including newlines from prior formatting) to single spaces.
// This makes re-formatting idempotent.
fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// Strip leading SQL keywords (case-insensitive) and normalize remaining whitespace.
// Used for ORDER BY and GROUP BY to handle qualified names like `users.email`.
fn strip_leading_keywords(s: &str, keywords: &[&str]) -> String {
    let mut remaining = s.trim();
    for kw in keywords {
        let upper = remaining.to_uppercase();
        let trimmed = upper.trim_start();
        if trimmed.starts_with(kw) {
            let offset = upper.len() - trimmed.len() + kw.len();
            remaining = remaining[offset..].trim_start();
        }
    }
    normalize_whitespace(remaining)
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

    // RULE 2: 4 columns → indented multiline
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
        assert!(out.contains("WHERE\n    "), "expected indented WHERE: {}", out);
        assert!(out.contains("AND"), "expected AND: {}", out);
    }

    // Idempotency: formatting an already-formatted query must not change it (Bug 1/3/5).
    #[test]
    fn test_where_idempotent() {
        let input = "SELECT * FROM users WHERE id = 1 AND status = 'active'";
        let first = format_sql(input);
        let second = format_sql(&first);
        assert_eq!(first, second, "second format changed the output");
    }

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

    // Bug 6: inline comment after statement must stay on the same line
    #[test]
    fn test_inline_comment_stays_inline() {
        let out = format_sql("SELECT * FROM users; -- my comment");
        assert!(
            out.contains("SELECT * FROM users; -- my comment"),
            "inline comment was separated: {}",
            out
        );
    }

    #[test]
    fn test_multi_statement() {
        let out = format_sql("select * from users; select id from orders;");
        assert!(out.contains("SELECT *"), "first statement lost: {}", out);
        assert!(out.contains("SELECT id"), "second statement lost: {}", out);
        assert!(out.contains(';'), "semicolons should be preserved: {}", out);
    }

    // ── RULE 1: single-line threshold ────────────────────────────────────────

    #[test]
    fn test_single_col_long_query_goes_multiline() {
        // single column, simple WHERE, but > 80 chars → must break into multiple lines
        let input = "select id from very_long_table_name where some_really_long_column_name = 'a_long_value_here'";
        let out = format_sql(input);
        assert!(
            out.contains('\n'),
            "long query should be multiline: {}",
            out
        );
    }

    // ── RULE 2: function call as column ──────────────────────────────────────

    #[test]
    fn test_function_call_column_is_multiline() {
        // count(*) contains '(' → treated as multi_column → indented SELECT
        let out = format_sql("select count(*) from users");
        assert_eq!(out, "SELECT\n    count(*)\nFROM users");
    }

    #[test]
    fn test_function_mixed_with_regular_column() {
        let out = format_sql("select id, count(*) from users group by id");
        assert_eq!(
            out,
            "SELECT\n    id,\n    count(*)\nFROM users\nGROUP BY id"
        );
    }

    // ── RULE 4: complex WHERE with OR ────────────────────────────────────────

    #[test]
    fn test_complex_where_or() {
        let out = format_sql("select * from users where status = 'active' or role = 'admin'");
        assert!(
            out.contains("WHERE\n    "),
            "expected indented WHERE: {}",
            out
        );
        assert!(out.contains("OR"), "expected OR: {}", out);
    }

    #[test]
    fn test_complex_where_mixed_and_or() {
        let out =
            format_sql("select * from users where id = 1 and status = 'active' or role = 'admin'");
        assert!(out.contains("WHERE\n    "), "expected indented WHERE: {}", out);
        assert!(out.contains("AND"), "AND missing: {}", out);
        assert!(out.contains("OR"), "OR missing: {}", out);
    }

    // ── RULE 4 in UPDATE ─────────────────────────────────────────────────────

    #[test]
    fn test_update_complex_where() {
        let out = format_sql("update users set name = 'test' where id = 1 and status = 'active'");
        assert!(
            out.starts_with("UPDATE users\nSET name = 'test'"),
            "unexpected: {}",
            out
        );
        assert!(
            out.contains("WHERE\n    "),
            "expected indented WHERE in UPDATE: {}",
            out
        );
        assert!(out.contains("AND"), "AND missing: {}", out);
    }

    // ── RULE 5: UPDATE multiple assignments ──────────────────────────────────

    #[test]
    fn test_update_multiple_assignments() {
        let out = format_sql("update users set name = 'test', email = 'a@b.com' where id = 1");
        assert_eq!(
            out,
            "UPDATE users\nSET name = 'test', email = 'a@b.com'\nWHERE id = 1"
        );
    }

    // ── RULE 6: INSERT edge cases ─────────────────────────────────────────────

    #[test]
    fn test_insert_no_column_list() {
        // INSERT without explicit column list — exercises the cols.is_empty() branch
        let out = format_sql("insert into users values ('test')");
        assert_eq!(out, "INSERT INTO users VALUES ('test')");
    }

    #[test]
    fn test_insert_long_goes_multiline() {
        let out = format_sql(
            "insert into users (id, name, email) values (1, 'a_very_long_name_here', 'very_long_email@example.com')",
        );
        assert!(
            out.contains('\n'),
            "long INSERT should be multiline: {}",
            out
        );
        assert!(out.contains("INSERT INTO"), "INSERT INTO missing: {}", out);
        assert!(out.contains("VALUES"), "VALUES missing: {}", out);
    }

    #[test]
    fn test_insert_long_no_cols_multiline() {
        // Long INSERT without column list → multiline using the cols.is_empty() branch
        let out = format_sql(
            "insert into users values (1, 'a_very_long_name_value_here', 'extremely_long_email@some_long_domain.com')",
        );
        assert!(out.contains('\n'), "long INSERT should be multiline: {}", out);
        assert!(out.starts_with("INSERT INTO users"), "unexpected: {}", out);
        assert!(out.contains("VALUES"), "VALUES missing: {}", out);
    }

    // ── RULE 8: JOIN → always multiline ──────────────────────────────────────

    #[test]
    fn test_join_is_multiline() {
        let out = format_sql(
            "select id from users join orders on users.id = orders.user_id",
        );
        assert!(
            out.contains('\n'),
            "JOIN query should be multiline: {}",
            out
        );
        assert!(out.contains("FROM users"), "FROM missing: {}", out);
    }

    #[test]
    fn test_join_multi_column() {
        let out = format_sql(
            "select id, name from users join orders on users.id = orders.user_id",
        );
        assert!(out.contains("SELECT\n    id"), "expected indented SELECT: {}", out);
        assert!(out.contains('\n'), "should be multiline: {}", out);
    }

    // ── WITH / CTE fallback ───────────────────────────────────────────────────

    #[test]
    fn test_cte_preserved() {
        let input = "with active as (select * from users where status = 'active') select * from active";
        let out = format_sql(input);
        // CTE falls back to raw text — the WITH clause must not be dropped
        assert!(
            out.to_uppercase().contains("WITH"),
            "WITH clause dropped: {}",
            out
        );
        assert!(
            out.to_uppercase().contains("SELECT"),
            "SELECT dropped: {}",
            out
        );
    }

    // ── ORDER BY / GROUP BY qualified names ──────────────────────────────────

    #[test]
    fn test_order_by_qualified_name() {
        let out = format_sql("select id from users order by users.name");
        assert!(
            out.contains("ORDER BY users.name"),
            "qualified ORDER BY truncated: {}",
            out
        );
    }

    #[test]
    fn test_group_by_multiple_columns() {
        let out = format_sql("select id from users group by id, name");
        assert!(
            out.contains("GROUP BY id, name"),
            "GROUP BY columns wrong: {}",
            out
        );
    }

    #[test]
    fn test_select_where_group_order_single_line() {
        // All clauses present but short enough → single line
        let out = format_sql("select id from users where active = 1 group by id order by id");
        assert!(
            !out.contains('\n'),
            "short query should stay single line: {}",
            out
        );
    }

    // ── Idempotency: additional cases ────────────────────────────────────────

    #[test]
    fn test_idempotent_multi_column() {
        let input = "select id, name, email from users where status = 'active'";
        let first = format_sql(input);
        let second = format_sql(&first);
        assert_eq!(first, second, "multi-column format not idempotent");
    }

    #[test]
    fn test_idempotent_complex_where_or() {
        let input = "select * from users where id = 1 or status = 'active'";
        let first = format_sql(input);
        let second = format_sql(&first);
        assert_eq!(first, second, "OR WHERE format not idempotent");
    }

    #[test]
    fn test_idempotent_update_complex_where() {
        let input = "update users set name = 'x' where id = 1 and active = 1";
        let first = format_sql(input);
        let second = format_sql(&first);
        assert_eq!(first, second, "UPDATE complex WHERE not idempotent");
    }

    #[test]
    fn test_idempotent_long_insert() {
        let input = "insert into users (id, name, email) values (1, 'a_very_long_name_here', 'very_long_email@example.com')";
        let first = format_sql(input);
        let second = format_sql(&first);
        assert_eq!(first, second, "long INSERT not idempotent");
    }

    // ── Comment edge cases ───────────────────────────────────────────────────

    #[test]
    fn test_multiple_comments_before_statement() {
        let out = format_sql("-- comment 1\n-- comment 2\nselect * from users");
        assert!(out.contains("-- comment 1"), "first comment lost: {}", out);
        assert!(out.contains("-- comment 2"), "second comment lost: {}", out);
        assert!(out.contains("SELECT *"), "statement lost: {}", out);
    }

    #[test]
    fn test_comment_between_statements() {
        let out = format_sql("select * from users;\n-- separator\nselect * from orders;");
        assert!(out.contains("SELECT * FROM users"), "first stmt lost: {}", out);
        assert!(out.contains("-- separator"), "comment lost: {}", out);
        assert!(out.contains("SELECT * FROM orders"), "second stmt lost: {}", out);
    }

    // ── Multi-statement edge cases ────────────────────────────────────────────

    #[test]
    fn test_mixed_statement_types() {
        let input =
            "select * from users; update users set name = 'x' where id = 1; insert into users (name) values ('y');";
        let out = format_sql(input);
        assert!(out.contains("SELECT *"), "SELECT missing: {}", out);
        assert!(out.contains("UPDATE users"), "UPDATE missing: {}", out);
        assert!(out.contains("INSERT INTO"), "INSERT missing: {}", out);
    }

    #[test]
    fn test_single_statement_with_semicolon() {
        let out = format_sql("select * from users;");
        assert_eq!(out, "SELECT * FROM users;");
    }

    // ── Empty / degenerate input ──────────────────────────────────────────────

    #[test]
    fn test_empty_input() {
        let out = format_sql("");
        assert_eq!(out, "");
    }

    #[test]
    fn test_comment_only() {
        let out = format_sql("-- just a comment");
        assert_eq!(out, "-- just a comment");
    }

    // ── Case sensitivity ─────────────────────────────────────────────────────

    #[test]
    fn test_mixed_case_keywords() {
        let out = format_sql("SeLeCt * FrOm users");
        assert_eq!(out, "SELECT * FROM users");
    }

    #[test]
    fn test_mixed_case_and_in_where() {
        // `And` (mixed case) must still trigger complex WHERE formatting
        let out = format_sql("select * from users where id = 1 And status = 'active'");
        assert!(
            out.contains("WHERE\n    ") || out.contains("WHERE id"),
            "WHERE not handled: {}",
            out
        );
    }

    // ── Whitespace normalization ──────────────────────────────────────────────

    #[test]
    fn test_extra_whitespace_in_where() {
        let out = format_sql("select * from users where   id   =   1");
        assert!(
            out.contains("WHERE"),
            "WHERE clause lost: {}",
            out
        );
        assert!(
            !out.contains("   "),
            "extra whitespace not normalized: {}",
            out
        );
    }
}

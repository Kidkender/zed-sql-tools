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


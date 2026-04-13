use sql_lsp::formatter::format_sql;

// ── RULE 1: single-line threshold ────────────────────────────────────────────

#[test]
fn test_select_star() {
    let out = format_sql("select * from users");
    assert_eq!(out, "SELECT * FROM users");
}

#[test]
fn test_select_order_by() {
    let out = format_sql("select id from users order by name");
    assert_eq!(out, "SELECT id FROM users ORDER BY name");
}

#[test]
fn test_already_uppercase() {
    let out = format_sql("SELECT * FROM users");
    assert_eq!(out, "SELECT * FROM users");
}

#[test]
fn test_single_col_long_query_goes_multiline() {
    let input = "select id from very_long_table_name where some_really_long_column_name = 'a_long_value_here'";
    let out = format_sql(input);
    assert!(out.contains('\n'), "long query should be multiline: {}", out);
}

// ── RULE 2: multi-column / function call ─────────────────────────────────────

#[test]
fn test_select_columns_where() {
    let out = format_sql("select id, name from users where id = 1");
    assert_eq!(out, "SELECT\n    id,\n    name\nFROM users\nWHERE id = 1");
}

#[test]
fn test_multi_column_select() {
    let out = format_sql("select id, name, email, created_at from users");
    assert_eq!(out, "SELECT\n    id,\n    name,\n    email,\n    created_at\nFROM users");
}

#[test]
fn test_function_call_column_is_multiline() {
    let out = format_sql("select count(*) from users");
    assert_eq!(out, "SELECT\n    count(*)\nFROM users");
}

#[test]
fn test_function_mixed_with_regular_column() {
    let out = format_sql("select id, count(*) from users group by id");
    assert_eq!(out, "SELECT\n    id,\n    count(*)\nFROM users\nGROUP BY id");
}

// ── RULE 3/4: WHERE simple vs complex ────────────────────────────────────────

#[test]
fn test_complex_where() {
    let out = format_sql("select * from users where id = 1 and status = 'active'");
    assert!(out.contains("WHERE\n    "), "expected indented WHERE: {}", out);
    assert!(out.contains("AND"), "expected AND: {}", out);
}

#[test]
fn test_complex_where_or() {
    let out = format_sql("select * from users where status = 'active' or role = 'admin'");
    assert!(out.contains("WHERE\n    "), "expected indented WHERE: {}", out);
    assert!(out.contains("OR"), "expected OR: {}", out);
}

#[test]
fn test_complex_where_mixed_and_or() {
    let out = format_sql("select * from users where id = 1 and status = 'active' or role = 'admin'");
    assert!(out.contains("WHERE\n    "), "expected indented WHERE: {}", out);
    assert!(out.contains("AND"), "AND missing: {}", out);
    assert!(out.contains("OR"), "OR missing: {}", out);
}

#[test]
fn test_literal_not_uppercased() {
    let out = format_sql("select * from users where name = 'hello world'");
    assert!(out.contains("'hello world'"), "literal must be preserved: {}", out);
}

#[test]
fn test_extra_whitespace_in_where() {
    let out = format_sql("select * from users where   id   =   1");
    assert!(out.contains("WHERE"), "WHERE clause lost: {}", out);
    assert!(!out.contains("   "), "extra whitespace not normalized: {}", out);
}

// ── RULE 5: UPDATE ────────────────────────────────────────────────────────────

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
fn test_update_complex_where() {
    let out = format_sql("update users set name = 'test' where id = 1 and status = 'active'");
    assert!(out.starts_with("UPDATE users\nSET name = 'test'"), "unexpected: {}", out);
    assert!(out.contains("WHERE\n    "), "expected indented WHERE in UPDATE: {}", out);
    assert!(out.contains("AND"), "AND missing: {}", out);
}

#[test]
fn test_update_multiple_assignments() {
    let out = format_sql("update users set name = 'test', email = 'a@b.com' where id = 1");
    assert_eq!(out, "UPDATE users\nSET name = 'test', email = 'a@b.com'\nWHERE id = 1");
}

// ── RULE 6: INSERT ────────────────────────────────────────────────────────────

#[test]
fn test_insert() {
    let out = format_sql("insert into users (name) values ('test')");
    assert_eq!(out, "INSERT INTO users (name) VALUES ('test')");
}

#[test]
fn test_insert_no_column_list() {
    let out = format_sql("insert into users values ('test')");
    assert_eq!(out, "INSERT INTO users VALUES ('test')");
}

#[test]
fn test_insert_long_goes_multiline() {
    let out = format_sql(
        "insert into users (id, name, email) values (1, 'a_very_long_name_here', 'very_long_email@example.com')",
    );
    assert!(out.contains('\n'), "long INSERT should be multiline: {}", out);
    assert!(out.contains("INSERT INTO"), "INSERT INTO missing: {}", out);
    assert!(out.contains("VALUES"), "VALUES missing: {}", out);
}

#[test]
fn test_insert_long_no_cols_multiline() {
    let out = format_sql(
        "insert into users values (1, 'a_very_long_name_value_here', 'extremely_long_email@some_long_domain.com')",
    );
    assert!(out.contains('\n'), "long INSERT should be multiline: {}", out);
    assert!(out.starts_with("INSERT INTO users"), "unexpected: {}", out);
    assert!(out.contains("VALUES"), "VALUES missing: {}", out);
}

// ── RULE 8: JOIN ──────────────────────────────────────────────────────────────

#[test]
fn test_join_is_multiline() {
    let out = format_sql("select id from users join orders on users.id = orders.user_id");
    assert!(out.contains('\n'), "JOIN query should be multiline: {}", out);
    assert!(out.contains("FROM users"), "FROM missing: {}", out);
}

#[test]
fn test_join_multi_column() {
    let out = format_sql("select id, name from users join orders on users.id = orders.user_id");
    assert!(out.contains("SELECT\n    id"), "expected indented SELECT: {}", out);
}

// ── GROUP BY / ORDER BY ───────────────────────────────────────────────────────

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
fn test_order_by_qualified_name() {
    let out = format_sql("select id from users order by users.name");
    assert!(out.contains("ORDER BY users.name"), "qualified ORDER BY truncated: {}", out);
}

#[test]
fn test_group_by_multiple_columns() {
    let out = format_sql("select id from users group by id, name");
    assert!(out.contains("GROUP BY id, name"), "GROUP BY columns wrong: {}", out);
}

#[test]
fn test_select_where_group_order_single_line() {
    let out = format_sql("select id from users where active = 1 group by id order by id");
    assert!(!out.contains('\n'), "short query should stay single line: {}", out);
}

// ── WITH / CTE fallback ───────────────────────────────────────────────────────

#[test]
fn test_cte_preserved() {
    let input = "with active as (select * from users where status = 'active') select * from active";
    let out = format_sql(input);
    assert!(out.to_uppercase().contains("WITH"), "WITH clause dropped: {}", out);
    assert!(out.to_uppercase().contains("SELECT"), "SELECT dropped: {}", out);
}

// ── Idempotency ───────────────────────────────────────────────────────────────

#[test]
fn test_where_idempotent() {
    let input = "SELECT * FROM users WHERE id = 1 AND status = 'active'";
    let first = format_sql(input);
    let second = format_sql(&first);
    assert_eq!(first, second, "second format changed the output");
}

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

// ── Comments ──────────────────────────────────────────────────────────────────

#[test]
fn test_comment_preserved() {
    let out = format_sql("-- find all users\nselect * from users");
    assert!(out.starts_with("-- find all users"), "comment lost: {}", out);
    assert!(out.contains("SELECT *"), "statement lost: {}", out);
}

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

#[test]
fn test_comment_only() {
    let out = format_sql("-- just a comment");
    assert_eq!(out, "-- just a comment");
}

// ── Multi-statement ───────────────────────────────────────────────────────────

#[test]
fn test_multi_statement() {
    let out = format_sql("select * from users; select id from orders;");
    assert!(out.contains("SELECT *"), "first statement lost: {}", out);
    assert!(out.contains("SELECT id"), "second statement lost: {}", out);
    assert!(out.contains(';'), "semicolons should be preserved: {}", out);
}

#[test]
fn test_single_statement_with_semicolon() {
    let out = format_sql("select * from users;");
    assert_eq!(out, "SELECT * FROM users;");
}

#[test]
fn test_mixed_statement_types() {
    let input = "select * from users; update users set name = 'x' where id = 1; insert into users (name) values ('y');";
    let out = format_sql(input);
    assert!(out.contains("SELECT *"), "SELECT missing: {}", out);
    assert!(out.contains("UPDATE users"), "UPDATE missing: {}", out);
    assert!(out.contains("INSERT INTO"), "INSERT missing: {}", out);
}

// ── Empty / degenerate input ──────────────────────────────────────────────────

#[test]
fn test_empty_input() {
    let out = format_sql("");
    assert_eq!(out, "");
}

// ── Case sensitivity ──────────────────────────────────────────────────────────

#[test]
fn test_mixed_case_keywords() {
    let out = format_sql("SeLeCt * FrOm users");
    assert_eq!(out, "SELECT * FROM users");
}

#[test]
fn test_mixed_case_and_in_where() {
    let out = format_sql("select * from users where id = 1 And status = 'active'");
    assert!(
        out.contains("WHERE\n    ") || out.contains("WHERE id"),
        "WHERE not handled: {}",
        out
    );
}

// ── Bug regression ────────────────────────────────────────────────────────────

// Bug: DELETE was reformatted as "SELECT * FROM  WHERE ..." because format_statement
// saw the "from" child before the "delete" child and dispatched to format_select_stmt.
#[test]
fn test_delete_not_corrupted_to_select() {
    let input = "DELETE FROM scenarios WHERE id = $1::uuid";
    let out = format_sql(input);
    assert!(
        out.to_uppercase().starts_with("DELETE"),
        "DELETE was corrupted; got: {}",
        out
    );
    assert!(
        !out.to_uppercase().starts_with("SELECT"),
        "DELETE was turned into SELECT: {}",
        out
    );
}

// Bug: RETURNING * was silently dropped because it lives as a sibling to the
// insert/update node inside statement; format_statement returned before seeing it.
#[test]
fn test_insert_returning_preserved() {
    let input = "INSERT INTO steps (id) VALUES ($1) RETURNING *";
    let out = format_sql(input);
    assert!(
        out.to_uppercase().contains("RETURNING"),
        "RETURNING dropped from INSERT; got: {}",
        out
    );
}

#[test]
fn test_update_returning_preserved() {
    let input = "UPDATE steps SET question = $2 WHERE id = $1 RETURNING *";
    let out = format_sql(input);
    assert!(
        out.to_uppercase().contains("RETURNING"),
        "RETURNING dropped from UPDATE; got: {}",
        out
    );
}

// Bug: SELECT EXISTS(...) has no FROM node; the formatter was generating "FROM "
// with an empty table, corrupting the query.
#[test]
fn test_select_exists_no_from_corruption() {
    let input = "SELECT EXISTS( SELECT 1 FROM steps WHERE scenario_id = $1 AND order_index = $2 )";
    let out = format_sql(input);
    assert!(
        out.to_uppercase().contains("EXISTS"),
        "EXISTS lost; got: {}",
        out
    );
    // Must not have a dangling bare "FROM" line with no table name after it
    // (that was the corruption: "FROM " with empty table at the end).
    let trailing = out.trim_end();
    assert!(
        !trailing.ends_with("FROM") && !trailing.ends_with("FROM "),
        "dangling bare FROM emitted for no-table SELECT; got: {}",
        out
    );
}

// Bug: LIMIT $1 OFFSET $2 was silently dropped by the formatter because the grammar
// produces a program-level ERROR node for parameterised LIMIT/OFFSET expressions.
#[test]
fn test_limit_offset_params_preserved() {
    let input = "SELECT * FROM scenarios ORDER BY created_at DESC LIMIT $1 OFFSET $2";
    let out = format_sql(input);
    assert!(
        out.to_uppercase().contains("LIMIT"),
        "LIMIT was dropped; got: {}",
        out
    );
    assert!(
        out.to_uppercase().contains("OFFSET"),
        "OFFSET was dropped; got: {}",
        out
    );
}

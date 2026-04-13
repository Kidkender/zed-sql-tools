use sql_lsp::linter::lint_sql;

// --- Syntax errors ---

#[test]
fn test_empty_where_is_error() {
    let diags = lint_sql("SELECT * FROM users WHERE");
    assert!(!diags.is_empty(), "should detect bare WHERE");
    assert!(diags.iter().any(|d| d.is_error));
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

// --- UPDATE without WHERE ---

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

// --- DELETE without WHERE ---

#[test]
fn test_delete_without_where_is_warning() {
    let diags = lint_sql("DELETE FROM users");
    assert!(!diags.is_empty());
    let w = diags.iter().find(|d| !d.is_error).unwrap();
    assert!(w.message.contains("DELETE"));
}

#[test]
fn test_delete_with_where_no_warning() {
    let diags = lint_sql("DELETE FROM users WHERE id = 1");
    let warnings: Vec<_> = diags.iter().filter(|d| !d.is_error).collect();
    assert!(warnings.is_empty(), "no warning expected: {:?}", warnings.iter().map(|d| &d.message).collect::<Vec<_>>());
}

// --- SELECT missing columns ---

#[test]
fn test_select_missing_columns() {
    let diags = lint_sql("SELECT FROM users");
    assert!(!diags.is_empty(), "should detect missing columns");
    assert!(diags.iter().any(|d| d.message.contains("column")));
}

// --- SELECT * ---

#[test]
fn test_select_star_is_warning() {
    let diags = lint_sql("SELECT * FROM users");
    assert!(!diags.is_empty());
    let w = diags.iter().find(|d| !d.is_error && d.message.contains("SELECT *")).unwrap();
    assert!(w.message.contains("explicitly"));
}

#[test]
fn test_select_columns_no_star_warning() {
    let diags = lint_sql("SELECT id, name FROM users");
    let star_warnings: Vec<_> = diags.iter().filter(|d| d.message.contains("SELECT *")).collect();
    assert!(star_warnings.is_empty());
}

// --- = NULL ---

#[test]
fn test_eq_null_is_error() {
    let diags = lint_sql("SELECT id FROM users WHERE name = NULL");
    assert!(!diags.is_empty());
    let e = diags.iter().find(|d| d.is_error && d.message.contains("IS NULL")).unwrap();
    assert!(e.message.contains("IS NULL"));
}

#[test]
fn test_is_null_no_error() {
    let diags = lint_sql("SELECT id FROM users WHERE name IS NULL");
    let null_errors: Vec<_> = diags.iter().filter(|d| d.message.contains("IS NULL")).collect();
    assert!(null_errors.is_empty());
}

// --- IN () empty list ---

#[test]
fn test_empty_in_list_is_error() {
    let diags = lint_sql("SELECT id FROM users WHERE id IN ()");
    assert!(!diags.is_empty());
    let e = diags.iter().find(|d| d.is_error && d.message.contains("IN ()")).unwrap();
    assert!(e.message.contains("always false"));
}

#[test]
fn test_non_empty_in_list_no_error() {
    let diags = lint_sql("SELECT id FROM users WHERE id IN (1, 2, 3)");
    let in_errors: Vec<_> = diags.iter().filter(|d| d.message.contains("IN ()")).collect();
    assert!(in_errors.is_empty());
}

// --- LIMIT without ORDER BY ---

#[test]
fn test_limit_without_order_by_is_warning() {
    let diags = lint_sql("SELECT id FROM users LIMIT 10");
    let w = diags.iter().find(|d| d.message.contains("LIMIT")).unwrap();
    assert!(!w.is_error);
    assert!(w.message.contains("ORDER BY"));
}

#[test]
fn test_limit_with_order_by_no_warning() {
    let diags = lint_sql("SELECT id FROM users ORDER BY id LIMIT 10");
    let limit_warns: Vec<_> = diags.iter().filter(|d| d.message.contains("LIMIT")).collect();
    assert!(limit_warns.is_empty());
}

// --- WHERE always true ---

#[test]
fn test_where_always_true_is_warning() {
    let diags = lint_sql("SELECT * FROM users WHERE 1 = 1");
    let w = diags.iter().find(|d| d.message.contains("always true")).unwrap();
    assert!(!w.is_error);
}

#[test]
fn test_where_normal_condition_no_always_true_warning() {
    let diags = lint_sql("SELECT * FROM users WHERE id = 1");
    let warns: Vec<_> = diags.iter().filter(|d| d.message.contains("always true")).collect();
    assert!(warns.is_empty());
}

// --- Subquery without alias ---

#[test]
fn test_subquery_without_alias_is_warning() {
    let diags = lint_sql("SELECT * FROM (SELECT id FROM users)");
    let w = diags.iter().find(|d| d.message.contains("alias")).unwrap();
    assert!(!w.is_error);
}

#[test]
fn test_subquery_with_alias_no_warning() {
    let diags = lint_sql("SELECT * FROM (SELECT id FROM users) AS u");
    let warns: Vec<_> = diags.iter().filter(|d| d.message.contains("alias")).collect();
    assert!(warns.is_empty());
}

// --- Grammar false positive: LIMIT $n OFFSET $n ---

// Bug: tree-sitter-sequel cannot parse `LIMIT $1 OFFSET $2` and produces an ERROR
// node — the linter must not report this as a syntax error.
#[test]
fn test_limit_offset_params_no_syntax_error() {
    let diags = lint_sql("SELECT * FROM scenarios ORDER BY created_at DESC LIMIT $1 OFFSET $2");
    let syntax_errors: Vec<_> = diags.iter().filter(|d| d.is_error && d.message.contains("Syntax error")).collect();
    assert!(
        syntax_errors.is_empty(),
        "false positive syntax error for LIMIT $n OFFSET $n: {:?}",
        syntax_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

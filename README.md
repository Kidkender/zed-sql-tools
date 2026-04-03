# zed-sql-tools

SQL extension for [Zed](https://zed.dev) with formatting and linting — no external CLI tools required.

## Features

**Formatter**

Automatically formats SQL on save — keywords uppercased, each major clause on its own line.

```sql
-- before
select id, name from users where active = 1 order by name

-- after
SELECT id, name
FROM users
WHERE active = 1
ORDER BY name
```

Supports `SELECT`, `INSERT`, `UPDATE` with `WHERE`, `GROUP BY`, `ORDER BY`. Comments and string literals are preserved as-is.

**Linter**

Inline diagnostics as you type:

- Syntax errors highlighted immediately
- `UPDATE` without a `WHERE` clause — warns before you accidentally update every row
- `SELECT` without a column list (`SELECT FROM users`)

## Grammar

Uses [DerekStride/tree-sitter-sql](https://github.com/DerekStride/tree-sitter-sql) — the same grammar Zed uses internally for syntax highlighting.

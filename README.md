# zed-sql-tools

[![Release](https://img.shields.io/github/v/release/kidkender/zed-sql-tools)](https://github.com/kidkender/zed-sql-tools/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/kidkender/zed-sql-tools/release.yml?label=build)](https://github.com/kidkender/zed-sql-tools/actions/workflows/release.yml)

SQL extension for [Zed](https://zed.dev) with smart formatting and linting — no external CLI tools required.

## Features

### Formatter

Formats SQL on save. Short queries stay on one line; complex queries break into readable multi-line form.

**Short query — stays compact:**
```sql
-- before
select * from users where id = 1

-- after
SELECT * FROM users WHERE id = 1
```

**Multi-column — each column indented:**
```sql
-- before
select id, name, email from users where active = 1

-- after
SELECT
    id,
    name,
    email
FROM users
WHERE active = 1
```

**Complex WHERE — conditions indented:**
```sql
-- before
select * from users where id = 1 and status = 'active' or role = 'admin'

-- after
SELECT * FROM users
WHERE
    id = 1
    AND status = 'active'
    OR role = 'admin'
```

**UPDATE — always multi-line:**
```sql
-- before
update users set name = 'x' where id = 1

-- after
UPDATE users
SET name = 'x'
WHERE id = 1
```

Formatting rules:
- Queries ≤ 80 chars with no JOIN, no subquery, simple WHERE → single line
- Multiple columns or function calls (`COUNT(*)`) → indented under `SELECT`
- `AND`/`OR` conditions → indented under `WHERE`
- `UPDATE` always multi-line
- `JOIN` and subqueries always multi-line
- Comments and string literals preserved as-is
- Idempotent — formatting twice gives the same result

### Linter

Inline diagnostics as you type:

- Syntax errors highlighted immediately
- `UPDATE` without `WHERE` — warns before accidentally updating every row
- `SELECT` without columns (`SELECT FROM users`)

## Installation

The extension is available in the Zed extension marketplace. Search for **SQL Tools** in the Extensions panel (`cmd+shift+x` on macOS).

Zed will automatically download the `sql-lsp` language server binary on first use.

To enable format-on-save, add to your Zed settings:

```json
{
  "languages": {
    "SQL": {
      "formatter": "language_server",
      "format_on_save": "on"
    }
  }
}
```

## Grammar

Uses [DerekStride/tree-sitter-sql](https://github.com/DerekStride/tree-sitter-sql) for parsing and syntax highlighting.

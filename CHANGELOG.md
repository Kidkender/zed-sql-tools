# Changelog

## [0.4.0] - 2026-04-13

### Fixed
- `DELETE FROM ... WHERE ...` was corrupted to `SELECT * FROM  WHERE ...` by the formatter — `DELETE` statements are now preserved as-is
- `LIMIT $n OFFSET $n` with query parameters was silently dropped by the formatter and reported as a false positive syntax error by the linter — both now handle this grammar limitation correctly
- `RETURNING *` was silently dropped from `INSERT` and `UPDATE` statements after formatting
- `SELECT EXISTS(...)` produced a dangling bare `FROM` clause after formatting
- `SELECT EXISTS(...)` with a multiline subquery was incorrectly split into `SELECT\n    EXISTS(` — the subquery now stays inline with `SELECT`
- Blank line between query blocks was placed *after* comments instead of *before* them, breaking sqlc-style files
- Semicolons were lost on queries where `LIMIT $n OFFSET $n` caused a program-level parse error

### Added
- `format-file` dev binary: `cargo run -p sql-lsp --bin format-file -- file.sql` to test the formatter on real SQL files
- Regression tests for all 7 fixed bugs (formatter, linter, and LSP integration)

## [0.3.0] - 2026-04-07

### Added
- 7 new linter rules:
  - `DELETE` without `WHERE` — warns before deleting all rows
  - `SELECT *` — warns to specify columns explicitly
  - `= NULL` comparison — error, should use `IS NULL` / `IS NOT NULL`
  - `IN ()` empty list — error, condition is always false
  - `LIMIT` without `ORDER BY` — warns about inconsistent pagination results
  - `WHERE 1 = 1` — warns about always-true condition (debug code left in)
  - Subquery in `FROM` without alias — warns for clarity and DB compatibility
- Linter tests moved to `crates/sql-lsp/tests/linter_tests.rs` (separate from source)
- README: added Syntax Checking section and full Linter rule documentation

## [0.2.1] - 2026-04-04

### Changed
- Renamed extension ID from `zed-sql-tools` to `sql-tools` for marketplace compliance
- Added MIT license

## [0.2.0] - 2026-04-04

### Added
- Smart single-line vs multi-line formatting heuristic (threshold: 80 chars)
- Multi-column SELECT: each column indented on its own line
- Complex WHERE formatting: `AND`/`OR` conditions indented under `WHERE`
- `JOIN` and subqueries always trigger multi-line output
- `WITH`/CTE statements preserved as-is (no longer dropped)
- Inline comments after statements stay on the same line
- `INSERT` without column list now formats correctly
- Idempotent formatting — saving multiple times no longer adds extra blank lines
- 51 unit tests + 7 LSP end-to-end integration tests
- LSP integration test binary (`lsp-test`) for automated testing without Zed

### Fixed
- Repeat-save adding extra blank lines before `AND`/`OR` in WHERE clauses
- `ORDER BY users.email` being truncated to `ORDER BY users`
- Inline comments being pushed to a new line after formatting
- `WITH`/CTE clause being dropped entirely after format
- `INSERT INTO users VALUES (...)` mapping values to wrong slot

## [0.1.0] - 2026-04-03

### Added
- Initial release
- SQL language server (`sql-lsp`) with LSP protocol support
- AST-based formatter using tree-sitter-sql grammar
- Linter: syntax errors, `UPDATE` without `WHERE`, `SELECT` without columns
- Cross-platform release builds: Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64
- GitHub Actions release pipeline

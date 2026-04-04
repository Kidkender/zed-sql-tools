# Changelog

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

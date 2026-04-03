// Add tree-sitter-sql dependency and implement SqlNode + AST Explorer.

pub struct SqlNode {
    pub kind: String,
    pub text: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub children: Vec<SqlNode>,
}

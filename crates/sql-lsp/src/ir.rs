#[derive(Debug)]
pub enum SqlIr {
    Keyword(String),
    Text(String),
    Literal(String),
    Space,
    Newline,
    Comma,
}

pub fn render(tokens: &[SqlIr]) -> String {
    let mut out = String::new();
    for token in tokens {
        match token {
            SqlIr::Keyword(s) | SqlIr::Text(s) | SqlIr::Literal(s) => out.push_str(s),
            SqlIr::Space => out.push(' '),
            SqlIr::Newline => out.push('\n'),
            SqlIr::Comma => out.push_str(", "),
        }
    }
    out
}

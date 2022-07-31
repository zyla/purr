#[derive(PartialEq, Eq, Debug, Clone)]
pub enum Token {
    IntegerLiteral(usize),
    StringLiteral(String),
    CharLiteral(char),
    Identifier(String),

    // Layout
    LayoutStart,
    LayoutSep,
    LayoutEnd,

    // Operators
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Backtick,
    Equal,
    Pipe,
    Comma,
    Colon,
    Semicolon,
    Dot,

    Arrow,    // ->
    FatArrow, // =>
    TypeOf,   // ::
    Bind,     // <-

    Operator(String),

    // Keywords
    If,
    Then,
    Else,
    Ado,
    Do,
    Let,
    In,
    Where,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct TokenInfo {
    pub token: Token,
    /** Position where the whitespace before the token starts */
    pub whitespace_start: usize,
    /** Position where the token itself starts */
    pub start: usize,
    pub end: usize,
    /** Indentation level, i.e. column number of the first non-whitespace token on the line */
    pub indent_level: usize,
    /** Zero-based column number (offset since line start) */
    pub column: usize,
    pub newline_before: bool,
}

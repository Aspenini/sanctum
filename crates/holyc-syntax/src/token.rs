use crate::span::Span;

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// HolyC tokens. Multi-char operators match TempleOS `TK_*` where it matters.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Eof,
    Ident(String),
    Str(String),
    Int(i64),
    Float(f64),
    /// Packed little-endian char constant (`'ABC'` == 0x434241).
    Char(i64),

    // punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    DblColon,
    Dot,
    DotDot,
    Ellipsis,
    Question,     // unused in HolyC expressions; still lexed
    DollarDollar, // `$$`  (RIP / class offset)

    // operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Bang,
    Assign,
    Lt,
    Gt,
    Power, // `
    Arrow, // ->
    PlusPlus,
    MinusMinus,
    Shl,
    Shr,
    EqEq,
    NotEq,
    Le,
    Ge,
    AndAnd,
    OrOr,
    XorXor,
    ShlEq,
    ShrEq,
    MulEq,
    DivEq,
    ModEq,
    AndEq,
    OrEq,
    XorEq,
    AddEq,
    SubEq,

    /// `$IB,...$` — yields a pointer to a DolDoc bin (sprite).
    InsBin {
        idx: i64,
    },
    /// `$BS,...$` size of a bin.
    InsBinSize {
        idx: i64,
    },

    /// Preprocessor / compiler directive at line start: `#include` etc.
    /// The ident is the directive name (include, define, if, ...).
    Hash,
}

impl TokenKind {
    pub fn ident(&self) -> Option<&str> {
        match self {
            TokenKind::Ident(s) => Some(s),
            _ => None,
        }
    }
}

pub fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "class"
            | "union"
            | "if"
            | "else"
            | "for"
            | "while"
            | "do"
            | "switch"
            | "case"
            | "start"
            | "end"
            | "break"
            | "goto"
            | "return"
            | "try"
            | "catch"
            | "throw"
            | "sizeof"
            | "offset"
            | "public"
            | "static"
            | "extern"
            | "intern"
            | "import"
            | "_extern"
            | "_import"
            | "asm"
            | "lock"
            | "no_warn"
            | "lastclass"
            | "interrupt"
            | "haserrcode"
            | "argpop"
            | "noargpop"
            | "reg"
            | "noreg"
            | "defined"
    )
}

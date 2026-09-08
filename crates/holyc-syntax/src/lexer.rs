use crate::error::SyntaxError;
use crate::span::Span;
use crate::token::{Token, TokenKind};

pub struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    path: &'a str,
    file: u32,
    pos: usize,
    /// True if the next token is at the beginning of a line (only whitespace so far).
    line_start: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str, path: &'a str, file: u32) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            path,
            file,
            pos: 0,
            line_start: true,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, SyntaxError> {
        let mut out = Vec::new();
        loop {
            let tok = self.next_token()?;
            let eof = matches!(tok.kind, TokenKind::Eof);
            out.push(tok);
            if eof {
                break;
            }
        }
        Ok(out)
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file, start, end)
    }

    fn err(&self, start: usize, msg: impl Into<String>) -> SyntaxError {
        SyntaxError::at(self.path, self.src, self.span(start, self.pos), msg)
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let mut chars = self.src[self.pos..].chars();
        let ch = chars.next()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s)
    }

    fn skip_ws_and_comments(&mut self) -> Result<(), SyntaxError> {
        loop {
            while let Some(ch) = self.peek() {
                match ch {
                    ' ' | '\t' | '\r' | '\u{0}' => {
                        self.bump();
                    }
                    '\n' => {
                        self.bump();
                        self.line_start = true;
                    }
                    _ => break,
                }
            }
            if self.starts_with("//") {
                while let Some(ch) = self.peek() {
                    self.bump();
                    if ch == '\n' {
                        self.line_start = true;
                        break;
                    }
                }
                continue;
            }
            if self.starts_with("/*") {
                let start = self.pos;
                self.bump();
                self.bump();
                loop {
                    if self.peek().is_none() {
                        return Err(self.err(start, "unterminated block comment"));
                    }
                    if self.starts_with("*/") {
                        self.bump();
                        self.bump();
                        break;
                    }
                    if self.peek() == Some('\n') {
                        self.line_start = true;
                    }
                    self.bump();
                }
                continue;
            }
            break;
        }
        Ok(())
    }

    pub fn next_token(&mut self) -> Result<Token, SyntaxError> {
        self.skip_ws_and_comments()?;
        let start = self.pos;
        let Some(ch) = self.peek() else {
            return Ok(Token::new(TokenKind::Eof, self.span(start, start)));
        };

        // DolDoc command or `$$`.
        if ch == '$' {
            return self.lex_dollar(start);
        }

        if ch == '#' && self.line_start {
            self.bump();
            self.line_start = false;
            return Ok(Token::new(TokenKind::Hash, self.span(start, self.pos)));
        }

        self.line_start = false;

        if is_ident_start(ch) {
            return self.lex_ident(start);
        }
        if ch.is_ascii_digit()
            || (ch == '.' && self.bytes.get(self.pos + 1).is_some_and(|b| b.is_ascii_digit()))
        {
            return self.lex_number(start);
        }

        match ch {
            '"' => self.lex_string(start),
            '\'' => self.lex_char(start),
            '(' => {
                self.bump();
                Ok(Token::new(TokenKind::LParen, self.span(start, self.pos)))
            }
            ')' => {
                self.bump();
                Ok(Token::new(TokenKind::RParen, self.span(start, self.pos)))
            }
            '{' => {
                self.bump();
                Ok(Token::new(TokenKind::LBrace, self.span(start, self.pos)))
            }
            '}' => {
                self.bump();
                Ok(Token::new(TokenKind::RBrace, self.span(start, self.pos)))
            }
            '[' => {
                self.bump();
                Ok(Token::new(TokenKind::LBracket, self.span(start, self.pos)))
            }
            ']' => {
                self.bump();
                Ok(Token::new(TokenKind::RBracket, self.span(start, self.pos)))
            }
            ',' => {
                self.bump();
                Ok(Token::new(TokenKind::Comma, self.span(start, self.pos)))
            }
            ';' => {
                self.bump();
                Ok(Token::new(TokenKind::Semicolon, self.span(start, self.pos)))
            }
            '?' => {
                self.bump();
                Ok(Token::new(TokenKind::Question, self.span(start, self.pos)))
            }
            '~' => {
                self.bump();
                Ok(Token::new(TokenKind::Tilde, self.span(start, self.pos)))
            }
            '`' => {
                self.bump();
                Ok(Token::new(TokenKind::Power, self.span(start, self.pos)))
            }
            ':' => {
                self.bump();
                if self.peek() == Some(':') {
                    self.bump();
                    Ok(Token::new(TokenKind::DblColon, self.span(start, self.pos)))
                } else {
                    Ok(Token::new(TokenKind::Colon, self.span(start, self.pos)))
                }
            }
            '.' => {
                self.bump();
                if self.peek() == Some('.') {
                    self.bump();
                    if self.peek() == Some('.') {
                        self.bump();
                        Ok(Token::new(TokenKind::Ellipsis, self.span(start, self.pos)))
                    } else {
                        Ok(Token::new(TokenKind::DotDot, self.span(start, self.pos)))
                    }
                } else {
                    Ok(Token::new(TokenKind::Dot, self.span(start, self.pos)))
                }
            }
            '+' => self.lex_op2(start, '+', TokenKind::Plus, TokenKind::PlusPlus, TokenKind::AddEq),
            '-' => {
                self.bump();
                match self.peek() {
                    Some('-') => {
                        self.bump();
                        Ok(Token::new(TokenKind::MinusMinus, self.span(start, self.pos)))
                    }
                    Some('=') => {
                        self.bump();
                        Ok(Token::new(TokenKind::SubEq, self.span(start, self.pos)))
                    }
                    Some('>') => {
                        self.bump();
                        Ok(Token::new(TokenKind::Arrow, self.span(start, self.pos)))
                    }
                    _ => Ok(Token::new(TokenKind::Minus, self.span(start, self.pos))),
                }
            }
            '*' => self.lex_eq(start, TokenKind::Star, TokenKind::MulEq),
            '/' => self.lex_eq(start, TokenKind::Slash, TokenKind::DivEq),
            '%' => self.lex_eq(start, TokenKind::Percent, TokenKind::ModEq),
            '=' => {
                self.bump();
                if self.peek() == Some('=') {
                    self.bump();
                    Ok(Token::new(TokenKind::EqEq, self.span(start, self.pos)))
                } else {
                    Ok(Token::new(TokenKind::Assign, self.span(start, self.pos)))
                }
            }
            '!' => {
                self.bump();
                if self.peek() == Some('=') {
                    self.bump();
                    Ok(Token::new(TokenKind::NotEq, self.span(start, self.pos)))
                } else {
                    Ok(Token::new(TokenKind::Bang, self.span(start, self.pos)))
                }
            }
            '<' => {
                self.bump();
                match self.peek() {
                    Some('<') => {
                        self.bump();
                        if self.peek() == Some('=') {
                            self.bump();
                            Ok(Token::new(TokenKind::ShlEq, self.span(start, self.pos)))
                        } else {
                            Ok(Token::new(TokenKind::Shl, self.span(start, self.pos)))
                        }
                    }
                    Some('=') => {
                        self.bump();
                        Ok(Token::new(TokenKind::Le, self.span(start, self.pos)))
                    }
                    _ => Ok(Token::new(TokenKind::Lt, self.span(start, self.pos))),
                }
            }
            '>' => {
                self.bump();
                match self.peek() {
                    Some('>') => {
                        self.bump();
                        if self.peek() == Some('=') {
                            self.bump();
                            Ok(Token::new(TokenKind::ShrEq, self.span(start, self.pos)))
                        } else {
                            Ok(Token::new(TokenKind::Shr, self.span(start, self.pos)))
                        }
                    }
                    Some('=') => {
                        self.bump();
                        Ok(Token::new(TokenKind::Ge, self.span(start, self.pos)))
                    }
                    _ => Ok(Token::new(TokenKind::Gt, self.span(start, self.pos))),
                }
            }
            '&' => {
                self.bump();
                match self.peek() {
                    Some('&') => {
                        self.bump();
                        Ok(Token::new(TokenKind::AndAnd, self.span(start, self.pos)))
                    }
                    Some('=') => {
                        self.bump();
                        Ok(Token::new(TokenKind::AndEq, self.span(start, self.pos)))
                    }
                    _ => Ok(Token::new(TokenKind::Amp, self.span(start, self.pos))),
                }
            }
            '|' => {
                self.bump();
                match self.peek() {
                    Some('|') => {
                        self.bump();
                        Ok(Token::new(TokenKind::OrOr, self.span(start, self.pos)))
                    }
                    Some('=') => {
                        self.bump();
                        Ok(Token::new(TokenKind::OrEq, self.span(start, self.pos)))
                    }
                    _ => Ok(Token::new(TokenKind::Pipe, self.span(start, self.pos))),
                }
            }
            '^' => {
                self.bump();
                match self.peek() {
                    Some('^') => {
                        self.bump();
                        Ok(Token::new(TokenKind::XorXor, self.span(start, self.pos)))
                    }
                    Some('=') => {
                        self.bump();
                        Ok(Token::new(TokenKind::XorEq, self.span(start, self.pos)))
                    }
                    _ => Ok(Token::new(TokenKind::Caret, self.span(start, self.pos))),
                }
            }
            _ => Err(self.err(start, format!("unexpected character {ch:?}"))),
        }
    }

    fn lex_op2(
        &mut self,
        start: usize,
        ch: char,
        one: TokenKind,
        doubled: TokenKind,
        eq: TokenKind,
    ) -> Result<Token, SyntaxError> {
        self.bump();
        if self.peek() == Some(ch) {
            self.bump();
            Ok(Token::new(doubled, self.span(start, self.pos)))
        } else if self.peek() == Some('=') {
            self.bump();
            Ok(Token::new(eq, self.span(start, self.pos)))
        } else {
            Ok(Token::new(one, self.span(start, self.pos)))
        }
    }

    fn lex_eq(
        &mut self,
        start: usize,
        one: TokenKind,
        eq: TokenKind,
    ) -> Result<Token, SyntaxError> {
        self.bump();
        if self.peek() == Some('=') {
            self.bump();
            Ok(Token::new(eq, self.span(start, self.pos)))
        } else {
            Ok(Token::new(one, self.span(start, self.pos)))
        }
    }

    fn lex_ident(&mut self, start: usize) -> Result<Token, SyntaxError> {
        while let Some(ch) = self.peek() {
            if is_ident_continue(ch) {
                self.bump();
            } else {
                break;
            }
        }
        let s = self.src[start..self.pos].to_string();
        // TempleOS aliases π / ∞ as named constants; keep them as idents.
        Ok(Token::new(TokenKind::Ident(s), self.span(start, self.pos)))
    }

    fn lex_number(&mut self, start: usize) -> Result<Token, SyntaxError> {
        if self.starts_with("0x") || self.starts_with("0X") {
            self.bump();
            self.bump();
            let hex_start = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.bump();
            }
            let hex = &self.src[hex_start..self.pos];
            if hex.is_empty() {
                return Err(self.err(start, "invalid hex literal"));
            }
            let val = i64::from_str_radix(hex, 16)
                .or_else(|_| u64::from_str_radix(hex, 16).map(|u| u as i64))
                .map_err(|_| self.err(start, "hex literal out of range"))?;
            return Ok(Token::new(TokenKind::Int(val), self.span(start, self.pos)));
        }

        let mut is_float = false;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        if self.peek() == Some('.') {
            // Distinguish `1..2` (dotdot) from `1.2`.
            let after = self.bytes.get(self.pos + 1).copied();
            if after.is_some_and(|b| b.is_ascii_digit()) {
                is_float = true;
                self.bump();
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.bump();
                }
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
        }

        let lit = &self.src[start..self.pos];
        if is_float {
            let v: f64 = lit
                .parse()
                .map_err(|_| self.err(start, "invalid float literal"))?;
            Ok(Token::new(TokenKind::Float(v), self.span(start, self.pos)))
        } else {
            let v: i64 = lit
                .parse()
                .or_else(|_| lit.parse::<u64>().map(|u| u as i64))
                .map_err(|_| self.err(start, "invalid integer literal"))?;
            Ok(Token::new(TokenKind::Int(v), self.span(start, self.pos)))
        }
    }

    fn lex_string(&mut self, start: usize) -> Result<Token, SyntaxError> {
        self.bump(); // "
        let mut buf = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err(start, "unterminated string")),
                Some('"') => break,
                Some('\\') => buf.push(self.lex_escape(start)?),
                Some('$') if self.peek() == Some('$') => {
                    self.bump();
                    buf.push('$');
                }
                Some(ch) => buf.push(ch),
            }
        }
        Ok(Token::new(TokenKind::Str(buf), self.span(start, self.pos)))
    }

    fn lex_char(&mut self, start: usize) -> Result<Token, SyntaxError> {
        self.bump(); // '
        let mut val: i64 = 0;
        let mut shift = 0;
        loop {
            match self.bump() {
                None => return Err(self.err(start, "unterminated character constant")),
                Some('\'') => break,
                Some('\\') => {
                    let ch = self.lex_escape(start)?;
                    val |= (ch as u8 as i64) << shift;
                    shift += 8;
                }
                Some(ch) => {
                    for b in ch.encode_utf8(&mut [0; 4]).bytes() {
                        val |= (b as i64) << shift;
                        shift += 8;
                    }
                }
            }
            if shift > 64 {
                return Err(self.err(start, "character constant longer than 8 bytes"));
            }
        }
        Ok(Token::new(TokenKind::Char(val), self.span(start, self.pos)))
    }

    fn lex_escape(&mut self, start: usize) -> Result<char, SyntaxError> {
        match self.bump() {
            Some('n') => Ok('\n'),
            Some('t') => Ok('\t'),
            Some('r') => Ok('\r'),
            Some('0') => Ok('\0'),
            Some('\\') => Ok('\\'),
            Some('"') => Ok('"'),
            Some('\'') => Ok('\''),
            Some('x') => {
                let h1 = self.bump().ok_or_else(|| self.err(start, "bad \\x escape"))?;
                let h2 = self.bump().ok_or_else(|| self.err(start, "bad \\x escape"))?;
                let s = format!("{h1}{h2}");
                let v = u8::from_str_radix(&s, 16)
                    .map_err(|_| self.err(start, "bad \\x escape"))?;
                Ok(v as char)
            }
            Some(ch) => Ok(ch),
            None => Err(self.err(start, "unterminated escape")),
        }
    }

    /// `$...$` DolDoc. `$$` is the `$$` token. `$IB,"..",BI=n$` becomes InsBin.
    fn lex_dollar(&mut self, start: usize) -> Result<Token, SyntaxError> {
        self.bump(); // $
        if self.peek() == Some('$') {
            self.bump();
            self.line_start = false;
            return Ok(Token::new(
                TokenKind::DollarDollar,
                self.span(start, self.pos),
            ));
        }
        let cmd_start = self.pos;
        while let Some(ch) = self.peek() {
            if ch == '$' {
                break;
            }
            if ch == '\n' {
                self.line_start = true;
            }
            self.bump();
        }
        if self.peek() != Some('$') {
            return Err(self.err(start, "unterminated DolDoc command"));
        }
        let cmd = &self.src[cmd_start..self.pos];
        self.bump(); // closing $

        let kind = parse_doldoc_cmd(cmd).unwrap_or(None);
        match kind {
            Some(k) => {
                self.line_start = false;
                Ok(Token::new(k, self.span(start, self.pos)))
            }
            None => {
                // Text / widget DolDoc: skip, continue lexing.
                self.next_token()
            }
        }
    }
}

fn parse_doldoc_cmd(cmd: &str) -> Option<Option<TokenKind>> {
    // Returns None = not a cmd we understand as skip? We always understand skip.
    // Some(None) = skip, Some(Some(tok)) = emit.
    let cmd = cmd.trim();
    let tag = cmd.split([',', '+']).next().unwrap_or(cmd).trim();
    match tag {
        "IB" => {
            let idx = parse_bi(cmd).unwrap_or(0);
            Some(Some(TokenKind::InsBin { idx }))
        }
        "BS" => {
            let idx = parse_bi(cmd).unwrap_or(0);
            Some(Some(TokenKind::InsBinSize { idx }))
        }
        _ => Some(None),
    }
}

fn parse_bi(cmd: &str) -> Option<i64> {
    let rest = cmd.split("BI=").nth(1)?;
    let num: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    num.parse().ok()
}

fn is_ident_start(ch: char) -> bool {
    ch == '_'
        || ch.is_ascii_alphabetic()
        || (!ch.is_ascii() && ch.is_alphabetic())
        || ch == 'π'
        || ch == '∞'
        || ch == 'θ'
        || ch == 'φ'
}

fn is_ident_continue(ch: char) -> bool {
    is_ident_start(ch) || ch.is_ascii_digit() || ch == '@'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        let toks = Lexer::new(src, "t.HC", 0).tokenize().unwrap();
        toks.into_iter()
            .filter(|t| !matches!(t.kind, TokenKind::Eof))
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn hello_tokens() {
        let k = kinds("U0 Main()\n{\n  \"Hello world\\n\";\n}\nMain;\n");
        assert!(matches!(k[0], TokenKind::Ident(ref s) if s == "U0"));
        assert!(matches!(k[1], TokenKind::Ident(ref s) if s == "Main"));
        assert!(matches!(&k[5], TokenKind::Str(s) if s == "Hello world\n"));
    }

    #[test]
    fn doldoc_skip_and_ib() {
        let k = kinds("$WW,1$ x $IB,\"<1>\",BI=2$");
        assert!(matches!(k[0], TokenKind::Ident(ref s) if s == "x"));
        assert!(matches!(k[1], TokenKind::InsBin { idx: 2 }));
    }

    #[test]
    fn dollar_dollar() {
        let k = kinds("$$");
        assert!(matches!(k[0], TokenKind::DollarDollar));
    }
}

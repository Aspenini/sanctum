use crate::error::SyntaxError;
use crate::lexer::Lexer;
use crate::span::{FileId, SourceFile, Span};
use crate::token::{Token, TokenKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// TempleOS source files use Code Page 437 and may have a NUL-delimited binary
// DolDoc payload appended after the source text.
const CP437_HIGH: &str = concat!(
    "ÇüéâäàåçêëèïîìÄÅ",
    "ÉæÆôöòûùÿÖÜ¢£¥₧ƒ",
    "áíóúñÑªº¿⌐¬½¼¡«»",
    "░▒▓│┤╡╢╖╕╣║╗╝╜╛┐",
    "└┴┬├─┼╞╟╚╔╩╦╠═╬╧",
    "╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀",
    "αßΓπΣσµτΦΘΩδ∞φε∩",
    "≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ",
);

fn decode_source(bytes: &[u8]) -> String {
    let text = bytes
        .iter()
        .position(|byte| *byte == 0)
        .map_or(bytes, |end| &bytes[..end]);
    if let Ok(utf8) = std::str::from_utf8(text) {
        return utf8.to_owned();
    }
    let high: Vec<char> = CP437_HIGH.chars().collect();
    text.iter()
        .map(|byte| {
            if byte.is_ascii() {
                char::from(*byte)
            } else {
                high[usize::from(*byte) - 0x80]
            }
        })
        .collect()
}

fn read_source(path: &Path) -> Result<(String, Vec<u8>), SyntaxError> {
    let bytes = std::fs::read(path).map_err(|e| SyntaxError::Io(format!("{path:?}: {e}")))?;
    let binary_tail = bytes
        .iter()
        .position(|byte| *byte == 0)
        .map_or_else(Vec::new, |end| bytes[end + 1..].to_vec());
    Ok((decode_source(&bytes), binary_tail))
}

pub struct PreprocessOpts {
    pub include_dirs: Vec<PathBuf>,
    /// `::/` → this directory (vendored TempleOS root).
    pub system_root: Option<PathBuf>,
}

impl Default for PreprocessOpts {
    fn default() -> Self {
        Self {
            include_dirs: Vec::new(),
            system_root: None,
        }
    }
}

pub struct Session {
    pub files: Vec<SourceFile>,
    next_id: u32,
}

impl Session {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            next_id: 0,
        }
    }

    pub fn add_file(&mut self, path: PathBuf, src: String) -> FileId {
        self.add_file_with_binary(path, src, Vec::new())
    }

    fn add_file_with_binary(&mut self, path: PathBuf, src: String, binary_tail: Vec<u8>) -> FileId {
        let id = FileId(self.next_id);
        self.next_id += 1;
        self.files.push(SourceFile {
            id: FileId(id.0),
            path,
            src,
            binary_tail,
        });
        id
    }

    pub fn file(&self, id: u32) -> Option<&SourceFile> {
        self.files.iter().find(|f| f.id.0 == id)
    }
}

/// Expand `#include` / `#define` / `#ifdef` and drop other `#` directives
/// (`#help_index`, `#assert` until implemented, `#exe` skipped with a warning-as-skip).
pub fn preprocess(
    session: &mut Session,
    path: &Path,
    opts: &PreprocessOpts,
) -> Result<Vec<Token>, SyntaxError> {
    let (src, binary_tail) = read_source(path)?;
    let id = session.add_file_with_binary(path.to_path_buf(), src, binary_tail);
    let file = session.file(id.0).unwrap();
    let src = file.src.clone();
    let path_str = path.display().to_string();
    let tokens = Lexer::new(&src, &path_str, id.0).tokenize()?;
    let mut ctx = PpCtx {
        session,
        opts,
        defines: HashMap::new(),
        include_stack: vec![path.to_path_buf()],
    };
    ctx.expand(&tokens, path)
}

struct PpCtx<'a> {
    session: &'a mut Session,
    opts: &'a PreprocessOpts,
    defines: HashMap<String, Vec<Token>>,
    include_stack: Vec<PathBuf>,
}

impl<'a> PpCtx<'a> {
    fn expand(&mut self, tokens: &[Token], from: &Path) -> Result<Vec<Token>, SyntaxError> {
        let mut out = Vec::new();
        let mut i = 0;
        // skip-stack: true means currently emitting
        let mut emitting = vec![true];

        while i < tokens.len() {
            if matches!(tokens[i].kind, TokenKind::Eof) {
                break;
            }
            if matches!(tokens[i].kind, TokenKind::Hash) {
                i += 1;
                let Some(name) = tokens
                    .get(i)
                    .and_then(|t| t.kind.ident().map(|s| s.to_string()))
                else {
                    return Err(SyntaxError::at(
                        &from.display().to_string(),
                        "",
                        tokens.get(i).map(|t| t.span).unwrap_or(Span::dummy()),
                        "expected preprocessor directive name",
                    ));
                };
                i += 1;
                match name.as_str() {
                    "include" => {
                        if !emitting.last().copied().unwrap_or(true) {
                            self.skip_to_eol(tokens, &mut i);
                            continue;
                        }
                        let Some(TokenKind::Str(inc)) = tokens.get(i).map(|t| t.kind.clone())
                        else {
                            return Err(SyntaxError::Io(format!(
                                "{}: #include expects a string",
                                from.display()
                            )));
                        };
                        i += 1;
                        self.expect_semi_or_eol(tokens, &mut i);
                        let nested = self.include_file(from, &inc)?;
                        out.extend(nested);
                    }
                    "define" => {
                        if !emitting.last().copied().unwrap_or(true) {
                            self.skip_to_eol(tokens, &mut i);
                            continue;
                        }
                        let Some(ident) = tokens
                            .get(i)
                            .and_then(|t| t.kind.ident().map(str::to_string))
                        else {
                            return Err(SyntaxError::Io("#define needs a name".into()));
                        };
                        i += 1;
                        let def_file = tokens.get(i.saturating_sub(1)).map(|t| t.span.file);
                        let def_line = tokens.get(i).and_then(|t| {
                            let f = self.session.file(t.span.file)?;
                            Some(crate::error::line_col(&f.src, t.span.start as usize).0)
                        });
                        let mut body = Vec::new();
                        while i < tokens.len()
                            && !matches!(tokens[i].kind, TokenKind::Eof)
                            && !at_directive(&tokens, i)
                        {
                            if matches!(tokens[i].kind, TokenKind::Hash) {
                                break;
                            }
                            if let Some(file) = self.session.file(tokens[i].span.file) {
                                let line = crate::error::line_col(
                                    &file.src,
                                    tokens[i].span.start as usize,
                                )
                                .0;
                                if Some(tokens[i].span.file) == def_file
                                    && def_line.is_some_and(|dl| line > dl)
                                {
                                    break;
                                }
                            }
                            if matches!(tokens[i].kind, TokenKind::Semicolon) {
                                // `;` on the same line is part of some TOS macros; still stop.
                                i += 1;
                                break;
                            }
                            body.push(tokens[i].clone());
                            i += 1;
                        }
                        self.defines.insert(ident, body);
                    }
                    "ifdef" | "ifndef" | "if" => {
                        let cond = self.eval_if(&name, tokens, &mut i);
                        let parent = emitting.last().copied().unwrap_or(true);
                        emitting.push(parent && cond);
                    }
                    "else" => {
                        self.skip_to_eol(tokens, &mut i);
                        if let Some(last) = emitting.last_mut() {
                            *last = !*last;
                            // if parent is off, stay off
                        }
                        if emitting.len() >= 2 && !emitting[emitting.len() - 2] {
                            if let Some(last) = emitting.last_mut() {
                                *last = false;
                            }
                        }
                    }
                    "endif" => {
                        self.skip_to_eol(tokens, &mut i);
                        emitting.pop();
                        if emitting.is_empty() {
                            emitting.push(true);
                        }
                    }
                    // Ignored compiler directives (TempleOS leftover).
                    "help_index" | "help_file" | "assert" | "exe" => {
                        self.skip_balanced_or_eol(tokens, &mut i, &name);
                    }
                    _ => {
                        self.skip_to_eol(tokens, &mut i);
                    }
                }
                continue;
            }

            if !emitting.last().copied().unwrap_or(true) {
                i += 1;
                continue;
            }

            if let Some(ident) = tokens[i].kind.ident() {
                if let Some(body) = self.defines.get(ident).cloned() {
                    out.extend(body);
                    i += 1;
                    continue;
                }
            }
            out.push(tokens[i].clone());
            i += 1;
        }
        Ok(out)
    }

    fn eval_if(&mut self, kind: &str, tokens: &[Token], i: &mut usize) -> bool {
        match kind {
            "ifdef" => {
                let name = tokens
                    .get(*i)
                    .and_then(|t| t.kind.ident().map(str::to_string));
                if name.is_some() {
                    *i += 1;
                }
                self.skip_to_eol(tokens, i);
                name.is_some_and(|n| self.defines.contains_key(&n))
            }
            "ifndef" => {
                let name = tokens
                    .get(*i)
                    .and_then(|t| t.kind.ident().map(str::to_string));
                if name.is_some() {
                    *i += 1;
                }
                self.skip_to_eol(tokens, i);
                name.is_none_or(|n| !self.defines.contains_key(&n))
            }
            _ => {
                // `#if 0` / `#if 1` / `#if defined(X)`
                let mut val = true;
                if let Some(tok) = tokens.get(*i) {
                    match &tok.kind {
                        TokenKind::Int(n) => val = *n != 0,
                        TokenKind::Ident(s) if s == "defined" => {
                            *i += 1;
                            let name;
                            if matches!(tokens.get(*i).map(|t| &t.kind), Some(TokenKind::LParen)) {
                                *i += 1;
                                name = tokens
                                    .get(*i)
                                    .and_then(|t| t.kind.ident().map(str::to_string));
                                *i += 1;
                                if matches!(
                                    tokens.get(*i).map(|t| &t.kind),
                                    Some(TokenKind::RParen)
                                ) {
                                    *i += 1;
                                }
                            } else {
                                name = tokens
                                    .get(*i)
                                    .and_then(|t| t.kind.ident().map(str::to_string));
                                *i += 1;
                            }
                            val = name.is_some_and(|n| self.defines.contains_key(&n));
                        }
                        _ => val = true,
                    }
                }
                self.skip_to_eol(tokens, i);
                val
            }
        }
    }

    fn skip_to_eol(&self, tokens: &[Token], i: &mut usize) {
        // Without line numbers on every token, skip until Hash or Eof — but that
        // would swallow the next directive. Directives are themselves Hash, so
        // stop before the next Hash. For `#define` we already consumed the body.
        // For `#include "x"` we consumed the string. Remaining junk until `;` or Hash.
        while *i < tokens.len() {
            match tokens[*i].kind {
                TokenKind::Hash | TokenKind::Eof => break,
                TokenKind::Semicolon => {
                    *i += 1;
                    break;
                }
                _ => *i += 1,
            }
        }
    }

    fn skip_balanced_or_eol(&self, tokens: &[Token], i: &mut usize, name: &str) {
        if name == "exe" {
            // `#exe { ... }` — skip a brace block if present.
            while *i < tokens.len()
                && !matches!(
                    tokens[*i].kind,
                    TokenKind::LBrace | TokenKind::Hash | TokenKind::Eof
                )
            {
                *i += 1;
            }
            if matches!(tokens.get(*i).map(|t| &t.kind), Some(TokenKind::LBrace)) {
                let mut depth = 0;
                while *i < tokens.len() {
                    match tokens[*i].kind {
                        TokenKind::LBrace => depth += 1,
                        TokenKind::RBrace => {
                            depth -= 1;
                            *i += 1;
                            if depth == 0 {
                                break;
                            }
                            continue;
                        }
                        TokenKind::Eof => break,
                        _ => {}
                    }
                    *i += 1;
                }
            }
            return;
        }
        self.skip_to_eol(tokens, i);
    }

    fn expect_semi_or_eol(&self, tokens: &[Token], i: &mut usize) {
        if matches!(tokens.get(*i).map(|t| &t.kind), Some(TokenKind::Semicolon)) {
            *i += 1;
        }
    }

    fn include_file(&mut self, from: &Path, inc: &str) -> Result<Vec<Token>, SyntaxError> {
        let path = resolve_include(from, inc, self.opts)?;
        if self.include_stack.iter().any(|p| p == &path) {
            return Err(SyntaxError::Io(format!(
                "cyclic #include of {}",
                path.display()
            )));
        }
        let (src, binary_tail) = read_source(&path)?;
        let id = self
            .session
            .add_file_with_binary(path.clone(), src, binary_tail);
        let file = self.session.file(id.0).unwrap();
        let src = file.src.clone();
        let path_str = path.display().to_string();
        let tokens = Lexer::new(&src, &path_str, id.0).tokenize()?;
        self.include_stack.push(path.clone());
        let out = self.expand(&tokens, &path)?;
        self.include_stack.pop();
        Ok(out)
    }
}

fn at_directive(tokens: &[Token], i: usize) -> bool {
    matches!(tokens.get(i).map(|t| &t.kind), Some(TokenKind::Hash))
}

fn resolve_include(from: &Path, inc: &str, opts: &PreprocessOpts) -> Result<PathBuf, SyntaxError> {
    let inc = inc.replace('\\', "/");
    if let Some(rest) = inc.strip_prefix("::/") {
        if let Some(root) = &opts.system_root {
            let p = root.join(rest);
            if p.exists() {
                return Ok(p);
            }
        }
    }
    if let Some(parent) = from.parent() {
        let p = parent.join(&inc);
        if p.exists() {
            return Ok(p);
        }
    }
    for dir in &opts.include_dirs {
        let p = dir.join(&inc);
        if p.exists() {
            return Ok(p);
        }
    }
    Err(SyntaxError::Io(format!("cannot find include `{inc}`")))
}

/// Lex a complete in-memory buffer (no includes). Used by tests and `--run` of snippets.
pub fn lex_buffer(session: &mut Session, path: &str, src: &str) -> Result<Vec<Token>, SyntaxError> {
    let id = session.add_file(PathBuf::from(path), src.to_string());
    let file = session.file(id.0).unwrap();
    let owned = file.src.clone();
    let mut ctx = PpCtx {
        session,
        opts: &PreprocessOpts::default(),
        defines: HashMap::new(),
        include_stack: vec![PathBuf::from(path)],
    };
    let tokens = Lexer::new(&owned, path, id.0).tokenize()?;
    ctx.expand(&tokens, Path::new(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_cp437_and_ignores_appended_binary() {
        assert_eq!(CP437_HIGH.chars().count(), 128);
        assert_eq!(decode_source(b"F64 \xE3;\0\xFF\x00"), "F64 π;");
    }

    #[test]
    fn preserves_utf8_sources() {
        assert_eq!(decode_source("F64 π;".as_bytes()), "F64 π;");
    }
}

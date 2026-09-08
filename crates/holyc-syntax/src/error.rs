use crate::span::Span;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyntaxError {
    #[error("{path}:{line}:{col}: {message}")]
    Message {
        path: String,
        line: usize,
        col: usize,
        message: String,
        span: Span,
    },
    #[error("{0}")]
    Io(String),
}

impl SyntaxError {
    pub fn at(path: &str, src: &str, span: Span, message: impl Into<String>) -> Self {
        let (line, col) = line_col(src, span.start as usize);
        Self::Message {
            path: path.to_string(),
            line,
            col,
            message: message.into(),
            span,
        }
    }

    pub fn span(&self) -> Option<Span> {
        match self {
            Self::Message { span, .. } => Some(*span),
            Self::Io(_) => None,
        }
    }
}

pub fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(src.len());
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, ch) in src.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

//! HolyC lexer, DolDoc-in-source scanner, and preprocessor.

mod error;
mod lexer;
mod preprocess;
mod span;
mod token;

pub use error::{line_col, SyntaxError};
pub use lexer::Lexer;
pub use preprocess::{lex_buffer, preprocess, PreprocessOpts, Session};
pub use span::{FileId, SourceFile, Span};
pub use token::{is_keyword, Token, TokenKind};

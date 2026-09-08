//! HolyC lexer, DolDoc-in-source scanner, and preprocessor.

mod error;
mod lexer;
mod preprocess;
mod span;
mod token;

pub use error::{SyntaxError, line_col};
pub use lexer::Lexer;
pub use preprocess::{PreprocessOpts, Session, lex_buffer, preprocess};
pub use span::{FileId, SourceFile, Span};
pub use token::{Token, TokenKind, is_keyword};

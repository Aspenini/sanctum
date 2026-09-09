//! Source locations.

use std::fmt;
use std::path::{Path, PathBuf};

/// File identity in a compile session.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FileId(pub u32);

#[derive(Clone, Debug)]
pub struct SourceFile {
    pub id: FileId,
    pub path: PathBuf,
    pub src: String,
    /// Bytes following the first NUL in a TempleOS source document.
    pub binary_tail: Vec<u8>,
}

impl SourceFile {
    pub fn name(&self) -> &Path {
        &self.path
    }
}

/// Byte span within a file. `end` is exclusive.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Span {
    pub file: u32,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file: u32, start: usize, end: usize) -> Self {
        Self {
            file,
            start: start as u32,
            end: end as u32,
        }
    }

    pub fn dummy() -> Self {
        Self {
            file: 0,
            start: 0,
            end: 0,
        }
    }

    pub fn merge(self, other: Span) -> Span {
        Span {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}..{}", self.file, self.start, self.end)
    }
}

//! DolDoc runtime. Compiler lexer already skips `$` text cmds and emits `$IB`.
//! Phase 4 stores sprite bins and walks `CSprite` streams.

#[derive(Clone, Debug, Default)]
pub struct DocBin {
    pub idx: i64,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub bins: Vec<DocBin>,
}

impl Doc {
    pub fn bin_ptr(&self, idx: i64) -> *const u8 {
        self.bins
            .iter()
            .find(|b| b.idx == idx)
            .map(|b| b.bytes.as_ptr())
            .unwrap_or(std::ptr::null())
    }
}

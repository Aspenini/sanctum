//! DolDoc runtime. Compiler lexer already skips `$` text cmds and emits `$IB`.
//! Phase 4 stores sprite bins and walks `CSprite` streams.

use std::collections::HashSet;

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

#[derive(Clone, Copy, Debug)]
struct BinHeader {
    offset: usize,
    idx: i64,
    size: usize,
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn header_at(bytes: &[u8], offset: usize) -> Option<BinHeader> {
    let idx = i64::from(u32_at(bytes, offset)?);
    let flags = u32_at(bytes, offset + 4)?;
    let size = usize::try_from(u32_at(bytes, offset + 8)?).ok()?;
    let use_count = u32_at(bytes, offset + 12)?;
    let first_sprite_type = *bytes.get(offset + 16)? & 0x7f;
    if flags != 0
        || size == 0
        || use_count == 0
        || use_count > 64
        || first_sprite_type >= 30
        || idx > 1_000_000
    {
        return None;
    }
    Some(BinHeader { offset, idx, size })
}

fn next_header(
    bytes: &[u8],
    expected_offset: usize,
    after_offset: usize,
    expected: &HashSet<i64>,
    seen: &HashSet<i64>,
) -> Option<BinHeader> {
    let lo = expected_offset.saturating_sub(64);
    let hi = expected_offset
        .saturating_add(64)
        .min(bytes.len().saturating_sub(16));
    (lo..=hi)
        .filter(|offset| *offset > after_offset)
        .filter_map(|offset| header_at(bytes, offset))
        .min_by_key(|header| {
            let known_rank = if header.idx != 0 && !seen.contains(&header.idx) {
                if expected.contains(&header.idx) { 0 } else { 1 }
            } else if header.idx == 0 {
                2
            } else {
                3
            };
            (known_rank, header.offset.abs_diff(expected_offset))
        })
}

fn i32_at(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// Return the serialized size of a well-formed `CSprite` stream.
pub fn sprite_size(bytes: &[u8]) -> Option<usize> {
    const BASE: [usize; 30] = [
        1, 2, 3, 5, 17, 1, 1, 9, 9, 13, 17, 5, 17, 25, 13, 25, 29, 5, 5, 5, 5, 9, 9, 17, 9, 21, 17,
        9, 9, 9,
    ];
    let mut offset = 0usize;
    loop {
        let ty = usize::from(*bytes.get(offset)? & 0x7f);
        if ty == 0 {
            return Some(offset + 1);
        }
        let mut size = *BASE.get(ty)?;
        let count = |at: usize| usize::try_from(i32_at(bytes, offset + at)?).ok();
        match ty {
            9 => size = size.checked_add((count(1)?.checked_mul(3)? + 7) >> 3)?,
            11 => size = size.checked_add(count(1)?.checked_mul(8)?)?,
            17..=20 => size = size.checked_add(count(1)?.checked_mul(12)?)?,
            23 => {
                let width = count(9)?;
                let height = count(13)?;
                size = size.checked_add(((width + 7) & !7).checked_mul(height)?)?;
            }
            24 => {
                size = size.checked_add(count(1)?.checked_mul(12)?)?;
                size = size.checked_add(count(5)?.checked_mul(16)?)?;
            }
            25 => {
                size = size.checked_add(count(13)?.checked_mul(12)?)?;
                size = size.checked_add(count(17)?.checked_mul(16)?)?;
            }
            27..=29 => {
                let string = bytes.get(offset + 9..)?;
                size = size.checked_add(string.iter().position(|byte| *byte == 0)? + 1)?;
            }
            _ => {}
        }
        if size == 0 || offset.checked_add(size)? > bytes.len() {
            return None;
        }
        offset += size;
    }
}

fn repair_payload(mut raw: Vec<u8>, size: usize) -> Vec<u8> {
    if raw.len() > size {
        raw.truncate(size);
    }
    let deficit = size.saturating_sub(raw.len());
    let mut padded = raw.clone();
    padded.resize(size, 0);
    if sprite_size(&padded) == Some(size) {
        return padded;
    }

    // Some text-mode copies of TempleOS sources lose one control byte from a
    // bitmap header. Recover it from the bin's authoritative serialized size.
    if deficit == 1 && raw.first().is_some_and(|ty| ty & 0x7f == 23) {
        for offset in 1..=raw.len().min(17) {
            for byte in 0u8..=u8::MAX {
                let mut candidate = Vec::with_capacity(size);
                candidate.extend_from_slice(&raw[..offset]);
                candidate.push(byte);
                candidate.extend_from_slice(&raw[offset..]);
                if sprite_size(&candidate) == Some(size) {
                    return candidate;
                }
            }
        }
    }
    padded
}

/// Parse the NUL-delimited binary tail of a TempleOS source document.
///
/// Original TempleOS files are occasionally copied through text-only tools,
/// which can drop a few control or trailing NUL bytes. The stored bin size is
/// authoritative, so parsing resynchronizes within a small bounded window and
/// zero-pads truncated payloads. `expected_indices` comes from the source's
/// `$IB`/`$SP` tokens and prevents random sprite bytes from becoming headers.
pub fn parse_embedded_bins(bytes: &[u8], expected_indices: &[i64]) -> Vec<DocBin> {
    let expected: HashSet<i64> = expected_indices.iter().copied().collect();
    let Some(mut header) = (0..bytes.len().min(64)).find_map(|offset| header_at(bytes, offset))
    else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let mut seen = HashSet::new();

    loop {
        let mut idx = header.idx;
        if idx == 0 {
            idx = result
                .last()
                .map(|bin: &DocBin| bin.idx + 1)
                .filter(|candidate| expected.contains(candidate) && !seen.contains(candidate))
                .or_else(|| {
                    expected
                        .iter()
                        .copied()
                        .find(|candidate| !seen.contains(candidate))
                })
                .unwrap_or(0);
        }
        let data_start = header.offset + 16;
        let nominal_end = data_start.saturating_add(header.size);
        let next = next_header(bytes, nominal_end, header.offset, &expected, &seen);
        let actual_end = next.map_or(bytes.len(), |candidate| candidate.offset);
        if idx > 0 && data_start <= actual_end && data_start <= bytes.len() {
            let raw = bytes[data_start..actual_end.min(bytes.len())].to_vec();
            result.push(DocBin {
                idx,
                bytes: repair_payload(raw, header.size),
            });
            seen.insert(idx);
        }
        let Some(candidate) = next else { break };
        if candidate.offset <= header.offset || seen.len() > expected.len().saturating_add(16) {
            break;
        }
        header = candidate;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(idx: u32, size: u32, use_count: u32) -> Vec<u8> {
        [idx, 0, size, use_count]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect()
    }

    #[test]
    fn walks_line_sprite() {
        let mut sprite = vec![1, 3, 10];
        sprite.extend_from_slice(&[0; 16]);
        sprite.push(0);
        assert_eq!(sprite_size(&sprite), Some(20));
    }

    #[test]
    fn resynchronizes_and_pads_truncated_bins() {
        let sprite1 = [1, 4, 0];
        let sprite2 = [1, 7];
        let mut tail = header(1, sprite1.len() as u32, 1);
        tail.extend_from_slice(&sprite1);
        tail.extend_from_slice(&header(2, 3, 1));
        tail.extend_from_slice(&sprite2);
        let bins = parse_embedded_bins(&tail, &[1, 2]);
        assert_eq!(bins.len(), 2);
        assert_eq!(bins[0].bytes, sprite1);
        assert_eq!(bins[1].bytes, [1, 7, 0]);
    }

    #[test]
    fn parses_talons_embedded_sprites_when_checkout_is_present() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../TempleOS/Demo/Games/Talons.HC");
        let Ok(file) = std::fs::read(path) else {
            return;
        };
        let tail = file
            .iter()
            .position(|byte| *byte == 0)
            .map(|nul| &file[nul + 1..])
            .unwrap_or_default();
        // Bins 9 and 10 are only mentioned by source-display `$SP` tags; the
        // runtime `$IB` references still need the parser to cross them.
        let bins = parse_embedded_bins(tail, &[1, 2, 3, 4, 5, 6, 7, 8]);
        let mut indices: Vec<_> = bins.iter().map(|bin| bin.idx).collect();
        indices.sort_unstable();
        assert_eq!(indices, (1..=10).collect::<Vec<_>>());
        for bin in bins {
            let sprite_len = sprite_size(&bin.bytes).expect("valid leading sprite stream");
            assert!(sprite_len <= bin.bytes.len(), "bin {}", bin.idx);
        }
    }
}

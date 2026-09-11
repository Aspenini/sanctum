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
    if expected_offset >= bytes.len() {
        return None;
    }
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

fn sprite_element_size(bytes: &[u8]) -> Option<usize> {
    let ty = usize::from(*bytes.first()? & 0x7f);
    const BASE: [usize; 30] = [
        1, 2, 3, 5, 17, 1, 1, 9, 9, 13, 17, 5, 17, 25, 13, 25, 29, 5, 5, 5, 5, 9, 9, 17, 9, 21, 17,
        9, 9, 9,
    ];
    let mut size = *BASE.get(ty)?;
    let count = |at: usize| usize::try_from(i32_at(bytes, at)?).ok();
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
            size = size.checked_add(bytes.get(9..)?.iter().position(|byte| *byte == 0)? + 1)?;
        }
        _ => {}
    }
    Some(size)
}

#[derive(Clone, Copy)]
struct FieldStep {
    score: i64,
    previous_missing: usize,
    bytes: [u8; 4],
}

fn inserted_five_fields(raw: &[u8], missing: usize) -> impl Iterator<Item = [u8; 4]> + '_ {
    (0_u8..16).filter_map(move |mask| {
        if mask.count_ones() as usize != missing {
            return None;
        }
        let mut source = 0usize;
        let mut bytes = [0u8; 4];
        for (index, byte) in bytes.iter_mut().enumerate() {
            if mask & (1 << index) != 0 {
                *byte = 5;
            } else {
                *byte = *raw.get(source)?;
                source += 1;
            }
        }
        (source == raw.len()).then_some(bytes)
    })
}

fn mesh_field_score(
    field: usize,
    vertex_fields: usize,
    vertex_count: i32,
    bytes: [u8; 4],
) -> Option<i64> {
    let value = i32::from_le_bytes(bytes);
    if field < vertex_fields {
        let magnitude = i64::from(value).unsigned_abs();
        if magnitude > 1_000_000 {
            return None;
        }
        return Some(1_000 - i64::try_from(magnitude.min(999)).ok()?);
    }
    match (field - vertex_fields) % 4 {
        0 => {
            let color = value as u32;
            ((color & 0xff) < 16).then_some(if color & !0x300f == 0 { 500 } else { 100 })
        }
        _ => (0..vertex_count).contains(&value).then_some(2_000),
    }
}

fn repair_mesh_element(raw: &[u8], original_size: usize, missing: usize) -> Option<(Vec<u8>, i64)> {
    let ty = *raw.first()? & 0x7f;
    let (base, vertex_count, triangle_count) = match ty {
        24 => (9usize, i32_at(raw, 1)?, i32_at(raw, 5)?),
        25 => (21usize, i32_at(raw, 13)?, i32_at(raw, 17)?),
        _ => return None,
    };
    if vertex_count < 0 || triangle_count < 0 || raw.len() < base {
        return None;
    }
    let vertex_fields = usize::try_from(vertex_count).ok()?.checked_mul(3)?;
    let fields =
        vertex_fields.checked_add(usize::try_from(triangle_count).ok()?.checked_mul(4)?)?;
    if original_size != base.checked_add(fields.checked_mul(4)?)?
        || raw.len() != original_size.checked_sub(missing)?
    {
        return None;
    }
    let field_raw = &raw[base..];
    let mut table = vec![vec![None::<FieldStep>; missing + 1]; fields + 1];
    table[0][0] = Some(FieldStep {
        score: 0,
        previous_missing: 0,
        bytes: [0; 4],
    });
    for field in 0..fields {
        for used in 0..=missing {
            let Some(previous) = table[field][used] else {
                continue;
            };
            let source_offset = field.checked_mul(4)?.checked_sub(used)?;
            for added in 0..=4.min(missing - used) {
                let consumed = 4 - added;
                let Some(source) = field_raw.get(source_offset..source_offset + consumed) else {
                    continue;
                };
                for bytes in inserted_five_fields(source, added) {
                    let Some(field_score) =
                        mesh_field_score(field, vertex_fields, vertex_count, bytes)
                    else {
                        continue;
                    };
                    let score = previous.score + field_score;
                    let next = &mut table[field + 1][used + added];
                    if next.is_none_or(|current| score > current.score) {
                        *next = Some(FieldStep {
                            score,
                            previous_missing: used,
                            bytes,
                        });
                    }
                }
            }
        }
    }
    let final_step = table[fields][missing]?;
    let mut repaired_fields = vec![[0u8; 4]; fields];
    let mut used = missing;
    for field in (1..=fields).rev() {
        let step = table[field][used]?;
        repaired_fields[field - 1] = step.bytes;
        used = step.previous_missing;
    }
    let mut repaired = Vec::with_capacity(original_size);
    repaired.extend_from_slice(&raw[..base]);
    for field in repaired_fields {
        repaired.extend_from_slice(&field);
    }
    Some((repaired, final_step.score))
}

fn inserted_five_headers(raw: &[u8], missing: usize) -> impl Iterator<Item = Vec<u8>> + '_ {
    let combinations = 1_u32 << 17;
    (0..combinations).filter_map(move |mask| {
        if mask.count_ones() as usize != missing {
            return None;
        }
        let mut source = 0usize;
        let mut header = Vec::with_capacity(17);
        for index in 0..17 {
            if mask & (1 << index) != 0 {
                header.push(5);
            } else {
                header.push(*raw.get(source)?);
                source += 1;
            }
        }
        (source == raw.len()).then_some(header)
    })
}

fn repair_sprite_stream(
    raw: &[u8],
    original_size: usize,
    missing: usize,
    depth: usize,
) -> Option<(Vec<u8>, i64)> {
    if depth > 1_024 || raw.len().checked_add(missing)? != original_size {
        return None;
    }
    if original_size == 1 {
        return (raw == [0] && missing == 0).then_some((vec![0], 0));
    }
    let ty = *raw.first()? & 0x7f;
    if ty == 0 {
        return None;
    }
    let mut best: Option<(Vec<u8>, i64)> = None;
    if matches!(ty, 24 | 25) {
        let element_size = sprite_element_size(raw)?;
        for element_missing in 0..=missing.min(element_size) {
            let raw_size = element_size.checked_sub(element_missing)?;
            let Some(element_raw) = raw.get(..raw_size) else {
                continue;
            };
            let Some((element, element_score)) =
                repair_mesh_element(element_raw, element_size, element_missing)
            else {
                continue;
            };
            let Some((rest, rest_score)) = repair_sprite_stream(
                &raw[raw_size..],
                original_size.checked_sub(element_size)?,
                missing - element_missing,
                depth + 1,
            ) else {
                continue;
            };
            let score = element_score + rest_score;
            if best.as_ref().is_none_or(|(_, current)| score > *current) {
                let mut bytes = element;
                bytes.extend_from_slice(&rest);
                best = Some((bytes, score));
            }
        }
    } else if ty == 23 {
        for header_missing in 0..=missing.min(4) {
            let Some(header_raw) = raw.get(..17usize.checked_sub(header_missing)?) else {
                continue;
            };
            for header in inserted_five_headers(header_raw, header_missing) {
                let Some(element_size) = sprite_element_size(&header) else {
                    continue;
                };
                let coordinates = [i32_at(&header, 1)?, i32_at(&header, 5)?];
                let dimensions = [i32_at(&header, 9)?, i32_at(&header, 13)?];
                if coordinates
                    .iter()
                    .any(|value| value.unsigned_abs() > 1_000_000)
                    || dimensions.iter().any(|value| !(0..=4096).contains(value))
                {
                    continue;
                }
                for element_missing in header_missing..=missing.min(element_size) {
                    let raw_size = element_size.checked_sub(element_missing)?;
                    if raw_size < header_raw.len() || raw_size > raw.len() {
                        continue;
                    }
                    let payload_missing = element_missing - header_missing;
                    let mut element = header.clone();
                    element.extend_from_slice(&raw[header_raw.len()..raw_size]);
                    element.resize(element_size, 5);
                    let Some((rest, rest_score)) = repair_sprite_stream(
                        &raw[raw_size..],
                        original_size.checked_sub(element_size)?,
                        missing - element_missing,
                        depth + 1,
                    ) else {
                        continue;
                    };
                    let score = rest_score + 2_000
                        - i64::from(coordinates[0].unsigned_abs().min(999))
                        - i64::from(coordinates[1].unsigned_abs().min(999))
                        - i64::try_from(payload_missing).ok()?;
                    if best.as_ref().is_none_or(|(_, current)| score > *current) {
                        element.extend_from_slice(&rest);
                        best = Some((element, score));
                    }
                }
            }
        }
    } else if missing == 0 {
        let element_size = sprite_element_size(raw)?;
        let (rest, score) = repair_sprite_stream(
            raw.get(element_size..)?,
            original_size.checked_sub(element_size)?,
            0,
            depth + 1,
        )?;
        let mut bytes = raw.get(..element_size)?.to_vec();
        bytes.extend_from_slice(&rest);
        best = Some((bytes, score));
    }
    best
}

fn repair_ascii_sprite_payload(raw: &[u8], size: usize, deficit: usize) -> Option<Vec<u8>> {
    let mut best = repair_sprite_stream(raw, size, deficit, 0);
    if deficit > 0 {
        // A deleted 0x05 bin number can make header resynchronization borrow
        // the preceding sprite's zero terminator. Try restoring that byte too.
        let mut with_terminator = raw.to_vec();
        with_terminator.push(0);
        if let Some(candidate) = repair_sprite_stream(&with_terminator, size, deficit - 1, 0)
            && best.as_ref().is_none_or(|(_, score)| candidate.1 > *score)
        {
            best = Some(candidate);
        }
    }
    best.map(|(bytes, _)| bytes)
}

fn repair_payload(mut raw: Vec<u8>, size: usize) -> Vec<u8> {
    if raw.len() > size {
        raw.truncate(size);
    }
    let deficit = size.saturating_sub(raw.len());
    if deficit > 0
        && !raw.contains(&5)
        && let Some(repaired) = repair_ascii_sprite_payload(&raw, size, deficit)
        && sprite_size(&repaired) == Some(size)
    {
        return repaired;
    }

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
    fn restores_deleted_control_bytes_in_mesh_fields() {
        let mut sprite = vec![24];
        sprite.extend_from_slice(&6_i32.to_le_bytes());
        sprite.extend_from_slice(&1_i32.to_le_bytes());
        for value in [
            5_i32, 0, 0, 10, 0, 0, 20, 0, 0, 30, 0, 0, 40, 0, 0, 50, 0, 0,
        ] {
            sprite.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0x3007_i32, 0, 5, 1] {
            sprite.extend_from_slice(&value.to_le_bytes());
        }
        sprite.push(0);
        let converted: Vec<_> = sprite.iter().copied().filter(|byte| *byte != 5).collect();
        let mut tail = header(1, sprite.len() as u32, 1);
        tail.extend_from_slice(&converted);
        let bins = parse_embedded_bins(&tail, &[1]);
        let repaired = &bins[0].bytes;
        assert_eq!(sprite_size(repaired), Some(sprite.len()));
        assert_eq!(i32_at(repaired, 9 + 6 * 12 + 8), Some(5));
        assert_eq!(repaired.iter().filter(|byte| **byte == 5).count(), 2);
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
            let mut offset = 0usize;
            while bin.bytes[offset] & 0x7f != 0 {
                let ty = bin.bytes[offset] & 0x7f;
                let element_size = sprite_element_size(&bin.bytes[offset..]).unwrap();
                if matches!(ty, 24 | 25) {
                    let (base, vertex_count, triangle_count) = if ty == 24 {
                        (
                            9usize,
                            i32_at(&bin.bytes, offset + 1).unwrap(),
                            i32_at(&bin.bytes, offset + 5).unwrap(),
                        )
                    } else {
                        (
                            21usize,
                            i32_at(&bin.bytes, offset + 13).unwrap(),
                            i32_at(&bin.bytes, offset + 17).unwrap(),
                        )
                    };
                    let triangles = offset + base + vertex_count as usize * 12;
                    for triangle in 0..triangle_count as usize {
                        for component in 1..=3 {
                            let index =
                                i32_at(&bin.bytes, triangles + triangle * 16 + component * 4)
                                    .unwrap();
                            assert!(
                                (0..vertex_count).contains(&index),
                                "bin {} triangle {triangle}",
                                bin.idx
                            );
                        }
                    }
                }
                offset += element_size;
            }
        }
    }
}

//! Bounds-checked reader for TempleOS RedSea installation media.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use thiserror::Error;

const SECTOR: usize = 512;
const REDSEA_SIGNATURE: u8 = 0x88;
const BOOT_SIGNATURE: u16 = 0xaa55;
const ATTR_DIR: u16 = 0x10;
const ATTR_DELETED: u16 = 0x100;
const ATTR_COMPRESSED: u16 = 0x400;
const ENTRY_SIZE: usize = 64;
const NAME_SIZE: usize = 38;
const MAX_ENTRIES: usize = 100_000;
const MAX_EXPANDED_FILE: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct ImageInfo {
    pub image_size: u64,
    pub sha256: String,
    pub filesystem_offset: u64,
    pub file_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractProgress {
    pub files_done: u64,
    pub files_total: u64,
    pub path: PathBuf,
}

#[derive(Debug, Error)]
pub enum RedSeaError {
    #[error("unable to read ISO: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a TempleOS RedSea ISO: {0}")]
    Invalid(&'static str),
    #[error("unsafe or duplicate path in ISO: {0}")]
    UnsafePath(String),
    #[error("corrupt compressed file: {0}")]
    Compression(&'static str),
    #[error("installation cancelled")]
    Cancelled,
}

#[derive(Clone)]
struct Entry {
    name: String,
    attr: u16,
    cluster: u64,
    size: u64,
}

struct Image {
    bytes: Vec<u8>,
    base: usize,
    root: u64,
    sectors: u64,
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn le_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

fn locate(bytes: Vec<u8>) -> Result<Image, RedSeaError> {
    if bytes.len() < SECTOR {
        return Err(RedSeaError::Invalid("image is too small"));
    }
    // TempleOS hybrid CDs place the RedSea volume at logical DVD block 19.
    // Also scan the early image so supplemental and ISO.C images are accepted.
    let preferred = 19 * 2048;
    let mut candidates =
        std::iter::once(preferred).chain((0..bytes.len().min(2 * 1024 * 1024)).step_by(SECTOR));
    let base = candidates.find(|&offset| {
        bytes.get(offset + 3) == Some(&REDSEA_SIGNATURE)
            && le_u16(&bytes, offset + 510) == Some(BOOT_SIGNATURE)
    });
    let Some(base) = base else {
        return Err(RedSeaError::Invalid("RedSea boot record was not found"));
    };
    let sectors = le_u64(&bytes, base + 16).ok_or(RedSeaError::Invalid("short boot record"))?;
    let root = le_u64(&bytes, base + 24).ok_or(RedSeaError::Invalid("short boot record"))?;
    let volume_bytes = usize::try_from(sectors)
        .ok()
        .and_then(|value| value.checked_mul(SECTOR))
        .ok_or(RedSeaError::Invalid("volume size overflows"))?;
    if base
        .checked_add(volume_bytes)
        .is_none_or(|end| end > bytes.len())
    {
        return Err(RedSeaError::Invalid("volume exceeds ISO bounds"));
    }
    Ok(Image {
        bytes,
        base,
        root,
        sectors,
    })
}

impl Image {
    fn extent(&self, cluster: u64, size: u64) -> Result<&[u8], RedSeaError> {
        let offset = usize::try_from(cluster)
            .ok()
            .and_then(|cluster| cluster.checked_mul(SECTOR))
            .ok_or(RedSeaError::Invalid("cluster offset overflows"))?;
        let size = usize::try_from(size).map_err(|_| RedSeaError::Invalid("file is too large"))?;
        let end = offset
            .checked_add(size)
            .ok_or(RedSeaError::Invalid("file extent overflows"))?;
        let first_cluster = (self.base / SECTOR) as u64;
        let volume_end = self
            .base
            .checked_add(
                usize::try_from(self.sectors)
                    .ok()
                    .and_then(|sectors| sectors.checked_mul(SECTOR))
                    .ok_or(RedSeaError::Invalid("volume size overflows"))?,
            )
            .ok_or(RedSeaError::Invalid("volume size overflows"))?;
        if cluster < first_cluster
            || cluster >= first_cluster.saturating_add(self.sectors)
            || end > volume_end
        {
            return Err(RedSeaError::Invalid("file extent exceeds ISO bounds"));
        }
        Ok(&self.bytes[offset..end])
    }

    fn directory(&self, cluster: u64) -> Result<Vec<Entry>, RedSeaError> {
        let header = self.extent(cluster, SECTOR as u64)?;
        let size = le_u64(header, 48).ok_or(RedSeaError::Invalid("short directory header"))?;
        if size == 0 || size % ENTRY_SIZE as u64 != 0 {
            return Err(RedSeaError::Invalid("invalid directory size"));
        }
        let bytes = self.extent(cluster, size)?;
        let mut entries = Vec::new();
        for raw in bytes.as_chunks::<ENTRY_SIZE>().0 {
            let name_bytes = &raw[2..2 + NAME_SIZE];
            let end = name_bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(NAME_SIZE);
            if end == 0 {
                break;
            }
            let name = String::from_utf8_lossy(&name_bytes[..end]).into_owned();
            let attr = le_u16(raw, 0).ok_or(RedSeaError::Invalid("short directory entry"))?;
            if attr & ATTR_DELETED == 0 && name != "." && name != ".." {
                entries.push(Entry {
                    name,
                    attr,
                    cluster: le_u64(raw, 40).ok_or(RedSeaError::Invalid("short cluster"))?,
                    size: le_u64(raw, 48).ok_or(RedSeaError::Invalid("short size"))?,
                });
            }
        }
        Ok(entries)
    }

    fn walk(&self) -> Result<Vec<(PathBuf, Entry)>, RedSeaError> {
        let mut output = Vec::new();
        let mut stack = vec![(PathBuf::new(), self.root, 0usize)];
        let mut visited = HashSet::new();
        while let Some((parent, cluster, depth)) = stack.pop() {
            if depth > 128 || !visited.insert(cluster) {
                return Err(RedSeaError::Invalid("recursive directory cycle"));
            }
            for entry in self.directory(cluster)? {
                validate_name(&entry.name)?;
                let path = parent.join(&entry.name);
                if output.len() >= MAX_ENTRIES {
                    return Err(RedSeaError::Invalid("too many directory entries"));
                }
                if entry.attr & ATTR_DIR != 0 {
                    stack.push((path.clone(), entry.cluster, depth + 1));
                }
                output.push((path, entry));
            }
        }
        Ok(output)
    }
}

fn validate_name(name: &str) -> Result<(), RedSeaError> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':', '\0']) {
        return Err(RedSeaError::UnsafePath(name.to_string()));
    }
    Ok(())
}

pub fn inspect_iso(path: &Path) -> Result<ImageInfo, RedSeaError> {
    let bytes = fs::read(path)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let image_size = bytes.len() as u64;
    let image = locate(bytes)?;
    let files = image.walk()?;
    let file_count = files
        .iter()
        .filter(|(_, entry)| entry.attr & ATTR_DIR == 0)
        .count() as u64;
    Ok(ImageInfo {
        image_size,
        sha256: digest,
        filesystem_offset: image.base as u64,
        file_count,
    })
}

pub fn extract_iso(
    path: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(ExtractProgress),
) -> Result<ImageInfo, RedSeaError> {
    let bytes = fs::read(path)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let image_size = bytes.len() as u64;
    let image = locate(bytes)?;
    let entries = image.walk()?;
    let mut seen = HashSet::new();
    let total = entries
        .iter()
        .filter(|(_, entry)| entry.attr & ATTR_DIR == 0)
        .count() as u64;
    fs::create_dir_all(destination)?;
    let mut done = 0;
    for (mut relative, entry) in entries {
        if cancelled.load(Ordering::Acquire) {
            return Err(RedSeaError::Cancelled);
        }
        if entry.attr & ATTR_COMPRESSED != 0 {
            let Some(name) = relative
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
            else {
                return Err(RedSeaError::UnsafePath(relative.display().to_string()));
            };
            if let Some(name) = name.strip_suffix(".Z") {
                relative.set_file_name(name);
            }
        }
        let normalized = relative.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(normalized) {
            return Err(RedSeaError::UnsafePath(relative.display().to_string()));
        }
        let output = destination.join(&relative);
        if entry.attr & ATTR_DIR != 0 {
            fs::create_dir_all(output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let source = image.extent(entry.cluster, entry.size)?;
        let contents = if entry.attr & ATTR_COMPRESSED != 0 {
            expand_archive(source)?
        } else {
            source.to_vec()
        };
        fs::write(&output, contents)?;
        done += 1;
        progress(ExtractProgress {
            files_done: done,
            files_total: total,
            path: relative,
        });
    }
    Ok(ImageInfo {
        image_size,
        sha256: digest,
        filesystem_offset: image.base as u64,
        file_count: total,
    })
}

#[derive(Clone, Copy, Default)]
struct DictEntry {
    base: u16,
    ch: u8,
    next: Option<usize>,
}

struct Decoder {
    entries: [DictEntry; 4096],
    hash: [Option<usize>; 4096],
    min_entry: usize,
    free_idx: usize,
    free_limit: usize,
    current: Option<usize>,
    next: Option<usize>,
    current_bits: usize,
    next_bits: usize,
    entry_used: bool,
}

impl Decoder {
    fn new(min_bits: usize) -> Self {
        let mut decoder = Self {
            entries: [DictEntry::default(); 4096],
            hash: [None; 4096],
            min_entry: 1 << min_bits,
            free_idx: 1 << min_bits,
            free_limit: 1 << (min_bits + 1),
            current: None,
            next: None,
            current_bits: min_bits + 1,
            next_bits: min_bits + 1,
            entry_used: true,
        };
        decoder.advance();
        decoder.entry_used = true;
        decoder
    }

    fn advance(&mut self) {
        if !self.entry_used {
            return;
        }
        let mut index = self.free_idx;
        self.entry_used = false;
        self.current = self.next;
        self.current_bits = self.next_bits;
        if self.next_bits < 12 {
            self.next = Some(index);
            index += 1;
            if index == self.free_limit {
                self.next_bits += 1;
                self.free_limit = 1 << self.next_bits;
            }
        } else {
            loop {
                index += 1;
                if index == self.free_limit {
                    index = self.min_entry;
                }
                if self.hash[index].is_none() {
                    break;
                }
            }
            self.next = Some(index);
            let old = self.entries[index];
            let bucket = old.base as usize;
            let mut previous: Option<usize> = None;
            let mut cursor = self.hash[bucket];
            while let Some(item) = cursor {
                if item == index {
                    let next = self.entries[item].next;
                    if let Some(previous) = previous {
                        self.entries[previous].next = next;
                    } else {
                        self.hash[bucket] = next;
                    }
                    break;
                }
                previous = Some(item);
                cursor = self.entries[item].next;
            }
        }
        self.free_idx = index;
    }

    fn insert(&mut self, base: usize, ch: u8) -> Result<(), RedSeaError> {
        let index = self
            .current
            .ok_or(RedSeaError::Compression("missing dictionary slot"))?;
        if base >= 4096 {
            return Err(RedSeaError::Compression("dictionary code out of range"));
        }
        self.entries[index] = DictEntry {
            base: base as u16,
            ch,
            next: self.hash[base],
        };
        self.hash[base] = Some(index);
        self.entry_used = true;
        Ok(())
    }
}

fn bits(bytes: &[u8], position: usize, count: usize) -> Option<usize> {
    if count == 0 || position.checked_add(count)? > bytes.len().checked_mul(8)? {
        return None;
    }
    let mut value = 0usize;
    for bit in 0..count {
        value |= usize::from((bytes[(position + bit) / 8] >> ((position + bit) % 8)) & 1) << bit;
    }
    Some(value)
}

pub fn expand_archive(archive: &[u8]) -> Result<Vec<u8>, RedSeaError> {
    if archive.len() < 17 {
        return Err(RedSeaError::Compression("short archive header"));
    }
    let compressed = le_u64(archive, 0).ok_or(RedSeaError::Compression("short size"))? as usize;
    let expanded = le_u64(archive, 8).ok_or(RedSeaError::Compression("short size"))? as usize;
    if compressed > archive.len() || expanded > MAX_EXPANDED_FILE {
        return Err(RedSeaError::Compression("invalid archive size"));
    }
    match archive[16] {
        1 => archive
            .get(17..17 + expanded)
            .map(ToOwned::to_owned)
            .ok_or(RedSeaError::Compression("short uncompressed body")),
        kind @ (2 | 3) => {
            let min_bits = if kind == 2 { 7 } else { 8 };
            let mut decoder = Decoder::new(min_bits);
            let mut position = 17 * 8;
            let limit = compressed * 8;
            let mut output = Vec::with_capacity(expanded);
            let mut last = bits(archive, position, decoder.next_bits)
                .ok_or(RedSeaError::Compression("missing first code"))?;
            position += decoder.next_bits;
            if last >= decoder.min_entry {
                return Err(RedSeaError::Compression("invalid first code"));
            }
            output.push(last as u8);
            decoder.advance();
            let mut last_ch = last as u8;
            while output.len() < expanded && position + decoder.next_bits <= limit {
                let code = bits(archive, position, decoder.next_bits)
                    .ok_or(RedSeaError::Compression("truncated code"))?;
                position += decoder.next_bits;
                if code >= 4096 {
                    return Err(RedSeaError::Compression("code exceeds dictionary"));
                }
                let mut stack = Vec::new();
                let mut cursor = if decoder.current == Some(code) {
                    stack.push(last_ch);
                    last
                } else {
                    code
                };
                let mut guard = 0;
                while cursor >= decoder.min_entry {
                    if cursor >= 4096 || guard >= 4096 {
                        return Err(RedSeaError::Compression("dictionary cycle"));
                    }
                    let entry = decoder.entries[cursor];
                    stack.push(entry.ch);
                    cursor = entry.base as usize;
                    guard += 1;
                }
                last_ch = cursor as u8;
                stack.push(last_ch);
                decoder.insert(last, last_ch)?;
                decoder.advance();
                output.extend(stack.into_iter().rev().take(expanded - output.len()));
                last = code;
            }
            (output.len() == expanded)
                .then_some(output)
                .ok_or(RedSeaError::Compression("expanded size mismatch"))
        }
        _ => Err(RedSeaError::Compression("unknown compression type")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn directory_entry(
        bytes: &mut [u8],
        offset: usize,
        name: &str,
        attr: u16,
        cluster: u64,
        size: u64,
    ) {
        bytes[offset..offset + 2].copy_from_slice(&attr.to_le_bytes());
        bytes[offset + 2..offset + 2 + name.len()].copy_from_slice(name.as_bytes());
        put_u64(bytes, offset + 40, cluster);
        put_u64(bytes, offset + 48, size);
    }

    fn synthetic_image() -> Vec<u8> {
        let base_sector = 4_u64;
        let root_cluster = 6_u64;
        let mut bytes = vec![0_u8; 16 * SECTOR];
        let boot = base_sector as usize * SECTOR;
        bytes[boot + 3] = REDSEA_SIGNATURE;
        bytes[boot + 510..boot + 512].copy_from_slice(&BOOT_SIGNATURE.to_le_bytes());
        put_u64(&mut bytes, boot + 8, base_sector);
        put_u64(&mut bytes, boot + 16, 12);
        put_u64(&mut bytes, boot + 24, root_cluster);

        let root = root_cluster as usize * SECTOR;
        directory_entry(
            &mut bytes,
            root,
            ".",
            ATTR_DIR,
            root_cluster,
            4 * ENTRY_SIZE as u64,
        );
        directory_entry(
            &mut bytes,
            root + ENTRY_SIZE,
            "Kernel",
            ATTR_DIR,
            7,
            3 * ENTRY_SIZE as u64,
        );
        let archive_size = 17_usize + 5;
        directory_entry(
            &mut bytes,
            root + 2 * ENTRY_SIZE,
            "ReadMe.TXT.Z",
            ATTR_COMPRESSED,
            9,
            archive_size as u64,
        );

        let kernel = 7 * SECTOR;
        directory_entry(&mut bytes, kernel, ".", ATTR_DIR, 7, 3 * ENTRY_SIZE as u64);
        directory_entry(&mut bytes, kernel + ENTRY_SIZE, "KernelA.HH", 0, 8, 6);
        bytes[8 * SECTOR..8 * SECTOR + 6].copy_from_slice(b"kernel");
        let archive = 9 * SECTOR;
        put_u64(&mut bytes, archive, archive_size as u64);
        put_u64(&mut bytes, archive + 8, 5);
        bytes[archive + 16] = 1;
        bytes[archive + 17..archive + 22].copy_from_slice(b"hello");
        bytes
    }

    #[test]
    fn expands_stored_archive() {
        let body = b"HolyC";
        let mut archive = Vec::new();
        archive.extend_from_slice(&(17_u64 + body.len() as u64).to_le_bytes());
        archive.extend_from_slice(&(body.len() as u64).to_le_bytes());
        archive.push(1);
        archive.extend_from_slice(body);
        assert_eq!(expand_archive(&archive).unwrap(), body);
    }

    #[test]
    fn rejects_unsafe_names() {
        assert!(validate_name("Talons.HC").is_ok());
        assert!(validate_name("../Talons.HC").is_err());
        assert!(validate_name("C:evil").is_err());
    }

    #[test]
    fn extracts_nested_files_from_an_embedded_volume() {
        let root = std::env::temp_dir().join(format!("tos-redsea-extract-{}", std::process::id()));
        let image_path = root.with_extension("iso");
        let _ = fs::remove_dir_all(&root);
        fs::write(&image_path, synthetic_image()).unwrap();
        let info = inspect_iso(&image_path).unwrap();
        assert_eq!(info.filesystem_offset, 4 * SECTOR as u64);
        let cancelled = AtomicBool::new(false);
        let mut progress = Vec::new();
        extract_iso(&image_path, &root, &cancelled, |event| progress.push(event)).unwrap();
        assert_eq!(fs::read(root.join("Kernel/KernelA.HH")).unwrap(), b"kernel");
        assert_eq!(fs::read(root.join("ReadMe.TXT")).unwrap(), b"hello");
        assert_eq!(progress.len(), 2);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(image_path);
    }
}

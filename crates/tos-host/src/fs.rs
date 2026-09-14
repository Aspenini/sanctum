//! Read-only TempleOS file calls, sandboxed to the program directory and `::/`.

use std::cell::RefCell;
use std::ffi::CStr;
use std::fs;
use std::path::{Path, PathBuf};
use tos_abi::{
    CDIR_ENTRY_SIZE, CDIR_FILENAME_LEN, CDirEntry, FUF_JUST_DIRS, FUF_JUST_FILES, FUF_RECURSE,
    FUF_SCAN_PARENTS, FUF_SINGLE, RS_ATTR_COMPRESSED, RS_ATTR_DIR,
};

const MAX_FILE: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Volume {
    Project,
    System,
}

struct FsState {
    project: Option<PathBuf>,
    system: Option<PathBuf>,
    volume: Volume,
    cur_dir: String,
}

impl Default for FsState {
    fn default() -> Self {
        Self {
            project: None,
            system: None,
            volume: Volume::Project,
            cur_dir: "/".into(),
        }
    }
}

thread_local! {
    static STATE: RefCell<FsState> = const { RefCell::new(FsState {
        project: None,
        system: None,
        volume: Volume::Project,
        cur_dir: String::new(),
    }) };
}

fn with_state<T>(f: impl FnOnce(&mut FsState) -> T) -> T {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.cur_dir.is_empty() {
            *state = FsState::default();
        }
        f(&mut state)
    })
}

pub fn reset() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.volume = Volume::Project;
        state.cur_dir = "/".into();
    });
}

pub fn set_roots(project: Option<PathBuf>, system: Option<PathBuf>) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.project = project;
        state.system = system;
        state.volume = Volume::Project;
        state.cur_dir = "/".into();
    });
}

fn c_str<'a>(ptr: *const u8) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr.cast()) }.to_str().ok()
}

fn alloc_bytes(bytes: &[u8]) -> *mut u8 {
    let size = i64::try_from(bytes.len().saturating_add(1)).unwrap_or(i64::MAX);
    let ptr = unsafe { tos_runtime::tos_MAlloc(size, std::ptr::null_mut()) };
    if ptr.is_null() {
        return ptr;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        *ptr.add(bytes.len()) = 0;
    }
    ptr
}

fn alloc_str(text: &str) -> *mut u8 {
    alloc_bytes(text.as_bytes())
}

fn join_virt(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let combined = if rel.starts_with('/') {
        rel.to_string()
    } else if base == "/" {
        format!("/{rel}")
    } else {
        format!("{base}/{rel}")
    };
    for part in combined.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop()?;
            continue;
        }
        if part.contains(':') || part.contains('\0') {
            return None;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        Some("/".into())
    } else {
        Some(format!("/{}", parts.join("/")))
    }
}

fn host_root(state: &FsState, volume: Volume) -> Option<PathBuf> {
    match volume {
        Volume::Project => state
            .project
            .clone()
            .or_else(|| std::env::current_dir().ok()),
        Volume::System => state.system.clone(),
    }
}

fn host_path(state: &FsState, volume: Volume, virt: &str) -> Option<PathBuf> {
    let root = host_root(state, volume)?;
    let mut path = root.clone();
    for part in virt.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains('\0') {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

struct Resolved {
    volume: Volume,
    virt: String,
}

fn parse_name(state: &FsState, name: &str) -> Option<Resolved> {
    let name = name.replace('\\', "/");
    if name.is_empty() {
        return Some(Resolved {
            volume: state.volume,
            virt: state.cur_dir.clone(),
        });
    }
    if let Some(rest) = name.strip_prefix("::/") {
        return Some(Resolved {
            volume: Volume::System,
            virt: join_virt("/", rest)?,
        });
    }
    if let Some(rest) = name.strip_prefix(":/") {
        return Some(Resolved {
            volume: Volume::System,
            virt: join_virt("/", rest)?,
        });
    }
    if name == "~" || name == "~/" {
        return Some(Resolved {
            volume: Volume::Project,
            virt: "/".into(),
        });
    }
    if let Some(rest) = name.strip_prefix("~/") {
        return Some(Resolved {
            volume: Volume::Project,
            virt: join_virt("/", rest)?,
        });
    }
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        let volume = if name.as_bytes()[0] == b':' {
            Volume::System
        } else {
            Volume::Project
        };
        let rest = &name[2..];
        let virt = if rest.is_empty() {
            "/".into()
        } else if rest.starts_with('/') {
            join_virt("/", rest)?
        } else {
            join_virt(&state.cur_dir, rest)?
        };
        return Some(Resolved { volume, virt });
    }
    if name.starts_with('/') {
        return Some(Resolved {
            volume: state.volume,
            virt: join_virt("/", &name)?,
        });
    }
    Some(Resolved {
        volume: state.volume,
        virt: join_virt(&state.cur_dir, &name)?,
    })
}

fn z_candidates(virt: &str) -> Vec<String> {
    let mut names = vec![virt.to_string()];
    if let Some(stripped) = virt.strip_suffix(".Z") {
        names.push(stripped.to_string());
    } else {
        names.push(format!("{virt}.Z"));
    }
    names
}

fn split_mask(mask: &str) -> (String, String) {
    let mask = mask.replace('\\', "/");
    match mask.rsplit_once('/') {
        Some((dir, glob)) if !glob.is_empty() => {
            if dir.is_empty() {
                ("/".into(), glob.into())
            } else {
                (dir.into(), glob.into())
            }
        }
        _ => (String::new(), mask),
    }
}

fn wild_match(text: &str, pattern: &str) -> bool {
    fn rec(text: &[u8], pattern: &[u8]) -> bool {
        if pattern.is_empty() {
            return text.is_empty();
        }
        match pattern[0] {
            b'*' => rec(text, &pattern[1..]) || (!text.is_empty() && rec(&text[1..], pattern)),
            b'?' => !text.is_empty() && rec(&text[1..], &pattern[1..]),
            byte => {
                !text.is_empty()
                    && text[0].eq_ignore_ascii_case(&byte)
                    && rec(&text[1..], &pattern[1..])
            }
        }
    }
    pattern
        .split(';')
        .filter(|part| !part.is_empty())
        .any(|part| rec(text.as_bytes(), part.as_bytes()))
}

fn fill_name(slot: &mut [u8; CDIR_FILENAME_LEN], name: &str) {
    slot.fill(0);
    let bytes = name.as_bytes();
    let len = bytes.len().min(CDIR_FILENAME_LEN - 1);
    slot[..len].copy_from_slice(&bytes[..len]);
}

fn virt_full(volume: Volume, virt: &str) -> String {
    let drive = match volume {
        Volume::Project => "C:",
        Volume::System => ":",
    };
    if virt == "/" {
        format!("{drive}/")
    } else {
        format!("{drive}{virt}")
    }
}

fn fill_entry(entry: &mut CDirEntry, volume: Volume, virt: &str, host: &Path) {
    *entry = unsafe { std::mem::zeroed() };
    let name = host
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    fill_name(&mut entry.name, &name);
    entry.full_name = alloc_str(&virt_full(volume, virt));
    if let Ok(meta) = fs::metadata(host) {
        if meta.is_dir() {
            entry.attr = RS_ATTR_DIR;
        }
        entry.size = i64::try_from(meta.len()).unwrap_or(i64::MAX);
    }
    if name.ends_with(".Z") {
        entry.attr |= RS_ATTR_COMPRESSED;
    }
}

fn locate_existing(
    state: &FsState,
    resolved: &Resolved,
    z_or_not: bool,
) -> Option<(Volume, String, PathBuf)> {
    let names = if z_or_not {
        z_candidates(&resolved.virt)
    } else {
        vec![resolved.virt.clone()]
    };
    for virt in names {
        let host = host_path(state, resolved.volume, &virt)?;
        if host.exists() {
            return Some((resolved.volume, virt, host));
        }
    }
    None
}

fn scan_parents(
    state: &FsState,
    resolved: &Resolved,
    z_or_not: bool,
) -> Option<(Volume, String, PathBuf)> {
    let file = resolved.virt.rsplit('/').next()?.to_string();
    let mut dir = if let Some((parent, _)) = resolved.virt.rsplit_once('/') {
        if parent.is_empty() {
            "/".into()
        } else {
            parent.to_string()
        }
    } else {
        "/".into()
    };
    loop {
        let virt = if dir == "/" {
            format!("/{file}")
        } else {
            format!("{dir}/{file}")
        };
        if let Some(found) = locate_existing(
            state,
            &Resolved {
                volume: resolved.volume,
                virt,
            },
            z_or_not,
        ) {
            return Some(found);
        }
        if dir == "/" {
            return None;
        }
        dir = match dir.rsplit_once('/') {
            Some((parent, _)) if !parent.is_empty() => parent.to_string(),
            _ => "/".into(),
        };
    }
}

fn matches_kind(host: &Path, flags: i64) -> bool {
    let is_dir = host.is_dir();
    if flags & FUF_JUST_DIRS != 0 && !is_dir {
        return false;
    }
    if flags & FUF_JUST_FILES != 0 && is_dir {
        return false;
    }
    true
}

fn collect_matches(
    state: &FsState,
    volume: Volume,
    virt_dir: &str,
    glob: &str,
    flags: i64,
    out: &mut Vec<(*mut CDirEntry, String)>,
) {
    let Some(host_dir) = host_path(state, volume, virt_dir) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&host_dir) else {
        return;
    };
    let mut names = entries
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    names.sort();
    for host in names {
        let file_name = host
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let child_virt = if virt_dir == "/" {
            format!("/{file_name}")
        } else {
            format!("{virt_dir}/{file_name}")
        };
        if glob != "*"
            && !glob.is_empty()
            && !wild_match(&file_name, glob)
            && !wild_match(&child_virt, glob)
        {
            if flags & FUF_RECURSE != 0 && host.is_dir() {
                collect_matches(state, volume, &child_virt, glob, flags, out);
            }
            continue;
        }
        if matches_kind(&host, flags) {
            let raw =
                unsafe { tos_runtime::tos_CAlloc(CDIR_ENTRY_SIZE as i64, std::ptr::null_mut()) }
                    .cast::<CDirEntry>();
            if !raw.is_null() {
                unsafe { fill_entry(&mut *raw, volume, &child_virt, &host) };
                out.push((raw, child_virt.clone()));
            }
        }
        if flags & FUF_RECURSE != 0 && host.is_dir() {
            collect_matches(state, volume, &child_virt, glob, flags, out);
        }
    }
}

fn link_entries(entries: Vec<*mut CDirEntry>) -> *mut CDirEntry {
    let mut head: *mut CDirEntry = std::ptr::null_mut();
    let mut prev: *mut CDirEntry = std::ptr::null_mut();
    for entry in entries {
        if prev.is_null() {
            head = entry;
        } else {
            unsafe { (*prev).next = entry };
        }
        prev = entry;
    }
    head
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_FileRead(
    filename: *const u8,
    size: *mut i64,
    attr: *mut i64,
) -> *mut u8 {
    let Some(name) = c_str(filename) else {
        return std::ptr::null_mut();
    };
    let found = with_state(|state| {
        let resolved = parse_name(state, name)?;
        locate_existing(state, &resolved, true)
    });
    let Some((_volume, virt, host)) = found else {
        return std::ptr::null_mut();
    };
    if host.is_dir() {
        return std::ptr::null_mut();
    }
    let Ok(bytes) = fs::read(&host) else {
        return std::ptr::null_mut();
    };
    if bytes.len() > MAX_FILE {
        return std::ptr::null_mut();
    }
    if !size.is_null() {
        unsafe { *size = bytes.len() as i64 };
    }
    if !attr.is_null() {
        let mut flags = 0_i64;
        if virt.ends_with(".Z") {
            flags |= i64::from(RS_ATTR_COMPRESSED);
        }
        unsafe { *attr = flags };
    }
    alloc_bytes(&bytes)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_FileFind(
    filename: *const u8,
    entry: *mut CDirEntry,
    flags: i64,
) -> i64 {
    let Some(name) = c_str(filename) else {
        return 0;
    };
    let found = with_state(|state| {
        let resolved = parse_name(state, name)?;
        if flags & FUF_SCAN_PARENTS != 0 {
            scan_parents(state, &resolved, true)
        } else {
            locate_existing(state, &resolved, true)
        }
    });
    let Some((volume, virt, host)) = found else {
        if !entry.is_null() {
            unsafe { std::ptr::write_bytes(entry.cast::<u8>(), 0, CDIR_ENTRY_SIZE) };
        }
        return 0;
    };
    if !matches_kind(&host, flags) {
        return 0;
    }
    if !entry.is_null() {
        unsafe { fill_entry(&mut *entry, volume, &virt, &host) };
    }
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_FilesFind(mask: *const u8, flags: i64) -> *mut CDirEntry {
    let Some(mask) = c_str(mask) else {
        return std::ptr::null_mut();
    };
    if flags & FUF_SINGLE != 0 {
        let raw = unsafe { tos_runtime::tos_CAlloc(CDIR_ENTRY_SIZE as i64, std::ptr::null_mut()) }
            .cast::<CDirEntry>();
        if raw.is_null() {
            return std::ptr::null_mut();
        }
        if unsafe { tos_FileFind(mask.as_ptr(), raw, flags & !FUF_SINGLE) } == 0 {
            unsafe { tos_runtime::tos_Free(raw.cast()) };
            return std::ptr::null_mut();
        }
        return raw;
    }
    let (dir_part, glob) = split_mask(mask);
    let glob = if glob.is_empty() { "*".into() } else { glob };
    let (volume, virt_dir) = with_state(|state| {
        let resolved = if dir_part.is_empty() {
            Resolved {
                volume: state.volume,
                virt: state.cur_dir.clone(),
            }
        } else {
            parse_name(state, &dir_part)?
        };
        Some((resolved.volume, resolved.virt))
    })
    .unwrap_or((Volume::Project, "/".into()));
    let mut entries = Vec::new();
    with_state(|state| {
        collect_matches(state, volume, &virt_dir, &glob, flags, &mut entries);
    });
    link_entries(entries.into_iter().map(|(ptr, _)| ptr).collect())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DirEntryDel(entry: *mut CDirEntry) {
    if entry.is_null() {
        return;
    }
    unsafe {
        tos_runtime::tos_Free((*entry).full_name);
        tos_runtime::tos_Free(entry.cast());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DirEntryDel2(entry: *mut CDirEntry) {
    if entry.is_null() {
        return;
    }
    unsafe {
        tos_runtime::tos_Free((*entry).full_name);
        tos_runtime::tos_Free((*entry).user_data as *mut u8);
        tos_runtime::tos_Free(entry.cast());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DirTreeDel(mut entry: *mut CDirEntry) {
    while !entry.is_null() {
        let next = unsafe { (*entry).next };
        let sub = unsafe { (*entry).sub };
        if !sub.is_null() {
            unsafe { tos_DirTreeDel(sub) };
        }
        unsafe { tos_DirEntryDel(entry) };
        entry = next;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DirTreeDel2(mut entry: *mut CDirEntry) {
    while !entry.is_null() {
        let next = unsafe { (*entry).next };
        let sub = unsafe { (*entry).sub };
        if !sub.is_null() {
            unsafe { tos_DirTreeDel2(sub) };
        }
        unsafe { tos_DirEntryDel2(entry) };
        entry = next;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Cd(dirname: *const u8, _make_dirs: i64) -> i64 {
    let name = if dirname.is_null() {
        "~"
    } else {
        c_str(dirname).unwrap_or("")
    };
    if name.is_empty() {
        return 1;
    }
    with_state(|state| {
        let resolved = parse_name(state, name)?;
        let host = host_path(state, resolved.volume, &resolved.virt)?;
        if !host.is_dir() {
            return None;
        }
        state.volume = resolved.volume;
        state.cur_dir = resolved.virt;
        Some(())
    })
    .is_some() as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_IsDir(name: *const u8) -> i64 {
    let Some(name) = c_str(name) else {
        return 0;
    };
    with_state(|state| {
        let resolved = parse_name(state, name)?;
        let host = host_path(state, resolved.volume, &resolved.virt)?;
        Some(host.is_dir())
    })
    .unwrap_or(false) as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DirCur(_task: *mut u8, _mem_task: *mut u8) -> *mut u8 {
    let text = with_state(|state| virt_full(state.volume, &state.cur_dir));
    alloc_str(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sanctum-fs-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(path.join("sub")).unwrap();
        fs::write(path.join("note.TXT"), b"hello file").unwrap();
        fs::write(path.join("sub").join("inner.HC"), b"U0 Inner(){}\n").unwrap();
        path
    }

    #[test]
    fn join_virt_rejects_escaping_the_root() {
        assert_eq!(join_virt("/", "a/b"), Some("/a/b".into()));
        assert_eq!(join_virt("/a/b", ".."), Some("/a".into()));
        assert_eq!(join_virt("/", ".."), None);
        assert_eq!(join_virt("/a", "../.."), None);
    }

    #[test]
    fn wild_match_accepts_templeos_globs() {
        assert!(wild_match("Talons.HC", "*.HC"));
        assert!(wild_match("note.TXT", "note.*"));
        assert!(!wild_match("note.TXT", "*.HC"));
        assert!(wild_match("a.HC", "*.HC;*.HH"));
    }

    #[test]
    fn file_read_and_find_stay_inside_the_project_root() {
        let root = scratch();
        set_roots(Some(root.clone()), None);
        reset();
        unsafe {
            let mut size = 0_i64;
            let bytes = tos_FileRead(b"note.TXT\0".as_ptr(), &mut size, std::ptr::null_mut());
            assert!(!bytes.is_null());
            assert_eq!(size, 10);
            assert_eq!(CStr::from_ptr(bytes.cast()).to_bytes(), b"hello file");
            tos_runtime::tos_Free(bytes);

            assert_eq!(
                tos_FileRead(
                    b"../note.TXT\0".as_ptr(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut()
                ),
                std::ptr::null_mut()
            );
            assert_eq!(tos_Cd(b"sub\0".as_ptr(), 0), 1);
            let inner = tos_FileRead(
                b"inner.HC\0".as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert!(!inner.is_null());
            tos_runtime::tos_Free(inner);
            assert_eq!(tos_Cd(b"..\0".as_ptr(), 0), 1);
            assert_eq!(tos_IsDir(b"sub\0".as_ptr()), 1);

            let mut entry = std::mem::zeroed::<CDirEntry>();
            assert_eq!(tos_FileFind(b"note.TXT\0".as_ptr(), &mut entry, 0), 1);
            assert_eq!(
                CStr::from_ptr(entry.name.as_ptr().cast()).to_string_lossy(),
                "note.TXT"
            );
            tos_runtime::tos_Free(entry.full_name);

            let list = tos_FilesFind(b"*.TXT\0".as_ptr(), FUF_JUST_FILES);
            assert!(!list.is_null());
            assert_eq!(
                CStr::from_ptr((*list).name.as_ptr().cast()).to_string_lossy(),
                "note.TXT"
            );
            tos_DirTreeDel(list);
        }
        let _ = fs::remove_dir_all(root);
        reset();
        set_roots(None, None);
    }
}

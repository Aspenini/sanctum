//! Small, source-compatible subset of the TempleOS registry.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

static PERSISTENT: AtomicBool = AtomicBool::new(false);

thread_local! {
    static MEMORY: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static GLOBALS: RefCell<HashMap<String, BoundGlobal>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Copy)]
struct BoundGlobal {
    address: usize,
    size: usize,
}

/// A finalized compiled global that registry assignment text may update.
pub struct GlobalBinding<'a> {
    pub name: &'a str,
    pub address: *mut u8,
    pub size: usize,
}

pub fn set_persistent(persistent: bool) {
    PERSISTENT.store(persistent, Ordering::Release);
}

pub fn reset() {
    MEMORY.with(|memory| memory.borrow_mut().clear());
    unbind();
}

pub fn unbind() {
    GLOBALS.with(|globals| globals.borrow_mut().clear());
}

pub fn bind(bindings: &[GlobalBinding<'_>]) {
    GLOBALS.with(|globals| {
        let mut globals = globals.borrow_mut();
        globals.clear();
        globals.extend(bindings.iter().map(|binding| {
            (
                binding.name.to_string(),
                BoundGlobal {
                    address: binding.address as usize,
                    size: binding.size,
                },
            )
        }));
    });
}

fn registry_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SANCTUM_DATA_DIR") {
        return Some(PathBuf::from(path).join("registry"));
    }
    if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        return Some(PathBuf::from(path).join("Sanctum").join("registry"));
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .map(|path| path.join("sanctum").join("registry"))
}

fn registry_file(path: &str) -> Option<PathBuf> {
    let root = registry_root()?;
    let normalized = path.replace('\\', "/");
    let relative = PathBuf::from(normalized);
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component.as_os_str().to_string_lossy().contains(':')
        })
    {
        return None;
    }
    let mut file = root.join(relative);
    file.set_extension("HC");
    Some(file)
}

fn read(path: &str) -> Option<String> {
    if PERSISTENT.load(Ordering::Acquire) {
        fs::read_to_string(registry_file(path)?).ok()
    } else {
        MEMORY.with(|memory| memory.borrow().get(path).cloned())
    }
}

fn write_default(path: &str, value: &str) -> io::Result<bool> {
    if PERSISTENT.load(Ordering::Acquire) {
        let file = registry_file(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid registry path"))?;
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        match OpenOptions::new().write(true).create_new(true).open(file) {
            Ok(mut output) => {
                output.write_all(value.as_bytes())?;
                Ok(false)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(true),
            Err(error) => Err(error),
        }
    } else {
        Ok(MEMORY.with(|memory| {
            let mut values = memory.borrow_mut();
            let existed = values.contains_key(path);
            values
                .entry(path.to_string())
                .or_insert_with(|| value.into());
            existed
        }))
    }
}

fn write_value(path: &str, value: &str) -> io::Result<()> {
    if PERSISTENT.load(Ordering::Acquire) {
        let file = registry_file(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid registry path"))?;
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file, value)
    } else {
        MEMORY.with(|memory| {
            memory
                .borrow_mut()
                .insert(path.to_string(), value.to_string());
        });
        Ok(())
    }
}

fn parse_integer(value: &str) -> Option<i64> {
    let value = value.trim();
    match value {
        "TRUE" | "ON" => Some(1),
        "FALSE" | "OFF" | "NULL" => Some(0),
        _ => value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .and_then(|hex| i64::from_str_radix(hex, 16).ok())
            .or_else(|| value.parse().ok()),
    }
}

fn apply_assignment(statement: &str) -> bool {
    let Some((left, right)) = statement.split_once('=') else {
        return false;
    };
    let mut words = left.split_whitespace();
    let first = words.next().unwrap_or_default();
    let (kind, name) = match words.next() {
        Some(name) => (first, name),
        None => ("", first),
    };
    let Some(binding) = GLOBALS.with(|globals| globals.borrow().get(name).copied()) else {
        return false;
    };

    let address = binding.address as *mut u8;
    unsafe {
        match kind {
            "F64" if binding.size >= size_of::<f64>() => {
                let Some(value) = right.trim().parse::<f64>().ok() else {
                    return false;
                };
                address.cast::<f64>().write_unaligned(value);
            }
            "I8" | "U8" | "Bool" if binding.size >= 1 => {
                let Some(value) = parse_integer(right) else {
                    return false;
                };
                address.write(value as u8);
            }
            "I16" | "U16" if binding.size >= 2 => {
                let Some(value) = parse_integer(right) else {
                    return false;
                };
                address.cast::<i16>().write_unaligned(value as i16);
            }
            "I32" | "U32" if binding.size >= 4 => {
                let Some(value) = parse_integer(right) else {
                    return false;
                };
                address.cast::<i32>().write_unaligned(value as i32);
            }
            _ if binding.size >= size_of::<i64>() => {
                let Some(value) = parse_integer(right) else {
                    return false;
                };
                address.cast::<i64>().write_unaligned(value);
            }
            _ => return false,
        }
    }
    true
}

fn apply_source(source: &str) -> i64 {
    source
        .split(';')
        .filter(|statement| apply_assignment(statement.trim()))
        .count() as i64
}

fn format_one_f64(format: &str, value: f64) -> String {
    let Some(percent) = format.find('%') else {
        return format.to_string();
    };
    let bytes = format.as_bytes();
    let mut cursor = percent + 1;
    if bytes.get(cursor) == Some(&b'%') {
        let mut result = format.to_string();
        result.replace_range(percent..=cursor, "%");
        return result;
    }
    while matches!(bytes.get(cursor), Some(b'-' | b'+' | b' ' | b'0' | b'#')) {
        cursor += 1;
    }
    let width_start = cursor;
    while matches!(bytes.get(cursor), Some(b'0'..=b'9')) {
        cursor += 1;
    }
    let width = format[width_start..cursor].parse::<usize>().unwrap_or(0);
    let precision = if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let start = cursor;
        while matches!(bytes.get(cursor), Some(b'0'..=b'9')) {
            cursor += 1;
        }
        format[start..cursor].parse::<usize>().unwrap_or(6)
    } else {
        6
    };
    let Some(specifier @ (b'f' | b'F' | b'g' | b'G' | b'e' | b'E')) = bytes.get(cursor).copied()
    else {
        return format.to_string();
    };
    let mut rendered = match specifier {
        b'e' => format!("{value:.precision$e}"),
        b'E' => format!("{value:.precision$E}"),
        _ => format!("{value:.precision$}"),
    };
    if rendered.len() < width {
        rendered = format!("{}{rendered}", " ".repeat(width - rendered.len()));
    }
    let mut result = format.to_string();
    result.replace_range(percent..=cursor, &rendered);
    result
}

unsafe fn c_string(pointer: *const u8) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(pointer.cast()) }
            .to_string_lossy()
            .into_owned(),
    )
}

#[unsafe(no_mangle)]
/// Add a default registry branch unless it already exists.
///
/// # Safety
///
/// Both pointers must be null or readable NUL-terminated byte strings.
pub unsafe extern "C" fn tos_RegDft(path: *const u8, defaults: *const u8) -> i64 {
    let (Some(path), Some(defaults)) = (unsafe { c_string(path) }, unsafe { c_string(defaults) })
    else {
        return 0;
    };
    write_default(&path, &defaults).map(i64::from).unwrap_or(0)
}

#[unsafe(no_mangle)]
/// Apply simple scalar assignments from a registry branch to bound globals.
///
/// # Safety
///
/// `path` must be null or a readable NUL-terminated byte string.
pub unsafe extern "C" fn tos_RegExe(path: *const u8) -> i64 {
    let Some(path) = (unsafe { c_string(path) }) else {
        return 0;
    };
    read(&path).map_or(0, |source| apply_source(&source))
}

#[unsafe(no_mangle)]
/// Rewrite a registry branch using its single floating-point format argument.
///
/// # Safety
///
/// Both pointers must be null or readable NUL-terminated byte strings.
pub unsafe extern "C" fn tos_RegWrite(path: *const u8, format: *const u8, value: f64) -> i64 {
    let (Some(path), Some(format)) = (unsafe { c_string(path) }, unsafe { c_string(format) })
    else {
        return 0;
    };
    i64::from(write_value(&path, &format_one_f64(&format, value)).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn defaults_load_and_rewrites_a_bound_f64() {
        set_persistent(false);
        reset();
        let mut best_score = 0.0_f64;
        bind(&[GlobalBinding {
            name: "best_score",
            address: (&mut best_score as *mut f64).cast(),
            size: size_of::<f64>(),
        }]);
        let path = CString::new("TempleOS/Talons").unwrap();
        let defaults = CString::new("F64 best_score=9999;\n").unwrap();
        let format = CString::new("F64 best_score=%5.4f;\n").unwrap();

        unsafe {
            assert_eq!(
                tos_RegDft(path.as_ptr().cast(), defaults.as_ptr().cast()),
                0
            );
            assert_eq!(tos_RegExe(path.as_ptr().cast()), 1);
            assert_eq!(best_score, 9999.0);
            assert_eq!(
                tos_RegWrite(path.as_ptr().cast(), format.as_ptr().cast(), 12.3456),
                1
            );
            best_score = 0.0;
            assert_eq!(tos_RegExe(path.as_ptr().cast()), 1);
            assert!((best_score - 12.3456).abs() < 1e-9);
            assert_eq!(
                tos_RegDft(path.as_ptr().cast(), defaults.as_ptr().cast()),
                1
            );
        }
    }

    #[test]
    fn rejects_registry_path_traversal() {
        assert!(registry_file("../outside").is_none());
        assert!(registry_file("TempleOS/Talons").is_some());
    }
}

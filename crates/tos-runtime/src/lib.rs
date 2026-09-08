//! Kernel-ish primitives HolyC programs call (`Print`, heaps, …).

use std::cell::RefCell;
use std::ffi::CStr;
use std::io::{self, Write};
use std::slice;
use std::sync::Mutex;

thread_local! {
    static CAPTURE: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

static BOOT_ONCE: Mutex<bool> = Mutex::new(false);

/// Capture Print output for tests instead of writing stdout.
pub fn capture_begin() {
    CAPTURE.with(|c| *c.borrow_mut() = Some(Vec::new()));
}

pub fn capture_take() -> Option<Vec<u8>> {
    CAPTURE.with(|c| c.borrow_mut().take())
}

pub fn jit_symbols() -> Vec<(&'static str, *const u8)> {
    vec![
        ("tos_Print", tos_Print as *const u8),
        ("tos_PutChars", tos_PutChars as *const u8),
        ("tos_ToI64", tos_ToI64 as *const u8),
        ("tos_ToF64", tos_ToF64 as *const u8),
        ("tos_ToBool", tos_ToBool as *const u8),
        ("tos_MAlloc", tos_MAlloc as *const u8),
        ("tos_CAlloc", tos_CAlloc as *const u8),
        ("tos_Free", tos_Free as *const u8),
        ("tos_StrLen", tos_StrLen as *const u8),
        ("tos_MemCpy", tos_MemCpy as *const u8),
        ("tos_MemSet", tos_MemSet as *const u8),
        ("tos_boot", tos_boot as *const u8),
    ]
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_boot() {
    let mut g = BOOT_ONCE.lock().unwrap();
    if !*g {
        *g = true;
    }
}

fn emit(bytes: &[u8]) {
    CAPTURE.with(|c| {
        if let Some(buf) = c.borrow_mut().as_mut() {
            buf.extend_from_slice(bytes);
            return;
        }
        let mut out = io::stdout();
        let _ = out.write_all(bytes);
        let _ = out.flush();
    });
}

/// `Print(fmt, argc, argv)` — HolyC variadics lowered to argc/argv of I64/F64 slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Print(fmt: *const u8, argc: i64, argv: *const i64) -> i64 {
    if fmt.is_null() {
        return 0;
    }
    let cstr = unsafe { CStr::from_ptr(fmt as *const i8) };
    let fmt = cstr.to_string_lossy();
    let args: &[i64] = if argc > 0 && !argv.is_null() {
        unsafe { slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let s = format_tos(&fmt, args);
    emit(s.as_bytes());
    s.len() as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_PutChars(ch: i64) -> i64 {
    let mut buf = [0u8; 8];
    let mut n = 0;
    let mut v = ch as u64;
    while v != 0 && n < 8 {
        buf[n] = v as u8;
        v >>= 8;
        n += 1;
    }
    emit(&buf[..n]);
    n as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_ToI64(x: f64) -> i64 {
    x as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_ToF64(x: i64) -> f64 {
    x as f64
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_ToBool(x: i64) -> i64 {
    if x != 0 { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_MAlloc(size: i64) -> *mut u8 {
    if size <= 0 {
        return std::ptr::null_mut();
    }
    let layout = std::alloc::Layout::from_size_align(size as usize, 8).unwrap();
    unsafe { std::alloc::alloc(layout) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_CAlloc(size: i64) -> *mut u8 {
    let p = unsafe { tos_MAlloc(size) };
    if !p.is_null() {
        unsafe { std::ptr::write_bytes(p, 0, size as usize) };
    }
    p
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    // Size is not tracked yet; Phase 2 heap will record MSize. Leak is
    // acceptable for hello-world; Free(NULL) is defined.
    let _ = ptr;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_StrLen(s: *const u8) -> i64 {
    if s.is_null() {
        return 0;
    }
    unsafe { CStr::from_ptr(s as *const i8) }.to_bytes().len() as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_MemCpy(dst: *mut u8, src: *const u8, n: i64) -> *mut u8 {
    if n > 0 && !dst.is_null() && !src.is_null() {
        unsafe { std::ptr::copy_nonoverlapping(src, dst, n as usize) };
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_MemSet(dst: *mut u8, val: i64, n: i64) -> *mut u8 {
    if n > 0 && !dst.is_null() {
        unsafe { std::ptr::write_bytes(dst, val as u8, n as usize) };
    }
    dst
}

fn format_tos(fmt: &str, args: &[i64]) -> String {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    let mut ai = 0usize;
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            None => out.push('%'),
            Some('%') => out.push('%'),
            Some('s') => {
                let p = *args.get(ai).unwrap_or(&0);
                ai += 1;
                if p == 0 {
                    out.push_str("NULL");
                } else {
                    let s = unsafe { CStr::from_ptr(p as *const i8) };
                    out.push_str(&s.to_string_lossy());
                }
            }
            Some('c') => {
                let v = *args.get(ai).unwrap_or(&0);
                ai += 1;
                if let Some(c) = char::from_u32((v as u8) as u32) {
                    out.push(c);
                }
            }
            Some('d' | 'i' | 'n' | 'D') => {
                let v = *args.get(ai).unwrap_or(&0);
                ai += 1;
                out.push_str(&format!("{v}"));
            }
            Some('x') => {
                let v = *args.get(ai).unwrap_or(&0);
                ai += 1;
                out.push_str(&format!("{v:x}"));
            }
            Some('X') => {
                let v = *args.get(ai).unwrap_or(&0) as u64;
                ai += 1;
                out.push_str(&format!("{v:X}"));
            }
            Some('p') => {
                let v = *args.get(ai).unwrap_or(&0) as u64;
                ai += 1;
                out.push_str(&format!("{v:#X}"));
            }
            Some('f' | 'e' | 'g') => {
                let bits = *args.get(ai).unwrap_or(&0) as u64;
                ai += 1;
                let v = f64::from_bits(bits);
                out.push_str(&format!("{v}"));
            }
            Some(other) => {
                out.push('%');
                out.push(other);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_plain() {
        capture_begin();
        unsafe {
            let s = b"Hello world\n\0";
            tos_Print(s.as_ptr(), 0, std::ptr::null());
        }
        let out = capture_take().unwrap();
        assert_eq!(out, b"Hello world\n");
    }
}

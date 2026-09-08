//! Kernel-ish primitives HolyC programs call (`Print`, heaps, …).

mod bit;
mod heap;
mod math;
mod que;
mod rand;
mod task;
mod time;

use std::cell::RefCell;
use std::ffi::CStr;
use std::io::{self, Write};
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tos_abi::CQue;

static BACKGROUND_TASKS_ENABLED: AtomicBool = AtomicBool::new(false);
static BACKGROUND_TASKS_CANCELLED: AtomicBool = AtomicBool::new(false);
static BACKGROUND_TASKS_STARTED: AtomicUsize = AtomicUsize::new(0);
static BACKGROUND_TASKS_QUIESCED: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static CAPTURE: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// Capture Print output for tests instead of writing stdout.
pub fn capture_begin() {
    CAPTURE.with(|c| *c.borrow_mut() = Some(Vec::new()));
}

pub fn set_background_tasks_enabled(enabled: bool) {
    BACKGROUND_TASKS_CANCELLED.store(false, Ordering::Release);
    BACKGROUND_TASKS_STARTED.store(0, Ordering::Release);
    BACKGROUND_TASKS_QUIESCED.store(0, Ordering::Release);
    BACKGROUND_TASKS_ENABLED.store(enabled, Ordering::Release);
}

pub fn background_checkpoint() {
    if task::is_background() && BACKGROUND_TASKS_CANCELLED.load(Ordering::Acquire) {
        BACKGROUND_TASKS_QUIESCED.fetch_add(1, Ordering::AcqRel);
        loop {
            std::thread::park();
        }
    }
}

pub fn cancel_background_tasks() {
    BACKGROUND_TASKS_CANCELLED.store(true, Ordering::Release);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while BACKGROUND_TASKS_QUIESCED.load(Ordering::Acquire)
        < BACKGROUND_TASKS_STARTED.load(Ordering::Acquire)
        && std::time::Instant::now() < deadline
    {
        std::thread::yield_now();
    }
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
        ("tos_MSize", tos_MSize as *const u8),
        ("tos_QueInit", tos_QueInit as *const u8),
        ("tos_QueIns", tos_QueIns as *const u8),
        ("tos_QueRem", tos_QueRem as *const u8),
        ("tos_QueDel", tos_QueDel as *const u8),
        ("tos_Bt", tos_Bt as *const u8),
        ("tos_Bts", tos_Bts as *const u8),
        ("tos_Btr", tos_Btr as *const u8),
        ("tos_LBts", tos_LBts as *const u8),
        ("tos_LBtr", tos_LBtr as *const u8),
        ("tos_tS", tos_tS as *const u8),
        ("tos_Jiffies", tos_Jiffies as *const u8),
        ("tos_Blink", tos_Blink as *const u8),
        ("tos_Rand", tos_Rand as *const u8),
        ("tos_RandU16", tos_RandU16 as *const u8),
        ("tos_RandU32", tos_RandU32 as *const u8),
        ("tos_RandI16", tos_RandI16 as *const u8),
        ("tos_RandI64", tos_RandI64 as *const u8),
        ("tos_Abs", tos_Abs as *const u8),
        ("tos_SqrI64", tos_SqrI64 as *const u8),
        ("tos_ClampI64", tos_ClampI64 as *const u8),
        ("tos_Wrap", tos_Wrap as *const u8),
        ("tos_Sleep", tos_Sleep as *const u8),
        ("tos_Yield", tos_Yield as *const u8),
        ("tos_Fs", tos_Fs as *const u8),
        ("tos_Gs", tos_Gs as *const u8),
        ("tos_mp_cnt", tos_mp_cnt as *const u8),
        ("tos_Spawn", tos_Spawn as *const u8),
        ("tos_SndTaskEndCB", tos_SndTaskEndCB as *const u8),
        ("tos_Beep", tos_Beep as *const u8),
        ("tos_Snd", tos_Snd as *const u8),
        ("tos_Play", tos_Play as *const u8),
        ("tos_MusicSettingsRst", tos_MusicSettingsRst as *const u8),
        ("tos_RegDft", tos_RegDft as *const u8),
        ("tos_RegExe", tos_RegExe as *const u8),
        ("tos_RegWrite", tos_RegWrite as *const u8),
        ("tos_SettingsPush", tos_SettingsPush as *const u8),
        ("tos_SettingsPop", tos_SettingsPop as *const u8),
        ("tos_MenuPush", tos_MenuPush as *const u8),
        ("tos_MenuPop", tos_MenuPop as *const u8),
        ("tos_AutoComplete", tos_AutoComplete as *const u8),
        ("tos_WinBorder", tos_WinBorder as *const u8),
        ("tos_WinMax", tos_WinMax as *const u8),
        ("tos_DocCursor", tos_DocCursor as *const u8),
        ("tos_DocClear", tos_DocClear as *const u8),
        ("tos_PutExcept", tos_PutExcept as *const u8),
        ("tos_Exit", tos_Exit as *const u8),
        ("tos_Mat4x4IdentEqu", tos_Mat4x4IdentEqu as *const u8),
        ("tos_Mat4x4IdentNew", tos_Mat4x4IdentNew as *const u8),
        ("tos_Mat4x4RotX", tos_Mat4x4RotX as *const u8),
        ("tos_Mat4x4RotZ", tos_Mat4x4RotZ as *const u8),
        ("tos_Mat4x4MulXYZ", tos_Mat4x4MulXYZ as *const u8),
        (
            "tos_Mat4x4TranslationEqu",
            tos_Mat4x4TranslationEqu as *const u8,
        ),
        ("tos_Mat4x4Scale", tos_Mat4x4Scale as *const u8),
        ("tos_D3Sub", tos_D3Sub as *const u8),
        ("tos_D3NormSqr", tos_D3NormSqr as *const u8),
        ("tos_D3Unit", tos_D3Unit as *const u8),
        ("tos_SwapI64", tos_SwapI64 as *const u8),
        ("tos_Sin", tos_Sin as *const u8),
        ("tos_Cos", tos_Cos as *const u8),
        ("tos_Sqrt", tos_Sqrt as *const u8),
        ("tos_ACos", tos_ACos as *const u8),
        ("tos_Min", tos_Min as *const u8),
        ("tos_Max", tos_Max as *const u8),
        ("tos_Clamp", tos_Clamp as *const u8),
        ("tos_Sign", tos_Sign as *const u8),
        ("tos_Tri", tos_Tri as *const u8),
        ("tos_Saw", tos_Saw as *const u8),
        ("tos_boot", tos_boot as *const u8),
    ]
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_boot() {
    time::boot();
    task::boot_task();
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Jiffies() -> i64 {
    time::jiffies()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Blink() -> i64 {
    if ((time::ts() * 5.0) as i64 & 1) == 0 {
        1
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Spawn(
    fp_start_addr: *const u8,
    data: *mut u8,
    _task_name: *const u8,
    target_cpu: i64,
    parent: *mut tos_abi::CTask,
    _stk_size: i64,
    _flags: i64,
) -> *mut tos_abi::CTask {
    let spawned = task::spawn(parent);
    // CPU-addressed jobs are TempleOS data-parallel work units. Running them
    // inline gives the single-core host correct completion semantics.
    if target_cpu >= 0 && !fp_start_addr.is_null() {
        let entry: extern "C" fn(*mut u8) = unsafe { std::mem::transmute(fp_start_addr) };
        entry(data);
    } else if BACKGROUND_TASKS_ENABLED.load(Ordering::Acquire) && !fp_start_addr.is_null() {
        let entry = fp_start_addr as usize;
        let data = data as usize;
        let spawned_addr = spawned as usize;
        BACKGROUND_TASKS_STARTED.fetch_add(1, Ordering::AcqRel);
        std::thread::spawn(move || {
            task::enter_background(spawned_addr as *mut tos_abi::CTask);
            let entry: extern "C" fn(*mut u8) = unsafe { std::mem::transmute(entry) };
            entry(data as *mut u8);
        });
    }
    spawned
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_SndTaskEndCB() {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Beep(_ona: i64, _busy: i64) {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Snd(_ona: i64) {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Play(_song: *const u8, _words: *const u8) {
    // Keep a silent music task paced until audio synthesis is implemented.
    background_checkpoint();
    std::thread::sleep(std::time::Duration::from_millis(100));
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_MusicSettingsRst() {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_RegDft(_path: *const u8, _defaults: *const u8) {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_RegExe(_path: *const u8) {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_RegWrite(_path: *const u8, _fmt: *const u8, _value: f64) {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_SettingsPush() -> *mut u8 {
    std::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_SettingsPop() {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_MenuPush(_menu: *const u8) -> *mut u8 {
    std::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_MenuPop() {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_AutoComplete() -> i64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_WinBorder() -> i64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_WinMax() {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_DocCursor() -> i64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_DocClear() {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_PutExcept() {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Exit() {}

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
pub extern "C" fn tos_Tri(t: f64, period: f64) -> f64 {
    if period == 0.0 {
        return 0.0;
    }
    let phase = 2.0 * (t.abs() % period) / period;
    if phase <= 1.0 { phase } else { 2.0 - phase }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Saw(t: f64, period: f64) -> f64 {
    if period == 0.0 {
        0.0
    } else if t >= 0.0 {
        (t % period) / period
    } else {
        1.0 + (t % period) / period
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Min(a: f64, b: f64) -> f64 {
    a.min(b)
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Max(a: f64, b: f64) -> f64 {
    a.max(b)
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Clamp(value: f64, lo: f64, hi: f64) -> f64 {
    value.clamp(lo, hi)
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Sign(value: f64) -> f64 {
    value.signum()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Mat4x4IdentNew() -> *mut i64 {
    // TempleOS callers own this result and release it with `Free`, so it must
    // come from the same header-bearing heap as `MAlloc`/`CAlloc`.
    let matrix = unsafe { heap::calloc((16 * size_of::<i64>()) as i64) }.cast::<i64>();
    unsafe { math::mat_identity(matrix) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_MAlloc(size: i64, _task: *mut tos_abi::CTask) -> *mut u8 {
    unsafe { heap::malloc(size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_CAlloc(size: i64, _task: *mut tos_abi::CTask) -> *mut u8 {
    unsafe { heap::calloc(size) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Free(ptr: *mut u8) {
    unsafe { heap::free(ptr) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_MSize(ptr: *mut u8) -> i64 {
    unsafe { heap::msize(ptr) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_QueInit(head: *mut CQue) {
    unsafe { que::init(head) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_QueIns(entry: *mut CQue, pred: *mut CQue) {
    unsafe { que::ins(entry, pred) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_QueRem(entry: *mut CQue) {
    unsafe { que::rem(entry) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_QueDel(head: *mut CQue, remove_first: i64) {
    unsafe { que::del(head, remove_first != 0) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Bt(field: *const u8, bit: i64) -> i64 {
    unsafe { bit::bt(field, bit) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Bts(field: *mut u8, bit: i64) -> i64 {
    unsafe { bit::bts(field, bit) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Btr(field: *mut u8, bit: i64) -> i64 {
    unsafe { bit::btr(field, bit) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_LBts(field: *mut u8, bit: i64) -> i64 {
    unsafe { bit::lbts(field, bit) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_LBtr(field: *mut u8, bit: i64) -> i64 {
    unsafe { bit::lbtr(field, bit) }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_tS() -> f64 {
    time::ts()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Rand() -> f64 {
    rand::rand_f64()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_RandU16() -> i64 {
    rand::rand_u16()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_RandU32() -> i64 {
    rand::rand_u32()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_RandI16() -> i64 {
    rand::rand_i16()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_RandI64() -> i64 {
    rand::rand_i64()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Abs(x: i64) -> i64 {
    x.abs()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_SqrI64(x: i64) -> i64 {
    x.wrapping_mul(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_ClampI64(x: i64, lo: i64, hi: i64) -> i64 {
    x.clamp(lo, hi)
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Wrap(a: f64, base: f64) -> f64 {
    // Wrap to [base, base + 2π), matching TempleOS AMath.HC.
    let tau = std::f64::consts::TAU;
    let mut x = a % tau;
    if x >= base + tau {
        x -= tau;
    }
    if x < base {
        x += tau;
    }
    x
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Sleep(ms: i64) {
    background_checkpoint();
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Yield() {
    std::thread::yield_now();
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Fs() -> *mut tos_abi::CTask {
    task::fs()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Gs() -> *mut tos_abi::CCPU {
    task::gs()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_mp_cnt() -> i64 {
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Mat4x4IdentEqu(r: *mut i64) -> *mut i64 {
    unsafe { math::mat_identity(r) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Mat4x4RotX(r: *mut i64, angle: f64) -> *mut i64 {
    unsafe { math::mat_rotate_x(r, angle) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Mat4x4RotZ(r: *mut i64, angle: f64) -> *mut i64 {
    unsafe { math::mat_rotate_z(r, angle) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Mat4x4MulXYZ(r: *const i64, x: *mut i64, y: *mut i64, z: *mut i64) {
    unsafe { math::mat_mul_xyz(r, x, y, z) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Mat4x4TranslationEqu(r: *mut i64, x: i64, y: i64, z: i64) -> *mut i64 {
    unsafe { math::mat_translate(r, x, y, z) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_Mat4x4Scale(r: *mut i64, scale: f64) -> *mut i64 {
    unsafe { math::mat_scale(r, scale) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_D3Sub(
    dst: *mut tos_abi::CD3,
    lhs: *const tos_abi::CD3,
    rhs: *const tos_abi::CD3,
) -> *mut tos_abi::CD3 {
    unsafe { math::d3_sub(dst, lhs, rhs) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_D3NormSqr(value: *const tos_abi::CD3) -> f64 {
    unsafe { math::d3_norm_sqr(value) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_D3Unit(value: *mut tos_abi::CD3) -> *mut tos_abi::CD3 {
    unsafe { math::d3_unit(value) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_SwapI64(lhs: *mut i64, rhs: *mut i64) {
    unsafe { math::swap_i64(lhs, rhs) }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Sin(value: f64) -> f64 {
    value.sin()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Cos(value: f64) -> f64 {
    value.cos()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Sqrt(value: f64) -> f64 {
    value.sqrt()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_ACos(value: f64) -> f64 {
    value.acos()
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

    #[test]
    fn new_matrix_uses_the_templeos_heap() {
        let matrix = tos_Mat4x4IdentNew();
        assert!(!matrix.is_null());
        assert!(unsafe { tos_MSize(matrix.cast()) } >= (16 * size_of::<i64>()) as i64);
        unsafe { tos_Free(matrix.cast()) };
    }
}

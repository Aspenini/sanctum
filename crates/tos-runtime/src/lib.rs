//! Kernel-ish primitives HolyC programs call (`Print`, heaps, …).

mod bit;
mod env;
mod except;
mod heap;
mod math;
mod que;
mod rand;
mod shell;
mod task;
mod time;

pub use except::{tos_ClearExcept, tos_ExceptCh, tos_HasExcept, tos_Throw};
pub use shell::current_menu_source;
use shell::{
    tos_AutoComplete, tos_DocClear, tos_DocCursor, tos_MenuPop, tos_MenuPush, tos_SettingsPop,
    tos_SettingsPush, tos_WinBorder, tos_WinMax,
};

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
    shell::reset();
    except::reset();
    env::reset();
    BACKGROUND_TASKS_CANCELLED.store(false, Ordering::Release);
    BACKGROUND_TASKS_STARTED.store(0, Ordering::Release);
    BACKGROUND_TASKS_QUIESCED.store(0, Ordering::Release);
    BACKGROUND_TASKS_ENABLED.store(enabled, Ordering::Release);
}

pub fn background_checkpoint() {
    if task::is_background() && BACKGROUND_TASKS_CANCELLED.load(Ordering::Acquire) {
        if let Some(callback) = task::finish_background() {
            callback();
        }
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
        ("tos_Throw", except::tos_Throw as *const u8),
        ("tos_HasExcept", except::tos_HasExcept as *const u8),
        ("tos_ClearExcept", except::tos_ClearExcept as *const u8),
        ("tos_ExceptCh", except::tos_ExceptCh as *const u8),
        ("tos_PowI64", tos_PowI64 as *const u8),
        ("tos_PowF64", tos_PowF64 as *const u8),
        ("tos_EnvPush", env::tos_EnvPush as *const u8),
        ("tos_EnvPop", env::tos_EnvPop as *const u8),
        ("tos_EnvPeek", env::tos_EnvPeek as *const u8),
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
            if let Some(callback) = task::finish_background() {
                callback();
            }
            BACKGROUND_TASKS_QUIESCED.fetch_add(1, Ordering::AcqRel);
        });
    }
    spawned
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_PutExcept() {
    let ch = except::tos_ExceptCh();
    let text = format!("Exception {ch}\n");
    emit(text.as_bytes());
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Exit() {
    except::tos_Throw(0);
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_PowI64(base: i64, exp: i64) -> i64 {
    math::pow_i64(base, exp)
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_PowF64(base: f64, exp: f64) -> f64 {
    math::pow_f64(base, exp)
}

pub fn request_throw(ch: i64) {
    except::request_throw(ch);
}

pub fn reset_exceptions() {
    except::reset();
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
    let _ = except::tos_HasExcept();
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Yield() {
    let _ = except::tos_HasExcept();
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
    let bytes = fmt.as_bytes();
    let mut result = String::new();
    let mut index = 0usize;
    let mut arg_index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            result.push(char::from(bytes[index]));
            index += 1;
            continue;
        }
        index += 1;
        if bytes.get(index) == Some(&b'%') {
            result.push('%');
            index += 1;
            continue;
        }

        let mut left_aligned = false;
        let mut zero_padded = false;
        let mut force_sign = false;
        while let Some(flag) = bytes.get(index).copied() {
            match flag {
                b'-' => left_aligned = true,
                b'0' => zero_padded = true,
                b'+' => force_sign = true,
                b' ' | b'#' => {}
                _ => break,
            }
            index += 1;
        }
        let mut width = 0usize;
        while let Some(digit) = bytes.get(index).filter(|byte| byte.is_ascii_digit()) {
            width = width.saturating_mul(10) + usize::from(*digit - b'0');
            index += 1;
        }
        let precision = if bytes.get(index) == Some(&b'.') {
            index += 1;
            let mut places = 0usize;
            while let Some(digit) = bytes.get(index).filter(|byte| byte.is_ascii_digit()) {
                places = places.saturating_mul(10) + usize::from(*digit - b'0');
                index += 1;
            }
            Some(places.min(32))
        } else {
            None
        };
        let Some(specifier) = bytes.get(index).copied() else {
            result.push('%');
            break;
        };
        index += 1;
        let argument = *args.get(arg_index).unwrap_or(&0);
        arg_index += 1;

        let mut formatted = match specifier {
            b's' if argument == 0 => "NULL".to_string(),
            b's' => unsafe {
                CStr::from_ptr(argument as *const i8)
                    .to_string_lossy()
                    .into_owned()
            },
            b'c' => char::from(argument as u8).to_string(),
            b'd' | b'i' | b'n' | b'D' => argument.to_string(),
            b'u' => (argument as u64).to_string(),
            b'x' => format!("{:x}", argument as u64),
            b'X' => format!("{:X}", argument as u64),
            b'p' => format!("{:#X}", argument as u64),
            b'f' | b'g' | b'e' | b'E' => {
                let value = f64::from_bits(argument as u64);
                match (specifier, precision) {
                    (b'e', Some(places)) => format!("{value:.places$e}"),
                    (b'E', Some(places)) => format!("{value:.places$E}"),
                    (b'e', None) => format!("{value:e}"),
                    (b'E', None) => format!("{value:E}"),
                    (_, Some(places)) => format!("{value:.places$}"),
                    _ => value.to_string(),
                }
            }
            other => {
                result.push('%');
                char::from(other).to_string()
            }
        };
        if force_sign
            && !formatted.starts_with('-')
            && matches!(specifier, b'd' | b'i' | b'f' | b'g' | b'e' | b'E')
        {
            formatted.insert(0, '+');
        }
        if formatted.len() < width {
            let padding = if zero_padded && !left_aligned {
                '0'
            } else {
                ' '
            };
            let padding = padding.to_string().repeat(width - formatted.len());
            if left_aligned {
                formatted.push_str(&padding);
            } else if zero_padded && matches!(formatted.as_bytes().first(), Some(b'+' | b'-')) {
                formatted.insert_str(1, &padding);
            } else {
                formatted.insert_str(0, &padding);
            }
        }
        result.push_str(&formatted);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    static BACKGROUND_READY: AtomicBool = AtomicBool::new(false);
    static END_CALLBACKS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn background_end_callback() {
        END_CALLBACKS.fetch_add(1, Ordering::Relaxed);
        // A task callback may use ordinary runtime services. This must not
        // recursively enter the cancellation checkpoint.
        tos_Sleep(0);
    }

    extern "C" fn cancellable_background_task(_data: *mut u8) {
        unsafe { (*tos_Fs()).task_end_cb = Some(background_end_callback) };
        BACKGROUND_READY.store(true, Ordering::Release);
        loop {
            tos_Sleep(0);
        }
    }

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
    fn print_formats_width_precision_and_padding() {
        let args = [1.5_f64.to_bits() as i64, 7, 0xab];
        assert_eq!(
            format_tos("v=%5.2f n=%04d x=%X", &args),
            "v= 1.50 n=0007 x=AB"
        );
    }

    #[test]
    fn new_matrix_uses_the_templeos_heap() {
        let matrix = tos_Mat4x4IdentNew();
        assert!(!matrix.is_null());
        assert!(unsafe { tos_MSize(matrix.cast()) } >= (16 * size_of::<i64>()) as i64);
        unsafe { tos_Free(matrix.cast()) };
    }

    #[test]
    fn cancellation_runs_background_task_end_callback_once() {
        BACKGROUND_READY.store(false, Ordering::Relaxed);
        END_CALLBACKS.store(0, Ordering::Relaxed);
        set_background_tasks_enabled(true);
        unsafe {
            tos_Spawn(
                cancellable_background_task as *const u8,
                std::ptr::null_mut(),
                std::ptr::null(),
                -1,
                std::ptr::null_mut(),
                0,
                0,
            );
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while !BACKGROUND_READY.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(BACKGROUND_READY.load(Ordering::Acquire));

        cancel_background_tasks();
        assert_eq!(END_CALLBACKS.load(Ordering::Relaxed), 1);
        set_background_tasks_enabled(false);
    }
}

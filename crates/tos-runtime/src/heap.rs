//! Per-allocation header so `MSize`/`Free` work like TempleOS.

use std::alloc::{alloc, dealloc, Layout};

const HDR: usize = 16;
const MAGIC: u64 = 0x544F53414C4C4F43; // "TOSALLOC"

#[repr(C)]
struct Header {
    size: u64,
    magic: u64,
}

fn round_up(n: usize) -> usize {
    if n == 0 {
        return 8;
    }
    if n > 64 {
        n.next_power_of_two()
    } else {
        n.div_ceil(8) * 8
    }
}

pub unsafe fn malloc(size: i64) -> *mut u8 {
    if size < 0 {
        return std::ptr::null_mut();
    }
    let usable = round_up(size as usize);
    let layout = Layout::from_size_align(usable + HDR, 8).expect("layout");
    let raw = unsafe { alloc(layout) };
    if raw.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        raw.cast::<Header>().write(Header {
            size: usable as u64,
            magic: MAGIC,
        });
        raw.add(HDR)
    }
}

pub unsafe fn calloc(size: i64) -> *mut u8 {
    let p = unsafe { malloc(size) };
    if !p.is_null() {
        let n = unsafe { msize(p) } as usize;
        unsafe { std::ptr::write_bytes(p, 0, n) };
    }
    p
}

unsafe fn header(ptr: *mut u8) -> Option<*mut Header> {
    if ptr.is_null() {
        return None;
    }
    let h = unsafe { ptr.sub(HDR).cast::<Header>() };
    if unsafe { (*h).magic } != MAGIC {
        return None;
    }
    Some(h)
}

pub unsafe fn msize(ptr: *mut u8) -> i64 {
    match unsafe { header(ptr) } {
        Some(h) => unsafe { (*h).size as i64 },
        None => 0,
    }
}

pub unsafe fn free(ptr: *mut u8) {
    let Some(h) = (unsafe { header(ptr) }) else {
        return;
    };
    let usable = unsafe { (*h).size } as usize;
    let layout = Layout::from_size_align(usable + HDR, 8).expect("layout");
    unsafe {
        (*h).magic = 0;
        dealloc(h.cast(), layout);
    }
}

use std::sync::atomic::{AtomicU8, Ordering};

unsafe fn byte_and_mask(field: *mut u8, bit: i64) -> (*mut u8, u8) {
    let idx = (bit >> 3) as isize;
    let mask = 1u8 << (bit & 7);
    (unsafe { field.offset(idx) }, mask)
}

pub unsafe fn bt(field: *const u8, bit: i64) -> i64 {
    if field.is_null() {
        return 0;
    }
    let (p, mask) = unsafe { byte_and_mask(field as *mut u8, bit) };
    i64::from(unsafe { *p } & mask != 0)
}

pub unsafe fn bts(field: *mut u8, bit: i64) -> i64 {
    if field.is_null() {
        return 0;
    }
    let (p, mask) = unsafe { byte_and_mask(field, bit) };
    let old = unsafe { *p };
    unsafe { *p = old | mask };
    i64::from(old & mask != 0)
}

pub unsafe fn btr(field: *mut u8, bit: i64) -> i64 {
    if field.is_null() {
        return 0;
    }
    let (p, mask) = unsafe { byte_and_mask(field, bit) };
    let old = unsafe { *p };
    unsafe { *p = old & !mask };
    i64::from(old & mask != 0)
}

pub unsafe fn lbts(field: *mut u8, bit: i64) -> i64 {
    if field.is_null() {
        return 0;
    }
    let (p, mask) = unsafe { byte_and_mask(field, bit) };
    let atom = unsafe { AtomicU8::from_ptr(p) };
    let old = atom.fetch_or(mask, Ordering::SeqCst);
    i64::from(old & mask != 0)
}

pub unsafe fn lbtr(field: *mut u8, bit: i64) -> i64 {
    if field.is_null() {
        return 0;
    }
    let (p, mask) = unsafe { byte_and_mask(field, bit) };
    let atom = unsafe { AtomicU8::from_ptr(p) };
    let old = atom.fetch_and(!mask, Ordering::SeqCst);
    i64::from(old & mask != 0)
}

//! Stack of HolyC nested-function environments.
//!
//! Each frame is `[parent: *mut u8][slots…]`. Nested functions read outer
//! locals through `tos_EnvPeek` so their TempleOS callback ABI stays unchanged.

use std::cell::Cell;
use std::ptr;

thread_local! {
    static CURRENT: Cell<*mut u8> = const { Cell::new(ptr::null_mut()) };
}

pub fn reset() {
    CURRENT.set(ptr::null_mut());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_EnvPush(frame: *mut u8) {
    if frame.is_null() {
        return;
    }
    unsafe { frame.cast::<*mut u8>().write(CURRENT.get()) };
    CURRENT.set(frame);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_EnvPop() {
    let current = CURRENT.get();
    if current.is_null() {
        return;
    }
    CURRENT.set(unsafe { current.cast::<*mut u8>().read() });
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_EnvPeek() -> *mut u8 {
    CURRENT.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_peek_and_pop_restore_the_parent_frame() {
        reset();
        let mut outer = [0_u8; 16];
        let mut inner = [0_u8; 16];
        unsafe {
            tos_EnvPush(outer.as_mut_ptr());
            assert_eq!(tos_EnvPeek(), outer.as_mut_ptr());
            tos_EnvPush(inner.as_mut_ptr());
            assert_eq!(tos_EnvPeek(), inner.as_mut_ptr());
            assert_eq!(inner.as_ptr().cast::<*mut u8>().read(), outer.as_mut_ptr());
            tos_EnvPop();
            assert_eq!(tos_EnvPeek(), outer.as_mut_ptr());
            tos_EnvPop();
        }
        assert!(tos_EnvPeek().is_null());
        reset();
    }
}

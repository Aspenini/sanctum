//! HolyC `try`/`throw`/`catch` as a host-side exception flag.
//!
//! Generated code checks `tos_HasExcept` after calls. A pending throw from
//! another thread (Stop, tests) is taken on the HolyC thread at that check.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static CODE: Cell<i64> = const { Cell::new(0) };
}

static PENDING: AtomicBool = AtomicBool::new(false);
static PENDING_CODE: AtomicI64 = AtomicI64::new(0);

pub fn reset() {
    ACTIVE.set(false);
    CODE.set(0);
    PENDING.store(false, Ordering::Release);
    PENDING_CODE.store(0, Ordering::Release);
}

pub fn request_throw(ch: i64) {
    PENDING_CODE.store(ch, Ordering::Release);
    PENDING.store(true, Ordering::Release);
}

fn take_pending() {
    if PENDING.swap(false, Ordering::AcqRel) && !ACTIVE.get() {
        ACTIVE.set(true);
        CODE.set(PENDING_CODE.load(Ordering::Acquire));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Throw(ch: i64) {
    ACTIVE.set(true);
    CODE.set(ch);
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_HasExcept() -> i64 {
    take_pending();
    i64::from(ACTIVE.get())
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_ClearExcept() {
    ACTIVE.set(false);
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_ExceptCh() -> i64 {
    take_pending();
    CODE.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throw_is_visible_until_cleared() {
        reset();
        tos_Throw(7);
        assert_eq!(tos_HasExcept(), 1);
        assert_eq!(tos_ExceptCh(), 7);
        tos_ClearExcept();
        assert_eq!(tos_HasExcept(), 0);
        reset();
    }

    #[test]
    fn pending_throw_is_taken_on_the_checking_thread() {
        reset();
        request_throw(11);
        assert_eq!(tos_HasExcept(), 1);
        assert_eq!(tos_ExceptCh(), 11);
        reset();
    }
}

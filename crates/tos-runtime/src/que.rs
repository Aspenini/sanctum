use tos_abi::CQue;

pub unsafe fn init(head: *mut CQue) {
    if head.is_null() {
        return;
    }
    unsafe {
        (*head).next = head;
        (*head).last = head;
    }
}

pub unsafe fn ins(entry: *mut CQue, pred: *mut CQue) {
    if entry.is_null() || pred.is_null() {
        return;
    }
    unsafe {
        let succ = (*pred).next;
        (*entry).next = succ;
        (*entry).last = pred;
        (*pred).next = entry;
        if !succ.is_null() {
            (*succ).last = entry;
        }
    }
}

pub unsafe fn rem(entry: *mut CQue) {
    if entry.is_null() {
        return;
    }
    unsafe {
        let next = (*entry).next;
        let last = (*entry).last;
        if !last.is_null() {
            (*last).next = next;
        }
        if !next.is_null() {
            (*next).last = last;
        }
        (*entry).next = entry;
        (*entry).last = entry;
    }
}

pub unsafe fn del(head: *mut CQue, remove_first: bool) {
    if head.is_null() {
        return;
    }
    let mut entry = unsafe { (*head).next };
    while !entry.is_null() && entry != head {
        let next = unsafe { (*entry).next };
        if remove_first {
            unsafe { rem(entry) };
        }
        unsafe { crate::heap::free(entry.cast()) };
        entry = next;
    }
    unsafe { init(head) };
}

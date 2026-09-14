use std::cell::{Cell, UnsafeCell};
use std::ptr;
use std::sync::OnceLock;
use tos_abi::{CCPU, CTask};

thread_local! {
    static FS: Cell<*mut CTask> = const { Cell::new(ptr::null_mut()) };
    static GS: Cell<*mut CCPU> = const { Cell::new(ptr::null_mut()) };
    static BACKGROUND: Cell<bool> = const { Cell::new(false) };
}

struct HostTask(UnsafeCell<CTask>);
unsafe impl Sync for HostTask {}
unsafe impl Send for HostTask {}

struct HostCpu(UnsafeCell<CCPU>);
unsafe impl Sync for HostCpu {}
unsafe impl Send for HostCpu {}

static TASK0: OnceLock<HostTask> = OnceLock::new();
static CPU0: OnceLock<HostCpu> = OnceLock::new();

pub fn boot_task() {
    let task = TASK0.get_or_init(|| HostTask(UnsafeCell::new(CTask::host_default())));
    let cpu = CPU0.get_or_init(|| {
        HostCpu(UnsafeCell::new(CCPU {
            num: 0,
            idle_factor: 0.01,
        }))
    });
    let tp = task.0.get();
    unsafe { (*tp).addr = tp };
    FS.set(tp);
    GS.set(cpu.0.get());
}

/// Create a host-side task record. Cooperative scheduling is introduced later;
/// keeping spawned game loops dormant makes initialization deterministic today.
pub fn spawn(parent: *mut CTask) -> *mut CTask {
    let parent = if parent.is_null() { fs() } else { parent };
    let mut task = CTask::host_default();
    task.pix_width = unsafe { (*parent).pix_width };
    task.pix_height = unsafe { (*parent).pix_height };
    task.pix_left = unsafe { (*parent).pix_left };
    task.pix_top = unsafe { (*parent).pix_top };
    let task = Box::new(task);
    let task = Box::into_raw(task);
    unsafe { (*task).addr = task };
    task
}

pub fn fs() -> *mut CTask {
    let p = FS.get();
    if p.is_null() {
        boot_task();
        FS.get()
    } else {
        p
    }
}

pub fn gs() -> *mut CCPU {
    let p = GS.get();
    if p.is_null() {
        boot_task();
        GS.get()
    } else {
        p
    }
}

pub fn enter_background(task: *mut CTask) {
    boot_task();
    FS.set(task);
    BACKGROUND.set(true);
}

pub fn is_background() -> bool {
    BACKGROUND.get()
}

/// Leave background-task mode and take the task's termination callback.
///
/// Clearing the callback before invoking it gives TempleOS task callbacks
/// one-shot semantics. Clearing `BACKGROUND` also lets a callback call
/// `Sleep` without recursively entering the cancellation checkpoint.
pub fn finish_background() -> Option<extern "C" fn()> {
    if !BACKGROUND.replace(false) {
        return None;
    }
    let task = fs();
    unsafe { (*task).task_end_cb.take() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CALLBACKS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn count_callback() {
        CALLBACKS.fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn background_end_callback_is_taken_once() {
        CALLBACKS.store(0, Ordering::Relaxed);
        let task = spawn(ptr::null_mut());
        unsafe { (*task).task_end_cb = Some(count_callback) };
        enter_background(task);

        finish_background().unwrap()();
        assert!(finish_background().is_none());
        assert_eq!(CALLBACKS.load(Ordering::Relaxed), 1);

        unsafe { drop(Box::from_raw(task)) };
        FS.set(ptr::null_mut());
        boot_task();
    }
}

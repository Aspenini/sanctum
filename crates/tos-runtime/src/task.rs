use std::cell::{Cell, UnsafeCell};
use std::ptr;
use std::sync::OnceLock;
use tos_abi::{CCPU, CTask, GR_HEIGHT, GR_WIDTH};

thread_local! {
    static FS: Cell<*mut CTask> = const { Cell::new(ptr::null_mut()) };
    static GS: Cell<*mut CCPU> = const { Cell::new(ptr::null_mut()) };
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
    let task = TASK0.get_or_init(|| {
        HostTask(UnsafeCell::new(CTask {
            addr: ptr::null_mut(),
            pix_width: GR_WIDTH,
            pix_height: GR_HEIGHT,
            draw_it: None,
        }))
    });
    let cpu = CPU0.get_or_init(|| HostCpu(UnsafeCell::new(CCPU { num: 0 })));
    let tp = task.0.get();
    unsafe { (*tp).addr = tp };
    FS.set(tp);
    GS.set(cpu.0.get());
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

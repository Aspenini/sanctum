use std::cell::{Cell, RefCell};
use std::ffi::CStr;
use std::ptr;
use tos_abi::CTask;

struct TaskSettings {
    task: *mut CTask,
    draw_it: Option<extern "C" fn(*mut CTask, *mut tos_abi::CDC)>,
    task_end_cb: Option<extern "C" fn()>,
    song_task: *mut CTask,
    animate_task: *mut CTask,
    autocomplete: bool,
    border: bool,
    cursor: bool,
}

struct Menu {
    source: Vec<u8>,
}

thread_local! {
    static SETTINGS: RefCell<Vec<Box<TaskSettings>>> = const { RefCell::new(Vec::new()) };
    static MENUS: RefCell<Vec<Box<Menu>>> = const { RefCell::new(Vec::new()) };
    static AUTOCOMPLETE: Cell<bool> = const { Cell::new(false) };
    static BORDER: Cell<bool> = const { Cell::new(true) };
    static CURSOR: Cell<bool> = const { Cell::new(true) };
}

pub fn reset() {
    SETTINGS.with(|settings| settings.borrow_mut().clear());
    MENUS.with(|menus| menus.borrow_mut().clear());
    AUTOCOMPLETE.set(false);
    BORDER.set(true);
    CURSOR.set(true);
}

pub fn current_menu_source() -> Option<Vec<u8>> {
    MENUS.with(|menus| menus.borrow().last().map(|menu| menu.source.clone()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_SettingsPush(task: *mut CTask, _flags: i64) -> *mut u8 {
    let task = if task.is_null() {
        super::task::fs()
    } else {
        task
    };
    let mut saved = Box::new(TaskSettings {
        task,
        draw_it: unsafe { (*task).draw_it },
        task_end_cb: unsafe { (*task).task_end_cb },
        song_task: unsafe { (*task).song_task },
        animate_task: unsafe { (*task).animate_task },
        autocomplete: AUTOCOMPLETE.get(),
        border: BORDER.get(),
        cursor: CURSOR.get(),
    });
    unsafe {
        (*task).song_task = ptr::null_mut();
        (*task).animate_task = ptr::null_mut();
    }
    let address = (&mut *saved as *mut TaskSettings).cast::<u8>();
    SETTINGS.with(|settings| settings.borrow_mut().push(saved));
    address
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_SettingsPop(task: *mut CTask, _flags: i64) {
    let task = if task.is_null() {
        super::task::fs()
    } else {
        task
    };
    let saved = SETTINGS.with(|settings| {
        let mut settings = settings.borrow_mut();
        let index = settings.iter().rposition(|saved| saved.task == task)?;
        Some(settings.remove(index))
    });
    let Some(saved) = saved else {
        return;
    };
    unsafe {
        (*task).draw_it = saved.draw_it;
        (*task).task_end_cb = saved.task_end_cb;
        (*task).song_task = saved.song_task;
        (*task).animate_task = saved.animate_task;
    }
    AUTOCOMPLETE.set(saved.autocomplete);
    BORDER.set(saved.border);
    CURSOR.set(saved.cursor);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_MenuPush(menu: *const u8) -> *mut u8 {
    let source = if menu.is_null() {
        Vec::new()
    } else {
        unsafe { CStr::from_ptr(menu.cast()) }.to_bytes().to_vec()
    };
    let mut menu = Box::new(Menu { source });
    let address = (&mut *menu as *mut Menu).cast::<u8>();
    MENUS.with(|menus| menus.borrow_mut().push(menu));
    address
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_MenuPop() {
    MENUS.with(|menus| {
        menus.borrow_mut().pop();
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_AutoComplete(enabled: i64) -> i64 {
    i64::from(AUTOCOMPLETE.replace(enabled != 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_WinBorder(enabled: i64, _task: *mut CTask) -> i64 {
    i64::from(BORDER.replace(enabled != 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_WinMax(_task: *mut CTask) {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_DocCursor(show: i64, _doc: *mut u8) -> i64 {
    i64::from(CURSOR.replace(show != 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_DocClear(_doc: *mut u8, _clear_holds: i64) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_stack_retains_source_for_a_graphical_host() {
        reset();
        let first = b"File { Exit; }\0";
        let second = b"Play { Left; Right; }\0";
        unsafe {
            assert!(!tos_MenuPush(first.as_ptr()).is_null());
            assert!(!tos_MenuPush(second.as_ptr()).is_null());
        }
        assert_eq!(
            current_menu_source().as_deref(),
            Some(&second[..second.len() - 1])
        );
        tos_MenuPop();
        assert_eq!(
            current_menu_source().as_deref(),
            Some(&first[..first.len() - 1])
        );
        tos_MenuPop();
        assert!(current_menu_source().is_none());
    }

    #[test]
    fn settings_restore_reduced_task_and_shell_state() {
        reset();
        let task = super::super::task::fs();
        let old_song = 1usize as *mut CTask;
        unsafe { (*task).song_task = old_song };
        let saved = unsafe { tos_SettingsPush(task, 0) };
        assert!(!saved.is_null());
        assert!(unsafe { (*task).song_task }.is_null());
        assert_eq!(tos_WinBorder(0, task), 1);
        assert_eq!(tos_DocCursor(0, ptr::null_mut()), 1);
        assert_eq!(tos_AutoComplete(1), 0);

        unsafe { tos_SettingsPop(task, 0) };
        assert_eq!(unsafe { (*task).song_task }, old_song);
        assert_eq!(tos_WinBorder(1, task), 1);
        assert_eq!(tos_DocCursor(1, ptr::null_mut()), 1);
        assert_eq!(tos_AutoComplete(0), 0);
        unsafe { (*task).song_task = ptr::null_mut() };
    }
}

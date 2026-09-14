//! Host display and input boundary for TempleOS programs.

mod audio;
mod fs;
pub mod input;
mod registry;

pub use registry::GlobalBinding;

use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use tos_abi::{CDC, CMsStateGlbls, MS_STATE_SIZE};

pub const DEFAULT_WIDTH: u32 = 640;
pub const DEFAULT_HEIGHT: u32 = 480;
const HEADLESS_FRAME_LIMIT: usize = 12;

static SCREEN: OnceLock<usize> = OnceLock::new();
static FRAMES_PRESENTED: AtomicUsize = AtomicUsize::new(0);
static INTERACTIVE: AtomicBool = AtomicBool::new(false);
static HOST_MODE: AtomicUsize = AtomicUsize::new(HostMode::Headless as usize);
static WINMGR_GLOBAL: AtomicUsize = AtomicUsize::new(0);
static MS_GLOBAL: AtomicUsize = AtomicUsize::new(0);
static MS_X: AtomicI64 = AtomicI64::new(0);
static MS_Y: AtomicI64 = AtomicI64::new(0);
static MS_BUTTONS: AtomicU8 = AtomicU8::new(0);
static KEYS: OnceLock<Mutex<VecDeque<(i64, i64)>>> = OnceLock::new();
static FRAME: OnceLock<Mutex<FrameSnapshot>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(usize)]
pub enum HostMode {
    #[default]
    Headless = 0,
    NativeWindow = 1,
    External = 2,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameSnapshot {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub indexed: Vec<u8>,
    pub menu: Option<Vec<u8>>,
}

impl Default for FrameSnapshot {
    fn default() -> Self {
        Self {
            sequence: 0,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            indexed: vec![0; DEFAULT_WIDTH as usize * DEFAULT_HEIGHT as usize],
            menu: None,
        }
    }
}

thread_local! {
    static WINDOW: RefCell<Option<HostWindow>> = const { RefCell::new(None) };
}

struct HostWindow {
    window: Window,
    pixels: Vec<u32>,
}

pub fn jit_symbols() -> Vec<(&'static str, *const u8)> {
    vec![
        ("tos_Refresh", tos_Refresh as *const u8),
        ("tos_ScanKey", tos_ScanKey as *const u8),
        ("tos_GetKey", tos_GetKey as *const u8),
        ("tos_SndTaskEndCB", audio::tos_SndTaskEndCB as *const u8),
        ("tos_Beep", audio::tos_Beep as *const u8),
        ("tos_Snd", audio::tos_Snd as *const u8),
        ("tos_Play", audio::tos_Play as *const u8),
        (
            "tos_MusicSettingsRst",
            audio::tos_MusicSettingsRst as *const u8,
        ),
        ("tos_RegDft", registry::tos_RegDft as *const u8),
        ("tos_RegExe", registry::tos_RegExe as *const u8),
        ("tos_RegWrite", registry::tos_RegWrite as *const u8),
        ("tos_FileRead", fs::tos_FileRead as *const u8),
        ("tos_FileFind", fs::tos_FileFind as *const u8),
        ("tos_FilesFind", fs::tos_FilesFind as *const u8),
        ("tos_DirEntryDel", fs::tos_DirEntryDel as *const u8),
        ("tos_DirEntryDel2", fs::tos_DirEntryDel2 as *const u8),
        ("tos_DirTreeDel", fs::tos_DirTreeDel as *const u8),
        ("tos_DirTreeDel2", fs::tos_DirTreeDel2 as *const u8),
        ("tos_Cd", fs::tos_Cd as *const u8),
        ("tos_IsDir", fs::tos_IsDir as *const u8),
        ("tos_DirCur", fs::tos_DirCur as *const u8),
    ]
}

pub fn reset() {
    audio::unbind();
    audio::reset();
    registry::reset();
    fs::reset();
    tos_runtime::reset_exceptions();
    WINMGR_GLOBAL.store(0, Ordering::Release);
    MS_GLOBAL.store(0, Ordering::Release);
    MS_X.store(0, Ordering::Release);
    MS_Y.store(0, Ordering::Release);
    MS_BUTTONS.store(0, Ordering::Release);
    FRAMES_PRESENTED.store(0, Ordering::Release);
    WINDOW.with(|window| *window.borrow_mut() = None);
    key_queue().lock().expect("key queue poisoned").clear();
    *frame().lock().expect("frame snapshot poisoned") = FrameSnapshot::default();
    let dc = screen();
    tos_gr::set_default_dc(dc);
    if let Some(dc) = unsafe { dc.as_ref() }
        && !dc.body.is_null()
    {
        unsafe {
            std::ptr::write_bytes(
                dc.body,
                0,
                (dc.width as usize).saturating_mul(dc.height as usize),
            )
        };
    }
}

/// Stop host resources that can outlive a cancelled HolyC task.
pub fn shutdown() {
    audio::stop();
    audio::unbind();
    registry::unbind();
    WINMGR_GLOBAL.store(0, Ordering::Release);
    MS_GLOBAL.store(0, Ordering::Release);
}

pub fn set_file_roots(project: Option<std::path::PathBuf>, system: Option<std::path::PathBuf>) {
    fs::set_roots(project, system);
}

pub fn bind_globals(bindings: &[GlobalBinding<'_>]) {
    registry::bind(bindings);
    if let Some(music) = bindings.iter().find(|binding| binding.name == "music") {
        audio::bind_music_global(music.address, music.size);
    } else {
        audio::unbind();
    }
    if let Some(binding) = bindings
        .iter()
        .find(|binding| binding.name == "winmgr" && binding.size >= size_of::<i64>())
    {
        WINMGR_GLOBAL.store(binding.address as usize, Ordering::Release);
    }
    if let Some(binding) = bindings
        .iter()
        .find(|binding| binding.name == "ms" && binding.size >= MS_STATE_SIZE)
    {
        MS_GLOBAL.store(binding.address as usize, Ordering::Release);
        write_bound_mouse();
    }
}

fn advance_window_update() {
    let address = WINMGR_GLOBAL.load(Ordering::Acquire);
    if address != 0 {
        let updates = address as *mut i64;
        unsafe {
            updates.write_unaligned(updates.read_unaligned().wrapping_add(1));
        }
    }
}

pub fn set_interactive(interactive: bool) {
    set_host_mode(if interactive {
        HostMode::NativeWindow
    } else {
        HostMode::Headless
    });
    registry::set_persistent(interactive);
}

pub fn set_host_mode(mode: HostMode) {
    HOST_MODE.store(mode as usize, Ordering::Release);
    INTERACTIVE.store(mode != HostMode::Headless, Ordering::Release);
    registry::set_persistent(mode != HostMode::Headless);
}

pub fn host_mode() -> HostMode {
    match HOST_MODE.load(Ordering::Acquire) {
        1 => HostMode::NativeWindow,
        2 => HostMode::External,
        _ => HostMode::Headless,
    }
}

fn screen() -> *mut CDC {
    *SCREEN.get_or_init(|| {
        tos_gr::tos_DCNew(
            i64::from(DEFAULT_WIDTH),
            i64::from(DEFAULT_HEIGHT),
            ptr::null_mut(),
            0,
        ) as usize
    }) as *mut CDC
}

fn key_queue() -> &'static Mutex<VecDeque<(i64, i64)>> {
    KEYS.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn frame() -> &'static Mutex<FrameSnapshot> {
    FRAME.get_or_init(|| Mutex::new(FrameSnapshot::default()))
}

pub fn frames_presented() -> usize {
    FRAMES_PRESENTED.load(Ordering::Acquire)
}

pub fn frame_snapshot_after(sequence: u64) -> Option<FrameSnapshot> {
    let snapshot = frame().lock().expect("frame snapshot poisoned");
    (snapshot.sequence > sequence).then(|| snapshot.clone())
}

pub fn push_key_event(ch: i64, scan_code: i64) {
    key_queue()
        .lock()
        .expect("key queue poisoned")
        .push_back((ch, scan_code));
}

pub fn request_exit() {
    push_key_event(0x1b, 0);
    tos_runtime::request_throw(0x1b);
    tos_runtime::cancel_background_tasks();
}

pub fn set_mouse(x: i64, y: i64, left: bool, right: bool) {
    MS_X.store(x, Ordering::Release);
    MS_Y.store(y, Ordering::Release);
    MS_BUTTONS.store(u8::from(left) | (u8::from(right) << 1), Ordering::Release);
    write_bound_mouse();
}

fn write_bound_mouse() {
    let address = MS_GLOBAL.load(Ordering::Acquire);
    if address == 0 {
        return;
    }
    let ms = address as *mut CMsStateGlbls;
    unsafe {
        (*ms).pos.x = MS_X.load(Ordering::Acquire);
        (*ms).pos.y = MS_Y.load(Ordering::Acquire);
        let buttons = MS_BUTTONS.load(Ordering::Acquire);
        (*ms).lb = buttons & 1;
        (*ms).rb = (buttons >> 1) & 1;
        (*ms).show = 1;
    }
}

pub fn set_muted(muted: bool) {
    audio::set_muted(muted);
}

/// Copy the last indexed framebuffer through the standard TempleOS palette.
pub fn framebuffer_rgb() -> Vec<u8> {
    let snapshot = frame().lock().expect("frame snapshot poisoned");
    let mut rgb = Vec::with_capacity(snapshot.indexed.len() * 3);
    for &pixel in &snapshot.indexed {
        rgb.extend_from_slice(&tos_gr::palette_rgb(pixel));
    }
    rgb
}

fn native_printable_char(key: Key, shift: bool) -> Option<char> {
    let base = match key {
        Key::A => 'a',
        Key::B => 'b',
        Key::C => 'c',
        Key::D => 'd',
        Key::E => 'e',
        Key::F => 'f',
        Key::G => 'g',
        Key::H => 'h',
        Key::I => 'i',
        Key::J => 'j',
        Key::K => 'k',
        Key::L => 'l',
        Key::M => 'm',
        Key::N => 'n',
        Key::O => 'o',
        Key::P => 'p',
        Key::Q => 'q',
        Key::R => 'r',
        Key::S => 's',
        Key::T => 't',
        Key::U => 'u',
        Key::V => 'v',
        Key::W => 'w',
        Key::X => 'x',
        Key::Y => 'y',
        Key::Z => 'z',
        Key::Key0 => '0',
        Key::Key1 => '1',
        Key::Key2 => '2',
        Key::Key3 => '3',
        Key::Key4 => '4',
        Key::Key5 => '5',
        Key::Key6 => '6',
        Key::Key7 => '7',
        Key::Key8 => '8',
        Key::Key9 => '9',
        Key::Apostrophe => '\'',
        Key::Backquote => '`',
        Key::Backslash => '\\',
        Key::Comma => ',',
        Key::Equal => '=',
        Key::LeftBracket => '[',
        Key::Minus => '-',
        Key::Period => '.',
        Key::RightBracket => ']',
        Key::Semicolon => ';',
        Key::Slash => '/',
        _ => return None,
    };
    if !shift {
        return Some(base);
    }
    Some(match base {
        'a'..='z' => base.to_ascii_uppercase(),
        '0' => ')',
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '\'' => '"',
        '`' => '~',
        '\\' => '|',
        ',' => '<',
        '=' => '+',
        '[' => '{',
        '-' => '_',
        '.' => '>',
        ']' => '}',
        ';' => ':',
        '/' => '?',
        _ => base,
    })
}

fn native_key_event(key: Key, shift: bool, ctrl: bool, alt: bool) -> Option<(i64, i64)> {
    let flags = input::scan_flags(shift, ctrl, alt);
    match key {
        Key::Escape if shift => Some((0x1c, 0x01 | flags)),
        Key::Escape => Some((0x1b, 0x01 | flags)),
        Key::Tab => Some((b'\t' as i64, 0x0f | flags)),
        Key::Backspace => Some((0x08, 0x0e | flags)),
        Key::Enter => Some((b'\n' as i64, 0x1c | flags)),
        Key::Space if shift => Some((0x1f, 0x39 | flags)),
        Key::Space => Some((b' ' as i64, 0x39 | flags)),
        Key::Home => Some((0, 0x47 | flags)),
        Key::Up => Some((0, 0x48 | flags)),
        Key::PageUp => Some((0, 0x49 | flags)),
        Key::Left => Some((0, 0x4b | flags)),
        Key::Right => Some((0, 0x4d | flags)),
        Key::End => Some((0, 0x4f | flags)),
        Key::Down => Some((0, 0x50 | flags)),
        Key::PageDown => Some((0, 0x51 | flags)),
        Key::Insert => Some((0, 0x52 | flags)),
        Key::Delete => Some((0, 0x53 | flags)),
        Key::F1 => Some((0, 0x3b | flags)),
        Key::F2 => Some((0, 0x3c | flags)),
        Key::F3 => Some((0, 0x3d | flags)),
        Key::F4 => Some((0, 0x3e | flags)),
        Key::F5 => Some((0, 0x3f | flags)),
        Key::F6 => Some((0, 0x40 | flags)),
        Key::F7 => Some((0, 0x41 | flags)),
        Key::F8 => Some((0, 0x42 | flags)),
        Key::F9 => Some((0, 0x43 | flags)),
        Key::F10 => Some((0, 0x44 | flags)),
        Key::F11 => Some((0, 0x57 | flags)),
        Key::F12 => Some((0, 0x58 | flags)),
        Key::LeftShift | Key::RightShift => Some((0, 0x2a | flags)),
        Key::LeftCtrl | Key::RightCtrl => Some((0, 0x1d | flags)),
        Key::LeftAlt | Key::RightAlt => Some((0, 0x38 | flags)),
        _ => native_printable_char(key, shift)
            .and_then(|ch| input::ascii_key_event(ch, shift, ctrl, alt)),
    }
}

fn push_native_key(key: Key, shift: bool, ctrl: bool, alt: bool) {
    if let Some(key) = native_key_event(key, shift, ctrl, alt) {
        key_queue()
            .lock()
            .expect("key queue poisoned")
            .push_back(key);
    }
}

fn present_window(snapshot: &FrameSnapshot) {
    WINDOW.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            match Window::new(
                "Sanctum - TempleOS",
                DEFAULT_WIDTH as usize,
                DEFAULT_HEIGHT as usize,
                WindowOptions::default(),
            ) {
                Ok(window) => {
                    *slot = Some(HostWindow {
                        window,
                        pixels: vec![0; DEFAULT_WIDTH as usize * DEFAULT_HEIGHT as usize],
                    });
                }
                Err(error) => {
                    eprintln!("unable to create Sanctum window: {error}");
                    key_queue()
                        .lock()
                        .expect("key queue poisoned")
                        .push_back((0x1b, 0));
                    return;
                }
            }
        }

        let host = slot.as_mut().expect("window was initialized");
        for (dst, &color) in host.pixels.iter_mut().zip(&snapshot.indexed) {
            let [r, g, b] = tos_gr::palette_rgb(color);
            *dst = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
        }
        if let Err(error) = host.window.update_with_buffer(
            &host.pixels,
            DEFAULT_WIDTH as usize,
            DEFAULT_HEIGHT as usize,
        ) {
            eprintln!("unable to update Sanctum window: {error}");
            key_queue()
                .lock()
                .expect("key queue poisoned")
                .push_back((0x1b, 0));
            return;
        }
        if !host.window.is_open() {
            key_queue()
                .lock()
                .expect("key queue poisoned")
                .push_back((0x1b, 0));
        }
        let shift =
            host.window.is_key_down(Key::LeftShift) || host.window.is_key_down(Key::RightShift);
        let ctrl =
            host.window.is_key_down(Key::LeftCtrl) || host.window.is_key_down(Key::RightCtrl);
        let alt = host.window.is_key_down(Key::LeftAlt) || host.window.is_key_down(Key::RightAlt);
        for key in host.window.get_keys_pressed(KeyRepeat::Yes) {
            push_native_key(key, shift, ctrl, alt);
        }
        if let Some((x, y)) = host.window.get_mouse_pos(MouseMode::Clamp) {
            set_mouse(
                x.round() as i64,
                y.round() as i64,
                host.window.get_mouse_down(MouseButton::Left),
                host.window.get_mouse_down(MouseButton::Right),
            );
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Refresh() {
    tos_runtime::background_checkpoint();
    let _ = tos_runtime::tos_HasExcept();
    advance_window_update();
    let task = tos_runtime::tos_Fs();
    let dc = screen();
    if let Some(draw) = unsafe { task.as_ref() }.and_then(|task| task.draw_it) {
        draw(task, dc);
    }
    let sequence = FRAMES_PRESENTED.fetch_add(1, Ordering::AcqRel) as u64 + 1;
    if let Some(dc) = unsafe { dc.as_ref() }
        && !dc.body.is_null()
    {
        let indexed = unsafe {
            slice::from_raw_parts(
                dc.body,
                (dc.width as usize).saturating_mul(dc.height as usize),
            )
        }
        .to_vec();
        let snapshot = FrameSnapshot {
            sequence,
            width: dc.width.max(0) as u32,
            height: dc.height.max(0) as u32,
            indexed,
            menu: tos_runtime::current_menu_source(),
        };
        *frame().lock().expect("frame snapshot poisoned") = snapshot.clone();
        if host_mode() == HostMode::NativeWindow {
            present_window(&snapshot);
        }
    } else if host_mode() == HostMode::NativeWindow {
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

#[unsafe(no_mangle)]
/// Poll the shared host key queue using TempleOS `ScanKey` conventions.
///
/// # Safety
///
/// Non-null output pointers must be valid and writable for one `i64`.
pub unsafe extern "C" fn tos_ScanKey(ch: *mut i64, scan_code: *mut i64, _echo: i64) -> i64 {
    if INTERACTIVE.load(Ordering::Acquire) {
        let key = key_queue().lock().expect("key queue poisoned").pop_front();
        let Some((key_ch, key_scan)) = key else {
            return 0;
        };
        if key_ch == 0x1b {
            tos_runtime::cancel_background_tasks();
        }
        if !ch.is_null() {
            unsafe { *ch = key_ch };
        }
        if !scan_code.is_null() {
            unsafe { *scan_code = key_scan };
        }
        return 1;
    }
    // Give progressive renderers enough updates to publish actual geometry
    // before the headless host sends Escape. Interactive hosts use their key
    // queue and have no frame limit.
    if frames_presented() < HEADLESS_FRAME_LIMIT {
        return 0;
    }
    if !ch.is_null() {
        unsafe { *ch = 0x1b };
    }
    if !scan_code.is_null() {
        unsafe { *scan_code = 0 };
    }
    1
}

#[unsafe(no_mangle)]
/// Block until `ScanKey` reports a key, presenting frames like TempleOS's
/// window manager while the foreground task waits.
///
/// # Safety
///
/// Non-null `scan_code` must be valid and writable for one `i64`.
pub unsafe extern "C" fn tos_GetKey(scan_code: *mut i64, echo: i64, _raw_cursor: i64) -> i64 {
    loop {
        let mut ch = 0_i64;
        let mut sc = 0_i64;
        if unsafe { tos_ScanKey(&mut ch, &mut sc, echo) } != 0 {
            if !scan_code.is_null() {
                unsafe { *scan_code = sc };
            }
            return ch;
        }
        tos_Refresh();
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framebuffer_has_rgb_triplet_per_pixel() {
        assert_eq!(
            framebuffer_rgb().len(),
            DEFAULT_WIDTH as usize * DEFAULT_HEIGHT as usize * 3
        );
    }

    #[test]
    fn shares_window_keys_with_the_game_thread() {
        key_queue().lock().unwrap().clear();
        std::thread::spawn(|| push_native_key(Key::Up, false, false, false))
            .join()
            .unwrap();
        set_interactive(true);
        let (mut ch, mut scan) = (-1, -1);
        unsafe { assert_eq!(tos_ScanKey(&mut ch, &mut scan, 0), 1) };
        assert_eq!((ch, scan), (0, 0x48));
        set_interactive(false);
    }

    #[test]
    fn native_window_maps_printable_and_extended_keys() {
        assert_eq!(
            native_key_event(Key::A, true, false, false),
            Some((b'A' as i64, 0x1e | input::SCF_SHIFT))
        );
        assert_eq!(
            native_key_event(Key::C, false, true, false),
            Some((3, 0x2e | input::SCF_CTRL))
        );
        assert_eq!(
            native_key_event(Key::Slash, true, false, true),
            Some((b'?' as i64, 0x35 | input::SCF_SHIFT | input::SCF_ALT))
        );
        assert_eq!(
            native_key_event(Key::F12, false, false, false),
            Some((0, 0x58))
        );
    }

    #[test]
    fn refresh_counter_updates_the_bound_winmgr_global() {
        let mut updates = 41_i64;
        bind_globals(&[GlobalBinding {
            name: "winmgr",
            address: (&mut updates as *mut i64).cast(),
            size: size_of::<i64>(),
        }]);
        advance_window_update();
        assert_eq!(updates, 42);
        WINMGR_GLOBAL.store(0, Ordering::Release);
        MS_GLOBAL.store(0, Ordering::Release);
    }

    #[test]
    fn mouse_writes_through_the_bound_global() {
        let mut ms = unsafe { std::mem::zeroed::<CMsStateGlbls>() };
        bind_globals(&[GlobalBinding {
            name: "ms",
            address: (&mut ms as *mut CMsStateGlbls).cast(),
            size: MS_STATE_SIZE,
        }]);
        set_mouse(150, 250, true, false);
        assert_eq!((ms.pos.x, ms.pos.y, ms.lb, ms.rb), (150, 250, 1, 0));
        set_mouse(10, 20, false, true);
        assert_eq!((ms.pos.x, ms.pos.y, ms.lb, ms.rb), (10, 20, 0, 1));
        MS_GLOBAL.store(0, Ordering::Release);
    }
}

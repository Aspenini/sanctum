//! Host display and input boundary for TempleOS programs.

mod audio;

use minifb::{Key, KeyRepeat, Window, WindowOptions};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use tos_abi::CDC;

pub const DEFAULT_WIDTH: u32 = 640;
pub const DEFAULT_HEIGHT: u32 = 480;

static SCREEN: OnceLock<usize> = OnceLock::new();
static FRAMES_PRESENTED: AtomicUsize = AtomicUsize::new(0);
static INTERACTIVE: AtomicBool = AtomicBool::new(false);
static KEYS: OnceLock<Mutex<VecDeque<(i64, i64)>>> = OnceLock::new();

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
        ("tos_SndTaskEndCB", audio::tos_SndTaskEndCB as *const u8),
        ("tos_Beep", audio::tos_Beep as *const u8),
        ("tos_Snd", audio::tos_Snd as *const u8),
        ("tos_Play", audio::tos_Play as *const u8),
        (
            "tos_MusicSettingsRst",
            audio::tos_MusicSettingsRst as *const u8,
        ),
    ]
}

pub fn reset() {
    audio::reset();
    FRAMES_PRESENTED.store(0, Ordering::Release);
    WINDOW.with(|window| *window.borrow_mut() = None);
    key_queue().lock().expect("key queue poisoned").clear();
}

/// Stop host resources that can outlive a cancelled HolyC task.
pub fn shutdown() {
    audio::stop();
}

pub fn set_interactive(interactive: bool) {
    INTERACTIVE.store(interactive, Ordering::Release);
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

pub fn frames_presented() -> usize {
    FRAMES_PRESENTED.load(Ordering::Acquire)
}

/// Copy the last indexed framebuffer through the standard TempleOS palette.
pub fn framebuffer_rgb() -> Vec<u8> {
    let dc = screen();
    if dc.is_null() {
        return Vec::new();
    }
    let dc = unsafe { &*dc };
    if dc.body.is_null() {
        return Vec::new();
    }
    let pixels = unsafe {
        slice::from_raw_parts(
            dc.body,
            (dc.width as usize).saturating_mul(dc.height as usize),
        )
    };
    let mut rgb = Vec::with_capacity(pixels.len() * 3);
    for &pixel in pixels {
        rgb.extend_from_slice(&tos_gr::palette_rgb(pixel));
    }
    rgb
}

fn push_key(key: Key) {
    let mapped = match key {
        Key::Escape => Some((0x1b, 0)),
        Key::Enter => Some((b'\n' as i64, 0)),
        Key::Space => Some((b' ' as i64, 0)),
        Key::Up => Some((0, 0x48)),
        Key::Down => Some((0, 0x50)),
        Key::Left => Some((0, 0x4b)),
        Key::Right => Some((0, 0x4d)),
        _ => None,
    };
    if let Some(key) = mapped {
        key_queue()
            .lock()
            .expect("key queue poisoned")
            .push_back(key);
    }
}

fn present_window() {
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
        let dc = screen();
        if let Some(dc) = unsafe { dc.as_ref() }
            && !dc.body.is_null()
        {
            let indexed = unsafe {
                slice::from_raw_parts(
                    dc.body,
                    (dc.width as usize).saturating_mul(dc.height as usize),
                )
            };
            for (dst, &color) in host.pixels.iter_mut().zip(indexed) {
                let [r, g, b] = tos_gr::palette_rgb(color);
                *dst = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
            }
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
        for key in host.window.get_keys_pressed(KeyRepeat::Yes) {
            push_key(key);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Refresh() {
    tos_runtime::background_checkpoint();
    let task = tos_runtime::tos_Fs();
    if let Some(draw) = unsafe { task.as_ref() }.and_then(|task| task.draw_it) {
        draw(task, screen());
        FRAMES_PRESENTED.fetch_add(1, Ordering::AcqRel);
        if INTERACTIVE.load(Ordering::Acquire) {
            present_window();
        }
    } else if INTERACTIVE.load(Ordering::Acquire) {
        // Background animation tasks use Refresh as their cooperative yield.
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
    // Let the HolyC loop produce and present one frame before the headless
    // host sends Escape. Interactive hosts replace this with their key queue.
    if frames_presented() == 0 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framebuffer_has_rgb_triplet_per_pixel() {
        reset();
        assert_eq!(
            framebuffer_rgb().len(),
            DEFAULT_WIDTH as usize * DEFAULT_HEIGHT as usize * 3
        );
    }

    #[test]
    fn shares_window_keys_with_the_game_thread() {
        key_queue().lock().unwrap().clear();
        std::thread::spawn(|| push_key(Key::Up)).join().unwrap();
        set_interactive(true);
        let (mut ch, mut scan) = (-1, -1);
        unsafe { assert_eq!(tos_ScanKey(&mut ch, &mut scan, 0), 1) };
        assert_eq!((ch, scan), (0, 0x48));
        set_interactive(false);
    }
}

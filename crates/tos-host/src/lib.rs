//! Host display and input boundary for TempleOS programs.

use std::ptr;
use std::slice;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use tos_abi::CDC;

pub const DEFAULT_WIDTH: u32 = 640;
pub const DEFAULT_HEIGHT: u32 = 480;

static SCREEN: OnceLock<usize> = OnceLock::new();
static FRAMES_PRESENTED: AtomicUsize = AtomicUsize::new(0);

pub fn jit_symbols() -> Vec<(&'static str, *const u8)> {
    vec![
        ("tos_Refresh", tos_Refresh as *const u8),
        ("tos_ScanKey", tos_ScanKey as *const u8),
    ]
}

pub fn reset() {
    FRAMES_PRESENTED.store(0, Ordering::Release);
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

#[unsafe(no_mangle)]
pub extern "C" fn tos_Refresh() {
    let task = tos_runtime::tos_Fs();
    if let Some(draw) = unsafe { task.as_ref() }.and_then(|task| task.draw_it) {
        draw(task, screen());
        FRAMES_PRESENTED.fetch_add(1, Ordering::AcqRel);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_ScanKey(ch: *mut i64, scan_code: *mut i64, _echo: i64) -> i64 {
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
}

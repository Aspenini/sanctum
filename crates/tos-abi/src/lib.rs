//! `#[repr(C)]` layouts matching TempleOS `KernelA.HH`.
//! Field offsets are the ABI. Tests pin the ones HolyC code actually pokes.

#![allow(non_camel_case_types)]

pub const STR_LEN: usize = 144;
pub const COLORS_NUM: usize = 16;
pub const GR_WIDTH: i64 = 640;
pub const GR_HEIGHT: i64 = 480;
pub const FONT_WIDTH: i64 = 8;
pub const FONT_HEIGHT: i64 = 8;
pub const JIFFY_FREQ: i64 = 1000;
pub const MP_PROCESSORS_NUM: usize = 128;
pub const BIN_SIGNATURE_VAL: u32 = u32::from_le_bytes(*b"TOSB");

/// Circular queue head. TempleOS `CQue`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CQue {
    pub next: *mut CQue,
    pub last: *mut CQue,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CD3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CD3I32 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CD3I64 {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

/// Packed color + ROP. HolyC `CColorROPU32`.
pub type CColorROPU32 = u32;

pub const BLACK: u32 = 0;
pub const BLUE: u32 = 1;
pub const GREEN: u32 = 2;
pub const CYAN: u32 = 3;
pub const RED: u32 = 4;
pub const PURPLE: u32 = 5;
pub const BROWN: u32 = 6;
pub const LTGRAY: u32 = 7;
pub const DKGRAY: u32 = 8;
pub const LTBLUE: u32 = 9;
pub const LTGREEN: u32 = 10;
pub const LTCYAN: u32 = 11;
pub const LTRED: u32 = 12;
pub const LTPURPLE: u32 = 13;
pub const YELLOW: u32 = 14;
pub const WHITE: u32 = 15;

pub const ROPF_DITHER: u32 = 0x40000000;

/// Current-task block. HolyC `Fs` points here. Offsets must match the
/// compiler's injected `CTask` class (not yet the full KernelA.HH layout).
#[repr(C)]
pub struct CTask {
    pub addr: *mut CTask,
    pub pix_width: i64,
    pub pix_height: i64,
    pub draw_it: Option<extern "C" fn(*mut CTask, *mut CDC)>,
    pub task_end_cb: Option<extern "C" fn()>,
    pub song_task: *mut CTask,
    pub animate_task: *mut CTask,
}

/// Current CPU. HolyC `Gs` points here.
#[repr(C)]
pub struct CCPU {
    pub num: i64,
    pub idle_factor: f64,
}

/// Graphics device context. Layout will be matched to KernelA.HH in Phase 3.
#[repr(C)]
pub struct CDC {
    pub width: i32,
    pub height: i32,
    pub flags: i32,
    pub color: CColorROPU32,
    pub r: *mut i64,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub thick: i32,
    pub transform: Option<extern "C" fn(*mut CDC, *mut i64, *mut i64, *mut i64)>,
    pub body: *mut u8,
    pub depth_buf: *mut i32,
    // Host-side extension used to model TempleOS `CGrSym`. Appending these
    // fields preserves every HolyC-visible offset above.
    pub sym_x: i32,
    pub sym_y: i32,
    pub sym_z: i32,
    pub sym_nx: f64,
    pub sym_ny: f64,
    pub sym_nz: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn que_layout() {
        assert_eq!(offset_of!(CQue, next), 0);
        assert_eq!(offset_of!(CQue, last), 8);
    }
}

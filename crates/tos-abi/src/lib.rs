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
    pub pix_left: i64,
    pub pix_top: i64,
}

impl CTask {
    pub fn host_default() -> Self {
        Self {
            addr: std::ptr::null_mut(),
            pix_width: GR_WIDTH,
            pix_height: GR_HEIGHT,
            draw_it: None,
            task_end_cb: None,
            song_task: std::ptr::null_mut(),
            animate_task: std::ptr::null_mut(),
            pix_left: 0,
            pix_top: 0,
        }
    }
}

/// Packed public mouse state. Field offsets match the HolyC `CMsStateGlbls`
/// subset programs actually read (`pos` and button flags).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CMsStateGlbls {
    pub pos: CD3I64,
    pub pos_text: CD3I64,
    pub presnap: CD3I64,
    pub offset: CD3I64,
    pub scale: CD3,
    pub speed: f64,
    pub timestamp: i64,
    pub dbl_time: f64,
    pub left_dbl_time: f64,
    pub right_dbl_time: f64,
    pub lb: u8,
    pub rb: u8,
    pub show: u8,
    pub has_wheel: u8,
    pub left_dbl: u8,
    pub left_down_sent: u8,
    pub right_dbl: u8,
    pub right_down_sent: u8,
}

pub const MS_STATE_SIZE: usize = 168;

pub const CDIR_FILENAME_LEN: usize = 38;
pub const CDIR_ENTRY_SIZE: usize = 112;
pub const RS_ATTR_DIR: u16 = 0x10;
pub const RS_ATTR_COMPRESSED: u16 = 0x400;

pub const FUF_RECURSE: i64 = 1;
pub const FUF_SINGLE: i64 = 1 << 9;
pub const FUF_JUST_DIRS: i64 = 1 << 10;
pub const FUF_JUST_FILES: i64 = 1 << 11;
pub const FUF_Z_OR_NOT_Z: i64 = 1 << 18;
pub const FUF_SCAN_PARENTS: i64 = 1 << 20;

/// Packed `CDirEntry` from KernelA.HH. Offsets are the ABI.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CDirEntry {
    pub next: *mut CDirEntry,
    pub parent: *mut CDirEntry,
    pub sub: *mut CDirEntry,
    pub full_name: *mut u8,
    pub user_data: i64,
    pub user_data2: i64,
    pub attr: u16,
    pub name: [u8; CDIR_FILENAME_LEN],
    pub clus: i64,
    pub size: i64,
    pub datetime: i64,
}

/// Current CPU. HolyC `Gs` points here.
#[repr(C)]
pub struct CCPU {
    pub num: i64,
    pub idle_factor: f64,
}

/// Packed public music settings used by TempleOS programs. Its deliberately
/// unaligned fields match `CMusicGlbls` rather than Rust's native layout.
#[repr(C, packed)]
pub struct CMusicGlbls {
    pub cur_song: *mut u8,
    pub cur_song_task: *mut CTask,
    pub octave: i64,
    pub note_len: f64,
    pub note_map: [u8; 7],
    pub mute: i64,
    pub meter_top: i64,
    pub meter_bottom: i64,
    pub tempo: f64,
    pub staccato_factor: f64,
    pub play_note_num: i64,
    pub tm_correction: f64,
    pub last_beat: f64,
    pub last_tm: f64,
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
    pub transform: Option<unsafe extern "C" fn(*mut CDC, *mut i64, *mut i64, *mut i64)>,
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
    // More host-only state used by TempleOS's default mesh lighting callback.
    pub light_x: i32,
    pub light_y: i32,
    pub light_z: i32,
    pub dither_probability_u16: u32,
    // Host-only ownership markers. `DCAlias` shares the source image and
    // depth buffers, while every context owns its header and rotation matrix.
    pub owns_body: bool,
    pub owns_depth_buf: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn que_layout() {
        assert_eq!(offset_of!(CQue, next), 0);
        assert_eq!(offset_of!(CQue, last), 8);
    }

    #[test]
    fn music_layout_matches_packed_holyc_class() {
        assert_eq!(size_of::<CMusicGlbls>(), 111);
        assert_eq!(offset_of!(CMusicGlbls, octave), 16);
        assert_eq!(offset_of!(CMusicGlbls, note_map), 32);
        assert_eq!(offset_of!(CMusicGlbls, mute), 39);
        assert_eq!(offset_of!(CMusicGlbls, tempo), 63);
        assert_eq!(offset_of!(CMusicGlbls, play_note_num), 79);
    }

    #[test]
    fn task_exposes_window_pixel_origin() {
        assert_eq!(offset_of!(CTask, pix_width), 8);
        assert_eq!(offset_of!(CTask, pix_left), 56);
        assert_eq!(offset_of!(CTask, pix_top), 64);
        assert_eq!(size_of::<CTask>(), 72);
    }

    #[test]
    fn mouse_layout_matches_injected_holyc_class() {
        assert_eq!(size_of::<CMsStateGlbls>(), MS_STATE_SIZE);
        assert_eq!(offset_of!(CMsStateGlbls, pos), 0);
        assert_eq!(offset_of!(CMsStateGlbls, lb), 160);
        assert_eq!(offset_of!(CMsStateGlbls, rb), 161);
    }

    #[test]
    fn dir_entry_layout_is_packed() {
        assert_eq!(size_of::<CDirEntry>(), CDIR_ENTRY_SIZE);
        assert_eq!(offset_of!(CDirEntry, full_name), 24);
        assert_eq!(offset_of!(CDirEntry, attr), 48);
        assert_eq!(offset_of!(CDirEntry, name), 50);
        assert_eq!(offset_of!(CDirEntry, size), 96);
        assert_eq!(offset_of!(CDirEntry, datetime), 104);
    }
}

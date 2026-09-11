//! TempleOS-compatible 16-color software graphics primitives.

use std::ffi::CStr;
use std::ptr;
use std::slice;
use tos_abi::{CD3I32, CDC, CTask};

const DCF_TRANSFORMATION: i32 = 0x100;
const DCF_SYMMETRY: i32 = 0x200;
const DCF_JUST_MIRROR: i32 = 0x400;
const ROPF_HALF_RANGE_COLOR: u32 = 0x1000;
const ROPF_TWO_SIDED: u32 = 0x2000;
const ROPF_PROBABILITY_DITHER: u32 = 0x80000000;

// TempleOS's standard 8x8 glyphs for printable ASCII, copied from
// Kernel/FontStd.HC. Each low-to-high byte is one scanline.
const FONT_ASCII: [u64; 96] = [
    0x0000000000000000,
    0x00180018183C3C18,
    0x0000000000363636,
    0x006C6CFE6CFE6C6C,
    0x00187ED07C16FC30,
    0x0060660C18306606,
    0x00DC66B61C36361C,
    0x0000000000181818,
    0x0030180C0C0C1830,
    0x000C18303030180C,
    0x0000187E3C7E1800,
    0x000018187E181800,
    0x0C18180000000000,
    0x000000007E000000,
    0x0018180000000000,
    0x0000060C18306000,
    0x003C666E7E76663C,
    0x007E181818181C18,
    0x007E0C183060663C,
    0x003C66603860663C,
    0x0030307E363C3830,
    0x003C6660603E067E,
    0x003C66663E060C38,
    0x000C0C0C1830607E,
    0x003C66663C66663C,
    0x001C30607C66663C,
    0x0018180018180000,
    0x0C18180018180000,
    0x0030180C060C1830,
    0x0000007E007E0000,
    0x000C18306030180C,
    0x001800181830663C,
    0x003C06765676663C,
    0x006666667E66663C,
    0x003E66663E66663E,
    0x003C66060606663C,
    0x001E36666666361E,
    0x007E06063E06067E,
    0x000606063E06067E,
    0x003C66667606663C,
    0x006666667E666666,
    0x007E18181818187E,
    0x001C36303030307C,
    0x0066361E0E1E3666,
    0x007E060606060606,
    0x00C6C6D6D6FEEEC6,
    0x006666767E6E6666,
    0x003C66666666663C,
    0x000606063E66663E,
    0x006C36566666663C,
    0x006666363E66663E,
    0x003C66603C06663C,
    0x001818181818187E,
    0x003C666666666666,
    0x00183C6666666666,
    0x00C6EEFED6D6C6C6,
    0x0066663C183C6666,
    0x001818183C666666,
    0x007E060C1830607E,
    0x003E06060606063E,
    0x00006030180C0600,
    0x007C60606060607C,
    0x000000000000663C,
    0xFFFF000000000000,
    0x000000000030180C,
    0x007C667C603C0000,
    0x003E6666663E0606,
    0x003C6606663C0000,
    0x007C6666667C6060,
    0x003C067E663C0000,
    0x000C0C0C3E0C0C38,
    0x3C607C66667C0000,
    0x00666666663E0606,
    0x003C1818181C0018,
    0x0E181818181C0018,
    0x0066361E36660606,
    0x003C18181818181C,
    0x00C6D6D6FE6C0000,
    0x00666666663E0000,
    0x003C6666663C0000,
    0x06063E66663E0000,
    0xE0607C66667C0000,
    0x000606066E360000,
    0x003E603C067C0000,
    0x00380C0C0C3E0C0C,
    0x007C666666660000,
    0x00183C6666660000,
    0x006CFED6D6C60000,
    0x00663C183C660000,
    0x3C607C6666660000,
    0x007E0C18307E0000,
    0x003018180E181830,
    0x0018181818181818,
    0x000C18187018180C,
    0x000000000062D68C,
    0xFFFFFFFFFFFFFFFF,
];

pub fn palette_rgb(color: u8) -> [u8; 3] {
    // Standard TempleOS 16-color VGA-ish palette.
    match color & 0x0F {
        0 => [0x00, 0x00, 0x00],
        1 => [0x00, 0x00, 0xAA],
        2 => [0x00, 0xAA, 0x00],
        3 => [0x00, 0xAA, 0xAA],
        4 => [0xAA, 0x00, 0x00],
        5 => [0xAA, 0x00, 0xAA],
        6 => [0xAA, 0x55, 0x00],
        7 => [0xAA, 0xAA, 0xAA],
        8 => [0x55, 0x55, 0x55],
        9 => [0x55, 0x55, 0xFF],
        10 => [0x55, 0xFF, 0x55],
        11 => [0x55, 0xFF, 0xFF],
        12 => [0xFF, 0x55, 0x55],
        13 => [0xFF, 0x55, 0xFF],
        14 => [0xFF, 0xFF, 0x55],
        _ => [0xFF, 0xFF, 0xFF],
    }
}

pub fn jit_symbols() -> Vec<(&'static str, *const u8)> {
    vec![
        ("tos_DCNew", tos_DCNew as *const u8),
        ("tos_DCAlias", tos_DCAlias as *const u8),
        ("tos_DCDel", tos_DCDel as *const u8),
        ("tos_DCFill", tos_DCFill as *const u8),
        ("tos_DCDepthBufAlloc", tos_DCDepthBufAlloc as *const u8),
        ("tos_DCDepthBufRst", tos_DCDepthBufRst as *const u8),
        ("tos_DCMat4x4Set", tos_DCMat4x4Set as *const u8),
        ("tos_DCSymmetrySet", tos_DCSymmetrySet as *const u8),
        ("tos_DCClipLine", tos_DCClipLine as *const u8),
        ("tos_GrLine3", tos_GrLine3 as *const u8),
        ("tos_GrFillPoly3", tos_GrFillPoly3 as *const u8),
        ("tos_GrBlot", tos_GrBlot as *const u8),
        ("tos_GrPrint", tos_GrPrint as *const u8),
        ("tos_Sprite3", tos_Sprite3 as *const u8),
        ("tos_Sprite3B", tos_Sprite3B as *const u8),
        ("tos_SpriteInterpolate", tos_SpriteInterpolate as *const u8),
        ("tos_SpriteTransform", tos_SpriteTransform as *const u8),
    ]
}

fn identity_matrix() -> *mut i64 {
    tos_runtime::tos_Mat4x4IdentNew()
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_DCNew(
    width: i64,
    height: i64,
    _task: *mut CTask,
    null_bitmap: i64,
) -> *mut CDC {
    let width = width.clamp(0, i32::MAX as i64) as i32;
    let height = height.clamp(0, i32::MAX as i64) as i32;
    let body = if null_bitmap != 0 || width == 0 || height == 0 {
        ptr::null_mut()
    } else {
        let len = (width as usize).saturating_mul(height as usize);
        unsafe { tos_runtime::tos_CAlloc(len.min(i64::MAX as usize) as i64, ptr::null_mut()) }
    };
    Box::into_raw(Box::new(CDC {
        width,
        height,
        flags: 0,
        color: 0,
        r: identity_matrix(),
        x: 0,
        y: 0,
        z: 0,
        thick: 1,
        transform: None,
        body,
        depth_buf: ptr::null_mut(),
        sym_x: 0,
        sym_y: 0,
        sym_z: 0,
        sym_nx: 1.0,
        sym_ny: 0.0,
        sym_nz: 0.0,
        light_x: 37_837,
        light_y: 37_837,
        light_z: 37_837,
        dither_probability_u16: 0,
        owns_body: !body.is_null(),
        owns_depth_buf: false,
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DCAlias(dc: *mut CDC, _task: *mut CTask) -> *mut CDC {
    if dc.is_null() {
        return ptr::null_mut();
    }
    let src = unsafe { &*dc };
    Box::into_raw(Box::new(CDC {
        width: src.width,
        height: src.height,
        flags: src.flags,
        color: src.color,
        r: identity_matrix(),
        x: src.x,
        y: src.y,
        z: src.z,
        thick: src.thick,
        transform: src.transform,
        body: src.body,
        depth_buf: src.depth_buf,
        sym_x: src.sym_x,
        sym_y: src.sym_y,
        sym_z: src.sym_z,
        sym_nx: src.sym_nx,
        sym_ny: src.sym_ny,
        sym_nz: src.sym_nz,
        light_x: src.light_x,
        light_y: src.light_y,
        light_z: src.light_z,
        dither_probability_u16: src.dither_probability_u16,
        owns_body: false,
        owns_depth_buf: false,
    }))
}

#[unsafe(no_mangle)]
/// Release a device-context header and all buffers owned by it.
///
/// # Safety
///
/// `dc` must be null or a live pointer returned by `DCNew` or `DCAlias`, and
/// it must not be used again after this call.
pub unsafe extern "C" fn tos_DCDel(dc: *mut CDC) {
    if dc.is_null() {
        return;
    }
    let dc = unsafe { Box::from_raw(dc) };
    unsafe {
        tos_runtime::tos_Free(dc.r.cast());
        if dc.owns_body {
            tos_runtime::tos_Free(dc.body);
        }
        if dc.owns_depth_buf {
            tos_runtime::tos_Free(dc.depth_buf.cast());
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DCFill(dc: *mut CDC, color: i64) {
    if dc.is_null() {
        return;
    }
    let dc = unsafe { &mut *dc };
    if !dc.body.is_null() {
        unsafe {
            ptr::write_bytes(
                dc.body,
                color as u8,
                (dc.width as usize).saturating_mul(dc.height as usize),
            )
        };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DCDepthBufAlloc(dc: *mut CDC) -> *mut i32 {
    if dc.is_null() {
        return ptr::null_mut();
    }
    let dc = unsafe { &mut *dc };
    let len = (dc.width as usize).saturating_mul(dc.height as usize);
    if dc.owns_depth_buf {
        unsafe { tos_runtime::tos_Free(dc.depth_buf.cast()) };
    }
    let byte_len = len.saturating_mul(size_of::<i32>());
    dc.depth_buf =
        unsafe { tos_runtime::tos_MAlloc(byte_len.min(i64::MAX as usize) as i64, ptr::null_mut()) }
            .cast();
    dc.owns_depth_buf = !dc.depth_buf.is_null();
    unsafe { tos_DCDepthBufRst(dc) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DCDepthBufRst(dc: *mut CDC) -> *mut i32 {
    if dc.is_null() {
        return ptr::null_mut();
    }
    let dc = unsafe { &mut *dc };
    if !dc.depth_buf.is_null() {
        unsafe {
            for value in slice::from_raw_parts_mut(
                dc.depth_buf,
                (dc.width as usize).saturating_mul(dc.height as usize),
            ) {
                *value = i32::MAX;
            }
        }
    }
    dc.depth_buf
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DCMat4x4Set(dc: *mut CDC, r: *mut i64) {
    if let Some(dc) = unsafe { dc.as_mut() } {
        dc.r = r;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DCSymmetrySet(
    dc: *mut CDC,
    x1: i64,
    y1: i64,
    x2: i64,
    y2: i64,
) -> i64 {
    if dc.is_null() || (x1 == x2 && y1 == y2) {
        return 0;
    }
    let nx = (y2 - y1) as f64;
    let ny = (x1 - x2) as f64;
    let magnitude = nx.hypot(ny);
    if magnitude == 0.0 {
        return 0;
    }
    unsafe {
        (*dc).sym_x = x1.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        (*dc).sym_y = y1.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        (*dc).sym_z = 0;
        (*dc).sym_nx = nx / magnitude;
        (*dc).sym_ny = ny / magnitude;
        (*dc).sym_nz = 0.0;
    }
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_DCClipLine(
    dc: *mut CDC,
    x1: *mut i64,
    y1: *mut i64,
    x2: *mut i64,
    y2: *mut i64,
    _line_width: i64,
    _line_height: i64,
) -> i64 {
    if dc.is_null() || x1.is_null() || y1.is_null() || x2.is_null() || y2.is_null() {
        return 0;
    }
    let dc = unsafe { &*dc };
    if dc.width <= 0 || dc.height <= 0 {
        return 0;
    }
    let xmax = dc.width as i64 - 1;
    let ymax = dc.height as i64 - 1;
    unsafe {
        *x1 = (*x1).clamp(0, xmax);
        *x2 = (*x2).clamp(0, xmax);
        *y1 = (*y1).clamp(0, ymax);
        *y2 = (*y2).clamp(0, ymax);
    }
    1
}

unsafe fn plot(dc: *mut CDC, x: i64, y: i64, z: i64) -> bool {
    if dc.is_null() {
        return false;
    }
    let dc = unsafe { &mut *dc };
    if x < 0 || y < 0 || x >= dc.width as i64 || y >= dc.height as i64 || dc.body.is_null() {
        return false;
    }
    let index = y as usize * dc.width as usize + x as usize;
    if !dc.depth_buf.is_null() {
        let depth = unsafe { &mut *dc.depth_buf.add(index) };
        if z > *depth as i64 {
            return false;
        }
        *depth = z as i32;
    }
    let pixel = unsafe { &mut *dc.body.add(index) };
    let color = if dc.color & ROPF_PROBABILITY_DITHER != 0 {
        // TempleOS samples RandU16 here. A coordinate hash gives the same
        // probability distribution without flickering between host frames.
        let mut hash = (x as u64).wrapping_mul(0x9e37_79b1_85eb_ca87)
            ^ (y as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
            ^ (z as u64).wrapping_mul(0x1656_67b1_9e37_79f9);
        hash ^= hash >> 30;
        hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        hash ^= hash >> 27;
        let sample = (hash ^ (hash >> 31)) as u16;
        if u32::from(sample) < dc.dither_probability_u16.min(65_536) {
            ((dc.color >> 16) & 0x0f) as u8
        } else {
            (dc.color & 0x0f) as u8
        }
    } else {
        (dc.color & 0x0f) as u8
    };
    let changed = *pixel != color;
    *pixel = color;
    changed
}

unsafe fn reflect(dc: *const CDC, x: &mut i64, y: &mut i64, z: &mut i64) {
    let ctx = unsafe { &*dc };
    let dx = (*x - i64::from(ctx.sym_x)) as f64;
    let dy = (*y - i64::from(ctx.sym_y)) as f64;
    let dz = (*z - i64::from(ctx.sym_z)) as f64;
    let distance = dx * ctx.sym_nx + dy * ctx.sym_ny + dz * ctx.sym_nz;
    *x = ((*x as f64) - 2.0 * distance * ctx.sym_nx).round() as i64;
    *y = ((*y as f64) - 2.0 * distance * ctx.sym_ny).round() as i64;
    *z = ((*z as f64) - 2.0 * distance * ctx.sym_nz).round() as i64;
}

unsafe fn transformed_point(dc: *mut CDC, point: CD3I32) -> CD3I32 {
    let (mut x, mut y, mut z) = (i64::from(point.x), i64::from(point.y), i64::from(point.z));
    if let Some(ctx) = unsafe { dc.as_ref() }
        && ctx.flags & DCF_TRANSFORMATION != 0
        && let Some(transform) = ctx.transform
    {
        transform(dc, &mut x, &mut y, &mut z);
    }
    CD3I32 {
        x: x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        y: y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        z: z.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
    }
}

unsafe fn light_triangle(dc: *mut CDC, points: &[CD3I32; 3], mut color: u32) {
    let Some(ctx) = (unsafe { dc.as_mut() }) else {
        return;
    };
    let v1 = (
        (i64::from(points[0].x) - i64::from(points[1].x)) as f64,
        (i64::from(points[0].y) - i64::from(points[1].y)) as f64,
        (i64::from(points[0].z) - i64::from(points[1].z)) as f64,
    );
    let v2 = (
        (i64::from(points[2].x) - i64::from(points[1].x)) as f64,
        (i64::from(points[2].y) - i64::from(points[1].y)) as f64,
        (i64::from(points[2].z) - i64::from(points[1].z)) as f64,
    );
    let mut normal = (
        v1.1 * v2.2 - v1.2 * v2.1,
        v1.2 * v2.0 - v1.0 * v2.2,
        v1.0 * v2.1 - v1.1 * v2.0,
    );
    let magnitude = (normal.0 * normal.0 + normal.1 * normal.1 + normal.2 * normal.2).sqrt();
    if magnitude != 0.0 {
        let scale = 65_536.0 / magnitude;
        normal.0 *= scale;
        normal.1 *= scale;
        normal.2 *= scale;
    }
    let mut illumination = ((normal.0 * f64::from(ctx.light_x)
        + normal.1 * f64::from(ctx.light_y)
        + normal.2 * f64::from(ctx.light_z))
        / 65_536.0) as i64;
    if color & ROPF_TWO_SIDED != 0 {
        color &= !ROPF_TWO_SIDED;
        illumination = illumination.abs().saturating_mul(2);
    } else {
        illumination = illumination.saturating_add(65_536);
    }
    let mut base = color & 0x0f;
    if color & ROPF_HALF_RANGE_COLOR != 0 {
        illumination >>= 1;
        if base >= 8 {
            base -= 8;
            illumination = illumination.saturating_add(65_536);
        }
    }
    if illumination < 65_536 {
        ctx.color = ROPF_PROBABILITY_DITHER | (base << 16);
        ctx.dither_probability_u16 = illumination.clamp(0, 65_536) as u32;
    } else {
        ctx.color = ROPF_PROBABILITY_DITHER | ((base ^ 8) << 16) | base;
        ctx.dither_probability_u16 = (illumination - 65_536).clamp(0, 65_536) as u32;
    }
}

unsafe fn plot_brush(dc: *mut CDC, x: i64, y: i64, z: i64) -> i64 {
    let thick = unsafe { (*dc).thick }.clamp(1, 128) as i64;
    if thick == 1 {
        return unsafe { plot(dc, x, y, z) } as i64;
    }
    let radius = thick;
    let half = thick / 2;
    let mut changed = 0;
    for dy in -half..=half {
        for dx in -half..=half {
            if 4 * (dx * dx + dy * dy) <= radius * radius {
                changed += unsafe { plot(dc, x + dx, y + dy, z) } as i64;
            }
        }
    }
    changed
}

unsafe fn raster_line(
    dc: *mut CDC,
    mut x1: i64,
    mut y1: i64,
    z1: i64,
    x2: i64,
    y2: i64,
    z2: i64,
    step: i64,
    start: i64,
) -> i64 {
    let dx = (x2 - x1).abs();
    let sx = if x1 < x2 { 1 } else { -1 };
    let dy = -(y2 - y1).abs();
    let sy = if y1 < y2 { 1 } else { -1 };
    let total = dx.max(-dy).max(1);
    let mut err = dx + dy;
    let mut changed = 0_i64;
    let mut n = 0_i64;
    loop {
        if n >= start && (n - start) % step.max(1) == 0 {
            let z = z1 + (z2 - z1) * n / total;
            changed += unsafe { plot_brush(dc, x1, y1, z) };
        }
        if x1 == x2 && y1 == y2 {
            break;
        }
        let twice = 2 * err;
        if twice >= dy {
            err += dy;
            x1 += sx;
        }
        if twice <= dx {
            err += dx;
            y1 += sy;
        }
        n += 1;
    }
    changed
}

unsafe fn fill_polygon_pixels(dc: *mut CDC, points: &[CD3I32]) -> i64 {
    let Some(ctx) = (unsafe { dc.as_ref() }) else {
        return 0;
    };
    let (width, height) = (ctx.width, ctx.height);
    if width <= 0 || height <= 0 {
        return 0;
    }
    let min_y = points.iter().map(|point| point.y).min().unwrap_or(0).max(0);
    let max_y = points
        .iter()
        .map(|point| point.y)
        .max()
        .unwrap_or(-1)
        .min(height - 1);
    let mut changed = 0;
    for y in min_y..=max_y {
        let mut crossings = Vec::new();
        for index in 0..points.len() {
            let a = points[index];
            let b = points[(index + 1) % points.len()];
            if (a.y <= y && b.y > y) || (b.y <= y && a.y > y) {
                let x = a.x as i64 + (y - a.y) as i64 * (b.x - a.x) as i64 / (b.y - a.y) as i64;
                let z = i64::from(a.z)
                    + ((i128::from(b.z) - i128::from(a.z)) * i128::from(y - a.y)
                        / i128::from(b.y - a.y)) as i64;
                crossings.push((x, z));
            }
        }
        crossings.sort_unstable_by_key(|crossing| crossing.0);
        for pair in crossings.chunks_exact(2) {
            let (left_x, left_z) = pair[0];
            let (right_x, right_z) = pair[1];
            let first_x = left_x.max(0);
            let last_x = right_x.min(i64::from(width - 1));
            for x in first_x..=last_x {
                let z = if right_x == left_x {
                    left_z
                } else {
                    left_z
                        + ((i128::from(right_z) - i128::from(left_z)) * i128::from(x - left_x)
                            / i128::from(right_x - left_x)) as i64
                };
                changed += unsafe { plot(dc, x, i64::from(y), z) } as i64;
            }
        }
    }
    changed
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_GrLine3(
    dc: *mut CDC,
    mut x1: i64,
    mut y1: i64,
    mut z1: i64,
    mut x2: i64,
    mut y2: i64,
    mut z2: i64,
    step: i64,
    start: i64,
) -> i64 {
    if let Some(ctx) = unsafe { dc.as_ref() } {
        if ctx.flags & DCF_TRANSFORMATION != 0 {
            if let Some(transform) = ctx.transform {
                transform(dc, &mut x1, &mut y1, &mut z1);
                transform(dc, &mut x2, &mut y2, &mut z2);
            }
        }
    }
    let mut changed = 0_i64;
    if let Some(flags) = unsafe { dc.as_ref() }.map(|ctx| ctx.flags)
        && flags & DCF_SYMMETRY != 0
    {
        let (mut mx1, mut my1, mut mz1) = (x1, y1, z1);
        let (mut mx2, mut my2, mut mz2) = (x2, y2, z2);
        unsafe {
            reflect(dc, &mut mx1, &mut my1, &mut mz1);
            reflect(dc, &mut mx2, &mut my2, &mut mz2);
            changed += raster_line(dc, mx1, my1, mz1, mx2, my2, mz2, step, start);
        }
        if flags & DCF_JUST_MIRROR != 0 {
            return changed;
        }
    }
    changed + unsafe { raster_line(dc, x1, y1, z1, x2, y2, z2, step, start) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_GrFillPoly3(dc: *mut CDC, n: i64, poly: *const CD3I32) -> i64 {
    if dc.is_null() || poly.is_null() || !(3..=4096).contains(&n) {
        return 0;
    }
    let source = unsafe { slice::from_raw_parts(poly, n as usize) };
    let transformed;
    let points = if let Some(ctx) = unsafe { dc.as_ref() }
        && ctx.flags & DCF_TRANSFORMATION != 0
        && ctx.transform.is_some()
    {
        transformed = source
            .iter()
            .map(|point| unsafe { transformed_point(dc, *point) })
            .collect::<Vec<_>>();
        transformed.as_slice()
    } else {
        source
    };
    let mut changed = 0;
    if let Some(flags) = unsafe { dc.as_ref() }.map(|ctx| ctx.flags)
        && flags & DCF_SYMMETRY != 0
    {
        let mut mirrored = points.to_vec();
        for point in &mut mirrored {
            let (mut x, mut y, mut z) =
                (i64::from(point.x), i64::from(point.y), i64::from(point.z));
            unsafe { reflect(dc, &mut x, &mut y, &mut z) };
            point.x = x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            point.y = y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            point.z = z.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        }
        changed += unsafe { fill_polygon_pixels(dc, &mirrored) };
        if flags & DCF_JUST_MIRROR != 0 {
            return changed;
        }
    }
    changed + unsafe { fill_polygon_pixels(dc, points) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_GrBlot(dc: *mut CDC, x: i64, y: i64, image: *mut CDC) -> i64 {
    if dc.is_null() || image.is_null() {
        return 0;
    }
    let image = unsafe { &*image };
    if image.body.is_null() {
        return 0;
    }
    let mut changed = 0;
    for iy in 0..image.height as i64 {
        for ix in 0..image.width as i64 {
            let color = unsafe {
                *image
                    .body
                    .add(iy as usize * image.width as usize + ix as usize)
            };
            unsafe { (*dc).color = color as u32 };
            changed += unsafe { plot(dc, x + ix, y + iy, 0) } as i64;
        }
    }
    changed
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_GrPrint(
    dc: *mut CDC,
    x: i64,
    y: i64,
    fmt: *const u8,
    argc: i64,
    argv: *const i64,
) -> i64 {
    if dc.is_null() || fmt.is_null() {
        return 0;
    }
    let fmt = unsafe { CStr::from_ptr(fmt.cast()) }.to_bytes();
    let args = if argv.is_null() || argc <= 0 {
        &[][..]
    } else {
        unsafe { slice::from_raw_parts(argv, argc.clamp(0, 1024) as usize) }
    };
    let text = format_graphics_text(fmt, args);
    let mut cursor_x = x;
    let mut cursor_y = y;
    let mut changed = 0;
    for byte in text.bytes() {
        match byte {
            b'\n' => {
                cursor_x = x;
                cursor_y += tos_abi::FONT_HEIGHT;
            }
            b'\t' => {
                let column = ((cursor_x - x) / tos_abi::FONT_WIDTH).max(0);
                cursor_x = x + ((column + 8) & !7) * tos_abi::FONT_WIDTH;
            }
            32..=127 => {
                let glyph = FONT_ASCII[usize::from(byte - 32)];
                for row in 0..8 {
                    let bits = (glyph >> (row * 8)) as u8;
                    for column in 0..8 {
                        if bits & (1 << column) != 0 {
                            changed +=
                                unsafe { plot(dc, cursor_x + column, cursor_y + row as i64, 0) }
                                    as i64;
                        }
                    }
                }
                cursor_x += tos_abi::FONT_WIDTH;
            }
            _ => cursor_x += tos_abi::FONT_WIDTH,
        }
    }
    changed
}

fn pad_formatted(mut value: String, width: usize) -> String {
    if value.len() < width {
        value.insert_str(0, &" ".repeat(width - value.len()));
    }
    value
}

fn format_graphics_text(fmt: &[u8], args: &[i64]) -> String {
    let mut result = String::new();
    let mut index = 0usize;
    let mut arg_index = 0usize;
    while index < fmt.len() {
        if fmt[index] != b'%' {
            result.push(char::from(fmt[index]));
            index += 1;
            continue;
        }
        index += 1;
        if fmt.get(index) == Some(&b'%') {
            result.push('%');
            index += 1;
            continue;
        }
        let mut width = 0usize;
        while let Some(digit) = fmt.get(index).filter(|byte| byte.is_ascii_digit()) {
            width = width.saturating_mul(10) + usize::from(*digit - b'0');
            index += 1;
        }
        let precision = if fmt.get(index) == Some(&b'.') {
            index += 1;
            let mut value = 0usize;
            while let Some(digit) = fmt.get(index).filter(|byte| byte.is_ascii_digit()) {
                value = value.saturating_mul(10) + usize::from(*digit - b'0');
                index += 1;
            }
            Some(value.min(32))
        } else {
            None
        };
        let Some(specifier) = fmt.get(index).copied() else {
            result.push('%');
            break;
        };
        index += 1;
        let argument = *args.get(arg_index).unwrap_or(&0);
        if specifier != b'%' {
            arg_index += 1;
        }
        let formatted = match specifier {
            b'd' | b'i' => argument.to_string(),
            b'u' => (argument as u64).to_string(),
            b'x' => format!("{:x}", argument as u64),
            b'X' => format!("{:X}", argument as u64),
            b'c' => char::from(argument as u8).to_string(),
            b's' if argument != 0 => unsafe {
                CStr::from_ptr(argument as *const i8)
                    .to_string_lossy()
                    .into_owned()
            },
            b's' => String::new(),
            b'f' | b'g' | b'e' => {
                let value = f64::from_bits(argument as u64);
                match (specifier, precision) {
                    (b'e', Some(places)) => format!("{value:.places$e}"),
                    (b'e', None) => format!("{value:e}"),
                    (_, Some(places)) => format!("{value:.places$}"),
                    _ => value.to_string(),
                }
            }
            other => {
                result.push('%');
                char::from(other).to_string()
            }
        };
        result.push_str(&pad_formatted(formatted, width));
    }
    result
}

fn draw_sprite_placeholder(dc: *mut CDC, mut x: i64, mut y: i64, mut z: i64, color: u32) {
    if dc.is_null() {
        return;
    }
    if let Some(ctx) = unsafe { dc.as_ref() }
        && ctx.flags & DCF_TRANSFORMATION != 0
        && let Some(transform) = ctx.transform
    {
        transform(dc, &mut x, &mut y, &mut z);
    }
    let old_color = unsafe { (*dc).color };
    unsafe { (*dc).color = color };
    for offset in -3..=3 {
        unsafe {
            plot(dc, x + offset, y, z);
            plot(dc, x, y + offset, z);
        }
    }
    unsafe { (*dc).color = old_color };
}

const MAX_SPRITE_BYTES: usize = 16 * 1024 * 1024;

unsafe fn sprite_i32(elems: *const u8, offset: usize) -> i32 {
    unsafe { ptr::read_unaligned(elems.add(offset).cast::<i32>()) }
}

unsafe fn sprite_write_i32(elems: *mut u8, offset: usize, value: i32) {
    unsafe { ptr::write_unaligned(elems.add(offset).cast::<i32>(), value) };
}

unsafe fn sprite_element_size(elems: *const u8) -> Option<usize> {
    const BASE: [usize; 30] = [
        1, 2, 3, 5, 17, 1, 1, 9, 9, 13, 17, 5, 17, 25, 13, 25, 29, 5, 5, 5, 5, 9, 9, 17, 9, 21, 17,
        9, 9, 9,
    ];
    let ty = usize::from(unsafe { *elems } & 0x7f);
    let mut size = *BASE.get(ty)?;
    let count = |offset| {
        let value = unsafe { sprite_i32(elems, offset) };
        usize::try_from(value)
            .ok()
            .filter(|value| *value <= 1_000_000)
    };
    match ty {
        9 => size = size.checked_add((count(1)?.checked_mul(3)? + 7) >> 3)?,
        11 => size = size.checked_add(count(1)?.checked_mul(8)?)?,
        17..=20 => size = size.checked_add(count(1)?.checked_mul(12)?)?,
        23 => {
            let width = count(9)?;
            let height = count(13)?;
            size = size.checked_add(((width + 7) & !7).checked_mul(height)?)?;
        }
        24 => {
            size = size.checked_add(count(1)?.checked_mul(12)?)?;
            size = size.checked_add(count(5)?.checked_mul(16)?)?;
        }
        25 => {
            size = size.checked_add(count(13)?.checked_mul(12)?)?;
            size = size.checked_add(count(17)?.checked_mul(16)?)?;
        }
        27..=29 => {
            let mut len = 0usize;
            while len < 65_536 && unsafe { *elems.add(9 + len) } != 0 {
                len += 1;
            }
            if len == 65_536 {
                return None;
            }
            size = size.checked_add(len + 1)?;
        }
        _ => {}
    }
    (size <= MAX_SPRITE_BYTES).then_some(size)
}

unsafe fn sprite_len(elems: *const u8) -> Option<usize> {
    if elems.is_null() || elems as usize <= 4096 {
        return None;
    }
    let mut total = 0usize;
    for _ in 0..65_536 {
        let current = unsafe { elems.add(total) };
        if unsafe { *current } & 0x7f == 0 {
            return Some(total + 1);
        }
        total = total.checked_add(unsafe { sprite_element_size(current) }?)?;
        if total >= MAX_SPRITE_BYTES {
            return None;
        }
    }
    None
}

unsafe fn sprite_plot(dc: *mut CDC, mut x: i64, mut y: i64, mut z: i64) -> bool {
    if let Some(ctx) = unsafe { dc.as_ref() }
        && ctx.flags & DCF_TRANSFORMATION != 0
        && let Some(transform) = ctx.transform
    {
        transform(dc, &mut x, &mut y, &mut z);
    }
    let mut changed = 0;
    if let Some(flags) = unsafe { dc.as_ref() }.map(|ctx| ctx.flags)
        && flags & DCF_SYMMETRY != 0
    {
        let (mut mx, mut my, mut mz) = (x, y, z);
        unsafe { reflect(dc, &mut mx, &mut my, &mut mz) };
        changed += unsafe { plot_brush(dc, mx, my, mz) };
        if flags & DCF_JUST_MIRROR != 0 {
            return changed != 0;
        }
    }
    changed + unsafe { plot_brush(dc, x, y, z) } != 0
}

unsafe fn sprite_line(dc: *mut CDC, x1: i64, y1: i64, z1: i64, x2: i64, y2: i64, z2: i64) {
    unsafe { tos_GrLine3(dc, x1, y1, z1, x2, y2, z2, 1, 0) };
}

unsafe fn draw_mesh(dc: *mut CDC, x: i64, y: i64, z: i64, elem: *const u8, shifted: bool) {
    let (base, vertex_count, triangle_count, shift) = if shifted {
        (
            21usize,
            unsafe { sprite_i32(elem, 13) },
            unsafe { sprite_i32(elem, 17) },
            (
                i64::from(unsafe { sprite_i32(elem, 1) }),
                i64::from(unsafe { sprite_i32(elem, 5) }),
                i64::from(unsafe { sprite_i32(elem, 9) }),
            ),
        )
    } else {
        (
            9usize,
            unsafe { sprite_i32(elem, 1) },
            unsafe { sprite_i32(elem, 5) },
            (0, 0, 0),
        )
    };
    if !(0..=65_536).contains(&vertex_count) || !(0..=65_536).contains(&triangle_count) {
        return;
    }
    let vertices = unsafe { elem.add(base) };
    let triangles = unsafe { vertices.add(vertex_count as usize * 12) };
    for triangle in 0..triangle_count as usize {
        let tri = unsafe { triangles.add(triangle * 16) };
        let color = unsafe { sprite_i32(tri, 0) };
        let indices = [
            unsafe { sprite_i32(tri, 4) },
            unsafe { sprite_i32(tri, 8) },
            unsafe { sprite_i32(tri, 12) },
        ];
        if indices
            .iter()
            .any(|index| *index < 0 || *index >= vertex_count)
        {
            continue;
        }
        let mut points = [CD3I32 { x: 0, y: 0, z: 0 }; 3];
        for (point, index) in points.iter_mut().zip(indices) {
            let vertex = unsafe { vertices.add(index as usize * 12) };
            point.x = unsafe { sprite_i32(vertex, 0) }.saturating_add(
                (x + shift.0).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            );
            point.y = unsafe { sprite_i32(vertex, 4) }.saturating_add(
                (y + shift.1).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            );
            point.z = unsafe { sprite_i32(vertex, 8) }.saturating_add(
                (z + shift.2).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            );
        }
        let transformed = points.map(|point| unsafe { transformed_point(dc, point) });
        let flags = unsafe { (*dc).flags };
        if flags & DCF_SYMMETRY != 0 {
            let mut mirrored = transformed;
            for point in &mut mirrored {
                let (mut mx, mut my, mut mz) =
                    (i64::from(point.x), i64::from(point.y), i64::from(point.z));
                unsafe { reflect(dc, &mut mx, &mut my, &mut mz) };
                point.x = mx.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                point.y = my.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                point.z = mz.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            }
            let mirrored_winding = [mirrored[0], mirrored[2], mirrored[1]];
            unsafe {
                light_triangle(dc, &mirrored_winding, color as u32);
                fill_polygon_pixels(dc, &mirrored_winding);
            }
            if flags & DCF_JUST_MIRROR != 0 {
                continue;
            }
        }
        unsafe {
            light_triangle(dc, &transformed, color as u32);
            fill_polygon_pixels(dc, &transformed);
        }
    }
}

unsafe fn draw_sprite(
    dc: *mut CDC,
    mut x: i64,
    mut y: i64,
    z: i64,
    elems: *mut u8,
    just_one: bool,
) -> bool {
    if dc.is_null() || unsafe { sprite_len(elems) }.is_none() {
        return false;
    }
    let old_color = unsafe { (*dc).color };
    let old_thick = unsafe { (*dc).thick };
    let old_flags = unsafe { (*dc).flags };
    let mut offset = 0usize;
    loop {
        let elem = unsafe { elems.add(offset) };
        let ty = unsafe { *elem } & 0x7f;
        if ty == 0 {
            break;
        }
        match ty {
            1 => unsafe { (*dc).color = u32::from(*elem.add(1)) },
            2 => unsafe { (*dc).color = u32::from(*elem.add(1)) },
            3 => unsafe { (*dc).thick = sprite_i32(elem, 1).max(1) },
            5 => unsafe { (*dc).flags |= DCF_TRANSFORMATION },
            6 => unsafe { (*dc).flags &= !DCF_TRANSFORMATION },
            7 => {
                x += i64::from(unsafe { sprite_i32(elem, 1) });
                y += i64::from(unsafe { sprite_i32(elem, 5) });
            }
            8 => {
                unsafe {
                    sprite_plot(
                        dc,
                        x + i64::from(sprite_i32(elem, 1)),
                        y + i64::from(sprite_i32(elem, 5)),
                        z,
                    )
                };
            }
            10 | 12 | 26 => {
                let x1 = x + i64::from(unsafe { sprite_i32(elem, 1) });
                let y1 = y + i64::from(unsafe { sprite_i32(elem, 5) });
                let x2 = x + i64::from(unsafe { sprite_i32(elem, 9) });
                let y2 = y + i64::from(unsafe { sprite_i32(elem, 13) });
                if ty == 12 {
                    unsafe {
                        sprite_line(dc, x1, y1, z, x2, y1, z);
                        sprite_line(dc, x2, y1, z, x2, y2, z);
                        sprite_line(dc, x2, y2, z, x1, y2, z);
                        sprite_line(dc, x1, y2, z, x1, y1, z);
                    }
                } else {
                    unsafe { sprite_line(dc, x1, y1, z, x2, y2, z) };
                }
            }
            11 => {
                let count = unsafe { sprite_i32(elem, 1) }.clamp(0, 65_536) as usize;
                for point in 1..count {
                    let a = unsafe { elem.add(5 + (point - 1) * 8) };
                    let b = unsafe { elem.add(5 + point * 8) };
                    unsafe {
                        sprite_line(
                            dc,
                            x + i64::from(sprite_i32(a, 0)),
                            y + i64::from(sprite_i32(a, 4)),
                            z,
                            x + i64::from(sprite_i32(b, 0)),
                            y + i64::from(sprite_i32(b, 4)),
                            z,
                        )
                    };
                }
            }
            14..=16 => {
                let cx = x + i64::from(unsafe { sprite_i32(elem, 1) });
                let cy = y + i64::from(unsafe { sprite_i32(elem, 5) });
                let (rx, ry, angle, sides) = match ty {
                    14 => {
                        let radius = f64::from(unsafe { sprite_i32(elem, 9) }.abs());
                        (radius, radius, 0.0, 32usize)
                    }
                    15 => (
                        f64::from(unsafe { sprite_i32(elem, 9) }.abs()),
                        f64::from(unsafe { sprite_i32(elem, 13) }.abs()),
                        f64::from_bits(unsafe { ptr::read_unaligned(elem.add(17).cast::<u64>()) }),
                        32,
                    ),
                    _ => (
                        f64::from(unsafe { sprite_i32(elem, 9) }.abs()),
                        f64::from(unsafe { sprite_i32(elem, 13) }.abs()),
                        f64::from_bits(unsafe { ptr::read_unaligned(elem.add(17).cast::<u64>()) }),
                        unsafe { sprite_i32(elem, 25) }.clamp(3, 256) as usize,
                    ),
                };
                let mut previous = None;
                for point in 0..=sides {
                    let theta = angle + std::f64::consts::TAU * point as f64 / sides as f64;
                    let current = (
                        cx + (rx * theta.cos()).round() as i64,
                        cy + (ry * theta.sin()).round() as i64,
                    );
                    if let Some((px, py)) = previous {
                        unsafe { sprite_line(dc, px, py, z, current.0, current.1, z) };
                    }
                    previous = Some(current);
                }
            }
            23 => {
                let bx = x + i64::from(unsafe { sprite_i32(elem, 1) });
                let by = y + i64::from(unsafe { sprite_i32(elem, 5) });
                let width = unsafe { sprite_i32(elem, 9) }.clamp(0, 4096) as usize;
                let height = unsafe { sprite_i32(elem, 13) }.clamp(0, 4096) as usize;
                let stride = (width + 7) & !7;
                for iy in 0..height {
                    for ix in 0..width {
                        unsafe { (*dc).color = u32::from(*elem.add(17 + iy * stride + ix)) };
                        unsafe { sprite_plot(dc, bx + ix as i64, by + iy as i64, z) };
                    }
                }
            }
            24 => unsafe { draw_mesh(dc, x, y, z, elem, false) },
            25 => unsafe { draw_mesh(dc, x, y, z, elem, true) },
            _ => {}
        }
        if just_one {
            break;
        }
        let Some(size) = (unsafe { sprite_element_size(elem) }) else {
            break;
        };
        offset += size;
    }
    unsafe {
        (*dc).color = old_color;
        (*dc).thick = old_thick;
        (*dc).flags = (*dc).flags & !DCF_TRANSFORMATION | old_flags & DCF_TRANSFORMATION;
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Sprite3(dc: *mut CDC, x: i64, y: i64, z: i64, elems: *mut u8, just_one: i64) {
    if !elems.is_null() && !unsafe { draw_sprite(dc, x, y, z, elems, just_one != 0) } {
        draw_sprite_placeholder(dc, x, y, z, tos_abi::YELLOW);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Sprite3B(dc: *mut CDC, x: i64, y: i64, z: i64, elems: *mut u8) {
    if elems.is_null() || dc.is_null() {
        return;
    }
    let (old_x, old_y, old_z, old_transform) = unsafe {
        let ctx = &mut *dc;
        let old = (ctx.x, ctx.y, ctx.z, ctx.flags & DCF_TRANSFORMATION);
        ctx.x = x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        ctx.y = y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        ctx.z = z.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        ctx.flags |= DCF_TRANSFORMATION;
        old
    };
    if !unsafe { draw_sprite(dc, 0, 0, 0, elems, false) } {
        draw_sprite_placeholder(dc, x, y, z, tos_abi::LTGRAY);
    }
    unsafe {
        (*dc).x = old_x;
        (*dc).y = old_y;
        (*dc).z = old_z;
        (*dc).flags = (*dc).flags & !DCF_TRANSFORMATION | old_transform;
    }
}

unsafe fn owned_sprite_copy(source: *const u8) -> *mut u8 {
    let Some(len) = (unsafe { sprite_len(source) }) else {
        return ptr::null_mut();
    };
    let result = unsafe { tos_runtime::tos_CAlloc(len as i64, ptr::null_mut()) };
    if !result.is_null() {
        unsafe { ptr::copy_nonoverlapping(source, result, len) };
    }
    result
}

unsafe fn mutate_sprite_points(
    elems: *mut u8,
    mut transform: impl FnMut(&mut i64, &mut i64, &mut i64),
) {
    let Some(_) = (unsafe { sprite_len(elems) }) else {
        return;
    };
    let mut offset = 0usize;
    loop {
        let elem = unsafe { elems.add(offset) };
        let ty = unsafe { *elem } & 0x7f;
        if ty == 0 {
            break;
        }
        let (base, count) = match ty {
            24 => (9usize, unsafe { sprite_i32(elem, 1) }.max(0) as usize),
            25 => (21usize, unsafe { sprite_i32(elem, 13) }.max(0) as usize),
            _ => (0, 0),
        };
        for point in 0..count.min(65_536) {
            let point = unsafe { elem.add(base + point * 12) };
            let (mut x, mut y, mut z) = (
                i64::from(unsafe { sprite_i32(point, 0) }),
                i64::from(unsafe { sprite_i32(point, 4) }),
                i64::from(unsafe { sprite_i32(point, 8) }),
            );
            transform(&mut x, &mut y, &mut z);
            unsafe {
                sprite_write_i32(
                    point,
                    0,
                    x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
                );
                sprite_write_i32(
                    point,
                    4,
                    y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
                );
                sprite_write_i32(
                    point,
                    8,
                    z.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
                );
            }
        }
        let Some(size) = (unsafe { sprite_element_size(elem) }) else {
            break;
        };
        offset += size;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_SpriteInterpolate(t: f64, a: *mut u8, b: *mut u8) -> *mut u8 {
    let result = unsafe { owned_sprite_copy(a) };
    if result.is_null() || unsafe { sprite_len(a) } != unsafe { sprite_len(b) } {
        return result;
    }
    unsafe {
        let mut ao = 0usize;
        let mut bo = 0usize;
        let mut ro = 0usize;
        loop {
            let ae = a.add(ao);
            let be = b.add(bo);
            let re = result.add(ro);
            let ty = *ae & 0x7f;
            if ty == 0 || ty != (*be & 0x7f) {
                break;
            }
            let (base, count) = match ty {
                24 => (9usize, sprite_i32(ae, 1).max(0) as usize),
                25 => (21usize, sprite_i32(ae, 13).max(0) as usize),
                _ => (0, 0),
            };
            for point in 0..count.min(65_536) {
                for component in 0..3 {
                    let at = sprite_i32(ae, base + point * 12 + component * 4);
                    let bt = sprite_i32(be, base + point * 12 + component * 4);
                    let value = f64::from(at) + (f64::from(bt) - f64::from(at)) * t;
                    sprite_write_i32(
                        re,
                        base + point * 12 + component * 4,
                        value
                            .round()
                            .clamp(f64::from(i32::MIN), f64::from(i32::MAX))
                            as i32,
                    );
                }
            }
            let Some(size) = sprite_element_size(ae) else {
                break;
            };
            ao += size;
            bo += size;
            ro += size;
        }
    }
    result
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_SpriteTransform(elems: *mut u8, r: *mut i64) -> *mut u8 {
    let result = unsafe { owned_sprite_copy(elems) };
    if result.is_null() || r.is_null() {
        return result;
    }
    unsafe {
        mutate_sprite_points(result, |x, y, z| tos_runtime::tos_Mat4x4MulXYZ(r, x, y, z));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn translate(_dc: *mut CDC, x: *mut i64, y: *mut i64, _z: *mut i64) {
        unsafe {
            *x += 2;
            *y += 2;
        }
    }

    #[test]
    fn deletes_alias_without_freeing_shared_buffers() {
        let dc = tos_DCNew(8, 8, ptr::null_mut(), 0);
        unsafe {
            let depth = tos_DCDepthBufAlloc(dc);
            let alias = tos_DCAlias(dc, ptr::null_mut());
            assert!(!(*alias).owns_body);
            assert!(!(*alias).owns_depth_buf);
            assert_ne!((*alias).r, (*dc).r);

            tos_DCDel(alias);
            tos_DCFill(dc, i64::from(tos_abi::LTRED));
            assert_eq!(*(*dc).body, tos_abi::LTRED as u8);
            assert_eq!((*dc).depth_buf, depth);
            assert_eq!(*tos_DCDepthBufRst(dc), i32::MAX);
            tos_DCDel(dc);
        }
    }

    #[test]
    fn draws_line_into_16_color_bitmap() {
        let dc = tos_DCNew(8, 8, ptr::null_mut(), 0);
        unsafe {
            (*dc).color = 4;
            assert_eq!(tos_GrLine3(dc, 0, 0, 0, 7, 7, 0, 1, 0), 8);
            assert_eq!(*(*dc).body, 4);
            assert_eq!(*(*dc).body.add(63), 4);
        }
    }

    #[test]
    fn transforms_polygon_vertices_before_rasterizing() {
        let dc = tos_DCNew(8, 8, ptr::null_mut(), 0);
        let triangle = [
            CD3I32 { x: 0, y: 0, z: 0 },
            CD3I32 { x: 2, y: 0, z: 0 },
            CD3I32 { x: 0, y: 2, z: 0 },
        ];
        unsafe {
            (*dc).color = 10;
            (*dc).flags |= DCF_TRANSFORMATION;
            (*dc).transform = Some(translate);
            assert!(tos_GrFillPoly3(dc, triangle.len() as i64, triangle.as_ptr()) > 0);
            assert_eq!(*(*dc).body.add(2 * 8 + 2), 10);
            assert_eq!(*(*dc).body, 0);
        }
    }

    #[test]
    fn unresolved_sprite_handles_render_and_clone_safely() {
        let dc = tos_DCNew(8, 8, ptr::null_mut(), 0);
        let opaque = 2_usize as *mut u8;
        tos_Sprite3(dc, 4, 4, 0, opaque, 0);
        unsafe {
            assert_eq!(*(*dc).body.add(4 * 8 + 4), tos_abi::YELLOW as u8);
        }

        let interpolated = tos_SpriteInterpolate(0.25, opaque, 3_usize as *mut u8);
        assert!(interpolated.is_null());
    }

    #[test]
    fn decodes_line_and_triangle_mesh_sprites() {
        let dc = tos_DCNew(16, 16, ptr::null_mut(), 0);
        let mut line = vec![1, tos_abi::LTRED as u8, 10];
        for value in [1_i32, 2, 8, 2] {
            line.extend_from_slice(&value.to_le_bytes());
        }
        line.push(0);
        tos_Sprite3(dc, 0, 0, 0, line.as_mut_ptr(), 0);
        unsafe { assert_eq!(*(*dc).body.add(2 * 16 + 4), tos_abi::LTRED as u8) };

        let mut mesh = vec![24];
        mesh.extend_from_slice(&3_i32.to_le_bytes());
        mesh.extend_from_slice(&1_i32.to_le_bytes());
        for (x, y, z) in [(2_i32, 4_i32, 0_i32), (10, 4, 0), (2, 12, 0)] {
            mesh.extend_from_slice(&x.to_le_bytes());
            mesh.extend_from_slice(&y.to_le_bytes());
            mesh.extend_from_slice(&z.to_le_bytes());
        }
        for value in [tos_abi::LTGREEN as i32, 0, 1, 2] {
            mesh.extend_from_slice(&value.to_le_bytes());
        }
        mesh.push(0);
        tos_Sprite3(dc, 0, 0, 0, mesh.as_mut_ptr(), 0);
        unsafe { assert_eq!(*(*dc).body.add(6 * 16 + 3), tos_abi::LTGREEN as u8) };
    }

    #[test]
    fn lights_mesh_colors_with_probability_dithering() {
        let dc = tos_DCNew(32, 32, ptr::null_mut(), 0);
        let triangle = [
            CD3I32 { x: 2, y: 2, z: 0 },
            CD3I32 { x: 28, y: 2, z: 0 },
            CD3I32 { x: 2, y: 28, z: 0 },
        ];
        unsafe {
            light_triangle(
                dc,
                &triangle,
                ROPF_TWO_SIDED | ROPF_HALF_RANGE_COLOR | tos_abi::LTGREEN,
            );
            assert_eq!(
                (*dc).color,
                ROPF_PROBABILITY_DITHER | (tos_abi::LTGREEN << 16) | tos_abi::GREEN
            );
            assert!((1..65_536).contains(&(*dc).dither_probability_u16));
            fill_polygon_pixels(dc, &triangle);
            let pixels = slice::from_raw_parts((*dc).body, 32 * 32);
            assert!(pixels.contains(&(tos_abi::GREEN as u8)));
            assert!(pixels.contains(&(tos_abi::LTGREEN as u8)));
        }
    }

    #[test]
    fn renders_thick_and_mirrored_lines() {
        let dc = tos_DCNew(20, 12, ptr::null_mut(), 0);
        unsafe {
            (*dc).color = tos_abi::YELLOW;
            (*dc).thick = 3;
            tos_GrLine3(dc, 2, 5, 0, 5, 5, 0, 1, 0);
            assert_eq!(*(*dc).body.add(4 * 20 + 3), tos_abi::YELLOW as u8);

            (*dc).thick = 1;
            (*dc).flags |= DCF_SYMMETRY;
            assert_eq!(tos_DCSymmetrySet(dc, 10, 0, 10, 1), 1);
            tos_GrLine3(dc, 2, 2, 0, 5, 2, 0, 1, 0);
            assert_eq!(*(*dc).body.add(2 * 20 + 15), tos_abi::YELLOW as u8);
        }
    }

    #[test]
    fn formats_and_draws_hud_text() {
        let args = [1.5_f64.to_bits() as i64, 7];
        assert_eq!(
            format_graphics_text(b"Pitch:%5.1f Fish:%d", &args),
            "Pitch:  1.5 Fish:7"
        );

        let dc = tos_DCNew(32, 16, ptr::null_mut(), 0);
        let text = b"HUD\0";
        unsafe {
            (*dc).color = tos_abi::WHITE;
            assert!(tos_GrPrint(dc, 0, 0, text.as_ptr(), 0, ptr::null()) > 0);
            assert!(
                slice::from_raw_parts((*dc).body, 32 * 16)
                    .iter()
                    .any(|pixel| *pixel == tos_abi::WHITE as u8)
            );
        }
    }

    #[test]
    fn interpolates_polygon_depth_across_scanlines() {
        let dc = tos_DCNew(8, 8, ptr::null_mut(), 0);
        let triangle = [
            CD3I32 { x: 0, y: 0, z: 10 },
            CD3I32 { x: 7, y: 0, z: 70 },
            CD3I32 { x: 0, y: 7, z: 70 },
        ];
        unsafe {
            tos_DCDepthBufAlloc(dc);
            (*dc).color = tos_abi::LTBLUE;
            tos_GrFillPoly3(dc, 3, triangle.as_ptr());
            let depth = *(*dc).depth_buf.add(8 + 1);
            assert!(depth > 10 && depth < 70, "interpolated depth was {depth}");
        }
    }
}

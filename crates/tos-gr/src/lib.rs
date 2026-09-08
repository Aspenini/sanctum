//! TempleOS-compatible 16-color software graphics primitives.

use std::ffi::CStr;
use std::ptr;
use std::slice;
use tos_abi::{CD3I32, CDC, CTask};

const DCF_TRANSFORMATION: i32 = 0x100;

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
    let mut r = Box::new([0_i64; 16]);
    for i in [0, 5, 10, 15] {
        r[i] = 1_i64 << 32;
    }
    Box::into_raw(r).cast::<i64>()
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
        Box::into_raw(vec![0_u8; len].into_boxed_slice()).cast::<u8>()
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
    }))
}

/// Device contexts intentionally live for the JIT program lifetime for now.
#[unsafe(no_mangle)]
pub extern "C" fn tos_DCDel(_dc: *mut CDC) {}

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
    dc.depth_buf = Box::into_raw(vec![i32::MAX; len].into_boxed_slice()).cast::<i32>();
    dc.depth_buf
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
pub extern "C" fn tos_DCSymmetrySet(_dc: *mut CDC, _x1: i64, _y1: i64, _x2: i64, _y2: i64) -> i64 {
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
    let color = (dc.color & 0x0f) as u8;
    let changed = *pixel != color;
    *pixel = color;
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
            changed += unsafe { plot(dc, x1, y1, z) } as i64;
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tos_GrFillPoly3(dc: *mut CDC, n: i64, poly: *const CD3I32) -> i64 {
    if dc.is_null() || poly.is_null() || !(3..=4096).contains(&n) {
        return 0;
    }
    let source = unsafe { slice::from_raw_parts(poly, n as usize) };
    let mut transformed;
    let points = if let Some(ctx) = unsafe { dc.as_ref() }
        && ctx.flags & DCF_TRANSFORMATION != 0
        && let Some(transform) = ctx.transform
    {
        transformed = Vec::with_capacity(source.len());
        for point in source {
            let (mut x, mut y, mut z) =
                (i64::from(point.x), i64::from(point.y), i64::from(point.z));
            transform(dc, &mut x, &mut y, &mut z);
            transformed.push(CD3I32 {
                x: x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
                y: y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
                z: z.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            });
        }
        transformed.as_slice()
    } else {
        source
    };
    let min_y = points.iter().map(|p| p.y).min().unwrap_or(0);
    let max_y = points.iter().map(|p| p.y).max().unwrap_or(-1);
    let mut changed = 0;
    for y in min_y..=max_y {
        let mut crossings = Vec::new();
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            if (a.y <= y && b.y > y) || (b.y <= y && a.y > y) {
                let x = a.x as i64 + (y - a.y) as i64 * (b.x - a.x) as i64 / (b.y - a.y) as i64;
                crossings.push(x);
            }
        }
        crossings.sort_unstable();
        for pair in crossings.chunks_exact(2) {
            for x in pair[0]..=pair[1] {
                changed += unsafe { plot(dc, x, y as i64, points[0].z as i64) } as i64;
            }
        }
    }
    changed
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
    _dc: *mut CDC,
    _x: i64,
    _y: i64,
    fmt: *const u8,
    _argc: i64,
    _argv: *const i64,
) -> i64 {
    if fmt.is_null() {
        0
    } else {
        unsafe { CStr::from_ptr(fmt.cast()) }.to_bytes().len() as i64
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Sprite3(
    _dc: *mut CDC,
    _x: i64,
    _y: i64,
    _z: i64,
    _elems: *mut u8,
    _just_one: i64,
) {
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Sprite3B(_dc: *mut CDC, _x: i64, _y: i64, _z: i64, _elems: *mut u8) {}

#[unsafe(no_mangle)]
pub extern "C" fn tos_SpriteInterpolate(t: f64, a: *mut u8, b: *mut u8) -> *mut u8 {
    if t < 0.5 { a } else { b }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_SpriteTransform(elems: *mut u8, _r: *mut i64) -> *mut u8 {
    elems
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
}

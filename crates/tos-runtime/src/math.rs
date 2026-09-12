use tos_abi::CD3;

const FIXED_ONE: i64 = 1_i64 << 32;

fn fixed(value: f64) -> i64 {
    (value * FIXED_ONE as f64).round() as i64
}

fn fixed_mul(lhs: i64, rhs: i64) -> i64 {
    ((i128::from(lhs) * i128::from(rhs)) >> 32) as i64
}

unsafe fn matrix_mut<'a>(ptr: *mut i64) -> Option<&'a mut [i64; 16]> {
    unsafe { ptr.cast::<[i64; 16]>().as_mut() }
}

fn premultiply(target: &mut [i64; 16], lhs: &[i64; 16]) {
    let rhs = *target;
    for row in 0..4 {
        for col in 0..4 {
            let mut sum = 0_i128;
            for k in 0..4 {
                sum += i128::from(lhs[row * 4 + k]) * i128::from(rhs[k * 4 + col]);
            }
            target[row * 4 + col] = (sum >> 32) as i64;
        }
    }
}

pub unsafe fn mat_identity(ptr: *mut i64) -> *mut i64 {
    let Some(matrix) = (unsafe { matrix_mut(ptr) }) else {
        return ptr;
    };
    matrix.fill(0);
    for index in 0..4 {
        matrix[index * 4 + index] = FIXED_ONE;
    }
    ptr
}

pub unsafe fn mat_rotate_x(ptr: *mut i64, angle: f64) -> *mut i64 {
    let Some(matrix) = (unsafe { matrix_mut(ptr) }) else {
        return ptr;
    };
    let (sin, cos) = angle.sin_cos();
    let mut rotation = [0; 16];
    rotation[0] = FIXED_ONE;
    rotation[5] = fixed(cos);
    rotation[6] = fixed(-sin);
    rotation[9] = fixed(sin);
    rotation[10] = fixed(cos);
    rotation[15] = FIXED_ONE;
    // TempleOS applies each new rotation before the existing transform.
    premultiply(matrix, &rotation);
    ptr
}

pub unsafe fn mat_rotate_z(ptr: *mut i64, angle: f64) -> *mut i64 {
    let Some(matrix) = (unsafe { matrix_mut(ptr) }) else {
        return ptr;
    };
    let (sin, cos) = angle.sin_cos();
    let mut rotation = [0; 16];
    rotation[0] = fixed(cos);
    rotation[1] = fixed(-sin);
    rotation[4] = fixed(sin);
    rotation[5] = fixed(cos);
    rotation[10] = FIXED_ONE;
    rotation[15] = FIXED_ONE;
    premultiply(matrix, &rotation);
    ptr
}

pub unsafe fn mat_translate(ptr: *mut i64, x: i64, y: i64, z: i64) -> *mut i64 {
    let Some(matrix) = (unsafe { matrix_mut(ptr) }) else {
        return ptr;
    };
    // `Mat4x4TranslationEqu` assigns the translation column. It does not
    // compose a translation matrix (and therefore must not rotate it).
    matrix[3] = x.wrapping_shl(32);
    matrix[7] = y.wrapping_shl(32);
    matrix[11] = z.wrapping_shl(32);
    matrix[15] = FIXED_ONE;
    ptr
}

pub unsafe fn mat_scale(ptr: *mut i64, scale: f64) -> *mut i64 {
    let Some(matrix) = (unsafe { matrix_mut(ptr) }) else {
        return ptr;
    };
    // TempleOS scales every element, including the translation column.
    for value in matrix {
        *value = (*value as f64 * scale) as i64;
    }
    ptr
}

pub unsafe fn mat_mul_xyz(matrix: *const i64, x: *mut i64, y: *mut i64, z: *mut i64) {
    if matrix.is_null() || x.is_null() || y.is_null() || z.is_null() {
        return;
    }
    let matrix = unsafe { &*matrix.cast::<[i64; 16]>() };
    let (old_x, old_y, old_z) = unsafe { (*x, *y, *z) };
    let transform = |row: usize| {
        fixed_mul(matrix[row * 4], old_x)
            .wrapping_add(fixed_mul(matrix[row * 4 + 1], old_y))
            .wrapping_add(fixed_mul(matrix[row * 4 + 2], old_z))
            .wrapping_add(matrix[row * 4 + 3] >> 32)
    };
    unsafe {
        *x = transform(0);
        *y = transform(1);
        *z = transform(2);
    }
}

pub unsafe fn d3_sub(dst: *mut CD3, lhs: *const CD3, rhs: *const CD3) -> *mut CD3 {
    if dst.is_null() || lhs.is_null() || rhs.is_null() {
        return dst;
    }
    unsafe {
        (*dst).x = (*lhs).x - (*rhs).x;
        (*dst).y = (*lhs).y - (*rhs).y;
        (*dst).z = (*lhs).z - (*rhs).z;
    }
    dst
}

pub unsafe fn d3_norm_sqr(value: *const CD3) -> f64 {
    if value.is_null() {
        return 0.0;
    }
    let value = unsafe { &*value };
    value.x * value.x + value.y * value.y + value.z * value.z
}

pub unsafe fn d3_unit(value: *mut CD3) -> *mut CD3 {
    let Some(value_ref) = (unsafe { value.as_mut() }) else {
        return value;
    };
    let norm =
        (value_ref.x * value_ref.x + value_ref.y * value_ref.y + value_ref.z * value_ref.z).sqrt();
    if norm != 0.0 {
        value_ref.x /= norm;
        value_ref.y /= norm;
        value_ref.z /= norm;
    }
    value
}

pub unsafe fn swap_i64(lhs: *mut i64, rhs: *mut i64) {
    if !lhs.is_null() && !rhs.is_null() {
        unsafe { std::ptr::swap(lhs, rhs) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_z_rotation_transform_points() {
        let mut matrix = [0_i64; 16];
        unsafe {
            mat_identity(matrix.as_mut_ptr());
            mat_rotate_z(matrix.as_mut_ptr(), std::f64::consts::FRAC_PI_2);
        }
        let (mut x, mut y, mut z) = (10, 0, 0);
        unsafe { mat_mul_xyz(matrix.as_ptr(), &mut x, &mut y, &mut z) };
        assert!(x.abs() <= 1);
        assert_eq!(y, 10);
        assert_eq!(z, 0);
    }

    #[test]
    fn rotations_are_composed_in_templeos_order() {
        let mut matrix = [0_i64; 16];
        unsafe {
            mat_identity(matrix.as_mut_ptr());
            mat_rotate_z(matrix.as_mut_ptr(), std::f64::consts::FRAC_PI_2);
            mat_rotate_x(matrix.as_mut_ptr(), std::f64::consts::FRAC_PI_2);
        }
        let (mut x, mut y, mut z) = (10, 0, 0);
        unsafe { mat_mul_xyz(matrix.as_ptr(), &mut x, &mut y, &mut z) };
        assert!(x.abs() <= 1);
        assert!(y.abs() <= 1);
        assert_eq!(z, 10);
    }

    #[test]
    fn translation_equ_is_not_rotated_by_the_existing_matrix() {
        let mut matrix = [0_i64; 16];
        unsafe {
            mat_identity(matrix.as_mut_ptr());
            mat_rotate_z(matrix.as_mut_ptr(), std::f64::consts::FRAC_PI_2);
            mat_translate(matrix.as_mut_ptr(), 10, 20, 30);
        }
        let (mut x, mut y, mut z) = (0, 0, 0);
        unsafe { mat_mul_xyz(matrix.as_ptr(), &mut x, &mut y, &mut z) };
        assert_eq!((x, y, z), (10, 20, 30));
    }

    #[test]
    fn scaling_includes_translation_like_templeos() {
        let mut matrix = [0_i64; 16];
        unsafe {
            mat_identity(matrix.as_mut_ptr());
            mat_translate(matrix.as_mut_ptr(), 10, 20, 30);
            mat_scale(matrix.as_mut_ptr(), 0.5);
        }
        let (mut x, mut y, mut z) = (0, 0, 0);
        unsafe { mat_mul_xyz(matrix.as_ptr(), &mut x, &mut y, &mut z) };
        assert_eq!((x, y, z), (5, 10, 15));
    }
}

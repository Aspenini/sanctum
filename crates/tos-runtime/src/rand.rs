use std::cell::Cell;

thread_local! {
    static STATE: Cell<u64> = const { Cell::new(0xC0FFEE_u64.wrapping_mul(0x9E37_79B9_7F4A_7C15)) };
}

fn next() -> u64 {
    STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            x = 0xC0FFEE;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}

pub fn rand_f64() -> f64 {
    (next() >> 11) as f64 / ((1u64 << 53) as f64)
}

pub fn rand_u16() -> i64 {
    (next() as u16) as i64
}

pub fn rand_u32() -> i64 {
    (next() as u32) as i64
}

pub fn rand_i16() -> i64 {
    next() as i16 as i64
}

pub fn rand_i64() -> i64 {
    next() as i64
}

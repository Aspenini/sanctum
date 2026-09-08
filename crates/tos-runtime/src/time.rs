use std::sync::OnceLock;
use std::time::Instant;

static BOOT: OnceLock<Instant> = OnceLock::new();

pub fn boot() {
    let _ = BOOT.set(Instant::now());
}

pub fn ts() -> f64 {
    let t0 = BOOT.get_or_init(Instant::now);
    t0.elapsed().as_secs_f64()
}

#[allow(dead_code)]
pub fn jiffies() -> i64 {
    (ts() * 1000.0) as i64
}

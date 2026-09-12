use holyc::{CompileOptions, compile_file, run_program};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};
use templeos_compat::{HostMode, host};

const COORDINATE_SCALE: i64 = 256;
const MAP_SCALE: i64 = 150;

unsafe fn read_usize(address: usize) -> usize {
    unsafe { std::ptr::read_unaligned(address as *const usize) }
}

unsafe fn read_i64(address: usize) -> i64 {
    unsafe { std::ptr::read_unaligned(address as *const i64) }
}

unsafe fn read_f64(address: usize) -> f64 {
    unsafe { std::ptr::read_unaligned(address as *const f64) }
}

unsafe fn write_i64(address: usize, value: i64) {
    unsafe { std::ptr::write_unaligned(address as *mut i64, value) }
}

#[test]
fn talons_lowers_claws_and_catches_a_fish() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../TempleOS/Demo/Games/Talons.HC");
    if !path.is_file() {
        eprintln!(
            "skipping Talons gameplay test: {} is absent",
            path.display()
        );
        return;
    }

    let mut program = compile_file(&path, &CompileOptions::default()).unwrap();
    let globals = program.global_bindings().unwrap();
    let address = |name: &str| {
        globals
            .iter()
            .find(|global| global.name == name)
            .unwrap_or_else(|| panic!("Talons global `{name}` is missing"))
            .address as usize
    };

    let panel_head = address("panel_head");
    let x = address("x");
    let y = address("y");
    let z = address("z");
    let fish_left = address("fish_left");
    let claws_down = address("claws_down");

    // The JIT globals stay alive for the duration of run_program. This helper
    // observes the real Talons queues, places the aircraft near one fish, and
    // lets AnimateTask perform the approach and catch exactly as normal play.
    let helper = thread::spawn(move || {
        let result = (|| -> Result<(usize, f64, i64), String> {
            let startup_deadline = Instant::now() + Duration::from_secs(20);
            while host::frames_presented() < 20 {
                if Instant::now() >= startup_deadline {
                    return Err("Talons did not render 20 frames".into());
                }
                thread::sleep(Duration::from_millis(10));
            }

            let mut fish = Vec::new();
            unsafe {
                let mut panel = read_usize(panel_head);
                let mut panels_seen = 0;
                while panel != 0 && panels_seen < 2_000_000 {
                    // Panel's embedded object-queue sentinel starts at byte 32.
                    let sentinel = panel + 32;
                    let mut object = read_usize(sentinel);
                    let mut objects_seen = 0;
                    while object != sentinel && object != 0 && objects_seen < 100_000 {
                        // Packed Obj: next, last, CD3I64 p, img, Bool fish.
                        if read_i64(object + 48) != 0 {
                            fish.push((
                                read_i64(object + 16),
                                read_i64(object + 24),
                                read_i64(object + 32),
                            ));
                        }
                        object = read_usize(object);
                        objects_seen += 1;
                    }
                    panel = read_usize(panel);
                    panels_seen += 1;
                }
            }
            let &(fish_x, fish_y, fish_z) = fish
                .first()
                .ok_or_else(|| "Talons generated no fish objects".to_owned())?;

            unsafe {
                // Stay outside catch distance (2*MAP_SCALE), but inside claw
                // approach distance (4*MAP_SCALE).
                write_i64(x, (fish_x + 3 * MAP_SCALE) * COORDINATE_SCALE);
                write_i64(y, fish_y * COORDINATE_SCALE);
                write_i64(z, (fish_z + MAP_SCALE) * COORDINATE_SCALE);
            }

            let approach_deadline = Instant::now() + Duration::from_secs(3);
            let claw_value = loop {
                let value = unsafe { read_f64(claws_down) };
                if value > 0.02 {
                    break value;
                }
                if Instant::now() >= approach_deadline {
                    return Err(format!(
                        "Talons never lowered its claws (last value {value})"
                    ));
                }
                thread::sleep(Duration::from_millis(10));
            };

            unsafe {
                write_i64(x, (fish_x + MAP_SCALE / 2) * COORDINATE_SCALE);
                write_i64(y, fish_y * COORDINATE_SCALE);
                write_i64(z, (fish_z + MAP_SCALE) * COORDINATE_SCALE);
            }

            let catch_deadline = Instant::now() + Duration::from_secs(3);
            let remaining = loop {
                let value = unsafe { read_i64(fish_left) };
                if value < 10 {
                    break value;
                }
                if Instant::now() >= catch_deadline {
                    return Err(format!("Talons did not catch the fish ({value} remaining)"));
                }
                thread::sleep(Duration::from_millis(10));
            };
            Ok((fish.len(), claw_value, remaining))
        })();
        host::push_key_event(0x1b, 0);
        result
    });

    run_program(program, HostMode::External).unwrap();
    let (generated_fish, claw_value, remaining) = helper.join().unwrap().unwrap();
    assert!(generated_fish >= 10, "only generated {generated_fish} fish");
    assert!(claw_value > 0.02);
    assert_eq!(remaining, 9);
}

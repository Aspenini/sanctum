use holyc::{CompileOptions, compile_file, run_program};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};
use templeos_compat::{HostMode, host};

const SCRN_SCALE: i64 = 512;
const DKGRAY: u8 = 8;

unsafe fn read_usize(address: usize) -> usize {
    unsafe { std::ptr::read_unaligned(address as *const usize) }
}

unsafe fn read_i64(address: usize) -> i64 {
    unsafe { std::ptr::read_unaligned(address as *const i64) }
}

unsafe fn write_i64(address: usize, value: i64) {
    unsafe { std::ptr::write_unaligned(address as *mut i64, value) }
}

unsafe fn write_u8(address: usize, value: u8) {
    unsafe { std::ptr::write_unaligned(address as *mut u8, value) }
}

#[test]
fn castle_frankenstein_fires_at_a_monster_in_front() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../TempleOS/Demo/Games/CastleFrankenstein.HC");
    if !path.is_file() {
        eprintln!(
            "skipping Castle Frankenstein gameplay test: {} is absent",
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
            .unwrap_or_else(|| panic!("Castle Frankenstein global `{name}` is missing"))
            .address as usize
    };

    let map = address("map");
    let map_width = address("map_width");
    let man_xx = address("man_xx");
    let man_yy = address("man_yy");
    let monsters = address("monsters");
    let monsters_left = address("monsters_left");

    let helper = thread::spawn(move || {
        let result = (|| -> Result<(i64, i64, i64), String> {
            let startup = Instant::now() + Duration::from_secs(30);
            while host::frames_presented() < 2 {
                if Instant::now() >= startup {
                    return Err("Castle Frankenstein did not present a frame".into());
                }
                thread::sleep(Duration::from_millis(10));
            }

            let width = unsafe { read_i64(map_width) };
            let map_ptr = unsafe { read_usize(map) };
            let xx = unsafe { read_i64(man_xx) };
            let yy = unsafe { read_i64(man_yy) };
            if width < 3 || map_ptr == 0 {
                return Err(format!(
                    "map was not built (width={width}, ptr={map_ptr:#x})"
                ));
            }

            let cell_x = xx / SCRN_SCALE;
            let cell_y = yy / SCRN_SCALE;
            let cell = |x: i64, y: i64| map_ptr + (y * width + x) as usize;
            unsafe {
                write_u8(cell(cell_x, cell_y), DKGRAY);
                write_u8(cell(cell_x + 1, cell_y), DKGRAY);
                // Packed Monster: I64 x, I64 y, Bool dead.
                write_i64(monsters, xx + SCRN_SCALE);
                write_i64(monsters + 8, yy);
                write_u8(monsters + 16, 0);
            }

            host::push_key_event(b' ' as i64, 0x39);

            let fire = Instant::now() + Duration::from_secs(3);
            let remaining = loop {
                let left = unsafe { read_i64(monsters_left) };
                let dead = unsafe { std::ptr::read_unaligned((monsters + 16) as *const u8) };
                if left < 10 || dead != 0 {
                    break left;
                }
                if Instant::now() >= fire {
                    return Err(format!(
                        "Castle Frankenstein did not fire (left={left}, dead={dead})"
                    ));
                }
                thread::sleep(Duration::from_millis(10));
            };
            Ok((width, unsafe { read_i64(monsters_left) }, remaining))
        })();
        host::push_key_event(0x1b, 0);
        result
    });

    run_program(program, HostMode::External).unwrap();
    let (width, left_after, remaining) = helper.join().unwrap().unwrap();
    assert!(width > 2, "map width was {width}");
    assert!(
        remaining < 10 || left_after < 10,
        "monster survived fire (left={left_after}, remaining={remaining})"
    );
}

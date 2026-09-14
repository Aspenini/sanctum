use holyc::{CompileOptions, compile_file, run_program};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};
use templeos_compat::{HostMode, host};

unsafe fn read_i8(address: usize) -> i8 {
    unsafe { std::ptr::read_unaligned(address as *const i8) }
}

unsafe fn read_board(board: usize) -> [i8; 9] {
    unsafe {
        [
            read_i8(board),
            read_i8(board + 1),
            read_i8(board + 2),
            read_i8(board + 3),
            read_i8(board + 4),
            read_i8(board + 5),
            read_i8(board + 6),
            read_i8(board + 7),
            read_i8(board + 8),
        ]
    }
}

fn occupy_cell(board: usize, index: usize, x: i64, y: i64, player: i8) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(8);
    while unsafe { read_i8(board + index) } == 0 {
        if Instant::now() >= deadline {
            return Err(format!("cell {index} stayed empty; board={:?}", unsafe {
                read_board(board)
            }));
        }
        host::set_mouse(x, y, false, false);
        thread::sleep(Duration::from_millis(20));
        host::set_mouse(x, y, true, false);
        thread::sleep(Duration::from_millis(80));
        host::set_mouse(x, y, false, false);
        thread::sleep(Duration::from_millis(40));
    }
    let value = unsafe { read_i8(board + index) };
    if value != player {
        return Err(format!(
            "cell {index} is {value}, expected {player}; board={:?}",
            unsafe { read_board(board) }
        ));
    }
    Ok(())
}

#[test]
fn tictactoe_plays_a_winning_column_for_x() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../TempleOS/Demo/Games/TicTacToe.HC");
    if !path.is_file() {
        eprintln!(
            "skipping TicTacToe gameplay test: {} is absent",
            path.display()
        );
        return;
    }

    let mut program = compile_file(&path, &CompileOptions::default()).unwrap();
    let globals = program.global_bindings().unwrap();
    let board = globals
        .iter()
        .find(|global| global.name == "board")
        .unwrap_or_else(|| panic!("TicTacToe global `board` is missing"))
        .address as usize;

    let helper = thread::spawn(move || {
        let result = (|| -> Result<[i8; 9], String> {
            let startup = Instant::now() + Duration::from_secs(10);
            while host::frames_presented() < 2 {
                if Instant::now() >= startup {
                    return Err("TicTacToe did not present a board".into());
                }
                thread::sleep(Duration::from_millis(10));
            }

            // X: (0,0), O: (1,0), X: (0,1), O: (2,0), X: (0,2) wins the left column.
            occupy_cell(board, 0, 150, 150, 1)?;
            occupy_cell(board, 1, 250, 150, 2)?;
            occupy_cell(board, 3, 150, 250, 1)?;
            occupy_cell(board, 2, 350, 150, 2)?;
            occupy_cell(board, 6, 150, 350, 1)?;
            Ok(unsafe { read_board(board) })
        })();
        templeos_compat::runtime::request_throw(0x1b);
        result
    });

    run_program(program, HostMode::External).unwrap();
    let cells = helper
        .join()
        .expect("TicTacToe helper thread panicked")
        .unwrap();
    assert_eq!(cells[0], 1);
    assert_eq!(cells[3], 1);
    assert_eq!(cells[6], 1);
    assert_eq!(cells[1], 2);
    assert_eq!(cells[2], 2);
}

use sanctum::runner::{RunnerEvent, RunnerSession};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn run_once(executable: &Path, entry: &Path, data: &Path, verify_input: bool) {
    let mut runner = RunnerSession::spawn_with_executable(executable, entry, None, data).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut received_frame = false;
    let mut received_input_frame = false;
    let mut received_exit = false;
    while Instant::now() < deadline {
        while let Some(event) = runner.try_recv() {
            match event {
                RunnerEvent::Frame { indexed, .. } => {
                    let painted = indexed.iter().any(|pixel| *pixel != indexed[0]);
                    if !received_frame && painted {
                        received_frame = true;
                        if verify_input {
                            runner.send_key(0, 0x48).unwrap();
                        } else {
                            runner.stop().unwrap();
                        }
                    } else if verify_input
                        && !received_input_frame
                        && indexed.iter().any(|pixel| pixel & 0x0f == 5)
                    {
                        received_input_frame = true;
                        runner.stop().unwrap();
                    }
                }
                RunnerEvent::Exited(success) => {
                    assert!(success);
                    received_exit = true;
                }
                RunnerEvent::Error(error) => panic!("runner error: {error}"),
                _ => {}
            }
        }
        if !runner.update_lifecycle().unwrap() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(received_frame, "runner did not publish a frame");
    assert!(
        !verify_input || received_input_frame,
        "runner did not deliver input to the HolyC program"
    );
    assert!(received_exit, "runner did not report a clean exit");
    assert!(!runner.update_lifecycle().unwrap(), "runner remained alive");
}

#[test]
fn graphical_runner_can_stop_and_restart_without_orphans() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_sanctum"));
    let entry = workspace.join("tests/holyc/gui_smoke.HC");
    let data = std::env::temp_dir().join(format!("sanctum-runner-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    run_once(&executable, &entry, &data, true);
    run_once(&executable, &entry, &data, true);
    let talons = workspace.join("TempleOS/Demo/Games/Talons.HC");
    if talons.is_file() {
        run_once(&executable, &talons, &data, false);
    }
    let _ = std::fs::remove_dir_all(data);
}

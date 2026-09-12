use sanctum::runner::{RunnerEvent, RunnerSession};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn run_once(executable: &Path, entry: &Path, data: &Path) {
    let mut runner = RunnerSession::spawn_with_executable(executable, entry, None, data).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut received_frame = false;
    let mut received_exit = false;
    while Instant::now() < deadline {
        while let Some(event) = runner.try_recv() {
            match event {
                RunnerEvent::Frame { indexed, .. } if !received_frame => {
                    assert!(indexed.iter().any(|pixel| *pixel != indexed[0]));
                    received_frame = true;
                    runner.send_key(0x1b, 0).unwrap();
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
    run_once(&executable, &entry, &data);
    run_once(&executable, &entry, &data);
    let _ = std::fs::remove_dir_all(data);
}

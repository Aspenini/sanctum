#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() == Some(std::ffi::OsStr::new("--runner")) {
        let Some(entry) = args.next().map(PathBuf::from) else {
            std::process::exit(2)
        };
        if let Err(error) = sanctum::runner::run_child(entry) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    sanctum::app::run()?;
    Ok(())
}

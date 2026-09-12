#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() -> eframe::Result {
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
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Sanctum")
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([760.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Sanctum",
        options,
        Box::new(|cc| Ok(Box::new(sanctum::app::SanctumApp::new(cc)))),
    )
}

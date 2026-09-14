pub mod app;
#[cfg(all(target_os = "android", feature = "android-backend"))]
pub mod android_picker;
pub mod controls;
pub mod model;
pub mod protocol;
pub mod runner;

pub use model::{LibraryEntry, SanctumConfig, ScaleMode, StorageMode};
pub use protocol::{IndexedFrame, KeyEvent};

#[cfg(all(target_os = "android", feature = "android-backend"))]
#[unsafe(no_mangle)]
fn android_main(android_app: slint::android::AndroidApp) {
    if let Some(path) = android_app.internal_data_path() {
        model::set_android_data_dir(path);
    }
    android_picker::set_app(android_app.clone());
    slint::android::init(android_app).expect("failed to initialize the Android Slint backend");
    if let Err(error) = app::run() {
        eprintln!("{error}");
    }
}

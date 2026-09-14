//! Android Storage Access Framework picker, via Kotlin `SlintActivity`.
//!
//! Same shape as gopher64: a NativeActivity subclass forwards
//! `onActivityResult` into JNI. Kotlin copies the picked tree or file into
//! app storage so HolyC sees ordinary filesystem paths.

use jni::objects::{JClass, JObject, JString};
use jni::sys::{jint, jobject};
use jni::{JNIEnv, JavaVM};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug)]
pub enum PickerKind {
    LibraryFolder,
    TempleOsFolder,
    HolyCFile,
    CoverImage,
}

type PickHandler = Box<dyn Fn(PickerKind, Option<PathBuf>)>;

static ANDROID_APP: Mutex<Option<slint::android::AndroidApp>> = Mutex::new(None);
static PENDING: Mutex<Option<PickerKind>> = Mutex::new(None);

thread_local! {
    static HANDLER: RefCell<Option<PickHandler>> = const { RefCell::new(None) };
}

pub fn set_app(app: slint::android::AndroidApp) {
    *ANDROID_APP.lock().expect("android app lock") = Some(app);
}

pub fn set_handler(handler: PickHandler) {
    HANDLER.with(|slot| *slot.borrow_mut() = Some(handler));
}

pub fn begin(kind: PickerKind) {
    *PENDING.lock().expect("pending picker lock") = Some(kind);
    if let Err(error) = start_picker(kind) {
        eprintln!("unable to start Android picker: {error}");
        deliver(None);
    }
}

fn start_picker(kind: PickerKind) -> Result<(), String> {
    let app = ANDROID_APP
        .lock()
        .expect("android app lock")
        .clone()
        .ok_or_else(|| "Android app not initialized".to_string())?;
    let vm =
        unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.map_err(|error| error.to_string())?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| error.to_string())?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jobject) };
    match kind {
        PickerKind::LibraryFolder | PickerKind::TempleOsFolder => {
            env.call_method(&activity, "pickFolder", "()V", &[])
                .map_err(|error| error.to_string())?;
        }
        PickerKind::HolyCFile | PickerKind::CoverImage => {
            let mime = match kind {
                PickerKind::CoverImage => "image/*",
                _ => "*/*",
            };
            let mime = env.new_string(mime).map_err(|error| error.to_string())?;
            env.call_method(
                &activity,
                "pickFile",
                "(Ljava/lang/String;)V",
                &[(&mime).into()],
            )
            .map_err(|error| error.to_string())?;
        }
    }
    std::mem::forget(activity);
    Ok(())
}

fn deliver(path: Option<PathBuf>) {
    let kind = PENDING.lock().expect("pending picker lock").take();
    let Some(kind) = kind else {
        return;
    };
    let _ = slint::invoke_from_event_loop(move || {
        HANDLER.with(|slot| {
            if let Some(handler) = slot.borrow().as_ref() {
                handler(kind, path);
            }
        });
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_sanctum_app_SlintActivity_nativeOnPicked(
    mut env: JNIEnv,
    _class: JClass,
    _request_code: jint,
    path: JString,
) {
    if path.is_null() {
        deliver(None);
        return;
    }
    match env.get_string(&path) {
        Ok(text) => {
            let text = text.to_string_lossy();
            if text.is_empty() {
                deliver(None);
            } else {
                deliver(Some(PathBuf::from(text.as_ref())));
            }
        }
        Err(_) => deliver(None),
    }
}

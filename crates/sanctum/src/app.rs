use crate::model::{self, LibraryEntry, SanctumConfig, ScaleMode, StorageMode, StoragePaths};
use crate::runner::{RunnerEvent, RunnerSession};
use slint::platform::Key;
use slint::{
    CloseRequestResponse, ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer,
    SharedString, Timer, TimerMode, VecModel,
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;
#[cfg(test)]
use templeos_compat::host::input::{SCF_ALT, SCF_CTRL, SCF_SHIFT};
use templeos_compat::host::input::{ascii_key_event, scan_flags};
use uuid::Uuid;

slint::include_modules!();

#[derive(Clone, Copy, Eq, PartialEq)]
enum Filter {
    All,
    Favorites,
    Recent,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Sort {
    Title,
    Recent,
    Added,
}

#[derive(Clone)]
enum RunTarget {
    Library(Uuid),
    Direct(PathBuf),
}

struct EntryChoiceRequest {
    root: PathBuf,
    files: Vec<PathBuf>,
    editing: Uuid,
    launch_after: bool,
}

struct AppState {
    paths: StoragePaths,
    config: SanctumConfig,
    filter: Filter,
    sort: Sort,
    search: String,
    selected: Option<Uuid>,
    entry_choices: Option<EntryChoiceRequest>,
    runner: Option<RunnerSession>,
    running: Option<RunTarget>,
    pending_restart: Option<RunTarget>,
    pending_storage: Option<StorageMode>,
    status: String,
    log: String,
    frame: Option<Image>,
    frame_width: u32,
    frame_height: u32,
    frame_sequence: u64,
    menu: Option<String>,
    detached: bool,
    fullscreen: bool,
    log_open: bool,
    library_dirty: bool,
}

impl AppState {
    fn new() -> Self {
        let paths = StoragePaths::detect().expect("Sanctum data directory is unavailable");
        let (mut config, recovery) = model::load_recovering(&paths).unwrap_or_default();
        config.storage_mode = if paths.portable {
            StorageMode::Portable
        } else {
            StorageMode::PerUser
        };
        Self {
            paths,
            config,
            filter: Filter::All,
            sort: Sort::Title,
            search: String::new(),
            selected: None,
            entry_choices: None,
            runner: None,
            running: None,
            pending_restart: None,
            pending_storage: None,
            status: recovery.map_or_else(
                || "Ready".into(),
                |path| format!("Recovered a corrupt library database to {}", path.display()),
            ),
            log: String::new(),
            frame: None,
            frame_width: 640,
            frame_height: 480,
            frame_sequence: 0,
            menu: None,
            detached: false,
            fullscreen: false,
            log_open: false,
            library_dirty: true,
        }
    }

    fn persist(&mut self) {
        if let Err(error) = model::save(&self.paths, &self.config) {
            self.status = format!("Could not save library: {error}");
        }
    }

    fn add_library_folder(&mut self, root: &Path) {
        let candidates = match model::discover_library(root) {
            Ok(candidates) => candidates,
            Err(error) => {
                self.status = format!("Unable to scan library folder: {error}");
                return;
            }
        };
        let mut added = 0usize;
        let mut duplicates = 0usize;
        for candidate in candidates {
            let entry = match candidate {
                model::DiscoveredEntry::Source(path) => LibraryEntry::from_source(&path),
                model::DiscoveredEntry::Project(path) => LibraryEntry::from_directory(&path),
            };
            let Ok(entry) = entry else { continue };
            let duplicate = if entry.entrypoint.as_os_str().is_empty() {
                self.config
                    .library
                    .iter()
                    .any(|item| item.project_root == entry.project_root)
            } else {
                self.config
                    .library
                    .iter()
                    .any(|item| item.source_path() == entry.source_path())
            };
            if duplicate {
                duplicates += 1;
            } else {
                self.config.library.push(entry);
                added += 1;
            }
        }
        self.selected = None;
        self.library_dirty = true;
        if added > 0 {
            self.persist();
        }
        self.status = match (added, duplicates) {
            (0, 0) => "No HolyC programs found in that folder".into(),
            (0, duplicates) => format!("All {duplicates} discovered programs are already added"),
            (added, 0) => format!("Added {added} programs"),
            (added, duplicates) => {
                format!("Added {added} programs; skipped {duplicates} duplicates")
            }
        };
    }

    fn scan_project(&mut self, root: PathBuf, editing: Uuid, launch_after: bool) {
        match model::discover_holyc(&root) {
            Ok(files) if files.len() == 1 => {
                if let Some(item) = self
                    .config
                    .library
                    .iter_mut()
                    .find(|item| item.id == editing)
                {
                    item.project_root = root;
                    item.entrypoint = files[0].clone();
                    self.persist();
                    self.library_dirty = true;
                    if launch_after {
                        self.launch(editing);
                    }
                }
            }
            Ok(files) if !files.is_empty() => {
                self.entry_choices = Some(EntryChoiceRequest {
                    root,
                    files,
                    editing,
                    launch_after,
                });
            }
            Ok(_) => self.status = "No .HC files found in that folder".into(),
            Err(error) => self.status = format!("Unable to scan folder: {error}"),
        }
    }

    fn launch(&mut self, id: Uuid) {
        let Some(item) = self.config.library.iter().find(|item| item.id == id) else {
            return;
        };
        if item.entrypoint.as_os_str().is_empty() {
            self.scan_project(item.project_root.clone(), id, true);
            return;
        }
        let source = item.source_path();
        if !source.is_file() {
            self.status = "The selected entrypoint is missing".into();
            return;
        }
        if self.spawn_source(&source, RunTarget::Library(id)) {
            if let Some(item) = self.config.library.iter_mut().find(|item| item.id == id) {
                item.last_played = Some(model::now());
            }
            self.library_dirty = true;
            self.persist();
        }
    }

    fn launch_direct(&mut self, path: &Path) {
        let entry = match LibraryEntry::from_source(path) {
            Ok(entry) => entry,
            Err(error) => {
                self.status = format!("Unable to run program: {error}");
                return;
            }
        };
        let source = entry.source_path();
        self.spawn_source(&source, RunTarget::Direct(source.clone()));
    }

    fn spawn_source(&mut self, source: &Path, target: RunTarget) -> bool {
        let templeos_root = self
            .config
            .templeos_root
            .clone()
            .filter(|root| root.join("Kernel/KernelA.HH").is_file());
        if let Some(mut runner) = self.runner.take() {
            runner.kill();
        }
        match RunnerSession::spawn(source, templeos_root.as_deref(), &self.paths.root) {
            Ok(runner) => {
                self.running = Some(target);
                self.runner = Some(runner);
                self.status = "Starting".into();
                self.log.clear();
                self.frame = None;
                self.frame_sequence = 0;
                self.menu = None;
                if let Some(runner) = &self.runner {
                    let _ = runner.set_muted(self.config.muted);
                }
                true
            }
            Err(error) => {
                self.status = format!("Unable to start runner: {error}");
                false
            }
        }
    }

    fn restart(&mut self) {
        let Some(target) = self.running.clone() else {
            return;
        };
        if let Some(runner) = &mut self.runner {
            self.pending_restart = Some(target);
            let _ = runner.stop();
            self.status = "Restarting".into();
        } else {
            self.launch_target(target);
        }
    }

    fn launch_target(&mut self, target: RunTarget) {
        match target {
            RunTarget::Library(id) => self.launch(id),
            RunTarget::Direct(path) => self.launch_direct(&path),
        }
    }

    fn stop(&mut self) {
        self.pending_restart = None;
        if let Some(runner) = &mut self.runner {
            let _ = runner.stop();
            self.status = "Stopping".into();
        }
    }

    fn poll_runner(&mut self) {
        let mut events = Vec::new();
        if let Some(runner) = &self.runner {
            while let Some(event) = runner.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                RunnerEvent::Status(status) => self.status = status,
                RunnerEvent::Log(text) => self.log.push_str(&text),
                RunnerEvent::Warning(text) => {
                    append_line(&mut self.log, &text);
                    self.status = "Running with warnings".into();
                }
                RunnerEvent::Error(text) => {
                    append_line(&mut self.log, &text);
                    self.status = "Error".into();
                }
                RunnerEvent::Menu(menu) => self.menu = menu,
                RunnerEvent::Exited(success) => {
                    self.status = if success { "Exited" } else { "Failed" }.into()
                }
                RunnerEvent::Frame {
                    sequence,
                    width,
                    height,
                    indexed,
                } => {
                    self.frame_sequence = sequence;
                    self.frame_width = width;
                    self.frame_height = height;
                    self.frame = Some(indexed_image(width, height, &indexed));
                    self.capture_cover(width, height, &indexed);
                }
            }
        }
        if let Some(runner) = &mut self.runner {
            match runner.update_lifecycle() {
                Ok(true) => {}
                Ok(false) => self.finish_runner(),
                Err(error) => {
                    self.status = format!("Runner error: {error}");
                    self.finish_runner();
                }
            }
        }
    }

    fn finish_runner(&mut self) {
        let restart = self.pending_restart.take();
        self.runner = None;
        self.running = None;
        if restart.is_none() {
            self.detached = false;
            self.fullscreen = false;
        }
        if let Some(target) = restart {
            self.launch_target(target);
        }
    }

    fn capture_cover(&mut self, width: u32, height: u32, indexed: &[u8]) {
        let Some(RunTarget::Library(id)) = self.running.as_ref() else {
            return;
        };
        let id = *id;
        let Some(item) = self.config.library.iter_mut().find(|item| item.id == id) else {
            return;
        };
        if item.cover.is_some() || indexed.iter().copied().collect::<HashSet<_>>().len() < 5 {
            return;
        }
        let path = self.paths.covers.join(format!("{id}.png"));
        let mut rgb = Vec::with_capacity(indexed.len() * 3);
        for color in indexed {
            rgb.extend_from_slice(&templeos_compat::graphics::palette_rgb(*color));
        }
        let Some(image) = image::RgbImage::from_raw(width, height, rgb) else {
            return;
        };
        let crop_width = width.min(height.saturating_mul(4) / 3).max(1);
        let crop_height = height.min(width.saturating_mul(3) / 4).max(1);
        let left = (width - crop_width) / 2;
        let top = (height - crop_height) / 2;
        let cropped =
            image::imageops::crop_imm(&image, left, top, crop_width, crop_height).to_image();
        let cover =
            image::imageops::resize(&cropped, 640, 480, image::imageops::FilterType::Nearest);
        if cover.save(&path).is_ok() {
            item.cover = Some(path);
            self.library_dirty = true;
            self.persist();
        }
    }

    fn import_cover(&mut self, id: Uuid) {
        let Some(source) = rfd::FileDialog::new()
            .add_filter("Image", &["png", "jpg", "jpeg"])
            .pick_file()
        else {
            return;
        };
        let extension = source
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("png");
        let destination = self.paths.covers.join(format!("{id}-custom.{extension}"));
        match fs::copy(source, &destination) {
            Ok(_) => {
                if let Some(item) = self.config.library.iter_mut().find(|item| item.id == id) {
                    item.cover = Some(destination);
                    self.persist();
                    self.library_dirty = true;
                }
            }
            Err(error) => self.status = format!("Unable to import cover: {error}"),
        }
    }

    fn send_input(&self, text: &str, shift: bool, ctrl: bool, alt: bool) {
        let Some(runner) = &self.runner else { return };
        if let Some((ch, scan)) = map_slint_special_key(text, shift, ctrl, alt) {
            let _ = runner.send_key(ch, scan);
            return;
        }
        for (ch, scan) in text_key_events(text, shift, ctrl, alt) {
            let _ = runner.send_key(ch, scan);
        }
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        if let Some(runner) = &mut self.runner {
            runner.kill();
        }
        self.persist();
    }
}

macro_rules! simple_main_callback {
    ($ui:expr, $state:expr, $method:ident, |$s:ident: &mut AppState| $body:block) => {{
        let ui_weak = $ui.as_weak();
        let shared = $state.clone();
        $ui.$method(move || {
            {
                let $s = &mut *shared.borrow_mut();
                $body
            }
            if let Some(ui) = ui_weak.upgrade() {
                if shared.borrow().library_dirty {
                    sync_library(&ui, &shared);
                }
                sync_main_only(&ui, &shared);
            }
        });
    }};
    ($ui:expr, $state:expr, $method:ident, |$s:ident: &mut AppState, $a:ident: $t:ty| $body:block) => {{
        let ui_weak = $ui.as_weak();
        let shared = $state.clone();
        $ui.$method(move |$a: $t| {
            {
                let $s = &mut *shared.borrow_mut();
                $body
            }
            if let Some(ui) = ui_weak.upgrade() {
                if shared.borrow().library_dirty {
                    sync_library(&ui, &shared);
                }
                sync_main_only(&ui, &shared);
            }
        });
    }};
    ($ui:expr, $state:expr, $method:ident, |$s:ident: &mut AppState, $a:ident: $ta:ty, $b:ident: $tb:ty, $c:ident: $tc:ty, $d:ident: $td:ty| $body:block) => {{
        let ui_weak = $ui.as_weak();
        let shared = $state.clone();
        $ui.$method(move |$a: $ta, $b: $tb, $c: $tc, $d: $td| {
            {
                let $s = &mut *shared.borrow_mut();
                $body
            }
            if let Some(ui) = ui_weak.upgrade() {
                if shared.borrow().library_dirty {
                    sync_library(&ui, &shared);
                }
                sync_main_only(&ui, &shared);
            }
        });
    }};
}

pub fn run() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let game = GameWindow::new()?;
    let state = Rc::new(RefCell::new(AppState::new()));

    install_main_callbacks(&ui, &game, &state);
    install_game_callbacks(&ui, &game, &state);
    sync_all(&ui, &game, &state);

    let ui_weak = ui.as_weak();
    let game_weak = game.as_weak();
    let timer_state = state.clone();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        timer_state.borrow_mut().poll_runner();
        if let (Some(ui), Some(game)) = (ui_weak.upgrade(), game_weak.upgrade()) {
            sync_runtime(&ui, &game, &timer_state);
            if timer_state.borrow().library_dirty {
                sync_library(&ui, &timer_state);
            }
        }
    });

    let ui_weak = ui.as_weak();
    let game_weak = game.as_weak();
    let game_close_state = state.clone();
    game.window().on_close_requested(move || {
        let fullscreen = {
            let mut state = game_close_state.borrow_mut();
            state.detached = false;
            state.fullscreen
        };
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_detached(false);
            ui.window().set_fullscreen(fullscreen);
        }
        if let Some(game) = game_weak.upgrade() {
            game.window().set_fullscreen(false);
        }
        CloseRequestResponse::HideWindow
    });

    let game_weak = game.as_weak();
    let close_state = state.clone();
    ui.window().on_close_requested(move || {
        if let Some(game) = game_weak.upgrade() {
            let _ = game.hide();
        }
        if let Some(mut runner) = close_state.borrow_mut().runner.take() {
            runner.kill();
        }
        CloseRequestResponse::HideWindow
    });

    ui.run()
}

fn install_main_callbacks(ui: &MainWindow, game: &GameWindow, state: &Rc<RefCell<AppState>>) {
    let weak = ui.as_weak();
    let game_weak = game.as_weak();
    let shared = state.clone();
    ui.on_add_library(move || {
        if let Some(root) = rfd::FileDialog::new().pick_folder() {
            shared.borrow_mut().add_library_folder(&root);
        }
        sync_from_weaks(&weak, &game_weak, &shared);
    });

    let weak = ui.as_weak();
    let game_weak = game.as_weak();
    let shared = state.clone();
    ui.on_run_file(move || {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("HolyC", &["HC", "hc"])
            .pick_file()
        {
            shared.borrow_mut().launch_direct(&path);
        }
        sync_from_weaks(&weak, &game_weak, &shared);
    });

    simple_main_callback!(
        ui,
        state,
        on_search_changed,
        |state: &mut AppState, value: SharedString| {
            state.search = value.to_string();
            state.library_dirty = true;
        }
    );
    simple_main_callback!(
        ui,
        state,
        on_set_filter,
        |state: &mut AppState, value: i32| {
            state.filter = match value {
                1 => Filter::Favorites,
                2 => Filter::Recent,
                _ => Filter::All,
            };
            state.library_dirty = true;
        }
    );
    simple_main_callback!(
        ui,
        state,
        on_set_sort,
        |state: &mut AppState, value: i32| {
            state.sort = match value {
                1 => Sort::Recent,
                2 => Sort::Added,
                _ => Sort::Title,
            };
            state.library_dirty = true;
        }
    );
    simple_main_callback!(
        ui,
        state,
        on_select_game,
        |state: &mut AppState, value: SharedString| {
            state.selected = Uuid::parse_str(value.as_str()).ok();
        }
    );
    simple_main_callback!(
        ui,
        state,
        on_run_game,
        |state: &mut AppState, value: SharedString| {
            if let Ok(id) = Uuid::parse_str(value.as_str()) {
                state.launch(id);
            }
        }
    );
    simple_main_callback!(
        ui,
        state,
        on_toggle_favorite,
        |state: &mut AppState, value: SharedString| {
            if let Ok(id) = Uuid::parse_str(value.as_str())
                && let Some(item) = state.config.library.iter_mut().find(|item| item.id == id)
            {
                item.favorite = !item.favorite;
                state.persist();
                state.library_dirty = true;
            }
        }
    );
    simple_main_callback!(ui, state, on_close_details, |state: &mut AppState| {
        state.selected = None;
    });
    simple_main_callback!(
        ui,
        state,
        on_save_title,
        |state: &mut AppState, value: SharedString| {
            if let Some(id) = state.selected
                && let Some(item) = state.config.library.iter_mut().find(|item| item.id == id)
            {
                item.title = value.to_string();
                state.persist();
                state.library_dirty = true;
            }
        }
    );
    simple_main_callback!(
        ui,
        state,
        on_toggle_selected_favorite,
        |state: &mut AppState| {
            if let Some(id) = state.selected
                && let Some(item) = state.config.library.iter_mut().find(|item| item.id == id)
            {
                item.favorite = !item.favorite;
                state.persist();
                state.library_dirty = true;
            }
        }
    );

    let weak = ui.as_weak();
    let game_weak = game.as_weak();
    let shared = state.clone();
    ui.on_choose_entrypoint(move || {
        let selection = {
            let state = shared.borrow();
            state.selected.and_then(|id| {
                state
                    .config
                    .library
                    .iter()
                    .find(|item| item.id == id)
                    .map(|item| (item.project_root.clone(), id))
            })
        };
        if let Some((root, id)) = selection {
            shared.borrow_mut().scan_project(root, id, false);
        }
        sync_from_weaks(&weak, &game_weak, &shared);
    });

    simple_main_callback!(ui, state, on_choose_cover, |state: &mut AppState| {
        if let Some(id) = state.selected {
            state.import_cover(id);
        }
    });
    simple_main_callback!(ui, state, on_remove_selected, |state: &mut AppState| {
        if let Some(id) = state.selected.take() {
            state.config.library.retain(|item| item.id != id);
            state.persist();
            state.library_dirty = true;
        }
    });
    simple_main_callback!(ui, state, on_run_selected, |state: &mut AppState| {
        if let Some(id) = state.selected.take() {
            state.launch(id);
        }
    });
    simple_main_callback!(
        ui,
        state,
        on_choose_entry,
        |state: &mut AppState, value: SharedString| {
            if let Some(request) = state.entry_choices.take() {
                let relative = PathBuf::from(value.as_str());
                let id = request.editing;
                if let Some(item) = state.config.library.iter_mut().find(|item| item.id == id) {
                    item.project_root = request.root;
                    item.entrypoint = relative;
                    state.persist();
                    state.library_dirty = true;
                    if request.launch_after {
                        state.launch(id);
                    }
                }
            }
        }
    );
    simple_main_callback!(ui, state, on_cancel_choices, |state: &mut AppState| {
        state.entry_choices = None;
    });

    {
        let weak = ui.as_weak();
        ui.on_show_settings(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_settings_open(true);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_close_settings(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_settings_open(false);
            }
        });
    }

    let weak = ui.as_weak();
    let game_weak = game.as_weak();
    let shared = state.clone();
    ui.on_choose_templeos(move || {
        if let Some(root) = rfd::FileDialog::new().pick_folder() {
            let mut state = shared.borrow_mut();
            if root.join("Kernel/KernelA.HH").is_file() {
                state.config.templeos_root = Some(root);
                state.status = "TempleOS folder selected".into();
                state.persist();
            } else {
                state.status = "That folder is not a TempleOS root".into();
            }
        }
        sync_from_weaks(&weak, &game_weak, &shared);
    });

    simple_main_callback!(
        ui,
        state,
        on_set_scaling,
        |state: &mut AppState, fit: bool| {
            state.config.scale_mode = if fit {
                ScaleMode::Fit
            } else {
                ScaleMode::Integer
            };
            state.persist();
        }
    );

    {
        let weak = ui.as_weak();
        let shared = state.clone();
        ui.on_request_storage_change(move || {
            let mut state = shared.borrow_mut();
            state.pending_storage = Some(if state.paths.portable {
                StorageMode::PerUser
            } else {
                StorageMode::Portable
            });
            if let Some(ui) = weak.upgrade() {
                ui.set_storage_confirm_open(true);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let shared = state.clone();
        ui.on_cancel_storage_change(move || {
            shared.borrow_mut().pending_storage = None;
            if let Some(ui) = weak.upgrade() {
                ui.set_storage_confirm_open(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let shared = state.clone();
        ui.on_confirm_storage_change(move || {
            let mut state = shared.borrow_mut();
            let Some(target) = state.pending_storage.take() else {
                return;
            };
            state.config.storage_mode = target;
            state.persist();
            match migrate_storage(&state.paths, target == StorageMode::Portable) {
                Ok(()) => match std::env::current_exe()
                    .and_then(|exe| std::process::Command::new(exe).spawn())
                {
                    Ok(_) => {
                        let _ = slint::quit_event_loop();
                    }
                    Err(error) => state.status = format!("Data moved, but restart failed: {error}"),
                },
                Err(error) => {
                    state.config.storage_mode = if state.paths.portable {
                        StorageMode::Portable
                    } else {
                        StorageMode::PerUser
                    };
                    state.status = format!("Storage migration failed: {error}");
                }
            }
            if let Some(ui) = weak.upgrade() {
                ui.set_storage_confirm_open(false);
            }
        });
    }

    simple_main_callback!(ui, state, on_show_library, |state: &mut AppState| {
        state.detached = false;
        state.stop();
    });
    simple_main_callback!(ui, state, on_stop_runner, |state: &mut AppState| {
        state.stop();
    });
    simple_main_callback!(ui, state, on_restart_runner, |state: &mut AppState| {
        state.restart();
    });
    simple_main_callback!(ui, state, on_toggle_mute, |state: &mut AppState| {
        state.config.muted = !state.config.muted;
        if let Some(runner) = &state.runner {
            let _ = runner.set_muted(state.config.muted);
        }
        state.persist();
    });

    let ui_weak = ui.as_weak();
    let game_weak = game.as_weak();
    let shared = state.clone();
    ui.on_toggle_fullscreen(move || {
        let fullscreen = {
            let mut state = shared.borrow_mut();
            state.fullscreen = !state.fullscreen;
            state.fullscreen
        };
        if let (Some(ui), Some(game)) = (ui_weak.upgrade(), game_weak.upgrade()) {
            if shared.borrow().detached {
                game.window().set_fullscreen(fullscreen);
            } else {
                ui.window().set_fullscreen(fullscreen);
            }
            sync_runtime(&ui, &game, &shared);
        }
    });

    let ui_weak = ui.as_weak();
    let game_weak = game.as_weak();
    let shared = state.clone();
    ui.on_toggle_detached(move || {
        if let (Some(ui), Some(game)) = (ui_weak.upgrade(), game_weak.upgrade()) {
            toggle_detached(&ui, &game, &shared);
        }
    });

    simple_main_callback!(ui, state, on_toggle_log, |state: &mut AppState| {
        state.log_open = !state.log_open;
    });
    simple_main_callback!(
        ui,
        state,
        on_invoke_menu,
        |state: &mut AppState, value: SharedString| {
            if let Some((ch, scan)) = decode_menu_command(value.as_str())
                && let Some(runner) = &state.runner
            {
                let _ = runner.send_key(ch, scan);
            }
        }
    );
    let shared = state.clone();
    ui.on_game_key(move |text, shift, control, alt| {
        shared
            .borrow()
            .send_input(text.as_str(), shift, control, alt);
    });
}

fn install_game_callbacks(ui: &MainWindow, game: &GameWindow, state: &Rc<RefCell<AppState>>) {
    let ui_weak = ui.as_weak();
    let game_weak = game.as_weak();
    let shared = state.clone();
    game.on_dock(move || {
        if let (Some(ui), Some(game)) = (ui_weak.upgrade(), game_weak.upgrade()) {
            toggle_detached(&ui, &game, &shared);
        }
    });

    let ui_weak = ui.as_weak();
    let game_weak = game.as_weak();
    let shared = state.clone();
    game.on_stop_runner(move || {
        shared.borrow_mut().stop();
        sync_from_weaks(&ui_weak, &game_weak, &shared);
    });
    let ui_weak = ui.as_weak();
    let game_weak = game.as_weak();
    let shared = state.clone();
    game.on_restart_runner(move || {
        shared.borrow_mut().restart();
        sync_from_weaks(&ui_weak, &game_weak, &shared);
    });
    let ui_weak = ui.as_weak();
    let game_weak = game.as_weak();
    let shared = state.clone();
    game.on_toggle_mute(move || {
        let mut state = shared.borrow_mut();
        state.config.muted = !state.config.muted;
        if let Some(runner) = &state.runner {
            let _ = runner.set_muted(state.config.muted);
        }
        state.persist();
        drop(state);
        sync_from_weaks(&ui_weak, &game_weak, &shared);
    });
    let ui_weak = ui.as_weak();
    let game_weak = game.as_weak();
    let shared = state.clone();
    game.on_toggle_fullscreen(move || {
        let fullscreen = {
            let mut state = shared.borrow_mut();
            state.fullscreen = !state.fullscreen;
            state.fullscreen
        };
        if let Some(game) = game_weak.upgrade() {
            game.window().set_fullscreen(fullscreen);
        }
        sync_from_weaks(&ui_weak, &game_weak, &shared);
    });
    let shared = state.clone();
    game.on_game_key(move |text, shift, control, alt| {
        shared
            .borrow()
            .send_input(text.as_str(), shift, control, alt);
    });
}

fn sync_from_weaks(
    ui: &slint::Weak<MainWindow>,
    game: &slint::Weak<GameWindow>,
    state: &Rc<RefCell<AppState>>,
) {
    if let (Some(ui), Some(game)) = (ui.upgrade(), game.upgrade()) {
        sync_all(&ui, &game, state);
    }
}

fn sync_all(ui: &MainWindow, game: &GameWindow, state: &Rc<RefCell<AppState>>) {
    sync_library(ui, state);
    sync_runtime(ui, game, state);
    sync_main_only(ui, state);
}

fn sync_main_only(ui: &MainWindow, state: &Rc<RefCell<AppState>>) {
    let state = state.borrow();
    ui.set_status(state.status.clone().into());
    ui.set_filter_index(match state.filter {
        Filter::All => 0,
        Filter::Favorites => 1,
        Filter::Recent => 2,
    });
    ui.set_sort_index(match state.sort {
        Sort::Title => 0,
        Sort::Recent => 1,
        Sort::Added => 2,
    });
    ui.set_muted(state.config.muted);
    ui.set_log_open(state.log_open);
    ui.set_program_log(state.log.clone().into());
    ui.set_fit_scaling(state.config.scale_mode == ScaleMode::Fit);
    ui.set_data_path(state.paths.root.display().to_string().into());
    ui.set_storage_label(
        if state.paths.portable {
            "Portable"
        } else {
            "Per-user"
        }
        .into(),
    );
    ui.set_storage_action(
        if state.paths.portable {
            "Switch to per-user storage"
        } else {
            "Switch to portable storage"
        }
        .into(),
    );
    ui.set_portable_available(
        state.paths.portable || directory_is_writable(&state.paths.executable_dir),
    );
    ui.set_templeos_path(
        state
            .config
            .templeos_root
            .as_ref()
            .filter(|root| root.join("Kernel/KernelA.HH").is_file())
            .map_or_else(
                || "Not selected — optional for self-contained programs".into(),
                |root| root.display().to_string(),
            )
            .into(),
    );
    ui.set_details_open(state.selected.is_some());
    if let Some(item) = state
        .selected
        .and_then(|id| state.config.library.iter().find(|item| item.id == id))
    {
        ui.set_selected_title(item.title.clone().into());
        ui.set_selected_path(
            if item.entrypoint.as_os_str().is_empty() {
                format!(
                    "{} (entrypoint chosen on first run)",
                    item.project_root.display()
                )
            } else {
                item.source_path().display().to_string()
            }
            .into(),
        );
        ui.set_selected_favorite(item.favorite);
    }
    ui.set_choices_open(state.entry_choices.is_some());
    let choices: Vec<ChoiceItem> = state
        .entry_choices
        .as_ref()
        .map(|request| {
            request
                .files
                .iter()
                .map(|path| ChoiceItem {
                    path: path.display().to_string().into(),
                })
                .collect()
        })
        .unwrap_or_default();
    ui.set_entry_choices(ModelRc::new(VecModel::from(choices)));
}

fn sync_runtime(ui: &MainWindow, game: &GameWindow, state: &Rc<RefCell<AppState>>) {
    let state = state.borrow();
    let running = state.runner.is_some();
    ui.set_running(running);
    ui.set_status(state.status.clone().into());
    ui.set_detached(state.detached);
    ui.set_muted(state.config.muted);
    ui.set_fullscreen(state.fullscreen);
    ui.set_log_open(state.log_open);
    ui.set_program_log(state.log.clone().into());
    ui.set_frame_ready(state.frame.is_some());
    ui.set_frame_width(state.frame_width as i32);
    ui.set_frame_height(state.frame_height as i32);
    game.set_frame_ready(state.frame.is_some());
    game.set_fit_scaling(state.config.scale_mode == ScaleMode::Fit);
    game.set_frame_width(state.frame_width as i32);
    game.set_frame_height(state.frame_height as i32);
    game.set_status(state.status.clone().into());
    game.set_muted(state.config.muted);
    game.set_fullscreen(state.fullscreen);
    if let Some(frame) = &state.frame {
        ui.set_game_frame(frame.clone());
        game.set_game_frame(frame.clone());
    }
    ui.set_menu_items(ModelRc::new(VecModel::from(parse_menu(
        state.menu.as_deref().unwrap_or(""),
    ))));
    if !running {
        let _ = game.hide();
    }
}

fn sync_library(ui: &MainWindow, state: &Rc<RefCell<AppState>>) {
    let mut state = state.borrow_mut();
    let query = state.search.to_ascii_lowercase();
    let mut entries = state
        .config
        .library
        .iter()
        .filter(|item| {
            (query.is_empty() || item.title.to_ascii_lowercase().contains(&query))
                && match state.filter {
                    Filter::All => true,
                    Filter::Favorites => item.favorite,
                    Filter::Recent => item.last_played.is_some(),
                }
        })
        .cloned()
        .collect::<Vec<_>>();
    entries.sort_by(|lhs, rhs| match state.sort {
        Sort::Title => lhs
            .title
            .to_ascii_lowercase()
            .cmp(&rhs.title.to_ascii_lowercase()),
        Sort::Recent => rhs.last_played.cmp(&lhs.last_played),
        Sort::Added => rhs.added_at.cmp(&lhs.added_at),
    });
    let games: Vec<GameItem> = entries
        .into_iter()
        .map(|item| {
            let cover = item
                .cover
                .as_ref()
                .and_then(|path| Image::load_from_path(path).ok());
            let needs_entrypoint = item.entrypoint.as_os_str().is_empty();
            let source_path = if needs_entrypoint {
                item.project_root.clone()
            } else {
                item.source_path()
            };
            let missing = !needs_entrypoint && !source_path.is_file();
            GameItem {
                id: item.id.to_string().into(),
                title: item.title.into(),
                path: source_path.display().to_string().into(),
                cover: cover.clone().unwrap_or_default(),
                has_cover: cover.is_some(),
                favorite: item.favorite,
                missing,
                needs_entrypoint,
            }
        })
        .collect();
    ui.set_games(ModelRc::new(VecModel::from(games)));
    state.library_dirty = false;
}

fn toggle_detached(ui: &MainWindow, game: &GameWindow, state: &Rc<RefCell<AppState>>) {
    let (detached, fullscreen) = {
        let mut state = state.borrow_mut();
        state.detached = !state.detached;
        (state.detached, state.fullscreen)
    };
    ui.set_detached(detached);
    if detached {
        ui.window().set_fullscreen(false);
        game.window().set_fullscreen(fullscreen);
        let _ = game.show();
    } else {
        let _ = game.hide();
        game.window().set_fullscreen(false);
        ui.window().set_fullscreen(fullscreen);
    }
}

fn indexed_image(width: u32, height: u32, indexed: &[u8]) -> Image {
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
    for (target, color) in pixels.make_mut_slice().iter_mut().zip(indexed) {
        let [r, g, b] = templeos_compat::graphics::palette_rgb(*color);
        *target = Rgba8Pixel { r, g, b, a: 255 };
    }
    Image::from_rgba8(pixels)
}

fn append_line(target: &mut String, text: &str) {
    target.push_str(text);
    if !text.ends_with('\n') {
        target.push('\n');
    }
}

fn parse_menu(source: &str) -> Vec<MenuItem> {
    let mut result = Vec::new();
    let mut rest = source;
    while let Some(open) = rest.find('{') {
        let title = rest[..open].split_whitespace().last().unwrap_or("Menu");
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break;
        };
        let body = &after[..close];
        let mut added = false;
        for declaration in body
            .split(';')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let label = declaration.split('(').next().unwrap_or(declaration).trim();
            let key = menu_key(declaration);
            result.push(MenuItem {
                label: format!("{title} · {label}").into(),
                command: key.map_or_else(SharedString::default, |(ch, scan)| {
                    format!("{ch}:{scan}").into()
                }),
                enabled: key.is_some(),
            });
            added = true;
        }
        if !added {
            result.push(MenuItem {
                label: title.into(),
                command: SharedString::default(),
                enabled: false,
            });
        }
        rest = &after[close + 1..];
    }
    result
}

fn menu_key(declaration: &str) -> Option<(i64, i64)> {
    if declaration.contains("CH_SHIFT_ESC") {
        Some((0x1c, 1 << 9))
    } else if declaration.contains("CH_ESC") {
        Some((0x1b, 0))
    } else if declaration.contains("SC_CURSOR_UP") {
        Some((0, 0x48))
    } else if declaration.contains("SC_CURSOR_DOWN") {
        Some((0, 0x50))
    } else if declaration.contains("SC_CURSOR_LEFT") {
        Some((0, 0x4b))
    } else if declaration.contains("SC_CURSOR_RIGHT") {
        Some((0, 0x4d))
    } else {
        None
    }
}

fn decode_menu_command(command: &str) -> Option<(i64, i64)> {
    let (ch, scan) = command.split_once(':')?;
    Some((ch.parse().ok()?, scan.parse().ok()?))
}

fn text_key_events(
    text: &str,
    shift: bool,
    ctrl: bool,
    alt: bool,
) -> impl Iterator<Item = (i64, i64)> + '_ {
    text.chars()
        .filter_map(move |ch| ascii_key_event(ch, shift, ctrl, alt))
}

fn key_text(key: Key) -> SharedString {
    key.into()
}

fn map_slint_special_key(text: &str, shift: bool, ctrl: bool, alt: bool) -> Option<(i64, i64)> {
    let flags = scan_flags(shift, ctrl, alt);
    let is = |key| key_text(key).as_str() == text;
    if is(Key::Escape) {
        Some((if shift { 0x1c } else { 0x1b }, 0x01 | flags))
    } else if is(Key::Tab) || is(Key::Backtab) {
        Some((b'\t' as i64, 0x0f | flags))
    } else if is(Key::Backspace) {
        Some((0x08, 0x0e | flags))
    } else if is(Key::Return) {
        Some((b'\n' as i64, 0x1c | flags))
    } else if is(Key::Space) {
        Some((if shift { 0x1f } else { b' ' as i64 }, 0x39 | flags))
    } else if is(Key::Home) {
        Some((0, 0x47 | flags))
    } else if is(Key::UpArrow) {
        Some((0, 0x48 | flags))
    } else if is(Key::PageUp) {
        Some((0, 0x49 | flags))
    } else if is(Key::LeftArrow) {
        Some((0, 0x4b | flags))
    } else if is(Key::RightArrow) {
        Some((0, 0x4d | flags))
    } else if is(Key::End) {
        Some((0, 0x4f | flags))
    } else if is(Key::DownArrow) {
        Some((0, 0x50 | flags))
    } else if is(Key::PageDown) {
        Some((0, 0x51 | flags))
    } else if is(Key::Insert) {
        Some((0, 0x52 | flags))
    } else if is(Key::Delete) {
        Some((0, 0x53 | flags))
    } else if is(Key::F1) {
        Some((0, 0x3b | flags))
    } else if is(Key::F2) {
        Some((0, 0x3c | flags))
    } else if is(Key::F3) {
        Some((0, 0x3d | flags))
    } else if is(Key::F4) {
        Some((0, 0x3e | flags))
    } else if is(Key::F5) {
        Some((0, 0x3f | flags))
    } else if is(Key::F6) {
        Some((0, 0x40 | flags))
    } else if is(Key::F7) {
        Some((0, 0x41 | flags))
    } else if is(Key::F8) {
        Some((0, 0x42 | flags))
    } else if is(Key::F9) {
        Some((0, 0x43 | flags))
    } else if is(Key::F10) {
        Some((0, 0x44 | flags))
    } else if is(Key::F11) {
        Some((0, 0x57 | flags))
    } else if is(Key::F12) {
        Some((0, 0x58 | flags))
    } else {
        None
    }
}

fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for item in fs::read_dir(source)? {
        let item = item?;
        let target = destination.join(item.file_name());
        if item.file_type()?.is_dir() {
            copy_tree(&item.path(), &target)?;
        } else {
            fs::copy(item.path(), target)?;
        }
    }
    Ok(())
}

fn tree_manifest(root: &Path) -> std::io::Result<Vec<(PathBuf, u64)>> {
    fn visit(root: &Path, current: &Path, files: &mut Vec<(PathBuf, u64)>) -> std::io::Result<()> {
        for item in fs::read_dir(current)? {
            let item = item?;
            if item.file_type()?.is_dir() {
                visit(root, &item.path(), files)?;
            } else {
                files.push((
                    item.path()
                        .strip_prefix(root)
                        .unwrap_or(&item.path())
                        .to_path_buf(),
                    item.metadata()?.len(),
                ));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn directory_is_writable(directory: &Path) -> bool {
    let probe = directory.join(format!(".sanctum-write-test-{}", Uuid::new_v4()));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => fs::remove_file(probe).is_ok(),
        Err(_) => false,
    }
}

fn migrate_storage(paths: &StoragePaths, portable: bool) -> std::io::Result<()> {
    let destination = if portable {
        paths.executable_dir.join("SanctumData")
    } else {
        directories::ProjectDirs::from_path(PathBuf::from("Sanctum"))
            .ok_or_else(|| std::io::Error::other("user data directory unavailable"))?
            .data_local_dir()
            .to_path_buf()
    };
    if destination != paths.root {
        let staging = destination.with_extension(format!("migrating-{}", Uuid::new_v4()));
        let previous = destination.with_extension("previous");
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        copy_tree(&paths.root, &staging)?;
        if tree_manifest(&paths.root)? != tree_manifest(&staging)? {
            let _ = fs::remove_dir_all(&staging);
            return Err(std::io::Error::other("copied data failed verification"));
        }
        if previous.exists() {
            fs::remove_dir_all(&previous)?;
        }
        if destination.exists() {
            fs::rename(&destination, &previous)?;
        }
        if let Err(error) = fs::rename(&staging, &destination) {
            if previous.exists() && !destination.exists() {
                let _ = fs::rename(&previous, &destination);
            }
            return Err(error);
        }
    }
    let marker = paths.executable_dir.join("portable.mode");
    if portable {
        fs::write(marker, b"portable\n")
    } else if marker.exists() {
        fs::remove_file(marker)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_templeos_special_keys_and_modifiers() {
        assert_eq!(
            map_slint_special_key(key_text(Key::Escape).as_str(), false, false, false),
            Some((0x1b, 0x01))
        );
        assert_eq!(
            map_slint_special_key(key_text(Key::Escape).as_str(), true, false, false),
            Some((0x1c, 0x01 | SCF_SHIFT))
        );
        assert_eq!(
            map_slint_special_key(key_text(Key::Return).as_str(), false, true, false),
            Some((b'\n' as i64, 0x1c | SCF_CTRL))
        );
        assert_eq!(
            map_slint_special_key(key_text(Key::Space).as_str(), false, false, true),
            Some((b' ' as i64, 0x39 | SCF_ALT))
        );
        assert_eq!(
            map_slint_special_key(key_text(Key::UpArrow).as_str(), true, true, true),
            Some((0, 0x48 | (7 << 9)))
        );
        assert_eq!(
            map_slint_special_key(key_text(Key::F12).as_str(), false, false, true),
            Some((0, 0x58 | SCF_ALT))
        );
    }

    #[test]
    fn maps_ascii_text_and_common_punctuation() {
        let mapped: Vec<_> =
            text_key_events("HolyC!?[]{};:'\".,/\\`~ Ω", false, false, false).collect();
        assert_eq!(mapped.last(), Some(&(b' ' as i64, 0x39)));
        assert_eq!(mapped.len(), 22);
        assert!(mapped.contains(&(b'?' as i64, 0x35 | SCF_SHIFT)));
        assert!(mapped.contains(&(b'\\' as i64, 0x2b)));
        assert!(mapped.contains(&(b'~' as i64, 0x29 | SCF_SHIFT)));
        assert!(mapped.contains(&(b'H' as i64, 0x23 | SCF_SHIFT)));
    }

    #[test]
    fn parses_supported_menu_actions() {
        let menu = parse_menu("File { Exit(CH_ESC); } Play { Up(SC_CURSOR_UP); Other(); }");
        assert_eq!(menu.len(), 3);
        assert!(menu[0].enabled);
        assert_eq!(
            decode_menu_command(menu[0].command.as_str()),
            Some((0x1b, 0))
        );
        assert!(!menu[2].enabled);
    }
}

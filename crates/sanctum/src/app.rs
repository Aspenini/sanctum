use crate::model::{self, LibraryEntry, SanctumConfig, ScaleMode, StorageMode, StoragePaths};
use crate::runner::{RunnerEvent, RunnerSession};
use eframe::egui::{self, Color32, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
#[cfg(test)]
use templeos_compat::host::input::{SCF_ALT, SCF_CTRL, SCF_SHIFT};
use templeos_compat::host::input::{ascii_key_event, scan_flags};
use uuid::Uuid;

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

enum InstallEvent {
    Progress(u64, u64, PathBuf),
    Done(Result<PathBuf, String>),
}

pub struct SanctumApp {
    paths: StoragePaths,
    config: SanctumConfig,
    filter: Filter,
    sort: Sort,
    search: String,
    selected: Option<Uuid>,
    entry_choices: Option<(PathBuf, Vec<PathBuf>, Option<Uuid>)>,
    runner: Option<RunnerSession>,
    running: Option<Uuid>,
    status: String,
    log: String,
    frame: Option<TextureHandle>,
    frame_size: [usize; 2],
    frame_sequence: u64,
    focus_canvas_on_frame: bool,
    menu: Option<String>,
    covers: HashMap<Uuid, TextureHandle>,
    show_settings: bool,
    fullscreen: bool,
    pending_restart: Option<Uuid>,
    pending_storage: Option<StorageMode>,
    install_rx: Option<mpsc::Receiver<InstallEvent>>,
    install_cancel: Option<Arc<AtomicBool>>,
    install_progress: Option<(u64, u64, String)>,
}

impl SanctumApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);
        let paths = StoragePaths::detect().expect("Sanctum data directory is unavailable");
        let (mut config, recovery) = model::load_recovering(&paths).unwrap_or_default();
        config.storage_mode = if paths.portable {
            StorageMode::Portable
        } else {
            StorageMode::PerUser
        };
        if config.templeos_root.is_none() && paths.templeos.join("Kernel/KernelA.HH").is_file() {
            config.templeos_root = Some(paths.templeos.clone());
        }
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
            status: recovery.map_or_else(
                || "Ready".into(),
                |path| format!("Recovered a corrupt library database to {}", path.display()),
            ),
            log: String::new(),
            frame: None,
            frame_size: [640, 480],
            frame_sequence: 0,
            focus_canvas_on_frame: false,
            menu: None,
            covers: HashMap::new(),
            show_settings: false,
            fullscreen: false,
            pending_restart: None,
            pending_storage: None,
            install_rx: None,
            install_cancel: None,
            install_progress: None,
        }
    }

    fn persist(&mut self) {
        if let Err(error) = model::save(&self.paths, &self.config) {
            self.status = format!("Could not save library: {error}");
        }
    }

    fn add_source(&mut self, path: &Path) {
        match LibraryEntry::from_source(path) {
            Ok(entry) => {
                if let Some(existing) = self
                    .config
                    .library
                    .iter()
                    .find(|item| item.source_path() == entry.source_path())
                {
                    self.selected = Some(existing.id);
                    self.status = "Program is already in the library".into();
                } else {
                    self.selected = Some(entry.id);
                    self.config.library.push(entry);
                    self.persist();
                }
            }
            Err(error) => self.status = format!("Unable to add program: {error}"),
        }
    }

    fn add_folder_entry(&mut self, root: PathBuf, relative: PathBuf) {
        match LibraryEntry::from_project(&root, &relative) {
            Ok(entry) => {
                if let Some(existing) = self
                    .config
                    .library
                    .iter()
                    .find(|item| item.source_path() == entry.source_path())
                {
                    self.selected = Some(existing.id);
                    self.status = "Program is already in the library".into();
                } else {
                    self.selected = Some(entry.id);
                    self.config.library.push(entry);
                    self.persist();
                }
            }
            Err(error) => self.status = format!("Unable to add program: {error}"),
        }
    }

    fn restart(&mut self, id: Uuid) {
        if let Some(runner) = &mut self.runner {
            self.pending_restart = Some(id);
            let _ = runner.stop();
            self.status = "Restarting".into();
        } else {
            self.launch(id);
        }
    }

    fn launch(&mut self, id: Uuid) {
        let root = self
            .config
            .templeos_root
            .clone()
            .filter(|root| root.join("Kernel/KernelA.HH").is_file());
        let Some(item) = self.config.library.iter_mut().find(|item| item.id == id) else {
            return;
        };
        if !item.source_path().is_file() {
            self.status = "The selected entrypoint is missing".into();
            return;
        }
        if let Some(mut runner) = self.runner.take() {
            runner.kill();
        }
        match RunnerSession::spawn(&item.source_path(), root.as_deref(), &self.paths.root) {
            Ok(runner) => {
                item.last_played = Some(model::now());
                self.running = Some(id);
                self.runner = Some(runner);
                self.status = "Starting".into();
                self.log.clear();
                self.frame = None;
                self.frame_sequence = 0;
                self.focus_canvas_on_frame = true;
                self.menu = None;
                self.persist();
                if let Some(runner) = &self.runner {
                    let _ = runner.set_muted(self.config.muted);
                }
            }
            Err(error) => self.status = format!("Unable to start runner: {error}"),
        }
    }

    fn poll_runner(&mut self, ctx: &egui::Context) {
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
                    self.log.push_str(&text);
                    if !text.ends_with('\n') {
                        self.log.push('\n');
                    }
                    self.status = "Running with warnings".into();
                }
                RunnerEvent::Error(text) => {
                    self.log.push_str(&text);
                    if !text.ends_with('\n') {
                        self.log.push('\n');
                    }
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
                    self.frame_size = [width as usize, height as usize];
                    let pixels = indexed
                        .iter()
                        .map(|color| {
                            let [r, g, b] = templeos_compat::graphics::palette_rgb(*color);
                            Color32::from_rgb(r, g, b)
                        })
                        .collect();
                    let image = egui::ColorImage::new(self.frame_size, pixels);
                    if let Some(texture) = &mut self.frame {
                        texture.set(image, TextureOptions::NEAREST);
                    } else {
                        self.frame = Some(ctx.load_texture(
                            "templeos-frame",
                            image,
                            TextureOptions::NEAREST,
                        ));
                    }
                    self.capture_cover(width, height, &indexed);
                }
            }
        }
        if let Some(runner) = &mut self.runner {
            match runner.update_lifecycle() {
                Ok(true) => ctx.request_repaint_after(std::time::Duration::from_millis(16)),
                Ok(false) => {
                    self.runner = None;
                    self.running = None;
                    if let Some(id) = self.pending_restart.take() {
                        self.launch(id);
                    }
                }
                Err(error) => {
                    self.status = format!("Runner error: {error}");
                    self.runner = None;
                    self.running = None;
                    if let Some(id) = self.pending_restart.take() {
                        self.launch(id);
                    }
                }
            }
        }
    }

    fn capture_cover(&mut self, width: u32, height: u32, indexed: &[u8]) {
        let Some(id) = self.running else { return };
        let Some(item) = self.config.library.iter_mut().find(|item| item.id == id) else {
            return;
        };
        if item.cover.is_some()
            || indexed
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len()
                < 5
        {
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
            self.persist();
        }
    }

    fn poll_install(&mut self) {
        let mut messages = Vec::new();
        if let Some(rx) = &self.install_rx {
            while let Ok(message) = rx.try_recv() {
                messages.push(message);
            }
        }
        for message in messages {
            match message {
                InstallEvent::Progress(done, total, path) => {
                    self.install_progress = Some((done, total, path.display().to_string()));
                }
                InstallEvent::Done(result) => {
                    self.install_progress = None;
                    self.install_rx = None;
                    self.install_cancel = None;
                    match result {
                        Ok(root) => {
                            self.config.templeos_root = Some(root);
                            self.status = "TempleOS installed".into();
                            self.persist();
                        }
                        Err(error) => self.status = format!("ISO import failed: {error}"),
                    }
                }
            }
        }
    }

    fn install_iso(&mut self, iso: PathBuf) {
        if self.install_rx.is_some() {
            return;
        }
        let root = self.paths.templeos.clone();
        let (tx, rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let extraction_cancelled = cancelled.clone();
        self.install_rx = Some(rx);
        self.install_cancel = Some(cancelled);
        self.status = "Inspecting TempleOS ISO".into();
        std::thread::spawn(move || {
            let staging = root.with_extension("installing");
            let previous = root.with_extension("previous");
            let _ = fs::remove_dir_all(&staging);
            let result =
                tos_redsea::extract_iso(&iso, &staging, &extraction_cancelled, |progress| {
                    let _ = tx.send(InstallEvent::Progress(
                        progress.files_done,
                        progress.files_total,
                        progress.path,
                    ));
                })
                .map_err(|error| error.to_string())
                .and_then(|info| {
                    if !staging.join("Kernel/KernelA.HH").is_file() {
                        return Err("ISO did not contain Kernel/KernelA.HH".into());
                    }
                    let manifest = serde_json::json!({
                        "version": 1,
                        "iso_sha256": info.sha256,
                        "iso_size": info.image_size,
                        "filesystem_offset": info.filesystem_offset,
                        "file_count": info.file_count,
                        "imported_at": model::now(),
                    });
                    fs::write(
                        staging.join("sanctum-install.json"),
                        serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
                    )
                    .map_err(|error| error.to_string())?;
                    let _ = fs::remove_dir_all(&previous);
                    if root.exists() {
                        fs::rename(&root, &previous).map_err(|error| error.to_string())?;
                    }
                    fs::rename(&staging, &root).map_err(|error| error.to_string())?;
                    Ok(root)
                });
            if result.is_err() {
                let _ = fs::remove_dir_all(&staging);
            }
            let _ = tx.send(InstallEvent::Done(result));
        });
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
                    self.covers.remove(&id);
                    self.persist();
                }
            }
            Err(error) => self.status = format!("Unable to import cover: {error}"),
        }
    }

    fn send_input(&mut self, ctx: &egui::Context) {
        let Some(runner) = &self.runner else { return };
        let active_modifiers = ctx.input(|input| input.modifiers);
        let events = ctx.input(|input| input.events.clone());
        let has_text = events
            .iter()
            .any(|event| matches!(event, egui::Event::Text(text) if !text.is_empty()));
        let text_has_space = events
            .iter()
            .any(|event| matches!(event, egui::Event::Text(text) if text.contains(' ')));
        for event in events {
            match event {
                egui::Event::Text(text) => {
                    for (ch, scan) in text_key_events(
                        &text,
                        active_modifiers.shift,
                        active_modifiers.ctrl,
                        active_modifiers.alt,
                    ) {
                        let _ = runner.send_key(ch, scan);
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let special = if key == egui::Key::Space && text_has_space {
                        None
                    } else {
                        map_special_key(key, modifiers.shift, modifiers.ctrl, modifiers.alt)
                    };
                    if let Some((ch, scan)) = special.or_else(|| {
                        if !has_text {
                            map_printable_key(key, modifiers.shift, modifiers.ctrl, modifiers.alt)
                        } else {
                            None
                        }
                    }) {
                        let _ = runner.send_key(ch, scan);
                    }
                }
                _ => {}
            }
        }
    }

    fn library_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("SANCTUM");
            ui.add_space(12.0);
            ui.text_edit_singleline(&mut self.search);
            if ui.button("Add .HC").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("HolyC", &["HC", "hc"])
                    .pick_file()
            {
                self.add_source(&path);
            }
            if ui.button("Add Folder").clicked()
                && let Some(root) = rfd::FileDialog::new().pick_folder()
            {
                match model::discover_holyc(&root) {
                    Ok(files) if files.len() == 1 => self.add_folder_entry(root, files[0].clone()),
                    Ok(files) if !files.is_empty() => {
                        self.entry_choices = Some((root, files, None))
                    }
                    Ok(_) => self.status = "No .HC files found in that folder".into(),
                    Err(error) => self.status = format!("Unable to scan folder: {error}"),
                }
            }
            if ui.button("Settings").clicked() {
                self.show_settings = true;
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.filter, Filter::All, "All");
            ui.selectable_value(&mut self.filter, Filter::Favorites, "Favorites");
            ui.selectable_value(&mut self.filter, Filter::Recent, "Recent");
            ui.separator();
            egui::ComboBox::from_id_salt("sort")
                .selected_text(match self.sort {
                    Sort::Title => "Title",
                    Sort::Recent => "Recently played",
                    Sort::Added => "Recently added",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.sort, Sort::Title, "Title");
                    ui.selectable_value(&mut self.sort, Sort::Recent, "Recently played");
                    ui.selectable_value(&mut self.sort, Sort::Added, "Recently added");
                });
        });
        ui.add_space(8.0);

        let query = self.search.to_ascii_lowercase();
        let mut ids = self
            .config
            .library
            .iter()
            .filter(|item| {
                (query.is_empty() || item.title.to_ascii_lowercase().contains(&query))
                    && match self.filter {
                        Filter::All => true,
                        Filter::Favorites => item.favorite,
                        Filter::Recent => item.last_played.is_some(),
                    }
            })
            .map(|item| item.id)
            .collect::<Vec<_>>();
        ids.sort_by(|lhs, rhs| {
            let lhs = self
                .config
                .library
                .iter()
                .find(|item| item.id == *lhs)
                .unwrap();
            let rhs = self
                .config
                .library
                .iter()
                .find(|item| item.id == *rhs)
                .unwrap();
            match self.sort {
                Sort::Title => lhs
                    .title
                    .to_ascii_lowercase()
                    .cmp(&rhs.title.to_ascii_lowercase()),
                Sort::Recent => rhs.last_played.cmp(&lhs.last_played),
                Sort::Added => rhs.added_at.cmp(&lhs.added_at),
            }
        });
        let columns = (ui.available_width() / 230.0).floor().max(1.0) as usize;
        egui::Grid::new("library-grid")
            .num_columns(columns)
            .spacing([12.0, 16.0])
            .show(ui, |ui| {
                for (index, id) in ids.into_iter().enumerate() {
                    let item = self
                        .config
                        .library
                        .iter()
                        .find(|item| item.id == id)
                        .unwrap()
                        .clone();
                    ui.vertical(|ui| {
                        let response = if let Some(texture) = self.cover_texture(ui.ctx(), &item) {
                            ui.add(
                                egui::Button::new(egui::Image::new((
                                    texture.id(),
                                    egui::vec2(200.0, 150.0),
                                )))
                                .frame(true),
                            )
                        } else {
                            ui.add_sized([200.0, 150.0], egui::Button::new("HOLY\nC"))
                        };
                        if response.clicked() {
                            self.selected = Some(id);
                        }
                        ui.horizontal(|ui| {
                            if ui.button(if item.favorite { "★" } else { "☆" }).clicked()
                                && let Some(item) =
                                    self.config.library.iter_mut().find(|item| item.id == id)
                            {
                                item.favorite = !item.favorite;
                                self.persist();
                            }
                            if ui.button("▶").clicked() {
                                self.launch(id);
                            }
                            ui.label(&item.title);
                        });
                        if !item.source_path().is_file() {
                            ui.colored_label(Color32::LIGHT_RED, "Missing");
                        }
                    });
                    if (index + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
    }

    fn cover_texture(&mut self, ctx: &egui::Context, item: &LibraryEntry) -> Option<TextureHandle> {
        if let Some(texture) = self.covers.get(&item.id) {
            return Some(texture.clone());
        }
        let path = item.cover.as_ref()?;
        let image = image::open(path).ok()?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        let texture = ctx.load_texture(format!("cover-{}", item.id), color, TextureOptions::LINEAR);
        self.covers.insert(item.id, texture.clone());
        Some(texture)
    }

    fn runner_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("← Library").clicked() {
                self.pending_restart = None;
                if let Some(runner) = &mut self.runner {
                    let _ = runner.stop();
                }
            }
            if ui.button("Stop").clicked()
                && let Some(runner) = &mut self.runner
            {
                self.pending_restart = None;
                let _ = runner.stop();
                self.status = "Stopping".into();
            }
            if ui.button("Restart").clicked()
                && let Some(id) = self.running
            {
                self.restart(id);
            }
            if ui.checkbox(&mut self.config.muted, "Mute").changed() {
                if let Some(runner) = &self.runner {
                    let _ = runner.set_muted(self.config.muted);
                }
                self.persist();
            }
            if ui
                .button(if self.fullscreen {
                    "Windowed"
                } else {
                    "Fullscreen"
                })
                .clicked()
            {
                self.fullscreen = !self.fullscreen;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            ui.label(&self.status);
        });
        if let Some(menu) = self.menu.clone() {
            menu_bar(ui, &menu, |ch, scan| {
                if let Some(runner) = &self.runner {
                    let _ = runner.send_key(ch, scan);
                }
            });
        }
        ui.separator();
        let mut canvas_has_focus = false;
        if let Some(texture) = &self.frame {
            let available = ui.available_size();
            let native = egui::vec2(self.frame_size[0] as f32, self.frame_size[1] as f32);
            let ratio = (available.x / native.x).min(available.y / native.y);
            let scale = match self.config.scale_mode {
                ScaleMode::Integer => ratio.floor().max(1.0),
                ScaleMode::Fit => ratio.max(0.1),
            };
            let size = native * scale;
            let response = ui
                .push_id("sanctum-canvas", |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add(
                            egui::Image::new((texture.id(), size))
                                .texture_options(TextureOptions::NEAREST)
                                .sense(egui::Sense::click()),
                        )
                    })
                    .inner
                })
                .inner;
            if response.clicked() || self.focus_canvas_on_frame {
                response.request_focus();
                self.focus_canvas_on_frame = false;
            }
            canvas_has_focus = response.has_focus();
        } else {
            ui.centered_and_justified(|ui| {
                ui.spinner();
            });
        }
        if !self.log.is_empty() {
            egui::CollapsingHeader::new("Compiler and program log").show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        ui.monospace(&self.log);
                    });
            });
        }
        if canvas_has_focus {
            self.send_input(ui.ctx());
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some((root, files, editing)) = self.entry_choices.clone() {
            egui::Window::new("Choose HolyC entrypoint")
                .collapsible(false)
                .show(ctx, |ui| {
                    for file in files {
                        if ui.button(file.display().to_string()).clicked() {
                            if let Some(id) = editing {
                                if let Some(item) =
                                    self.config.library.iter_mut().find(|item| item.id == id)
                                {
                                    item.project_root = root.clone();
                                    item.entrypoint = file;
                                    self.persist();
                                }
                            } else {
                                self.add_folder_entry(root.clone(), file);
                            }
                            self.entry_choices = None;
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.entry_choices = None;
                    }
                });
        }
        if let Some(id) = self.selected {
            let mut open = true;
            egui::Window::new("Program details")
                .open(&mut open)
                .show(ctx, |ui| {
                    let mut changed = false;
                    if let Some(item) = self.config.library.iter_mut().find(|item| item.id == id) {
                        ui.label(item.source_path().display().to_string());
                        changed |= ui.text_edit_singleline(&mut item.title).changed();
                        changed |= ui.checkbox(&mut item.favorite, "Favorite").changed();
                    }
                    if ui.button("Change entrypoint…").clicked()
                        && let Some(item) = self.config.library.iter().find(|item| item.id == id)
                    {
                        match model::discover_holyc(&item.project_root) {
                            Ok(files) if !files.is_empty() => {
                                self.entry_choices =
                                    Some((item.project_root.clone(), files, Some(id)));
                            }
                            Ok(_) => self.status = "No .HC files found in the project".into(),
                            Err(error) => self.status = format!("Unable to scan project: {error}"),
                        }
                    }
                    if ui.button("Choose cover…").clicked() {
                        self.import_cover(id);
                    }
                    if ui.button("Run").clicked() {
                        self.launch(id);
                    }
                    if ui.button("Remove from library").clicked() {
                        self.config.library.retain(|item| item.id != id);
                        self.selected = None;
                        changed = true;
                    }
                    if changed {
                        self.persist();
                    }
                });
            if !open {
                self.selected = None;
            }
        }
        if self.show_settings {
            let mut open = true;
            egui::Window::new("Sanctum settings")
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(format!("Data: {}", self.paths.root.display()));
                    ui.label(format!(
                        "Mode: {}",
                        if self.paths.portable {
                            "Portable"
                        } else {
                            "Per-user"
                        }
                    ));
                    if let Some(root) = self
                        .config
                        .templeos_root
                        .as_ref()
                        .filter(|root| root.join("Kernel/KernelA.HH").is_file())
                    {
                        ui.label(format!("TempleOS files: {}", root.display()));
                    } else {
                        ui.label("TempleOS files: not installed (optional for self-contained programs)");
                    }
                    if ui.button("Import TempleOS ISO…").clicked()
                        && let Some(iso) = rfd::FileDialog::new()
                            .add_filter("TempleOS ISO", &["ISO", "iso"])
                            .pick_file()
                    {
                        self.install_iso(iso);
                    }
                    if ui.button("Use extracted TempleOS folder…").clicked()
                        && let Some(root) = rfd::FileDialog::new().pick_folder()
                    {
                        if root.join("Kernel/KernelA.HH").is_file() {
                            self.config.templeos_root = Some(root);
                            self.persist();
                        } else {
                            self.status = "That folder is not a TempleOS root".into();
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Scaling");
                        let changed = ui.selectable_value(
                            &mut self.config.scale_mode,
                            ScaleMode::Integer,
                            "Sharp integer",
                        ).changed() | ui.selectable_value(
                            &mut self.config.scale_mode,
                            ScaleMode::Fit,
                            "Fit window",
                        ).changed();
                        if changed {
                            self.persist();
                        }
                    });
                    let target = if self.paths.portable {
                        StorageMode::PerUser
                    } else {
                        StorageMode::Portable
                    };
                    let portable_available = self.paths.portable
                        || directory_is_writable(&self.paths.executable_dir);
                    let response = ui.add_enabled(
                        portable_available,
                        egui::Button::new(if self.paths.portable {
                            "Switch to per-user storage"
                        } else {
                            "Switch to portable storage"
                        }),
                    );
                    if response.clicked() {
                        self.pending_storage = Some(target);
                    }
                    if !portable_available {
                        response.on_disabled_hover_text(
                            "The executable directory is not writable, so portable mode is unavailable.",
                        );
                    }
                    if let Some((done, total, path)) = &self.install_progress {
                        ui.add(
                            egui::ProgressBar::new(*done as f32 / (*total).max(1) as f32)
                                .text(path),
                        );
                        if ui.button("Cancel installation").clicked()
                            && let Some(cancelled) = &self.install_cancel
                        {
                            cancelled.store(true, Ordering::Release);
                        }
                    }
                });
            self.show_settings = open;
        }
        if let Some(target) = self.pending_storage {
            egui::Window::new("Move Sanctum data?")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(match target {
                        StorageMode::Portable => {
                            "Sanctum-owned settings, covers, saves, and TempleOS files will be copied beside the executable. Imported project folders remain in place."
                        }
                        StorageMode::PerUser => {
                            "Sanctum-owned data will be copied to this account's application-data directory. Imported project folders remain in place."
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.pending_storage = None;
                        }
                        if ui.button("Move data and restart").clicked() {
                            self.config.storage_mode = target;
                            self.persist();
                            match migrate_storage(&self.paths, target == StorageMode::Portable) {
                                Ok(()) => {
                                    match std::env::current_exe()
                                        .and_then(|executable| std::process::Command::new(executable).spawn())
                                    {
                                        Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                                        Err(error) => self.status = format!("Data moved, but restart failed: {error}"),
                                    }
                                }
                                Err(error) => {
                                    self.config.storage_mode = if self.paths.portable {
                                        StorageMode::Portable
                                    } else {
                                        StorageMode::PerUser
                                    };
                                    self.status = format!("Storage migration failed: {error}");
                                }
                            }
                            self.pending_storage = None;
                        }
                    });
                });
        }
    }
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

fn map_printable_key(key: egui::Key, shift: bool, ctrl: bool, alt: bool) -> Option<(i64, i64)> {
    let ch = match key {
        egui::Key::A => 'a',
        egui::Key::B => 'b',
        egui::Key::C => 'c',
        egui::Key::D => 'd',
        egui::Key::E => 'e',
        egui::Key::F => 'f',
        egui::Key::G => 'g',
        egui::Key::H => 'h',
        egui::Key::I => 'i',
        egui::Key::J => 'j',
        egui::Key::K => 'k',
        egui::Key::L => 'l',
        egui::Key::M => 'm',
        egui::Key::N => 'n',
        egui::Key::O => 'o',
        egui::Key::P => 'p',
        egui::Key::Q => 'q',
        egui::Key::R => 'r',
        egui::Key::S => 's',
        egui::Key::T => 't',
        egui::Key::U => 'u',
        egui::Key::V => 'v',
        egui::Key::W => 'w',
        egui::Key::X => 'x',
        egui::Key::Y => 'y',
        egui::Key::Z => 'z',
        egui::Key::Num0 => '0',
        egui::Key::Num1 => '1',
        egui::Key::Num2 => '2',
        egui::Key::Num3 => '3',
        egui::Key::Num4 => '4',
        egui::Key::Num5 => '5',
        egui::Key::Num6 => '6',
        egui::Key::Num7 => '7',
        egui::Key::Num8 => '8',
        egui::Key::Num9 => '9',
        egui::Key::Colon => ':',
        egui::Key::Comma => ',',
        egui::Key::Backslash | egui::Key::IntlBackslash => '\\',
        egui::Key::Slash => '/',
        egui::Key::Pipe => '|',
        egui::Key::Questionmark => '?',
        egui::Key::Exclamationmark => '!',
        egui::Key::OpenBracket => '[',
        egui::Key::CloseBracket => ']',
        egui::Key::OpenCurlyBracket => '{',
        egui::Key::CloseCurlyBracket => '}',
        egui::Key::Backtick => '`',
        egui::Key::Minus => '-',
        egui::Key::Period => '.',
        egui::Key::Plus => '+',
        egui::Key::Equals => '=',
        egui::Key::Semicolon => ';',
        egui::Key::Quote => '\'',
        _ => return None,
    };
    ascii_key_event(ch, shift, ctrl, alt)
}

fn map_special_key(key: egui::Key, shift: bool, ctrl: bool, alt: bool) -> Option<(i64, i64)> {
    let flags = scan_flags(shift, ctrl, alt);
    match key {
        egui::Key::Escape if shift => Some((0x1c, 0x01 | flags)),
        egui::Key::Escape => Some((0x1b, 0x01 | flags)),
        egui::Key::Tab => Some((b'\t' as i64, 0x0f | flags)),
        egui::Key::Backspace => Some((0x08, 0x0e | flags)),
        egui::Key::Enter => Some((b'\n' as i64, 0x1c | flags)),
        egui::Key::Space if shift => Some((0x1f, 0x39 | flags)),
        egui::Key::Space => Some((b' ' as i64, 0x39 | flags)),
        egui::Key::Home => Some((0, 0x47 | flags)),
        egui::Key::ArrowUp => Some((0, 0x48 | flags)),
        egui::Key::PageUp => Some((0, 0x49 | flags)),
        egui::Key::ArrowDown => Some((0, 0x50 | flags)),
        egui::Key::PageDown => Some((0, 0x51 | flags)),
        egui::Key::Insert => Some((0, 0x52 | flags)),
        egui::Key::Delete => Some((0, 0x53 | flags)),
        egui::Key::ArrowLeft => Some((0, 0x4b | flags)),
        egui::Key::ArrowRight => Some((0, 0x4d | flags)),
        egui::Key::End => Some((0, 0x4f | flags)),
        egui::Key::F1 => Some((0, 0x3b | flags)),
        egui::Key::F2 => Some((0, 0x3c | flags)),
        egui::Key::F3 => Some((0, 0x3d | flags)),
        egui::Key::F4 => Some((0, 0x3e | flags)),
        egui::Key::F5 => Some((0, 0x3f | flags)),
        egui::Key::F6 => Some((0, 0x40 | flags)),
        egui::Key::F7 => Some((0, 0x41 | flags)),
        egui::Key::F8 => Some((0, 0x42 | flags)),
        egui::Key::F9 => Some((0, 0x43 | flags)),
        egui::Key::F10 => Some((0, 0x44 | flags)),
        egui::Key::F11 => Some((0, 0x57 | flags)),
        egui::Key::F12 => Some((0, 0x58 | flags)),
        egui::Key::ShiftLeft | egui::Key::ShiftRight => Some((0, 0x2a | flags)),
        egui::Key::ControlLeft | egui::Key::ControlRight => Some((0, 0x1d | flags)),
        egui::Key::AltLeft | egui::Key::AltRight => Some((0, 0x38 | flags)),
        _ => None,
    }
}

impl eframe::App for SanctumApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_runner(&ctx);
        self.poll_install();
        egui::CentralPanel::default().show(ui, |ui| {
            if self.runner.is_some() {
                self.runner_ui(ui);
            } else {
                self.library_ui(ui);
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.label(&self.status);
            });
        });
        self.dialogs(&ctx);
    }

    fn on_exit(&mut self) {
        if let Some(runner) = &mut self.runner {
            runner.kill();
        }
        self.persist();
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(9, 20, 31);
    visuals.window_fill = Color32::from_rgb(15, 32, 47);
    visuals.selection.bg_fill = Color32::from_rgb(30, 125, 145);
    visuals.hyperlink_color = Color32::from_rgb(85, 255, 255);
    ctx.set_visuals(visuals);
}

fn menu_bar(ui: &mut egui::Ui, source: &str, mut send: impl FnMut(i64, i64)) {
    egui::MenuBar::new().ui(ui, |ui| {
        let mut rest = source;
        while let Some(open) = rest.find('{') {
            let title = rest[..open].split_whitespace().last().unwrap_or("Menu");
            let after = &rest[open + 1..];
            let Some(close) = after.find('}') else { break };
            let body = &after[..close];
            ui.menu_button(title, |ui| {
                for declaration in body
                    .split(';')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                {
                    let label = declaration.split('(').next().unwrap_or(declaration).trim();
                    let key = if declaration.contains("CH_SHIFT_ESC") {
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
                    };
                    if ui
                        .add_enabled(key.is_some(), egui::Button::new(label))
                        .clicked()
                        && let Some((ch, scan)) = key
                    {
                        send(ch, scan);
                        ui.close();
                    }
                }
            });
            rest = &after[close + 1..];
        }
    });
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
            map_special_key(egui::Key::Escape, false, false, false),
            Some((0x1b, 0x01))
        );
        assert_eq!(
            map_special_key(egui::Key::Escape, true, false, false),
            Some((0x1c, 0x01 | SCF_SHIFT))
        );
        assert_eq!(
            map_special_key(egui::Key::Enter, false, true, false),
            Some((b'\n' as i64, 0x1c | SCF_CTRL))
        );
        assert_eq!(
            map_special_key(egui::Key::Space, false, false, true),
            Some((b' ' as i64, 0x39 | SCF_ALT))
        );
        assert_eq!(
            map_special_key(egui::Key::ArrowUp, true, true, true),
            Some((0, 0x48 | (7 << 9)))
        );
        assert_eq!(
            map_special_key(egui::Key::ArrowDown, false, false, false),
            Some((0, 0x50))
        );
        assert_eq!(
            map_special_key(egui::Key::ArrowLeft, false, false, false),
            Some((0, 0x4b))
        );
        assert_eq!(
            map_special_key(egui::Key::ArrowRight, false, false, false),
            Some((0, 0x4d))
        );
        assert_eq!(
            map_special_key(egui::Key::Home, false, true, false),
            Some((0, 0x47 | SCF_CTRL))
        );
        assert_eq!(
            map_special_key(egui::Key::Delete, true, false, false),
            Some((0, 0x53 | SCF_SHIFT))
        );
        assert_eq!(
            map_special_key(egui::Key::F12, false, false, true),
            Some((0, 0x58 | SCF_ALT))
        );
        assert_eq!(
            map_special_key(egui::Key::Space, true, false, false),
            Some((0x1f, 0x39 | SCF_SHIFT))
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
    fn maps_printable_keys_with_modifiers() {
        assert_eq!(
            map_printable_key(egui::Key::C, false, true, false),
            Some((3, 0x2e | SCF_CTRL))
        );
        assert_eq!(
            map_printable_key(egui::Key::Z, false, false, true),
            Some((b'z' as i64, 0x2c | SCF_ALT))
        );
        assert_eq!(
            map_printable_key(egui::Key::Questionmark, false, false, true),
            Some((b'?' as i64, 0x35 | SCF_SHIFT | SCF_ALT))
        );
    }
}

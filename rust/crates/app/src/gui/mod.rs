//! The Professional window (egui): sidebar, dashboard, asset workspace, operation pages with
//! live output and an answer box, and the config.ini editor.

mod app_ui;
mod flags;
mod log;
mod screenshot;
mod settings;
mod theme;
mod widgets;

use std::path::Path;
use std::sync::{mpsc, Arc};

use eframe::egui::{self, vec2, Color32, RichText};
use habbo_core::hotels::{self, HabboHotel};
use habbo_core::workspace::{self, runtime as workspace_runtime, AssetWorkspaceFolder, AssetWorkspacePaths, WorkspaceSettingsStore};

use crate::args::RunMode;
use crate::catalog::{self, OperationCategory, OperationDefinition};
use crate::runner::{OperationResult, OperationRunner, RunnerEvent};
use log::LogState;
use theme::{Fixed, Palette};
use widgets::{card, chip, dot, primary_button, secondary_button};

/// Operations that read or write the asset workspace when one is connected.
const WORKSPACE_OPERATION_IDS: &[&str] = &[
    "habbo.furnidata", "nitro.furniture", "nitro.clothes", "tools.decompile-nitro", "tools.compile-nitro",
    "tools.decompile-hab", "tools.compile-hab", "tools.decompile-swf", "tools.swf-furniture-nitro",
    "tools.swf-furniture-hab", "tools.nitro-furniture-hab", "tools.hab-furniture-32", "tools.swf-clothes-nitro",
    "tools.swf-clothes-hab", "tools.nitro-clothes-hab", "tools.swf-pets-nitro", "tools.swf-pets-hab",
    "tools.nitro-pets-hab", "tools.swf-effects-nitro", "tools.swf-effects-hab", "tools.nitro-effects-hab",
    "database.offer-id", "database.item-settings", "database.sprite-id",
];

/// Sidebar label, caption, icon and colour of each module.
const MODULES: &[(OperationCategory, &str, &str, &str, &str)] = &[
    (OperationCategory::HabboOriginal, "Habbo Original", "Official downloads", "⬇", "#4F5FE6"),
    (OperationCategory::NitroCustom, "Nitro Custom", "Custom imports", "⭐", "#8B5CF6"),
    (OperationCategory::HotelTools, "Hotel Tools", "Conversion tools", "🔧", "#0E9F9E"),
    (OperationCategory::Database, "Database", "Maintenance jobs", "🗄", "#E08A0B"),
];

pub fn run(runtime: tokio::runtime::Handle, show_selector: bool) {
    let size = if show_selector { [860.0, 440.0] } else { [1380.0, 860.0] };
    let min = if show_selector { [760.0, 420.0] } else { [1040.0, 700.0] };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("All-in-1 Converter")
        .with_inner_size(size)
        .with_min_inner_size(min);
    if let Ok(icon) = image::load_from_memory(include_bytes!("../../resources/app.ico")) {
        let icon = icon.into_rgba8();
        let (width, height) = icon.dimensions();
        viewport = viewport.with_icon(egui::IconData { rgba: icon.into_raw(), width, height });
    }
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };
    let result = eframe::run_native(
        "All-in-1 Converter",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::System);
            Ok(Box::new(ProApp::new(runtime, show_selector)))
        }),
    );
    if let Err(error) = result {
        eprintln!("[gui] the window could not start: {error}");
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Dashboard,
    Workspace,
    Category(OperationCategory),
    Settings,
}

enum Dialog {
    Workspace(&'static OperationDefinition),
    Destructive(&'static OperationDefinition),
    LeaveSettings(Box<dyn FnOnce(&mut ProApp)>),
    SwitchInterface,
}

struct RunHistoryItem {
    title: &'static str,
    succeeded: bool,
    finished_at: chrono::DateTime<chrono::Local>,
}

struct WorkspaceView {
    root_path: String,
    status: String,
    paths: Option<AssetWorkspacePaths>,
    folders: Vec<AssetWorkspaceFolder>,
    busy: Option<mpsc::Receiver<Result<(AssetWorkspacePaths, Vec<AssetWorkspaceFolder>), String>>>,
}

pub struct ProApp {
    runner: OperationRunner,
    selector: bool,
    page: Page,
    selected: Option<&'static OperationDefinition>,
    events: Option<mpsc::Receiver<RunnerEvent>>,
    log: LogState,
    is_running: bool,
    has_finished_run: bool,
    last_run_succeeded: bool,
    status_text: String,
    input_text: String,
    recent_runs: Vec<RunHistoryItem>,
    hotel: Option<HabboHotel>,
    hotel_known: bool,
    settings: settings::SettingsPage,
    workspace: WorkspaceView,
    dialog: Option<Dialog>,
    focus_input: bool,
    /// Set by `start`; the next frame launches it (the repaint hook needs the egui context).
    pending_start: Option<&'static OperationDefinition>,
    running_title: Option<&'static str>,
}

impl ProApp {
    fn new(runtime: tokio::runtime::Handle, selector: bool) -> Self {
        let store_root = WorkspaceSettingsStore::new(&habbo_core::ini::default_config_path())
            .ok()
            .and_then(|s| s.load_root().ok().flatten())
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let mut app = Self {
            runner: OperationRunner::new(runtime),
            selector,
            page: Page::Dashboard,
            selected: None,
            events: None,
            log: LogState::default(),
            is_running: false,
            has_finished_run: false,
            last_run_succeeded: false,
            status_text: "Ready".into(),
            input_text: String::new(),
            recent_runs: Vec::new(),
            hotel: None,
            hotel_known: false,
            settings: settings::SettingsPage::new(),
            workspace: WorkspaceView {
                root_path: store_root,
                status: "Choose your nitro-assets folder".into(),
                paths: None,
                folders: Vec::new(),
                busy: None,
            },
            dialog: None,
            focus_input: false,
            pending_start: None,
            running_title: None,
        };
        app.refresh_hotel();
        app
    }

    fn refresh_hotel(&mut self) {
        match habbo_core::config::ConfigIniFile::load(None) {
            Ok(file) => {
                self.hotel = hotels::detect(|key| file.get(key));
                self.hotel_known = true;
            }
            Err(_) => {
                self.hotel = None;
                self.hotel_known = false;
            }
        }
    }

    // ------------------------------------------------------------ navigation

    fn title(&self) -> (&'static str, &'static str) {
        match self.page {
            Page::Dashboard => ("Dashboard", "Your Habbo asset workstation at a glance"),
            Page::Workspace => ("Asset Workspace", "Connect the converter directly to your Nitro asset folders"),
            Page::Settings => ("Settings", "Edit config.ini: Habbo hotel, downloads, Nitro retro and database"),
            Page::Category(OperationCategory::HabboOriginal) => ("Habbo Original", "Download official assets from the Habbo CDN"),
            Page::Category(OperationCategory::NitroCustom) => ("Nitro Custom", "Import custom Nitro furniture and clothing"),
            Page::Category(OperationCategory::HotelTools) => ("Hotel Tools", "Merge, (de)compile and convert to .nitro / .hab (json + WebP Lossless)"),
            Page::Category(OperationCategory::Database) => ("Database", "Inspect and maintain the configured hotel database"),
            Page::Category(OperationCategory::General) => ("About", "Version and project information"),
        }
    }

    /// Goes to a page, asking first when the settings have unsaved edits.
    fn navigate(&mut self, page: Page) {
        if self.is_running {
            return;
        }
        if self.page == Page::Settings && page == Page::Settings {
            return;
        }
        if self.page == Page::Settings && self.settings.is_dirty() {
            self.dialog = Some(Dialog::LeaveSettings(Box::new(move |app: &mut ProApp| {
                app.settings.reload();
                app.go(page);
            })));
            return;
        }
        self.go(page);
    }

    fn go(&mut self, page: Page) {
        self.page = page;
        match page {
            Page::Category(category) => {
                self.selected = catalog::for_category(category).first().copied();
                self.clear_log();
            }
            Page::Settings => {
                self.selected = None;
                self.settings.reload();
            }
            Page::Workspace => {
                self.selected = None;
                if !self.workspace.root_path.trim().is_empty() && self.workspace.paths.is_none() {
                    self.refresh_workspace(false);
                }
            }
            Page::Dashboard => self.selected = None,
        }
    }

    fn open_operation(&mut self, operation: &'static OperationDefinition) {
        if self.is_running {
            return;
        }
        self.navigate(Page::Category(operation.category));
        if self.page == Page::Category(operation.category) {
            self.selected = Some(operation);
        }
    }

    // ------------------------------------------------------------ running

    fn clear_log(&mut self) {
        if self.is_running {
            return;
        }
        self.log.clear();
        self.has_finished_run = false;
        self.status_text = "Ready".into();
    }

    fn request_run(&mut self) {
        let Some(operation) = self.selected else { return };
        if self.is_running {
            return;
        }
        if workspace_runtime::router().is_configured() && WORKSPACE_OPERATION_IDS.contains(&operation.id) {
            self.dialog = Some(Dialog::Workspace(operation));
            return;
        }
        if operation.is_destructive {
            self.dialog = Some(Dialog::Destructive(operation));
            return;
        }
        self.start(operation);
    }

    fn start(&mut self, operation: &'static OperationDefinition) {
        self.log.clear();
        self.has_finished_run = false;
        self.is_running = true;
        self.status_text = format!("Running: {}", operation.title);
        self.focus_input = true;
        self.events = None;
        // The repaint closure is installed on the first frame (it needs the egui context).
        self.pending_start = Some(operation);
    }

    fn drain_events(&mut self) {
        let Some(events) = &self.events else { return };
        let mut finished: Option<OperationResult> = None;
        let mut chunk = String::new();
        while let Ok(event) = events.try_recv() {
            match event {
                RunnerEvent::Output(text) => chunk.push_str(&text),
                RunnerEvent::Color(color) => {
                    // Text so far keeps the previous colour.
                    self.log.append(&std::mem::take(&mut chunk));
                    self.log.set_color(color);
                }
                RunnerEvent::Finished(result) => finished = Some(result),
            }
        }
        self.log.append(&chunk);
        if let Some(result) = finished {
            self.events = None;
            self.is_running = false;
            self.has_finished_run = true;
            self.last_run_succeeded = result.succeeded;
            let title = self.running_title.unwrap_or("Operation");
            self.status_text = if result.succeeded {
                format!("{title} completed in {}", duration(result.elapsed))
            } else if result.cancelled {
                format!("{title} cancelled after {}", duration(result.elapsed))
            } else {
                format!("{title} failed: {}", result.error.unwrap_or_default())
            };
            self.recent_runs.insert(0, RunHistoryItem { title, succeeded: result.succeeded, finished_at: result.finished_at });
            self.recent_runs.truncate(8);
        }
    }

    fn submit_input(&mut self) {
        if !self.is_running {
            return;
        }
        let value = self.input_text.trim().to_string();
        self.input_text.clear();
        self.log.append(&format!("{}\n", if value.is_empty() { "(Enter: default)" } else { value.as_str() }));
        self.runner.submit_input(value);
    }

    // ------------------------------------------------------------ workspace

    fn refresh_workspace(&mut self, save: bool) {
        let root = self.workspace.root_path.trim().to_string();
        if root.is_empty() {
            self.workspace.status = "Choose your nitro-assets folder".into();
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.workspace.busy = Some(receiver);
        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                let paths = AssetWorkspacePaths::from_root(&root).map_err(|e| e.to_string())?;
                if !paths.root.is_dir() {
                    return Err(format!("Folder not found: {}", paths.root.display()));
                }
                if save {
                    WorkspaceSettingsStore::new(&habbo_core::ini::default_config_path())
                        .and_then(|s| s.save_root(&paths.root.to_string_lossy()))
                        .map_err(|e| e.to_string())?;
                }
                let folders = workspace::inspect(&paths);
                Ok((paths, folders))
            })();
            let _ = sender.send(result);
        });
    }

    fn poll_workspace(&mut self) {
        let Some(receiver) = &self.workspace.busy else { return };
        let Ok(result) = receiver.try_recv() else { return };
        self.workspace.busy = None;
        match result {
            Ok((paths, folders)) => {
                let existing = folders.iter().filter(|f| f.exists).count();
                let total: usize = folders.iter().map(|f| f.file_count).sum();
                self.workspace.root_path = paths.root.display().to_string();
                self.workspace.status = format!("{existing}/5 folders ready · {} files", habbo_core::console::n0(total as i64));
                workspace_runtime::set(Some(paths.clone()));
                self.workspace.paths = Some(paths);
                self.workspace.folders = folders;
            }
            Err(message) => {
                self.workspace.paths = None;
                self.workspace.folders.clear();
                self.workspace.status = message;
                workspace_runtime::set(None);
            }
        }
        self.status_text = self.workspace.status.clone();
    }
}

// Fields used only to hand data between frames.
impl ProApp {
    fn take_pending_start(&mut self, ctx: &egui::Context) {
        if let Some(operation) = self.pending_start.take() {
            let ctx = ctx.clone();
            let wake: Arc<dyn Fn() + Send + Sync> =
                Arc::new(move || ctx.request_repaint_after(std::time::Duration::from_millis(100)));
            self.running_title = Some(operation.title);
            self.events = Some(self.runner.start(operation, wake));
        }
    }
}

fn duration(time: std::time::Duration) -> String {
    let seconds = time.as_secs_f64();
    if seconds < 60.0 { format!("{seconds:.1} s") } else { format!("{} min {} s", (seconds / 60.0) as u64, time.as_secs() % 60) }
}

/// `Process.Start(UseShellExecute)`: opens a file or folder with the system handler.
pub fn open_path(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer").arg(path).spawn().map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn().map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn().map(|_| ())
    }
}

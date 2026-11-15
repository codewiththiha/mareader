//! Mareader's desktop entry point: the window, then `mareader_ui_egui` runs.
#![deny(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use mareader_engine::command::Command;
use mareader_engine::documents::OpenRequest;
use mareader_engine::host::{Machine, Shared};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::Sent;

use desk::Desk;
use dialogs::Shell;
use inbox::Inbox;
use single_instance::Claim;

mod desk;
mod dialogs;
mod inbox;
mod open_files;
mod reduced_motion;
mod single_instance;
mod system;
#[cfg(any(test, all(target_os = "macos", feature = "traffic-lights")))]
mod traffic_lights;

/// The app plus the window glue only a desktop binary can do.
struct Desktop {
    app: MareaderApp,
    inbox: Inbox,
}

/// Opens the newest of `paths`, as mareader does; false if none.
fn open_latest(app: &mut MareaderApp, paths: Vec<PathBuf>) -> bool {
    let Some(path) = paths.into_iter().last() else {
        return false;
    };
    app.queue(Sent::Engine(Command::Open(OpenRequest::path(path))));
    true
}

impl eframe::App for Desktop {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        self.app.clear_color(visuals)
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let mail = self.inbox.take();
        if mail.raise {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        open_latest(&mut self.app, mail.paths);
        self.app.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
        #[cfg(all(target_os = "macos", feature = "traffic-lights"))]
        {
            let fullscreen = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
            let (lights, zoom) = (self.app.native_lights(), ui.ctx().zoom_factor());
            let spots = traffic_lights::place(frame, lights, fullscreen, zoom);
            self.app.set_light_spots(spots);
        }
    }

    fn on_exit(&mut self) {
        self.app.flush();
    }
}

fn viewport() -> egui::ViewportBuilder {
    let v = egui::ViewportBuilder::default()
        .with_title("Mareader")
        .with_app_id("mareader")
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size([640.0, 480.0]);
    // macOS shows the bundle's icon; elsewhere the window and taskbar take it.
    #[cfg(not(target_os = "macos"))]
    let v = match eframe::icon_data::from_png_bytes(include_bytes!("../icons/128x128@2x.png")) {
        Ok(icon) => v.with_icon(icon),
        Err(_) => v,
    };
    // macOS keeps its traffic lights; elsewhere the app draws captions.
    if cfg!(target_os = "macos") {
        v.with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
    } else {
        v.with_decorations(false)
    }
}

/// Restarts with macOS malloc's large-block cache off; it kept freed models.
#[cfg(target_os = "macos")]
fn without_large_cache() {
    use std::os::unix::process::CommandExt;
    const KEY: &str = "MallocLargeCache";
    // Set already, by us or the user: theirs wins.
    if std::env::var_os(KEY).is_some() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    // libmalloc reads it only at start, so the process runs again.
    let args = std::env::args_os().skip(1);
    let failed = std::process::Command::new(exe)
        .args(args)
        .env(KEY, "0")
        .exec();
    eprintln!("mareader: kept malloc's large cache: {failed}");
}

fn main() -> eframe::Result {
    #[cfg(target_os = "macos")]
    without_large_cache();
    let files: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .filter(|a| !open_files::is_launch_noise(a))
        .map(PathBuf::from)
        .collect();
    let inbox = Inbox::default();
    if let Some(dir) = desk::state_dir()
        && single_instance::claim(&dir, &files, &inbox) == Claim::Forwarded
    {
        return Ok(());
    }
    // `mareader book.pdf` opens it; Finder's files join it before launch.
    inbox.post(files, false);
    #[cfg(all(target_os = "macos", feature = "open-files"))]
    open_files::install(inbox.clone());
    let options = eframe::NativeOptions {
        viewport: viewport(),
        renderer: eframe::Renderer::Wgpu,
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "mareader",
        options,
        Box::new(|cc| {
            // The machine arrives here, and the app is built on it.
            let host: Shared = Arc::new(Desk::new(cc.egui_ctx.clone()));
            let machine = Machine {
                ai_helper: dev_helper(),
                ..Machine::under(&*host)
            };
            // The machine's dialogs: the front end asks, this app runs them.
            let mut app = MareaderApp::new(cc, host, machine).with_dialogs(Arc::new(Shell));
            inbox.wake_with(cc.egui_ctx.clone());
            // A file asked for at launch, else what was read last time.
            if !open_latest(&mut app, inbox.take().paths) {
                app.resume();
            }
            reduced_motion::watch(app.reduced_motion(), cc.egui_ctx.clone());
            Ok(Box::new(Desktop { app, inbox }))
        }),
    )
}

/// Debug builds also find the helper `tools/fm-helper.sh` builds.
fn dev_helper() -> Option<PathBuf> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/helper/FMBridge");
    cfg!(debug_assertions).then(|| PathBuf::from(path))
}

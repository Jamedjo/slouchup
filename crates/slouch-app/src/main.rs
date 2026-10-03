mod art;
mod camera_view;
mod config;
mod engine;
mod frames;
mod notifier;
mod ui;

use std::sync::{Arc, Mutex};

use dioxus::desktop::{Config, WindowBuilder, WindowCloseBehaviour, icon_from_memory};

use crate::camera_view::FrameSlot;
use crate::config::{APP_NAME, cache_dir};
use crate::engine::Engine;
use crate::notifier::Notifier;

fn main() {
    let args = config::parse_args();
    let cache = cache_dir();
    let files = art::write_files(&cache).expect("writing artwork to the cache directory");
    if args.test_notification {
        Notifier::new(files).nag("You're 20% closer to the screen than usual.");
        return;
    }
    let _lock = match single_instance(&cache) {
        Some(lock) => lock,
        None => {
            eprintln!("{APP_NAME} is already running");
            std::process::exit(1);
        }
    };

    let (command_tx, command_rx) = crossbeam_channel::unbounded();
    let (view_tx, view_rx) = futures_channel::mpsc::unbounded();
    let preview = FrameSlot::default();
    if let Err(error) = Engine::start(
        args.camera,
        preview.clone(),
        Notifier::new(files.clone()),
        view_tx,
        command_rx,
    ) {
        tracing::error!("{error}");
        Notifier::new(files).info("Slouch can't start 😿", &error);
    }
    let bridge = ui::Bridge {
        commands: command_tx,
        views: Arc::new(Mutex::new(Some(view_rx))),
        start_with_game: args.game,
        preview,
    };

    let window = WindowBuilder::new()
        .with_title(APP_NAME)
        .with_visible(args.show || args.game)
        .with_inner_size(dioxus::desktop::LogicalSize::new(680.0, 600.0));
    let desktop = Config::new()
        .with_window(window)
        .with_menu(None)
        .with_icon(icon_from_memory(&art::icon_png(art::Mood::Rainbow, 128)).expect("icon decodes"))
        .with_close_behaviour(WindowCloseBehaviour::WindowHides)
        .with_exits_when_last_window_closes(false);
    dioxus::LaunchBuilder::desktop()
        .with_cfg(desktop)
        .with_context(bridge)
        .launch(ui::App);
}

fn single_instance(cache: &std::path::Path) -> Option<std::fs::File> {
    let lock = std::fs::File::create(cache.join("lock")).ok()?;
    lock.try_lock().ok()?;
    Some(lock)
}

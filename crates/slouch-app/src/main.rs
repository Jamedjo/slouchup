// Without this, Windows opens a console window alongside the tray app.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod art;
mod camera_view;
mod config;
mod demo;
mod engine;
mod finder;
mod frames;
mod notifier;
mod onboarding;
mod screens;
mod settings;
mod source;
mod style;
mod ui;
#[cfg(windows)]
mod windows_shell;

use std::sync::{Arc, Mutex};

use dioxus::desktop::{WindowBuilder, WindowCloseBehaviour};

use crate::art::Theme;
use crate::camera_view::FrameSlot;
use crate::config::{APP_NAME, cache_dir};
use crate::engine::{Command, Engine};
use crate::notifier::Notifier;
use crate::source::Source;

fn main() {
    let args = config::parse_args();
    let cache = cache_dir();
    let files = art::write_files(&cache).expect("writing artwork to the cache directory");
    if args.test_notification {
        Notifier::new(files, Arc::new(|| {})).nag("You're 20% closer to the screen than usual.");
        return;
    }
    // The demo runs alongside the real app, so screenshots don't mean quitting it.
    let _lock = if args.demo {
        None
    } else {
        match single_instance(&cache) {
            Some(lock) => Some(lock),
            None => {
                eprintln!("{APP_NAME} is already running");
                std::process::exit(1);
            }
        }
    };

    let (command_tx, command_rx) = crossbeam_channel::unbounded();
    // The first run keeps the camera off until its welcome says to turn it on.
    let onboarding = !args.demo && !config::has_thresholds();
    if onboarding {
        let _ = command_tx.send(Command::Pause(true));
    }
    let (view_tx, view_rx) = futures_channel::mpsc::unbounded();
    let preview = FrameSlot::default();
    if !args.demo
        && let Err(error) = config::adopt_old_settings()
    {
        tracing::warn!("couldn't copy settings from before the rename: {error}");
    }
    let preferences = config::load_preferences();
    let (source, settings) = if args.demo {
        (Source::Demo, posture::Settings::default())
    } else {
        // `--camera N` picks by position, as OpenCV numbers cameras; otherwise the saved choice.
        let camera = match args.camera {
            Some(index) => match source::list_cameras().get(index) {
                Some(camera) => Some(camera.id.clone()),
                None => {
                    eprintln!("No camera {index}; using the saved choice instead");
                    preferences.camera.clone()
                }
            },
            None => preferences.camera.clone(),
        };
        (
            Source::Camera(camera),
            preferences.settings(config::load_thresholds()),
        )
    };
    let snooze = command_tx.clone();
    Engine::start(
        source,
        settings,
        preview.clone(),
        files,
        Arc::new(move || {
            let _ = snooze.send(Command::Snooze(true));
        }),
        view_tx,
        command_rx,
    );
    let bridge = ui::Bridge {
        commands: command_tx,
        views: Arc::new(Mutex::new(Some(view_rx))),
        start_with_game: args.game,
        persist: !args.demo,
        start_with_settings: args.settings,
        preview,
        onboarding,
    };

    let window = WindowBuilder::new()
        .with_title(APP_NAME)
        .with_visible(args.show || onboarding)
        .with_inner_size(dioxus::desktop::LogicalSize::new(680.0, 600.0));
    let desktop = ui::window_config(window, Theme::Night)
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

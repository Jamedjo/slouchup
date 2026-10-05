// Without this, Windows opens a console window alongside the tray app.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod art;
mod camera_picker;
mod camera_view;
mod config;
mod demo;
mod engine;
mod finder;
mod frames;
mod history;
mod history_window;
#[cfg(any(windows, target_os = "linux"))]
mod installed;
#[cfg(target_os = "linux")]
mod launcher;
mod notifier;
mod onboarding;
mod screens;
mod settings;
mod source;
mod still_running;
mod style;
mod ui;
#[cfg(windows)]
mod windows_shell;

use std::sync::{Arc, Mutex};

use dioxus::desktop::{WindowBuilder, WindowCloseBehaviour};

use crate::art::Theme;
use crate::camera_view::FrameSlot;
use crate::config::{APP_NAME, cache_dir};
use crate::engine::{Command, Engine, Links};
use crate::notifier::Notifier;
use crate::source::Source;

static QUIT: std::sync::OnceLock<crossbeam_channel::Sender<Command>> = std::sync::OnceLock::new();

/// End the app from anywhere: the engine saves the history and exits, and if it's stuck or
/// gone, the app exits anyway shortly after.
pub fn quit() {
    #[cfg(any(windows, target_os = "linux"))]
    installed::update_on_quit();
    if let Some(commands) = QUIT.get() {
        let _ = commands.send(Command::Quit);
    }
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(2));
        std::process::exit(0);
    });
}

fn main() {
    #[cfg(any(windows, target_os = "linux"))]
    installed::run_installer_step();
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

    #[cfg(any(windows, target_os = "linux"))]
    if !args.demo {
        installed::keep_up_to_date();
    }

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
    #[cfg(target_os = "linux")]
    if !args.demo
        && let Some(launcher) = launcher::Launcher::from_env()
    {
        launcher.start();
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
    let history = Arc::new(Mutex::new(if args.demo {
        demo::sample_history(chrono::Local::now())
    } else {
        config::load_history()
    }));
    let links = Links {
        preview: preview.clone(),
        files: files.clone(),
        on_snooze: Arc::new(move || {
            let _ = snooze.send(Command::Snooze(true));
        }),
        history: history.clone(),
        events: view_tx,
        commands: command_rx,
    };
    Engine::start(source, settings, links);
    let _ = QUIT.set(command_tx.clone());
    let bridge = ui::Bridge {
        commands: command_tx,
        views: Arc::new(Mutex::new(Some(view_rx))),
        start_with_game: args.game,
        persist: !args.demo,
        start_on: if args.settings {
            ui::Page::Settings
        } else if args.history {
            ui::Page::History
        } else {
            ui::Page::Camera
        },
        history,
        preview,
        onboarding,
        files,
    };

    let window = WindowBuilder::new()
        .with_title(APP_NAME)
        .with_visible(args.show || args.settings || args.history || onboarding)
        // Tall enough for a 4:3 camera's picture to fill the window's width, and short enough
        // for a 1366x768 laptop's screen.
        .with_inner_size(dioxus::desktop::LogicalSize::new(680.0, 684.0))
        .with_min_inner_size(dioxus::desktop::LogicalSize::new(560.0, 480.0));
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

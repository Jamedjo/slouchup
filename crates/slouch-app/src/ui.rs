//! The tray, the camera window, and the full-screen "look here" prompt used by the calibration game.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Sender;
use dioxus::desktop::tao::monitor::MonitorHandle;
use dioxus::desktop::tao::window::Fullscreen;
use dioxus::desktop::trayicon::menu::{Menu, MenuItem, PredefinedMenuItem};
use dioxus::desktop::trayicon::{Icon, TrayIcon};
use dioxus::desktop::{
    Config, DesktopContext, WindowBuilder, use_muda_event_handler, use_tray_menu_event_handler,
    window,
};
use dioxus::prelude::*;
use futures_channel::mpsc::UnboundedReceiver;
use futures_util::StreamExt;

use crate::art::{self, Mood};
use crate::camera_view::{CameraView, FrameSlot};
use crate::engine::{Banner, Command, FaceView, LookAt, View};
use crate::screens;
use crate::settings::{SettingsHandle, SettingsPage, SettingsPageProps};

const STYLE: &str = include_str!("style.css");

/// Channels between the UI and the engine, handed to the app through context.
#[derive(Clone)]
pub struct Bridge {
    pub commands: Sender<Command>,
    pub views: Arc<Mutex<Option<UnboundedReceiver<View>>>>,
    pub start_with_game: bool,
    pub preview: FrameSlot,
    /// Whether settings and calibration are saved; demos leave them alone.
    pub persist: bool,
    pub start_with_settings: bool,
}

impl Bridge {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

struct Tray {
    icon: TrayIcon,
    status: MenuItem,
    pause: MenuItem,
    icons: [(Mood, Icon); 3],
    /// What's showing, since setting the icon goes over D-Bus and the engine updates 5 times a second.
    shown: std::cell::RefCell<(Option<Mood>, String)>,
}

impl Tray {
    fn new() -> Self {
        let icon = |mood| {
            Icon::from_rgba(art::rasterise(&art::icon_svg(mood), 64, 64), 64, 64)
                .expect("icon is 64x64")
        };
        let status = MenuItem::with_id("status", "Starting…", false, None);
        let pause = MenuItem::with_id("pause", "Pause", true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &status,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("show", "Show camera", true, None),
            &MenuItem::with_id("game", "Calibration game", true, None),
            &MenuItem::with_id("recalibrate", "Recalibrate", true, None),
            &MenuItem::with_id("settings", "Settings…", true, None),
            &pause,
            &MenuItem::with_id("quit", "Quit", true, None),
        ])
        .expect("tray menu builds");
        let icons = [
            (Mood::Good, icon(Mood::Good)),
            (Mood::Bad, icon(Mood::Bad)),
            (Mood::Idle, icon(Mood::Idle)),
        ];
        let icon = dioxus::desktop::trayicon::init_tray_icon(menu, Some(icons[2].1.clone()));
        let _ = icon.set_tooltip(Some("Slouch"));
        Self {
            icon,
            status,
            pause,
            icons,
            shown: Default::default(),
        }
    }

    fn show(&self, mood: Mood, status: &str) {
        let mut shown = self.shown.borrow_mut();
        if shown.0 != Some(mood)
            && let Some((_, icon)) = self.icons.iter().find(|(m, _)| *m == mood)
        {
            let _ = self.icon.set_icon(Some(icon.clone()));
            shown.0 = Some(mood);
        }
        if shown.1 != status {
            self.status.set_text(status);
            shown.1 = status.to_string();
        }
    }
}

fn show_main_window() {
    let main = window();
    main.set_visible(true);
    main.set_focus();
}

#[component]
pub fn App() -> Element {
    let bridge = use_context::<Bridge>();
    let mut view = use_signal(View::default);
    let tray = use_hook(|| std::rc::Rc::new(Tray::new()));
    let mut paused = use_signal(|| false);
    let monitors = use_signal(Vec::<MonitorHandle>::new);
    let look_here = use_hook(|| SharedLookAt(Arc::new(Mutex::new(LookAt::default()))));
    // The game's full-screen window: which screen it's for, and the window once it has opened.
    let mut look_here_window = use_signal(|| None::<(Option<usize>, Option<DesktopContext>)>);

    let receiver = use_hook({
        let bridge = bridge.clone();
        move || std::rc::Rc::new(std::cell::RefCell::new(bridge.views.lock().unwrap().take()))
    });
    use_future(move || {
        let receiver = receiver.clone();
        async move {
            let Some(mut views) = receiver.borrow_mut().take() else {
                return;
            };
            while let Some(next) = views.next().await {
                view.set(next);
            }
        }
    });

    let start_game = {
        let bridge = bridge.clone();
        let mut monitors = monitors;
        move || {
            let found: Vec<MonitorHandle> = window().available_monitors().collect();
            let screens = screens::describe(&found);
            monitors.set(found);
            bridge.send(Command::Game { screens });
        }
    };
    let settings_open = use_hook(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));
    let open_settings = {
        let bridge = bridge.clone();
        move || {
            if settings_open.swap(true, std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            let handle = SettingsHandle {
                bridge: bridge.clone(),
                open: settings_open.clone(),
            };
            let dom = VirtualDom::new_with_props(
                SettingsPage,
                SettingsPageProps {
                    handle,
                    initial: view.peek().settings,
                },
            );
            let config = Config::new().with_menu(None).with_window(
                WindowBuilder::new()
                    .with_title("Slouch settings")
                    .with_inner_size(dioxus::desktop::LogicalSize::new(480.0, 640.0)),
            );
            let _ = window().new_window(dom, config);
        }
    };
    use_hook({
        let open_settings = open_settings.clone();
        let wanted = bridge.start_with_settings;
        move || {
            if wanted {
                open_settings();
            }
        }
    });
    use_hook({
        let mut start_game = start_game.clone();
        let wanted = bridge.start_with_game;
        move || {
            if wanted {
                start_game();
            }
        }
    });

    let on_menu = use_hook({
        let bridge = bridge.clone();
        let tray = tray.clone();
        let mut start_game = start_game.clone();
        let open_settings = open_settings.clone();
        move || {
            std::rc::Rc::new(std::cell::RefCell::new(move |id: &str| match id {
                "show" => show_main_window(),
                "settings" => open_settings(),
                "game" => start_game(),
                "recalibrate" => bridge.send(Command::Recalibrate),
                "pause" => {
                    let now_paused = !paused();
                    paused.set(now_paused);
                    tray.pause
                        .set_text(if now_paused { "Resume" } else { "Pause" });
                    bridge.send(Command::Pause(now_paused));
                }
                "quit" => std::process::exit(0),
                _ => {}
            }))
        }
    });
    // Tray menus are muda menus, and dioxus-desktop installs one global muda handler for the
    // menubar and another for the tray; whichever it installs last receives every click.
    use_tray_menu_event_handler({
        let on_menu = on_menu.clone();
        move |event| on_menu.borrow_mut()(event.id().0.as_str())
    });
    use_muda_event_handler(move |event| on_menu.borrow_mut()(event.id().0.as_str()));

    use_effect({
        let tray = tray.clone();
        move || {
            let current = view.read();
            tray.show(current.mood, &current.status);
        }
    });

    // Put each game step full screen, on the screen it wants you to look at.
    use_effect({
        let look_here = look_here.clone();
        let preview = bridge.preview.clone();
        move || {
            let target = view.read().look_at.clone();
            let Some(target) = target else {
                if let Some((_, Some(old))) = look_here_window.take() {
                    old.close();
                }
                return;
            };
            let screen = target.screen;
            *look_here.0.lock().unwrap() = target;
            if look_here_window
                .peek()
                .as_ref()
                .is_some_and(|(open, _)| *open == screen)
            {
                return;
            }
            if let Some((_, Some(old))) = look_here_window.take() {
                old.close();
            }
            let main = window();
            let monitor = match screen {
                Some(index) => monitors.peek().get(index).cloned(),
                None => screens::built_in(&monitors.peek())
                    .or_else(|| main.current_monitor())
                    .or_else(|| main.primary_monitor()),
            };
            let dom = VirtualDom::new_with_props(
                LookHere,
                LookHereProps {
                    step: look_here.clone(),
                    slot: preview.clone(),
                },
            );
            let config = Config::new().with_menu(None).with_window(
                WindowBuilder::new()
                    .with_title("Slouch: calibration game")
                    .with_decorations(false)
                    .with_always_on_top(true)
                    .with_fullscreen(Some(Fullscreen::Borderless(monitor))),
            );
            // Recorded before it opens, since the view updates again before a window is ready.
            look_here_window.set(Some((screen, None)));
            let pending = main.new_window(dom, config);
            spawn(async move {
                let opened = pending.await;
                if look_here_window
                    .peek()
                    .as_ref()
                    .is_some_and(|(s, w)| *s == screen && w.is_none())
                {
                    look_here_window.set(Some((screen, Some(opened))));
                } else {
                    opened.close();
                }
            });
        }
    });

    let preview = bridge.preview.clone();
    let current = view.read().clone();
    let (frame_width, frame_height) = current.frame_size;
    rsx! {
        style { {STYLE} }
        div { class: "app",
            div {
                class: "stage",
                // As wide as fits both the window's width and its height, so the picture keeps its shape.
                style: "aspect-ratio: {frame_width} / {frame_height}; width: min(100%, calc((100vh - 72px) * {frame_width} / {frame_height}))",
                CameraView { slot: preview }
                svg {
                    class: "overlay",
                    view_box: "0 0 {frame_width} {frame_height}",
                    preserve_aspect_ratio: "none",
                    if let Some((line, limit)) = current.lines {
                        line { class: "baseline", x1: "0", x2: "{frame_width}", y1: "{line}", y2: "{line}" }
                        text { class: "baseline-label", x: "8", y: "{line - 6.0}", "baseline" }
                        line { class: "limit", x1: "0", x2: "{frame_width}", y1: "{limit}", y2: "{limit}" }
                        text { class: "limit-label", x: "8", y: "{limit + 16.0}", "slouch" }
                    }
                    if let Some(face) = &current.face {
                        FaceMarks { face: face.clone() }
                    }
                }
                if let Some(reading) = current.reading {
                    div { class: "bars",
                        Bar { name: "drop", ratio: reading.drop / current.settings.thresholds.drop }
                        Bar { name: "lean", ratio: reading.lean / current.settings.thresholds.lean }
                    }
                }
                if let Some(banner) = &current.banner {
                    BannerView { banner: banner.clone() }
                }
                div { class: "status mood-{current.mood:?}", "{current.status}" }
            }
            div { class: "buttons",
                button { onclick: move |_| { let mut start = start_game.clone(); start() }, "Calibration game" }
                button { onclick: move |_| bridge.send(Command::Recalibrate), "Quick recalibrate" }
                button { onclick: move |_| { let open = open_settings.clone(); open() }, "Settings" }
            }
        }
    }
}

#[component]
fn FaceMarks(face: FaceView) -> Element {
    rsx! {
        rect { class: "face", x: "{face.x}", y: "{face.y}", width: "{face.width}", height: "{face.height}" }
        for [x, y] in face.points {
            circle { class: "landmark", cx: "{x}", cy: "{y}", r: "3" }
        }
    }
}

#[component]
fn Bar(name: &'static str, ratio: f32) -> Element {
    let fill = (ratio.clamp(0.0, 1.5) / 1.5 * 100.0).max(0.0);
    let level = if ratio <= 0.5 {
        "ok"
    } else if ratio <= 1.0 {
        "near"
    } else {
        "over"
    };
    rsx! {
        div { class: "bar",
            span { class: "bar-name", "{name}" }
            div { class: "bar-track",
                div { class: "bar-fill {level}", style: "width: {fill}%" }
                div { class: "bar-limit" }
            }
        }
    }
}

#[component]
fn BannerView(banner: Banner) -> Element {
    rsx! {
        div { class: "banner",
            div { class: "banner-title", "{banner.title}" }
            for line in banner.lines {
                div { class: "banner-line", "{line}" }
            }
            if let Some(progress) = banner.progress {
                div { class: "progress", div { class: "progress-fill", style: "width: {progress * 100.0}%" } }
            }
        }
    }
}

/// The current game step, shared with the full-screen window, which has its own virtual DOM.
#[derive(Clone)]
pub struct SharedLookAt(Arc<Mutex<LookAt>>);

impl PartialEq for SharedLookAt {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[component]
fn LookHere(step: SharedLookAt, slot: FrameSlot) -> Element {
    let mut shown = use_signal(LookAt::default);
    use_future(move || {
        let step = step.clone();
        async move {
            loop {
                let latest = step.0.lock().unwrap().clone();
                if *shown.peek() != latest {
                    shown.set(latest);
                }
                futures_timer::Delay::new(Duration::from_millis(100)).await;
            }
        }
    });
    let step = shown.read().clone();
    rsx! {
        style { {STYLE} }
        div { class: "look-here",
            if step.screen.is_some() {
                div { class: "look-eyes", "👀" }
                div { class: "look-title", "Look here" }
            }
            div { class: "look-prompt", "{step.prompt}" }
            div { class: "look-detail", "{step.detail}" }
            div { class: "look-progress",
                div { class: "progress-fill", style: "width: {step.progress * 100.0}%" }
            }
            div { class: "look-camera", CameraView { slot } }
        }
    }
}

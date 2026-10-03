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
use crate::engine::{Banner, Command, FaceView, View};

const STYLE: &str = include_str!("style.css");

/// Channels between the UI and the engine, handed to the app through context.
#[derive(Clone)]
pub struct Bridge {
    pub commands: Sender<Command>,
    pub views: Arc<Mutex<Option<UnboundedReceiver<View>>>>,
    pub start_with_game: bool,
    pub preview: FrameSlot,
}

impl Bridge {
    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

struct Tray {
    icon: TrayIcon,
    status: MenuItem,
    pause: MenuItem,
    icons: [(Mood, Icon); 3],
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
        }
    }

    fn show(&self, mood: Mood, status: &str) {
        if let Some((_, icon)) = self.icons.iter().find(|(m, _)| *m == mood) {
            let _ = self.icon.set_icon(Some(icon.clone()));
        }
        self.status.set_text(status);
    }
}

/// Name each screen by where it sits, like "top-left screen", so the game can say where to look.
fn describe_screens(monitors: &[MonitorHandle]) -> Vec<String> {
    let rects: Vec<(f64, f64, f64, f64)> = monitors
        .iter()
        .map(|m| {
            let (p, s) = (m.position(), m.size());
            (p.x as f64, p.y as f64, s.width as f64, s.height as f64)
        })
        .collect();
    let left = rects.iter().map(|r| r.0).fold(f64::MAX, f64::min);
    let right = rects.iter().map(|r| r.0 + r.2).fold(f64::MIN, f64::max);
    let top = rects.iter().map(|r| r.1).fold(f64::MAX, f64::min);
    let bottom = rects.iter().map(|r| r.1 + r.3).fold(f64::MIN, f64::max);
    let distinct = |values: Vec<f64>| values.iter().any(|v| *v != values[0]);
    let stacked = distinct(rects.iter().map(|r| r.1).collect());
    let side_by_side = distinct(rects.iter().map(|r| r.0).collect());
    rects
        .iter()
        .map(|&(x, y, w, h)| {
            let across = (x + w / 2.0 - left) / (right - left);
            let down = (y + h / 2.0 - top) / (bottom - top);
            let mut parts = Vec::new();
            if stacked {
                parts.push(if down < 0.5 { "top" } else { "bottom" });
            }
            if side_by_side {
                parts.push(if across < 1.0 / 3.0 {
                    "left"
                } else if across > 2.0 / 3.0 {
                    "right"
                } else {
                    "middle"
                });
            }
            format!("{} screen", parts.join("-"))
        })
        .collect()
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
    let look_here = use_hook(|| LookHereText(Arc::new(Mutex::new(String::new()))));
    let mut look_here_window = use_signal(|| None::<(usize, DesktopContext)>);

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
            let screens = describe_screens(&found);
            monitors.set(found);
            show_main_window();
            bridge.send(Command::Game { screens });
        }
    };
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
        move || {
            std::rc::Rc::new(std::cell::RefCell::new(move |id: &str| match id {
                "show" => show_main_window(),
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

    // Put the game's "look here" prompt on the screen it wants you to look at.
    use_effect({
        let look_here = look_here.clone();
        move || {
            let target = view.read().look_at.clone();
            let open = look_here_window.peek().as_ref().map(|(screen, _)| *screen);
            match target {
                Some((screen, text)) => {
                    *look_here.0.lock().unwrap() = text;
                    if open == Some(screen) {
                        return;
                    }
                    if let Some((_, old)) = look_here_window.take() {
                        old.close();
                    }
                    let Some(monitor) = monitors.peek().get(screen).cloned() else {
                        return;
                    };
                    let dom = VirtualDom::new_with_props(
                        LookHere,
                        LookHereProps {
                            text: look_here.clone(),
                        },
                    );
                    let config = Config::new().with_menu(None).with_window(
                        WindowBuilder::new()
                            .with_title("Slouch: look here")
                            .with_decorations(false)
                            .with_always_on_top(true)
                            .with_fullscreen(Some(Fullscreen::Borderless(Some(monitor)))),
                    );
                    let pending = window().new_window(dom, config);
                    spawn(async move {
                        look_here_window.set(Some((screen, pending.await)));
                    });
                }
                None => {
                    if let Some((_, old)) = look_here_window.take() {
                        old.close();
                    }
                }
            }
        }
    });

    let preview = bridge.preview.clone();
    let current = view.read().clone();
    let (frame_width, frame_height) = current.frame_size;
    rsx! {
        style { {STYLE} }
        div { class: "app",
            div { class: "stage", style: "aspect-ratio: {frame_width} / {frame_height}",
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
                        Bar { name: "drop", ratio: reading.drop / current.thresholds.drop }
                        Bar { name: "lean", ratio: reading.lean / current.thresholds.lean }
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

/// Text shared with the separate "look here" window, which has its own virtual DOM.
#[derive(Clone)]
pub struct LookHereText(Arc<Mutex<String>>);

impl PartialEq for LookHereText {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[component]
fn LookHere(text: LookHereText) -> Element {
    let mut shown = use_signal(String::new);
    use_future(move || {
        let text = text.clone();
        async move {
            loop {
                let latest = text.0.lock().unwrap().clone();
                if *shown.peek() != latest {
                    shown.set(latest);
                }
                futures_timer::Delay::new(Duration::from_millis(100)).await;
            }
        }
    });
    let shown = shown.read();
    let mut lines = shown.lines();
    let prompt = lines.next().unwrap_or_default().to_string();
    let detail = lines.next().unwrap_or_default().to_string();
    rsx! {
        style { {STYLE} }
        div { class: "look-here",
            div { class: "look-eyes", "👀" }
            div { class: "look-title", "Look here" }
            div { class: "look-prompt", "{prompt}" }
            div { class: "look-detail", "{detail}" }
        }
    }
}

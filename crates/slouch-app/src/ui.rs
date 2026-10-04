//! The tray, the camera window, and the full-screen "look here" prompt used by the calibration game.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Sender;
use dioxus::desktop::tao::monitor::MonitorHandle;
use dioxus::desktop::tao::window::{Fullscreen, Icon as WindowIcon, Theme as SystemTheme};
use dioxus::desktop::trayicon::menu::{Menu, MenuItem, PredefinedMenuItem};
use dioxus::desktop::trayicon::{Icon, TrayIcon};
use dioxus::desktop::{
    Config, DesktopContext, WindowBuilder, use_muda_event_handler, use_tray_menu_event_handler,
    window,
};
use dioxus::prelude::*;
use futures_channel::mpsc::UnboundedReceiver;
use futures_util::StreamExt;

use crate::art::{self, Mood, Theme};
use crate::camera_view::{CameraView, FrameSlot};
use crate::config::APP_NAME;
use crate::engine::{Banner, Command, LookAt, SNOOZE, View};
use crate::settings::{SettingsHandle, SettingsPage, SettingsPageProps};
use crate::{screens, style};

/// What every window shares: the stylesheet, the app icon, no menu bar, and its theme's ground
/// painted before the page loads, so it doesn't flash white.
pub fn window_config(window: WindowBuilder, theme: Theme) -> Config {
    let size = 128;
    let icon = WindowIcon::from_rgba(art::rasterise(&art::app_icon_svg(), size, size), size, size)
        .expect("icon is square");
    Config::new()
        .with_menu(None)
        .with_window(window)
        .with_icon(icon)
        .with_background_color(theme.ground())
        .with_custom_head(style::head())
}

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
    snooze: MenuItem,
    icons: [(Mood, Icon); 3],
    /// What's showing, since setting the icon goes over D-Bus and the engine updates 5 times a second.
    shown: std::cell::RefCell<(Option<Mood>, String, bool)>,
}

fn snooze_label(snoozed: bool) -> String {
    if snoozed {
        "Stop snoozing".into()
    } else {
        format!("Snooze for {} minutes", SNOOZE.as_secs() / 60)
    }
}

impl Tray {
    fn new(theme: Theme) -> Self {
        let icon = |mood| {
            Icon::from_rgba(art::rasterise(&art::tray_svg(mood, theme), 64, 64), 64, 64)
                .expect("icon is 64x64")
        };
        let status = MenuItem::with_id("status", "Starting…", false, None);
        let pause = MenuItem::with_id("pause", "Pause", true, None);
        let snooze = MenuItem::with_id("snooze", snooze_label(false), true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &status,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("show", "Show camera", true, None),
            &MenuItem::with_id("game", "Calibration game", true, None),
            &MenuItem::with_id("recalibrate", "Recalibrate", true, None),
            &MenuItem::with_id("settings", "Settings…", true, None),
            &snooze,
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
        let _ = icon.set_tooltip(Some(APP_NAME));
        Self {
            icon,
            status,
            pause,
            snooze,
            icons,
            shown: Default::default(),
        }
    }

    fn show(&self, view: &View) {
        let mut shown = self.shown.borrow_mut();
        if shown.0 != Some(view.mood)
            && let Some((_, icon)) = self.icons.iter().find(|(m, _)| *m == view.mood)
        {
            let _ = self.icon.set_icon(Some(icon.clone()));
            shown.0 = Some(view.mood);
        }
        if shown.1 != view.status {
            self.status.set_text(&view.status);
            shown.1 = view.status.clone();
        }
        if shown.2 != view.snoozed {
            self.snooze.set_text(snooze_label(view.snoozed));
            shown.2 = view.snoozed;
        }
    }
}

/// The theme of the panel the tray sits on. GNOME's top bar is dark whatever the desktop theme,
/// and nothing says what a Linux panel looks like, so there it's taken as dark; macOS and Windows
/// panels follow the system theme.
fn panel_theme() -> Theme {
    if cfg!(target_os = "linux") {
        return Theme::Night;
    }
    match window().window.theme() {
        SystemTheme::Dark => Theme::Night,
        _ => Theme::Day,
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
    let tray = use_hook(|| std::rc::Rc::new(Tray::new(panel_theme())));
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
            let config = window_config(
                WindowBuilder::new()
                    .with_title(format!("{APP_NAME} settings"))
                    .with_inner_size(dioxus::desktop::LogicalSize::new(480.0, 680.0)),
                Theme::Day,
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
                "snooze" => bridge.send(Command::Snooze(!view.peek().snoozed)),
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
            tray.show(&current);
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
            let config = window_config(
                WindowBuilder::new()
                    .with_title(format!("{APP_NAME}: calibration game"))
                    .with_decorations(false)
                    .with_always_on_top(true)
                    .with_fullscreen(Some(Fullscreen::Borderless(monitor))),
                Theme::Night,
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
    let down = |y: f32| y / frame_height * 100.0;
    rsx! {
        div { class: "app", "data-theme": Theme::Night.name(),
            div {
                class: "stage",
                style: "--aspect: {frame_width / frame_height}",
                CameraView { slot: preview }
                if let Some((line, limit)) = current.lines {
                    div { class: "line baseline", style: "top: {down(line)}%", span { "baseline" } }
                    div { class: "line limit", style: "top: {down(limit)}%", span { "slouch" } }
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
                div { class: "status mood-{current.mood:?}",
                    Eyes { mood: current.mood }
                    "{current.status}"
                    if current.snoozed {
                        span { class: "snoozed", "Nudges snoozed" }
                    }
                }
            }
            div { class: "buttons",
                button { class: "button primary", onclick: move |_| { let mut start = start_game.clone(); start() }, "Calibration game" }
                button { class: "button", onclick: move |_| bridge.send(Command::Recalibrate), "Quick recalibrate" }
                button { class: "button", onclick: move |_| { let open = open_settings.clone(); open() }, "Settings" }
            }
        }
    }
}

/// The tray's eyes, inline, in the colour of the text around them.
#[component]
fn Eyes(mood: Mood) -> Element {
    rsx! {
        span { class: "eyes", dangerous_inner_html: art::eyes_markup(mood) }
    }
}

#[component]
fn Bar(name: &'static str, ratio: f32) -> Element {
    let fill = (ratio.clamp(0.0, 1.5) / 1.5 * 100.0).max(0.0);
    let over = if ratio > 1.0 { "over" } else { "" };
    rsx! {
        div { class: "bar",
            span { class: "bar-name", "{name}" }
            div { class: "bar-track",
                div { class: "bar-fill {over}", style: "width: {fill}%" }
                div { class: "bar-limit" }
            }
        }
    }
}

#[component]
fn BannerView(banner: Banner) -> Element {
    rsx! {
        div { class: "banner",
            div { class: "banner-title", RisingUp { text: banner.title } }
            for line in banner.lines {
                div { class: "banner-line", "{line}" }
            }
            if let Some(progress) = banner.progress {
                div { class: "progress", span { style: "width: {progress * 100.0}%" } }
            }
        }
    }
}

/// Text with its word "up" lifted above the baseline: the brand's one gesture.
#[component]
fn RisingUp(text: String) -> Element {
    match split_at_up(&text) {
        Some((before, after)) => rsx! {
            "{before}"
            span { class: "up", "up" }
            "{after}"
        },
        None => rsx! { "{text}" },
    }
}

/// The text either side of the first whole word "up", if there is one.
fn split_at_up(text: &str) -> Option<(&str, &str)> {
    let alone = |c: Option<char>| !c.is_some_and(char::is_alphanumeric);
    text.match_indices("up")
        .map(|(at, _)| (&text[..at], &text[at + 2..]))
        .find(|(before, after)| alone(before.chars().next_back()) && alone(after.chars().next()))
}

/// The current game step, shared with the full-screen window, which has its own virtual DOM.
#[derive(Clone)]
pub struct SharedLookAt(Arc<Mutex<LookAt>>);

impl PartialEq for SharedLookAt {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Everything on one vertical line, so your gaze runs from the eyes to the words to yourself.
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
        div { class: "look-here", "data-theme": Theme::Night.name(),
            if step.screen.is_some() {
                div { class: "look-eyes", dangerous_inner_html: art::looking_up_svg() }
                div { class: "look-title", "Look here" }
            }
            div { class: "look-prompt", RisingUp { text: step.prompt } }
            div { class: "look-detail", "{step.detail}" }
            div { class: "look-camera", CameraView { slot } }
            div { class: "progress look-progress",
                span { style: "width: {step.progress * 100.0}%" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_at_up;

    #[test]
    fn up_rises_only_as_a_whole_word() {
        assert_eq!(
            split_at_up("Sit up straight, look at the left screen"),
            Some(("Sit ", " straight, look at the left screen"))
        );
        assert_eq!(split_at_up("Psst, sit up"), Some(("Psst, sit ", "")));
        assert_eq!(split_at_up("upright, cupboard"), None);
        assert_eq!(split_at_up("Slouch down, don't lean forward"), None);
    }
}

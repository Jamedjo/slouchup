//! The tray, the app's window with its camera, history and settings views, and the full-screen
//! "look here" prompt used by the calibration game.

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::tao::monitor::MonitorHandle;
use dioxus::desktop::tao::window::{Fullscreen, Icon as WindowIcon};
use dioxus::desktop::trayicon::menu::{
    Icon as MenuIcon, IconMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu,
};
use dioxus::desktop::trayicon::{Icon, TrayIcon};
use dioxus::desktop::{
    Config, DesktopContext, WindowBuilder, use_muda_event_handler, use_tray_menu_event_handler,
    use_wry_event_handler, window,
};
use dioxus::prelude::*;
use futures_channel::mpsc::UnboundedReceiver;
use futures_util::StreamExt;

use crate::art::{self, Files, Mark, Mood, Theme};
use crate::camera_picker::CameraPicker;
use crate::camera_view::{CameraView, FrameSlot};
use crate::config::{self, APP_NAME};
use crate::engine::{Banner, Command, LookAt, PAUSES, View};
use crate::history_window::HistoryPage;
use crate::notification_access::{self, Fix, Health, HealthStrip};
use crate::notifier;
use crate::onboarding::Onboarding;
use crate::settings::SettingsPage;
use crate::source::{self, CameraInfo};
use crate::{screens, still_running, style};

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
    /// The first run, which waits for the welcome to turn the camera on.
    pub onboarding: bool,
    pub files: Files,
    /// The view the window opens on.
    pub start_on: Page,
    pub history: crate::engine::SharedHistory,
}

/// The window's views, switched between in its header.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Page {
    Camera,
    History,
    Settings,
}

impl Page {
    const ALL: [Page; 3] = [Page::Camera, Page::History, Page::Settings];

    fn label(self) -> &'static str {
        match self {
            Page::Camera => "Camera",
            Page::History => "History",
            Page::Settings => "Settings",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Page::Camera => CAMERA_ICON,
            Page::History => HISTORY_ICON,
            Page::Settings => SETTINGS_ICON,
        }
    }
}

/// Line icons in the design system's stroke, coloured by the text around them.
const PAUSE_ICON: &str = r#"<svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="5" width="4" height="14" rx="1.5"/><rect x="14" y="5" width="4" height="14" rx="1.5"/></svg>"#;
const RESUME_ICON: &str = r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5.5v13a1 1 0 0 0 1.5.9l10.4-6.5a1 1 0 0 0 0-1.8L9.5 4.6A1 1 0 0 0 8 5.5Z"/></svg>"#;
pub const CAMERA_ICON: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="6" width="13" height="12" rx="3"/><path d="m16 10.5 5-3v9l-5-3"/></svg>"#;
const HISTORY_ICON: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round"><path d="M4 20h16M7 16v-4M12 16V6M17 16V9"/></svg>"#;
const SETTINGS_ICON: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round"><path d="M4 7h9M19 7h1M4 17h3M13 17h7"/><circle cx="16" cy="7" r="2.5"/><circle cx="10" cy="17" r="2.5"/></svg>"#;

impl Bridge {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

struct Tray {
    icon: TrayIcon,
    menu: Menu,
    status: MenuItem,
    /// The ways to pause, swapped for `resume` while paused.
    pause: Submenu,
    resume: MenuItem,
    calibrate: Submenu,
    /// The fix the status row offers while nudges can't reach you.
    fix: Cell<Option<Fix>>,
    /// What's showing, since setting the icon goes over D-Bus and the engine updates 5 times a second.
    shown: RefCell<Shown>,
    slide: RefCell<Slide>,
    /// Whether a task is redrawing the icon until the slide settles.
    sliding: Cell<bool>,
}

struct Shown {
    mark: Mark,
    theme: Theme,
    status: String,
    paused: bool,
    /// Whether the mark is struck through, as nudges can't reach you.
    struck: bool,
}

/// The tray's letters easing towards where the latest reading puts them.
struct Slide {
    target: Mark,
    from: f32,
    started: Instant,
}

impl Slide {
    fn settled_at(target: Mark) -> Self {
        Self {
            target,
            from: target.dy(),
            started: Instant::now() - art::SLIDE,
        }
    }

    /// The target, with its letters part way from where they were. Turning over isn't eased.
    fn at(&self, now: Instant) -> Mark {
        let progress = (now - self.started).as_secs_f32() / art::SLIDE.as_secs_f32();
        match self.target {
            Mark::Live { dy, down } if progress < 1.0 => {
                let eased = 1.0 - (1.0 - progress).powi(3);
                Mark::Live {
                    dy: self.from + (dy - self.from) * eased,
                    down,
                }
            }
            target => target,
        }
    }

    fn head_for(&mut self, target: Mark, now: Instant) {
        if worth_moving(self.target, target) {
            self.from = self.at(now).dy();
            self.target = target;
            self.started = now;
        }
    }

    fn settled(&self, now: Instant) -> bool {
        now - self.started >= art::SLIDE
    }
}

/// Readings wobble a little while you sit still, and every redraw goes over D-Bus, so the letters
/// only set off for a move of at least this many grid units.
const LEAST_MOVE: f32 = 0.5;

fn worth_moving(from: Mark, to: Mark) -> bool {
    match (from, to) {
        (Mark::Live { down, .. }, Mark::Live { down: to_down, .. }) if down == to_down => {
            (to.dy() - from.dy()).abs() >= LEAST_MOVE
        }
        _ => from != to,
    }
}

/// How often the icon is redrawn while the letters slide.
const SLIDE_FRAME: Duration = Duration::from_millis(30);
/// The smallest move of the letters worth redrawing for, in the mark's grid units: half a pixel
/// of the 64px icon.
const DRAWN_STEP: f32 = 0.25;

/// The mark the view calls for. `was_down` is whether it read "dn" before, for the hysteresis.
fn tray_mark(view: &View, was_down: bool) -> Mark {
    match view.mood {
        Mood::Good | Mood::Bad => {
            let limits = view.settings.thresholds;
            let t = view.reading.map_or(0.0, |reading| {
                art::slouch_t(reading.drop, limits.drop, reading.lean, limits.lean)
            });
            Mark::Live {
                dy: art::letters_dy(t),
                down: art::reads_down(was_down, t),
            }
        }
        Mood::Idle => Mark::Lost,
        Mood::Paused => Mark::Paused,
    }
}

fn rounded(mark: Mark) -> Mark {
    match mark {
        Mark::Live { dy, down } => Mark::Live {
            dy: (dy / DRAWN_STEP).round() * DRAWN_STEP,
            down,
        },
        other => other,
    }
}

fn tray_icon(mark: Mark, theme: Theme, struck: bool) -> Icon {
    Icon::from_rgba(
        art::rasterise(&art::tray_svg(mark, theme, struck), 64, 64),
        64,
        64,
    )
    .expect("icon is 64x64")
}

/// The tray menu id for each of [`PAUSES`], by its place in the list.
fn pause_id(index: usize) -> String {
    format!("pause-{index}")
}

/// The length of pause a tray menu id stands for.
fn pause_for(id: &str) -> Option<Option<Duration>> {
    let index: usize = id.strip_prefix("pause-")?.parse().ok()?;
    PAUSES.get(index).map(|&(_, length)| length)
}

/// The tray's names for [`PAUSES`], under Pause.
const TRAY_PAUSES: [&str; 3] = ["30 minutes", "1 hour", "Until I resume"];

/// Where Pause sits in the tray menu, after SlouchUp, the status and a separator.
const PAUSE_AT: usize = 3;

/// A menu icon from `svg`, with any `currentColor` in `colour`.
fn menu_icon(svg: &str, colour: &str) -> Option<MenuIcon> {
    let size = 32;
    let svg = svg.replace("currentColor", colour);
    MenuIcon::from_rgba(art::rasterise(&svg, size, size), size, size).ok()
}

impl Tray {
    fn new(theme: Theme, paused: bool) -> Self {
        let shown = Shown {
            mark: if paused { Mark::Paused } else { Mark::Lost },
            theme,
            status: "Starting…".into(),
            paused,
            struck: false,
        };
        let status = MenuItem::with_id("status", &shown.status, false, None);
        let pause = Submenu::with_id("pause", "Pause", true);
        for (index, label) in TRAY_PAUSES.iter().enumerate() {
            pause
                .append(&MenuItem::with_id(pause_id(index), label, true, None))
                .expect("pause menu builds");
        }
        let resume = MenuItem::with_id("resume", "Resume", true, None);
        // Menus follow the system's theme, as the panel does, so its text colour suits the icons.
        let ink = match theme {
            Theme::Night => "#F2EADB",
            Theme::Day => "#2B2724",
        };
        let row = |id: &str, label: &str, svg: &str| {
            IconMenuItem::with_id(id, label, true, menu_icon(svg, ink), None)
        };
        let calibrate = Submenu::with_id_and_items(
            "calibrate",
            "Calibrate",
            !paused,
            &[
                &MenuItem::with_id("quick", "Quick", true, None),
                &MenuItem::with_id("guided", "Guided, full screen", true, None),
            ],
        )
        .expect("calibrate menu builds");
        let menu = Menu::new();
        menu.append_items(&[
            &row("show", APP_NAME, &art::app_icon_svg()),
            &status,
            &PredefinedMenuItem::separator(),
            if paused { &resume } else { &pause },
            &calibrate,
            &PredefinedMenuItem::separator(),
            &row("camera", "Camera", CAMERA_ICON),
            &row("history", "History", HISTORY_ICON),
            &row("settings", "Settings", SETTINGS_ICON),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("quit", "Quit", true, None),
        ])
        .expect("tray menu builds");
        let icon = dioxus::desktop::trayicon::init_tray_icon(
            menu.clone(),
            Some(tray_icon(shown.mark, shown.theme, shown.struck)),
        );
        let _ = icon.set_tooltip(Some(APP_NAME));
        // appindicator has no tooltip; panels show its title, which falls back to GLib's app name.
        #[cfg(target_os = "linux")]
        glib::set_application_name(APP_NAME);
        Self {
            icon,
            menu,
            status,
            pause,
            resume,
            calibrate,
            fix: Cell::new(None),
            slide: Slide::settled_at(shown.mark).into(),
            shown: shown.into(),
            sliding: false.into(),
        }
    }

    /// Show `view`, with `health` in place of its status while nudges can't reach you, and say
    /// whether the letters have started sliding and need redrawing.
    fn show(&self, view: &View, health: Option<&Health>) -> bool {
        let now = Instant::now();
        {
            let mut slide = self.slide.borrow_mut();
            let was_down = matches!(slide.target, Mark::Live { down: true, .. });
            slide.head_for(tray_mark(view, was_down), now);
        }
        let struck = health.is_some();
        if self.shown.borrow().struck != struck {
            self.shown.borrow_mut().struck = struck;
            self.draw();
            let tip = health.map_or(APP_NAME.to_string(), |h| {
                format!("{APP_NAME} · {}", h.title)
            });
            let _ = self.icon.set_tooltip(Some(tip));
        }
        let sliding = self.redraw();
        let mut shown = self.shown.borrow_mut();
        let status = health.map_or_else(|| view.status.clone(), Health::line);
        if shown.status != status {
            self.status.set_text(&status);
            shown.status = status;
        }
        let fix = health.and_then(|h| h.fix);
        if self.fix.replace(fix) != fix {
            self.status.set_enabled(fix.is_some());
        }
        if shown.paused != view.paused {
            shown.paused = view.paused;
            self.offer_pauses(!view.paused);
        }
        sliding
    }

    /// Offer Pause, or while paused, Resume in its place. Calibrating needs the
    /// camera, so it's offered only while it's on.
    fn offer_pauses(&self, offer: bool) {
        let result = if offer {
            self.menu
                .remove(&self.resume)
                .and_then(|()| self.menu.insert(&self.pause, PAUSE_AT))
        } else {
            self.menu
                .remove(&self.pause)
                .and_then(|()| self.menu.insert(&self.resume, PAUSE_AT))
        };
        if let Err(error) = result {
            tracing::warn!("couldn't update the tray menu: {error}");
        }
        self.calibrate.set_enabled(offer);
    }

    /// Draw the letters where the slide has them now, and say whether they're still moving.
    fn redraw(&self) -> bool {
        let now = Instant::now();
        let slide = self.slide.borrow();
        let mark = rounded(slide.at(now));
        let mut shown = self.shown.borrow_mut();
        if shown.mark != mark {
            shown.mark = mark;
            drop(shown);
            self.draw();
        }
        !slide.settled(now)
    }

    fn draw(&self) {
        let shown = self.shown.borrow();
        let _ = self
            .icon
            .set_icon(Some(tray_icon(shown.mark, shown.theme, shown.struck)));
    }

    fn suit_panel(&self, theme: Theme) {
        let changed = std::mem::replace(&mut self.shown.borrow_mut().theme, theme) != theme;
        if changed {
            self.draw();
        }
    }
}

/// The theme of the panel the tray sits on. Windows themes its taskbar apart from apps, and the
/// macOS menu bar follows the system. Nothing says what a Linux panel looks like, but GNOME's top
/// bar is dark whatever the desktop theme, so there it's taken as dark.
fn panel_theme() -> Theme {
    #[cfg(windows)]
    let light = crate::windows_shell::taskbar_is_light();
    #[cfg(target_os = "macos")]
    let light = window().window.theme() == dioxus::desktop::tao::window::Theme::Light;
    #[cfg(not(any(windows, target_os = "macos")))]
    let light = false;
    if light { Theme::Day } else { Theme::Night }
}

/// The least time between listing the cameras again, since listing opens each one.
const RELIST_AFTER: Duration = Duration::from_secs(5);

/// How often to check whether the panel has changed theme, say for night mode.
const PANEL_CHECK: Duration = Duration::from_secs(2);

/// Move focus between the elements matching `selector` for an arrow, Home or End `key`, as a
/// list does, and say whether it was one of those.
pub fn move_focus(selector: &str, key: &Key) -> bool {
    let step = match key {
        Key::ArrowDown | Key::ArrowRight => "(i + 1) % all.length",
        Key::ArrowUp | Key::ArrowLeft => "(i - 1 + all.length) % all.length",
        Key::Home => "0",
        Key::End => "all.length - 1",
        _ => return false,
    };
    document::eval(&format!(
        "const all = [...document.querySelectorAll('{selector}')]; \
         const i = all.indexOf(document.activeElement); all[{step}]?.focus();"
    ));
    true
}

/// `say`, but only the first time it's called, so a notice comes once a run.
fn once(say: impl Fn() + 'static) -> std::rc::Rc<dyn Fn()> {
    let said = std::cell::Cell::new(false);
    std::rc::Rc::new(move || {
        if !said.replace(true) {
            say();
        }
    })
}

fn show_main_window() {
    let main = window();
    main.set_visible(true);
    main.set_focus();
}

#[component]
pub fn App() -> Element {
    let bridge = use_context::<Bridge>();
    let mut view = use_signal(|| View {
        paused: bridge.onboarding,
        ..View::default()
    });
    let tray = use_hook(|| std::rc::Rc::new(Tray::new(panel_theme(), bridge.onboarding)));
    let access = notification_access::use_access().0;
    use_future({
        let tray = tray.clone();
        move || {
            let tray = tray.clone();
            async move {
                loop {
                    futures_timer::Delay::new(PANEL_CHECK).await;
                    tray.suit_panel(panel_theme());
                }
            }
        }
    });
    let paused = use_memo(move || view.read().paused);
    let mut onboarding = use_signal(|| bridge.onboarding);
    let mut cameras = use_signal(Vec::<CameraInfo>::new);
    let list_cameras = use_callback({
        let demo = !bridge.persist;
        move |()| {
            spawn(async move {
                let (found, listed) = futures_channel::oneshot::channel();
                std::thread::spawn(move || {
                    let _ = found.send(if demo {
                        source::demo_cameras()
                    } else {
                        source::list_cameras()
                    });
                });
                if let Ok(listed) = listed.await {
                    cameras.set(listed);
                }
            });
        }
    });
    use_hook(move || list_cameras(()));
    let mut chosen = use_signal(|| {
        bridge
            .persist
            .then(|| config::load_preferences().camera)
            .flatten()
    });
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

    let start_game = use_callback({
        let bridge = bridge.clone();
        let mut monitors = monitors;
        move |()| {
            let found: Vec<MonitorHandle> = window().available_monitors().collect();
            let screens = screens::describe(&found);
            monitors.set(found);
            bridge.send(Command::Game { screens });
        }
    });
    let mut page = use_signal(|| bridge.start_on);
    let mut calibrate_open = use_signal(|| false);
    // Calibrating starts from the camera, where its choice opens and you can see yourself.
    let open_calibrate = use_callback(move |()| {
        page.set(Page::Camera);
        calibrate_open.set(!paused());
    });
    let open_page = use_callback(move |to: Page| {
        page.set(to);
        show_main_window();
    });
    use_hook({
        let wanted = bridge.start_with_game;
        move || {
            if wanted {
                start_game(());
            }
        }
    });

    // Closing the camera window, or putting off the welcome, only hides it, so the first time says
    // where the app went, and whether it's watching.
    let say_still_running = use_hook(|| {
        let files = bridge.files.clone();
        once(move || {
            still_running::open();
            notifier::still_running(&files);
        })
    });
    let say_camera_off = use_hook(|| {
        let files = bridge.files.clone();
        once(move || notifier::camera_off(&files))
    });
    use_wry_event_handler({
        let main = window().id();
        let say_camera_off = say_camera_off.clone();
        let mut listed = Instant::now();
        move |event, _| {
            let Event::WindowEvent {
                event, window_id, ..
            } = event
            else {
                return;
            };
            if *window_id != main {
                return;
            }
            match event {
                WindowEvent::CloseRequested if paused() => say_camera_off(),
                WindowEvent::CloseRequested => say_still_running(),
                // Coming back to the window is when a camera plugged in meanwhile is wanted.
                WindowEvent::Focused(true) if listed.elapsed() > RELIST_AFTER => {
                    listed = Instant::now();
                    list_cameras(());
                }
                _ => {}
            }
        }
    });
    let pause = use_callback({
        let bridge = bridge.clone();
        move |length: Option<Duration>| bridge.send(Command::Pause(length))
    });
    let resume = use_callback({
        let bridge = bridge.clone();
        move |()| {
            bridge.send(Command::Resume);
            // Resuming turns the camera on, which is all the welcome was waiting for.
            onboarding.set(false);
        }
    });
    let start_watching = {
        let bridge = bridge.clone();
        move |camera: Option<String>| {
            if bridge.persist {
                let mut preferences = config::load_preferences();
                preferences.camera = camera.clone();
                if let Err(error) = config::save_preferences(&preferences) {
                    tracing::warn!("couldn't save the camera choice: {error}");
                }
            }
            chosen.set(camera.clone());
            bridge.send(Command::UseCamera(camera));
            bridge.send(Command::Resume);
            onboarding.set(false);
            start_game(());
        }
    };
    let choose_camera = use_callback({
        let bridge = bridge.clone();
        move |id: String| {
            chosen.set(Some(id.clone()));
            // The demo's cameras are stand-ins; the engine keeps drawing the person.
            if bridge.persist {
                bridge.send(Command::UseCamera(Some(id)));
            }
        }
    });
    let not_now = move |()| {
        window().set_visible(false);
        say_camera_off();
    };

    let on_menu = use_hook({
        let bridge = bridge.clone();
        let tray = tray.clone();
        move || {
            std::rc::Rc::new(std::cell::RefCell::new(move |id: &str| match id {
                "show" => show_main_window(),
                "camera" => open_page(Page::Camera),
                "settings" => open_page(Page::Settings),
                "history" => open_page(Page::History),
                "guided" => start_game(()),
                "quick" => bridge.send(Command::Recalibrate),
                "resume" => resume(()),
                "status" => {
                    if let Some(fix) = tray.fix.get() {
                        notification_access::apply(fix);
                    }
                }
                "quit" => crate::quit(),
                id => {
                    if let Some(length) = pause_for(id) {
                        pause(length);
                    }
                }
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
            let health = access().and_then(|now| notification_access::health(now, current.missed));
            if tray.show(&current, health.as_ref()) && !tray.sliding.replace(true) {
                let tray = tray.clone();
                spawn(async move {
                    loop {
                        futures_timer::Delay::new(SLIDE_FRAME).await;
                        if !tray.redraw() {
                            break;
                        }
                    }
                    tray.sliding.set(false);
                });
            }
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

    if onboarding() && page() == Page::Camera {
        let chosen = bridge
            .persist
            .then(|| config::load_preferences().camera)
            .flatten();
        return rsx! {
            Onboarding { chosen, on_start: start_watching, on_later: not_now }
        };
    }
    let shown = page();
    // The saved camera when it's plugged in, as the engine does; otherwise the first.
    let active_camera = {
        let cameras = cameras.read();
        let chosen = chosen.read();
        cameras
            .iter()
            .find(|c| Some(&c.id) == chosen.as_ref())
            .or(cameras.first())
            .map(|c| c.id.clone())
    };
    rsx! {
        div { class: "app", "data-theme": Theme::Night.name(),
            header { class: "topbar",
                div { class: "brand", "aria-label": APP_NAME, role: "img",
                    span { class: "wordmark", "slouch", span { class: "up", "up" } }
                }
                nav { class: "views", "aria-label": "Views",
                    for to in Page::ALL {
                        button {
                            class: "view-tab",
                            "aria-current": if shown == to { "page" } else { "false" },
                            title: "{to.label()}",
                            onclick: move |_| page.set(to),
                            span { class: "icon", dangerous_inner_html: to.icon() }
                            span { class: "view-label", "{to.label()}" }
                        }
                    }
                }
            }
            if shown != Page::Settings {
                HealthStrip { missed: view().missed }
            }
            main { class: "view",
                match shown {
                    Page::Camera => rsx! {
                        CameraPage {
                            current: view(),
                            paused: paused(),
                            cameras: cameras(),
                            active_camera: active_camera.clone().unwrap_or_default(),
                            choose_camera,
                            pause,
                            resume,
                            start_game,
                            calibrate_open,
                        }
                    },
                    Page::History => rsx! { HistoryPage {} },
                    Page::Settings => rsx! {
                        SettingsPage {
                            initial: view.peek().settings,
                            calibrated: view.peek().calibrated,
                            on_calibrate: open_calibrate,
                            missed: view.peek().missed,
                        }
                    },
                }
            }
        }
    }
}

/// The live picture with its lines and readings, and the controls on its bottom strip, as video
/// calls have: the camera, Pause and Calibrate.
#[component]
fn CameraPage(
    current: View,
    paused: bool,
    cameras: Vec<CameraInfo>,
    active_camera: String,
    choose_camera: Callback<String>,
    pause: Callback<Option<Duration>>,
    resume: Callback<()>,
    start_game: Callback<()>,
    calibrate_open: Signal<bool>,
) -> Element {
    let bridge = use_context::<Bridge>();
    let pause_open = use_signal(|| false);
    let (frame_width, frame_height) = current.frame_size;
    let down = |y: f32| y / frame_height * 100.0;
    rsx! {
        div { class: "camera-page",
            div {
                class: "stage",
                style: "--aspect: {frame_width / frame_height}",
                CameraView { slot: bridge.preview.clone() }
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
                if paused {
                    div { class: "paused-cover",
                        p { "{APP_NAME} is paused, and your camera is off." }
                        button { class: "button primary", onclick: move |_| resume(()), "Resume" }
                    }
                }
                div { class: "status mood-{current.mood:?}",
                    PostureMark { mood: current.mood }
                    span { class: "status-text", "{current.status}" }
                    div { class: "controls",
                        if cameras.len() > 1 {
                            CameraPicker { cameras, active: active_camera, on_choose: choose_camera }
                        }
                        if paused {
                            button {
                                class: "control pause-button",
                                "aria-pressed": "true",
                                title: "Turn the camera back on",
                                onclick: move |_| resume(()),
                                span { class: "icon", dangerous_inner_html: RESUME_ICON }
                                "Resume"
                            }
                        } else {
                            ChoiceMenu {
                                name: "pause",
                                label: "Pause",
                                icon: PAUSE_ICON,
                                open: pause_open,
                                choices: pause_choices(pause),
                            }
                            ChoiceMenu {
                                name: "calibrate",
                                label: "Calibrate",
                                primary: true,
                                open: calibrate_open,
                                choices: vec![
                                    Choice {
                                        name: "Quick".into(),
                                        detail: "Sit nicely for 3 seconds. Handy after you move your laptop or chair.".into(),
                                        choose: Callback::new(move |()| bridge.send(Command::Recalibrate)),
                                    },
                                    Choice {
                                        name: "Guided, full screen".into(),
                                        detail: "Step by step, about 30 seconds. Best the first time, or at a new desk.".into(),
                                        choose: start_game,
                                    },
                                ],
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The ways to pause, each saying when the camera comes back on.
fn pause_choices(pause: Callback<Option<Duration>>) -> Vec<Choice> {
    PAUSES
        .iter()
        .map(|&(name, length)| Choice {
            name: name.into(),
            detail: match length {
                Some(length) => format!(
                    "Back on at {}",
                    (chrono::Local::now() + chrono::Duration::from_std(length).unwrap_or_default())
                        .format("%H:%M")
                ),
                None => "Resume from here or the tray menu.".into(),
            },
            choose: Callback::new(move |()| pause(length)),
        })
        .collect()
}

/// One of a [`ChoiceMenu`]'s choices.
#[derive(Clone, PartialEq)]
struct Choice {
    name: String,
    detail: String,
    choose: Callback<()>,
}

/// A button in the bar opening a short menu of choices, such as how to calibrate.
#[component]
fn ChoiceMenu(
    /// Names the menu, for styling and for keeping arrow keys to its own choices.
    name: &'static str,
    label: &'static str,
    /// An icon before the label, as SVG markup.
    #[props(default)]
    icon: &'static str,
    #[props(default)] primary: bool,
    open: Signal<bool>,
    choices: Vec<Choice>,
) -> Element {
    let mut button = use_signal(|| None::<std::rc::Rc<MountedData>>);
    let mut close = move || {
        open.set(false);
        if let Some(button) = button() {
            spawn(async move {
                let _ = button.set_focus(true).await;
            });
        }
    };
    let primary = if primary { "primary" } else { "" };
    rsx! {
        div { class: "choices {name}",
            button {
                class: "control {primary} choices-button",
                "aria-haspopup": "menu",
                "aria-expanded": "{open()}",
                onmounted: move |event| button.set(Some(event.data())),
                onclick: move |_| open.toggle(),
                if !icon.is_empty() {
                    span { class: "icon", dangerous_inner_html: icon }
                }
                "{label}"
                span { class: "icon caret", dangerous_inner_html: CARET_UP }
            }
            if open() {
                div { class: "backdrop", onclick: move |_| close() }
                div {
                    class: "choices-menu",
                    role: "menu",
                    "aria-label": label,
                    onkeydown: move |event| match event.key() {
                        Key::Escape => {
                            event.prevent_default();
                            close();
                        }
                        key => {
                            if move_focus(&format!(".{name} .choice"), &key) {
                                event.prevent_default();
                            }
                        }
                    },
                    for (index, Choice { name, detail, choose }) in choices.into_iter().enumerate() {
                        button {
                            class: "choice",
                            role: "menuitem",
                            onmounted: move |event| async move {
                                if index == 0 {
                                    let _ = event.data().set_focus(true).await;
                                }
                            },
                            onclick: move |_| {
                                close();
                                choose(());
                            },
                            span { class: "choice-name", "{name}" }
                            span { class: "choice-detail", "{detail}" }
                        }
                    }
                }
            }
        }
    }
}

const CARET_UP: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round"><path d="m6 15 6-6 6 6"/></svg>"#;

/// The tray's up/dn mark for `mood`, inline, in the colour of the text around them.
#[component]
pub fn PostureMark(mood: Mood) -> Element {
    rsx! {
        span { class: "mark", dangerous_inner_html: art::mark_markup(Mark::of(mood)) }
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
    use super::*;

    #[test]
    fn tray_letters_ease_to_a_new_height_but_turn_over_at_once() {
        let start = Instant::now();
        let mut slide = Slide::settled_at(Mark::Live {
            dy: 0.0,
            down: false,
        });
        slide.head_for(
            Mark::Live {
                dy: 8.0,
                down: true,
            },
            start,
        );
        let Mark::Live { dy, down } = slide.at(start + art::SLIDE / 2) else {
            panic!("still live");
        };
        assert!(down);
        assert!(dy > 4.0 && dy < 8.0, "eased out: {dy}");
        assert!(!slide.settled(start + art::SLIDE / 2));
        assert_eq!(
            slide.at(start + art::SLIDE),
            Mark::Live {
                dy: 8.0,
                down: true
            }
        );
        assert!(slide.settled(start + art::SLIDE));
    }

    #[test]
    fn tray_letters_stay_put_for_a_wobble() {
        let still = Mark::Live {
            dy: 2.0,
            down: false,
        };
        assert!(!worth_moving(
            still,
            Mark::Live {
                dy: 2.3,
                down: false
            }
        ));
        assert!(worth_moving(
            still,
            Mark::Live {
                dy: 2.5,
                down: false
            }
        ));
        assert!(worth_moving(
            still,
            Mark::Live {
                dy: 2.0,
                down: true
            }
        ));
        assert!(worth_moving(still, Mark::Lost));
    }

    #[test]
    fn tray_pause_ids_name_their_lengths() {
        for (index, (_, length)) in PAUSES.iter().enumerate() {
            assert_eq!(pause_for(&pause_id(index)), Some(*length));
        }
        assert_eq!(pause_for("pause-9"), None);
        assert_eq!(pause_for("quick"), None);
    }

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

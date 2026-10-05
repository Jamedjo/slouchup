//! The popover the tray icon opens: whether SlouchUp is on, how you're sitting, Pause and
//! Calibrate, the window's views, and Quit.
//!
//! It's a window of its own, with its own virtual DOM, made hidden at startup and shown by the
//! tray. What it shows comes from the app's window, and what's chosen in it goes back there.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dioxus::desktop::{Config, DesktopContext, LogicalSize, WindowBuilder, WindowCloseBehaviour};
use dioxus::prelude::*;
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures_util::StreamExt;
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
use tray_popover_tao::tray_popover::Anchor;
use tray_popover_tao::{PopoverEvent, TaoPopover, TaoSurface};

use crate::art::{Mood, Theme};
use crate::config::APP_NAME;
use crate::engine::{PAUSES, View};
use crate::notification_access::{Fix, Health};
use crate::ui::{Page, PostureMark, move_focus, window_config};

/// The popover's size in logical pixels. It doesn't grow: its menus open over what's below them.
const SIZE: (f64, f64) = (300.0, 286.0);

/// Whether to log each step from a tray click to the popover, for diagnosing it on a platform:
/// set `SLOUCHUP_TRACE_TRAY` to anything.
pub fn tracing_tray() -> bool {
    static ON: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var_os("SLOUCHUP_TRACE_TRAY").is_some());
    *ON
}

/// Log `what` when [`tracing_tray`] is on.
pub fn trace(what: impl FnOnce() -> String) {
    if tracing_tray() {
        tracing::info!("tray: {}", what());
    }
}

/// The names for [`PAUSES`], under Pause.
const PAUSE_NAMES: [&str; 3] = ["30 minutes", "1 hour", "Until I resume"];

/// The two ways to calibrate, each with a tooltip on when it suits.
const CALIBRATIONS: [(&str, &str); 2] = [
    (
        "Quick",
        "Sit nicely for 3 seconds. Handy after you move your laptop or chair.",
    ),
    (
        "Guided, full screen",
        "Step by step, about 30 seconds. Best the first time, or at a new desk.",
    ),
];

/// Something chosen in the popover, for the app's window to act on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Open(Page),
    Pause(Option<Duration>),
    Resume,
    QuickCalibration,
    GuidedCalibration,
    Quit,
    /// The fix for nudges not reaching you, such as turning notifications on.
    Fix(Fix),
    /// Esc, with no menu open.
    Close,
    /// The page has loaded, which is when dioxus-desktop shows a window made hidden, on macOS
    /// and Windows, if the app's own window was showing at startup.
    Loaded,
}

/// What the popover shows.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub mood: Mood,
    pub status: String,
    pub paused: bool,
    /// What's stopping nudges reaching you, shown in place of the status while it lasts.
    pub health: Option<Health>,
    /// Counts the times it has opened, so it can take the keyboard focus each time.
    pub opened: u32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            mood: Mood::Idle,
            status: "Starting…".into(),
            paused: false,
            health: None,
            opened: 0,
        }
    }
}

/// The popover's ends of its channels to the app's window.
#[derive(Clone)]
pub struct Link {
    pub states: Arc<Mutex<Option<UnboundedReceiver<State>>>>,
    pub actions: UnboundedSender<Action>,
}

impl PartialEq for Link {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.states, &other.states)
    }
}

impl Link {
    fn send(&self, action: Action) {
        let _ = self.actions.unbounded_send(action);
    }
}

/// The app's side of the popover: its window, and what it was last told to show.
pub struct PopoverWindow {
    /// The popover, once its window has opened.
    popover: RefCell<Option<(TaoPopover, DesktopContext)>>,
    /// Events from the popover's window that arrived while it was busy, as showing or hiding a
    /// window can send them straight back on Windows.
    held: RefCell<Vec<PopoverEvent>>,
    states: UnboundedSender<State>,
    shown: RefCell<State>,
    #[cfg(target_os = "linux")]
    host: tray_popover_tao::tray_popover::linux::Host,
}

impl PopoverWindow {
    /// Open the popover's window, hidden, from the app's window. Choices made in it go to
    /// `actions`.
    pub fn open(actions: UnboundedSender<Action>) -> std::rc::Rc<Self> {
        let (states, receiver) = futures_channel::mpsc::unbounded();
        let link = Link {
            states: Arc::new(Mutex::new(Some(receiver))),
            actions,
        };
        let dom = VirtualDom::new_with_props(Popover, PopoverProps { link });
        let pending = dioxus::desktop::window().new_window(dom, window());
        let opened = std::rc::Rc::new(Self {
            popover: RefCell::new(None),
            held: RefCell::new(Vec::new()),
            states,
            shown: RefCell::new(State::default()),
            #[cfg(target_os = "linux")]
            host: tray_popover_tao::detect_host(),
        });
        let filling = opened.clone();
        spawn(async move {
            let context = pending.await;
            let popover = TaoPopover::new(TaoSurface::new(context.window.clone()));
            // dioxus-desktop passes a window's events only to handlers made for that window.
            let window = context.window.id();
            let events = filling.clone();
            context.create_wry_event_handler(move |event, _| {
                // The page closes it on Esc itself, since tao also sees the Esc that closes one
                // of its menus.
                if let Some(event @ PopoverEvent::Focused(_)) = PopoverEvent::of(window, event) {
                    trace(|| format!("popover window {event:?}"));
                    events.held.borrow_mut().push(event);
                    events.with_popover(|_, _| {});
                }
            });
            *filling.popover.borrow_mut() = Some((popover, context));
        });
        opened
    }

    /// A click on the tray icon: a left or right click opens or closes the popover by the icon.
    /// There's no menu, so a right click, or a Control-click on macOS, does what a left one does.
    pub fn click(&self, event: &TrayIconEvent) {
        let TrayIconEvent::Click {
            button: MouseButton::Left | MouseButton::Right,
            button_state: MouseButtonState::Up,
            position,
            rect,
            ..
        } = event
        else {
            return;
        };
        let anchor = self.anchor(position, rect);
        trace(|| format!("click, anchor {anchor:?}"));
        self.with_popover(|popover, context| {
            let was = popover.shown();
            popover.click(anchor, Instant::now());
            trace(|| format!("popover shown {was} -> {}", popover.shown()));
            if popover.shown() {
                let _ = context.webview.focus();
                let mut shown = self.shown.borrow_mut();
                shown.opened += 1;
                let _ = self.states.unbounded_send(shown.clone());
            }
        });
    }

    /// Run `act` on the popover once its window has opened, then apply any events its window
    /// sent meanwhile. Called again while busy, as from an event, it does nothing, and the
    /// first call applies the events.
    fn with_popover(&self, act: impl FnOnce(&mut TaoPopover, &DesktopContext)) {
        let Ok(mut popover) = self.popover.try_borrow_mut() else {
            trace(|| "popover busy; its events wait".into());
            return;
        };
        let Some((popover, context)) = popover.as_mut() else {
            trace(|| "popover window not open yet".into());
            self.held.borrow_mut().clear();
            return;
        };
        act(popover, context);
        loop {
            let held = std::mem::take(&mut *self.held.borrow_mut());
            if held.is_empty() {
                break;
            }
            for event in held {
                event.apply(popover);
                trace(|| format!("after {event:?}, popover shown {}", popover.shown()));
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn anchor(
        &self,
        position: &tray_icon::dpi::PhysicalPosition<f64>,
        _: &tray_icon::Rect,
    ) -> Anchor {
        self.host.anchor(position.x as i32, position.y as i32)
    }

    #[cfg(not(target_os = "linux"))]
    fn anchor(
        &self,
        position: &tray_icon::dpi::PhysicalPosition<f64>,
        rect: &tray_icon::Rect,
    ) -> Anchor {
        use tray_popover_tao::tray_popover::{Point, Rect};
        if rect.size.width == 0 || rect.size.height == 0 {
            return Anchor::Point(Point::new(position.x as i32, position.y as i32));
        }
        Anchor::Icon(Rect::new(
            rect.position.x as i32,
            rect.position.y as i32,
            rect.size.width as i32,
            rect.size.height as i32,
        ))
    }

    pub fn hide(&self) {
        self.with_popover(|popover, _| popover.hide());
    }

    /// Hide the window again if it was shown without being opened.
    pub fn settle(&self) {
        self.with_popover(|popover, _| popover.settle());
    }

    /// Show the engine's latest view, and what's stopping nudges reaching you, if it changes
    /// what the popover shows.
    pub fn show(&self, view: &View, health: Option<&Health>) {
        let mut shown = self.shown.borrow_mut();
        let health = health.cloned();
        if (shown.mood, &shown.status, shown.paused, &shown.health)
            != (view.mood, &view.status, view.paused, &health)
        {
            shown.mood = view.mood;
            shown.status = view.status.clone();
            shown.paused = view.paused;
            shown.health = health;
            let _ = self.states.unbounded_send(shown.clone());
        }
    }
}

/// The popover's window: hidden until the tray opens it, borderless, above other windows, and
/// only hidden by closing it.
fn window() -> Config {
    let builder = WindowBuilder::new()
        .with_title(APP_NAME)
        .with_inner_size(LogicalSize::new(SIZE.0, SIZE.1));
    window_config(tray_popover_tao::window_builder(builder), Theme::Night)
        .with_close_behaviour(WindowCloseBehaviour::WindowHides)
}

#[component]
pub fn Popover(link: Link) -> Element {
    let mut state = use_signal(State::default);
    use_future({
        let link = link.clone();
        move || {
            let receiver = link.states.lock().unwrap().take();
            async move {
                let Some(mut states) = receiver else {
                    return;
                };
                while let Some(next) = states.next().await {
                    state.set(next);
                }
            }
        }
    });
    // Sent while the page first renders. The app's window acts on it later, after
    // dioxus-desktop has shown this window.
    use_hook({
        let link = link.clone();
        move || link.send(Action::Loaded)
    });
    let opened = use_memo(move || state.read().opened);
    // Focus the popover itself rather than its first button, so Tab starts at the top and no
    // focus ring shows for a click.
    use_effect(move || {
        if opened() > 0 {
            document::eval("document.querySelector('.popover')?.focus()");
        }
    });

    let current = state();
    let send = use_callback(move |action: Action| link.send(action));
    let calibrations = CALIBRATIONS.to_vec();
    let calibrate_title = if current.paused {
        "Resume to calibrate"
    } else {
        "Calibrate"
    };
    rsx! {
        div {
            class: "popover",
            "data-theme": Theme::Night.name(),
            tabindex: "-1",
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    event.prevent_default();
                    send(Action::Close);
                }
            },
            header { class: "popover-head",
                button {
                    class: "popover-brand",
                    title: "Open {APP_NAME}",
                    "aria-label": "Open {APP_NAME}",
                    onclick: move |_| send(Action::Open(Page::Camera)),
                    span { class: "wordmark", "slouch", span { class: "up", "up" } }
                }
                span {
                    class: if current.paused { "popover-state paused" } else { "popover-state" },
                    if current.paused { "Paused" } else { "On" }
                }
            }
            if let Some(health) = current.health.clone() {
                div { class: "popover-health mood-{current.mood:?}", role: "status",
                    div { class: "health-top",
                        PostureMark { mood: current.mood }
                        span { class: "health-title", "{health.title}" }
                    }
                    div { class: "health-bottom",
                        span { class: "health-count", "{health.count.clone().unwrap_or_default()}" }
                        if let Some(fix) = health.fix {
                            button { class: "health-fix", onclick: move |_| send(Action::Fix(fix)), "{fix.label}" }
                        }
                    }
                }
            } else {
                p { class: "popover-status mood-{current.mood:?}", role: "status",
                    PostureMark { mood: current.mood }
                    "{current.status}"
                }
            }
            div { class: "popover-actions",
                if current.paused {
                    button {
                        class: "control primary",
                        onclick: move |_| send(Action::Resume),
                        "Resume"
                    }
                } else {
                    DropMenu {
                        label: "Pause",
                        title: "Pause",
                        primary: true,
                        choices: PAUSE_NAMES.iter().map(|name| (*name, "")).collect::<Vec<_>>(),
                        on_choose: move |index: usize| send(Action::Pause(PAUSES[index].1)),
                    }
                }
                DropMenu {
                    label: "Calibrate",
                    primary: false,
                    disabled: current.paused,
                    title: calibrate_title,
                    choices: calibrations,
                    on_choose: move |index: usize| {
                        send(if index == 0 { Action::QuickCalibration } else { Action::GuidedCalibration })
                    },
                }
            }
            nav { class: "popover-views", "aria-label": "Open {APP_NAME} on",
                for page in Page::ALL {
                    button {
                        class: "popover-view",
                        title: "{page.label()}",
                        onclick: move |_| send(Action::Open(page)),
                        span { class: "icon", dangerous_inner_html: page.icon() }
                        span { "{page.label()}" }
                    }
                }
            }
            button { class: "popover-quit", onclick: move |_| send(Action::Quit), "Quit {APP_NAME}" }
        }
    }
}

/// A button opening a menu of `choices` below it, each a name with an optional tooltip. The
/// menu takes arrows, Home and End, and Esc closes it without closing the popover.
#[component]
fn DropMenu(
    label: &'static str,
    primary: bool,
    #[props(default)] disabled: bool,
    title: &'static str,
    choices: Vec<(&'static str, &'static str)>,
    on_choose: Callback<usize>,
) -> Element {
    let mut open = use_signal(|| false);
    let mut button = use_signal(|| None::<std::rc::Rc<MountedData>>);
    let mut close = move || {
        open.set(false);
        if let Some(button) = button() {
            spawn(async move {
                let _ = button.set_focus(true).await;
            });
        }
    };
    let class = if primary {
        "control primary drop-button"
    } else {
        "control drop-button"
    };
    rsx! {
        div { class: "drop",
            button {
                class,
                disabled,
                title,
                "aria-haspopup": "menu",
                "aria-expanded": "{open()}",
                onmounted: move |event| button.set(Some(event.data())),
                onclick: move |_| open.toggle(),
                "{label}"
                span { class: "icon caret", dangerous_inner_html: CARET_DOWN }
            }
            if open() {
                div { class: "backdrop", onclick: move |_| close() }
                div {
                    class: "drop-menu",
                    role: "menu",
                    "aria-label": label,
                    onkeydown: move |event| match event.key() {
                        Key::Escape => {
                            event.prevent_default();
                            event.stop_propagation();
                            close();
                        }
                        key => {
                            if move_focus(".drop-option", &key) {
                                event.prevent_default();
                            }
                        }
                    },
                    for (index, (name, detail)) in choices.into_iter().enumerate() {
                        button {
                            class: "drop-option",
                            role: "menuitem",
                            title: detail,
                            onmounted: move |event| async move {
                                if index == 0 {
                                    let _ = event.data().set_focus(true).await;
                                }
                            },
                            onclick: move |_| {
                                open.set(false);
                                on_choose(index);
                            },
                            "{name}"
                        }
                    }
                }
            }
        }
    }
}

const CARET_DOWN: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>"#;

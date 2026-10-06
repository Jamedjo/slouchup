//! SlouchUp's own nudge card, for when a notification can't show: a small window by the clock or
//! under the menu bar, with the nudge's look and buttons. It never takes focus, stays out of screen
//! shares where the system allows, and goes when you sit up.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Sender;
use dioxus::desktop::{LogicalPosition, LogicalSize, PendingDesktopContext, WindowBuilder, window};
use dioxus::prelude::*;

use crate::art::{self, Theme};
use crate::engine::{Command, NUDGE_PAUSE};
use crate::ui::{RisingUp, window_config};

const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 200.0;
/// Room left between the card and the screen's edge, and on Windows, the taskbar.
const MARGIN: f64 = 16.0;
const TASKBAR: f64 = 48.0;

/// The reason the card shows, shared with its window, which has its own virtual DOM.
#[derive(Clone)]
pub struct Shared(pub Arc<Mutex<String>>);

impl PartialEq for Shared {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Open the card for `reason`, by the clock on Windows and at the top right elsewhere.
pub fn open(reason: Shared, commands: Sender<Command>) -> PendingDesktopContext {
    let main = window();
    let place = main
        .primary_monitor()
        .or_else(|| main.current_monitor())
        .map(|monitor| {
            let screen = monitor.size().to_logical::<f64>(monitor.scale_factor());
            let origin = monitor.position().to_logical::<f64>(monitor.scale_factor());
            let x = origin.x + screen.width - WIDTH - MARGIN;
            let y = if cfg!(windows) {
                origin.y + screen.height - HEIGHT - MARGIN - TASKBAR
            } else {
                origin.y + MARGIN + 24.0
            };
            LogicalPosition::new(x, y)
        });
    let mut builder = WindowBuilder::new()
        .with_title("SlouchUp nudge")
        .with_inner_size(LogicalSize::new(WIDTH, HEIGHT))
        .with_resizable(false)
        .with_decorations(false)
        .with_always_on_top(true)
        .with_focused(false)
        // Windows would otherwise take focus each time the card is shown again. Elsewhere a window
        // that can't take focus doesn't take clicks either.
        .with_focusable(!cfg!(windows))
        .with_content_protection(true)
        .with_visible_on_all_workspaces(true);
    if let Some(place) = place {
        builder = builder.with_position(place);
    }
    // It's a nudge, not a window to switch to.
    #[cfg(windows)]
    {
        use dioxus::desktop::tao::platform::windows::WindowBuilderExtWindows;
        builder = builder.with_skip_taskbar(true);
    }
    #[cfg(target_os = "linux")]
    {
        use dioxus::desktop::tao::platform::unix::WindowBuilderExtUnix;
        builder = builder.with_skip_taskbar(true);
    }
    let dom = VirtualDom::new_with_props(
        Card,
        CardProps {
            reason,
            commands: Commands(commands),
        },
    );
    main.new_window(dom, window_config(builder, Theme::Night))
}

/// The engine's commands, for the card's buttons.
#[derive(Clone)]
pub struct Commands(Sender<Command>);

impl PartialEq for Commands {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[component]
fn Card(reason: Shared, commands: Commands) -> Element {
    let mut shown = use_signal(String::new);
    use_future(move || {
        let reason = reason.clone();
        async move {
            loop {
                let latest = reason.0.lock().unwrap().clone();
                if *shown.peek() != latest {
                    shown.set(latest);
                }
                futures_timer::Delay::new(Duration::from_millis(200)).await;
            }
        }
    });
    let send = use_callback(move |command| {
        let _ = commands.0.send(command);
    });
    rsx! {
        div { class: "nudge-card", "data-theme": Theme::Night.name(),
            div { class: "nudge-card-icon", dangerous_inner_html: art::nudge_icon_svg() }
            div { class: "nudge-card-body",
                div { class: "nudge-card-title", RisingUp { text: "Psst, sit up" } }
                p { class: "nudge-card-reason", "{shown} Shoulders back, chin up." }
                div { class: "buttons",
                    button { class: "button primary", onclick: move |_| send(Command::CloseCard), "I'm up" }
                    button { class: "button", onclick: move |_| send(Command::Pause(Some(NUDGE_PAUSE))), "Pause 30 min" }
                }
                p { class: "nudge-card-why", "Showing here because notifications can't show." }
            }
        }
    }
}

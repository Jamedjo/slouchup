//! The small window that opens when the camera window first closes: slouchup carries on in the
//! tray, and this is the quick way to quit it instead.

use dioxus::desktop::{LogicalSize, WindowBuilder, window};
use dioxus::prelude::*;

use crate::art::Theme;
use crate::config::APP_NAME;
use crate::ui::window_config;

/// Say SlouchUp carries on, in the tray, or with `no_tray`, how to get the window back.
pub fn open(no_tray: bool) {
    let config = window_config(
        WindowBuilder::new()
            .with_title(APP_NAME)
            .with_inner_size(LogicalSize::new(440.0, 190.0))
            .with_resizable(false)
            .with_always_on_top(true),
        Theme::Day,
    );
    let _ = window().new_window(
        VirtualDom::new_with_props(StillRunning, StillRunningProps { no_tray }),
        config,
    );
}

#[component]
fn StillRunning(no_tray: bool) -> Element {
    let where_it_is = if no_tray {
        "There's no tray here, so start SlouchUp again to bring its window back."
    } else {
        "It's in your tray, ready to nudge you when your head drops."
    };
    rsx! {
        div { class: "still-running", "data-theme": Theme::Day.name(),
            h1 {
                span { class: "wordmark", "slouch", span { class: "up", "up" } }
                " is still running"
            }
            p { "{where_it_is}" }
            div { class: "buttons",
                button { class: "button primary", autofocus: true, onclick: keep_running, "Keep running" }
                button { class: "button", onclick: quit, "Quit {APP_NAME}" }
            }
        }
    }
}

fn keep_running(_: MouseEvent) {
    window().close();
}

fn quit(_: MouseEvent) {
    crate::quit();
}

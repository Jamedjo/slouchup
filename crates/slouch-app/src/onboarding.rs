//! The first run's welcome: the camera stays off until you turn it on here. It also shows the tray
//! icon you'll be looking for afterwards, and where your desktop puts it.

use dioxus::prelude::*;

use crate::art::{Mood, Theme};
use crate::source;
use crate::ui::Eyes;

const MOODS: [(Mood, &str); 3] = [
    (Mood::Good, "Sitting tall"),
    (Mood::Bad, "Sinking"),
    (Mood::Idle, "Can't see you"),
];

/// Where this desktop shows tray icons.
const TRAY_HOME: &str = if cfg!(windows) {
    "In the corner of the taskbar, by the clock. If it's not there, click the ^ arrow, and drag it \
     out to keep it in sight."
} else if cfg!(target_os = "macos") {
    "In the menu bar, at the top right of your screen."
} else {
    "In your panel's tray, usually at the top or bottom of the screen."
};

/// `on_start` gets the chosen camera's id, or `None` for the first that works.
#[component]
pub fn Onboarding(
    chosen: Option<String>,
    on_start: EventHandler<Option<String>>,
    on_later: EventHandler<()>,
) -> Element {
    let cameras = use_hook(source::list_cameras);
    let mut camera = use_signal(|| chosen);
    rsx! {
        div { class: "settings onboarding", "data-theme": Theme::Day.name(),
            h1 { "Hi, I'm slouch", span { class: "up", "up" } }
            p { class: "lede",
                "I watch your posture through your webcam and nudge you once when you start to slouch. Everything happens on your computer."
            }
            section {
                h2 { "Camera" }
                select {
                    "aria-label": "Camera",
                    onchange: move |event| camera.set(Some(event.value()).filter(|id| !id.is_empty())),
                    option { value: "", selected: camera().is_none(), "First camera that works" }
                    for found in cameras {
                        option {
                            value: "{found.id}",
                            selected: camera().as_deref() == Some(found.id.as_str()),
                            "{found.name}"
                        }
                    }
                }
                p { class: "hint", "It stays off until you turn it on below." }
            }
            section {
                h2 { "Where to find me" }
                div { class: "tray-moods",
                    for (mood, label) in MOODS {
                        span { class: "tray-mood mood-{mood:?}", Eyes { mood }, "{label}" }
                    }
                }
                p { class: "hint", "{TRAY_HOME}" }
            }
            div { class: "buttons",
                button { class: "button primary", onclick: move |_| on_start.call(camera()), "Turn on the camera" }
                button { class: "button", onclick: move |_| on_later.call(()), "Not now" }
            }
        }
    }
}

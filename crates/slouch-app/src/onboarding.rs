//! The first run's welcome: the camera stays off until you turn it on here. It also shows the tray
//! icon you'll be looking for afterwards, and where your desktop puts it, and sends a test nudge to
//! check one can reach you.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use dioxus::prelude::*;

use crate::art::{Mood, Theme};
use crate::ui::{Bridge, PostureMark};
use crate::{notifier, source};

const MOODS: [(Mood, &str); 4] = [
    (Mood::Good, "Sitting tall"),
    (Mood::Bad, "Sinking"),
    (Mood::Idle, "Out of frame"),
    (Mood::Paused, "Paused"),
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

/// Where a nudge pops up on this desktop.
const NUDGE_HOME: &str = if cfg!(windows) {
    "It pops up by the clock."
} else if cfg!(target_os = "macos") {
    "It pops up at the top right of your screen."
} else {
    "It pops up near your panel."
};

/// Why a test nudge might not have shown, and what to do about it.
const NOT_SEEN: &str = if cfg!(windows) {
    "Notifications for SlouchUp may be off, or Do Not Disturb is on. Turn them on in Settings, then \
     try again."
} else if cfg!(target_os = "macos") {
    "Notifications for SlouchUp may be off, or a Focus is on. Turn on Allow notifications for \
     SlouchUp in System Settings, then try again."
} else {
    "Your desktop may not be running a notification service, or Do Not Disturb is on. Turn it off \
     from the clock menu, then try again."
};

const SETTINGS_NAME: Option<&str> = if cfg!(windows) {
    Some("Open Settings")
} else if cfg!(target_os = "macos") {
    Some("Open System Settings")
} else {
    None
};

/// How far the test nudge has got.
#[derive(Clone, Copy, PartialEq)]
enum Trial {
    NotYet,
    Sent,
    Seen,
    NotSeen,
}

/// How often to look for an answer from the test nudge's buttons.
const ANSWER_CHECK: Duration = Duration::from_millis(300);

/// Send a real nudge and ask whether it showed, so you know what to look for, and can fix it now
/// rather than wondering later why nothing happened.
#[component]
fn TryNudge() -> Element {
    let bridge = use_context::<Bridge>();
    let mut trial = use_signal(|| Trial::NotYet);
    let answered = use_hook(|| Arc::new(AtomicBool::new(false)));
    let send = {
        let answered = answered.clone();
        move |_| {
            answered.store(false, Ordering::Release);
            notifier::test_nudge(&bridge.files, answered.clone());
            trial.set(Trial::Sent);
        }
    };
    // Pressing a button on the nudge itself also says it was seen.
    use_future(move || {
        let answered = answered.clone();
        async move {
            loop {
                futures_timer::Delay::new(ANSWER_CHECK).await;
                if *trial.peek() == Trial::Sent && answered.load(Ordering::Acquire) {
                    trial.set(Trial::Seen);
                }
            }
        }
    });
    rsx! {
        section {
            h2 { "Nudges" }
            match trial() {
                Trial::NotYet => rsx! {
                    p { class: "hint", "This is how SlouchUp tells you to sit up. Send one now so you know what to look for." }
                    div { class: "buttons",
                        button { class: "button", onclick: send, "Try a nudge" }
                    }
                },
                Trial::Sent => rsx! {
                    p { class: "status-line", "Did you see it?" }
                    p { class: "hint", "{NUDGE_HOME}" }
                    div { class: "buttons",
                        button { class: "button", onclick: move |_| trial.set(Trial::Seen), "I saw it" }
                        button { class: "button", onclick: move |_| trial.set(Trial::NotSeen), "Nothing showed" }
                    }
                },
                Trial::Seen => rsx! {
                    p { class: "status-line", "That's a nudge" }
                    p { class: "hint", "You'll get one like it when your head starts to sink." }
                    if cfg!(target_os = "macos") {
                        p { class: "hint", "Did it slide away before you saw it? Choose Alerts for SlouchUp so each nudge waits for you." }
                        div { class: "buttons",
                            button { class: "button", onclick: |_| notifier::open_settings(), "Open System Settings" }
                        }
                    }
                },
                Trial::NotSeen => rsx! {
                    p { class: "status-line", "Nudges can't reach you yet" }
                    p { class: "hint", "{NOT_SEEN}" }
                    div { class: "buttons",
                        if let Some(label) = SETTINGS_NAME {
                            button { class: "button", onclick: |_| notifier::open_settings(), "{label}" }
                        }
                        button { class: "button", onclick: send, "Try again" }
                    }
                },
            }
        }
    }
}

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
            h1 {
                "Meet "
                span { class: "wordmark", "slouch", span { class: "up", "up" } }
            }
            p { class: "lede",
                "You get one nudge when your head starts to sink. Everything runs on your computer. Video never leaves it."
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
                h2 { "Where to find it" }
                div { class: "tray-moods",
                    for (mood, label) in MOODS {
                        span { class: "tray-mood mood-{mood:?}", PostureMark { mood }, "{label}" }
                    }
                }
                p { class: "hint", "{TRAY_HOME}" }
            }
            TryNudge {}
            div { class: "buttons",
                button { class: "button primary", onclick: move |_| on_start.call(camera()), "Turn on the camera" }
                button { class: "button", onclick: move |_| on_later.call(()), "Not now" }
            }
        }
    }
}

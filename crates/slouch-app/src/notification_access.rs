//! Whether a nudge can reach you, and what to do when it can't. macOS asks once and then leaves it
//! to System Settings, Windows can turn notifications off per app, for everyone, or by policy, and a
//! Linux desktop may have no notification service at all. Do Not Disturb and presenting are quiet
//! by your choice, so nudges are held then rather than pushed through.
//!
//! Healthy, nothing extra shows. Otherwise one health line takes the place of the tray's status,
//! the window shows it under its header, and Settings explains it in full.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use dioxus::prelude::*;

use crate::config::APP_NAME;

/// Whether nudges can show.
#[allow(dead_code)] // Each platform reads only some of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// They pop up. `fleeting` on macOS when they come as banners, which go by themselves.
    On {
        fleeting: bool,
    },
    /// Allowed, but they go straight to Notification Center without popping up.
    Quiet,
    /// macOS hasn't asked yet.
    NotAsked,
    /// Off for SlouchUp.
    Off,
    /// Off for every app.
    OffForAll,
    /// Turned off by your organisation, so there's nothing you can change.
    Policy,
    /// No notification service is running.
    NoService,
    DoNotDisturb,
    /// Presenting, or a full-screen app.
    Presenting,
}

impl Access {
    /// Whether a nudge due now goes unshown: missed when notifications are off, held when you've
    /// asked for quiet.
    pub fn holds_nudges(self) -> bool {
        !matches!(self, Access::On { .. } | Access::Quiet)
    }

    fn muted(self) -> bool {
        matches!(self, Access::DoNotDisturb | Access::Presenting)
    }
}

/// The latest reading, for the engine to decide whether a nudge can go out.
static CURRENT: Mutex<Option<Access>> = Mutex::new(None);
/// Set when macOS was asked but showed no prompt, as with an app it doesn't know the signer of.
static NO_PROMPT: AtomicBool = AtomicBool::new(false);

/// The latest reading, or `None` before the first or where it can't be read.
pub fn current() -> Option<Access> {
    *CURRENT.lock().unwrap()
}

/// Whether notifications may show, or `None` if it can't be read. It can take a moment, so it's
/// best called off the UI thread.
fn access() -> Option<Access> {
    #[cfg(target_os = "macos")]
    return crate::mac_notify::access();
    #[cfg(windows)]
    return crate::windows_shell::toast_access();
    #[cfg(not(any(windows, target_os = "macos")))]
    return Some(linux::access());
}

/// Have the system ask whether SlouchUp may show notifications, where it asks.
pub fn ask() {
    #[cfg(target_os = "macos")]
    crate::mac_notify::ask();
}

/// Open the system's notification settings, on SlouchUp's where it can.
pub fn open_settings() {
    #[cfg(target_os = "macos")]
    crate::mac_notify::open_settings();
    #[cfg(windows)]
    crate::windows_shell::open_settings("ms-settings:notifications");
}

/// What a fix button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Ask,
    OpenSettings,
    EndDoNotDisturb,
}

impl Action {
    pub fn run(self) {
        match self {
            Action::Ask => ask(),
            Action::OpenSettings => open_settings(),
            Action::EndDoNotDisturb => {
                #[cfg(windows)]
                crate::windows_shell::open_settings("ms-settings:quiethours");
                #[cfg(not(any(windows, target_os = "macos")))]
                linux::end_do_not_disturb();
            }
        }
    }
}

/// One button that sets it right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fix {
    pub label: &'static str,
    pub action: Action,
}

/// What's wrong, said in a line, and in full.
#[derive(Clone, Debug, PartialEq)]
pub struct Health {
    pub title: &'static str,
    /// The nudges it cost today, as "3 missed".
    pub count: Option<String>,
    /// The nudges it cost, then the cause.
    pub detail: String,
    pub fix: Option<Fix>,
    /// The full explanation, for the window and Settings.
    pub explanation: String,
}

const SETTINGS_NAME: &str = if cfg!(target_os = "macos") {
    "System Settings"
} else {
    "Settings"
};

/// What's wrong when nudges can't reach you, given how many have gone unshown today, or `None` when
/// they can.
pub fn health(access: Access, missed: u32) -> Option<Health> {
    let fix = |label, action| Some(Fix { label, action });
    let turn_on = fix("Turn on", Action::OpenSettings);
    let cant_reach = "Nudges can't reach you";
    let (title, cause, fix, explanation) = match access {
        Access::On { .. } => return None,
        Access::NotAsked if NO_PROMPT.load(Ordering::Relaxed) => (
            cant_reach,
            "macOS didn't ask",
            fix("Open System Settings", Action::OpenSettings),
            format!(
                "macOS didn't ask this time. Turn on notifications for {APP_NAME} in System Settings."
            ),
        ),
        Access::NotAsked => (
            cant_reach,
            "notifications aren't allowed yet",
            fix("Allow notifications", Action::Ask),
            "Nudges come as notifications. Choose Allow when macOS asks.".to_string(),
        ),
        Access::Off if cfg!(target_os = "macos") => (
            cant_reach,
            "notifications are off",
            turn_on,
            format!(
                "Nudges can't show yet. Turn on Allow notifications for {APP_NAME} in System Settings."
            ),
        ),
        Access::Off => (
            cant_reach,
            "notifications are off",
            turn_on,
            format!(
                "Notifications for {APP_NAME} are off in {SETTINGS_NAME}. Turn them on so nudges can show."
            ),
        ),
        Access::OffForAll => (
            cant_reach,
            "all notifications are off",
            turn_on,
            "Notifications are off for every app on this PC. Turn on Notifications in Settings."
                .to_string(),
        ),
        Access::Policy => (
            cant_reach,
            "your organisation turned them off",
            None,
            "Your organisation has turned notifications off on this PC.".to_string(),
        ),
        Access::NoService => (
            cant_reach,
            "no notification service",
            None,
            "Your desktop isn't running a notification service, so nudges can't show.".to_string(),
        ),
        Access::Quiet => (
            "Nudges are going quietly",
            "straight to Notification Center",
            fix("Choose Alerts", Action::OpenSettings),
            format!(
                "Nudges are going straight to Notification Center. Choose Alerts for {APP_NAME} so they pop up."
            ),
        ),
        Access::DoNotDisturb => (
            "Do Not Disturb is holding nudges",
            "",
            fix("Turn it off", Action::EndDoNotDisturb),
            format!("Do Not Disturb is on, so {APP_NAME} holds its nudges till it's off."),
        ),
        Access::Presenting => (
            "Nudges held while you present",
            "",
            None,
            "You're presenting or in a full-screen app, so nudges wait till you're done."
                .to_string(),
        ),
    };
    let count = match (missed, access.muted()) {
        (0, _) => None,
        (n, true) => Some(format!("{n} held")),
        (n, false) => Some(format!("{n} missed")),
    };
    let detail = match (&count, cause) {
        (Some(count), "") => count.clone(),
        (Some(count), cause) => format!("{count} · {cause}"),
        (None, cause) => cause.to_string(),
    };
    Some(Health {
        title,
        count,
        detail,
        fix,
        explanation,
    })
}

/// How many nudges went unshown today, in words, for the window and Settings.
fn missed_sentence(access: Access, missed: u32) -> Option<String> {
    let nudges = if missed == 1 { "nudge" } else { "nudges" };
    match missed {
        0 => None,
        _ if access.muted() => Some(format!("{missed} {nudges} held today.")),
        _ => Some(format!("{missed} {nudges} missed today.")),
    }
}

/// Whether notifications may show, kept up to date for the whole app through context.
#[derive(Clone, Copy)]
pub struct Shared(pub Signal<Option<Access>>);

/// How often to look again, since it's changed in the system's settings, outside the app.
const RECHECK: Duration = Duration::from_secs(2);
/// How long macOS gets to show its prompt before it's taken as not coming.
const PROMPT_WAIT: Duration = Duration::from_secs(4);

/// Start keeping `Shared` up to date.
pub fn use_access() -> Shared {
    let mut access = use_signal(|| None);
    use_future(move || async move {
        loop {
            let (found, read) = futures_channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = found.send(self::access());
            });
            if let Ok(now) = read.await
                && *access.peek() != now
            {
                *CURRENT.lock().unwrap() = now;
                access.set(now);
            }
            futures_timer::Delay::new(RECHECK).await;
        }
    });
    use_context_provider(|| Shared(access))
}

/// Carry out `fix`, and when it's asking macOS, notice if no prompt comes.
pub fn apply(fix: Fix) {
    fix.action.run();
    if fix.action == Action::Ask {
        std::thread::spawn(|| {
            std::thread::sleep(PROMPT_WAIT);
            if current() == Some(Access::NotAsked) {
                NO_PROMPT.store(true, Ordering::Relaxed);
            }
        });
    }
}

/// The Notifications section, for the welcome and the Settings view: whether nudges can reach
/// you, why not, and the fix. In the welcome, it shows only when something needs doing.
#[component]
pub fn NotificationsSection(welcome: bool, missed: u32) -> Element {
    let Shared(access) = use_context::<Shared>();
    let Some(now) = access() else {
        return rsx! {};
    };
    let health = health(now, missed);
    if welcome && health.is_none() {
        return rsx! {};
    }
    let alerts = (now == Access::On { fleeting: true }).then_some(Fix {
        label: "Choose Alerts",
        action: Action::OpenSettings,
    });
    rsx! {
        section {
            div { class: "section-head", h2 { "Notifications" } }
            match health {
                Some(health) => rsx! {
                    p { class: "status-line", "{health.title}" }
                    p { class: "hint", "{health.explanation}" }
                    if let Some(sentence) = missed_sentence(now, missed) {
                        p { class: "hint", "{sentence}" }
                    }
                    if let Some(fix) = health.fix {
                        button { class: "button primary", onclick: move |_| apply(fix), "{fix.label}" }
                    }
                },
                None => rsx! {
                    p { class: "status-line", "Nudges can reach you" }
                    if let Some(fix) = alerts {
                        p { class: "hint", "Did it slide away before you saw it? Choose Alerts so each nudge waits for you." }
                        button { class: "button", onclick: move |_| apply(fix), "{fix.label}" }
                    }
                },
            }
        }
    }
}

/// The window's strip under its header while nudges can't reach you, until you say not now.
#[component]
pub fn HealthStrip(missed: u32) -> Element {
    let Shared(access) = use_context::<Shared>();
    // Not now hides it for this reading; a different problem brings it back.
    let mut dismissed = use_signal(|| None::<Access>);
    let Some(now) = access() else {
        return rsx! {};
    };
    let Some(health) = health(now, missed) else {
        return rsx! {};
    };
    if dismissed() == Some(now) {
        return rsx! {};
    }
    let sentence = missed_sentence(now, missed).unwrap_or_default();
    rsx! {
        div { class: "health-strip", role: "status",
            p { "{health.explanation} {sentence}" }
            div { class: "buttons",
                if let Some(fix) = health.fix {
                    button { class: "button primary", onclick: move |_| apply(fix), "{fix.label}" }
                }
                button { class: "button", onclick: move |_| dismissed.set(Some(now)), "Not now" }
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod linux {
    use gio::prelude::*;

    use super::Access;

    const GNOME_NOTIFICATIONS: &str = "org.gnome.desktop.notifications";

    pub fn access() -> Access {
        if notify_rust::get_server_information().is_err() {
            Access::NoService
        } else if gnome_settings().is_some_and(|settings| !settings.boolean("show-banners")) {
            Access::DoNotDisturb
        } else {
            Access::On { fleeting: false }
        }
    }

    /// GNOME's notification settings, where GNOME is installed; asking for a missing schema aborts.
    fn gnome_settings() -> Option<gio::Settings> {
        gio::SettingsSchemaSource::default()?.lookup(GNOME_NOTIFICATIONS, true)?;
        Some(gio::Settings::new(GNOME_NOTIFICATIONS))
    }

    pub fn end_do_not_disturb() {
        if let Some(settings) = gnome_settings()
            && let Err(error) = settings.set_boolean("show-banners", true)
        {
            tracing::warn!("couldn't turn off Do Not Disturb: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_says_nothing() {
        assert_eq!(health(Access::On { fleeting: true }, 3), None);
    }

    #[test]
    fn health_counts_missed_nudges_and_offers_the_fix() {
        let off = health(Access::Off, 3).unwrap();
        assert_eq!(off.title, "Nudges can't reach you");
        assert_eq!(off.count.as_deref(), Some("3 missed"));
        assert_eq!(off.detail, "3 missed · notifications are off");
        assert_eq!(off.fix.map(|fix| fix.label), Some("Turn on"));
        let held = health(Access::Presenting, 2).unwrap();
        assert_eq!(held.title, "Nudges held while you present");
        assert_eq!(held.count.as_deref(), Some("2 held"));
        assert_eq!(held.fix, None);
    }

    #[test]
    fn only_muted_and_blocked_hold_nudges() {
        assert!(!Access::On { fleeting: false }.holds_nudges());
        assert!(!Access::Quiet.holds_nudges());
        assert!(Access::Off.holds_nudges());
        assert!(Access::DoNotDisturb.holds_nudges());
    }
}

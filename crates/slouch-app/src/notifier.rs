//! Desktop notifications, branded as this app rather than a generic sender.
//!
//! The nudge has "I'm up" and "Pause 30 min" buttons, but only "Pause 30 min" on macOS, which shows
//! one action button beside its own Close. Freedesktop servers can also update a notification
//! already on screen. macOS and Windows fire and forget, so there the nudge isn't withdrawn when you
//! sit up; pausing is in the tray's popover everywhere.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use notify_rust::{Notification, Timeout};

use crate::art::Files;
use crate::config::APP_NAME;

/// The installed app's `CFBundleIdentifier`, from `packaging/macos/Info.plist`.
#[cfg(target_os = "macos")]
const BUNDLE_ID: &str = "dev.weareframes.slouchup";

/// Set by `--demo --card`, to show every nudge in SlouchUp's own card instead.
pub static FORCE_CARD: AtomicBool = AtomicBool::new(false);

/// The nudge's title, friendly and short.
const NUDGE: &str = "Psst, sit up";

/// Called, from another thread, when the nudge's Pause button is pressed.
pub type OnPause = Arc<dyn Fn() + Send + Sync>;

const PAUSE: &str = "pause";
const QUIT: &str = "quit";

/// "I'm up" needs nothing doing: the notification closes and the tracker sees you sit up. macOS
/// shows a single action button, so there the notification's own Close stands in for it.
fn with_buttons(notification: &mut Notification) -> &mut Notification {
    if !cfg!(target_os = "macos") {
        notification.action("up", "I'm up");
    }
    notification.action(PAUSE, "Pause 30 min")
}

pub struct Notifier {
    files: Files,
    /// The one slouch notification, so repeats replace it and sitting up can dismiss it.
    #[cfg(all(unix, not(target_os = "macos")))]
    nudge: Option<xdg::Nudge>,
    /// Whether a nudge's buttons are still waiting for an answer.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    awaiting_answer: Arc<AtomicBool>,
    on_pause: OnPause,
}

impl Notifier {
    pub fn new(files: Files, on_pause: OnPause) -> Self {
        Self {
            files,
            #[cfg(all(unix, not(target_os = "macos")))]
            nudge: None,
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            awaiting_answer: Arc::default(),
            on_pause,
        }
    }

    /// A short notice.
    pub fn info(&self, summary: &str, body: &str) {
        show(&base(&self.files, &self.files.icon, summary, body, 3000));
    }

    /// Show the nudge, and say whether it could be.
    #[cfg(all(unix, not(target_os = "macos")))]
    pub fn nag(&mut self, reason: &str) -> bool {
        if FORCE_CARD.load(Ordering::Relaxed) {
            return false;
        }
        let body = nudge_body(reason);
        if let Some(nudge) = self.nudge.as_mut().filter(|n| n.is_open()) {
            nudge.update(NUDGE, &escape(&body));
            return true;
        }
        let mut notification = base(&self.files, &self.files.nudge_icon, NUDGE, &body, 15000);
        match with_buttons(&mut notification).show() {
            Ok(handle) => {
                self.nudge = Some(xdg::Nudge::listen(handle, self.on_pause.clone()));
                true
            }
            Err(error) => {
                tracing::warn!("notification failed: {error}");
                false
            }
        }
    }

    /// A thread waits until the nudge is answered or dismissed, which on macOS can be long after
    /// its banner has gone, so only one waits at a time, and nudges meanwhile come without buttons.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    pub fn nag(&mut self, reason: &str) -> bool {
        if FORCE_CARD.load(Ordering::Relaxed) {
            return false;
        }
        let mut notification = base(
            &self.files,
            &self.files.nudge_icon,
            NUDGE,
            &nudge_body(reason),
            15000,
        );
        if self.awaiting_answer.swap(true, Ordering::AcqRel) {
            show(&notification);
            return true;
        }
        match with_buttons(&mut notification).show() {
            Ok(handle) => {
                let awaiting_answer = self.awaiting_answer.clone();
                let on_pause = self.on_pause.clone();
                std::thread::spawn(move || {
                    let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
                        if matches!(response, notify_rust::NotificationResponse::Action(key) if key == PAUSE) {
                            on_pause();
                        }
                    });
                    awaiting_answer.store(false, Ordering::Release);
                });
                true
            }
            Err(error) => {
                self.awaiting_answer.store(false, Ordering::Release);
                tracing::warn!("notification failed: {error}");
                false
            }
        }
    }

    pub fn dismiss(&mut self) {
        #[cfg(all(unix, not(target_os = "macos")))]
        if let Some(nudge) = self.nudge.take() {
            nudge.close();
        }
    }
}

fn base(files: &Files, icon: &Path, summary: &str, body: &str, timeout_ms: u32) -> Notification {
    let mut notification = Notification::new();
    notification
        .appname(APP_NAME)
        .summary(summary)
        .body(&escape(body))
        .icon(&icon.to_string_lossy())
        .timeout(Timeout::Milliseconds(timeout_ms));
    #[cfg(all(unix, not(target_os = "macos")))]
    notification.hint(notify_rust::Hint::DesktopEntry(
        crate::config::DESKTOP_ID.into(),
    ));
    #[cfg(target_os = "macos")]
    send_as_this_app();
    #[cfg(windows)]
    if let Some(sender) = crate::windows_shell::toast_sender(&files.icon) {
        notification.app_id(sender);
    }
    #[cfg(not(windows))]
    let _ = files;
    notification
}

/// Says the app carries on in the tray once its window closes.
pub fn still_running(files: &Files) {
    in_tray(
        files,
        &format!("{APP_NAME} is still running"),
        "It's in your tray, ready to nudge you when your head drops.",
    );
}

/// Says the app waits in the tray with the camera off, and how to turn it on.
pub fn camera_off(files: &Files) {
    in_tray(
        files,
        &format!("{APP_NAME} is in your tray"),
        "The camera's off. Click its icon and choose Resume when you're ready.",
    );
}

/// A notice that the app is in the tray, with a button to quit it instead.
fn in_tray(files: &Files, summary: &str, body: &str) {
    let mut notification = base(files, &files.icon, summary, body, 8000);
    notification.action(QUIT, "Quit");
    match notification.show() {
        // Waits until the notification is answered or dismissed.
        Ok(handle) => {
            std::thread::spawn(move || {
                let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
                    if matches!(response, notify_rust::NotificationResponse::Action(key) if key == QUIT) {
                        crate::quit();
                    }
                });
            });
        }
        Err(error) => tracing::warn!("notification failed: {error}"),
    }
}

/// Without a sender, the first notification asks AppleScript for an app called "use_default",
/// which hangs until the Apple event times out two minutes later.
#[cfg(target_os = "macos")]
fn send_as_this_app() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if let Err(error) = notify_rust::set_application(BUNDLE_ID) {
            tracing::debug!("notifications not sent as {BUNDLE_ID}: {error}");
        }
    });
}

/// macOS waits for each notification to be delivered, so it sends from its own thread there.
fn show(notification: &Notification) {
    #[cfg(target_os = "macos")]
    {
        let notification = notification.clone();
        std::thread::spawn(move || send(&notification));
    }
    #[cfg(not(target_os = "macos"))]
    send(notification);
}

fn send(notification: &Notification) {
    if let Err(error) = notification.show() {
        tracing::warn!("notification failed: {error}");
    }
}

/// The nudge on freedesktop servers, and the thread that hears its buttons.
#[cfg(all(unix, not(target_os = "macos")))]
mod xdg {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use notify_rust::{ActionResponse, NotificationHandle};

    use super::{OnPause, PAUSE};

    pub struct Nudge {
        handle: NotificationHandle,
        /// Cleared once the server closes it, after which it can't be updated or answered.
        open: Arc<AtomicBool>,
    }

    impl Nudge {
        /// Listen for a button press or the notification closing.
        pub fn listen(handle: NotificationHandle, on_pause: OnPause) -> Self {
            let open = Arc::new(AtomicBool::new(true));
            let (id, closed) = (handle.id(), open.clone());
            std::thread::spawn(move || {
                let _ = notify_rust::handle_action(id, |response| {
                    if matches!(response, ActionResponse::Custom(PAUSE)) {
                        on_pause();
                    }
                });
                closed.store(false, Ordering::Relaxed);
            });
            Self { handle, open }
        }

        pub fn is_open(&self) -> bool {
            self.open.load(Ordering::Relaxed)
        }

        pub fn update(&mut self, summary: &str, body: &str) {
            self.handle.summary(summary).body(body);
            if let Err(error) = self.handle.update() {
                tracing::warn!("notification update failed: {error}");
            }
        }

        pub fn close(self) {
            self.handle.close();
        }
    }
}

/// What changed, in the detector's own numbers, then what to do about it.
fn nudge_body(reason: &str) -> String {
    format!("{reason} Shoulders back, chin up.")
}

/// Text made safe for a notification body, which freedesktop servers read as markup.
fn escape(text: &str) -> String {
    if cfg!(all(unix, not(target_os = "macos"))) {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    } else {
        text.to_string()
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn bundle_id_matches_the_info_plist() {
        let plist = include_str!("../../../packaging/macos/Info.plist");
        assert!(plist.contains(&format!("<string>{}</string>", super::BUNDLE_ID)));
    }
}

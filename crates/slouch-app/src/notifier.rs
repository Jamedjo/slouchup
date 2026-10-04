//! Desktop notifications, branded as this app rather than a generic sender.
//!
//! The nudge has "I'm up" and "Snooze" buttons, but only "Snooze" on macOS, which shows one action
//! button beside its own Close. Freedesktop servers can also update a notification already on
//! screen. macOS and Windows fire and forget, so there the nudge isn't withdrawn when you sit up;
//! snoozing is in the tray menu everywhere.

use std::path::Path;
use std::sync::Arc;
#[cfg(not(all(unix, not(target_os = "macos"))))]
use std::sync::atomic::{AtomicBool, Ordering};

use notify_rust::{Notification, Timeout};

use crate::art::Files;
use crate::config::APP_NAME;

/// The nudge's title, friendly and short.
const NUDGE: &str = "Psst, sit up";

/// Called, from another thread, when the nudge's Snooze button is pressed.
pub type OnSnooze = Arc<dyn Fn() + Send + Sync>;

const SNOOZE: &str = "snooze";

/// "I'm up" needs nothing doing: the notification closes and the tracker sees you sit up. macOS
/// shows a single action button, so there the notification's own Close stands in for it.
fn with_buttons(notification: &mut Notification) -> &mut Notification {
    if !cfg!(target_os = "macos") {
        notification.action("up", "I'm up");
    }
    notification.action(SNOOZE, "Snooze")
}

pub struct Notifier {
    files: Files,
    /// The one slouch notification, so repeats replace it and sitting up can dismiss it.
    #[cfg(all(unix, not(target_os = "macos")))]
    nudge: Option<xdg::Nudge>,
    /// Whether a nudge's buttons are still waiting for an answer.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    awaiting_answer: Arc<AtomicBool>,
    on_snooze: OnSnooze,
}

impl Notifier {
    pub fn new(files: Files, on_snooze: OnSnooze) -> Self {
        Self {
            files,
            #[cfg(all(unix, not(target_os = "macos")))]
            nudge: None,
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            awaiting_answer: Arc::default(),
            on_snooze,
        }
    }

    fn base(&self, icon: &Path, summary: &str, body: &str, timeout_ms: u32) -> Notification {
        let mut notification = Notification::new();
        notification
            .appname(APP_NAME)
            .summary(summary)
            .body(&escape(body))
            .icon(&icon.to_string_lossy())
            .timeout(Timeout::Milliseconds(timeout_ms));
        #[cfg(all(unix, not(target_os = "macos")))]
        notification.hint(notify_rust::Hint::DesktopEntry(
            crate::config::APP_ID.into(),
        ));
        #[cfg(windows)]
        if let Some(sender) = crate::windows_shell::toast_sender(&self.files.icon) {
            notification.app_id(sender);
        }
        notification
    }

    /// A short notice.
    pub fn info(&self, summary: &str, body: &str) {
        show(&self.base(&self.files.icon, summary, body, 3000));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    pub fn nag(&mut self, reason: &str) {
        let body = nudge_body(reason);
        if let Some(nudge) = self.nudge.as_mut().filter(|n| n.is_open()) {
            nudge.update(NUDGE, &escape(&body));
            return;
        }
        let mut notification = self.base(&self.files.nudge_icon, NUDGE, &body, 15000);
        match with_buttons(&mut notification).show() {
            Ok(handle) => self.nudge = Some(xdg::Nudge::listen(handle, self.on_snooze.clone())),
            Err(error) => tracing::warn!("notification failed: {error}"),
        }
    }

    /// A thread waits until the nudge is answered or dismissed, which on macOS can be long after
    /// its banner has gone, so only one waits at a time, and nudges meanwhile come without buttons.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    pub fn nag(&mut self, reason: &str) {
        let mut notification = self.base(&self.files.nudge_icon, NUDGE, &nudge_body(reason), 15000);
        if self.awaiting_answer.swap(true, Ordering::AcqRel) {
            show(&notification);
            return;
        }
        match with_buttons(&mut notification).show() {
            Ok(handle) => {
                let awaiting_answer = self.awaiting_answer.clone();
                let on_snooze = self.on_snooze.clone();
                std::thread::spawn(move || {
                    let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
                        if matches!(response, notify_rust::NotificationResponse::Action(key) if key == SNOOZE) {
                            on_snooze();
                        }
                    });
                    awaiting_answer.store(false, Ordering::Release);
                });
            }
            Err(error) => {
                self.awaiting_answer.store(false, Ordering::Release);
                tracing::warn!("notification failed: {error}");
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

fn show(notification: &Notification) {
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

    use super::{OnSnooze, SNOOZE};

    pub struct Nudge {
        handle: NotificationHandle,
        /// Cleared once the server closes it, after which it can't be updated or answered.
        open: Arc<AtomicBool>,
    }

    impl Nudge {
        /// Listen for a button press or the notification closing.
        pub fn listen(handle: NotificationHandle, on_snooze: OnSnooze) -> Self {
            let open = Arc::new(AtomicBool::new(true));
            let (id, closed) = (handle.id(), open.clone());
            std::thread::spawn(move || {
                let _ = notify_rust::handle_action(id, |response| {
                    if matches!(response, ActionResponse::Custom(SNOOZE)) {
                        on_snooze();
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

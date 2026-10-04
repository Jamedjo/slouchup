//! Desktop notifications, branded as this app rather than a generic sender.
//!
//! Freedesktop notification servers can update a notification already on screen, and give the
//! nudge its "I'm up" and "Snooze" buttons. macOS and Windows fire and forget, so there the nudge
//! has no buttons and isn't withdrawn when you sit up; snoozing is in the tray menu everywhere.

use std::path::Path;
use std::sync::Arc;

use notify_rust::{Notification, Timeout};

use crate::art::Files;
use crate::config::APP_NAME;

/// The nudge's title, friendly and short.
const NUDGE: &str = "Psst, sit up";

/// Called, from another thread, when the nudge's Snooze button is pressed.
pub type OnSnooze = Arc<dyn Fn() + Send + Sync>;

pub struct Notifier {
    files: Files,
    /// The one slouch notification, so repeats replace it and sitting up can dismiss it.
    #[cfg(all(unix, not(target_os = "macos")))]
    nudge: Option<xdg::Nudge>,
    #[cfg(all(unix, not(target_os = "macos")))]
    on_snooze: OnSnooze,
}

impl Notifier {
    pub fn new(files: Files, on_snooze: OnSnooze) -> Self {
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let _ = on_snooze;
        Self {
            files,
            #[cfg(all(unix, not(target_os = "macos")))]
            nudge: None,
            #[cfg(all(unix, not(target_os = "macos")))]
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
        notification
    }

    /// A short notice.
    pub fn info(&self, summary: &str, body: &str) {
        if let Err(error) = self.base(&self.files.icon, summary, body, 3000).show() {
            tracing::warn!("notification failed: {error}");
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    pub fn nag(&mut self, reason: &str) {
        let body = nudge_body(reason);
        if let Some(nudge) = self.nudge.as_mut().filter(|n| n.is_open()) {
            nudge.update(NUDGE, &escape(&body));
            return;
        }
        let mut notification = self.base(&self.files.nudge_icon, NUDGE, &body, 15000);
        notification
            .action(xdg::IM_UP, "I'm up")
            .action(xdg::SNOOZE, "Snooze");
        match notification.show() {
            Ok(handle) => self.nudge = Some(xdg::Nudge::listen(handle, self.on_snooze.clone())),
            Err(error) => tracing::warn!("notification failed: {error}"),
        }
    }

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    pub fn nag(&mut self, reason: &str) {
        let body = nudge_body(reason);
        if let Err(error) = self
            .base(&self.files.nudge_icon, NUDGE, &body, 15000)
            .show()
        {
            tracing::warn!("notification failed: {error}");
        }
    }

    pub fn dismiss(&mut self) {
        #[cfg(all(unix, not(target_os = "macos")))]
        if let Some(nudge) = self.nudge.take() {
            nudge.close();
        }
    }
}

/// The nudge on freedesktop servers, and the thread that hears its buttons.
#[cfg(all(unix, not(target_os = "macos")))]
mod xdg {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use notify_rust::{ActionResponse, NotificationHandle};

    use super::OnSnooze;

    pub const IM_UP: &str = "up";
    pub const SNOOZE: &str = "snooze";

    pub struct Nudge {
        handle: NotificationHandle,
        /// Cleared once the server closes it, after which it can't be updated or answered.
        open: Arc<AtomicBool>,
    }

    impl Nudge {
        /// Listen for a button press or the notification closing. "I'm up" needs nothing doing:
        /// the server closes the nudge and the tracker sees you sit up.
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

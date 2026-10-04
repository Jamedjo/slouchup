//! Desktop notifications, branded as this app rather than a generic sender.
//!
//! Freedesktop notification servers can update a notification already on screen. macOS and
//! Windows fire and forget, so there the nudge isn't withdrawn when you sit up.

use std::path::Path;

use notify_rust::{Notification, Timeout};

use crate::art::Files;
use crate::config::APP_NAME;

/// The nudge's title, friendly and short.
const NUDGE: &str = "Psst, sit up";

pub struct Notifier {
    files: Files,
    /// The one slouch notification, so repeats replace it and sitting up can dismiss it.
    #[cfg(all(unix, not(target_os = "macos")))]
    nudge: Option<notify_rust::NotificationHandle>,
}

impl Notifier {
    pub fn new(files: Files) -> Self {
        Self {
            files,
            #[cfg(all(unix, not(target_os = "macos")))]
            nudge: None,
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
        if let Some(handle) = &mut self.nudge {
            handle.summary(NUDGE).body(&escape(&body));
            if let Err(error) = handle.update() {
                tracing::warn!("notification update failed: {error}");
            }
            return;
        }
        match self
            .base(&self.files.nudge_icon, NUDGE, &body, 15000)
            .show()
        {
            Ok(handle) => self.nudge = Some(handle),
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
        if let Some(handle) = self.nudge.take() {
            handle.close();
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

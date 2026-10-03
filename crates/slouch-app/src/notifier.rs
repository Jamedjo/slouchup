//! Desktop notifications, branded as this app rather than a generic sender.

use notify_rust::{Hint, Notification, NotificationHandle, Timeout};

use crate::art::Files;
use crate::config::{APP_ID, APP_NAME};

pub struct Notifier {
    files: Files,
    /// The one slouch notification, so repeats replace it and sitting up can dismiss it.
    nag: Option<NotificationHandle>,
}

impl Notifier {
    pub fn new(files: Files) -> Self {
        Self { files, nag: None }
    }

    fn base(&self, summary: &str, body: &str, timeout_ms: u32) -> Notification {
        let mut notification = Notification::new();
        notification
            .appname(APP_NAME)
            .summary(summary)
            .body(body)
            .icon(&self.files.icon.to_string_lossy())
            .hint(Hint::DesktopEntry(APP_ID.into()))
            .timeout(Timeout::Milliseconds(timeout_ms));
        notification
    }

    pub fn info(&self, summary: &str, body_markup: &str) {
        if let Err(error) = self.base(summary, body_markup, 3000).show() {
            tracing::warn!("notification failed: {error}");
        }
    }

    pub fn nag(&mut self, reason: &str) {
        let body = format!(
            "<span foreground=\"#ff5f87\"><b>{reason}</b></span>\n\
             <span foreground=\"#5fffaf\">Shoulders back,</span> \
             <span foreground=\"#5fafff\">chin up!</span>\n\
             <img src=\"{}\" alt=\"sit up\"/>",
            self.files.banner.display()
        );
        let summary = "🦒 Stop slouching!";
        if let Some(handle) = &mut self.nag {
            handle.summary(summary).body(&body);
            if let Err(error) = handle.update() {
                tracing::warn!("notification update failed: {error}");
            }
            return;
        }
        match self.base(summary, &body, 15000).show() {
            Ok(handle) => self.nag = Some(handle),
            Err(error) => tracing::warn!("notification failed: {error}"),
        }
    }

    pub fn dismiss(&mut self) {
        if let Some(handle) = self.nag.take() {
            handle.close();
        }
    }
}

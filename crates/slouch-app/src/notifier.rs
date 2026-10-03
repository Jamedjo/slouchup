//! Desktop notifications, branded as this app rather than a generic sender.
//!
//! Freedesktop notification servers take coloured markup, an inline banner, and updates to a
//! notification already on screen. macOS and Windows take plain text and fire-and-forget, so
//! there the nag is plain and isn't withdrawn when you sit up.

use notify_rust::{Notification, Timeout};

use crate::art::Files;
use crate::config::APP_NAME;

pub struct Notifier {
    files: Files,
    /// The one slouch notification, so repeats replace it and sitting up can dismiss it.
    #[cfg(all(unix, not(target_os = "macos")))]
    nag: Option<notify_rust::NotificationHandle>,
}

impl Notifier {
    pub fn new(files: Files) -> Self {
        Self {
            files,
            #[cfg(all(unix, not(target_os = "macos")))]
            nag: None,
        }
    }

    fn base(&self, summary: &str, body: &str, timeout_ms: u32) -> Notification {
        let mut notification = Notification::new();
        notification
            .appname(APP_NAME)
            .summary(summary)
            .body(body)
            .icon(&self.files.icon.to_string_lossy())
            .timeout(Timeout::Milliseconds(timeout_ms));
        #[cfg(all(unix, not(target_os = "macos")))]
        notification.hint(notify_rust::Hint::DesktopEntry(
            crate::config::APP_ID.into(),
        ));
        notification
    }

    /// A short notice. `body_markup` may use Pango spans; they are stripped where unsupported.
    pub fn info(&self, summary: &str, body_markup: &str) {
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let body_markup = &strip_markup(body_markup);
        if let Err(error) = self.base(summary, body_markup, 3000).show() {
            tracing::warn!("notification failed: {error}");
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    pub fn nag(&mut self, reason: &str) {
        let reason = escape(reason);
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

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    pub fn nag(&mut self, reason: &str) {
        let body = format!("{reason}\nShoulders back, chin up!");
        if let Err(error) = self.base("🦒 Stop slouching!", &body, 15000).show() {
            tracing::warn!("notification failed: {error}");
        }
    }

    pub fn dismiss(&mut self) {
        #[cfg(all(unix, not(target_os = "macos")))]
        if let Some(handle) = self.nag.take() {
            handle.close();
        }
    }
}

/// Text made safe to put in a notification body, which Linux servers read as markup.
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Drop `<…>` tags, for notification systems that would show them literally.
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn strip_markup(markup: &str) -> String {
    let mut text = String::with_capacity(markup.len());
    let mut in_tag = false;
    for c in markup.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    text
}

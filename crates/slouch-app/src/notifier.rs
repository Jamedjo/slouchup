//! Desktop notifications, branded as this app rather than a generic sender.
//!
//! The nudge has "I'm up" and "Pause 30 min" buttons, but only "Pause 30 min" on macOS, which shows
//! one action button beside its own Close. Freedesktop servers and macOS can also take the nudge
//! away when you sit up. Windows fires and forgets, so there it stays; pausing is in the tray's
//! popover everywhere.

#[cfg(not(target_os = "macos"))]
use std::path::Path;
use std::sync::Arc;
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(target_os = "macos"))]
use notify_rust::{Notification, Timeout};

use crate::art::Files;
use crate::config::APP_NAME;

/// The nudge's title, friendly and short.
const NUDGE: &str = "Psst, sit up";

/// Called, from another thread, when the nudge's Pause button is pressed.
pub type OnPause = Arc<dyn Fn() + Send + Sync>;

const PAUSE: &str = "pause";
const QUIT: &str = "quit";

/// "I'm up" needs nothing doing: the notification closes and the tracker sees you sit up.
#[cfg(not(target_os = "macos"))]
fn with_buttons(notification: &mut Notification) -> &mut Notification {
    notification.action("up", "I'm up");
    notification.action(PAUSE, "Pause 30 min")
}

pub struct Notifier {
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    files: Files,
    /// The one slouch notification, so repeats replace it and sitting up can dismiss it.
    #[cfg(all(unix, not(target_os = "macos")))]
    nudge: Option<xdg::Nudge>,
    /// Whether a nudge's buttons are still waiting for an answer.
    #[cfg(windows)]
    awaiting_answer: Arc<AtomicBool>,
    #[cfg(not(target_os = "macos"))]
    on_pause: OnPause,
}

impl Notifier {
    pub fn new(files: Files, on_pause: OnPause) -> Self {
        #[cfg(target_os = "macos")]
        mac::start(on_pause);
        Self {
            files,
            #[cfg(all(unix, not(target_os = "macos")))]
            nudge: None,
            #[cfg(windows)]
            awaiting_answer: Arc::default(),
            #[cfg(not(target_os = "macos"))]
            on_pause,
        }
    }

    /// A short notice.
    #[cfg(not(target_os = "macos"))]
    pub fn info(&self, summary: &str, body: &str) {
        show(&base(&self.files, &self.files.icon, summary, body, 3000));
    }

    /// A short notice. Each replaces the last, as they'd be out of date.
    #[cfg(target_os = "macos")]
    pub fn info(&self, summary: &str, body: &str) {
        crate::mac_notify::post("info", summary, body, None);
    }

    #[cfg(target_os = "macos")]
    pub fn nag(&mut self, reason: &str) {
        crate::mac_notify::post(
            mac::NUDGE_ID,
            NUDGE,
            &nudge_body(reason),
            Some(mac::NUDGE_ID),
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    pub fn nag(&mut self, reason: &str) {
        let body = nudge_body(reason);
        if let Some(nudge) = self.nudge.as_mut().filter(|n| n.is_open()) {
            nudge.update(NUDGE, &escape(&body));
            return;
        }
        let mut notification = base(&self.files, &self.files.nudge_icon, NUDGE, &body, 15000);
        match with_buttons(&mut notification).show() {
            Ok(handle) => self.nudge = Some(xdg::Nudge::listen(handle, self.on_pause.clone())),
            Err(error) => tracing::warn!("notification failed: {error}"),
        }
    }

    /// A thread waits until the nudge is answered or dismissed, which can be long after its toast
    /// has gone, so only one waits at a time, and nudges meanwhile come without buttons.
    #[cfg(windows)]
    pub fn nag(&mut self, reason: &str) {
        let mut notification = base(
            &self.files,
            &self.files.nudge_icon,
            NUDGE,
            &nudge_body(reason),
            15000,
        );
        if self.awaiting_answer.swap(true, Ordering::AcqRel) {
            show(&notification);
            return;
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
        #[cfg(target_os = "macos")]
        crate::mac_notify::withdraw(mac::NUDGE_ID);
    }
}

#[cfg(not(target_os = "macos"))]
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
#[cfg(target_os = "macos")]
fn in_tray(_files: &Files, summary: &str, body: &str) {
    crate::mac_notify::post(mac::IN_TRAY_ID, summary, body, Some(mac::IN_TRAY_ID));
}

/// A notice that the app is in the tray, with a button to quit it instead.
#[cfg(not(target_os = "macos"))]
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

/// The nudge's and the tray notice's buttons on macOS, which are set up once, before any is posted.
#[cfg(target_os = "macos")]
mod mac {
    use std::sync::Arc;

    use super::{OnPause, PAUSE, QUIT};
    use crate::mac_notify::{self, Category};

    /// Both the nudge's id, so a new nudge replaces the last, and its kind, for its button.
    pub const NUDGE_ID: &str = "nudge";
    pub const IN_TRAY_ID: &str = "in-tray";

    pub fn start(on_pause: OnPause) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            mac_notify::start(&[
                Category {
                    id: NUDGE_ID,
                    buttons: &[(PAUSE, "Pause 30 min")],
                },
                Category {
                    id: IN_TRAY_ID,
                    buttons: &[(QUIT, "Quit")],
                },
            ]);
            mac_notify::on_action(PAUSE, on_pause);
            mac_notify::on_action(QUIT, Arc::new(crate::quit));
        });
    }
}

#[cfg(not(target_os = "macos"))]
fn show(notification: &Notification) {
    send(notification);
}

#[cfg(not(target_os = "macos"))]
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
#[cfg(not(target_os = "macos"))]
fn escape(text: &str) -> String {
    if cfg!(all(unix, not(target_os = "macos"))) {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    } else {
        text.to_string()
    }
}

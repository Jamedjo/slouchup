//! Fitting into the Windows shell: toasts that say they're from slouchup, and a tray icon that
//! suits the taskbar, which Windows themes apart from apps.

use std::path::Path;
use std::sync::OnceLock;

use windows_registry::CURRENT_USER;

use crate::config::APP_NAME;

/// Who toasts come from. Windows names and badges a toast by its sender's id, which an
/// unpackaged app registers for the current user.
const TOAST_SENDER: &str = "dev.weareframes.slouchup";

/// The toast sender, registered once with the app's name and icon, or `None` if that failed
/// and toasts should come from Windows' default sender instead.
pub fn toast_sender(icon: &Path) -> Option<&'static str> {
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    let registered = *REGISTERED.get_or_init(|| match register_toast_sender(icon) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!("couldn't register as a toast sender: {error}");
            false
        }
    });
    registered.then_some(TOAST_SENDER)
}

fn register_toast_sender(icon: &Path) -> windows_registry::Result<()> {
    let key = CURRENT_USER.create(toast_sender_key())?;
    key.set_string("DisplayName", APP_NAME)?;
    key.set_string("IconUri", icon.to_string_lossy())
}

/// For uninstalling, so the toast sender isn't left behind.
pub fn unregister_toast_sender() {
    let _ = CURRENT_USER.remove_tree(toast_sender_key());
}

fn toast_sender_key() -> String {
    format!(r"Software\Classes\AppUserModelId\{TOAST_SENDER}")
}

/// Whether the taskbar is light. Windows before 10 has no such setting, and a dark taskbar.
pub fn taskbar_is_light() -> bool {
    CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("SystemUsesLightTheme"))
        .is_ok_and(|light| light != 0)
}

/// Whether SlouchUp's toasts show, or `None` if Windows won't say, as before it's a toast sender.
pub fn toast_access() -> Option<crate::notification_access::Access> {
    use windows::UI::Notifications::{NotificationSetting, ToastNotificationManager};
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(
        &windows::core::HSTRING::from(TOAST_SENDER),
    )
    .ok()?;
    use crate::notification_access::Access;
    Some(match notifier.Setting().ok()? {
        NotificationSetting::Enabled if presenting() => Access::Presenting,
        NotificationSetting::Enabled => Access::On { fleeting: false },
        NotificationSetting::DisabledForUser => Access::OffForAll,
        NotificationSetting::DisabledByGroupPolicy => Access::Policy,
        _ => Access::Off,
    })
}

/// Whether Windows is keeping quiet for a presentation or a full-screen app.
fn presenting() -> bool {
    use windows::Win32::UI::Shell::{
        QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
        SHQueryUserNotificationState,
    };
    unsafe { SHQueryUserNotificationState() }.is_ok_and(|state| {
        [
            QUNS_BUSY,
            QUNS_PRESENTATION_MODE,
            QUNS_RUNNING_D3D_FULL_SCREEN,
        ]
        .contains(&state)
    })
}

/// Open a page of Settings, by its `ms-settings:` address.
pub fn open_settings(page: &str) {
    if let Err(error) = std::process::Command::new("explorer").arg(page).spawn() {
        tracing::warn!("couldn't open {page}: {error}");
    }
}

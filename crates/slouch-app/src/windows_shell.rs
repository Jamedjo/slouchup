//! Fitting into the Windows shell: a tray icon that suits the taskbar, which Windows themes apart
//! from apps.

use windows_registry::CURRENT_USER;

/// Whether the taskbar is light. Windows before 10 has no such setting, and a dark taskbar.
pub fn taskbar_is_light() -> bool {
    CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("SystemUsesLightTheme"))
        .is_ok_and(|light| light != 0)
}

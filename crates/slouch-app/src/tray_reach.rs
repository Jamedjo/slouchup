//! Whether the tray icon can be seen. Windows tucks new icons behind the ^ by the clock, a full menu
//! bar hides them on macOS, and some Linux desktops have no tray at all. While it's hidden, the
//! window keeps a place on the taskbar or in the Dock, so SlouchUp is never lost.

use dioxus::prelude::*;
use tray_icon::TrayIcon;

/// A button that sets it right: its label, and what it does.
pub type Fix = (&'static str, fn());

/// Where the tray icon is.
#[allow(dead_code)] // Each platform finds only some of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Visible,
    /// Behind the ^ on the Windows taskbar.
    Overflow,
    /// Pushed out of a full macOS menu bar, as by the camera notch.
    MenuBarFull,
    /// The desktop has no tray to show it in.
    NoTray,
}

impl Place {
    pub fn hidden(self) -> bool {
        self != Place::Visible
    }

    /// What to say about it, a button to set it right, and what the button says.
    pub fn advice(self) -> Option<(&'static str, Option<Fix>)> {
        match self {
            Place::Visible => None,
            Place::Overflow => Some((
                "SlouchUp's tucked behind the ^ by the clock. Keep it on the taskbar so you can see how you're sitting.",
                Some(("Keep it on the taskbar", keep_on_taskbar)),
            )),
            Place::MenuBarFull => Some((
                "Your menu bar's full, so SlouchUp's icon is hidden. Until there's room, it'll stay in your Dock.",
                None,
            )),
            // GNOME has no tray until an extension adds one; other desktops add one to the panel.
            Place::NoTray if on_gnome() => Some((
                "GNOME has no tray, so SlouchUp stays in its window. Add the AppIndicator extension to get the up mark in your top bar.",
                Some(("How to add it", how_to_add_a_tray)),
            )),
            Place::NoTray if has_task_list() => Some((
                "Your panel has no tray, so SlouchUp stays in its window. Add a tray, or status notifier, to your panel to get the up mark there.",
                None,
            )),
            Place::NoTray => Some((
                "There's no tray here, so SlouchUp stays in its window. Close it and SlouchUp keeps running; start SlouchUp again to bring it back.",
                None,
            )),
        }
    }
}

/// Where `icon` is, or `None` if that can't be told yet.
pub fn place(icon: &TrayIcon) -> Option<Place> {
    #[cfg(windows)]
    return windows::place(icon);
    #[cfg(target_os = "macos")]
    return macos::place(icon);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = icon;
        linux::place()
    }
}

/// The window's strip under its header while the tray icon can't be seen, with the fix where
/// there is one, until you say OK.
#[component]
pub fn TrayStrip(place: Option<Place>) -> Element {
    let mut dismissed = use_signal(|| None::<Place>);
    let Some((advice, fix)) = place.and_then(Place::advice) else {
        return rsx! {};
    };
    if dismissed() == place {
        return rsx! {};
    }
    rsx! {
        div { class: "tray-strip", role: "status",
            p { "{advice}" }
            div { class: "buttons",
                if let Some((label, fix)) = fix {
                    button { class: "button primary", onclick: move |_| fix(), "{label}" }
                }
                button { class: "button", onclick: move |_| dismissed.set(place), if fix.is_some() { "Not now" } else { "OK" } }
            }
        }
    }
}

fn keep_on_taskbar() {
    #[cfg(windows)]
    windows::keep_on_taskbar();
}

/// Whether a minimised window has somewhere to show: a taskbar, task list or Dock. Tiling window
/// managers such as sway and i3 usually have none, so there a minimised window would be lost.
pub fn has_task_list() -> bool {
    #[cfg(target_os = "linux")]
    {
        use tray_popover_tao::tray_popover::linux::Host;
        static HAS: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
            !matches!(
                tray_popover_tao::detect_host(),
                Host::Swaybar | Host::Waybar | Host::Snixembed | Host::Unknown
            )
        });
        *HAS
    }
    #[cfg(not(target_os = "linux"))]
    true
}

/// Whether this is a GNOME session, which needs an extension for a tray.
fn on_gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| {
        desktop
            .split(':')
            .any(|name| name.eq_ignore_ascii_case("gnome"))
    })
}

fn how_to_add_a_tray() {
    let page = "https://extensions.gnome.org/extension/615/appindicator-support/";
    if let Err(error) = std::process::Command::new("xdg-open").arg(page).spawn() {
        tracing::warn!("couldn't open {page}: {error}");
    }
}

#[cfg(windows)]
mod windows {
    use tray_icon::TrayIcon;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowRect};
    use windows::core::w;
    use windows_registry::CURRENT_USER;

    use super::Place;

    /// On the taskbar, the icon sits within the taskbar's own window; behind the ^ it doesn't, or
    /// has no place at all while the overflow is closed.
    pub fn place(icon: &TrayIcon) -> Option<Place> {
        let taskbar = taskbar_rect()?;
        let Some(rect) = icon.rect() else {
            return Some(Place::Overflow);
        };
        let (x, y) = (
            rect.position.x + rect.size.width as f64 / 2.0,
            rect.position.y + rect.size.height as f64 / 2.0,
        );
        let inside = (taskbar.left as f64..taskbar.right as f64).contains(&x)
            && (taskbar.top as f64..taskbar.bottom as f64).contains(&y);
        Some(if inside {
            Place::Visible
        } else {
            Place::Overflow
        })
    }

    fn taskbar_rect() -> Option<RECT> {
        let taskbar = unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.ok()?;
        let mut rect = RECT::default();
        unsafe { GetWindowRect(taskbar, &mut rect) }.ok()?;
        Some(rect)
    }

    /// Windows 11 keeps the icon on the taskbar by its entry in the taskbar's list of tray icons;
    /// Windows 10 only lets you choose in Settings.
    pub fn keep_on_taskbar() {
        let promoted = std::env::current_exe()
            .ok()
            .is_some_and(|exe| promote(&exe.to_string_lossy()).unwrap_or(false));
        if !promoted
            && let Err(error) = std::process::Command::new("explorer")
                .arg("ms-settings:taskbar")
                .spawn()
        {
            tracing::warn!("couldn't open the taskbar settings: {error}");
        }
    }

    /// Mark `exe`'s tray icon as shown on the taskbar, and say whether the taskbar listed it.
    fn promote(exe: &str) -> windows_registry::Result<bool> {
        let icons = CURRENT_USER.open(r"Control Panel\NotifyIconSettings")?;
        let mut found = false;
        for name in icons.keys()? {
            let icon = CURRENT_USER
                .options()
                .read()
                .write()
                .open(format!(r"Control Panel\NotifyIconSettings\{name}"))?;
            if icon
                .get_string("ExecutablePath")
                .is_ok_and(|path| path.eq_ignore_ascii_case(exe))
            {
                icon.set_u32("IsPromoted", 1)?;
                found = true;
            }
        }
        Ok(found)
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use dioxus::desktop::window;
    use tray_icon::TrayIcon;

    use super::Place;

    /// macOS moves status items it has no room for off the screen, or gives them no place.
    pub fn place(icon: &TrayIcon) -> Option<Place> {
        let Some(rect) = icon.rect() else {
            return Some(Place::MenuBarFull);
        };
        let main = window();
        let screen = main.primary_monitor().or_else(|| main.current_monitor())?;
        let (left, width) = (screen.position().x as f64, screen.size().width as f64);
        let on_screen =
            rect.position.x >= left && rect.position.x + rect.size.width as f64 <= left + width;
        Some(if on_screen {
            Place::Visible
        } else {
            Place::MenuBarFull
        })
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod linux {
    use gio::prelude::*;

    use super::Place;

    /// Tray icons are shown by a StatusNotifier host, which a desktop without a tray hasn't got.
    pub fn place() -> Option<Place> {
        let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
        let registered = bus
            .call_sync(
                Some("org.kde.StatusNotifierWatcher"),
                "/StatusNotifierWatcher",
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(
                    &(
                        "org.kde.StatusNotifierWatcher",
                        "IsStatusNotifierHostRegistered",
                    )
                        .to_variant(),
                ),
                None,
                gio::DBusCallFlags::NONE,
                1000,
                gio::Cancellable::NONE,
            )
            .ok()
            .and_then(|reply| reply.child_value(0).as_variant()?.get::<bool>());
        Some(if registered == Some(true) {
            Place::Visible
        } else {
            Place::NoTray
        })
    }
}

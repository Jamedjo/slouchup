//! Linux, through GTK. On X11 tao moves the window and GDK knows the work area. On Wayland a
//! client can't place its own window, so the popover is a layer-shell surface where it can be.

#[path = "layer.rs"]
mod layer;

use gtk::prelude::*;
use tao::monitor::MonitorHandle;
use tao::platform::unix::{WindowBuilderExtUnix, WindowExtUnix};
use tao::window::{Window, WindowBuilder};
use tray_popover::linux::{Host, parse_plasma_version};
use tray_popover::{Monitor, Placement, Rect};

pub fn builder(builder: WindowBuilder) -> WindowBuilder {
    builder.with_skip_taskbar(true)
}

pub fn prepare(window: &Window) {
    let gtk = window.gtk_window();
    gtk.set_skip_pager_hint(true);
    gtk.set_keep_above(true);
}

/// Make the window a wlr-layer-shell surface, where Wayland and libgtk-layer-shell allow, and
/// say whether it is one.
pub fn make_layer(window: &Window) -> bool {
    layer::prepare(window.gtk_window())
}

pub fn place_layer(window: &Window, placement: Placement, monitor: Monitor) {
    layer::place(window.gtk_window(), placement, monitor);
}

pub fn show(window: &Window) {
    window.set_visible(true);
    window.set_focus();
}

/// GDK's work area for the monitor, which on X11 is the desktop's `_NET_WORKAREA`.
pub fn work_area(_handle: &MonitorHandle, bounds: Rect) -> Option<Rect> {
    let display = gtk::gdk::Display::default()?;
    (0..display.n_monitors())
        .filter_map(|index| display.monitor(index))
        .find_map(|monitor| {
            let scale = monitor.scale_factor();
            let physical = |r: gtk::gdk::Rectangle| {
                Rect::new(
                    r.x() * scale,
                    r.y() * scale,
                    r.width() * scale,
                    r.height() * scale,
                )
            };
            (physical(monitor.geometry()) == bounds).then(|| physical(monitor.workarea()))
        })
}

/// The panel showing the tray icon, from the session's environment, asking Plasma 5 for its
/// minor version, since 5.26 and older give clicks in logical pixels.
pub fn detect_host() -> Host {
    let var = |name: &str| std::env::var(name).ok();
    let plasma = (var("KDE_SESSION_VERSION").as_deref() == Some("5"))
        .then(|| {
            std::process::Command::new("plasmashell")
                .arg("--version")
                .output()
        })
        .and_then(Result::ok)
        .and_then(|output| parse_plasma_version(&String::from_utf8_lossy(&output.stdout)));
    Host::detect(var, plasma)
}

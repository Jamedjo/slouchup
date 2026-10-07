//! Wayland: the popover as a wlr-layer-shell surface, anchored to the panel's edge under the tray
//! icon, since a Wayland client can't place an ordinary window.
//!
//! libgtk-layer-shell is loaded when it's first wanted rather than linked, so the app still starts
//! without it. Without it, or on a compositor without layer-shell such as GNOME's, the popover is
//! an ordinary window, which the compositor centres.

use std::ffi::{c_int, c_uint};
use std::sync::OnceLock;

use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
use tray_popover::{Edge, GAP, Monitor, Placement};

type Window = *mut gtk::ffi::GtkWindow;

const EDGE_LEFT: c_int = 0;
const EDGE_RIGHT: c_int = 1;
const EDGE_TOP: c_int = 2;
const EDGE_BOTTOM: c_int = 3;
const LAYER_OVERLAY: c_int = 3;
const KEYBOARD_ON_DEMAND: c_int = 2;

struct Shell {
    _library: libloading::Library,
    is_supported: unsafe extern "C" fn() -> gtk::glib::ffi::gboolean,
    init_for_window: unsafe extern "C" fn(Window),
    set_layer: unsafe extern "C" fn(Window, c_int),
    set_anchor: unsafe extern "C" fn(Window, c_int, gtk::glib::ffi::gboolean),
    set_margin: unsafe extern "C" fn(Window, c_int, c_int),
    set_monitor: unsafe extern "C" fn(Window, *mut gtk::gdk::ffi::GdkMonitor),
    set_keyboard_mode: Option<unsafe extern "C" fn(Window, c_int)>,
    set_keyboard_interactivity: unsafe extern "C" fn(Window, gtk::glib::ffi::gboolean),
    minor_version: unsafe extern "C" fn() -> c_uint,
}

impl Shell {
    fn load() -> Option<Self> {
        // SAFETY: loading a system library, and reading functions with the signatures its
        // header gives them.
        unsafe {
            let library = libloading::Library::new("libgtk-layer-shell.so.0").ok()?;
            macro_rules! get {
                ($name:literal) => {
                    *library.get(concat!($name, "\0").as_bytes()).ok()?
                };
            }
            Some(Self {
                is_supported: get!("gtk_layer_is_supported"),
                init_for_window: get!("gtk_layer_init_for_window"),
                set_layer: get!("gtk_layer_set_layer"),
                set_anchor: get!("gtk_layer_set_anchor"),
                set_margin: get!("gtk_layer_set_margin"),
                set_monitor: get!("gtk_layer_set_monitor"),
                set_keyboard_mode: library
                    .get(b"gtk_layer_set_keyboard_mode\0")
                    .ok()
                    .map(|f| *f),
                set_keyboard_interactivity: get!("gtk_layer_set_keyboard_interactivity"),
                minor_version: get!("gtk_layer_get_minor_version"),
                _library: library,
            })
        }
    }
}

/// libgtk-layer-shell, when it's installed and the compositor has layer-shell.
fn shell() -> Option<&'static Shell> {
    static SHELL: OnceLock<Option<Shell>> = OnceLock::new();
    SHELL
        .get_or_init(|| {
            let shell = Shell::load()?;
            // SAFETY: a query of the display GTK has already opened.
            (unsafe { (shell.is_supported)() } != 0).then_some(shell)
        })
        .as_ref()
}

fn raw(window: &gtk::ApplicationWindow) -> Window {
    let window: &gtk::Window = window.upcast_ref();
    window.to_glib_none().0
}

/// Make `window` a layer-shell surface above everything else, taking the keyboard when it's
/// clicked, and say whether it could be. It has to happen before the window is first shown.
pub fn prepare(window: &gtk::ApplicationWindow) -> bool {
    let Some(shell) = shell() else {
        return false;
    };
    if window.is_realized() {
        window.unrealize();
    }
    let window = raw(window);
    // SAFETY: the window is GTK's, alive for this call, and not yet mapped.
    unsafe {
        (shell.init_for_window)(window);
        (shell.set_layer)(window, LAYER_OVERLAY);
        // On-demand keyboard focus came in 0.6; before that, a layer surface takes the keyboard
        // or never does.
        match shell.set_keyboard_mode {
            Some(set) if (shell.minor_version)() >= 6 => set(window, KEYBOARD_ON_DEMAND),
            _ => (shell.set_keyboard_interactivity)(window, 1),
        }
    }
    true
}

/// Anchor `window` to `placement`'s panel edge on `monitor`, `GAP` from the panel and level
/// with the icon along it. The compositor keeps it clear of the panel itself.
pub fn place(window: &gtk::ApplicationWindow, placement: Placement, monitor: Monitor) {
    let Some(shell) = shell() else {
        return;
    };
    let gdk_monitor = gtk_monitor(&monitor);
    let logical = |physical: i32| (physical as f64 / monitor.scale).round() as c_int;
    let along_x = logical(placement.position.x - monitor.bounds.x);
    let along_y = logical(placement.position.y - monitor.bounds.y);
    let gap = GAP as c_int;
    let (anchors, margins) = match placement.edge {
        Edge::Top => (
            [EDGE_TOP, EDGE_LEFT],
            [(EDGE_TOP, gap), (EDGE_LEFT, along_x)],
        ),
        Edge::Bottom => (
            [EDGE_BOTTOM, EDGE_LEFT],
            [(EDGE_BOTTOM, gap), (EDGE_LEFT, along_x)],
        ),
        Edge::Left => (
            [EDGE_LEFT, EDGE_TOP],
            [(EDGE_LEFT, gap), (EDGE_TOP, along_y)],
        ),
        Edge::Right => (
            [EDGE_RIGHT, EDGE_TOP],
            [(EDGE_RIGHT, gap), (EDGE_TOP, along_y)],
        ),
    };
    let window = raw(window);
    // SAFETY: the window is GTK's and a layer surface, and the monitor, if any, is GDK's.
    unsafe {
        if let Some(gdk_monitor) = &gdk_monitor {
            (shell.set_monitor)(window, gdk_monitor.to_glib_none().0);
        }
        for edge in [EDGE_LEFT, EDGE_RIGHT, EDGE_TOP, EDGE_BOTTOM] {
            (shell.set_anchor)(window, edge, anchors.contains(&edge).into());
            (shell.set_margin)(window, edge, 0);
        }
        for (edge, margin) in margins {
            (shell.set_margin)(window, edge, margin.max(0));
        }
    }
}

/// GDK's monitor with `monitor`'s bounds.
fn gtk_monitor(monitor: &Monitor) -> Option<gtk::gdk::Monitor> {
    let display = gtk::gdk::Display::default()?;
    (0..display.n_monitors())
        .filter_map(|index| display.monitor(index))
        .find(|candidate| {
            let scale = candidate.scale_factor();
            let g = candidate.geometry();
            (g.x() * scale, g.y() * scale) == (monitor.bounds.x, monitor.bounds.y)
        })
}

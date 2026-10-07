//! A tray popover in a [tao] window.
//!
//! Build the window hidden, with [`window_builder`], when the app starts, so opening it later is
//! only a move and a show. Wrap it in a [`TaoSurface`], which turns it into a popover for the
//! platform, and drive it with a [`TaoPopover`]: pass it the tray icon's clicks, and every tao
//! event through [`handle_event`], which hides it when it loses the focus or Esc is pressed.
//!
//! Platform behaviour:
//! - **macOS**: the window becomes a non-activating `NSPanel` at the status bar's level, on every
//!   Space and over full-screen apps, so opening it doesn't switch Spaces or activate the app.
//! - **Windows**: rounded corners on Windows 11, and no taskbar button.
//! - **Linux**: moved with `set_outer_position` on X11. On Wayland, where a client can't place its
//!   own window, it's a wlr-layer-shell surface anchored to the panel's edge, when the compositor
//!   has layer-shell and libgtk-layer-shell is installed; otherwise the compositor centres it.
//!
//! The placement and toggling are in [`tray_popover`], with no windowing library, so another
//! adapter (winit, Blitz) can implement [`tray_popover::Surface`] alongside this one.

use std::sync::Arc;
use std::time::Instant;

use tao::dpi::PhysicalPosition;
use tao::event::{ElementState, Event, WindowEvent};
use tao::keyboard::Key;
use tao::monitor::MonitorHandle;
use tao::window::{Window, WindowBuilder, WindowId};
pub use tray_popover;
use tray_popover::{Monitor, Placement, Point, Popover, Rect, Size, Surface};

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
#[path = "other.rs"]
mod platform;

/// The panel holding the tray icon, which says what its clicks' positions are in.
#[cfg(target_os = "linux")]
pub use platform::detect_host;

pub type TaoPopover = Popover<TaoSurface>;

/// `builder` made into a popover's window: hidden, borderless, fixed in size, above other
/// windows and left out of the taskbar.
pub fn window_builder(builder: WindowBuilder) -> WindowBuilder {
    platform::builder(
        builder
            .with_visible(false)
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top(true),
    )
}

/// A tao window made into a popover.
pub struct TaoSurface {
    window: Arc<Window>,
    /// A wlr-layer-shell surface, anchored to the panel rather than placed by position.
    layered: bool,
}

impl TaoSurface {
    /// `window`, which should have come from [`window_builder`], made into a popover for the
    /// platform.
    pub fn new(window: Arc<Window>) -> Self {
        platform::prepare(&window);
        let layered = platform::make_layer(&window);
        Self { window, layered }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }
}

impl Surface for TaoSurface {
    fn monitors(&self) -> Vec<Monitor> {
        self.window.available_monitors().map(monitor).collect()
    }

    /// The outer size, or the inner size where that's bigger, as for a GTK window that has
    /// never been shown and so has no outer size yet.
    fn size(&self) -> Size {
        let (outer, inner) = (self.window.outer_size(), self.window.inner_size());
        Size::new(
            outer.width.max(inner.width) as i32,
            outer.height.max(inner.height) as i32,
        )
    }

    fn cursor(&self) -> Option<Point> {
        let at = self.window.cursor_position().ok()?;
        Some(Point::new(at.x.round() as i32, at.y.round() as i32))
    }

    fn show_placed(&mut self, placement: Placement) {
        if self.layered
            && let Some(monitor) = self.monitors().get(placement.monitor).copied()
        {
            platform::place_layer(&self.window, placement, monitor);
            platform::show(&self.window);
            return;
        }
        self.show_at(placement.position);
    }

    fn show_at(&mut self, position: Point) {
        let position = PhysicalPosition::new(position.x, position.y);
        self.window.set_outer_position(position);
        platform::show(&self.window);
        // Some window managers place a window as it's mapped, wherever it was asked to be.
        self.window.set_outer_position(position);
    }

    fn hide(&mut self) {
        self.window.set_visible(false);
    }

    fn focus(&mut self) {
        platform::show(&self.window);
    }
}

fn monitor(handle: MonitorHandle) -> Monitor {
    let at = handle.position();
    let size = handle.size();
    let bounds = Rect::new(at.x, at.y, size.width as i32, size.height as i32);
    Monitor {
        bounds,
        work_area: platform::work_area(&handle, bounds).unwrap_or(bounds),
        scale: handle.scale_factor(),
    }
}

/// What a window event means for a popover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopoverEvent {
    Focused(bool),
    Escape,
}

impl PopoverEvent {
    /// What `event` means for the popover in `window`, if anything.
    pub fn of<T>(window: WindowId, event: &Event<'_, T>) -> Option<Self> {
        let Event::WindowEvent {
            window_id, event, ..
        } = event
        else {
            return None;
        };
        if *window_id != window {
            return None;
        }
        match event {
            WindowEvent::Focused(focused) => Some(PopoverEvent::Focused(*focused)),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && event.logical_key == Key::Escape =>
            {
                Some(PopoverEvent::Escape)
            }
            _ => None,
        }
    }

    /// Hide `popover` if this is it losing the focus, or Esc.
    pub fn apply(self, popover: &mut TaoPopover) {
        match self {
            PopoverEvent::Focused(focused) => popover.focus(focused, Instant::now()),
            PopoverEvent::Escape => popover.hide(),
        }
    }
}

/// Hide the popover when its window loses the focus or gets Esc. Pass it every tao event.
///
/// tao sees Esc even when a webview in the window has handled it, say to close a menu. A page
/// that does should pass on only [`PopoverEvent::Focused`] and hide the popover on Esc itself.
/// Where showing or hiding the window can send events straight back, as on Windows, use
/// [`PopoverEvent`] to hold them until the popover is free.
pub fn handle_event<T>(popover: &mut TaoPopover, event: &Event<'_, T>) {
    if let Some(event) = PopoverEvent::of(popover.surface().window.id(), event) {
        event.apply(popover);
    }
}

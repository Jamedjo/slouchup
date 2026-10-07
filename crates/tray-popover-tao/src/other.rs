use tao::monitor::MonitorHandle;
use tao::window::{Window, WindowBuilder};
use tray_popover::Rect;

pub fn builder(builder: WindowBuilder) -> WindowBuilder {
    builder
}

pub fn prepare(_window: &Window) {}

pub fn show(window: &Window) {
    window.set_visible(true);
    window.set_focus();
}

pub fn work_area(_monitor: &MonitorHandle, _bounds: Rect) -> Option<Rect> {
    None
}

/// Only Wayland places a popover by anchoring it to the panel.
pub fn make_layer(_window: &Window) -> bool {
    false
}

pub fn place_layer(
    _window: &Window,
    _placement: tray_popover::Placement,
    _monitor: tray_popover::Monitor,
) {
}

use tao::monitor::MonitorHandle;
use tao::platform::windows::{MonitorHandleExtWindows, WindowBuilderExtWindows, WindowExtWindows};
use tao::window::{Window, WindowBuilder};
use tray_popover::Rect;
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO};

/// Where the window starts: off every screen, so a toolkit that shows it before the popover
/// opens, as dioxus-desktop does on this platform, shows nothing.
/// [`tray_popover::Popover::settle`] then hides it.
const OFF_SCREEN: tao::dpi::PhysicalPosition<i32> = tao::dpi::PhysicalPosition::new(-32000, -32000);

pub fn builder(builder: WindowBuilder) -> WindowBuilder {
    builder.with_skip_taskbar(true).with_position(OFF_SCREEN)
}

/// Rounded corners, as Windows 11 gives its own flyouts. Earlier versions refuse the attribute
/// and keep square ones.
pub fn prepare(window: &Window) {
    let preference = DWMWCP_ROUND;
    // SAFETY: the window handle is tao's live window, and the attribute is a 4-byte enum.
    unsafe {
        DwmSetWindowAttribute(
            window.hwnd() as _,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&preference as *const i32).cast(),
            size_of::<i32>() as u32,
        );
    }
}

pub fn show(window: &Window) {
    window.set_visible(true);
    window.set_focus();
}

/// The monitor less the taskbar and anything else docked to its edges.
pub fn work_area(handle: &MonitorHandle, _bounds: Rect) -> Option<Rect> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: the monitor handle comes from tao, and `info` is sized as the call expects.
    let found = unsafe { GetMonitorInfoW(handle.hmonitor() as HMONITOR, &mut info) };
    if found == 0 {
        return None;
    }
    let work = info.rcWork;
    Some(Rect::new(
        work.left,
        work.top,
        work.right - work.left,
        work.bottom - work.top,
    ))
}

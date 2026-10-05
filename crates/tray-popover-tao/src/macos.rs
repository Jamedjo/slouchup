//! macOS: the window becomes a non-activating `NSPanel`, so it opens over full-screen apps and on
//! every Space without activating the app, which would switch to the Space of its other windows.
//!
//! tao has no panels (tao#414), so the window's class is changed in place, as tauri-nspanel does.
//! `NSPanel` adds no instance variables to `NSWindow`, so the layout still fits. Two things are
//! lost with tao's class and put back by the panel's:
//! - tao's `canBecomeKeyWindow`, without which a borderless panel never takes the keyboard.
//! - Key-value observing. WebKit observes the window, so by the time the webview is in it, its
//!   class is KVO's hidden `NSKVONotifying_TaoWindow`. Swapping that out makes KVO throw when an
//!   observer is removed, which aborts a Rust program. The panel adds and removes observers with
//!   the window's earlier class put back for the call, so KVO always finds the observers it has.

use std::ffi::{CStr, c_void};
use std::sync::{Mutex, OnceLock};

use objc2::ffi::{object_getClass, object_setClass};
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{ClassType, msg_send, sel};
use objc2_app_kit::{
    NSPanel, NSScreen, NSStatusWindowLevel, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::NSString;
use tao::monitor::MonitorHandle;
use tao::platform::macos::{MonitorHandleExtMacOS, WindowExtMacOS};
use tao::window::{Window, WindowBuilder};
use tray_popover::Rect;

const PANEL: &CStr = c"TrayPopoverPanel";
const TAO_WINDOW: &CStr = c"TaoWindow";
const KVO_PREFIX: &[u8] = b"NSKVONotifying_";

/// Each panel's class before it became one, by the window's address.
static EARLIER: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

/// Where the window starts: off every screen, so a toolkit that shows it before the popover
/// opens, as dioxus-desktop does on this platform, shows nothing.
/// [`tray_popover::Popover::settle`] then hides it.
const OFF_SCREEN: tao::dpi::PhysicalPosition<i32> = tao::dpi::PhysicalPosition::new(-32000, -32000);

pub fn builder(builder: WindowBuilder) -> WindowBuilder {
    builder.with_position(OFF_SCREEN)
}

pub fn prepare(window: &Window) {
    let ns_window = window.ns_window().cast::<AnyObject>();
    // SAFETY: tao's window is a live NSWindow, and this runs on the main thread, as tao does.
    let class = unsafe { &*object_getClass(ns_window) };
    if !is_tao_window(class) {
        return;
    }
    EARLIER
        .lock()
        .unwrap()
        .push((ns_window as usize, class as *const AnyClass as usize));
    // SAFETY: the panel class adds methods only, to NSPanel, which adds no instance variables
    // to NSWindow; tao's class adds one, which nothing reads once these methods replace tao's.
    unsafe { object_setClass(ns_window, panel_class()) };
    // SAFETY: its class is now an NSPanel subclass.
    let panel = unsafe { &*ns_window.cast::<NSPanel>() };
    panel.setStyleMask(panel.styleMask() | NSWindowStyleMask::NonactivatingPanel);
    panel.setLevel(NSStatusWindowLevel);
    panel.setCollectionBehavior(
        panel.collectionBehavior()
            | NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel.setHidesOnDeactivate(false);
    panel.setBecomesKeyOnlyIfNeeded(false);
}

/// tao's window class, or KVO's subclass of it.
fn is_tao_window(class: &AnyClass) -> bool {
    class.name() == TAO_WINDOW
        || (class.name().to_bytes().starts_with(KVO_PREFIX)
            && class
                .superclass()
                .is_some_and(|parent| parent.name() == TAO_WINDOW))
}

/// Showing makes it key and brings it to the front. Focusing it as tao does would activate the
/// app, and that switches Spaces.
pub fn show(window: &Window) {
    window.set_visible(true);
}

/// The screen less the menu bar and the Dock. AppKit measures from the bottom left in points,
/// so the insets are taken from it and applied to tao's bounds.
pub fn work_area(handle: &MonitorHandle, bounds: Rect) -> Option<Rect> {
    let screen = handle.ns_screen()?.cast::<NSScreen>();
    // SAFETY: tao looked up a live NSScreen, on the main thread.
    let screen = unsafe { &*screen };
    let (frame, visible) = (screen.frame(), screen.visibleFrame());
    let scale = handle.scale_factor();
    let px = |points: f64| (points * scale).round() as i32;
    let left = px(visible.origin.x - frame.origin.x);
    let bottom = px(visible.origin.y - frame.origin.y);
    let right = px((frame.origin.x + frame.size.width) - (visible.origin.x + visible.size.width));
    let top = px((frame.origin.y + frame.size.height) - (visible.origin.y + visible.size.height));
    Some(Rect::new(
        bounds.x + left,
        bounds.y + top,
        bounds.width - left - right,
        bounds.height - top - bottom,
    ))
}

fn panel_class() -> &'static AnyClass {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        if let Some(existing) = AnyClass::get(PANEL) {
            return existing;
        }
        let mut builder =
            ClassBuilder::new(PANEL, NSPanel::class()).expect("panel class not yet defined");
        // SAFETY: each function's types match the selector's, as NSWindow and NSObject declare
        // them.
        unsafe {
            builder.add_method(
                sel!(canBecomeKeyWindow),
                yes as extern "C-unwind" fn(_, _) -> _,
            );
            builder.add_method(
                sel!(canBecomeMainWindow),
                no as extern "C-unwind" fn(_, _) -> _,
            );
            builder.add_method(
                sel!(addObserver:forKeyPath:options:context:),
                add_observer as extern "C-unwind" fn(_, _, _, _, _, _),
            );
            builder.add_method(
                sel!(removeObserver:forKeyPath:),
                remove_observer as extern "C-unwind" fn(_, _, _, _),
            );
            builder.add_method(
                sel!(removeObserver:forKeyPath:context:),
                remove_observer_with_context as extern "C-unwind" fn(_, _, _, _, _),
            );
        }
        builder.register()
    })
}

extern "C-unwind" fn yes(_this: &AnyObject, _sel: Sel) -> Bool {
    Bool::YES
}

extern "C-unwind" fn no(_this: &AnyObject, _sel: Sel) -> Bool {
    Bool::NO
}

extern "C-unwind" fn add_observer(
    this: &AnyObject,
    _sel: Sel,
    observer: &AnyObject,
    key_path: &NSString,
    options: usize,
    context: *mut c_void,
) {
    with_earlier_class(this, || unsafe {
        let _: () = msg_send![
            super(this, NSPanel::class()),
            addObserver: observer,
            forKeyPath: key_path,
            options: options,
            context: context
        ];
    });
}

extern "C-unwind" fn remove_observer(
    this: &AnyObject,
    _sel: Sel,
    observer: &AnyObject,
    key_path: &NSString,
) {
    with_earlier_class(this, || unsafe {
        let _: () = msg_send![
            super(this, NSPanel::class()),
            removeObserver: observer,
            forKeyPath: key_path
        ];
    });
}

extern "C-unwind" fn remove_observer_with_context(
    this: &AnyObject,
    _sel: Sel,
    observer: &AnyObject,
    key_path: &NSString,
    context: *mut c_void,
) {
    with_earlier_class(this, || unsafe {
        let _: () = msg_send![
            super(this, NSPanel::class()),
            removeObserver: observer,
            forKeyPath: key_path,
            context: context
        ];
    });
}

/// Run `call` with the window's class as it was before it became a panel, so KVO adds and
/// removes observers under the class it built for them. KVO may change the class during the
/// call, as it does for the first observer, so the class it leaves is kept for next time.
fn with_earlier_class(this: &AnyObject, call: impl FnOnce()) {
    let window = this as *const AnyObject as *mut AnyObject;
    let class = EARLIER
        .lock()
        .unwrap()
        .iter()
        .find(|(at, _)| *at == window as usize)
        .map(|(_, class)| *class);
    let Some(class) = class else {
        call();
        return;
    };
    // SAFETY: both classes are NSWindow subclasses whose layout fits the window. While the
    // earlier class is in place, these messages go to NSObject's methods rather than back here.
    let left = unsafe {
        object_setClass(window, class as *const AnyClass);
        call();
        let left = object_getClass(window) as usize;
        object_setClass(window, panel_class());
        left
    };
    if let Some(entry) = EARLIER
        .lock()
        .unwrap()
        .iter_mut()
        .find(|(at, _)| *at == window as usize)
    {
        entry.1 = left;
    }
}

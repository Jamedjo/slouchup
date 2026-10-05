//! macOS: the menu bar icon named for VoiceOver, and opened by an accessibility press as a
//! click opens it.
//!
//! tray-icon reads clicks from a view it lays over the status item's button, so pressing the
//! button itself, as VoiceOver and other accessibility clients do, otherwise does nothing. The
//! button gets a target that sends the same click a mouse would. A mouse click still lands on
//! tray-icon's view and never reaches the button, so it isn't counted twice.

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSScreen, NSStatusBarButton};
use objc2_foundation::NSString;
use tray_icon::dpi::{PhysicalPosition, PhysicalSize};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconEvent, TrayIconId};

use crate::config::{APP_ID, APP_NAME};

/// The icon's accessibility identifier: apart from the windows', which are titled SlouchUp.
const TRAY_ID: &str = "slouchup-tray";

type Clicks = Box<dyn Fn(TrayIconEvent) + Send + Sync>;

static SEND: OnceLock<Clicks> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SlouchUpTrayPress"]
    struct TrayPress;

    impl TrayPress {
        #[unsafe(method(press:))]
        fn press(&self, sender: Option<&AnyObject>) {
            let Some(button) = sender.and_then(|s| s.downcast_ref::<NSStatusBarButton>()) else {
                return;
            };
            if let (Some(send), Some(event)) = (SEND.get(), click_at(button)) {
                send(event);
            }
        }
    }
);

/// Name `icon`'s button for VoiceOver, and send an accessibility press on it to `send` as a
/// left click. Returns the button's target, which has to be kept for as long as the icon.
pub fn make_accessible(
    icon: &TrayIcon,
    send: impl Fn(TrayIconEvent) + Send + Sync + 'static,
) -> Option<Retained<NSObject>> {
    let mtm = MainThreadMarker::new()?;
    let button = icon.ns_status_item()?.button(mtm)?;
    let _ = SEND.set(Box::new(send));
    let target: Retained<TrayPress> = unsafe { msg_send![TrayPress::alloc(mtm), init] };
    // SAFETY: the target outlives the button, as the caller keeps it, and `press:` takes the
    // sender as an action method does.
    unsafe {
        let _: () = msg_send![&*button, setAccessibilityLabel: &*NSString::from_str(APP_NAME)];
        let _: () = msg_send![&*button, setAccessibilityIdentifier: &*NSString::from_str(TRAY_ID)];
        button.setTarget(Some(&target));
        button.setAction(Some(sel!(press:)));
    }
    Some(Retained::into_super(target))
}

/// A left click's release on `button`, placed as tray-icon places a mouse click: physical
/// pixels from the top left of the main screen.
fn click_at(button: &NSStatusBarButton) -> Option<TrayIconEvent> {
    let mtm = MainThreadMarker::from(button);
    let window = button.window()?;
    let frame = window.frame();
    let scale = window.backingScaleFactor();
    let main_height = NSScreen::screens(mtm).firstObject()?.frame().size.height;
    let top = main_height - (frame.origin.y + frame.size.height);
    let size = PhysicalSize::new(
        (frame.size.width * scale).round() as u32,
        (frame.size.height * scale).round() as u32,
    );
    let position = PhysicalPosition::new(frame.origin.x * scale, top * scale);
    Some(TrayIconEvent::Click {
        id: TrayIconId::new(APP_ID),
        position: PhysicalPosition::new(
            position.x + size.width as f64 / 2.0,
            position.y + size.height as f64 / 2.0,
        ),
        rect: tray_icon::Rect { size, position },
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
    })
}

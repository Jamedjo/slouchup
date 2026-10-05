//! Notifications on macOS, through the UserNotifications framework: posting them, and hearing
//! their buttons.
//!
//! Everything goes through UserNotifications, because once an app registers with it, macOS silently
//! drops the app's notifications sent the older way, as notify-rust sends them.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AllocAnyThread, define_class, msg_send};
use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSSet, NSString};
use objc2_user_notifications::{
    UNMutableNotificationContent, UNNotification, UNNotificationAction,
    UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

/// A kind of notification with buttons, each button an action id and its label.
pub struct Category {
    pub id: &'static str,
    pub buttons: &'static [(&'static str, &'static str)],
}

/// What a button press calls, by its action id.
type Handlers = Mutex<HashMap<String, Arc<dyn Fn() + Send + Sync>>>;

fn handlers() -> &'static Handlers {
    static HANDLERS: OnceLock<Handlers> = OnceLock::new();
    HANDLERS.get_or_init(Handlers::default)
}

/// The notification centre, or `None` outside an app bundle, where asking for it raises.
fn center() -> Option<Retained<UNUserNotificationCenter>> {
    NSBundle::mainBundle().bundleIdentifier()?;
    Some(UNUserNotificationCenter::currentNotificationCenter())
}

/// Set up the buttons for `categories`, and listen for presses. Call once, early on.
pub fn start(categories: &[Category]) {
    let Some(center) = center() else {
        tracing::warn!("no app bundle, so no notifications");
        return;
    };
    let categories: Vec<Retained<UNNotificationCategory>> = categories
        .iter()
        .map(|category| {
            let actions: Vec<Retained<UNNotificationAction>> = category
                .buttons
                .iter()
                .map(|(id, label)| {
                    UNNotificationAction::actionWithIdentifier_title_options(
                        &NSString::from_str(id),
                        &NSString::from_str(label),
                        UNNotificationActionOptions::empty(),
                    )
                })
                .collect();
            UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                &NSString::from_str(category.id),
                &NSArray::from_retained_slice(&actions),
                &NSArray::new(),
                UNNotificationCategoryOptions::empty(),
            )
        })
        .collect();
    center.setNotificationCategories(&NSSet::from_retained_slice(&categories));
    let delegate = Delegate::new();
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The centre only holds its delegate weakly, and this one is wanted for the app's lifetime.
    std::mem::forget(delegate);
}

/// Call `handler`, from another thread, when a button with `action` is pressed.
pub fn on_action(action: &str, handler: Arc<dyn Fn() + Send + Sync>) {
    handlers()
        .lock()
        .unwrap()
        .insert(action.to_string(), handler);
}

/// Post a notification, replacing any still showing with the same `id`.
pub fn post(id: &str, title: &str, body: &str, category: Option<&str>) {
    let Some(center) = center() else { return };
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    if let Some(category) = category {
        content.setCategoryIdentifier(&NSString::from_str(category));
    }
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(id),
        &content,
        None,
    );
    let handler = RcBlock::new(|error: *mut NSError| {
        if let Some(error) = unsafe { error.as_ref() } {
            tracing::warn!("notification failed: {}", error.localizedDescription());
        }
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&handler));
}

/// Take a notification off the screen and out of Notification Centre.
pub fn withdraw(id: &str) {
    if let Some(center) = center() {
        center.removeDeliveredNotificationsWithIdentifiers(&NSArray::from_retained_slice(&[
            NSString::from_str(id),
        ]));
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SlouchUpNotificationDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        // Without this, macOS keeps quiet while the app is frontmost, as it is with its window open.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion
                .call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            let action = response.actionIdentifier().to_string();
            let handler = handlers().lock().unwrap().get(&action).cloned();
            if let Some(handler) = handler {
                handler();
            }
            completion.call(());
        }
    }
);

impl Delegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

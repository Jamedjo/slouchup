//! The camera button on the picture's bottom strip, as video calls have it: it names the camera in use and
//! opens a list of the others. Choosing one switches the picture above, which shows which it is.

use std::rc::Rc;

use dioxus::prelude::*;

use crate::source::CameraInfo;

/// `cameras` to choose between, `active` the id of the one in use.
#[component]
pub fn CameraPicker(
    cameras: Vec<CameraInfo>,
    active: String,
    on_choose: EventHandler<String>,
) -> Element {
    let mut open = use_signal(|| false);
    let mut button = use_signal(|| None::<Rc<MountedData>>);
    let mut close = move || {
        open.set(false);
        if let Some(button) = button() {
            spawn(async move {
                let _ = button.set_focus(true).await;
            });
        }
    };
    let name = cameras
        .iter()
        .find(|c| c.id == active)
        .map_or("Camera", |c| c.name.as_str())
        .to_string();
    rsx! {
        div { class: "picker",
            button {
                class: "control camera-pill",
                title: "Choose a camera ({name})",
                "aria-haspopup": "listbox",
                "aria-expanded": "{open()}",
                onmounted: move |event| button.set(Some(event.data())),
                onclick: move |_| open.toggle(),
                span { class: "icon", dangerous_inner_html: crate::ui::CAMERA_ICON }
                span { class: "camera-name", "{name}" }
                span { class: "icon caret", dangerous_inner_html: CARET_UP }
            }
            if open() {
                div { class: "backdrop", onclick: move |_| close() }
                div {
                    class: "camera-menu",
                    role: "listbox",
                    "aria-label": "Camera",
                    onkeydown: move |event| match event.key() {
                        Key::Escape => {
                            event.prevent_default();
                            close();
                        }
                        key => {
                            if crate::ui::move_focus(".camera-option", &key) {
                                event.prevent_default();
                            }
                        }
                    },
                    for camera in cameras.iter().cloned() {
                        CameraOption {
                            key: "{camera.id}",
                            selected: camera.id == active,
                            camera,
                            on_choose: move |id: String| {
                                on_choose.call(id);
                                close();
                            },
                        }
                    }
                    p { class: "camera-hint", "Switching camera recalibrates, so sit up nicely for a moment." }
                    p { class: "camera-hint", "Everything runs on your computer. Video never leaves it." }
                }
            }
        }
    }
}

/// One camera in the list.
#[component]
fn CameraOption(camera: CameraInfo, selected: bool, on_choose: EventHandler<String>) -> Element {
    let id = camera.id.clone();
    rsx! {
        button {
            class: "camera-option",
            role: "option",
            "aria-selected": "{selected}",
            title: "{camera.id}",
            onmounted: move |event| async move {
                if selected {
                    let _ = event.data().set_focus(true).await;
                }
            },
            onclick: move |_| on_choose.call(id.clone()),
            span { class: "camera-option-name", "{camera.name}" }
            if selected {
                span { class: "icon tick", dangerous_inner_html: TICK }
            }
        }
    }
}

pub const CARET_UP: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round"><path d="m6 15 6-6 6 6"/></svg>"#;
const TICK: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>"#;

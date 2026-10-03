//! The settings window: which camera to use, the slouch limits, and how eagerly to nag.
//! Changes apply as they're made.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dioxus::prelude::*;
use posture::{Settings, Thresholds};

use crate::config::{self, Preferences};
use crate::engine::Command;
use crate::source;
use crate::ui::Bridge;

const STYLE: &str = include_str!("style.css");

/// What the settings window needs from the app, and a flag saying whether it's open.
#[derive(Clone)]
pub struct SettingsHandle {
    pub bridge: Bridge,
    pub open: Arc<AtomicBool>,
}

impl PartialEq for SettingsHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.open, &other.open)
    }
}

#[component]
pub fn SettingsPage(handle: SettingsHandle, initial: Settings) -> Element {
    let open = handle.open.clone();
    use_drop(move || open.store(false, Ordering::Relaxed));
    let cameras = use_hook(source::list_cameras);
    let persist = handle.bridge.persist;
    let mut preferences = use_signal(|| Preferences {
        camera: if persist {
            config::load_preferences().camera
        } else {
            None
        },
        grace: initial.grace,
        min_gap: initial.min_gap,
        cooldown: initial.cooldown,
    });
    let mut thresholds = use_signal(|| initial.thresholds);

    let bridge = handle.bridge.clone();
    let apply = use_callback(move |()| {
        let chosen = preferences();
        bridge.send(Command::Configure(chosen.settings(thresholds())));
        if persist {
            let saved = config::save_preferences(&chosen)
                .and_then(|_| config::save_thresholds(thresholds()));
            if let Err(error) = saved {
                tracing::warn!("couldn't save settings: {error}");
            }
        }
    });

    let bridge = handle.bridge.clone();
    let mut choose_camera = move |id: String| {
        let id = (!id.is_empty()).then_some(id);
        preferences.with_mut(|p| p.camera = id.clone());
        bridge.send(Command::UseCamera(id));
        apply.call(());
    };

    let chosen = preferences();
    let limits = thresholds();
    rsx! {
        style { {STYLE} }
        div { class: "settings",
            h1 { "Settings" }

            section {
                h2 { "Camera" }
                select {
                    onchange: move |event| choose_camera(event.value()),
                    option { value: "", selected: chosen.camera.is_none(), "First camera that works" }
                    for camera in cameras {
                        option {
                            value: "{camera.id}",
                            selected: chosen.camera.as_deref() == Some(camera.id.as_str()),
                            "{camera.name} ({camera.id})"
                        }
                    }
                }
                p { class: "hint", "Switching camera recalibrates, so sit up nicely for a moment." }
            }

            section {
                h2 { "Slouch limits" }
                Slider {
                    label: "Eyes drop by",
                    shown: format!("{:.2} face heights", limits.drop),
                    min: 0.2, max: 1.5, step: 0.01, value: limits.drop as f64,
                    onchange: move |v: f64| { thresholds.with_mut(|t| t.drop = v as f32); apply.call(()) },
                }
                Slider {
                    label: "Face grows by",
                    shown: format!("{:.0}%", limits.lean * 100.0),
                    min: 0.05, max: 0.6, step: 0.01, value: limits.lean as f64,
                    onchange: move |v: f64| { thresholds.with_mut(|t| t.lean = v as f32); apply.call(()) },
                }
                p { class: "hint", "The calibration game sets both from how you actually sit." }
            }

            section {
                h2 { "Nagging" }
                Slider {
                    label: "Nag after slouching for",
                    shown: format!("{:.1} s", chosen.grace),
                    min: 0.5, max: 10.0, step: 0.5, value: chosen.grace,
                    onchange: move |v| { preferences.with_mut(|p| p.grace = v); apply.call(()) },
                }
                Slider {
                    label: "Wait between separate slouches",
                    shown: format!("{:.0} s", chosen.min_gap),
                    min: 3.0, max: 120.0, step: 1.0, value: chosen.min_gap,
                    onchange: move |v| { preferences.with_mut(|p| p.min_gap = v); apply.call(()) },
                }
                Slider {
                    label: "Repeat during one long slouch every",
                    shown: format!("{:.0} s", chosen.cooldown),
                    min: 15.0, max: 600.0, step: 15.0, value: chosen.cooldown,
                    onchange: move |v| { preferences.with_mut(|p| p.cooldown = v); apply.call(()) },
                }
            }

            div { class: "buttons",
                button {
                    onclick: move |_| {
                        let defaults = Preferences { camera: preferences().camera, ..Preferences::default() };
                        preferences.set(defaults);
                        thresholds.set(Thresholds::default());
                        apply.call(());
                    },
                    "Restore defaults"
                }
            }
        }
    }
}

#[component]
fn Slider(
    label: &'static str,
    shown: String,
    min: f64,
    max: f64,
    step: f64,
    value: f64,
    onchange: EventHandler<f64>,
) -> Element {
    rsx! {
        label { class: "slider",
            span { class: "slider-label", "{label}" }
            span { class: "slider-value", "{shown}" }
            input {
                r#type: "range",
                min: "{min}",
                max: "{max}",
                step: "{step}",
                value: "{value}",
                oninput: move |event| {
                    if let Ok(v) = event.value().parse() {
                        onchange.call(v);
                    }
                },
            }
        }
    }
}

//! The settings view: when to nudge, what counts as slouching, and history. The camera is chosen
//! on the picture's bottom strip.
//! Changes apply as they're made.

use dioxus::prelude::*;
use posture::{Settings, Thresholds};

use crate::art::Theme;
use crate::config::{self, APP_NAME, Preferences};
use crate::engine::{Change, Command};
use crate::ui::Bridge;

/// The head-drop slider's range, in face sizes.
const DROP_RANGE: (f32, f32) = (0.2, 1.5);
/// The lean slider's range, as a fraction bigger.
const LEAN_RANGE: (f32, f32) = (0.05, 0.6);
/// How far the sensitivity slider goes either side of the calibration, in percent.
const SENSITIVITY_RANGE: i32 = 50;
const SENSITIVITY_STEP: i32 = 5;

#[component]
pub fn SettingsPage(
    initial: Settings,
    calibrated: Thresholds,
    on_calibrate: Callback<()>,
) -> Element {
    let bridge = use_context::<Bridge>();
    // The engine applies and saves each change; this view only shows what it last chose.
    let mut preferences = use_signal(|| Preferences {
        grace: initial.grace,
        min_gap: initial.min_gap,
        cooldown: initial.cooldown,
        keep_history: !bridge.persist || config::load_preferences().keep_history,
        ..Preferences::default()
    });
    let mut thresholds = use_signal(|| initial.thresholds);
    let send = use_callback({
        let bridge = bridge.clone();
        move |change: Change| bridge.send(Command::Change(change))
    });
    let mut set_limits = move |limits: Thresholds| {
        thresholds.set(limits);
        send.call(Change::Drop(limits.drop));
        send.call(Change::Lean(limits.lean));
    };

    let chosen = preferences();
    let limits = thresholds();
    let defaults = Preferences::default();
    let nudges_changed = (chosen.grace, chosen.min_gap, chosen.cooldown)
        != (defaults.grace, defaults.min_gap, defaults.cooldown);
    let sensitivity = sensitivity(limits, calibrated);
    rsx! {
        div { class: "settings settings-view", "data-theme": Theme::Day.name(),
            h1 { "Settings" }

            section {
                div { class: "section-head",
                    h2 { "Nudges" }
                    if nudges_changed {
                        button {
                            class: "link",
                            onclick: move |_| {
                                preferences.with_mut(|p| {
                                    p.grace = defaults.grace;
                                    p.min_gap = defaults.min_gap;
                                    p.cooldown = defaults.cooldown;
                                });
                                send.call(Change::NudgeDefaults);
                            },
                            "Reset"
                        }
                    }
                }
                Slider {
                    label: "Nudge after",
                    shown: seconds(chosen.grace),
                    min: 0.5, max: 10.0, step: 0.5, value: chosen.grace,
                    onchange: move |v| { preferences.with_mut(|p| p.grace = v); send.call(Change::Grace(v)) },
                }
                p { class: "hint",
                    "After {seconds(chosen.grace)} of slouching, then every {duration(chosen.cooldown)} while it lasts."
                }
                details {
                    summary { "Repeats" }
                    div { class: "more",
                        Slider {
                            label: "Again while slouching, every",
                            shown: duration(chosen.cooldown),
                            min: 15.0, max: 600.0, step: 15.0, value: chosen.cooldown,
                            onchange: move |v| { preferences.with_mut(|p| p.cooldown = v); send.call(Change::Cooldown(v)) },
                        }
                        Slider {
                            label: "Treat slouches this close as one",
                            shown: duration(chosen.min_gap),
                            min: 3.0, max: 120.0, step: 1.0, value: chosen.min_gap,
                            onchange: move |v| { preferences.with_mut(|p| p.min_gap = v); send.call(Change::MinGap(v)) },
                        }
                    }
                }
            }

            section {
                div { class: "section-head",
                    h2 { "Slouching" }
                    if limits != calibrated {
                        button {
                            class: "link",
                            title: "Back to the calibrated limits",
                            onclick: move |_| set_limits(calibrated),
                            "Reset"
                        }
                    }
                    button { class: "link", onclick: move |_| on_calibrate.call(()), "Calibrate…" }
                }
                Slider {
                    label: "Sensitivity",
                    shown: sensitivity_text(sensitivity),
                    min: -SENSITIVITY_RANGE as f64,
                    max: SENSITIVITY_RANGE as f64,
                    step: SENSITIVITY_STEP as f64,
                    value: sensitivity as f64,
                    reference: Some(0.0),
                    ends: Some(("Big slumps", "Small dips")),
                    onchange: move |v: f64| set_limits(with_sensitivity(thresholds(), calibrated, v as i32)),
                }
                p { class: "hint",
                    "Nudges when your head sinks {drop_text(limits.drop)} or you lean {lean_text(limits.lean)}."
                }
                details {
                    summary { "Head and lean separately" }
                    div { class: "more",
                        Slider {
                            label: "Head sinks by",
                            shown: drop_text(limits.drop),
                            min: DROP_RANGE.0 as f64, max: DROP_RANGE.1 as f64, step: 0.01, value: limits.drop as f64,
                            reference: Some(calibrated.drop as f64),
                            onchange: move |v: f64| set_limits(Thresholds { drop: v as f32, ..thresholds() }),
                        }
                        Slider {
                            label: "Leans in by",
                            shown: lean_text(limits.lean),
                            min: LEAN_RANGE.0 as f64, max: LEAN_RANGE.1 as f64, step: 0.01, value: limits.lean as f64,
                            reference: Some(calibrated.lean as f64),
                            onchange: move |v: f64| set_limits(Thresholds { lean: v as f32, ..thresholds() }),
                        }
                        p { class: "hint", "The marks show your calibration." }
                    }
                }
            }

            HistoryRow { keep: chosen.keep_history, on_keep: move |keep| {
                preferences.with_mut(|p| p.keep_history = keep);
                send.call(Change::KeepHistory(keep));
            } }
            div { class: "settings-quit",
                button { class: "button", onclick: move |_| crate::quit(), "Quit {APP_NAME}" }
            }
        }
    }
}

/// Keep history, with Clear on the same row asking once before it deletes anything.
#[component]
fn HistoryRow(keep: bool, on_keep: EventHandler<bool>) -> Element {
    let bridge = use_context::<Bridge>();
    let mut confirming = use_signal(|| false);
    let mut cleared = use_signal(|| false);
    // The confirm replaces the row, so Clear… is a new button when it comes back, focused then.
    let mut refocus = use_signal(|| false);
    let mut cancel = move || {
        confirming.set(false);
        refocus.set(true);
    };
    rsx! {
        section { class: "row-section",
            h2 { "History" }
            if confirming() {
                div {
                    class: "row confirm",
                    role: "alertdialog",
                    "aria-label": "Clear all history?",
                    onkeydown: move |event| {
                        if event.key() == Key::Escape {
                            event.prevent_default();
                            cancel();
                        }
                    },
                    span { class: "row-label", "Clear all history? This can't be undone." }
                    button {
                        class: "button small",
                        onmounted: move |event| async move {
                            let _ = event.data().set_focus(true).await;
                        },
                        onclick: move |_| cancel(),
                        "Cancel"
                    }
                    button {
                        class: "button small danger",
                        onclick: move |_| {
                            bridge.send(Command::ClearHistory);
                            cleared.set(true);
                            confirming.set(false);
                            refocus.set(true);
                        },
                        "Clear"
                    }
                }
            } else {
                div { class: "row",
                    label { class: "toggle",
                        input {
                            r#type: "checkbox",
                            checked: keep,
                            onchange: move |event| on_keep.call(event.checked()),
                        }
                        span { "Keep two weeks of history" }
                    }
                    button {
                        class: "button small",
                        // Still focusable once cleared, so the keyboard keeps its place.
                        "aria-disabled": "{cleared()}",
                        onmounted: move |event| async move {
                            if refocus() {
                                refocus.set(false);
                                let _ = event.data().set_focus(true).await;
                            }
                        },
                        onclick: move |_| confirming.set(!cleared()),
                        if cleared() { "Cleared" } else { "Clear…" }
                    }
                }
            }
            p { class: "hint", "Only on this computer. Turning it off keeps what's there." }
        }
    }
}

/// A slider with its value beside the label, an optional mark for the calibrated value, and
/// optional words for its two ends.
#[component]
fn Slider(
    label: &'static str,
    shown: String,
    min: f64,
    max: f64,
    step: f64,
    value: f64,
    #[props(default)] reference: Option<f64>,
    #[props(default)] ends: Option<(&'static str, &'static str)>,
    onchange: EventHandler<f64>,
) -> Element {
    let along = |v: f64| ((v - min) / (max - min)).clamp(0.0, 1.0);
    rsx! {
        label { class: "slider",
            span { class: "slider-label", "{label}" }
            span { class: "slider-value", "{shown}" }
            span { class: "slider-track",
                if let Some(reference) = reference {
                    span {
                        class: "slider-reference",
                        style: "--along: {along(reference)}",
                        title: "Calibrated",
                    }
                }
                input {
                    r#type: "range",
                    min: "{min}",
                    max: "{max}",
                    step: "{step}",
                    value: "{value}",
                    "aria-valuetext": "{shown}",
                    oninput: move |event| {
                        if let Ok(v) = event.value().parse() {
                            onchange.call(v);
                        }
                    },
                }
            }
            if let Some((low, high)) = ends {
                span { class: "slider-ends", span { "{low}" } span { "{high}" } }
            }
        }
    }
}

/// How much stricter than the calibration the limits are, in percent, rounded to the slider's
/// step. Both limits count equally, so moving one alone moves this half as far.
fn sensitivity(limits: Thresholds, calibrated: Thresholds) -> i32 {
    let scale = ((limits.drop / calibrated.drop) * (limits.lean / calibrated.lean)).sqrt();
    let percent = ((1.0 - scale) * 100.0).round() as i32;
    let stepped = (percent as f32 / SENSITIVITY_STEP as f32).round() as i32 * SENSITIVITY_STEP;
    stepped.clamp(-SENSITIVITY_RANGE, SENSITIVITY_RANGE)
}

/// The limits scaled together to `percent` stricter than the calibration, keeping any difference
/// set between them.
fn with_sensitivity(limits: Thresholds, calibrated: Thresholds, percent: i32) -> Thresholds {
    let scale = |limit: f32, calibrated: f32| limit / calibrated;
    let now = (scale(limits.drop, calibrated.drop) * scale(limits.lean, calibrated.lean)).sqrt();
    let wanted = 1.0 - percent as f32 / 100.0;
    let by = wanted / now;
    Thresholds {
        drop: (limits.drop * by).clamp(DROP_RANGE.0, DROP_RANGE.1),
        lean: (limits.lean * by).clamp(LEAN_RANGE.0, LEAN_RANGE.1),
    }
}

fn sensitivity_text(percent: i32) -> String {
    match percent {
        0 => "As calibrated".into(),
        p if p > 0 => format!("{p}% stricter"),
        p => format!("{}% gentler", -p),
    }
}

fn drop_text(drop: f32) -> String {
    format!("{:.0}% of a face", drop * 100.0)
}

fn lean_text(lean: f32) -> String {
    format!("{:.0}% closer", lean * 100.0)
}

fn seconds(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0} s")
    } else {
        format!("{value:.1} s")
    }
}

/// Seconds as people say them: "45 s", "1 min", "2 min 30 s".
fn duration(value: f64) -> String {
    let total = value.round() as u64;
    match (total / 60, total % 60) {
        (0, s) => format!("{s} s"),
        (m, 0) => format!("{m} min"),
        (m, s) => format!("{m} min {s} s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CALIBRATED: Thresholds = Thresholds {
        drop: 0.6,
        lean: 0.2,
    };

    #[test]
    fn sensitivity_scales_both_limits_from_the_calibration() {
        assert_eq!(sensitivity(CALIBRATED, CALIBRATED), 0);
        let stricter = with_sensitivity(CALIBRATED, CALIBRATED, 20);
        assert!((stricter.drop - 0.48).abs() < 1e-5);
        assert!((stricter.lean - 0.16).abs() < 1e-5);
        assert_eq!(sensitivity(stricter, CALIBRATED), 20);
        let back = with_sensitivity(stricter, CALIBRATED, 0);
        assert!((back.drop - CALIBRATED.drop).abs() < 1e-5);
        assert!((back.lean - CALIBRATED.lean).abs() < 1e-5);
    }

    #[test]
    fn sensitivity_keeps_a_difference_set_between_the_limits() {
        let leaner = Thresholds {
            lean: 0.1,
            ..CALIBRATED
        };
        let gentler = with_sensitivity(leaner, CALIBRATED, -20);
        assert!((gentler.drop / gentler.lean - leaner.drop / leaner.lean).abs() < 1e-4);
        assert_eq!(sensitivity(gentler, CALIBRATED), -20);
    }

    #[test]
    fn durations_read_as_people_say_them() {
        assert_eq!(duration(45.0), "45 s");
        assert_eq!(duration(60.0), "1 min");
        assert_eq!(duration(150.0), "2 min 30 s");
        assert_eq!(seconds(1.5), "1.5 s");
        assert_eq!(seconds(3.0), "3 s");
    }
}

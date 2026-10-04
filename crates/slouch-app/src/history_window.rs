//! The history window: how you've sat today, in quarter hours, and over the last week.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{DateTime, Local, Timelike};
use dioxus::prelude::*;

use crate::art::Theme;
use crate::engine::SharedHistory;
use crate::history::{History, Tally, midnight};

const SLOT_MINUTES: i64 = 15;
const REFRESH: Duration = Duration::from_secs(15);
/// The day's chart starts here at the latest, so a morning with nothing yet still has a frame.
const EARLIEST_START_HOUR: u32 = 8;

/// What the history window needs from the app, and a flag saying whether it's open.
#[derive(Clone)]
pub struct HistoryHandle {
    pub history: SharedHistory,
    pub open: Arc<AtomicBool>,
}

impl PartialEq for HistoryHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.open, &other.open)
    }
}

#[component]
pub fn HistoryPage(handle: HistoryHandle) -> Element {
    let open = handle.open.clone();
    use_drop(move || open.store(false, Ordering::Relaxed));
    let mut shown = use_signal(|| handle.history.lock().unwrap().clone());
    use_future(move || {
        let history = handle.history.clone();
        async move {
            loop {
                futures_timer::Delay::new(REFRESH).await;
                let latest = history.lock().unwrap().clone();
                shown.set(latest);
            }
        }
    });

    let now = Local::now();
    let history = shown.read();
    let today = history.tally(midnight(now), now + chrono::Duration::minutes(1));
    rsx! {
        div { class: "history", "data-theme": Theme::Day.name(),
            h1 { "History" }
            section {
                h2 { "Today" }
                p { class: "history-summary", {summary(&today)} }
                TodayChart { slots: day_slots(&history, now) }
                Legend {}
            }
            section {
                h2 { "Last 7 days" }
                WeekChart { days: history.days(now, 7) }
                p { class: "hint", "Share of the time you were at the camera that you spent slouching." }
            }
        }
    }
}

fn summary(today: &Tally) -> String {
    if today.present() == 0 {
        return "Nothing recorded yet today.".into();
    }
    let nudges = match today.nudges {
        1 => "1 nudge".to_string(),
        n => format!("{n} nudges"),
    };
    format!(
        "Slouching {:.0}% of the time you were here · {nudges}",
        today.slouching_share() * 100.0
    )
}

/// Today's quarter hours from whenever there's something to show (8am at the latest) to now.
fn day_slots(history: &History, now: DateTime<Local>) -> Vec<(DateTime<Local>, Tally)> {
    let slots = history.today(now, SLOT_MINUTES);
    let first_used = slots.iter().position(|(_, t)| t.checks() > 0);
    let by_default = slots
        .iter()
        .position(|(start, _)| start.hour() >= EARLIEST_START_HOUR);
    let from = match (first_used, by_default) {
        (Some(used), Some(default)) => used.min(default),
        (used, default) => used.or(default).unwrap_or(0),
    };
    // Start on the hour, so the hour labels line up with bars.
    let from = from - from % (60 / SLOT_MINUTES as usize);
    slots[from..].to_vec()
}

#[component]
fn TodayChart(slots: Vec<(DateTime<Local>, Tally)>) -> Element {
    const WIDTH: f32 = 640.0;
    const HEIGHT: f32 = 150.0;
    const LABELS: f32 = 22.0;
    let bar = WIDTH / slots.len().max(1) as f32;
    let gap = (bar * 0.15).min(2.0);
    rsx! {
        svg { class: "chart", view_box: "0 0 {WIDTH} {HEIGHT + LABELS}", role: "img",
            "aria-label": "Today's posture in quarter hours",
            line { class: "chart-axis", x1: "0", x2: "{WIDTH}", y1: "{HEIGHT}", y2: "{HEIGHT}" }
            for (i, (start, tally)) in slots.iter().enumerate() {
                for (class, from, to) in stack(tally) {
                    rect {
                        class: "{class}",
                        x: "{i as f32 * bar + gap / 2.0}",
                        y: "{HEIGHT * (1.0 - to)}",
                        width: "{bar - gap}",
                        height: "{HEIGHT * (to - from)}",
                    }
                }
                if start.minute() == 0 && start.hour() % 2 == 0 {
                    text { class: "chart-label", x: "{i as f32 * bar}", y: "{HEIGHT + 16.0}",
                        {start.format("%H:%M").to_string()}
                    }
                }
            }
        }
    }
}

/// Where each kind of check sits in a bar, as shares from the bottom: well, then slouching, then away.
fn stack(tally: &Tally) -> Vec<(&'static str, f32, f32)> {
    let checks = tally.checks();
    if checks == 0 {
        return Vec::new();
    }
    let share = |n: u32| n as f32 / checks as f32;
    let well = share(tally.well);
    let slouching = well + share(tally.slouching);
    vec![
        ("well", 0.0, well),
        ("slouching", well, slouching),
        ("away", slouching, 1.0),
    ]
}

#[component]
fn WeekChart(days: Vec<(DateTime<Local>, Tally)>) -> Element {
    const WIDTH: f32 = 640.0;
    const HEIGHT: f32 = 110.0;
    const LABELS: f32 = 22.0;
    let column = WIDTH / days.len().max(1) as f32;
    // Scale to the worst day, but never so far that a good week looks alarming.
    let top = days
        .iter()
        .map(|(_, t)| t.slouching_share())
        .fold(0.25, f32::max);
    rsx! {
        svg { class: "chart", view_box: "0 -18 {WIDTH} {HEIGHT + LABELS + 18.0}", role: "img",
            "aria-label": "Share of time spent slouching each day this week",
            line { class: "chart-axis", x1: "0", x2: "{WIDTH}", y1: "{HEIGHT}", y2: "{HEIGHT}" }
            for (i, (day, tally)) in days.iter().enumerate() {
                if tally.present() > 0 {
                    rect {
                        class: "slouching",
                        x: "{i as f32 * column + column * 0.25}",
                        y: "{HEIGHT * (1.0 - tally.slouching_share() / top)}",
                        width: "{column * 0.5}",
                        height: "{HEIGHT * tally.slouching_share() / top}",
                    }
                    text { class: "chart-value", x: "{(i as f32 + 0.5) * column}",
                        y: "{HEIGHT * (1.0 - tally.slouching_share() / top) - 6.0}",
                        "{tally.slouching_share() * 100.0:.0}%"
                    }
                }
                text { class: "chart-label centred", x: "{(i as f32 + 0.5) * column}", y: "{HEIGHT + 16.0}",
                    {day.format("%a").to_string()}
                }
            }
        }
    }
}

#[component]
fn Legend() -> Element {
    rsx! {
        div { class: "legend",
            for (class, name) in [("well", "Sitting well"), ("slouching", "Slouching"), ("away", "Away")] {
                span { span { class: "swatch {class}" } "{name}" }
            }
        }
    }
}

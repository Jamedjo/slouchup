//! The guided calibration's full-screen window: each step taught as it comes, then its Get
//! ready and Hold, then the results. Everything sits on one centred column, under where a
//! laptop's camera usually is, so looking at it doesn't turn your head.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Sender;
use dioxus::prelude::*;
use posture::{Pose, Thresholds};

use crate::art::{self, Theme};
use crate::camera_view::{CameraView, FrameSlot};
use crate::engine::{Answer, Command, LookAt, Phase};
use crate::ui::RisingUp;

/// The current step, shared with this window, which has its own virtual DOM.
#[derive(Clone)]
pub struct SharedLookAt(pub Arc<Mutex<LookAt>>);

impl PartialEq for SharedLookAt {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Where this window's buttons and keys go.
#[derive(Clone)]
pub struct Answers(pub Sender<Command>);

impl PartialEq for Answers {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Answers {
    fn send(&self, answer: Answer) {
        let _ = self.0.send(Command::Guided(answer));
    }
}

const ESCAPE: &str = r#"
document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
        event.preventDefault();
        dioxus.send("escape");
    }
});
await new Promise(() => {});
"#;

#[component]
pub fn Guided(step: SharedLookAt, slot: FrameSlot, answers: Answers) -> Element {
    let mut shown = use_signal(LookAt::default);
    use_future(move || {
        let step = step.clone();
        async move {
            loop {
                let latest = step.0.lock().unwrap().clone();
                if *shown.peek() != latest {
                    shown.set(latest);
                }
                futures_timer::Delay::new(Duration::from_millis(100)).await;
            }
        }
    });
    // Esc wherever focus is, since the Hold has nothing to focus. Leaving the results keeps them.
    use_future({
        let answers = answers.clone();
        move || {
            let answers = answers.clone();
            async move {
                let mut escapes = document::eval(ESCAPE);
                while escapes.recv::<String>().await.is_ok() {
                    let done = matches!(shown.peek().phase, Phase::Done(_));
                    answers.send(if done { Answer::Next } else { Answer::Cancel });
                }
            }
        }
    });
    let current = shown.read().clone();
    let stepping = matches!(current.phase, Phase::Teach | Phase::Ready | Phase::Hold);
    let stop = answers.clone();
    rsx! {
        div {
            class: "guided",
            "data-theme": Theme::Night.name(),
            header { class: "guided-top",
                span {
                    if stepping {
                        "Step {current.step} of {current.steps}"
                    }
                }
                // One for the whole calibration: its frames come through a handler that a second
                // preview would take over and then drop. At the top and centred, beside where a
                // laptop's camera sits, so glancing at it doesn't turn your head.
                div {
                    class: "mini-camera",
                    hidden: matches!(current.phase, Phase::Done(_)),
                    CameraView { slot }
                }
                span { class: "stop-place",
                    if stepping {
                        button {
                            class: "stop",
                            title: "Stop, keeping the calibration from before (Esc)",
                            onclick: move |_| stop.send(Answer::Cancel),
                            "Stop"
                        }
                    }
                }
            }
            match current.phase.clone() {
                Phase::Teach => rsx! { Teach { step: current.clone(), answers } },
                Phase::Ready => rsx! { Ready { step: current.clone(), answers } },
                Phase::Hold => rsx! { Hold { step: current.clone() } },
                Phase::Done(limits) => rsx! { Done { limits, answers } },
                Phase::Failed(prompt) => rsx! { Failed { prompt, answers } },
            }
        }
    }
}

/// The first step of each pose, the first time through: its drawing and one line on what to
/// do, until Next.
#[component]
fn Teach(step: LookAt, answers: Answers) -> Element {
    let pose = step.pose.unwrap_or(Pose::Upright);
    let label = if step.step == 1 { "Start" } else { "Next" };
    rsx! {
        div { class: "guided-body",
            div { class: "drawing", dangerous_inner_html: pose_svg(pose) }
            h1 { class: "guided-title", Prompt { pose, text: lesson_title(pose) } }
            p { class: "guided-detail", "{lesson(pose)}" }
        }
        footer { class: "guided-buttons",
            PrimaryButton {
                key: "teach-{step.step}",
                label,
                onclick: move |_| answers.send(Answer::Next),
            }
        }
    }
}

fn lesson_title(pose: Pose) -> String {
    match pose {
        Pose::Upright => "Sit up",
        Pose::Slump => "Slouch down, not forward",
        Pose::Lean => "Slouch forward",
    }
    .into()
}

fn lesson(pose: Pose) -> &'static str {
    match pose {
        Pose::Upright => "Sit tall and comfortable, the way you'd like to sit all day.",
        Pose::Slump => "Sink into your chair, like it's late on a Friday, without leaning in.",
        Pose::Lean => {
            "Lean in towards the screen, the way you do when something's too small to read."
        }
    }
}

/// A step's instructions, until you're in the pose. Next starts measuring straight away.
#[component]
fn Ready(step: LookAt, answers: Answers) -> Element {
    let pose = step.pose.unwrap_or(Pose::Upright);
    rsx! {
        div { class: "guided-body",
            div { class: "drawing", dangerous_inner_html: pose_svg(pose) }
            h1 { class: "guided-title", Prompt { pose, text: step.prompt } }
            p { class: "guided-detail", "{step.detail}" }
            p { class: "getting-ready",
                span { class: "pulse", "aria-hidden": "true" }
                if step.in_frame { "Get into it now" } else { "You're out of frame" }
            }
        }
        footer { class: "guided-buttons",
            PrimaryButton { key: "ready-{step.step}", label: "Next", onclick: move |_| answers.send(Answer::Next) }
        }
    }
}

/// Measuring: nothing to read, only the eyes to look at and the ring filling round them.
#[component]
fn Hold(step: LookAt) -> Element {
    rsx! {
        div { class: "guided-body hold",
            Ring { progress: step.progress }
            p { class: "hold-word", "Hold" }
        }
    }
}

#[component]
fn Done(limits: Thresholds, answers: Answers) -> Element {
    let again = answers.clone();
    rsx! {
        div { class: "guided-body",
            div { class: "result-eyes", dangerous_inner_html: art::looking_up_svg() }
            h1 { class: "guided-title all-set", "All set" }
            p { class: "guided-detail",
                "You'll get a nudge when your head sinks {limits.drop * 100.0:.0}% of a face lower than now, or you're {limits.lean * 100.0:.0}% closer to the screen."
            }
        }
        footer { class: "guided-buttons",
            button { class: "big", onclick: move |_| again.send(Answer::Again), "Do it again" }
            PrimaryButton { label: "Done", onclick: move |_| answers.send(Answer::Next) }
        }
    }
}

#[component]
fn Failed(prompt: String, answers: Answers) -> Element {
    let not_now = answers.clone();
    rsx! {
        div { class: "guided-body",
            h1 { class: "guided-title", "Let's try that again" }
            p { class: "guided-detail",
                "You were out of frame during “{prompt}”. Check you're in frame, then try again."
            }
        }
        footer { class: "guided-buttons",
            button { class: "big", onclick: move |_| not_now.send(Answer::Cancel), "Not now" }
            PrimaryButton { label: "Try again", onclick: move |_| answers.send(Answer::Again) }
        }
    }
}

/// The big button that Enter and Space press, focused as it appears so the keyboard needs no
/// Tab to reach it.
#[component]
fn PrimaryButton(label: &'static str, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button {
            class: "big primary",
            onmounted: move |event| async move {
                let _ = event.data().set_focus(true).await;
            },
            onclick: move |event| onclick.call(event),
            "{label}"
        }
    }
}

/// A prompt with the word that matters picked out: the "up" rises, "down" and "forward" are
/// in the accent colour.
#[component]
fn Prompt(pose: Pose, text: String) -> Element {
    let word = match pose {
        Pose::Upright => return rsx! { RisingUp { text } },
        Pose::Slump => "down",
        Pose::Lean => "forward",
    };
    match text.split_once(word) {
        Some((before, after)) => rsx! { "{before}" em { "{word}" } "{after}" },
        None => rsx! { "{text}" },
    }
}

/// The eyes, with a ring round them that fills as the Hold goes on.
#[component]
fn Ring(progress: f32) -> Element {
    let full = 2.0 * std::f32::consts::PI * 170.0;
    let filled = progress.clamp(0.0, 1.0) * full;
    let svg = format!(
        r##"<svg viewBox="0 0 400 400" aria-hidden="true"><circle cx="200" cy="200" r="170" fill="none" stroke="var(--track)" stroke-width="14"/><circle cx="200" cy="200" r="170" fill="none" stroke="var(--tomato)" stroke-width="14" stroke-linecap="round" stroke-dasharray="{filled:.1} {full:.1}" transform="rotate(-90 200 200)"/><g transform="translate(80 152) scale(1.2)">{eyes}</g></svg>"##,
        eyes = art::eyes_markup(),
    );
    rsx! {
        div { class: "ring", dangerous_inner_html: svg }
    }
}

/// The side view of someone at a desk in `pose`, with the upright pose ghosted behind the others
/// and an arrow for which way they've moved.
fn pose_svg(pose: Pose) -> String {
    let (figure, arrow) = match pose {
        Pose::Upright => (
            UPRIGHT_FIGURE,
            r##"<path d="M116 58 H252" stroke="#FFF1C9" stroke-width="2.5" stroke-dasharray="3 7" stroke-linecap="round" opacity=".55"/>"##,
        ),
        Pose::Slump => (
            r##"<path d="M116 86 H252" stroke="#FFF1C9" stroke-width="2.5" stroke-dasharray="3 7" stroke-linecap="round" opacity=".55"/><g stroke="#FFF1C9" stroke-width="7" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M108 146 Q66 140 82 110"/><path d="M82 110 L112 138 L168 134"/><path d="M108 146 L166 152 L166 205 L182 205"/><circle cx="94" cy="86" r="17"/><circle cx="102" cy="83" r="2.5" fill="#FFF1C9" stroke="none"/></g>"##,
            r##"<g stroke="#EC6247" stroke-width="6" stroke-linecap="round" stroke-linejoin="round" fill="none"><path d="M128 50 L128 92"/><path d="M119 83 L128 92 L137 83"/></g>"##,
        ),
        Pose::Lean => (
            r##"<path d="M166 72 H252" stroke="#FFF1C9" stroke-width="2.5" stroke-dasharray="3 7" stroke-linecap="round" opacity=".55"/><g stroke="#FFF1C9" stroke-width="7" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M82 142 Q92 112 118 94"/><path d="M118 94 L134 128 L178 132"/><path d="M82 142 L146 146 L146 205 L162 205"/><circle cx="144" cy="72" r="17"/><circle cx="152" cy="69" r="2.5" fill="#FFF1C9" stroke="none"/></g>"##,
            r##"<g stroke="#EC6247" stroke-width="6" stroke-linecap="round" stroke-linejoin="round" fill="none"><path d="M112 30 L162 30"/><path d="M153 21 L162 30 L153 39"/></g>"##,
        ),
    };
    let ghost = if pose == Pose::Upright {
        ""
    } else {
        GHOST_FIGURE
    };
    format!(
        r##"<svg viewBox="25 12 290 206" aria-hidden="true">{DESK}{ghost}{figure}{arrow}</svg>"##
    )
}

const DESK: &str = r##"<g stroke="#5C574C" stroke-width="6" stroke-linecap="round" fill="none"><path d="M50 152 H122 M44 152 L36 78 M56 152 V210 M116 152 V210"/><path d="M160 140 H304 M292 140 V210"/><path d="M262 44 V116 M262 116 V140 M248 140 H276"/></g><rect x="256" y="40" width="12" height="78" rx="4" fill="#5C574C"/>"##;
const UPRIGHT_FIGURE: &str = r##"<g stroke="#FFF1C9" stroke-width="7" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M82 142 Q80 112 86 84"/><path d="M86 84 L104 120 L166 132"/><path d="M82 142 L146 146 L146 205 L162 205"/><circle cx="94" cy="58" r="17"/><circle cx="102" cy="55" r="2.5" fill="#FFF1C9" stroke="none"/></g>"##;
const GHOST_FIGURE: &str = r##"<g stroke="#FFF1C9" stroke-width="7" fill="none" stroke-linecap="round" stroke-linejoin="round" opacity="0.18"><path d="M82 142 Q80 112 86 84"/><path d="M86 84 L104 120 L166 132"/><path d="M82 142 L146 146 L146 205 L162 205"/><circle cx="94" cy="58" r="17"/><circle cx="102" cy="55" r="2.5" fill="#FFF1C9" stroke="none"/></g>"##;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pose_has_a_drawing() {
        for pose in [Pose::Upright, Pose::Slump, Pose::Lean] {
            let svg = pose_svg(pose);
            assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        }
    }
}

//! A drawn stand-in for the webcam: an illustrated person who sits up, sinks and leans in.
//!
//! Used for screenshots and for trying the app without a camera. The face is drawn plainly
//! enough for the real face detector to find, so everything downstream runs for real.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use posture::Pose;

use crate::frames::Picture;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const FRAME_INTERVAL: Duration = Duration::from_millis(100);
/// Seconds to ease most of the way into a new pose.
const EASING: f32 = 0.5;
/// The pose cycle when nothing asks for a particular pose, in seconds.
const ROUTINE: [(Pose, f32); 4] = [
    (Pose::Upright, 8.0),
    (Pose::Slump, 5.0),
    (Pose::Upright, 6.0),
    (Pose::Lean, 5.0),
];

/// Shared with the drawing thread: what to act out, and whether to draw at all.
#[derive(Clone, Default)]
pub struct Demo {
    requested: Arc<Mutex<Option<Pose>>>,
    paused: Arc<AtomicBool>,
}

impl Demo {
    pub fn start(mut on_frame: impl FnMut(Picture) + Send + 'static) -> Self {
        let demo = Demo::default();
        let shared = demo.clone();
        std::thread::Builder::new()
            .name("slouch-demo".into())
            .spawn(move || {
                let started = Instant::now();
                let mut shown = Shape::of(Pose::Upright);
                loop {
                    std::thread::sleep(FRAME_INTERVAL);
                    if shared.paused.load(Ordering::Relaxed) {
                        continue;
                    }
                    let pose = shared
                        .requested
                        .lock()
                        .unwrap()
                        .unwrap_or_else(|| routine(started.elapsed()));
                    let ease = 1.0 - (-FRAME_INTERVAL.as_secs_f32() / EASING).exp();
                    shown = shown.towards(Shape::of(pose), ease);
                    let pixels = crate::art::rasterise(&person_svg(shown), WIDTH, HEIGHT);
                    on_frame(Picture::Rgba {
                        width: WIDTH,
                        height: HEIGHT,
                        pixels: Arc::new(pixels),
                    });
                }
            })
            .expect("spawning the demo thread");
        demo
    }

    /// Act out `pose`, or go back to the routine with `None`.
    pub fn act(&self, pose: Option<Pose>) {
        *self.requested.lock().unwrap() = pose;
    }

    pub fn set_active(&self, active: bool) {
        self.paused.store(!active, Ordering::Relaxed);
    }
}

fn routine(elapsed: Duration) -> Pose {
    let cycle: f32 = ROUTINE.iter().map(|(_, seconds)| seconds).sum();
    let mut t = elapsed.as_secs_f32() % cycle;
    for (pose, seconds) in ROUTINE {
        if t < seconds {
            return pose;
        }
        t -= seconds;
    }
    Pose::Upright
}

/// How the person is drawn: how far down they've sunk and how close they are.
#[derive(Clone, Copy)]
struct Shape {
    sink: f32,
    scale: f32,
}

impl Shape {
    fn of(pose: Pose) -> Self {
        match pose {
            Pose::Upright => Shape {
                sink: 0.0,
                scale: 1.0,
            },
            Pose::Slump => Shape {
                sink: 80.0,
                scale: 1.0,
            },
            Pose::Lean => Shape {
                sink: 40.0,
                scale: 1.4,
            },
        }
    }

    fn towards(self, target: Shape, amount: f32) -> Shape {
        Shape {
            sink: self.sink + amount * (target.sink - self.sink),
            scale: self.scale + amount * (target.scale - self.scale),
        }
    }
}

fn person_svg(shape: Shape) -> String {
    let Shape { sink, scale } = shape;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 480">
<defs>
<linearGradient id="wall" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#cfd8e6"/><stop offset="1" stop-color="#9aa8bf"/></linearGradient>
<radialGradient id="lamp" cx="0.5" cy="0.05" r="0.6"><stop offset="0" stop-color="#fff8e1" stop-opacity="0.9"/><stop offset="1" stop-color="#fff8e1" stop-opacity="0"/></radialGradient>
<radialGradient id="skin" cx="0.42" cy="0.38" r="0.7"><stop offset="0" stop-color="#f1c7a2"/><stop offset="0.7" stop-color="#d7a07a"/><stop offset="1" stop-color="#b47c59"/></radialGradient>
<radialGradient id="shirt" cx="0.5" cy="0.2" r="0.9"><stop offset="0" stop-color="#3fc9a8"/><stop offset="1" stop-color="#1d7f74"/></radialGradient>
<linearGradient id="hair" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#5b4033"/><stop offset="1" stop-color="#3a281f"/></linearGradient>
</defs>
<rect width="640" height="480" fill="url(#wall)"/>
<rect width="640" height="480" fill="url(#lamp)"/>
<rect x="470" y="120" width="120" height="150" rx="6" fill="#7d8aa3"/><rect x="480" y="130" width="100" height="130" rx="4" fill="#e9c46a"/>
<rect x="40" y="200" width="90" height="280" fill="#8b6f5a"/>
<g transform="translate(320 {cy}) scale({scale}) translate(-320 -240)">
<path d="M120 480 C130 380 210 350 320 350 C430 350 510 380 520 480 L520 640 L120 640 Z" fill="url(#shirt)"/>
<rect x="292" y="300" width="56" height="70" rx="20" fill="#c58f6b"/>
<path d="M228 250 C224 160 266 122 320 122 C374 122 416 160 412 250 L412 282 C402 292 390 290 386 280 L254 280 C250 290 238 292 228 282 Z" fill="url(#hair)"/>
<ellipse cx="320" cy="240" rx="76" ry="98" fill="url(#skin)"/>
<path d="M244 214 C250 160 284 140 320 140 C356 140 392 160 396 214 C380 182 352 172 334 178 C318 168 288 176 244 214 Z" fill="url(#hair)"/>
<path d="M270 212 Q288 205 304 211" stroke="#4a3328" stroke-width="4" fill="none" stroke-linecap="round"/>
<path d="M336 211 Q352 205 370 212" stroke="#4a3328" stroke-width="4" fill="none" stroke-linecap="round"/>
<ellipse cx="288" cy="234" rx="16" ry="10" fill="#fff"/><ellipse cx="352" cy="234" rx="16" ry="10" fill="#fff"/>
<circle cx="288" cy="234" r="7" fill="#3b2a1e"/><circle cx="352" cy="234" r="7" fill="#3b2a1e"/>
<circle cx="290" cy="232" r="2" fill="#fff"/><circle cx="354" cy="232" r="2" fill="#fff"/>
<path d="M320 240 L311 274 Q320 281 329 274 Z" fill="#c4875f"/>
<path d="M296 298 Q320 314 344 298" stroke="#9c5a50" stroke-width="5" fill="none" stroke-linecap="round"/>
</g></svg>"##,
        cy = 240.0 + sink,
    )
}

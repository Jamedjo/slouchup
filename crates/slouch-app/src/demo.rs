//! A drawn stand-in for the webcam: an illustrated person who sits up, sinks and leans in.
//!
//! Used for screenshots and for trying the app without a camera. The face is drawn plainly
//! enough for the real face detector to find, so everything downstream runs for real.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use posture::Pose;

use crate::frames::Picture;
use crate::history::{History, Sitting, midnight};

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

/// Shared with the drawing thread: what to act out, and when to stop.
#[derive(Clone, Default)]
pub struct Demo {
    requested: Arc<Mutex<Option<Pose>>>,
    stopped: Arc<AtomicBool>,
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
                while !shared.stopped.load(Ordering::Relaxed) {
                    std::thread::sleep(FRAME_INTERVAL);
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

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

/// A made-up week of history for the history chart, since a demo has none of its own:
/// working days with a lunch break, and slouching that creeps up through each afternoon.
pub fn sample_history(now: chrono::DateTime<chrono::Local>) -> History {
    let mut history = History::default();
    let mut seed = 0x2545_f491_u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 1000) as f32 / 1000.0
    };
    for days_back in 0..7 {
        let day = midnight(now - chrono::Duration::days(days_back));
        let tiredness = random() * 0.15;
        for minute in (9 * 60)..(17 * 60 + 30) {
            let at = day + chrono::Duration::minutes(minute);
            if at > now {
                break;
            }
            let hours_in = (minute - 9 * 60) as f32 / 60.0;
            let lunch = (12 * 60 + 30..13 * 60 + 15).contains(&minute);
            let slouchy = 0.04 + tiredness + 0.035 * hours_in;
            for _ in 0..5 {
                let sitting = if lunch || random() < 0.03 {
                    Sitting::Away
                } else if random() < slouchy {
                    Sitting::Slouching
                } else {
                    Sitting::Well
                };
                history.record(at, sitting);
            }
            if !lunch && random() < slouchy * 0.15 {
                history.nudged(at);
            }
        }
    }
    history
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

/// The person and their room, with `{cy}` and `{scale}` left for the pose. The homepage draws
/// the same file.
const PERSON: &str = include_str!("person.svg");

fn person_svg(shape: Shape) -> String {
    let Shape { sink, scale } = shape;
    PERSON
        .replace("{cy}", &(240.0 + sink).to_string())
        .replace("{scale}", &scale.to_string())
}

#[cfg(test)]
mod tests {
    use posture::{Posture, Reading, Thresholds};

    use super::*;
    use crate::frames::half_size;

    const MODEL: &[u8] = include_bytes!("../../../models/face_detection_yunet_2023mar.onnx");

    /// The demo person in `pose`, run through the same steps as a camera frame.
    fn posture_of(pose: Pose, detector: &yunet::Detector) -> Posture {
        let pixels = crate::art::rasterise(&person_svg(Shape::of(pose)), WIDTH, HEIGHT);
        let picture = Picture::Rgba {
            width: WIDTH,
            height: HEIGHT,
            pixels: Arc::new(pixels),
        };
        let half = half_size(&picture).expect("RGBA always converts");
        let faces = detector
            .detect(&half.rgb, half.width, half.height)
            .expect("detection runs");
        let l = faces
            .first()
            .unwrap_or_else(|| panic!("no face found in {pose:?}"))
            .landmarks;
        Posture::from_landmarks(l.right_eye, l.left_eye, l.right_mouth, l.left_mouth, 0.0)
    }

    #[test]
    fn slumping_and_leaning_cross_the_default_thresholds() {
        let detector =
            yunet::Detector::new(MODEL, (WIDTH / 2) as usize, (HEIGHT / 2) as usize).unwrap();
        let upright = posture_of(Pose::Upright, &detector);
        let limits = Thresholds::default();

        let slump = Reading::new(posture_of(Pose::Slump, &detector), upright);
        assert!(slump.drop > limits.drop, "slump drop {:.2}", slump.drop);
        assert!(slump.lean < limits.lean, "slump lean {:.2}", slump.lean);

        let lean = Reading::new(posture_of(Pose::Lean, &detector), upright);
        assert!(lean.lean > limits.lean, "lean {:.2}", lean.lean);
    }

    #[test]
    /// The tight crop trims a little context the detector uses, which moves readings by up to
    /// a few percent: under a tenth of any limit the calibration game has set.
    fn cropped_search_finds_the_same_landmarks_as_the_whole_frame() {
        let mut finder = crate::finder::FaceFinder::new();
        let whole =
            yunet::Detector::new(MODEL, (WIDTH / 2) as usize, (HEIGHT / 2) as usize).unwrap();
        // The first pose primes the finder from the whole frame; the rest go through the crop.
        for pose in [Pose::Upright, Pose::Upright, Pose::Slump, Pose::Lean] {
            let pixels = crate::art::rasterise(&person_svg(Shape::of(pose)), WIDTH, HEIGHT);
            let picture = Picture::Rgba {
                width: WIDTH,
                height: HEIGHT,
                pixels: Arc::new(pixels),
            };
            let half = half_size(&picture).unwrap();
            // A pose arrives all at once here, so let the square recentre before comparing.
            finder.find(&half.rgb, half.width, half.height);
            let found = finder
                .find(&half.rgb, half.width, half.height)
                .expect("finder sees the face");
            let reference = whole.detect(&half.rgb, half.width, half.height).unwrap()[0];
            let posture = |f: yunet::Face| {
                let l = f.landmarks;
                Posture::from_landmarks(l.right_eye, l.left_eye, l.right_mouth, l.left_mouth, 0.0)
            };
            let (found, reference) = (posture(found), posture(reference));
            let drop = (found.eye_y - reference.eye_y).abs() / reference.size;
            let size = (found.size / reference.size - 1.0).abs();
            eprintln!("{pose:?}: eye line off by {drop:.3} face sizes, size by {size:.3}");
            assert!(
                drop < 0.03 && size < 0.03,
                "{pose:?}: drop {drop:.3}, size {size:.3}"
            );
        }
    }

    #[test]
    fn routine_cycles_through_every_pose() {
        let poses: Vec<Pose> = (0..24).map(|s| routine(Duration::from_secs(s))).collect();
        for pose in [Pose::Upright, Pose::Slump, Pose::Lean] {
            assert!(poses.contains(&pose), "{pose:?} never shown");
        }
    }
}

//! Posture metrics, slouch detection and calibration scoring from facial landmarks.
//!
//! Nothing here touches a camera or a UI: feed [`Tracker`] a [`Posture`] (or `None` when no face
//! is visible) with the current time, and act on the [`Update`] it returns.

mod game;
mod tracker;

pub use game::{GameResult, MetricResult, Pose, Sample, Step, game_steps, score_game};
pub use tracker::{Action, Settings, State, Tracker, Update};

use serde::{Deserialize, Serialize};

/// How you are sitting, from one frame: where your eyes are and how big your face is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Posture {
    /// Height of the eye line in pixels, corrected for camera tilt. Larger is lower in the frame.
    pub eye_y: f32,
    /// Face size in pixels. Grows as you lean towards the camera.
    pub size: f32,
}

impl Posture {
    /// From the eye and mouth corner positions (`[x, y]` pixels) and the camera's vertical drift in pixels.
    pub fn from_landmarks(
        right_eye: [f32; 2],
        left_eye: [f32; 2],
        right_mouth: [f32; 2],
        left_mouth: [f32; 2],
        drift_px: f32,
    ) -> Self {
        let eyes = midpoint(right_eye, left_eye);
        let mouth = midpoint(right_mouth, left_mouth);
        // Averaging a horizontal and a vertical span keeps size steadier when the head turns or nods.
        let size = (distance(right_eye, left_eye) + distance(eyes, mouth)) / 2.0;
        Self {
            eye_y: eyes[1] - drift_px,
            size,
        }
    }

    /// Finite, with a face of some size.
    pub fn is_valid(&self) -> bool {
        self.eye_y.is_finite() && self.size.is_finite() && self.size > 0.0
    }

    fn lerp(self, towards: Posture, amount: f32) -> Posture {
        Posture {
            eye_y: self.eye_y + amount * (towards.eye_y - self.eye_y),
            size: self.size + amount * (towards.size - self.size),
        }
    }
}

fn midpoint(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// How far each metric may move from the baseline before it counts as slouching.
///
/// Serialised with the Python version's field names so both read the same thresholds file.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    /// How far the eyes may drop, in face sizes.
    #[serde(rename = "threshold")]
    pub drop: f32,
    /// How much bigger the face may get, as a fraction.
    pub lean: f32,
}

impl Thresholds {
    /// These limits with any that can't work (not a number, or not positive) replaced by
    /// the default; the thresholds file can be edited by hand.
    pub fn checked(self) -> Self {
        let defaults = Thresholds::default();
        let pick = |value: f32, default: f32| {
            if value.is_finite() && value > 0.0 {
                value
            } else {
                default
            }
        };
        Thresholds {
            drop: pick(self.drop, defaults.drop),
            lean: pick(self.lean, defaults.lean),
        }
    }
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            drop: 0.8,
            lean: 0.15,
        }
    }
}

/// A posture compared with the baseline.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    /// How far the eyes have dropped, in baseline face sizes.
    pub drop: f32,
    /// How much bigger the face is, as a fraction of the baseline size.
    pub lean: f32,
}

impl Reading {
    /// Compare `posture` with `baseline`, whose size must be positive.
    pub fn new(posture: Posture, baseline: Posture) -> Self {
        Self {
            drop: (posture.eye_y - baseline.eye_y) / baseline.size,
            lean: posture.size / baseline.size - 1.0,
        }
    }

    /// The metric closest to (or furthest past) its threshold, as a fraction of it.
    pub fn worst(&self, thresholds: Thresholds) -> (f32, Problem) {
        let lean = self.lean / thresholds.lean;
        let drop = self.drop / thresholds.drop;
        if lean >= drop {
            (lean, Problem::Leaning(self.lean))
        } else {
            (drop, Problem::Sinking)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Problem {
    /// Face is this fraction bigger than usual.
    Leaning(f32),
    /// Eyes have dropped below the baseline.
    Sinking,
    /// The face vanished while posture was already heading somewhere bad.
    DroppedOutOfView,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::Leaning(lean) => write!(
                f,
                "You're {:.0}% closer to the screen than usual.",
                lean * 100.0
            ),
            Problem::Sinking => write!(f, "Your head has sunk lower than usual."),
            Problem::DroppedOutOfView => write!(f, "Your head dropped out of view."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_read_the_python_versions_file() {
        let saved = r#"{"threshold": 0.5874, "lean": 0.2527}"#;
        let thresholds: Thresholds = serde_json::from_str(saved).unwrap();
        assert_eq!(
            thresholds,
            Thresholds {
                drop: 0.5874,
                lean: 0.2527
            }
        );
    }

    #[test]
    fn leaning_is_reported_when_it_is_the_worse_problem() {
        let baseline = Posture {
            eye_y: 100.0,
            size: 40.0,
        };
        let reading = Reading::new(
            Posture {
                eye_y: 104.0,
                size: 50.0,
            },
            baseline,
        );
        let (ratio, problem) = reading.worst(Thresholds::default());
        assert!(ratio > 1.0);
        assert_eq!(problem, Problem::Leaning(0.25));
    }
}

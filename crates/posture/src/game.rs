use serde::{Deserialize, Serialize};

use crate::{Posture, Reading, Thresholds};

/// Minimum frames of a face per step for a game to count.
const MIN_SAMPLES: usize = 5;
/// A metric must separate from sitting upright by this many times its jitter to be trusted.
const MIN_SEPARATION: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pose {
    Upright,
    Slump,
    Lean,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub pose: Pose,
    pub prompt: String,
    /// Which screen to look at, as an index into the names given to [`game_steps`].
    pub screen: Option<usize>,
}

/// One recorded frame: which step it belongs to and the posture seen.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Counting from 1, as shown to the player. Scoring takes the pose from the step itself.
    pub step: usize,
    /// The step's pose, kept so recordings can be read on their own.
    pub pose: Pose,
    pub posture: Posture,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricResult {
    /// Typical frame-to-frame noise while holding a pose.
    pub jitter: f32,
    /// The furthest any upright step sat from the baseline; looking at different screens moves it.
    pub upright_max: f32,
    pub slump: f32,
    pub lean: f32,
    /// Halfway between sitting upright and slouching, or `None` when the two don't separate.
    pub threshold: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameResult {
    pub baseline: Posture,
    pub drop: MetricResult,
    pub lean: MetricResult,
}

impl GameResult {
    /// `current` with every metric the game could measure replaced.
    pub fn thresholds(&self, current: Thresholds) -> Thresholds {
        Thresholds {
            drop: self.drop.threshold.unwrap_or(current.drop),
            lean: self.lean.threshold.unwrap_or(current.lean),
        }
    }
}

/// The game's steps: sit upright looking at each screen, then slump, sit up, and lean in.
pub fn game_steps(screens: &[String]) -> Vec<Step> {
    let step = |pose, prompt: &str, screen| Step {
        pose,
        prompt: prompt.to_string(),
        screen,
    };
    // Each prompt shows on the screen it's about, so "here" names it.
    let mut steps: Vec<Step> = if screens.len() < 2 {
        vec![step(Pose::Upright, "Sit up, look here", None)]
    } else {
        (0..screens.len())
            .map(|i| step(Pose::Upright, "Sit up, look here", Some(i)))
            .collect()
    };
    steps.extend([
        step(Pose::Slump, "Slouch down, not forward", None),
        step(Pose::Upright, "Sit up again", None),
        step(Pose::Lean, "Slouch forward", None),
    ]);
    steps
}

/// How far into a slouch, as a share of the current limit, counts as having moved into it.
const MOVED_SHARE: f32 = 0.5;
/// Frames that must sit still before a pose counts as reached.
const STEADY_FRAMES: usize = 6;
/// How much the eye line may wander between those frames, in face sizes.
const STEADY_EYES: f32 = 0.08;
/// How much the face size may wander between those frames, as a fraction.
const STEADY_SIZE: f32 = 0.04;

/// Watches someone getting into a step's pose, so a step can start measuring once they're there
/// rather than after a fixed wait.
#[derive(Clone, Debug)]
pub struct Arrival {
    pose: Pose,
    upright: Option<Posture>,
    limits: Thresholds,
    recent: Vec<Posture>,
}

impl Arrival {
    /// `upright` is how they sat in an earlier upright step, if there was one yet; without it,
    /// sitting still is enough.
    pub fn new(pose: Pose, upright: Option<Posture>, limits: Thresholds) -> Self {
        Self {
            pose,
            upright,
            limits,
            recent: Vec::new(),
        }
    }

    /// Note the latest frame's posture, or `None` with no face, and say whether they're in the
    /// pose and keeping still.
    pub fn arrived(&mut self, posture: Option<Posture>) -> bool {
        let Some(posture) = posture else {
            self.recent.clear();
            return false;
        };
        self.recent.push(posture);
        if self.recent.len() > STEADY_FRAMES {
            self.recent.remove(0);
        }
        self.steady() && self.in_pose(posture)
    }

    fn steady(&self) -> bool {
        if self.recent.len() < STEADY_FRAMES {
            return false;
        }
        let spread = |value: fn(&Posture) -> f32| {
            let values = self.recent.iter().map(value);
            values.clone().fold(f32::MIN, f32::max) - values.fold(f32::MAX, f32::min)
        };
        let size = self.recent.iter().map(|p| p.size).sum::<f32>() / self.recent.len() as f32;
        spread(|p| p.eye_y) < STEADY_EYES * size && spread(|p| p.size) < STEADY_SIZE * size
    }

    fn in_pose(&self, posture: Posture) -> bool {
        let Some(upright) = self.upright else {
            return true;
        };
        let reading = Reading::new(posture, upright);
        let moved = |value: f32, limit: f32| value >= MOVED_SHARE * limit;
        match self.pose {
            Pose::Upright => {
                !moved(reading.drop, self.limits.drop) && !moved(reading.lean, self.limits.lean)
            }
            Pose::Slump => moved(reading.drop, self.limits.drop),
            Pose::Lean => moved(reading.lean, self.limits.lean),
        }
    }
}

/// How someone sat across the frames of upright steps, as the middle of each measure.
pub fn typical(postures: &[Posture]) -> Option<Posture> {
    (!postures.is_empty()).then(|| Posture {
        eye_y: median(postures.iter().map(|p| p.eye_y).collect()),
        size: median(postures.iter().map(|p| p.size).collect()),
    })
}

/// Work out a baseline and thresholds from a played game, or `None` if a step saw too little of you.
pub fn score_game(steps: &[Step], samples: &[Sample]) -> Option<GameResult> {
    let by_step: Vec<Vec<Posture>> = (1..=steps.len())
        .map(|n| {
            samples
                .iter()
                .filter(|s| s.step == n)
                .map(|s| s.posture)
                .collect()
        })
        .collect();
    if by_step.iter().any(|postures| postures.len() < MIN_SAMPLES) {
        return None;
    }
    // Every kind of pose is needed: upright for the baseline, the others to measure slouching.
    if [Pose::Upright, Pose::Slump, Pose::Lean]
        .iter()
        .any(|pose| steps.iter().all(|s| s.pose != *pose))
    {
        return None;
    }
    let pose_steps = |pose: Pose| {
        steps
            .iter()
            .zip(&by_step)
            .filter(move |(s, _)| s.pose == pose)
            .map(|(_, p)| p)
    };
    let upright: Vec<Posture> = pose_steps(Pose::Upright).flatten().copied().collect();
    let baseline = Posture {
        eye_y: median(upright.iter().map(|p| p.eye_y).collect()),
        size: median(upright.iter().map(|p| p.size).collect()),
    };

    let readings = |postures: &[Posture]| -> Vec<Reading> {
        postures
            .iter()
            .map(|&p| Reading::new(p, baseline))
            .collect()
    };
    let metric = |value: fn(&Reading) -> f32| {
        let upright_steps: Vec<Vec<f32>> = pose_steps(Pose::Upright)
            .map(|p| readings(p).iter().map(value).collect())
            .collect();
        let upright_max = upright_steps
            .iter()
            .map(|v| median(v.clone()))
            .fold(f32::MIN, f32::max);
        let jitter =
            upright_steps.iter().map(|v| std_dev(v)).sum::<f32>() / upright_steps.len() as f32;
        let pose_median = |pose| {
            median(
                pose_steps(pose)
                    .flat_map(|p| readings(p))
                    .map(|r| value(&r))
                    .collect(),
            )
        };
        let (slump, lean) = (pose_median(Pose::Slump), pose_median(Pose::Lean));
        let moved = slump.max(lean);
        let usable = moved - upright_max > MIN_SEPARATION * jitter;
        MetricResult {
            jitter,
            upright_max,
            slump,
            lean,
            threshold: usable.then_some((upright_max + moved) / 2.0),
        }
    };
    Some(GameResult {
        baseline,
        drop: metric(|r| r.drop),
        lean: metric(|r| r.lean),
    })
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn std_dev(values: &[f32]) -> f32 {
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Poses resembling the first real game: three screens, where looking down at the laptop
    /// moves the eyes and must not count as slouching.
    fn play(steps: &[Step]) -> Vec<Sample> {
        let mut samples = Vec::new();
        let mut wobble = 0.0f32;
        for (n, step) in steps.iter().enumerate() {
            let (eye_y, size) = match (step.pose, step.screen) {
                (Pose::Upright, Some(2)) => (235.0, 72.0),
                (Pose::Upright, Some(1)) => (222.0, 75.0),
                (Pose::Upright, _) => (220.0, 74.0),
                (Pose::Slump, _) => (300.0, 75.0),
                (Pose::Lean, _) => (290.0, 104.0),
            };
            for _ in 0..30 {
                wobble = (wobble + 0.37) % 1.0;
                let posture = Posture {
                    eye_y: eye_y + 2.0 * (wobble - 0.5),
                    size: size + 0.8 * (wobble - 0.5),
                };
                samples.push(Sample {
                    step: n + 1,
                    pose: step.pose,
                    posture,
                });
            }
        }
        samples
    }

    #[test]
    fn thresholds_sit_between_every_upright_screen_and_slouching() {
        let screens = [
            "bottom-middle screen",
            "top-right screen",
            "top-left screen",
        ]
        .map(String::from);
        let steps = game_steps(&screens);
        assert_eq!(steps.len(), 6);
        let result = score_game(&steps, &play(&steps)).expect("enough samples");
        let drop = result.drop.threshold.expect("drop separates");
        let lean = result.lean.threshold.expect("lean separates");
        assert!(result.drop.upright_max < drop && drop < result.drop.slump);
        assert!(result.lean.upright_max < lean && lean < result.lean.lean);
    }

    #[test]
    fn games_missing_a_kind_of_pose_score_nothing() {
        assert!(score_game(&[], &[]).is_none());
        let upright_only: Vec<Step> = game_steps(&[])
            .into_iter()
            .filter(|s| s.pose == Pose::Upright)
            .collect();
        assert!(score_game(&upright_only, &play(&upright_only)).is_none());
    }

    fn still(eye_y: f32, size: f32) -> Option<Posture> {
        Some(Posture { eye_y, size })
    }

    #[test]
    fn arriving_needs_the_pose_and_a_moment_of_keeping_still() {
        let upright = Posture {
            eye_y: 220.0,
            size: 74.0,
        };
        let mut slump = Arrival::new(Pose::Slump, Some(upright), Thresholds::default());
        assert!(
            !(0..10).any(|_| slump.arrived(still(222.0, 74.0))),
            "still upright"
        );
        let arrived: Vec<bool> = (0..6).map(|_| slump.arrived(still(270.0, 74.0))).collect();
        assert_eq!(arrived, [false, false, false, false, false, true]);
        assert!(!slump.arrived(None), "out of frame");

        let mut lean = Arrival::new(Pose::Lean, Some(upright), Thresholds::default());
        assert!(
            !(0..10).any(|_| lean.arrived(still(270.0, 74.0))),
            "sunk, not leaning"
        );
        assert!(
            (0..6)
                .map(|_| lean.arrived(still(240.0, 90.0)))
                .last()
                .unwrap()
        );

        let mut back = Arrival::new(Pose::Upright, Some(upright), Thresholds::default());
        assert!(!(0..10).any(|_| back.arrived(still(270.0, 74.0))));
        assert!(
            (0..6)
                .map(|_| back.arrived(still(221.0, 74.0)))
                .last()
                .unwrap()
        );
    }

    #[test]
    fn arriving_waits_for_a_wobbling_head_to_settle() {
        let mut first = Arrival::new(Pose::Upright, None, Thresholds::default());
        let wobbling = [220.0, 232.0, 218.0, 235.0, 221.0, 230.0, 219.0];
        assert!(!wobbling.iter().any(|&y| first.arrived(still(y, 74.0))));
        assert!(
            (0..6)
                .map(|_| first.arrived(still(220.0, 74.0)))
                .last()
                .unwrap()
        );
    }

    #[test]
    fn a_step_without_a_face_spoils_the_game() {
        let steps = game_steps(&[]);
        let samples: Vec<Sample> = play(&steps).into_iter().filter(|s| s.step != 2).collect();
        assert!(score_game(&steps, &samples).is_none());
    }
}

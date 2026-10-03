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
    let mut steps: Vec<Step> = if screens.len() < 2 {
        vec![step(Pose::Upright, "Sit up straight", None)]
    } else {
        screens
            .iter()
            .enumerate()
            .map(|(i, name)| {
                step(
                    Pose::Upright,
                    &format!("Sit up straight, look at the {name}"),
                    Some(i),
                )
            })
            .collect()
    };
    steps.extend([
        step(Pose::Slump, "Slouch down, don't lean forward", None),
        step(Pose::Upright, "Sit up straight again", None),
        step(Pose::Lean, "Slouch leaning towards the screen", None),
    ]);
    steps
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

    #[test]
    fn a_step_without_a_face_spoils_the_game() {
        let steps = game_steps(&[]);
        let samples: Vec<Sample> = play(&steps).into_iter().filter(|s| s.step != 2).collect();
        assert!(score_game(&steps, &samples).is_none());
    }
}

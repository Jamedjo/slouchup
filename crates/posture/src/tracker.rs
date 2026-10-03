use crate::{Posture, Problem, Reading, Thresholds};

/// Seconds for the baseline to move most of the way towards how you are sitting now. Improvements
/// are adopted quickly so the bar rises with you; drifts in the slouchy direction (while still
/// within the thresholds) are followed slowly so gradual slouching isn't absorbed.
const ADAPT_BETTER: f32 = 180.0;
const ADAPT_WORSE: f32 = 1800.0;
const SMOOTHING: f32 = 0.6;
/// Faces flicker out for a frame or two; shorter gaps than this change nothing.
const FACE_BLINK: f64 = 1.5;
const FACE_LOST_GIVE_UP: f64 = 120.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub thresholds: Thresholds,
    /// Seconds of slouching before nagging.
    pub grace: f64,
    /// Minimum seconds between nags for separate slouches.
    pub min_gap: f64,
    /// Seconds between repeat nags during one long slouch.
    pub cooldown: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            thresholds: Thresholds::default(),
            grace: 1.5,
            min_gap: 10.0,
            cooldown: 60.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Good,
    Bad(Problem),
    /// No face in view, and nothing suggests that's a slouch.
    Away,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Nag(Problem),
    /// Posture recovered after a nag.
    Dismiss,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Update {
    pub state: State,
    pub reading: Option<Reading>,
    pub action: Option<Action>,
}

pub struct Tracker {
    pub settings: Settings,
    baseline: Posture,
    smoothed: Option<Posture>,
    last_tick: Option<f64>,
    last_seen: Option<f64>,
    last_ratio: f32,
    slouch_since: Option<f64>,
    last_nag: Option<f64>,
    nagged_this_slouch: bool,
}

impl Tracker {
    pub fn new(baseline: Posture, settings: Settings) -> Self {
        Self {
            settings,
            baseline,
            smoothed: None,
            last_tick: None,
            last_seen: None,
            last_ratio: 0.0,
            slouch_since: None,
            last_nag: None,
            nagged_this_slouch: false,
        }
    }

    pub fn baseline(&self) -> Posture {
        self.baseline
    }

    /// Feed one frame. `now` is in seconds from any fixed origin. Returns `None` while a face is
    /// only briefly missing, when nothing should change.
    pub fn update(&mut self, now: f64, posture: Option<Posture>) -> Option<Update> {
        let dt = self.last_tick.map_or(0.0, |t| (now - t) as f32);
        self.last_tick = Some(now);
        let since_seen = self.last_seen.map_or(f64::INFINITY, |t| now - t);
        if posture.is_none() && since_seen < FACE_BLINK {
            return None;
        }

        let mut reading = None;
        let problem = match posture {
            Some(posture) => {
                self.last_seen = Some(now);
                let smoothed = match self.smoothed {
                    None => posture,
                    Some(s) => s.lerp(posture, 1.0 - (-dt / SMOOTHING).exp()),
                };
                self.smoothed = Some(smoothed);
                let current = Reading::new(smoothed, self.baseline);
                reading = Some(current);
                let (ratio, problem) = current.worst(self.settings.thresholds);
                self.last_ratio = ratio;
                if ratio > 1.0 {
                    Some(problem)
                } else {
                    self.adapt(smoothed, dt);
                    None
                }
            }
            // Faces vanish when you hunch over the keyboard, so losing one mid-slouch counts.
            None if since_seen < FACE_LOST_GIVE_UP && self.last_ratio > 0.5 => {
                Some(Problem::DroppedOutOfView)
            }
            None => {
                self.smoothed = None;
                None
            }
        };

        let (state, action) = match problem {
            None => {
                self.slouch_since = None;
                let action =
                    std::mem::take(&mut self.nagged_this_slouch).then_some(Action::Dismiss);
                (
                    if posture.is_some() {
                        State::Good
                    } else {
                        State::Away
                    },
                    action,
                )
            }
            Some(problem) => (State::Bad(problem), self.maybe_nag(now, problem)),
        };
        Some(Update {
            state,
            reading,
            action,
        })
    }

    fn maybe_nag(&mut self, now: f64, problem: Problem) -> Option<Action> {
        let since = *self.slouch_since.get_or_insert(now);
        // Each new slouch nags promptly so the habit gets caught; one long slouch repeats slowly.
        let gap = if self.nagged_this_slouch {
            self.settings.cooldown
        } else {
            self.settings.min_gap
        };
        let rested = self.last_nag.is_none_or(|t| now - t >= gap);
        if now - since >= self.settings.grace && rested {
            self.last_nag = Some(now);
            self.nagged_this_slouch = true;
            return Some(Action::Nag(problem));
        }
        None
    }

    /// Drift the baseline towards how you are sitting now; lower is better for every metric.
    fn adapt(&mut self, posture: Posture, dt: f32) {
        let step = |base: f32, now: f32| {
            let tau = if now < base {
                ADAPT_BETTER
            } else {
                ADAPT_WORSE
            };
            base + (1.0 - (-dt / tau).exp()) * (now - base)
        };
        self.baseline = Posture {
            eye_y: step(self.baseline.eye_y, posture.eye_y),
            size: step(self.baseline.size, posture.size),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UPRIGHT: Posture = Posture {
        eye_y: 220.0,
        size: 74.0,
    };
    const SLUMPED: Posture = Posture {
        eye_y: 300.0,
        size: 74.0,
    };

    fn run(
        tracker: &mut Tracker,
        from: f64,
        to: f64,
        posture: Option<Posture>,
    ) -> Vec<(f64, Update)> {
        let mut updates = Vec::new();
        let mut now = from;
        while now < to {
            if let Some(update) = tracker.update(now, posture) {
                updates.push((now, update));
            }
            now += 0.2;
        }
        updates
    }

    fn nags(updates: &[(f64, Update)]) -> Vec<f64> {
        updates
            .iter()
            .filter(|(_, u)| matches!(u.action, Some(Action::Nag(_))))
            .map(|(t, _)| *t)
            .collect()
    }

    #[test]
    fn nags_about_two_seconds_into_a_slouch_and_dismisses_on_recovery() {
        let mut tracker = Tracker::new(UPRIGHT, Settings::default());
        run(&mut tracker, 0.0, 5.0, Some(UPRIGHT));
        let slouch = run(&mut tracker, 5.0, 10.0, Some(SLUMPED));
        let first = nags(&slouch);
        assert_eq!(first.len(), 1);
        assert!(first[0] - 5.0 < 2.5, "nagged {:.1}s in", first[0] - 5.0);
        let recover = run(&mut tracker, 10.0, 13.0, Some(UPRIGHT));
        assert!(
            recover
                .iter()
                .any(|(_, u)| u.action == Some(Action::Dismiss))
        );
    }

    #[test]
    fn a_new_slouch_nags_after_the_short_gap_but_a_long_one_repeats_slowly() {
        let mut tracker = Tracker::new(UPRIGHT, Settings::default());
        let mut all = run(&mut tracker, 0.0, 5.0, Some(SLUMPED));
        all.extend(run(&mut tracker, 5.0, 8.0, Some(UPRIGHT)));
        all.extend(run(&mut tracker, 20.0, 25.0, Some(SLUMPED)));
        assert_eq!(nags(&all).len(), 2, "separate slouches each nag");
        let long = run(&mut tracker, 25.0, 80.0, Some(SLUMPED));
        assert!(nags(&long).is_empty(), "no repeat inside the cooldown");
        assert_eq!(nags(&run(&mut tracker, 80.0, 90.0, Some(SLUMPED))).len(), 1);
    }

    #[test]
    fn brief_face_loss_changes_nothing_but_losing_it_mid_slouch_counts() {
        let mut tracker = Tracker::new(UPRIGHT, Settings::default());
        run(&mut tracker, 0.0, 3.0, Some(UPRIGHT));
        assert!(tracker.update(3.2, None).is_none());
        let away = run(&mut tracker, 3.2, 6.0, None);
        assert_eq!(away.last().unwrap().1.state, State::Away);

        let mut tracker = Tracker::new(UPRIGHT, Settings::default());
        run(&mut tracker, 0.0, 3.0, Some(SLUMPED));
        let gone = run(&mut tracker, 3.0, 6.0, None);
        assert_eq!(
            gone.last().unwrap().1.state,
            State::Bad(Problem::DroppedOutOfView)
        );
    }

    #[test]
    fn baseline_follows_better_posture_faster_than_worse() {
        let higher = Posture {
            eye_y: 210.0,
            ..UPRIGHT
        };
        let lower = Posture {
            eye_y: 230.0,
            ..UPRIGHT
        };
        let mut up = Tracker::new(UPRIGHT, Settings::default());
        let mut down = Tracker::new(UPRIGHT, Settings::default());
        run(&mut up, 0.0, 60.0, Some(higher));
        run(&mut down, 0.0, 60.0, Some(lower));
        let moved_up = UPRIGHT.eye_y - up.baseline().eye_y;
        let moved_down = down.baseline().eye_y - UPRIGHT.eye_y;
        assert!(
            moved_up > 5.0 * moved_down,
            "up {moved_up:.2} vs down {moved_down:.2}"
        );
    }
}

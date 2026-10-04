//! How you've been sitting, minute by minute, for the history chart. Two weeks are kept.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Local, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

const KEPT_MINUTES: i64 = 14 * 24 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sitting {
    Well,
    Slouching,
    Away,
}

/// Checks of each kind, and nudges sent, over some stretch of time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    pub well: u32,
    pub slouching: u32,
    pub away: u32,
    pub nudges: u32,
}

impl Tally {
    /// Checks where you were in front of the camera.
    pub fn present(&self) -> u32 {
        self.well + self.slouching
    }

    pub fn checks(&self) -> u32 {
        self.present() + self.away
    }

    /// Share of the time you were there that you spent slouching, from 0 to 1.
    pub fn slouching_share(&self) -> f32 {
        match self.present() {
            0 => 0.0,
            present => self.slouching as f32 / present as f32,
        }
    }

    fn add(&mut self, other: &Tally) {
        self.well += other.well;
        self.slouching += other.slouching;
        self.away += other.away;
        self.nudges += other.nudges;
    }
}

/// Tallies keyed by minute since the Unix epoch.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    minutes: BTreeMap<i64, Tally>,
}

fn minute(at: DateTime<Local>) -> i64 {
    at.timestamp().div_euclid(60)
}

impl History {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("tallies serialise")
    }

    pub fn record(&mut self, at: DateTime<Local>, sitting: Sitting) {
        let tally = self.minutes.entry(minute(at)).or_default();
        match sitting {
            Sitting::Well => tally.well += 1,
            Sitting::Slouching => tally.slouching += 1,
            Sitting::Away => tally.away += 1,
        }
    }

    pub fn nudged(&mut self, at: DateTime<Local>) {
        self.minutes.entry(minute(at)).or_default().nudges += 1;
    }

    pub fn clear(&mut self) {
        self.minutes.clear();
    }

    /// Forget anything older than two weeks before `now`.
    pub fn prune(&mut self, now: DateTime<Local>) {
        self.minutes = self.minutes.split_off(&(minute(now) - KEPT_MINUTES));
    }

    /// Everything from `from` up to, but not including, `to`.
    pub fn tally(&self, from: DateTime<Local>, to: DateTime<Local>) -> Tally {
        let mut total = Tally::default();
        for (_, tally) in self.minutes.range(minute(from)..minute(to)) {
            total.add(tally);
        }
        total
    }

    /// `slots` equal slices of the day so far, from midnight to the end of the current slot.
    pub fn today(&self, now: DateTime<Local>, slot_minutes: i64) -> Vec<(DateTime<Local>, Tally)> {
        let start = midnight(now);
        let step = chrono::Duration::minutes(slot_minutes);
        let mut slots = Vec::new();
        let mut at = start;
        while at <= now {
            slots.push((at, self.tally(at, at + step)));
            at += step;
        }
        slots
    }

    /// A tally per day for the `days` days ending today, oldest first.
    pub fn days(&self, now: DateTime<Local>, days: u32) -> Vec<(DateTime<Local>, Tally)> {
        let today = midnight(now);
        (0..days as i64)
            .rev()
            .map(|back| {
                let start = midnight(today - chrono::Duration::days(back));
                let end = midnight(start + chrono::Duration::hours(36));
                (start, self.tally(start, end))
            })
            .collect()
    }
}

/// The start of `at`'s day, local time. Midnight can be skipped by a clock change, so take the
/// earliest time that exists that day.
pub fn midnight(at: DateTime<Local>) -> DateTime<Local> {
    let date = at.date_naive();
    (0..24)
        .find_map(|hour| {
            Local
                .from_local_datetime(&date.and_time(NaiveTime::from_hms_opt(hour, 0, 0)?))
                .earliest()
        })
        .unwrap_or(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: u32, minute: u32) -> DateTime<Local> {
        let date = Local::now().date_naive();
        Local
            .from_local_datetime(&date.and_hms_opt(hour, minute, 0).unwrap())
            .earliest()
            .unwrap()
    }

    #[test]
    fn slots_add_up_each_minute_in_them() {
        let mut history = History::default();
        history.record(at(9, 1), Sitting::Well);
        history.record(at(9, 14), Sitting::Slouching);
        history.record(at(9, 16), Sitting::Away);
        history.nudged(at(9, 14));
        let slots = history.today(at(9, 20), 15);
        let nine = slots
            .iter()
            .find(|(start, _)| *start == at(9, 0))
            .unwrap()
            .1;
        assert_eq!(
            nine,
            Tally {
                well: 1,
                slouching: 1,
                away: 0,
                nudges: 1
            }
        );
        assert_eq!(nine.slouching_share(), 0.5);
        assert_eq!(slots.last().unwrap().1.away, 1);
    }

    #[test]
    fn days_split_at_midnight_and_old_minutes_are_pruned() {
        let mut history = History::default();
        let now = at(12, 0);
        history.record(now - chrono::Duration::days(1), Sitting::Slouching);
        history.record(now, Sitting::Well);
        history.record(now - chrono::Duration::days(20), Sitting::Well);
        history.prune(now);
        let days = history.days(now, 7);
        assert_eq!(days.len(), 7);
        assert_eq!(days[5].1.slouching, 1, "yesterday");
        assert_eq!(days[6].1.well, 1, "today");
        assert_eq!(
            history.tally(now - chrono::Duration::days(30), now).well,
            0,
            "pruned"
        );
    }

    #[test]
    fn clearing_forgets_everything() {
        let mut history = History::default();
        history.record(at(8, 0), Sitting::Slouching);
        history.clear();
        assert_eq!(history, History::default());
    }

    #[test]
    fn survives_a_round_trip_through_json() {
        let mut history = History::default();
        history.record(at(8, 0), Sitting::Well);
        let back: History = serde_json::from_str(&history.to_json()).unwrap();
        assert_eq!(back, history);
    }
}

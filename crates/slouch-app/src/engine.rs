//! The detection loop: camera frames in, posture decisions, notifications and UI snapshots out.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use camera_drift::{CameraDrift, Grey, Rect};
use crossbeam_channel::Receiver;
use futures_channel::mpsc::UnboundedSender;
use posture::{
    Action, Arrival, Pose, Posture, Reading, Sample, Settings, State, Step, Thresholds, Tracker,
};
use serde::Serialize;

use crate::art::{Files, Mood};
use crate::camera_view::FrameSlot;
use crate::config::{self, APP_NAME};
use crate::finder::FaceFinder;
use crate::frames;
use crate::history::{History, Sitting};
use crate::notifier::{Notifier, OnPause};
use crate::source::{Capture, Source};

const WATCH_INTERVAL: Duration = Duration::from_millis(200);
const BUSY_INTERVAL: Duration = Duration::from_millis(100);
const CALIBRATION: Duration = Duration::from_secs(3);
/// Long enough to read a step's instructions before it can start measuring.
const READY_AT_LEAST: Duration = Duration::from_millis(2000);
/// When a step starts measuring anyway, for someone who moved too little to notice.
const READY_AT_MOST: Duration = Duration::from_secs(8);
const GAME_RECORD: Duration = Duration::from_secs(4);
const RETRY_CALIBRATION: Duration = Duration::from_secs(5);
/// No frame for this long means the camera has gone, say unplugged.
const CAMERA_LOST: Duration = Duration::from_secs(4);
const RETRY_CAMERA: Duration = Duration::from_secs(5);
/// Laptop lids tilt rarely, so the background needn't be checked every frame.
const DRIFT_INTERVAL: Duration = Duration::from_secs(1);
const OUT_OF_FRAME: &str = "You're out of frame.";
/// How long the nudge's Pause button turns the camera off for.
pub const NUDGE_PAUSE: Duration = Duration::from_secs(30 * 60);
/// The ways to pause, as menus offer them: a length, or `None` until you resume.
pub const PAUSES: [(&str, Option<Duration>); 3] = [
    ("Pause for 30 minutes", Some(NUDGE_PAUSE)),
    ("Pause for 1 hour", Some(Duration::from_secs(60 * 60))),
    ("Pause until I resume", None),
];

pub enum Command {
    Recalibrate,
    Game {
        screens: Vec<String>,
    },
    /// Turn the camera off, for a while or until [`Command::Resume`].
    Pause(Option<Duration>),
    Resume,
    /// A button or key pressed in the guided calibration's full-screen window.
    Guided(Answer),
    /// Close SlouchUp's own nudge card, answered.
    CloseCard,
    Change(Change),
    /// Switch camera, by device id or `None` for the first that works.
    UseCamera(Option<String>),
    ClearHistory,
    /// Save what needs saving, then end the app.
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Answer {
    /// Next, Start or Done: on to whatever comes next.
    Next,
    /// Do it again, or Try again, after the results.
    Again,
    /// Esc or Not now: stop, keeping the calibration from before.
    Cancel,
}

/// One setting changed in the settings window. Sent singly, so a window opened before a
/// calibration game can't put back the limits the game replaced.
pub enum Change {
    Drop(f32),
    Lean(f32),
    Grace(f64),
    MinGap(f64),
    Cooldown(f64),
    KeepHistory(bool),
    /// The nudge timings back to their defaults. The limits go back with `Drop` and `Lean`.
    NudgeDefaults,
}

/// What the UI draws. Positions are in camera-frame pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub mood: Mood,
    pub status: String,
    pub frame_size: (f32, f32),
    pub reading: Option<Reading>,
    pub settings: Settings,
    /// The limits the last calibration game set.
    pub calibrated: Thresholds,
    /// Where the baseline eye line and the slouch line currently fall.
    pub lines: Option<(f32, f32)>,
    pub banner: Option<Banner>,
    /// What the calibration game is asking for right now.
    pub look_at: Option<LookAt>,
    pub paused: bool,
    /// A nudge's reason, shown in SlouchUp's own card because notifications can't show.
    pub card: Option<String>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            mood: Mood::Idle,
            status: "Starting…".into(),
            frame_size: (640.0, 480.0),
            reading: None,
            settings: Settings::default(),
            calibrated: Thresholds::default(),
            lines: None,
            banner: None,
            look_at: None,
            paused: false,
            card: None,
        }
    }
}

/// Where the guided calibration is, for its full-screen window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LookAt {
    /// The screen to look at, or `None` for the screen the camera window is on.
    pub screen: Option<usize>,
    pub phase: Phase,
    /// The step, counting from 1, and how many there are.
    pub step: usize,
    pub steps: usize,
    pub pose: Option<Pose>,
    pub prompt: String,
    /// What the prompt means, said while getting ready.
    pub detail: String,
    /// How far the Hold has got, from 0 to 1.
    pub progress: f32,
    pub in_frame: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Phase {
    /// The first step of each pose, the first time through: its drawing and what to do, until
    /// Next.
    #[default]
    Teach,
    /// Instructions for a step, until you're in its pose.
    Ready,
    /// Measuring, with nothing to read.
    Hold,
    /// Calibrated, with the limits that came out of it.
    Done(Thresholds),
    /// A step saw too little of you; names that step.
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Banner {
    pub title: String,
    pub lines: Vec<String>,
    pub progress: Option<f32>,
}

/// One processed frame, at detection resolution.
struct Observation {
    grey: Vec<u8>,
    width: usize,
    height: usize,
    /// Detection pixels to camera-frame pixels.
    scale: f32,
    face: Option<yunet::Face>,
}

impl Observation {
    fn grey(&self) -> Grey<'_> {
        Grey::new(&self.grey, self.width, self.height).expect("grey frames match their size")
    }

    fn person(&self) -> Option<Rect> {
        self.face.map(|f| Rect {
            x: f.x,
            y: f.y,
            width: f.width,
            height: f.height,
        })
    }

    fn posture(&self, drift: f32) -> Option<Posture> {
        let l = self.face?.landmarks;
        Some(Posture::from_landmarks(
            l.right_eye,
            l.left_eye,
            l.right_mouth,
            l.left_mouth,
            drift * self.height as f32,
        ))
    }
}

/// A recorded game frame, with everything needed to re-score it differently later.
#[derive(Serialize)]
struct GameRecord {
    #[serde(flatten)]
    sample: Sample,
    screen: Option<usize>,
    t: f32,
    drift: f32,
    face: [f32; 4],
    points: [[f32; 2]; 5],
}

pub struct Engine {
    source: Source,
    capture: Option<Capture>,
    /// When the last frame arrived, to notice a camera that has gone.
    last_frame: Instant,
    reopen_at: Option<Instant>,
    told_no_camera: bool,
    /// Commands that arrived while calibrating or playing the game.
    pending: VecDeque<Command>,
    preview: FrameSlot,
    persist: bool,
    finder: FaceFinder,
    notifier: Notifier,
    events: UnboundedSender<View>,
    commands: Receiver<Command>,
    settings: Settings,
    calibrated: Thresholds,
    tracker: Option<Tracker>,
    drift: Option<CameraDrift>,
    drift_checked: Option<Instant>,
    view: View,
    started: Instant,
    paused: bool,
    /// When a pause for a while ends, by the wall clock, so time asleep counts towards it.
    resume_at: Option<chrono::DateTime<chrono::Local>>,
    history: SharedHistory,
    history_saved: Instant,
    /// Off when the user has asked for no history to be kept.
    recording: bool,
    /// Whether a guided calibration has finished before, so its intro can be skipped.
    guided_before: bool,
}

/// The posture history, shared with the history window.
pub type SharedHistory = std::sync::Arc<std::sync::Mutex<History>>;
/// How often the history is written to disk, besides on pause and quit. A crash loses at most
/// this much.
const HISTORY_SAVE_EVERY: Duration = Duration::from_secs(10 * 60);

/// How the engine talks to the rest of the app.
pub struct Links {
    pub preview: FrameSlot,
    pub files: Files,
    pub on_pause: OnPause,
    pub history: SharedHistory,
    pub events: UnboundedSender<View>,
    pub commands: Receiver<Command>,
}

impl Engine {
    /// Run the engine on its own thread. The camera opens there too, so a slow or refused
    /// camera never holds up the UI, and a missing one is retried.
    pub fn start(source: Source, settings: Settings, links: Links) {
        let Links {
            preview,
            files,
            on_pause,
            history,
            events,
            commands,
        } = links;
        // A demo is for looking at, so it mustn't overwrite the real calibration.
        let persist = !matches!(source, Source::Demo);
        let preferences = config::load_preferences();
        // Limits saved before calibrations were kept apart most likely came from a game.
        let calibrated = persist
            .then_some(preferences.calibrated)
            .flatten()
            .unwrap_or(settings.thresholds);
        let crashed = (
            Notifier::new(files.clone(), on_pause.clone()),
            events.clone(),
        );
        let engine = Engine {
            source,
            capture: None,
            last_frame: Instant::now(),
            reopen_at: None,
            told_no_camera: false,
            pending: VecDeque::new(),
            persist,
            finder: FaceFinder::new(),
            notifier: Notifier::new(files, on_pause),
            events,
            commands,
            settings,
            calibrated,
            preview,
            tracker: None,
            drift: None,
            drift_checked: None,
            view: View::default(),
            started: Instant::now(),
            paused: false,
            resume_at: None,
            history,
            history_saved: Instant::now(),
            recording: !persist || preferences.keep_history,
            guided_before: persist && preferences.guided_before,
        };
        std::thread::Builder::new()
            .name("slouch-engine".into())
            .spawn(move || {
                let run = std::panic::AssertUnwindSafe(|| engine.run());
                if std::panic::catch_unwind(run).is_err() {
                    let (notifier, events) = crashed;
                    notifier.info(
                        &format!("{APP_NAME} stopped"),
                        "Something went wrong. Restart it to carry on.",
                    );
                    let _ = events.unbounded_send(View {
                        status: "Stopped after an error. Restart to carry on.".into(),
                        ..View::default()
                    });
                }
            })
            .expect("spawning the engine thread");
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn publish(&mut self) {
        self.view.settings = self.settings;
        self.view.calibrated = self.calibrated;
        self.view.paused = self.paused;
        let _ = self.events.unbounded_send(self.view.clone());
    }

    fn set_status(&mut self, mood: Mood, status: impl Into<String>) {
        let status = status.into();
        if self.view.status != status && (self.view.mood != mood || !status.starts_with("You're")) {
            tracing::info!("{mood:?}: {status}");
        }
        self.view.mood = mood;
        self.view.status = status;
    }

    fn run(mut self) {
        let mut announce = true;
        loop {
            while let Some(command) = self
                .pending
                .pop_front()
                .or_else(|| self.commands.try_recv().ok())
            {
                self.handle(command, &mut announce);
            }
            if self.paused {
                if self.resume_at.is_some_and(|at| chrono::Local::now() >= at) {
                    self.resume();
                    self.notifier
                        .info(&format!("{APP_NAME} is back on"), "Your pause is up.");
                    continue;
                }
                let status = match self.resume_at {
                    Some(at) => format!("Paused until {}", at.format("%H:%M")),
                    None => "Paused".into(),
                };
                self.set_status(Mood::Paused, status);
                self.publish();
                std::thread::sleep(WATCH_INTERVAL);
                continue;
            }
            if !self.camera_ready() {
                std::thread::sleep(WATCH_INTERVAL);
                continue;
            }
            if self.tracker.is_none() {
                if !self.calibrate(announce) {
                    self.set_status(
                        Mood::Idle,
                        "You were out of frame; calibrating again shortly",
                    );
                    self.publish();
                    std::thread::sleep(RETRY_CALIBRATION);
                }
                announce = false;
                continue;
            }
            std::thread::sleep(WATCH_INTERVAL);
            self.watch();
        }
    }

    fn handle(&mut self, command: Command, announce: &mut bool) {
        match command {
            Command::Recalibrate => {
                self.tracker = None;
                *announce = true;
            }
            Command::Game { screens } => {
                if self.paused || !self.camera_ready() {
                    self.notifier.info(
                        "Guided calibration",
                        &format!("It needs the camera, so resume {APP_NAME} first."),
                    );
                } else if !self.play_game(&screens) && self.tracker.is_none() {
                    *announce = true;
                }
            }
            Command::Pause(length) => {
                self.resume_at = length.map(|length| {
                    chrono::Local::now()
                        + chrono::Duration::from_std(length).expect("pauses are short")
                });
                if !std::mem::replace(&mut self.paused, true) {
                    self.save_history();
                    // Dropping the capture releases the camera, so its light goes off.
                    self.capture = None;
                    self.notifier.dismiss();
                    self.view.card = None;
                }
            }
            Command::Resume => self.resume(),
            Command::CloseCard => {
                self.view.card = None;
                self.publish();
            }
            Command::Change(change) => self.change(change),
            // Left over from a guided calibration that has already ended.
            Command::Guided(_) => {}
            Command::ClearHistory => {
                self.history.lock().unwrap().clear();
                if self.persist
                    && let Err(error) = config::delete_history()
                {
                    tracing::warn!("couldn't delete history: {error}");
                }
            }
            Command::Quit => {
                self.save_history();
                std::process::exit(0);
            }
            Command::UseCamera(id) => {
                let wanted = Source::Camera(id);
                if wanted == self.source && self.capture.is_some() {
                    return;
                }
                // Release the old camera before opening, in case it's the same device.
                self.capture = None;
                self.source = wanted;
                self.reopen_at = None;
                self.told_no_camera = false;
                // A different camera sees you from somewhere else, so start afresh.
                self.tracker = None;
                self.finder = FaceFinder::new();
                *announce = true;
                self.save_preferences();
            }
        }
    }

    fn resume(&mut self) {
        self.paused = false;
        self.resume_at = None;
        self.reopen_at = None;
    }

    fn change(&mut self, change: Change) {
        let settings = &mut self.settings;
        match change {
            Change::Drop(drop) => settings.thresholds.drop = drop,
            Change::Lean(lean) => settings.thresholds.lean = lean,
            Change::Grace(grace) => settings.grace = grace,
            Change::MinGap(gap) => settings.min_gap = gap,
            Change::Cooldown(cooldown) => settings.cooldown = cooldown,
            Change::KeepHistory(keep) => self.recording = keep,
            Change::NudgeDefaults => {
                let defaults = Settings::default();
                settings.grace = defaults.grace;
                settings.min_gap = defaults.min_gap;
                settings.cooldown = defaults.cooldown;
            }
        }
        if let Some(tracker) = &mut self.tracker {
            tracker.set_settings(self.settings);
        }
        self.save_thresholds(self.settings.thresholds);
        self.save_preferences();
    }

    fn save_thresholds(&self, thresholds: Thresholds) {
        if self.persist
            && let Err(error) = config::save_thresholds(thresholds)
        {
            tracing::warn!("couldn't save thresholds: {error}");
        }
    }

    fn save_preferences(&self) {
        let Source::Camera(camera) = &self.source else {
            return;
        };
        let preferences = config::Preferences {
            camera: camera.clone(),
            grace: self.settings.grace,
            min_gap: self.settings.min_gap,
            cooldown: self.settings.cooldown,
            keep_history: self.recording,
            calibrated: Some(self.calibrated),
            guided_before: self.guided_before,
            ..config::load_preferences()
        };
        if self.persist
            && let Err(error) = config::save_preferences(&preferences)
        {
            tracing::warn!("couldn't save settings: {error}");
        }
    }

    /// Open the camera if it isn't open, and notice if an open one has stopped sending frames.
    fn camera_ready(&mut self) -> bool {
        if self.capture.is_some() {
            if self.last_frame.elapsed() < CAMERA_LOST {
                return true;
            }
            tracing::warn!("no frames for {CAMERA_LOST:?}; reopening the camera");
            self.capture = None;
            self.set_status(Mood::Idle, "Camera lost; trying again…");
            self.reopen_at = Some(Instant::now() + RETRY_CAMERA);
            self.publish();
            return false;
        }
        if self.reopen_at.is_some_and(|at| Instant::now() < at) {
            return false;
        }
        match Capture::start(self.source.clone(), self.preview.clone()) {
            Ok(capture) => {
                self.capture = Some(capture);
                self.last_frame = Instant::now();
                self.told_no_camera = false;
                true
            }
            Err(error) => {
                tracing::warn!("{error}");
                self.set_status(Mood::Idle, format!("No camera: {error}"));
                self.publish();
                if !std::mem::replace(&mut self.told_no_camera, true) {
                    self.notifier.info("No camera found", &error);
                }
                self.reopen_at = Some(Instant::now() + RETRY_CAMERA);
                false
            }
        }
    }

    /// Collect commands that arrive mid-calibration, to wait their turn, and say whether one of
    /// them should cut it short.
    fn interrupted(&mut self) -> bool {
        while let Ok(command) = self.commands.try_recv() {
            self.pending.push_back(command);
        }
        self.stopping()
    }

    /// Whether a waiting command, such as Pause, should stop a calibration.
    fn stopping(&self) -> bool {
        self.pending
            .iter()
            .any(|c| matches!(c, Command::Pause(_) | Command::UseCamera(_) | Command::Quit))
    }

    fn act(&self, pose: Option<Pose>) {
        if let Some(capture) = &self.capture {
            capture.act(pose);
        }
    }

    /// Take the newest frame and find the face in it, at half resolution for speed.
    fn observe(&mut self) -> Option<Observation> {
        let frame = self.capture.as_ref()?.take()?;
        self.last_frame = Instant::now();
        let small = frames::half_size(&frame)?;
        let (width, height) = (small.width, small.height);
        let (frame_width, frame_height) = frame.size();
        self.view.frame_size = (frame_width as f32, frame_height as f32);
        let face = self.finder.find(&small.rgb, width, height);
        let grey = small.grey;
        Some(Observation {
            grey,
            width,
            height,
            scale: 2.0,
            face,
        })
    }

    fn watch(&mut self) {
        let Some(seen) = self.observe() else { return };
        let due = self
            .drift_checked
            .is_none_or(|at| at.elapsed() >= DRIFT_INTERVAL);
        let drift = match self.drift.as_mut() {
            Some(drift) if due => {
                self.drift_checked = Some(Instant::now());
                drift.update(seen.grey(), seen.person())
            }
            Some(drift) => drift.last(),
            None => 0.0,
        };
        let now = self.now();
        let tracker = self.tracker.as_mut().expect("calibrated");
        let update = tracker.update(now, seen.posture(drift));
        let baseline = tracker.baseline();
        let Some(update) = update else { return };

        let (mood, status, sitting) = match update.state {
            State::Good => (Mood::Good, "Sitting tall".to_string(), Sitting::Well),
            State::Bad(problem) => (Mood::Bad, problem.to_string(), Sitting::Slouching),
            State::Away => (Mood::Idle, OUT_OF_FRAME.to_string(), Sitting::Away),
        };
        self.set_status(mood, status);
        if self.recording {
            self.history
                .lock()
                .unwrap()
                .record(chrono::Local::now(), sitting);
        }
        match update.action {
            Some(Action::Nag(problem)) => {
                tracing::info!(
                    "nag: {problem} (camera drift {:.0}px)",
                    drift * seen.height as f32
                );
                let reason = problem.to_string();
                if !self.notifier.nag(&reason) {
                    self.view.card = Some(reason);
                }
                if self.recording {
                    self.history.lock().unwrap().nudged(chrono::Local::now());
                }
            }
            Some(Action::Dismiss) => {
                self.notifier.dismiss();
                self.view.card = None;
            }
            None => {}
        }
        let drift_px = drift * seen.height as f32;
        let line = (baseline.eye_y + drift_px) * seen.scale;
        let limit = line + self.settings.thresholds.drop * baseline.size * seen.scale;
        self.view.lines = Some((line, limit));
        self.view.reading = update.reading;
        self.publish();
        if self.history_saved.elapsed() >= HISTORY_SAVE_EVERY {
            self.save_history();
        }
    }

    fn save_history(&mut self) {
        if !self.persist || !self.recording {
            return;
        }
        self.history_saved = Instant::now();
        let json = {
            let mut history = self.history.lock().unwrap();
            history.prune(chrono::Local::now());
            history.to_json()
        };
        if let Err(error) = config::save_history(&json) {
            tracing::warn!("couldn't save history: {error}");
        }
    }

    fn calibrate(&mut self, announce: bool) -> bool {
        self.set_status(Mood::Idle, "Calibrating: sit up straight and hold it…");
        self.view.lines = None;
        self.view.reading = None;
        if announce {
            self.notifier.info(
                "Sit up nicely for 3 seconds",
                "This sets how you usually sit.",
            );
        }
        let mut postures = Vec::new();
        let mut reference = None;
        let end = Instant::now() + CALIBRATION;
        while Instant::now() < end {
            std::thread::sleep(BUSY_INTERVAL);
            if self.interrupted() {
                return false;
            }
            let Some(seen) = self.observe() else { continue };
            if let Some(posture) = seen.posture(0.0) {
                postures.push(posture);
                reference = Some(CameraDrift::new(seen.grey(), seen.person()));
            }
            self.publish();
        }
        let Some(reference) = reference else {
            return false;
        };
        let baseline = Posture {
            eye_y: median(postures.iter().map(|p| p.eye_y).collect()),
            size: median(postures.iter().map(|p| p.size).collect()),
        };
        tracing::info!(
            "baseline: eye line {:.0}px, face size {:.0}px",
            baseline.eye_y,
            baseline.size
        );
        self.start_tracking(baseline, reference);
        self.notifier.info("Got it", &format!("{APP_NAME} is on."));
        true
    }

    fn start_tracking(&mut self, baseline: Posture, drift: CameraDrift) {
        self.tracker = Some(Tracker::new(baseline, self.settings));
        self.drift = Some(drift);
    }

    /// The guided calibration: each step's Get ready and Hold, taught as it comes the first time,
    /// then the results, again as often as asked. Says whether it ended calibrated.
    fn play_game(&mut self, screens: &[String]) -> bool {
        let steps = posture::game_steps(screens);
        self.set_status(Mood::Idle, "Calibrating");
        self.view.lines = None;
        self.view.reading = None;
        let mut teach = !self.guided_before;
        let mut calibrated = false;
        while let Some((records, drift)) = self.play_steps(&steps, teach) {
            teach = false;
            let failed = self.finish_game(&steps, records, drift);
            calibrated |= failed.is_none();
            self.view.look_at = Some(LookAt {
                phase: failed.map_or(Phase::Done(self.settings.thresholds), Phase::Failed),
                ..LookAt::default()
            });
            self.publish();
            if self.wait_for_answer() != Answer::Again {
                break;
            }
        }
        self.act(None);
        self.view.look_at = None;
        self.view.banner = None;
        self.publish();
        calibrated
    }

    /// Teach step `index` until Next: its drawing and what to do, with nothing measured yet.
    /// Says whether to carry on.
    fn teach(&mut self, steps: &[Step], index: usize) -> bool {
        let step = &steps[index];
        tracing::info!("guided step {}: teach", index + 1);
        loop {
            std::thread::sleep(BUSY_INTERVAL);
            match self.guided_answer() {
                Some(Answer::Next) => return true,
                Some(Answer::Cancel) => return false,
                _ => {}
            }
            // Taking frames keeps the camera from counting as lost while this waits.
            let in_frame = self.observe().map(|seen| seen.face.is_some());
            self.view.look_at = Some(LookAt {
                screen: step.screen,
                phase: Phase::Teach,
                step: index + 1,
                steps: steps.len(),
                pose: Some(step.pose),
                prompt: step.prompt.clone(),
                in_frame: in_frame.unwrap_or(false),
                ..LookAt::default()
            });
            self.publish();
        }
    }

    /// Wait for a button or key in the full-screen window, or for something that stops it.
    fn wait_for_answer(&mut self) -> Answer {
        loop {
            if let Some(answer) = self.guided_answer() {
                return answer;
            }
            std::thread::sleep(BUSY_INTERVAL);
        }
    }

    /// The latest answer from the full-screen window, with anything that should stop the
    /// calibration, such as pausing, counting as Cancel.
    fn guided_answer(&mut self) -> Option<Answer> {
        let mut answer = None;
        while let Ok(command) = self.commands.try_recv() {
            match command {
                Command::Guided(given) => answer = Some(given),
                Command::Game { .. } => {}
                other => self.pending.push_back(other),
            }
        }
        if self.stopping() {
            Some(Answer::Cancel)
        } else {
            answer
        }
    }

    /// Each step: taught the first time its pose comes up if `teach`, then instructions until
    /// you're in the pose, then a quiet Hold while it records. `None` if it was stopped.
    fn play_steps(
        &mut self,
        steps: &[Step],
        teach: bool,
    ) -> Option<(Vec<GameRecord>, Option<CameraDrift>)> {
        let mut drift: Option<CameraDrift> = None;
        let mut records = Vec::new();
        let mut upright: Vec<Posture> = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            let first_of_pose = steps[..index].iter().all(|s| s.pose != step.pose);
            if teach && first_of_pose && !self.teach(steps, index) {
                return None;
            }
            tracing::info!("guided step {}: get ready", index + 1);
            self.act(Some(step.pose));
            let mut arrival = Arrival::new(
                step.pose,
                posture::typical(&upright),
                self.settings.thresholds,
            );
            let start = Instant::now();
            let mut holding_since: Option<Instant> = None;
            loop {
                std::thread::sleep(BUSY_INTERVAL);
                let answer = self.guided_answer();
                if answer == Some(Answer::Cancel) {
                    return None;
                }
                let Some(seen) = self.observe() else { continue };
                if drift.is_none() && seen.face.is_some() {
                    drift = Some(CameraDrift::new(seen.grey(), seen.person()));
                }
                let shift = drift
                    .as_mut()
                    .map_or(0.0, |d| d.update(seen.grey(), seen.person()));
                let posture = seen.posture(shift);
                if holding_since.is_none() {
                    let waited = start.elapsed();
                    let arrived = arrival.arrived(posture) && waited >= READY_AT_LEAST;
                    if arrived || answer == Some(Answer::Next) || waited >= READY_AT_MOST {
                        tracing::info!("guided step {}: hold", index + 1);
                        holding_since = Some(Instant::now());
                    }
                }
                let held = holding_since.map(|since| since.elapsed());
                if let (Some(held), Some(posture), Some(face)) = (held, posture, seen.face) {
                    if index == 0 {
                        upright.push(posture);
                    }
                    records.push(GameRecord {
                        sample: Sample {
                            step: index + 1,
                            pose: step.pose,
                            posture,
                        },
                        screen: step.screen,
                        t: held.as_secs_f32(),
                        drift: shift,
                        face: [face.x, face.y, face.width, face.height],
                        points: face.landmarks.points(),
                    });
                }
                let progress = held.map_or(0.0, |h| {
                    (h.as_secs_f32() / GAME_RECORD.as_secs_f32()).min(1.0)
                });
                self.view.banner = Some(Banner {
                    title: step.prompt.clone(),
                    lines: vec![
                        if held.is_some() {
                            "Hold"
                        } else {
                            "Get into it now"
                        }
                        .into(),
                    ],
                    progress: Some(progress),
                });
                self.view.look_at = Some(LookAt {
                    screen: step.screen,
                    phase: if held.is_some() {
                        Phase::Hold
                    } else {
                        Phase::Ready
                    },
                    step: index + 1,
                    steps: steps.len(),
                    pose: Some(step.pose),
                    prompt: step.prompt.clone(),
                    detail: step_detail(steps, index).into(),
                    progress,
                    in_frame: seen.face.is_some(),
                });
                self.view.status = if seen.face.is_some() {
                    "Calibrating"
                } else {
                    OUT_OF_FRAME
                }
                .into();
                self.publish();
                if held.is_some_and(|h| h >= GAME_RECORD) {
                    break;
                }
            }
        }
        Some((records, drift))
    }

    /// Score the steps and, if they measured you, start tracking with what they found. Returns
    /// the prompt of the first step that saw too little of you, if one did.
    fn finish_game(
        &mut self,
        steps: &[Step],
        records: Vec<GameRecord>,
        drift: Option<CameraDrift>,
    ) -> Option<String> {
        self.act(None);
        let samples: Vec<Sample> = records.iter().map(|r| r.sample).collect();
        let result = posture::score_game(steps, &samples);
        let dump = serde_json::json!({
            "steps": steps.iter().map(|s| serde_json::json!({"pose": s.pose, "prompt": s.prompt, "screen": s.screen})).collect::<Vec<_>>(),
            "samples": records,
            "result": result,
            "thresholds_before": self.settings.thresholds,
        });
        tracing::info!("game result: {result:?}");
        if self.persist {
            match config::save_game(unix_seconds(), &dump.to_string()) {
                Ok(path) => tracing::info!("game saved to {}", path.display()),
                Err(error) => tracing::warn!("couldn't save game: {error}"),
            }
        }
        let (Some(result), Some(drift)) = (result, drift) else {
            self.set_status(
                Mood::Idle,
                "You were out of frame for part of the calibration",
            );
            return Some(least_seen(steps, &samples));
        };
        let calibrated = result.thresholds(self.calibrated);
        self.settings.thresholds = self
            .settings
            .thresholds
            .recalibrated(self.calibrated, calibrated);
        self.calibrated = calibrated;
        self.save_thresholds(self.settings.thresholds);
        self.start_tracking(result.baseline, drift);
        self.guided_before = true;
        self.save_preferences();
        None
    }
}

/// What a step's prompt means, in a sentence under it.
fn step_detail(steps: &[Step], index: usize) -> &'static str {
    let after_slouching = index > 0 && steps[index - 1].pose != Pose::Upright;
    match steps[index].pose {
        Pose::Upright if after_slouching => "Back to sitting tall.",
        Pose::Upright => "Sit tall and comfortable, facing this screen.",
        Pose::Slump => "Sink into your chair. Keep your head the same distance from the screen.",
        Pose::Lean => {
            "Lean in towards the screen, the way you do when something's too small to read."
        }
    }
}

/// The prompt of the step with the fewest frames of a face.
fn least_seen(steps: &[Step], samples: &[Sample]) -> String {
    let seen = |n: usize| samples.iter().filter(|s| s.step == n).count();
    steps
        .iter()
        .enumerate()
        .min_by_key(|(i, _)| seen(i + 1))
        .map(|(_, step)| step.prompt.clone())
        .unwrap_or_default()
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

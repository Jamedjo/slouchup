//! The detection loop: camera frames in, posture decisions, notifications and UI snapshots out.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use camera_drift::{CameraDrift, Grey, Rect};
use crossbeam_channel::Receiver;
use dioxus_cameras::cameras::{self, Frame, PixelFormat, Resolution, StreamConfig};
use futures_channel::mpsc::UnboundedSender;
use posture::{Action, Posture, Reading, Sample, Settings, State, Step, Thresholds, Tracker};
use serde::Serialize;

use crate::art::Mood;
use crate::camera_view::FrameSlot;
use crate::config;
use crate::frames;
use crate::notifier::Notifier;

const MODEL: &[u8] = include_bytes!("../../../models/face_detection_yunet_2023mar.onnx");
const WATCH_INTERVAL: Duration = Duration::from_millis(200);
const BUSY_INTERVAL: Duration = Duration::from_millis(100);
const CALIBRATION: Duration = Duration::from_secs(3);
const GAME_SETTLE: Duration = Duration::from_millis(2500);
const GAME_RECORD: Duration = Duration::from_secs(4);
const RESULTS_SHOWN: Duration = Duration::from_secs(10);
const RESULTS_CARD: Duration = Duration::from_secs(4);
const RETRY_CALIBRATION: Duration = Duration::from_secs(5);
/// Laptop lids tilt rarely, so the background needn't be checked every frame.
const DRIFT_INTERVAL: Duration = Duration::from_secs(1);

pub enum Command {
    Recalibrate,
    Game { screens: Vec<String> },
    Pause(bool),
}

/// What the UI draws. Positions are in camera-frame pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub mood: Mood,
    pub status: String,
    pub frame_size: (f32, f32),
    pub face: Option<FaceView>,
    pub reading: Option<Reading>,
    pub thresholds: Thresholds,
    /// Where the baseline eye line and the slouch line currently fall.
    pub lines: Option<(f32, f32)>,
    pub banner: Option<Banner>,
    /// What the calibration game is asking for right now.
    pub look_at: Option<LookAt>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            mood: Mood::Idle,
            status: "Starting…".into(),
            frame_size: (640.0, 480.0),
            face: None,
            reading: None,
            thresholds: Thresholds::default(),
            lines: None,
            banner: None,
            look_at: None,
        }
    }
}

/// A calibration game step, for the full-screen prompt.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LookAt {
    /// The screen to look at, or `None` for the screen the camera window is on.
    pub screen: Option<usize>,
    pub prompt: String,
    pub detail: String,
    pub progress: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceView {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub points: [[f32; 2]; 5],
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
        Grey {
            pixels: &self.grey,
            width: self.width,
            height: self.height,
        }
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

    fn face_view(&self) -> Option<FaceView> {
        let f = self.face?;
        let s = self.scale;
        Some(FaceView {
            x: f.x * s,
            y: f.y * s,
            width: f.width * s,
            height: f.height * s,
            points: f.landmarks.points().map(|[x, y]| [x * s, y * s]),
        })
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
    latest: Arc<Mutex<Option<Frame>>>,
    pump: cameras::pump::Pump,
    detector: Option<yunet::Detector>,
    notifier: Notifier,
    events: UnboundedSender<View>,
    commands: Receiver<Command>,
    settings: Settings,
    tracker: Option<Tracker>,
    drift: Option<CameraDrift>,
    drift_checked: Option<Instant>,
    view: View,
    started: Instant,
    results_until: Option<Instant>,
    paused: bool,
}

impl Engine {
    pub fn start(
        camera: usize,
        preview: FrameSlot,
        notifier: Notifier,
        events: UnboundedSender<View>,
        commands: Receiver<Command>,
    ) -> Result<(), String> {
        let mut devices = cameras::devices().map_err(|e| format!("listing cameras: {e}"))?;
        // Number cameras like OpenCV does: by device path, skipping metadata nodes that can't capture.
        devices.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        let device = devices
            .into_iter()
            .filter(|d| cameras::probe(d).is_ok_and(|caps| !caps.formats.is_empty()))
            .nth(camera)
            .ok_or_else(|| format!("no camera {camera}"))?;
        let stream = |pixel_format| StreamConfig {
            resolution: Resolution {
                width: 640,
                height: 480,
            },
            framerate: 30,
            pixel_format,
        };
        let opened = cameras::open(&device, stream(PixelFormat::Yuyv))
            .or_else(|_| cameras::open(&device, stream(PixelFormat::Mjpeg)))
            .map_err(|e| format!("opening {}: {e}", device.name))?;
        tracing::info!("camera: {}", device.name);

        let latest = Arc::new(Mutex::new(None));
        let latest_for_pump = latest.clone();
        let mut frames = 0u64;
        let pump = cameras::pump::spawn(opened, move |frame| {
            frames += 1;
            // The preview converts every frame it's given to RGBA; half the camera's rate is plenty.
            if frames.is_multiple_of(2) {
                preview.publish(frame.clone());
            }
            *latest_for_pump.lock().unwrap() = Some(frame);
        });
        let engine = Engine {
            latest,
            pump,
            detector: None,
            notifier,
            events,
            commands,
            settings: Settings {
                thresholds: config::load_thresholds(),
                ..Settings::default()
            },
            tracker: None,
            drift: None,
            drift_checked: None,
            view: View::default(),
            started: Instant::now(),
            results_until: None,
            paused: false,
        };
        std::thread::Builder::new()
            .name("slouch-engine".into())
            .spawn(move || engine.run())
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn publish(&mut self) {
        self.view.thresholds = self.settings.thresholds;
        if self
            .results_until
            .is_some_and(|until| Instant::now() > until)
        {
            self.results_until = None;
            self.view.banner = None;
        }
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
            while let Ok(command) = self.commands.try_recv() {
                match command {
                    Command::Recalibrate => {
                        self.tracker = None;
                        announce = true;
                    }
                    Command::Game { screens } => {
                        if !self.play_game(&screens) && self.tracker.is_none() {
                            announce = true;
                        }
                    }
                    Command::Pause(paused) => {
                        self.paused = paused;
                        cameras::pump::set_active(&self.pump, !paused);
                    }
                }
            }
            if self.paused {
                self.set_status(Mood::Idle, "Paused");
                self.view.face = None;
                self.publish();
                std::thread::sleep(WATCH_INTERVAL);
                continue;
            }
            if self.tracker.is_none() {
                if !self.calibrate(announce) {
                    self.set_status(Mood::Idle, "Couldn't see a face to calibrate");
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

    /// Take the newest frame and find the face in it, at half resolution for speed.
    fn observe(&mut self) -> Option<Observation> {
        let frame = self.latest.lock().unwrap().take()?;
        let small = frames::half_size(&frame)?;
        let (width, height) = (small.width, small.height);
        self.view.frame_size = (frame.width as f32, frame.height as f32);
        if self.detector.is_none() {
            self.detector =
                Some(yunet::Detector::new(MODEL, width, height).expect("bundled model loads"));
        }
        let face = match self
            .detector
            .as_ref()
            .unwrap()
            .detect(&small.rgb, width, height)
        {
            Ok(faces) => faces
                .into_iter()
                .max_by(|a, b| (a.width * a.height).total_cmp(&(b.width * b.height))),
            Err(error) => {
                tracing::warn!("detection failed: {error}");
                None
            }
        };
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

        let (mood, status) = match update.state {
            State::Good => (Mood::Good, "Posture good".to_string()),
            State::Bad(problem) => (Mood::Bad, problem.to_string()),
            State::Away => (Mood::Idle, "No face in view".to_string()),
        };
        self.set_status(mood, status);
        match update.action {
            Some(Action::Nag(problem)) => {
                tracing::info!(
                    "nag: {problem} (camera drift {:.0}px)",
                    drift * seen.height as f32
                );
                self.notifier.nag(&problem.to_string());
            }
            Some(Action::Dismiss) => self.notifier.dismiss(),
            None => {}
        }
        let drift_px = drift * seen.height as f32;
        let line = (baseline.eye_y + drift_px) * seen.scale;
        let limit = line + self.settings.thresholds.drop * baseline.size * seen.scale;
        self.view.lines = Some((line, limit));
        self.view.face = seen.face_view();
        self.view.reading = update.reading;
        self.publish();
    }

    fn calibrate(&mut self, announce: bool) -> bool {
        self.set_status(Mood::Idle, "Calibrating — sit up nicely");
        self.view.lines = None;
        self.view.reading = None;
        if announce {
            self.notifier.info(
                "Calibrating 📏",
                "<span foreground=\"#ffd75f\">Sit up nicely for a few seconds…</span>",
            );
        }
        let mut postures = Vec::new();
        let mut reference = None;
        let end = Instant::now() + CALIBRATION;
        while Instant::now() < end {
            std::thread::sleep(BUSY_INTERVAL);
            let Some(seen) = self.observe() else { continue };
            if let Some(posture) = seen.posture(0.0) {
                postures.push(posture);
                reference = Some(CameraDrift::new(seen.grey(), seen.person()));
            }
            self.view.face = seen.face_view();
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
        self.notifier.info(
            "Slouch is watching 👀",
            "<span foreground=\"#5fffaf\">Calibrated. Stay tall!</span>",
        );
        true
    }

    fn start_tracking(&mut self, baseline: Posture, drift: CameraDrift) {
        self.tracker = Some(Tracker::new(baseline, self.settings));
        self.drift = Some(drift);
    }

    /// Walk through upright and slouched poses, then set thresholds between them.
    fn play_game(&mut self, screens: &[String]) -> bool {
        let steps = posture::game_steps(screens);
        self.set_status(Mood::Idle, "Calibration game");
        self.view.lines = None;
        self.view.reading = None;
        let mut drift: Option<CameraDrift> = None;
        let mut records = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            let title = format!("{}/{}  {}", index + 1, steps.len(), step.prompt);
            let start = Instant::now();
            while start.elapsed() < GAME_SETTLE + GAME_RECORD {
                std::thread::sleep(BUSY_INTERVAL);
                let Some(seen) = self.observe() else { continue };
                if drift.is_none() && seen.face.is_some() {
                    drift = Some(CameraDrift::new(seen.grey(), seen.person()));
                }
                let shift = drift
                    .as_mut()
                    .map_or(0.0, |d| d.update(seen.grey(), seen.person()));
                let elapsed = start.elapsed();
                let recording = elapsed >= GAME_SETTLE;
                if recording && let (Some(posture), Some(face)) = (seen.posture(shift), seen.face) {
                    records.push(GameRecord {
                        sample: Sample {
                            step: index + 1,
                            pose: step.pose,
                            posture,
                        },
                        screen: step.screen,
                        t: (elapsed - GAME_SETTLE).as_secs_f32(),
                        drift: shift,
                        face: [face.x, face.y, face.width, face.height],
                        points: face.landmarks.points(),
                    });
                }
                let (subtitle, progress) = if recording {
                    (
                        "Hold it…".to_string(),
                        (elapsed - GAME_SETTLE).as_secs_f32() / GAME_RECORD.as_secs_f32(),
                    )
                } else {
                    (
                        format!(
                            "Get ready {:.0}",
                            (GAME_SETTLE - elapsed).as_secs_f32().ceil()
                        ),
                        0.0,
                    )
                };
                self.view.banner = Some(Banner {
                    title: title.clone(),
                    lines: vec![subtitle.clone()],
                    progress: Some(progress),
                });
                self.view.look_at = Some(LookAt {
                    screen: step.screen,
                    prompt: step.prompt.clone(),
                    detail: subtitle.clone(),
                    progress,
                });
                self.view.face = seen.face_view();
                self.view.status = if seen.face.is_some() {
                    "Calibration game"
                } else {
                    "No face!"
                }
                .into();
                self.publish();
            }
        }
        self.finish_game(&steps, records, drift)
    }

    fn finish_game(
        &mut self,
        steps: &[Step],
        records: Vec<GameRecord>,
        drift: Option<CameraDrift>,
    ) -> bool {
        let samples: Vec<Sample> = records.iter().map(|r| r.sample).collect();
        let result = posture::score_game(steps, &samples);
        let dump = serde_json::json!({
            "steps": steps.iter().map(|s| serde_json::json!({"pose": s.pose, "prompt": s.prompt, "screen": s.screen})).collect::<Vec<_>>(),
            "samples": records,
            "result": result,
            "thresholds_before": self.settings.thresholds,
        });
        let dir = config::games_dir();
        let path = dir.join(format!("{}.json", unix_seconds()));
        if let Err(error) =
            std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, dump.to_string()))
        {
            tracing::warn!("couldn't save game: {error}");
        }
        tracing::info!("game saved to {}: {result:?}", path.display());

        let (Some(result), Some(drift)) = (result, drift) else {
            self.view.look_at = None;
            self.set_status(Mood::Idle, "The game didn't see your face enough");
            self.view.banner = Some(Banner {
                title: "Try again".into(),
                lines: vec!["Couldn't see your face in every step".into()],
                progress: None,
            });
            self.results_until = Some(Instant::now() + RESULTS_SHOWN);
            self.publish();
            return false;
        };
        self.settings.thresholds = result.thresholds(self.settings.thresholds);
        if let Err(error) = config::save_thresholds(self.settings.thresholds) {
            tracing::warn!("couldn't save thresholds: {error}");
        }
        self.start_tracking(result.baseline, drift);
        let line = |name: &str, m: &posture::MetricResult| {
            let new = m
                .threshold
                .map_or("unchanged".to_string(), |t| format!("limit {t:.2}"));
            format!(
                "{name}: upright ≤{:+.2}  slump {:+.2}  lean {:+.2}  {new}",
                m.upright_max, m.slump, m.lean
            )
        };
        let limits = self.settings.thresholds;
        self.view.look_at = Some(LookAt {
            screen: None,
            prompt: "Calibrated ✨".into(),
            detail: format!(
                "Nagging when your eyes drop {:.2} face heights or your face grows {:.0}%",
                limits.drop,
                limits.lean * 100.0
            ),
            progress: 1.0,
        });
        self.publish();
        std::thread::sleep(RESULTS_CARD);
        self.view.look_at = None;
        self.view.banner = Some(Banner {
            title: "Results".into(),
            lines: vec![line("drop", &result.drop), line("lean", &result.lean)],
            progress: None,
        });
        self.results_until = Some(Instant::now() + RESULTS_SHOWN);
        true
    }
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

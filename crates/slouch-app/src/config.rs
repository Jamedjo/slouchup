//! Where things live on disk. Shared with the Python version so either can read the other's calibration.

use std::path::PathBuf;

use posture::{Settings, Thresholds};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "slouch";
pub const APP_NAME: &str = "Slouch";

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_ID)
}

pub fn games_dir() -> PathBuf {
    cache_dir().join("games")
}

fn config_file(name: &str) -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_ID)
        .join(name)
}

fn thresholds_file() -> PathBuf {
    config_file("thresholds.json")
}

fn write_json(path: PathBuf, value: &impl Serialize) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().expect("file has a directory"))?;
    std::fs::write(path, serde_json::to_string_pretty(value)?)
}

/// Choices from the settings window. The limits live apart, in thresholds.json, which the
/// Python version also reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// The camera's device id, or `None` for the first that works.
    pub camera: Option<String>,
    pub grace: f64,
    pub min_gap: f64,
    pub cooldown: f64,
}

impl Default for Preferences {
    fn default() -> Self {
        let defaults = Settings::default();
        Self {
            camera: None,
            grace: defaults.grace,
            min_gap: defaults.min_gap,
            cooldown: defaults.cooldown,
        }
    }
}

impl Preferences {
    pub fn settings(&self, thresholds: Thresholds) -> Settings {
        Settings {
            thresholds,
            grace: self.grace,
            min_gap: self.min_gap,
            cooldown: self.cooldown,
        }
    }
}

pub fn load_preferences() -> Preferences {
    std::fs::read_to_string(config_file("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_preferences(preferences: &Preferences) -> std::io::Result<()> {
    write_json(config_file("settings.json"), preferences)
}

/// Whether a calibration has ever been saved; the first run starts with the game instead.
pub fn has_thresholds() -> bool {
    thresholds_file().exists()
}

pub fn load_thresholds() -> Thresholds {
    std::fs::read_to_string(thresholds_file())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_thresholds(thresholds: Thresholds) -> std::io::Result<()> {
    write_json(thresholds_file(), &thresholds)
}

#[derive(Clone, Debug, Default)]
pub struct Args {
    pub camera: Option<usize>,
    pub demo: bool,
    pub show: bool,
    pub game: bool,
    pub settings: bool,
    pub test_notification: bool,
}

pub fn parse_args() -> Args {
    let mut args = Args::default();
    let mut raw = std::env::args().skip(1);
    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "--show" => args.show = true,
            "--demo" => args.demo = true,
            "--game" => args.game = true,
            "--settings" => args.settings = true,
            "--test-notification" => args.test_notification = true,
            "--camera" => args.camera = raw.next().and_then(|n| n.parse().ok()),
            "--help" | "-h" => {
                println!(
                    "Usage: slouch [--show] [--game] [--settings] [--camera N | --demo] [--test-notification]"
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument {other}; try --help");
                std::process::exit(2);
            }
        }
    }
    args
}

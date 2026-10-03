//! Where things live on disk. Shared with the Python version so either can read the other's calibration.

use std::path::PathBuf;

use posture::Thresholds;

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

fn thresholds_file() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_ID)
        .join("thresholds.json")
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
    let path = thresholds_file();
    std::fs::create_dir_all(path.parent().expect("file has a directory"))?;
    std::fs::write(path, serde_json::to_string_pretty(&thresholds)?)
}

#[derive(Clone, Debug, Default)]
pub struct Args {
    pub camera: usize,
    pub demo: bool,
    pub show: bool,
    pub game: bool,
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
            "--test-notification" => args.test_notification = true,
            "--camera" => args.camera = raw.next().and_then(|n| n.parse().ok()).unwrap_or(0),
            "--help" | "-h" => {
                println!(
                    "Usage: slouch [--show] [--game] [--camera N | --demo] [--test-notification]"
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

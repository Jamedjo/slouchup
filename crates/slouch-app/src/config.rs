//! Where things live on disk.

use std::path::{Path, PathBuf};

use posture::{Settings, Thresholds};
use serde::{Deserialize, Serialize};

/// Names the settings and cache directories.
pub const APP_ID: &str = "slouchup";
/// The settings directory's name before the rename, which the Python version still uses.
const OLD_ID: &str = "slouch";
/// The product name as people see it. Files, folders and IDs keep the lowercase `APP_ID`.
pub const APP_NAME: &str = "SlouchUp";
/// Windows names app folders by publisher, then app.
const PUBLISHER: &str = "We Are Frames";
/// Names the Linux desktop file, its icon and its AppStream metadata.
#[cfg(all(unix, not(target_os = "macos")))]
pub const DESKTOP_ID: &str = "dev.weareframes.slouchup";

/// Per-user cache directory. Never a shared temporary one, where other users could plant files.
pub fn cache_dir() -> PathBuf {
    if cfg!(windows) {
        windows_dir()
    } else {
        dirs::cache_dir()
            .expect("a home directory for the cache")
            .join(APP_ID)
    }
}

fn settings_dir() -> PathBuf {
    if cfg!(windows) {
        windows_dir()
    } else {
        config_dir(APP_ID)
    }
}

/// Settings and cache together. Never `%LocalAppData%\slouchup`, where the installer puts the app
/// and which uninstalling empties.
fn windows_dir() -> PathBuf {
    dirs::data_local_dir()
        .expect("a home directory for app data")
        .join(PUBLISHER)
        .join(APP_ID)
}

/// Recent calibration games kept for looking back at; older ones are deleted.
const GAMES_KEPT: usize = 20;

/// Save a calibration game's recording, readable only by you since it holds face positions.
pub fn save_game(stamp: u64, json: &str) -> std::io::Result<PathBuf> {
    let dir = cache_dir().join("games");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{stamp}.json"));
    write_private(&path, json.as_bytes())?;
    let mut games: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .collect();
    games.sort();
    for old in games.iter().rev().skip(GAMES_KEPT) {
        let _ = std::fs::remove_file(old);
    }
    Ok(path)
}

fn config_dir(id: &str) -> PathBuf {
    dirs::config_dir()
        .expect("a home directory for settings")
        .join(id)
}

fn config_file(name: &str) -> PathBuf {
    settings_dir().join(name)
}

const SETTINGS_FILES: [&str; 2] = ["thresholds.json", "settings.json"];

/// Copy settings saved under the old name, once, so the rename doesn't lose a calibration.
pub fn adopt_old_settings() -> std::io::Result<()> {
    copy_settings(&config_dir(OLD_ID), &settings_dir())
}

/// Copy the settings files from `old` into `new`, unless `new` already has settings. On Windows,
/// `new` also holds the cache, so it's there before any settings are.
fn copy_settings(old: &Path, new: &Path) -> std::io::Result<()> {
    if SETTINGS_FILES.iter().any(|name| new.join(name).exists()) || !old.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(new)?;
    for name in SETTINGS_FILES {
        let from = old.join(name);
        if from.exists() {
            std::fs::copy(from, new.join(name))?;
        }
    }
    Ok(())
}

fn thresholds_file() -> PathBuf {
    config_file("thresholds.json")
}

/// Written beside and then renamed over the old file, so a crash or a second writer never
/// leaves a half-written file behind.
fn write_json(path: PathBuf, value: &impl Serialize) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().expect("file has a directory"))?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_string_pretty(value)?)?;
    std::fs::rename(temporary, path)
}

fn history_file() -> PathBuf {
    cache_dir().join("history.json")
}

pub fn load_history() -> crate::history::History {
    crate::history::History::load(&history_file())
}

/// Save the posture history privately, swapping the new file in whole.
pub fn delete_history() -> std::io::Result<()> {
    match std::fs::remove_file(history_file()) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub fn save_history(json: &str) -> std::io::Result<()> {
    let path = history_file();
    std::fs::create_dir_all(path.parent().expect("file has a directory"))?;
    let temporary = path.with_extension("json.tmp");
    write_private(&temporary, json.as_bytes())?;
    std::fs::rename(temporary, path)
}

fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(contents)
}

/// Choices from the settings window. The limits live apart, in thresholds.json, in the format
/// the Python version wrote.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// The camera's device id, or `None` for the first that works.
    pub camera: Option<String>,
    pub grace: f64,
    pub min_gap: f64,
    pub cooldown: f64,
    /// Whether the history window's record is kept. Turning it off stops recording.
    pub keep_history: bool,
    /// Whether the AppImage has added itself to the app launcher before, so an entry the user
    /// removed isn't added back.
    pub added_to_launcher: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        let defaults = Settings::default();
        Self {
            camera: None,
            grace: defaults.grace,
            min_gap: defaults.min_gap,
            cooldown: defaults.cooldown,
            keep_history: true,
            added_to_launcher: false,
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
        .checked()
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
        .and_then(|text| serde_json::from_str::<Thresholds>(&text).ok())
        .unwrap_or_default()
        .checked()
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
    pub history: bool,
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
            "--history" => args.history = true,
            "--test-notification" => args.test_notification = true,
            "--camera" => args.camera = raw.next().and_then(|n| n.parse().ok()),
            "--help" | "-h" => {
                println!(
                    "Usage: slouchup [--show] [--game] [--settings] [--history] [--camera N | --demo] [--test-notification]"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_are_copied_only_when_there_are_no_new_ones() {
        let root = std::env::temp_dir().join(format!("slouchup-test-{}", std::process::id()));
        let (old, new) = (root.join("old"), root.join("new"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("thresholds.json"), "old").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("lock"), "").unwrap();

        copy_settings(&old, &new).unwrap();
        assert_eq!(
            std::fs::read_to_string(new.join("thresholds.json")).unwrap(),
            "old"
        );
        assert!(!new.join("settings.json").exists());

        std::fs::write(new.join("thresholds.json"), "new").unwrap();
        copy_settings(&old, &new).unwrap();
        assert_eq!(
            std::fs::read_to_string(new.join("thresholds.json")).unwrap(),
            "new"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_files_are_kept_out_of_the_install_folder() {
        let install = dirs::data_local_dir().unwrap().join(APP_ID);
        assert!(!settings_dir().starts_with(&install));
        assert!(!cache_dir().starts_with(&install));
    }
}

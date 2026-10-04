//! The app as the Windows installer, Velopack, puts it: in `%LocalAppData%\slouchup`, which
//! uninstalling empties, so the app's own files are kept elsewhere (see `config`).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateManager, VelopackApp};

/// Releases, with the packages updates are made from.
const REPOSITORY: &str = "https://github.com/Jamedjo/slouchup";
/// Soon enough for a fix to arrive the same day, and far under GitHub's limit for anonymous use.
const CHECK_EVERY: Duration = Duration::from_secs(4 * 60 * 60);

static UPDATES: OnceLock<UpdateManager> = OnceLock::new();

/// The installer runs the app to carry out its steps, such as before uninstalling, and the app
/// exits once a step is done. Otherwise this returns straight away, unless an update downloaded
/// last time wasn't installed on quitting, in which case it's installed and the app restarted.
pub fn run_installer_step() {
    VelopackApp::build()
        // Settings stay, as Windows apps usually leave them, so a reinstall picks them up.
        .on_before_uninstall_fast_callback(|_| crate::windows_shell::unregister_toast_sender())
        .run();
}

/// Check for a new release now and every few hours, downloading it quietly for the next quit.
/// Does nothing when the installer didn't install this copy, as when it's built from source.
pub fn keep_up_to_date() {
    let source = GithubSource::new(REPOSITORY, None, false);
    let Ok(manager) = UpdateManager::new(source, None, None) else {
        return;
    };
    let manager = UPDATES.get_or_init(|| manager);
    let checker = std::thread::Builder::new()
        .name("updates".into())
        .spawn(move || {
            loop {
                if let Err(error) = download_update(manager) {
                    tracing::warn!("couldn't check for an update: {error}");
                }
                std::thread::sleep(CHECK_EVERY);
            }
        });
    if let Err(error) = checker {
        tracing::warn!("couldn't start checking for updates: {error}");
    }
}

fn download_update(manager: &UpdateManager) -> Result<(), velopack::Error> {
    if let UpdateCheck::UpdateAvailable(update) = manager.check_for_updates()? {
        manager.download_updates(&update, None)?;
    }
    Ok(())
}

/// Install a downloaded update once the app has exited. It isn't started again: quitting is
/// asked for, and starting would turn the camera back on.
pub fn update_on_quit() {
    // Quit can be chosen twice before the app is gone, and only one installer can run.
    static HANDED_OVER: AtomicBool = AtomicBool::new(false);
    if HANDED_OVER.swap(true, Ordering::Relaxed) {
        return;
    }
    let Some(manager) = UPDATES.get() else {
        return;
    };
    let Some(update) = manager.get_update_pending_restart() else {
        return;
    };
    if let Err(error) =
        manager.wait_exit_then_apply_updates(update, true, false, Vec::<String>::new())
    {
        tracing::warn!("couldn't install the update: {error}");
    }
}

//! The app as the Windows installer, Velopack, puts it: in `%LocalAppData%\slouchup`, which
//! uninstalling empties, so the app's own files are kept elsewhere (see `config`).

use velopack::VelopackApp;

/// The installer runs the app to carry out its steps, such as before uninstalling, and the app
/// exits once a step is done. Otherwise this returns straight away.
pub fn run_installer_step() {
    VelopackApp::build()
        // Settings stay, as Windows apps usually leave them, so a reinstall picks them up.
        .on_before_uninstall_fast_callback(|_| crate::windows_shell::unregister_toast_sender())
        .run();
}

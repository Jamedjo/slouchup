# Developing SlouchUp

How to build, run, test and package SlouchUp. [RELEASE.md](RELEASE.md) covers cutting a release.

## Layout

A Rust workspace of everything in `crates/`:

| Crate | What it does |
|---|---|
| [`yunet`](crates/yunet) | YuNet face detection with five landmarks, in pure Rust on tract. The model is in `models/`. |
| [`camera-drift`](crates/camera-drift) | How far the webcam has tilted since a reference frame, from the background around the person |
| [`posture`](crates/posture) | Posture metrics, slouch decisions, nudge timing and calibration game scoring, with no camera or UI |
| [`tray-popover`](crates/tray-popover) | Where the tray's popover goes against the monitors' work areas, the panel's edge and its scale, when a click opens or closes it, and what each Linux panel's click positions are in. No windowing library. |
| [`tray-popover-tao`](crates/tray-popover-tao) | The popover in a tao window: borderless, hidden until shown, closed when it loses the focus. A non-activating panel on macOS, with rounded corners on Windows 11. |
| [`slouch-app`](crates/slouch-app) | The app itself, built as the `slouchup` binary: the Dioxus windows, tray, notifications, camera, updates and the demo person |

`vendor/tract-core` is a patched tract-core with vectorised kernels that make face detection about
four times faster on x86. It's excluded from the workspace and patched in through `Cargo.toml`.

Elsewhere: `packaging/` builds the installers, `scripts/` holds the release and screenshot
scripts, `www/` is the homepage, and `docs/screenshots/` the README's images.

## Toolchain

- Rust 1.91 or newer (the workspace's `rust-version`), edition 2024. CI uses the latest stable.
- On Windows with MSVC, `.cargo/config.toml` links the C runtime statically, so the exe starts on
  machines without the Visual C++ redistributable.
- On Linux, the WebKitGTK, GTK, tray and clang development packages. On Ubuntu:

  ```sh
  sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
    libxdo-dev libsoup-3.0-dev libclang-dev
  ```

## Building and running

```sh
cargo build --release
target/release/slouchup
```

Build in release: face detection is very slow unoptimised. With no flags SlouchUp starts in the
tray, and opens its welcome on the first run. The flags, from `slouchup --help`:

| Flag | Does |
|---|---|
| `--show` | Opens the window on the camera view |
| `--settings` | Opens the window on the Settings view |
| `--history` | Opens the window on the History view |
| `--game` | Starts the calibration game |
| `--demo` | Uses the drawn demo person instead of a webcam, with a made-up week of history. Settings and history aren't saved, and it runs beside a real SlouchUp. |
| `--camera N` | Uses camera N, counting from 0, instead of the saved choice |
| `--card` | With `--demo`, shows every nudge in SlouchUp's own card, as when notifications can't show |
| `--test-notification` | Sends a sample nudge and exits |

Use `--demo` for anything you show or screenshot. The `slouch-app` examples help with diagnosing:
`cargo run --example cameras` lists cameras and their formats, and `cargo run --example monitors`
lists screens as the app names them.

## Tests and CI

CI (`.github/workflows/ci.yml`) runs on every pull request and push to `main`, on Linux, macOS and
Windows:

```sh
cargo fmt --all --check                                          # Linux only
cargo clippy --release --workspace --all-targets -- -D warnings
cargo test --release --workspace
cargo test --release --manifest-path vendor/tract-core/Cargo.toml  # Linux only
```

The patched tract-core sits outside the workspace, so its tests need their own run.

The Release workflow also packages every platform on pull requests that change `packaging/` or the
app icon, and the Pages workflow builds the homepage on pull requests that change `www/`.

## Screenshots

```sh
cargo build --release
scripts/screenshots.sh
```

This regenerates the camera, Settings, History and calibration game screenshots in
`docs/screenshots/`, from the demo person. It runs the app in a headless sway session with its own
D-Bus session, so nothing appears on your desktop or in its tray. It needs sway, waybar, grim,
swaync, jq and ImageMagick.

## Packaging

Each script runs from the repository root and writes to `target/release/`. The Release workflow
runs all three; see [RELEASE.md](RELEASE.md).

- **Windows**: `pwsh -File packaging/windows/setup.ps1` builds `velopack/slouchup-setup.exe`,
  a Velopack installer that installs for the current user into `%LOCALAPPDATA%\slouchup`. It needs
  the .NET SDK and `vpk` at the same version as the `velopack` crate
  (`dotnet tool install --global vpk --version <version>`).
- **Linux**: `packaging/linux/appimage.sh` builds `velopack/slouchup-x86_64.AppImage`, with
  linuxdeploy and its GTK plugin, `vpk` and mksquashfs. It bundles WebKit's helper processes and
  the tray library, which linuxdeploy can't find on its own. CI builds it on Ubuntu 22.04, so it
  runs on distributions with an older glibc.
- **macOS**: `packaging/macos/bundle.sh` builds `SlouchUp.app` for Apple silicon and Intel, and
  `slouchup-mac.dmg` to install it from. It needs both Rust targets
  (`rustup target add aarch64-apple-darwin x86_64-apple-darwin`). macOS only grants camera access
  to a bundle whose `Info.plist` says why it wants it.

The Windows and Linux scripts download the latest release's package with `vpk download github`,
so they can build a delta update from it.

## Homepage

`www/` is an [Astro](https://astro.build) site, served at [slouchup.com](https://slouchup.com).

```sh
cd www
npm install
npm run dev     # a local server that reloads on changes
npm run check   # type checks, as CI runs
npm run build   # into www/dist
```

The Pages workflow (`.github/workflows/pages.yml`) builds the site on pull requests and publishes
it to GitHub Pages on every push to `main` that changes `www/`, or the person or icon artwork it
draws from `crates/slouch-app/src`. Homepage changes go in the same pull request as the work they
describe.

## Platform notes

- **macOS notifications** all go through notify-rust, sent as the app's bundle id,
  `dev.weareframes.slouchup`. The sender has to be set before the first notification: without it,
  the first one asks AppleScript for an app called "use_default" and hangs for two minutes. A test
  checks the id matches `packaging/macos/Info.plist`.
- **The tray** is tray-icon's own, newer than the one dioxus-desktop re-exports, and on Linux it
  uses ksni, so a left click reaches the app as a StatusNotifierItem `Activate(x, y)`. Panels give
  that position in device or logical pixels, or not at all; `tray_popover::linux::Host` keeps the
  table. On X11 the popover opens by the icon; on Wayland the compositor places it, until it
  moves to layer-shell. Where a panel never sends clicks, starting SlouchUp again opens its
  window.
- **The Windows installer's splash** progress bar is coloured with vpk's `--splashProgressColor`;
  the splash image is drawn by `cargo run --example splash`.
- **Reinstalling on Windows**: uninstalling can leave `Update.exe` in `%LOCALAPPDATA%\slouchup`,
  which makes the installer offer Repair. For a clean reinstall, run `Update.exe --uninstall`, then
  delete that folder. Settings are kept separately, in `%LOCALAPPDATA%\We Are Frames\slouchup`.

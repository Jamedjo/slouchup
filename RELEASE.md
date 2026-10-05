# Releasing SlouchUp

A release is two steps: a pull request that raises the version, then a tag on `main` once it's
merged. The tag makes the Release workflow publish the packages as a GitHub release.

The crates carry the release version, and `scripts/release.sh` keeps them in step. Run it from the
repository root, on a clean checkout of `main` that matches `origin/main`; it refuses otherwise.

## 1. The release pull request

```sh
git switch main && git pull
scripts/release.sh 0.0.3
git switch -c release-v0.0.3
```

This raises every crate to 0.0.3, including the versions `slouchup` asks of the other crates, and
updates `Cargo.lock`. Commit those with anything else the release needs, such as homepage changes
in `www/` or README updates, and open the pull request. CI runs as for any other pull request.

## 2. The tag

Once the pull request is merged:

```sh
git switch main && git pull
scripts/release.sh --tag 0.0.3
```

This checks the crates on `main` are at 0.0.3, then tags `v0.0.3` and pushes the tag.

## What the Release workflow publishes

`.github/workflows/release.yml` packages SlouchUp on every push to `main`, keeping the packages
as workflow artifacts. A `v*` tag also publishes them as a GitHub release titled "SlouchUp v0.0.3",
with notes generated from the merged pull requests.

| Platform | Files | Built by |
|---|---|---|
| Windows | `slouchup-setup.exe`, the Velopack installer; `slouchup-*.nupkg`; `releases.win.json` | `packaging/windows/setup.ps1` |
| Linux | `slouchup-x86_64.AppImage`; `slouchup-*-linux-*.nupkg`; `releases.linux.json` | `packaging/linux/appimage.sh` |
| Mac | `slouchup-mac.dmg`, holding `SlouchUp.app` for Apple silicon and Intel | `packaging/macos/bundle.sh` |

Before publishing, the workflow checks that the tag is `v` plus the crates' version, and fails
otherwise. The packages carry the crates' version, and installed copies compare it to decide
whether to update, so a mismatched tag would publish a release nothing updates to.

## Downloads and updates

The homepage's download buttons link to
`https://github.com/Jamedjo/slouchup/releases/latest/download/<file>`, so they reach the new
release as soon as it's published, with no homepage change. Renaming a package means changing
`www/src/components/Download.astro` in the same pull request.

Installed Windows copies and the AppImage use Velopack to check the GitHub releases when they
start and every four hours after. Windows reads `releases.win.json` and Linux reads
`releases.linux.json`, so each finds its own packages. A newer release is downloaded in the
background and installed when SlouchUp quits, or else the next time it starts. Pre-releases are
skipped.

The Windows and Linux packaging also downloads the latest release's full package, so it can build
a delta package from it, which updates download instead of the whole app.

The Mac app doesn't update itself: a newer version is installed from the DMG.

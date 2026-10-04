# Builds target/release/velopack/slouchup-setup.exe, which installs slouchup for the current user
# without asking anything, beside the packages a release needs for updates. Needs vpk at the
# velopack crate's version, as the release workflow installs it. Run from the repository root.
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

cargo build --release -p slouchup
cargo run --release --example icon -- 256 target/release/slouchup.ico
cargo run --release --example splash -- target/release/slouchup-splash.png
$version = ((cargo pkgid -p slouchup) -split "[#@]")[-1]

# vpk packs a whole folder, so the exe gets one of its own.
$app = "target/release/velopack-app"
$out = "target/release/velopack"
Remove-Item -Recurse -Force $app, $out -ErrorAction Ignore
New-Item -ItemType Directory $app | Out-Null
Copy-Item target/release/slouchup.exe $app

# The Start menu shortcut shares the toast sender's app ID, so Windows groups the two as one app.
# There's deliberately no shortcut to start at login: the camera light would come on at every
# login, and the camera would be held from other apps.
vpk pack --packId slouchup --packVersion $version --packDir $app --mainExe slouchup.exe `
    --runtime win-x64 --packTitle SlouchUp --packAuthors "We Are Frames" `
    --icon target/release/slouchup.ico --splashImage target/release/slouchup-splash.png --splashProgressColor "#E8553A" --aumid dev.weareframes.slouchup --shortcuts StartMenuRoot `
    --noPortable --outputDir $out
Move-Item "$out/slouchup-win-Setup.exe" "$out/slouchup-setup.exe"

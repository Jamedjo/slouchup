#!/bin/sh
# Builds slouchup.app. macOS only grants camera access to a bundle that says why it wants it,
# and LSUIElement keeps a tray app out of the Dock. Run on a Mac from the repository root.
set -e
cargo build --release -p slouch
app=target/release/slouchup.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp target/release/slouch "$app/Contents/MacOS/slouch"
cp packaging/macos/Info.plist "$app/Contents/Info.plist"
codesign --force --sign - "$app"
echo "Built $app"

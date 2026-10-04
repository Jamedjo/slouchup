#!/bin/sh
# Builds target/release/SlouchUp.app for Apple silicon and Intel Macs, and slouchup-mac.dmg to
# install it from. macOS only grants camera access to a bundle that says why it wants it, and
# LSUIElement keeps a tray app out of the Dock. Run on a Mac from the repository root.
set -e
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release -p slouchup --target "$target"
done

app=target/release/SlouchUp.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/slouchup" \
    target/aarch64-apple-darwin/release/slouchup target/x86_64-apple-darwin/release/slouchup
cp packaging/macos/Info.plist "$app/Contents/Info.plist"
pkgid=$(cargo pkgid -p slouchup)
plutil -replace CFBundleShortVersionString -string "${pkgid##*[#@]}" "$app/Contents/Info.plist"

iconset=target/release/slouchup.iconset
rm -rf "$iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    cargo run --release --example icon -- "$size" "$iconset/icon_${size}x${size}.png"
    cargo run --release --example icon -- "$((size * 2))" "$iconset/icon_${size}x${size}@2x.png"
done
iconutil -c icns -o "$app/Contents/Resources/slouchup.icns" "$iconset"
codesign --force --sign - "$app"

# The disk image opens on the app beside a link to Applications, to drag it across.
staging=target/release/dmg
rm -rf "$staging"
mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
hdiutil create -volname SlouchUp -srcfolder "$staging" -ov -format UDZO target/release/slouchup-mac.dmg

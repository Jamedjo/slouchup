#!/bin/sh
# Builds target/release/slouchup-linux.AppImage. Needs linuxdeploy with its gtk and appimage
# plugins on PATH, and the webkit2gtk-4.1 development files. Run from the repository root.
set -e
export APPIMAGE_EXTRACT_AND_RUN=1
id=dev.weareframes.slouchup
cargo build --release -p slouchup
cargo run --release --example icon -- 256 "target/release/$id.png"

appdir=target/release/slouchup.AppDir
rm -rf "$appdir"
mkdir -p "$appdir/usr/share/metainfo"
cp packaging/linux/AppRun "$appdir/AppRun"
cp "packaging/linux/$id.metainfo.xml" "$appdir/usr/share/metainfo/"

# WebKit starts its helper processes from a directory it names by absolute path, under /usr.
# Bundle them at the same place under usr/, then make the library's copy of that path relative,
# as "././" in place of "/usr" keeps its length. AppRun runs from usr/ so it resolves there.
webkit=$(pkg-config --variable=libdir webkit2gtk-4.1)/webkit2gtk-4.1
inside=${webkit#/usr}
mkdir -p "$appdir/usr$inside"
cp -r "$webkit"/WebKit*Process "$webkit"/injected-bundle "$appdir/usr$inside/"

linuxdeploy --appdir "$appdir" \
    --executable target/release/slouchup \
    --desktop-file "packaging/linux/$id.desktop" \
    --icon-file "target/release/$id.png" \
    --deploy-deps-only "$appdir/usr$inside" \
    --plugin gtk
sed -i "s|$webkit|././$inside|g" "$appdir"/usr/lib/libwebkit2gtk-4.1.so*

pkgid=$(cargo pkgid -p slouchup)
LINUXDEPLOY_OUTPUT_VERSION=${pkgid##*[#@]} OUTPUT=target/release/slouchup-linux.AppImage \
    linuxdeploy-plugin-appimage --appdir "$appdir"

#!/bin/sh
# Builds target/release/velopack/slouchup-x86_64.AppImage, beside the packages a release needs for
# updates. Needs linuxdeploy with its gtk plugin, and vpk at the velopack crate's version with
# mksquashfs, on PATH, and the webkit2gtk-4.1 development files. Run from the repository root.
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

version=$(cargo pkgid -p slouchup | sed 's/.*[#@]//')
echo "X-AppImage-Version=$version" >> "$appdir/$id.desktop"

# The latest release's package, so vpk can make a delta from it, which AppImages download instead
# of the whole app. It's left out when it isn't older, as for builds of main before the version is
# raised, since vpk refuses to pack a version that isn't newer than the ones beside it.
out=target/release/velopack
rm -rf "$out"
vpk download github --repoUrl https://github.com/Jamedjo/slouchup --channel linux \
    --outputDir "$out" ${GITHUB_TOKEN:+--token "$GITHUB_TOKEN"}
older=
for package in "$out"/*.nupkg; do
    [ -e "$package" ] || continue
    released=$(basename "$package" | sed 's/^slouchup-\(.*\)-linux-full\.nupkg$/\1/')
    if [ "$released" != "$version" ] &&
        [ "$(printf '%s\n' "$released" "$version" | sort -V | head -1)" = "$released" ]; then
        older="$older $package"
    else
        rm "$package"
    fi
done

# vpk keeps the AppDir as it is, adding its updater beside the app.
vpk pack --packId slouchup --packVersion "$version" --packDir "$appdir" --mainExe slouchup \
    --channel linux --packTitle SlouchUp --packAuthors "We Are Frames" --outputDir "$out"
mv "$out/slouchup.AppImage" "$out/slouchup-x86_64.AppImage"
# Already on the release it came from.
[ -z "$older" ] || rm $older

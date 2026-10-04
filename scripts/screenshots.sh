#!/bin/sh
# Regenerates docs/screenshots from the demo person, in a headless sway session with its own
# D-Bus session, so nothing appears on the real desktop or in its tray.
# Needs sway, waybar, grim, swaync, jq and ImageMagick. Run from the repository root after
# `cargo build --release`.
set -e

if [ "$1" != "--inside" ]; then
    work=$(mktemp -d)
    # A panel with a tray, so the screenshots show the tray icon. Waybar rather than swaybar,
    # because swaybar can't show a tray icon named by its file path, as appindicator names them.
    cat > "$work/waybar.json" <<EOF
{ "position": "top", "height": 36, "modules-right": ["tray"], "tray": { "icon-size": 22 } }
EOF
    cat > "$work/waybar.css" <<EOF
window#waybar { background: #2a2a2e; }
#tray { padding: 0 12px; }
EOF
    cat > "$work/config" <<EOF
output HEADLESS-1 resolution 1100x860 position 0 0 bg #1b1d26 solid_color
output HEADLESS-2 resolution 1100x860 position 1100 0 bg #1b1d26 solid_color
focus output HEADLESS-1
default_border none
exec waybar -c "$work/waybar.json" -s "$work/waybar.css"
exec swaync
exec "$0" --inside "$work"
EOF
    # Two screens, so the calibration game has one to point at.
    WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=2 WLR_LIBINPUT_NO_DEVICES=1 \
        dbus-run-session -- sway -c "$work/config"
    rm -rf "$work"
    exit 0
fi

work=$2
out=docs/screenshots
mkdir -p "$out"
app=target/release/slouchup
shot() { grim -o HEADLESS-1 "$out/$1.png"; }

# The demo person sits up for 8 seconds, sinks for 5, sits up for 6, then leans in for 5.
"$app" --demo --show > "$work/watch.log" 2>&1 &
sleep 6.5 && shot good-posture
sleep 4.5 && shot slouching
sleep 11.5 && shot leaning
kill $!

"$app" --demo --settings > "$work/settings.log" 2>&1 &
sleep 3 && swaync-client -C > /dev/null && swaymsg -q '[title="slouchup settings"] focus' && sleep 1 && grim -g "$(swaymsg -t get_tree | jq -r '.. | select(.name? == "slouchup settings") | .rect | "\(.x),\(.y) \(.width)x\(.height)"')" "$out/settings.png"
kill $!

# The game's first step: sit up and look at one of the two screens. Whichever screen holds the
# full-screen prompt is the busier picture; the other is plain background.
"$app" --demo --game > "$work/game.log" 2>&1 &
sleep 7 && swaync-client -C > /dev/null
grim -o HEADLESS-1 "$work/screen-1.png" && grim -o HEADLESS-2 "$work/screen-2.png"
kill $!
busiest=$(for f in "$work"/screen-*.png; do echo "$(identify -format '%[standard-deviation]' "$f") $f"; done | sort -rn | head -1 | cut -d' ' -f2)
cp "$busiest" "$out/calibration-game.png"

swaymsg exit

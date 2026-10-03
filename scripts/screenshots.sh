#!/bin/sh
# Regenerates docs/screenshots from the demo person, in a headless sway session with its own
# D-Bus session, so nothing appears on the real desktop or in its tray.
# Needs sway, grim and swaync. Run from the repository root after `cargo build --release`.
set -e

if [ "$1" != "--inside" ]; then
    work=$(mktemp -d)
    cat > "$work/config" <<EOF
output HEADLESS-1 resolution 1100x860 bg #1b1d26 solid_color
default_border none
exec swaync
exec "$0" --inside "$work"
EOF
    WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 dbus-run-session -- sway -c "$work/config"
    rm -rf "$work"
    exit 0
fi

work=$2
out=docs/screenshots
mkdir -p "$out"
app=target/release/slouch
shot() { grim "$out/$1.png"; }

# The demo person sits up for 8 seconds, sinks for 5, sits up for 6, then leans in for 5.
"$app" --demo --show > "$work/watch.log" 2>&1 &
sleep 6.5 && shot good-posture
sleep 4.5 && shot slouching
sleep 11.5 && shot leaning
kill $!

# The game with one screen: sit up, slump, sit up again, lean.
"$app" --demo --game > "$work/game.log" 2>&1 &
sleep 9.5 && shot calibration-game
kill $!

swaymsg exit

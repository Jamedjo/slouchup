#!/bin/sh
# System site packages give us PyGObject, libnotify and AyatanaAppIndicator from apt.
set -e
cd "$(dirname "$0")"
dir="$(pwd)"

[ -d .venv ] || uv venv --system-site-packages --python /usr/bin/python3 .venv
uv pip install --python .venv/bin/python 'opencv-python-headless>=5' numpy

model=models/face_detection_yunet_2023mar.onnx
if [ ! -f "$model" ]; then
    mkdir -p models
    curl -fL -o "$model" https://github.com/opencv/opencv_zoo/raw/main/models/face_detection_yunet/$(basename "$model")
fi

.venv/bin/python -c 'import slouch; slouch.make_images()'
icons="$HOME/.local/share/icons/hicolor/128x128/apps"
mkdir -p "$icons"
cp "$HOME/.cache/slouch/slouch.png" "$icons/slouch.png"

apps="$HOME/.local/share/applications"
mkdir -p "$apps"
cat > "$apps/slouch.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Slouch
Comment=Nags you when you slouch at the webcam
Exec=$dir/.venv/bin/python $dir/slouch.py
Icon=slouch
Terminal=false
Categories=Utility;
StartupNotify=false
EOF

echo "Installed $apps/slouch.desktop"

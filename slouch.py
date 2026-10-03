#!/usr/bin/env python3
"""Watch the webcam and nag when your posture drifts from where it was at calibration."""

import argparse
import fcntl
import json
import logging
import math
import threading
import time
from dataclasses import dataclass
from pathlib import Path

import cv2
import gi
import numpy as np

gi.require_version("Gtk", "3.0")
gi.require_version("Notify", "0.7")
gi.require_version("Gdk", "3.0")
gi.require_version("AyatanaAppIndicator3", "0.1")
from gi.repository import AyatanaAppIndicator3 as AppIndicator, Gdk, GdkPixbuf, GLib, Gtk, Notify  # noqa: E402

try:
    gi.require_version("GtkLayerShell", "0.1")
    from gi.repository import GtkLayerShell
except (ValueError, ImportError):
    GtkLayerShell = None

APP_ID = "slouch"
APP_NAME = "Slouch"
CACHE = Path.home() / ".cache" / APP_ID
MODEL = Path(__file__).resolve().parent / "models" / "face_detection_yunet_2023mar.onnx"
RAINBOW = [(80, 80, 255), (60, 180, 255), (80, 230, 255), (120, 220, 90), (255, 160, 60), (220, 90, 200)]
STATE_COLOURS = {
    "good": [(120, 220, 90), (200, 200, 60)],
    "bad": [(60, 60, 255), (60, 140, 255)],
    "idle": [(140, 140, 140), (90, 90, 90)],
}
# Seconds for the baseline to move most of the way towards how you are sitting now. Improvements
# are adopted quickly so the bar rises with you; drifts in the slouchy direction (while still
# within the thresholds) are followed slowly so gradual slouching isn't absorbed.
ADAPT_BETTER = 180
ADAPT_WORSE = 1800
SMOOTHING = 0.6
SAMPLE_INTERVAL = 0.2
FACE_LOST_GIVE_UP = 120
FACE_BLINK = 1.5
THRESHOLDS_FILE = Path.home() / ".config" / APP_ID / "thresholds.json"
GAMES_DIR = CACHE / "games"
GAME_SETTLE = 2.5
GAME_RECORD = 4
METRICS = {"drop": "threshold", "lean": "lean"}

log = logging.getLogger(APP_ID)


def gradient(width, height, stops):
    xs = np.linspace(0, len(stops) - 1, width)
    row = np.array([
        np.interp(xs, range(len(stops)), [s[c] for s in stops]) for c in range(3)
    ]).T
    return np.repeat(row[np.newaxis], height, axis=0).astype(np.uint8)


def person_icon(stops):
    icon = cv2.cvtColor(gradient(128, 128, stops), cv2.COLOR_BGR2BGRA)
    mask = np.zeros((128, 128), np.uint8)
    cv2.circle(mask, (64, 64), 62, 255, -1)
    icon[:, :, 3] = mask
    cv2.circle(icon, (64, 40), 16, (255, 255, 255, 255), -1)
    cv2.line(icon, (64, 56), (64, 104), (255, 255, 255, 255), 10)
    return icon


def make_images():
    CACHE.mkdir(parents=True, exist_ok=True)
    cv2.imwrite(str(CACHE / f"{APP_ID}.png"), person_icon(RAINBOW))
    for state, stops in STATE_COLOURS.items():
        cv2.imwrite(str(CACHE / f"{APP_ID}-{state}.png"), person_icon(stops))

    banner = gradient(400, 60, RAINBOW)
    cv2.putText(banner, "SIT UP STRAIGHT", (40, 42), cv2.FONT_HERSHEY_DUPLEX, 1.2, (255, 255, 255), 2, cv2.LINE_AA)
    cv2.imwrite(str(CACHE / "banner.png"), banner)


def notify(summary, body, timeout_ms=6000):
    n = Notify.Notification.new(summary, body, str(CACHE / f"{APP_ID}.png"))
    n.set_hint("desktop-entry", GLib.Variant("s", APP_ID))
    n.set_timeout(timeout_ms)
    n.show()


class Nagger:
    """Owns the one slouch notification so repeats replace it and sitting up can dismiss it."""

    def __init__(self):
        self.current = None

    def nag(self, reason):
        body = (
            f'<span foreground="#ff5f87"><b>{reason}</b></span>\n'
            f'<span foreground="#5fffaf">Shoulders back,</span> '
            f'<span foreground="#5fafff">chin up!</span>\n'
            f'<img src="{CACHE / "banner.png"}" alt="sit up"/>'
        )
        if self.current is None:
            self.current = Notify.Notification.new("", "", str(CACHE / f"{APP_ID}.png"))
            self.current.set_hint("desktop-entry", GLib.Variant("s", APP_ID))
            self.current.set_timeout(15000)
        self.current.update("🦒 Stop slouching!", body, str(CACHE / f"{APP_ID}.png"))
        self.current.show()

    def dismiss(self):
        if self.current is not None:
            try:
                self.current.close()
            except GLib.Error:
                pass  # already expired or closed by the user


def describe_screens():
    """Return [(Gdk.Monitor, name)] with names like "top-left" from where each screen sits."""
    display = Gdk.Display.get_default()
    monitors = [display.get_monitor(i) for i in range(display.get_n_monitors())]
    rects = [m.get_geometry() for m in monitors]
    left = min(r.x for r in rects)
    width = max(r.x + r.width for r in rects) - left
    top = min(r.y for r in rects)
    height = max(r.y + r.height for r in rects) - top
    stacked = len({r.y for r in rects}) > 1
    side_by_side = len({r.x for r in rects}) > 1
    screens = []
    for monitor, r in zip(monitors, rects):
        across = (r.x + r.width / 2 - left) / width
        down = (r.y + r.height / 2 - top) / height
        parts = []
        if stacked:
            parts.append("top" if down < 0.5 else "bottom")
        if side_by_side:
            parts.append("left" if across < 1 / 3 else "right" if across > 2 / 3 else "middle")
        screens.append((monitor, "-".join(parts) + " screen"))
    return screens


def game_steps(screens):
    """[(pose, prompt, index into screens or None)] for the calibration game."""
    if len(screens) < 2:
        return [
            ("upright", "Sit up straight", None),
            ("slump", "Slouch down, don't lean forward", None),
            ("upright", "Sit up straight again", None),
            ("lean", "Slouch leaning towards the screen", None),
        ]
    looks = [("upright", f"Sit up straight, look at the {name}", i) for i, (_, name) in enumerate(screens)]
    return looks + [
        ("slump", "Slouch down, don't lean forward", None),
        ("upright", "Sit up straight again", None),
        ("lean", "Slouch leaning towards the screen", None),
    ]


@dataclass
class Face:
    """One YuNet detection, reduced to the numbers posture is judged on (pixels unless noted)."""

    box: np.ndarray
    points: np.ndarray
    eye_y: float
    size: float

    @classmethod
    def from_yunet(cls, row):
        box = row[:4]
        right_eye, left_eye, _nose, right_mouth, left_mouth = row[4:14].reshape(5, 2)
        eyes = (right_eye + left_eye) / 2
        mouth = (right_mouth + left_mouth) / 2
        # Averaging a horizontal and a vertical span keeps size steadier when the head turns or nods.
        size = (np.linalg.norm(right_eye - left_eye) + np.linalg.norm(mouth - eyes)) / 2
        return cls(box, row[4:14].reshape(5, 2), eyes[1], size)

    def posture(self, shift_px):
        return np.array([self.eye_y - shift_px, self.size])


@dataclass
class Reading:
    drop: float
    lean: float

    def worst(self, args):
        """The largest metric as a fraction of its threshold, and a message for it."""
        ratios = [
            (self.lean / args.lean, f"You're {self.lean:.0%} closer to the screen than usual."),
            (self.drop / args.threshold, "Your head has sunk lower than usual."),
        ]
        return max(ratios, key=lambda r: r[0])


class CameraShift:
    """Estimate how far the camera has tilted since calibration from background points.

    Tilting the lid moves the whole image, whereas slouching only moves the person. Rotating a
    camera shifts near and far things equally, so the background's vertical shift can be
    subtracted straight from the face position.
    """

    MIN_POINTS = 10
    LK = dict(winSize=(21, 21), maxLevel=3)

    def __init__(self, grey, box):
        self.carried = 0.0
        self.last = 0.0
        self._reference(grey, box)

    def _reference(self, grey, box):
        self.reference = grey
        self.points = cv2.goodFeaturesToTrack(grey, 300, 0.01, 8, mask=self._background(grey.shape, box))

    @staticmethod
    def _background(shape, box):
        """Mask out a generous region around and below the face, where the person's head and body are."""
        mask = np.full(shape, 255, np.uint8)
        x, y, w, h = box
        mask[max(0, int(y - 0.6 * h)):, max(0, int(x - w)):max(0, int(x + 2 * w))] = 0
        return mask

    def update(self, grey, box):
        """Return the vertical shift as a fraction of frame height."""
        if self.points is None or len(self.points) < self.MIN_POINTS:
            self._reference(grey, box)
            return self.last
        moved, found, _ = cv2.calcOpticalFlowPyrLK(self.reference, grey, self.points, None, **self.LK)
        back, found_back, _ = cv2.calcOpticalFlowPyrLK(grey, self.reference, moved, None, **self.LK)
        good = (found[:, 0] == 1) & (found_back[:, 0] == 1)
        good &= np.linalg.norm(back - self.points, axis=2)[:, 0] < 1
        height, width = grey.shape
        xs, ys = moved[:, 0, 0].astype(int), moved[:, 0, 1].astype(int)
        good &= (xs >= 0) & (xs < width) & (ys >= 0) & (ys < height)
        good[good] &= self._background(grey.shape, box)[ys[good], xs[good]] > 0
        if good.sum() >= self.MIN_POINTS:
            self.last = self.carried + float(np.median(moved[good, 0, 1] - self.points[good, 0, 1])) / height
        if good.sum() < len(self.points) * 0.4:
            # Lighting or the scene changed too much to keep tracking; restart from here keeping the offset.
            self.carried = self.last
            self._reference(grey, box)
        return self.last


class Watcher:
    def __init__(self, args, on_state, on_frame, on_target):
        self.args = args
        self.on_state = on_state
        self.on_frame = on_frame
        self.on_target = on_target
        self.steps = game_steps([])
        self.cap = cv2.VideoCapture(args.camera)
        if not self.cap.isOpened():
            raise SystemExit(f"Could not open camera {args.camera}")
        self.detector = cv2.FaceDetectorYN.create(str(MODEL), "", (640, 480), 0.7)
        self.paused = threading.Event()
        self.recalibrate = threading.Event()
        self.recalibrate.set()
        self.game = threading.Event()
        self.viewing = threading.Event()
        self.state = None
        self.baseline = None
        self.camera = None
        self.last_box = None
        self.nagger = Nagger()

    def set_state(self, state, text):
        if state != self.state:
            log.info("%s: %s", state, text)
            self.state = state
        self.on_state(state, text)

    def read(self):
        """Return (frame, grey, largest Face or None)."""
        ok, frame = self.cap.read()
        if not ok:
            return None, None, None
        height, width = frame.shape[:2]
        self.detector.setInputSize((width, height))
        _, faces = self.detector.detect(frame)
        grey = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
        if faces is None:
            return frame, grey, None
        return frame, grey, Face.from_yunet(max(faces, key=lambda f: f[2] * f[3]))

    def calibrate(self, announce=True):
        self.set_state("idle", "Calibrating — sit up nicely")
        if announce:
            GLib.idle_add(notify, "Calibrating 📏", '<span foreground="#ffd75f">Sit up nicely for a few seconds…</span>', 3000)
        samples, grey, box = [], None, None
        end = time.monotonic() + self.args.calibrate
        while time.monotonic() < end:
            frame, frame_grey, face = self.read()
            if face is not None:
                samples.append(face.posture(0))
                grey, box = frame_grey, face.box
            if frame is not None and self.viewing.is_set():
                self.on_frame(self.annotate(frame, face, None, "Calibrating"))
            time.sleep(0.1)
        if not samples:
            self.baseline = None
            return False
        self.baseline = np.median(samples, axis=0)
        self.camera = CameraShift(grey, box)
        self.last_box = box
        log.info("baseline: eye line %.0fpx, face size %.0fpx", *self.baseline)
        GLib.idle_add(notify, "Slouch is watching 👀", '<span foreground="#5fffaf">Calibrated. Stay tall!</span>', 3000)
        return True

    def play_game(self):
        """Walk through upright and slouched poses, then set thresholds between them."""
        self.set_state("idle", "Calibration game")
        steps = self.steps
        camera = None
        samples = []
        for number, (pose, prompt, screen) in enumerate(steps, 1):
            title = f"{number}/{len(steps)}  {prompt}"
            start = time.monotonic()
            while (elapsed := time.monotonic() - start) < GAME_SETTLE + GAME_RECORD:
                frame, grey, face = self.read()
                if frame is None or (face is None and camera is None):
                    continue
                if face is not None:
                    self.last_box = face.box
                camera = camera or CameraShift(grey, self.last_box)
                shift = camera.update(grey, self.last_box)
                recording = elapsed >= GAME_SETTLE
                if recording and face is not None:
                    samples.append({
                        "pose": pose, "step": number, "screen": screen, "t": elapsed - GAME_SETTLE,
                        "shift": shift, "posture": face.posture(shift * frame.shape[0]).tolist(),
                        "box": face.box.tolist(), "points": face.points.tolist(),
                    })
                if recording:
                    subtitle, progress = "Hold it...", (elapsed - GAME_SETTLE) / GAME_RECORD
                else:
                    subtitle, progress = f"Get ready {GAME_SETTLE - elapsed:.0f}", 0
                self.on_target(screen, subtitle)
                self.on_frame(self.annotate(frame, face, None, "No face!" if face is None else "",
                                            banner=(title, subtitle, progress)))
                time.sleep(0.05)
        self.on_target(None, "")

        GAMES_DIR.mkdir(parents=True, exist_ok=True)
        dump = GAMES_DIR / time.strftime("%Y%m%d-%H%M%S.json")
        result = self.score_game(samples)
        dump.write_text(json.dumps({"steps": steps, "samples": samples, "result": result, "args": vars(self.args)}, indent=1))
        log.info("game saved to %s: %s", dump, result)
        if result is None:
            self.set_state("idle", "Game didn't see your face enough")
            return False

        self.baseline = np.array(result["baseline"])
        self.camera = camera
        for metric, values in result["metrics"].items():
            if values["threshold"] is not None:
                setattr(self.args, METRICS[metric], values["threshold"])
        THRESHOLDS_FILE.parent.mkdir(parents=True, exist_ok=True)
        THRESHOLDS_FILE.write_text(json.dumps({METRICS[m]: getattr(self.args, METRICS[m]) for m in METRICS}, indent=1))
        self.show_results(result)
        return True

    def score_game(self, samples):
        by_step = {}
        for s in samples:
            by_step.setdefault((s["step"], s["pose"]), []).append(s["posture"])
        if len(by_step) < len(self.steps) or any(len(p) < 5 for p in by_step.values()):
            return None
        upright_steps = [np.array(p) for (_, pose), p in by_step.items() if pose == "upright"]
        self.baseline = np.median(np.vstack(upright_steps), axis=0)

        def readings(postures):
            return np.array([[getattr(self.read_posture(p), m) for m in METRICS] for p in postures])

        # Each upright step is a different screen, so how far apart they sit is normal movement, not slouching.
        upright_medians = np.array([np.median(readings(p), axis=0) for p in upright_steps])
        jitter = np.mean([np.std(readings(p), axis=0) for p in upright_steps], axis=0)
        slumps = {pose: np.median(readings(np.array(p)), axis=0) for (_, pose), p in by_step.items() if pose != "upright"}
        metrics = {}
        for i, metric in enumerate(METRICS):
            upright_max = float(upright_medians[:, i].max())
            moved = float(max(s[i] for s in slumps.values()))
            # A metric that barely separates from sitting upright would only produce false alarms.
            usable = moved - upright_max > 4 * jitter[i]
            metrics[metric] = {
                "jitter": float(jitter[i]),
                "upright_max": upright_max,
                "slump": float(slumps["slump"][i]),
                "lean": float(slumps["lean"][i]),
                "threshold": (upright_max + moved) / 2 if usable else None,
            }
        return {"baseline": self.baseline.tolist(), "metrics": metrics}

    def show_results(self, result):
        lines = []
        for metric, v in result["metrics"].items():
            new = "unchanged" if v["threshold"] is None else f"limit {v['threshold']:.2f}"
            lines.append(f"{metric}: upright <={v['upright_max']:+.2f}  slump {v['slump']:+.2f}  "
                         f"lean {v['lean']:+.2f}  {new}")
        end = time.monotonic() + 10
        while time.monotonic() < end and self.viewing.is_set():
            frame, _, face = self.read()
            if frame is not None:
                self.on_frame(self.annotate(frame, face, None, "Calibrated", banner=("Results", lines, None)))
            time.sleep(0.1)

    def read_posture(self, posture):
        base_y, base_size = self.baseline
        return Reading(drop=(posture[0] - base_y) / base_size, lean=posture[1] / base_size - 1)

    def adapt(self, posture, dt):
        """Drift the baseline towards how you are sitting now; lower is better for every metric."""
        tau = np.where(posture < self.baseline, ADAPT_BETTER, ADAPT_WORSE)
        self.baseline += (1 - np.exp(-dt / tau)) * (posture - self.baseline)

    def run(self):
        slouch_since = None
        last_nag = 0.0
        nagged_this_slouch = False
        smoothed = None
        last_tick = time.monotonic()
        last_seen = 0.0
        last_ratio = 0.0
        while True:
            if self.paused.is_set():
                time.sleep(0.5)
                continue
            if self.game.is_set():
                self.game.clear()
                self.recalibrate.clear()
                smoothed = None
                slouch_since = None
                if not self.play_game():
                    self.recalibrate.set()
                last_tick = time.monotonic()
                continue
            if self.recalibrate.is_set() or self.baseline is None:
                retrying = self.baseline is None and not self.recalibrate.is_set()
                self.recalibrate.clear()
                smoothed = None
                slouch_since = None
                if not self.calibrate(announce=not retrying):
                    self.set_state("idle", "Couldn't see a face to calibrate")
                    time.sleep(5)
                last_tick = time.monotonic()
                continue

            time.sleep(SAMPLE_INTERVAL)
            frame, grey, face = self.read()
            if frame is None:
                time.sleep(1)
                continue
            now = time.monotonic()
            dt, last_tick = now - last_tick, now
            if face is not None:
                self.last_box = face.box
            shift_px = self.camera.update(grey, self.last_box) * frame.shape[0]

            if face is None and now - last_seen < FACE_BLINK:
                continue
            reading = None
            if face is not None:
                last_seen = now
                posture = face.posture(shift_px)
                alpha = 1 - math.exp(-dt / SMOOTHING)
                smoothed = posture if smoothed is None else smoothed + alpha * (posture - smoothed)
                reading = self.read_posture(smoothed)
                last_ratio, reason = reading.worst(self.args)
                if last_ratio <= 1:
                    reason = None
                    self.adapt(smoothed, dt)
            elif now - last_seen < FACE_LOST_GIVE_UP and last_ratio > 0.5:
                # Faces vanish when you hunch over the keyboard, so losing one mid-slouch counts.
                reason = "Your head dropped out of view."
            else:
                smoothed = None
                reason = None

            if self.viewing.is_set():
                self.on_frame(self.annotate(frame, face, reading, reason or ("Posture good" if face else "No face")))

            if reason is None:
                slouch_since = None
                if nagged_this_slouch:
                    nagged_this_slouch = False
                    GLib.idle_add(self.nagger.dismiss)
                if face is None:
                    self.set_state("idle", "No face in view")
                else:
                    self.set_state("good", "Posture good")
            else:
                self.set_state("bad", reason)
                if slouch_since is None:
                    slouch_since = now
                # Each new slouch nags promptly so the habit gets caught; one long slouch repeats slowly.
                gap = self.args.cooldown if nagged_this_slouch else self.args.min_gap
                if now - slouch_since >= self.args.grace and now - last_nag >= gap:
                    log.info("nag: %s (camera shift %.0fpx)", reason, shift_px)
                    GLib.idle_add(self.nagger.nag, reason)
                    last_nag = now
                    nagged_this_slouch = True

    def annotate(self, frame, face, reading, status, banner=None):
        view = frame.copy()
        width = view.shape[1]
        if self.baseline is not None and self.camera is not None and banner is None:
            shift_px = self.camera.last * view.shape[0]
            base_y, base_size = self.baseline
            line = int(base_y + shift_px)
            limit = int(base_y + shift_px + self.args.threshold * base_size)
            cv2.line(view, (0, line), (width, line), (90, 220, 120), 2)
            cv2.line(view, (0, limit), (width, limit), (80, 80, 255), 1)
            cv2.putText(view, "baseline", (8, line - 6), cv2.FONT_HERSHEY_SIMPLEX, 0.5, (90, 220, 120), 1, cv2.LINE_AA)
            cv2.putText(view, "slouch", (8, limit + 16), cv2.FONT_HERSHEY_SIMPLEX, 0.5, (80, 80, 255), 1, cv2.LINE_AA)
        if face is not None:
            x, y, w, h = face.box.astype(int)
            cv2.rectangle(view, (x, y), (x + w, y + h), (255, 200, 80), 1)
            for px, py in face.points.astype(int):
                cv2.circle(view, (px, py), 3, (80, 230, 255), -1)
        if reading is not None:
            for i, (name, value, limit) in enumerate([
                ("drop", reading.drop, self.args.threshold),
                ("lean", reading.lean, self.args.lean),
            ]):
                self._bar(view, 30 + 26 * i, name, value / limit)
        if banner is not None:
            self._banner(view, *banner)
        cv2.rectangle(view, (0, view.shape[0] - 30), (width, view.shape[0]), (30, 30, 30), -1)
        cv2.putText(view, status, (10, view.shape[0] - 10), cv2.FONT_HERSHEY_SIMPLEX, 0.6, (255, 255, 255), 1, cv2.LINE_AA)
        return cv2.cvtColor(view, cv2.COLOR_BGR2RGB)

    @staticmethod
    def _banner(view, title, subtitle, progress):
        lines = subtitle if isinstance(subtitle, list) else [subtitle]
        width = view.shape[1]
        height = 60 + 24 * len(lines) + (14 if progress is not None else 0)
        band = view[:height]
        band[:] = (band * 0.35).astype(np.uint8)
        cv2.putText(view, title, (14, 38), cv2.FONT_HERSHEY_DUPLEX, 0.9, (80, 230, 255), 2, cv2.LINE_AA)
        for i, line in enumerate(lines):
            cv2.putText(view, line, (14, 66 + 24 * i), cv2.FONT_HERSHEY_SIMPLEX, 0.55, (255, 255, 255), 1, cv2.LINE_AA)
        if progress is not None:
            stops = gradient(width - 28, 8, RAINBOW)
            fill = int(progress * (width - 28))
            view[height - 16:height - 8, 14:14 + fill] = stops[:, :fill]

    @staticmethod
    def _bar(view, y, name, ratio):
        x0, length = view.shape[1] - 210, 150
        cv2.putText(view, name, (x0 - 50, y + 12), cv2.FONT_HERSHEY_SIMPLEX, 0.5, (255, 255, 255), 1, cv2.LINE_AA)
        cv2.rectangle(view, (x0, y), (x0 + length, y + 14), (60, 60, 60), -1)
        fill = int(np.clip(ratio, 0, 1.5) / 1.5 * length)
        colour = (90, 220, 120) if ratio <= 0.5 else (60, 200, 255) if ratio <= 1 else (80, 80, 255)
        cv2.rectangle(view, (x0, y), (x0 + fill, y + 14), colour, -1)
        mark = x0 + int(length / 1.5)
        cv2.line(view, (mark, y - 2), (mark, y + 16), (255, 255, 255), 1)


class LookHere(Gtk.Window):
    """A big marker floated over whichever screen the game wants you to look at."""

    def __init__(self):
        super().__init__(title=f"{APP_NAME} target")
        self.screen = None
        self.screens = []
        if GtkLayerShell is not None:
            GtkLayerShell.init_for_window(self)
            GtkLayerShell.set_layer(self, GtkLayerShell.Layer.OVERLAY)
        self.label = Gtk.Label()
        self.label.set_justify(Gtk.Justification.CENTER)
        frame = Gtk.EventBox()
        frame.add(self.label)
        frame.override_background_color(Gtk.StateFlags.NORMAL, Gdk.RGBA(0.1, 0.1, 0.2, 0.9))
        self.add(frame)
        self.set_default_size(520, 240)

    def show_on(self, screen, text):
        if screen is None:
            self.screen = None
            self.hide()
            return
        if screen != self.screen:
            self.hide()
            if GtkLayerShell is not None:
                GtkLayerShell.set_monitor(self, self.screens[screen][0])
            self.screen = screen
        self.label.set_markup(
            '<span size="64000">👀</span>\n'
            '<span size="32000" weight="bold" foreground="#ffd75f">Look here</span>\n'
            f'<span size="20000" foreground="#5fffaf">{GLib.markup_escape_text(text)}</span>'
        )
        self.show_all()


class Visualizer(Gtk.Window):
    def __init__(self, watcher):
        super().__init__(title=APP_NAME)
        self.watcher = watcher
        self.look_here = LookHere()
        self.set_icon_name(APP_ID)
        self.image = Gtk.Image()
        game = Gtk.Button(label="Calibration game")
        game.connect("clicked", lambda _: self.start_game())
        quick = Gtk.Button(label="Quick recalibrate")
        quick.connect("clicked", lambda _: watcher.recalibrate.set())
        buttons = Gtk.Box(spacing=6, homogeneous=True)
        buttons.pack_start(game, True, True, 0)
        buttons.pack_start(quick, True, True, 0)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        box.pack_start(self.image, True, True, 0)
        box.pack_start(buttons, False, False, 0)
        self.add(box)
        self.connect("delete-event", self.on_close)

    def present_window(self):
        self.watcher.viewing.set()
        self.show_all()
        self.present()

    def start_game(self):
        screens = describe_screens()
        self.look_here.screens = screens
        self.watcher.steps = game_steps(screens)
        self.present_window()
        self.watcher.game.set()

    def on_close(self, *_):
        self.watcher.viewing.clear()
        self.hide()
        return True

    def show_frame(self, rgb):
        height, width = rgb.shape[:2]
        pixbuf = GdkPixbuf.Pixbuf.new_from_bytes(
            GLib.Bytes.new(rgb.tobytes()), GdkPixbuf.Colorspace.RGB, False, 8, width, height, width * 3,
        )
        self.image.set_from_pixbuf(pixbuf)


class Tray:
    def __init__(self, watcher, visualizer):
        self.watcher = watcher
        self.state = None
        self.indicator = AppIndicator.Indicator.new(APP_ID, f"{APP_ID}-idle", AppIndicator.IndicatorCategory.APPLICATION_STATUS)
        self.indicator.set_icon_theme_path(str(CACHE))
        self.indicator.set_title(APP_NAME)
        self.indicator.set_status(AppIndicator.IndicatorStatus.ACTIVE)

        menu = Gtk.Menu()
        self.status_item = Gtk.MenuItem(label="Starting…")
        self.status_item.set_sensitive(False)
        menu.append(self.status_item)
        menu.append(Gtk.SeparatorMenuItem())
        show = Gtk.MenuItem(label="Show camera")
        show.connect("activate", lambda _: visualizer.present_window())
        menu.append(show)
        for label, action in [
            ("Calibration game", lambda _: visualizer.start_game()),
            ("Recalibrate", lambda _: watcher.recalibrate.set()),
            ("Pause", self.toggle_pause),
            ("Quit", lambda _: Gtk.main_quit()),
        ]:
            item = Gtk.MenuItem(label=label)
            item.connect("activate", action)
            menu.append(item)
        menu.show_all()
        self.indicator.set_menu(menu)
        self.indicator.set_secondary_activate_target(show)

    def toggle_pause(self, item):
        if self.watcher.paused.is_set():
            self.watcher.paused.clear()
            item.set_label("Pause")
        else:
            self.watcher.paused.set()
            item.set_label("Resume")
            self.set_state("idle", "Paused")

    def set_state(self, state, text):
        if state != self.state:
            self.indicator.set_icon_full(f"{APP_ID}-{state}", text)
            self.state = state
        self.status_item.set_label(text)


def single_instance():
    lock = open(CACHE / "lock", "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise SystemExit("Slouch is already running")
    return lock


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--camera", type=int, default=0)
    parser.add_argument("--threshold", type=float, default=0.8,
                        help="how far the eyes may drop, in face sizes, before it counts as slouching")
    parser.add_argument("--lean", type=float, default=0.15,
                        help="how much bigger the face may get (leaning in) before it counts as slouching")
    parser.add_argument("--grace", type=float, default=1.5, help="seconds of slouching before nagging")
    parser.add_argument("--min-gap", type=float, default=10, help="minimum seconds between nags for separate slouches")
    parser.add_argument("--cooldown", type=float, default=60, help="seconds between repeat nags during one long slouch")
    parser.add_argument("--calibrate", type=float, default=3, help="calibration duration in seconds")
    parser.add_argument("--show", action="store_true", help="open the camera window on start")
    parser.add_argument("--test-notification", action="store_true")
    parser.add_argument("--game", action="store_true", help="start with the calibration game")
    if THRESHOLDS_FILE.exists():
        parser.set_defaults(**json.loads(THRESHOLDS_FILE.read_text()))
    args = parser.parse_args()

    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(message)s")
    # Frames are small and infrequent, so OpenCV's thread pool costs more than it saves.
    cv2.setNumThreads(1)
    GLib.set_prgname(APP_ID)
    make_images()
    Notify.init(APP_NAME)
    if args.test_notification:
        Nagger().nag("You're 20% closer to the screen than usual.")
        return

    lock = single_instance()  # noqa: F841 - held for the life of the process
    tray = visualizer = None
    watcher = Watcher(
        args,
        on_state=lambda state, text: GLib.idle_add(tray.set_state, state, text),
        on_frame=lambda rgb: GLib.idle_add(visualizer.show_frame, rgb),
        on_target=lambda screen, text: GLib.idle_add(visualizer.look_here.show_on, screen, text),
    )
    visualizer = Visualizer(watcher)
    tray = Tray(watcher, visualizer)
    if args.game:
        visualizer.start_game()
    elif args.show:
        visualizer.present_window()
    threading.Thread(target=watcher.run, daemon=True).start()
    GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, 2, Gtk.main_quit)
    GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, 15, Gtk.main_quit)
    Gtk.main()


if __name__ == "__main__":
    main()

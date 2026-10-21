"""Editor windows for video and audio: Trim (waveform, film strip, fades, listen), Crop and Speed.

They only collect the settings. The native DragShift.exe runs ffmpeg and shows the result, so
closing a window never interrupts a job. Previews (frames, waveform, sound) come from the same
bundled ffmpeg, run as hidden low-priority child processes."""
from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

from PySide6.QtCore import QObject, QPointF, QRectF, QSize, Qt, QTimer, Signal
from PySide6.QtGui import QImage, QKeySequence, QPainter, QPainterPath, QPen, QPixmap, QShortcut
from PySide6.QtWidgets import (QCheckBox, QHBoxLayout, QLabel, QLineEdit, QPushButton, QSizePolicy,
                               QVBoxLayout, QWidget)

from . import glass
from .i18n import tr
from .tools import CropView, dims_text, section, slider, slider_row, toggle_row
from .util import file_size, human_size, ui_family
from .window import GlassWindow

IS_WIN = sys.platform == "win32"
NO_WINDOW = 0x08000000 if IS_WIN else 0
BELOW_NORMAL = 0x00004000 if IS_WIN else 0

Emit = Callable[[dict], None]


def ffmpeg_exe() -> str:
    return os.environ.get("DRAGSHIFT_FFMPEG") or shutil.which("ffmpeg") or "ffmpeg"


def run_ff(args: list[str], timeout: float | None = 60) -> subprocess.CompletedProcess:
    return subprocess.run([ffmpeg_exe(), "-hide_banner", "-nostdin", *args], stdin=subprocess.DEVNULL,
                          capture_output=True, timeout=timeout, creationflags=NO_WINDOW | BELOW_NORMAL)


def file_arg(p) -> str:
    return f"file:{p}"


def fmt_time(t: float, ms: bool = True) -> str:
    t = max(0.0, t)
    h, rem = divmod(t, 3600)
    m, s = divmod(rem, 60)
    sec = f"{s:06.3f}" if ms else f"{int(s):02d}"
    return f"{int(h)}:{int(m):02d}:{sec}" if h >= 1 else f"{int(m)}:{sec}"


def parse_time(text: str) -> float | None:
    text = text.strip().replace(",", ".")
    if not text:
        return None
    try:
        parts = [float(p) for p in text.split(":")]
    except ValueError:
        return None
    v = 0.0
    for p in parts:
        v = v * 60 + p
    return v


# ---------------------------------------------------------------- probing
@dataclass
class MediaInfo:
    duration: float = 0.0
    width: int = 0
    height: int = 0
    fps: float = 0.0
    has_video: bool = False
    has_audio: bool = False


def probe(path) -> MediaInfo:
    r = run_ff(["-i", file_arg(path)], timeout=30)
    text = r.stderr.decode("utf-8", "replace")
    info = MediaInfo()
    m = re.search(r"Duration: (\d+):(\d+):([\d.]+)", text)
    if m:
        info.duration = int(m[1]) * 3600 + int(m[2]) * 60 + float(m[3])
    rot = 0
    for line in text.splitlines():
        if re.search(r"Stream #\d+:\d+.*: Video:", line) and "attached pic" not in line and not info.has_video:
            info.has_video = True
            d = re.search(r", (\d{2,5})x(\d{2,5})", line)
            if d:
                info.width, info.height = int(d[1]), int(d[2])
            f = re.search(r", ([\d.]+) fps", line)
            if f:
                info.fps = float(f[1])
        elif re.search(r"Stream #\d+:\d+.*: Audio:", line):
            info.has_audio = True
        elif "rotation of" in line and info.has_video and not rot:
            g = re.search(r"rotation of (-?[\d.]+)", line)
            rot = round(float(g[1])) if g else 0
    if rot % 180:
        info.width, info.height = info.height, info.width
    return info


def frame_at(path, t: float, max_w: int = 960, max_h: int = 720) -> QImage:
    vf = f"scale='min({max_w},iw)':'min({max_h},ih)':force_original_aspect_ratio=decrease"
    r = run_ff(["-ss", f"{max(0.0, t):.3f}", "-i", file_arg(path), "-map", "0:v:0", "-frames:v", "1", "-an",
                "-vf", vf, "-f", "image2pipe", "-c:v", "png", "-"], timeout=30)
    img = QImage()
    img.loadFromData(r.stdout, "PNG")
    return img


def waveform(path, duration: float, buckets: int = 1800):
    """Peak level per bucket, 0..1 (numpy array), from a low-rate mono decode."""
    import numpy as np
    rate = 8000 if duration < 600 else max(400, int(8000 * 600 / max(duration, 1)))
    r = run_ff(["-i", file_arg(path), "-map", "0:a:0", "-vn", "-ac", "1", "-ar", str(rate), "-f", "s16le", "-"],
               timeout=600)
    a = np.frombuffer(r.stdout, dtype=np.int16)
    if a.size == 0:
        return None
    a = np.abs(a.astype(np.int32))
    n = min(buckets, a.size)
    idx = np.linspace(0, a.size, n, endpoint=False).astype(np.int64)
    peaks = np.maximum.reduceat(a, idx).astype(np.float32)
    top = float(np.percentile(peaks, 99.5)) or 1.0
    return np.clip(peaks / top, 0, 1) ** 0.8


class _Bus(QObject):
    """Results from worker threads arrive on the UI thread through these signals."""
    image = Signal(int, object)    # tag, QImage
    peaks = Signal(object)
    strip = Signal(int, object)    # index, QImage
    played = Signal(object)        # wav path or None

    def __init__(self):
        super().__init__()
        self.alive = True              # False once its window closed: loops stop early


_STOP = threading.Event()
_THREADS: list[threading.Thread] = []


def background(fn):
    _THREADS[:] = [t for t in _THREADS if t.is_alive()]
    t = threading.Thread(target=fn, daemon=True)
    _THREADS.append(t)
    t.start()


def stop_background(timeout: float = 10.0):
    """Before Qt goes away: let preview threads finish their ffmpeg call instead of having them
    emit into a torn-down application (that crashes the process on exit)."""
    import time
    _STOP.set()
    end = time.monotonic() + timeout
    for t in list(_THREADS):
        t.join(max(0.0, end - time.monotonic()))


def before_exit():
    """Wind down cleanly: preview threads first, then the windows still queued for deletion
    (closed windows delete themselves later; with no event loop left Qt would crash on them)."""
    from PySide6.QtCore import QCoreApplication, QEvent
    stop_background()
    QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)


# ---------------------------------------------------------------- sound preview (no extra libraries)
class Player:
    """Plays a section through winsound: decode it to a temp WAV with ffmpeg, then PlaySound."""

    def __init__(self):
        self.tmp = Path(tempfile.gettempdir()) / f"dragshift-preview-{os.getpid()}-{id(self)}.wav"
        self.started: float | None = None
        self.origin = 0.0
        self.until = 0.0
        self.serial = 0

    def prepare(self, path, start: float, length: float, af: list[str]) -> Path | None:
        args = ["-ss", f"{start:.3f}", "-t", f"{length:.3f}", "-i", file_arg(path), "-map", "0:a:0", "-vn",
                "-ac", "2", "-ar", "44100", "-c:a", "pcm_s16le"]
        if af:
            args += ["-af", ",".join(af)]
        tmp = self.tmp.with_name(self.tmp.stem + f"-{self.serial % 2}.wav")
        r = run_ff(["-y", *args, str(tmp)], timeout=120)
        return tmp if r.returncode == 0 and tmp.exists() else None

    def play(self, wav: Path, origin: float, until: float):
        if not IS_WIN:
            return
        import winsound
        winsound.PlaySound(str(wav), winsound.SND_FILENAME | winsound.SND_ASYNC | winsound.SND_NODEFAULT)
        self.started = time.monotonic()
        self.origin, self.until = origin, until

    def position(self) -> float | None:
        if self.started is None:
            return None
        t = self.origin + time.monotonic() - self.started
        if t >= self.until:
            self.stop()
            return None
        return t

    def stop(self):
        self.serial += 1
        if self.started is not None and IS_WIN:
            import winsound
            winsound.PlaySound(None, 0)
        self.started = None

    def cleanup(self):
        self.stop()
        for i in (0, 1):
            try:
                self.tmp.with_name(self.tmp.stem + f"-{i}.wav").unlink(missing_ok=True)
            except OSError:
                pass


# ---------------------------------------------------------------- timeline
class Timeline(QWidget):
    """Film strip + waveform with start/end handles, a playhead and fade ramps."""
    selection = Signal(float, float)
    scrubbed = Signal(float)
    moved = Signal(float)

    def __init__(self, duration: float, video: bool):
        super().__init__()
        self.duration = max(duration, 0.001)
        self.video = video
        self.start, self.end, self.playhead = 0.0, self.duration, 0.0
        self.fade_in = self.fade_out = 0.0
        self.peaks = None
        self.strip: dict[int, QImage] = {}
        self.strip_n = 10
        self._drag: str | None = None
        self.setMinimumHeight(132 if video else 150)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed if video else QSizePolicy.Expanding)
        self.setMouseTracking(True)
        self.setFocusPolicy(Qt.StrongFocus)

    # geometry
    def lane(self) -> QRectF:
        return QRectF(self.rect()).adjusted(12, 10, -12, -22)

    def x_of(self, t: float) -> float:
        L = self.lane()
        return L.left() + L.width() * (t / self.duration)

    def t_of(self, x: float) -> float:
        L = self.lane()
        return min(max((x - L.left()) / max(1.0, L.width()) * self.duration, 0.0), self.duration)

    def set_range(self, a: float, b: float, emit: bool = True):
        a = min(max(a, 0.0), self.duration)
        b = min(max(b, 0.0), self.duration)
        if b - a < 0.05:
            return
        self.start, self.end = a, b
        self.update()
        if emit:
            self.selection.emit(a, b)

    def set_playhead(self, t: float, emit: bool = False):
        self.playhead = min(max(t, 0.0), self.duration)
        self.update()
        self.moved.emit(self.playhead)
        if emit:
            self.scrubbed.emit(self.playhead)

    # input
    def _hit(self, x: float) -> str | None:
        for name, t in (("start", self.start), ("end", self.end)):
            if abs(x - self.x_of(t)) <= 9:
                return name
        return None

    def mousePressEvent(self, e):
        x = e.position().x()
        self._drag = self._hit(x) or "head"
        if self._drag == "head":
            self.set_playhead(self.t_of(x), emit=True)

    def mouseMoveEvent(self, e):
        x = e.position().x()
        if self._drag is None:
            self.setCursor(Qt.SizeHorCursor if self._hit(x) else Qt.PointingHandCursor)
            return
        t = self.t_of(x)
        if self._drag == "start":
            self.set_range(min(t, self.end - 0.05), self.end)
            self.set_playhead(self.start, emit=True)
        elif self._drag == "end":
            self.set_range(self.start, max(t, self.start + 0.05))
            self.set_playhead(self.end, emit=True)
        else:
            self.set_playhead(t, emit=True)

    def mouseReleaseEvent(self, e):
        self._drag = None

    # painting
    def paintEvent(self, e):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        L = self.lane()
        bg = QPainterPath()
        bg.addRoundedRect(L, 10, 10)
        p.fillPath(bg, glass.color("bg"))
        p.save()
        p.setClipPath(bg)
        wave_top = L.top()
        if self.video:
            sh = L.height() * 0.56
            n = self.strip_n
            for i in range(n):
                cell = QRectF(L.left() + L.width() * i / n, L.top(), L.width() / n + 0.5, sh)
                img = self.strip.get(i)
                if img is not None and not img.isNull():
                    # cover the cell, centre crop
                    s = max(cell.width() / img.width(), cell.height() / img.height())
                    sw, shh = cell.width() / s, cell.height() / s
                    src = QRectF((img.width() - sw) / 2, (img.height() - shh) / 2, sw, shh)
                    p.drawImage(cell, img, src)
                else:
                    p.fillRect(cell.adjusted(1, 1, -1, -1), glass.color("sector"))
            wave_top = L.top() + sh + 4
        wave = QRectF(L.left(), wave_top, L.width(), L.bottom() - wave_top)
        mid = wave.center().y()
        if self.peaks is not None and len(self.peaks):
            n = len(self.peaks)
            cols = max(1, int(wave.width() / 2))
            wave_col = glass.color("accent")
            path = QPainterPath()
            for c in range(cols):
                v = float(self.peaks[min(n - 1, int(c / cols * n))])
                t = c / cols * self.duration
                g = self._gain(t)
                h = max(1.0, v * g * (wave.height() / 2 - 2))
                x = wave.left() + c * wave.width() / cols
                path.addRect(QRectF(x, mid - h, max(1.0, wave.width() / cols - 0.6), 2 * h))
            p.fillPath(path, wave_col)
        else:
            p.setPen(QPen(glass.color("line"), 1, Qt.DashLine))
            p.drawLine(QPointF(wave.left(), mid), QPointF(wave.right(), mid))
        # outside the selection
        xs, xe = self.x_of(self.start), self.x_of(self.end)
        outside = glass.color("bg", 190)
        p.fillRect(QRectF(L.left(), L.top(), xs - L.left(), L.height()), outside)
        p.fillRect(QRectF(xe, L.top(), L.right() - xe, L.height()), outside)
        p.restore()
        # selection frame + handles
        accent = glass.color("accent")
        p.setPen(QPen(accent, 2))
        p.setBrush(Qt.NoBrush)
        p.drawRoundedRect(QRectF(xs, L.top() + 1, xe - xs, L.height() - 2), 6, 6)
        for x in (xs, xe):
            h = QRectF(x - 4, L.center().y() - 16, 8, 32)
            p.setPen(Qt.NoPen)
            p.setBrush(accent)
            p.drawRoundedRect(h, 4, 4)
            p.setBrush(glass.color("on_accent"))
            p.drawRect(QRectF(x - 0.75, L.center().y() - 7, 1.5, 14))
        # playhead
        xp = self.x_of(self.playhead)
        p.setPen(QPen(glass.color("text"), 1.6))
        p.drawLine(QPointF(xp, L.top() - 4), QPointF(xp, L.bottom() + 4))
        p.setPen(Qt.NoPen)
        p.setBrush(glass.color("text"))
        p.drawEllipse(QPointF(xp, L.top() - 4), 3.5, 3.5)
        # time ruler
        f = p.font()
        f.setFamily(ui_family(False))
        f.setPixelSize(10)
        p.setFont(f)
        p.setPen(glass.color("dim"))
        steps = 6
        for i in range(steps + 1):
            t = self.duration * i / steps
            x = self.x_of(t)
            al = Qt.AlignLeft if i == 0 else Qt.AlignRight if i == steps else Qt.AlignHCenter
            w = 60
            r = QRectF(x - (0 if i == 0 else w if i == steps else w / 2), L.bottom() + 4, w, 14)
            p.drawText(r, al | Qt.AlignTop, fmt_time(t, ms=False))

    def _gain(self, t: float) -> float:
        if t < self.start or t > self.end:
            return 1.0
        g = 1.0
        if self.fade_in > 0 and t < self.start + self.fade_in:
            g = min(g, (t - self.start) / self.fade_in)
        if self.fade_out > 0 and t > self.end - self.fade_out:
            g = min(g, (self.end - t) / self.fade_out)
        return max(0.0, g)


class FramePreview(QLabel):
    def __init__(self):
        super().__init__()
        self.setAlignment(Qt.AlignCenter)
        self.setMinimumSize(360, 220)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        self.setStyleSheet(f"border-radius: 12px; background: {glass.pal()['bg']};")
        self.img: QImage | None = None

    def set_image(self, img: QImage):
        if img is None or img.isNull():
            return
        self.img = img
        self._fit()

    def _fit(self):
        if self.img is not None:
            pm = QPixmap.fromImage(self.img)
            self.setPixmap(pm.scaled(self.size() - QSize(8, 8), Qt.KeepAspectRatio, Qt.SmoothTransformation))

    def resizeEvent(self, e):
        super().resizeEvent(e)
        self._fit()


# ---------------------------------------------------------------- base window
class MediaWindow(GlassWindow):
    def __init__(self, title: str, path, emit: Emit, size=(860, 640), info: MediaInfo | None = None):
        super().__init__(tr(title), size=size)
        self.path = Path(path)
        self.emit = emit
        self.info = info or probe(self.path)
        if not (self.info.has_video or self.info.has_audio):
            from .util import UserError
            raise UserError(tr("This file looks damaged, or it isn't a video or audio format DragShift can read."))
        bits = [self.path.name, human_size(file_size(self.path))]
        if self.info.duration:
            bits.append(fmt_time(self.info.duration, ms=False))
        if self.info.has_video and self.info.width:
            bits.append(dims_text(self.info.width, self.info.height))
        self.subtitle = QLabel("  ·  ".join(bits))
        self.subtitle.setObjectName("dim")
        self.body.addWidget(self.subtitle)
        self.bus = _Bus()
        self.primary_btn: QPushButton | None = None
        QShortcut(QKeySequence(Qt.Key_Return), self, activated=self._enter)
        QShortcut(QKeySequence(Qt.Key_Enter), self, activated=self._enter)

    def _enter(self):
        from PySide6.QtWidgets import QApplication
        if isinstance(QApplication.focusWidget(), QLineEdit):
            QApplication.focusWidget().clearFocus()
            return
        if self.primary_btn is not None and self.primary_btn.isEnabled():
            self.primary_btn.click()

    def footer(self, primary: str, on_primary, extra=()) -> QHBoxLayout:
        row = QHBoxLayout()
        for b in extra:
            row.addWidget(b)
        row.addStretch(1)
        cancel = QPushButton(tr("Cancel"))
        cancel.clicked.connect(self.close)
        row.addWidget(cancel)
        ok = QPushButton(tr(primary))
        ok.setObjectName("primary")
        ok.clicked.connect(on_primary)
        row.addWidget(ok)
        self.primary_btn = ok
        return row

    def send(self, kind: str, **params):
        self.emit({"ev": "media", "path": str(self.path), "op": {"kind": kind, **params}})


# ---------------------------------------------------------------- TRIM

    def closeEvent(self, e):
        self.bus.alive = False
        super().closeEvent(e)


class TrimWindow(MediaWindow):
    def __init__(self, path, emit: Emit):
        info = probe(path)
        super().__init__("Trim", path, emit, size=(900, 700) if info.has_video else (860, 470), info=info)
        info = self.info
        if info.duration <= 0:
            from .util import UserError
            raise UserError(tr("DragShift can't tell how long this file is."))
        self.player = Player()
        self.preview: FramePreview | None = None
        if info.has_video:
            self.preview = FramePreview()
            self.body.addWidget(self.preview, 1)
        self.timeline = Timeline(info.duration, info.has_video)
        self.body.addWidget(self.timeline, 0 if info.has_video else 1)

        # times
        row = QHBoxLayout()
        self.start_edit, self.end_edit = QLineEdit(), QLineEdit()
        for ed in (self.start_edit, self.end_edit):
            ed.setFixedWidth(110)
            ed.setAlignment(Qt.AlignCenter)
        b_in = QPushButton(tr("[ Start here"))
        b_in.setToolTip(tr("Set the start to the playhead (I)"))
        b_in.clicked.connect(self.mark_in)
        b_out = QPushButton(tr("End here ]"))
        b_out.setToolTip(tr("Set the end to the playhead (O)"))
        b_out.clicked.connect(self.mark_out)
        self.play_btn = QPushButton(tr("▶ Play"))
        self.play_btn.setToolTip(tr("Listen from the playhead (Space)"))
        self.play_btn.clicked.connect(self.toggle_play)
        self.play_btn.setEnabled(info.has_audio and IS_WIN)
        row.addWidget(QLabel(tr("From")))
        row.addWidget(self.start_edit)
        row.addWidget(b_in)
        row.addSpacing(10)
        row.addWidget(QLabel(tr("to")))
        row.addWidget(self.end_edit)
        row.addWidget(b_out)
        row.addStretch(1)
        self.len_label = QLabel()
        self.len_label.setObjectName("big")
        row.addWidget(self.len_label)
        row.addSpacing(10)
        self.head_label = QLabel()
        self.head_label.setObjectName("dim")
        self.head_label.setMinimumWidth(64)
        self.head_label.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        row.addWidget(self.head_label)
        row.addWidget(self.play_btn)
        self.body.addLayout(row)

        # fades
        grid_row = QHBoxLayout()
        from PySide6.QtWidgets import QGridLayout
        grid = QGridLayout()
        grid.setHorizontalSpacing(12)
        self.fin = slider(0, 100, 0)
        self.fout = slider(0, 100, 0)
        slider_row(grid, 0, "Fade in", self.fin, lambda v: f"{v / 10:.1f} " + tr("s"))
        slider_row(grid, 1, "Fade out", self.fout, lambda v: f"{v / 10:.1f} " + tr("s"))
        grid_row.addLayout(grid, 1)
        self.body.addLayout(grid_row)
        self.fast = None
        if info.has_video:
            self.fast = QCheckBox(tr("Fast cut: no re-encoding, but it starts at the nearest keyframe"))
            self.body.addWidget(self.fast)
        if not info.has_audio:
            self.fin.setEnabled(False)
            self.fout.setEnabled(False)

        extra = []
        if info.has_video:
            snap = QPushButton(tr("Save this frame"))
            snap.setToolTip(tr("Save the frame under the playhead as PNG"))
            snap.clicked.connect(self.save_frame)
            extra.append(snap)
        self.body.addLayout(self.footer("Trim", self.save, extra))

        # wiring
        self.timeline.selection.connect(lambda a, b: self._sync_fields())
        self.timeline.scrubbed.connect(self._scrubbed)
        self.timeline.moved.connect(lambda t: self.head_label.setText("▸ " + fmt_time(t)))
        self.head_label.setText("▸ " + fmt_time(0))
        self.start_edit.editingFinished.connect(self._from_fields)
        self.end_edit.editingFinished.connect(self._from_fields)
        self.fin.valueChanged.connect(self._fades)
        self.fout.valueChanged.connect(self._fades)
        self.bus.image.connect(self._on_frame)
        self.bus.peaks.connect(self._on_peaks)
        self.bus.strip.connect(self._on_strip)
        self.bus.played.connect(self._on_prepared)
        QShortcut(QKeySequence(Qt.Key_Space), self, activated=self.toggle_play)
        QShortcut(QKeySequence(Qt.Key_I), self, activated=self.mark_in)
        QShortcut(QKeySequence(Qt.Key_O), self, activated=self.mark_out)
        QShortcut(QKeySequence(Qt.Key_Left), self, activated=lambda: self._nudge(-1))
        QShortcut(QKeySequence(Qt.Key_Right), self, activated=lambda: self._nudge(1))
        self._tick = QTimer(self, interval=30, timeout=self._advance)
        self._frame_busy = False
        self._frame_want: float | None = None
        self._last_frame_req = 0.0
        self._sync_fields()
        self._load_previews()

    # ---- background loading
    def _load_previews(self):
        path, dur, bus = self.path, self.info.duration, self.bus
        if self.info.has_audio:
            background(lambda: bus.peaks.emit(waveform(path, dur)))
        if self.info.has_video:
            self._request_frame(min(dur * 0.1, 1.0))
            n = self.timeline.strip_n

            def strip():
                for i in range(n):
                    if _STOP.is_set() or not bus.alive:
                        return
                    t = dur * (i + 0.5) / n
                    try:
                        bus.strip.emit(i, frame_at(path, t, 240, 140))
                    except Exception:
                        return
            background(strip)

    def _on_peaks(self, peaks):
        self.timeline.peaks = peaks
        self.timeline.update()

    def _on_strip(self, i, img):
        self.timeline.strip[i] = img
        self.timeline.update()

    def _request_frame(self, t: float):
        if self.preview is None:
            return
        self._frame_want = t
        if self._frame_busy:
            return
        self._frame_busy = True
        path, bus = self.path, self.bus

        def work():
            while True:
                want = self._frame_want
                self._frame_want = None
                if want is None or _STOP.is_set() or not bus.alive:
                    break
                try:
                    bus.image.emit(0, frame_at(path, want))
                except Exception:
                    break
            self._frame_busy = False
        background(work)

    def _on_frame(self, _tag, img):
        if self.preview is not None:
            self.preview.set_image(img)

    # ---- fields
    def _sync_fields(self):
        tl = self.timeline
        self.start_edit.setText(fmt_time(tl.start))
        self.end_edit.setText(fmt_time(tl.end))
        self.len_label.setText(fmt_time(tl.end - tl.start, ms=tl.end - tl.start < 60))
        ok = tl.end - tl.start >= 0.05 and (tl.start > 0.01 or tl.end < tl.duration - 0.01 or
                                            self.fin.value() or self.fout.value())
        if self.primary_btn is not None:
            self.primary_btn.setEnabled(ok)
        limit = int((tl.end - tl.start) * 10 / 2)
        for s in (self.fin, self.fout):
            s.setMaximum(max(0, min(100, limit)))

    def _from_fields(self):
        a, b = parse_time(self.start_edit.text()), parse_time(self.end_edit.text())
        tl = self.timeline
        a = tl.start if a is None else a
        b = tl.end if b is None else b
        if b - a >= 0.05:
            tl.set_range(a, b, emit=False)
        self._sync_fields()

    def _fades(self):
        self.timeline.fade_in = self.fin.value() / 10
        self.timeline.fade_out = self.fout.value() / 10
        self.timeline.update()
        if self.fast is not None:
            has_fades = bool(self.fin.value() or self.fout.value())
            self.fast.setEnabled(not has_fades)
            if has_fades:
                self.fast.setChecked(False)
        self._sync_fields()

    def mark_in(self):
        tl = self.timeline
        if tl.playhead < tl.end - 0.05:
            tl.set_range(tl.playhead, tl.end)

    def mark_out(self):
        tl = self.timeline
        if tl.playhead > tl.start + 0.05:
            tl.set_range(tl.start, tl.playhead)

    def _nudge(self, d: int):
        step = 1 / (self.info.fps or 25) if self.info.has_video else 0.1
        self.timeline.set_playhead(self.timeline.playhead + d * step, emit=True)

    def _scrubbed(self, t: float):
        if self.player.started is not None:
            self.stop()
        self._request_frame(t)

    # ---- playback
    def toggle_play(self):
        if not self.play_btn.isEnabled():
            return
        if self.player.started is not None or self.play_btn.text() == tr("■ Stop"):
            self.stop()
            return
        tl = self.timeline
        origin = tl.playhead if tl.start <= tl.playhead < tl.end - 0.1 else tl.start
        length = min(tl.end - origin, 300.0)
        af = []
        fi, fo = tl.fade_in, tl.fade_out
        if fi > 0 and origin < tl.start + fi:
            done = origin - tl.start
            af.append(f"afade=t=in:st=0:d={max(0.05, fi - done):.3f}")
        if fo > 0:
            st = max(0.0, tl.end - fo - origin)
            af.append(f"afade=t=out:st={st:.3f}:d={min(fo, tl.end - origin):.3f}")
        self.play_btn.setText(tr("■ Stop"))
        serial = self.player.serial
        path, player, bus = self.path, self.player, self.bus

        def work():
            wav = player.prepare(path, origin, length, af)
            bus.played.emit((serial, wav, origin, origin + length))
        background(work)

    def _on_prepared(self, data):
        serial, wav, origin, until = data
        if serial != self.player.serial or wav is None or not self.isVisible():
            if serial == self.player.serial:
                self.play_btn.setText(tr("▶ Play"))
            return
        self.player.play(wav, origin, until)
        self._tick.start()

    def _advance(self):
        t = self.player.position()
        if t is None:
            self.stop()
            return
        self.timeline.set_playhead(t)
        now = time.monotonic()
        if self.preview is not None and now - self._last_frame_req > 0.3:
            self._last_frame_req = now
            self._request_frame(t)

    def stop(self):
        self.player.stop()
        self._tick.stop()
        self.play_btn.setText(tr("▶ Play"))

    # ---- result
    def save(self):
        tl = self.timeline
        self.stop()
        self.send("trim", start=round(tl.start, 3), end=round(tl.end, 3), fade_in=tl.fade_in, fade_out=tl.fade_out,
                  fast=bool(self.fast is not None and self.fast.isChecked()))
        self.close()

    def save_frame(self):
        self.send("frame", at=round(self.timeline.playhead, 3))

    def closeEvent(self, e):
        self.player.cleanup()
        super().closeEvent(e)


# ---------------------------------------------------------------- CROP (video)
class CropVideoWindow(MediaWindow):
    RATIOS = [("Free", None), ("Original", "orig"), ("1:1", 1.0), ("4:3", 4 / 3), ("16:9", 16 / 9),
              ("9:16", 9 / 16), ("4:5", 0.8), ("21:9", 21 / 9)]

    def __init__(self, path, emit: Emit):
        super().__init__("Crop Video", path, emit, size=(900, 720))
        info = self.info
        if not info.has_video or not info.width:
            from .util import UserError
            raise UserError(tr("This file has no video, only sound."))
        self.at = min(info.duration * 0.25, 2.0) if info.duration else 0.0
        img = frame_at(self.path, self.at, 1280, 900)
        if img.isNull():
            from .util import UserError
            raise UserError(tr("This file looks damaged, or it isn't a video or audio format DragShift can read."))
        self.view = CropView(img, info.width / img.width())
        self.body.addWidget(self.view, 1)

        self.body.addWidget(section("Aspect ratio"))
        row, self.grp = toggle_row([n for n, _ in self.RATIOS])
        self.grp.idClicked.connect(self._ratio)
        self.body.addLayout(row)

        pos_row = QHBoxLayout()
        pos_row.addWidget(QLabel(tr("Preview at")))
        self.scrub = slider(0, 1000, int(self.at / max(info.duration, 0.001) * 1000))
        pos_row.addWidget(self.scrub, 1)
        self.at_label = QLabel(fmt_time(self.at, ms=False))
        self.at_label.setObjectName("dim")
        pos_row.addWidget(self.at_label)
        self.out_label = QLabel()
        self.out_label.setObjectName("big")
        pos_row.addSpacing(16)
        pos_row.addWidget(self.out_label)
        self.body.addLayout(pos_row)
        self.body.addLayout(self.footer("Crop", self.save))

        self.view.changed.connect(self._update_label)
        self._update_label()
        self.bus.image.connect(lambda _t, im: self._on_frame(im))
        self._scrub_timer = QTimer(self, singleShot=True, interval=120, timeout=self._load_frame)
        self.scrub.valueChanged.connect(lambda _: self._scrub_timer.start())

    def _ratio(self, i):
        r = self.RATIOS[i][1]
        if r == "orig":
            r = self.view.image.width() / self.view.image.height()
        self.view.set_ratio(r)

    def _box(self) -> tuple[int, int, int, int]:
        s, k = self.view.sel, self.view.k
        W, H = self.info.width, self.info.height
        x, y = round(s.left() * k), round(s.top() * k)
        w, h = round(s.width() * k), round(s.height() * k)
        if x <= 3:
            w, x = w + x, 0
        if y <= 3:
            h, y = h + y, 0
        if abs(x + w - W) <= 3:
            w = W - x
        if abs(y + h - H) <= 3:
            h = H - y
        x, y = x - x % 2, y - y % 2
        w, h = min(w, W - x), min(h, H - y)
        return x, y, w - w % 2, h - h % 2

    def _update_label(self):
        x, y, w, h = self._box()
        self.out_label.setText(dims_text(w, h))
        full = (w, h) == (self.info.width, self.info.height)
        if self.primary_btn is not None:
            self.primary_btn.setEnabled(not full and w >= 16 and h >= 16)

    def _load_frame(self):
        t = self.scrub.value() / 1000 * self.info.duration
        self.at_label.setText(fmt_time(t, ms=False))
        path, bus = self.path, self.bus
        w = self.view.image.width()
        background(lambda: bus.image.emit(0, frame_at(path, t, w, 4000)))

    def _on_frame(self, img: QImage):
        if img.isNull() or img.size() != self.view.image.size():
            return
        sel = QRectF(self.view.sel)
        self.view.set_image(img)
        self.view.sel = sel
        self.view.update()

    def save(self):
        x, y, w, h = self._box()
        self.send("crop", x=x, y=y, w=w, h=h)
        self.close()


# ---------------------------------------------------------------- SPEED
class SpeedWindow(MediaWindow):
    PRESETS = [0.25, 0.5, 0.75, 1.25, 1.5, 2.0, 3.0, 4.0]

    def __init__(self, path, emit: Emit):
        super().__init__("Speed", path, emit, size=(620, 360))
        self.body.addWidget(section("Speed"))
        row, self.grp = toggle_row([f"{p:g}×" for p in self.PRESETS], 5)
        self.grp.idClicked.connect(lambda i: self.sl.setValue(int(self.PRESETS[i] * 100)))
        self.body.addLayout(row)
        from PySide6.QtWidgets import QGridLayout
        grid = QGridLayout()
        self.sl = slider(25, 400, 200)
        slider_row(grid, 0, "Exact", self.sl, lambda v: f"{v / 100:g}×")
        self.body.addLayout(grid)
        self.pitch = QCheckBox(tr("Keep the pitch (voices don't sound like chipmunks)"))
        self.pitch.setChecked(True)
        self.pitch.setEnabled(self.info.has_audio)
        self.body.addWidget(self.pitch)
        self.out = QLabel()
        self.out.setObjectName("big")
        self.body.addWidget(self.out)
        self.body.addStretch(1)
        self.body.addLayout(self.footer("Change speed", self.save))
        self.sl.valueChanged.connect(self._update)
        self._update()

    def factor(self) -> float:
        return self.sl.value() / 100

    def _update(self):
        f = self.factor()
        b = self.grp.checkedButton()
        if b is not None and abs(self.PRESETS[self.grp.id(b)] - f) > 1e-6:
            self.grp.setExclusive(False)
            b.setChecked(False)
            self.grp.setExclusive(True)
        d = self.info.duration
        if d:
            self.out.setText(tr("{}  →  {}", fmt_time(d, ms=False), fmt_time(d / f, ms=False)))
        if self.primary_btn is not None:
            self.primary_btn.setEnabled(abs(f - 1) > 1e-6)

    def save(self):
        self.send("speed", factor=self.factor(), keep_pitch=self.pitch.isChecked())
        self.close()


# ---------------------------------------------------------------- PRESET
def load_presets() -> list[dict]:
    from .settings import Settings
    v = Settings()._load().get("video_presets")
    return [p for p in v if isinstance(p, dict)] if isinstance(v, list) else []


def save_presets(presets: list[dict]):
    from .settings import Settings
    Settings().set("video_presets", presets)


def preset_text(p: dict) -> str:
    """'H.265 · 720p · 2500 kbps · AAC 128k' for the list of saved presets."""
    codec = dict(PresetWindow.VCODECS).get(p.get("vcodec"), "H.265")
    bits = [codec]
    if p.get("height"):
        bits.append(f"{p['height']}p")
    bits.append(tr("{} kbps", p["vkbps"]) if p.get("vkbps") else tr("by quality"))
    audio = dict(PresetWindow.ACODECS).get(p.get("acodec"), "AAC")
    bits.append(tr(audio) if p.get("acodec") == "copy" else f"{audio} {p.get('akbps', 128)}k")
    return " · ".join(bits)


class PresetWindow(MediaWindow):
    """Pick every encoding setting by hand; the preset lands in the video Compress ring."""
    VCODECS = [("hevc", "H.265"), ("h264", "H.264"), ("av1", "AV1"), ("vp9", "VP9")]
    VHINTS = {
        "hevc": "About half the size of H.264 at the same quality. Plays on phones, TVs and Windows 10/11.",
        "h264": "Plays everywhere, even on old devices and in every messenger. Files come out bigger.",
        "av1": "The smallest files, but slow to encode without a new graphics card. Old devices won't play it.",
        "vp9": "Open format for the web (WebM): browsers, YouTube. The sound becomes Opus.",
    }
    ACODECS = [("aac", "AAC"), ("opus", "Opus"), ("mp3", "MP3"), ("copy", "Original")]
    AHINTS = {
        "aac": "The usual sound for MP4. Plays everywhere.",
        "opus": "Best quality per kilobyte: voice and music stay clean at a low bitrate.",
        "mp3": "Old but universal.",
        "copy": "The sound stays as it is, no quality loss and no time spent.",
    }
    VRATES = [0, 500, 1000, 1500, 2500, 4000, 6000, 8000]
    HEIGHTS = [0, 2160, 1440, 1080, 720, 480, 360]
    ARATES = [64, 96, 128, 192, 256, 320]

    def __init__(self, path, emit: Emit):
        super().__init__("New preset", path, emit, size=(700, 720))
        if not self.info.has_video:
            from .util import UserError
            raise UserError(tr("This file has no video, only sound."))
        name_row = QHBoxLayout()
        name_row.addWidget(QLabel(tr("Name")))
        self.name = QLineEdit()
        name_row.addWidget(self.name, 1)
        self.body.addLayout(name_row)

        self.body.addWidget(section("Video codec"))
        row, self.vc = toggle_row([n for _, n in self.VCODECS])
        self.body.addLayout(row)
        self.vhint = self._hint()

        self.body.addWidget(section("Video bitrate"))
        row, self.vr = toggle_row([tr("Auto")] + [f"{r / 1000:g}M" for r in self.VRATES[1:]])
        self.body.addLayout(row)

        short = min(self.info.width, self.info.height) or 10 ** 6
        self.heights = [h for h in self.HEIGHTS if h == 0 or h < short]
        self.body.addWidget(section("Resolution"))
        row, self.res = toggle_row([tr("Original")] + [f"{h}p" for h in self.heights[1:]])
        self.body.addLayout(row)

        self.body.addWidget(section("Audio codec"))
        row, self.ac = toggle_row([n for _, n in self.ACODECS])
        self.body.addLayout(row)
        self.ahint = self._hint()
        self.body.addWidget(section("Audio bitrate"))
        row, self.ar = toggle_row([f"{k}k" for k in self.ARATES], self.ARATES.index(128))
        self.body.addLayout(row)

        self.estimate = QLabel()
        self.estimate.setObjectName("big")
        self.body.addWidget(self.estimate)

        self.saved = QVBoxLayout()
        self.body.addLayout(self.saved)
        self.body.addStretch(1)
        save_only = QPushButton(tr("Only save"))
        save_only.clicked.connect(lambda: self.save(run=False))
        self.body.addLayout(self.footer("Save and compress", lambda: self.save(run=True), extra=[save_only]))

        for g in (self.vc, self.vr, self.res, self.ac, self.ar):
            g.idClicked.connect(lambda _: self._update())
        self._list_saved()
        self._update()

    def _hint(self) -> QLabel:
        l = QLabel()
        l.setObjectName("dim")
        l.setWordWrap(True)
        self.body.addWidget(l)
        return l

    def preset(self) -> dict:
        p = {
            "vcodec": self.VCODECS[self.vc.checkedId()][0],
            "vkbps": self.VRATES[self.vr.checkedId()],
            "height": self.heights[self.res.checkedId()],
            "acodec": self.ACODECS[self.ac.checkedId()][0],
            "akbps": self.ARATES[self.ar.checkedId()],
        }
        p["name"] = self.name.text().strip() or preset_text(p).split(" · ")[0] + (f" {p['height']}p" if p["height"] else "")
        return p

    def _update(self):
        p = self.preset()
        vp9 = p["vcodec"] == "vp9"
        # WebM only takes Opus
        for i, (c, _) in enumerate(self.ACODECS):
            self.ac.button(i).setEnabled(self.info.has_audio and (not vp9 or c in ("opus", "copy")))
        if vp9 and p["acodec"] in ("aac", "mp3"):
            self.ac.button(1).setChecked(True)
            p = self.preset()
        self.vhint.setText(tr(self.VHINTS[p["vcodec"]]))
        self.ahint.setText(tr(self.AHINTS[p["acodec"]]))
        for b in self.ar.buttons():
            b.setEnabled(p["acodec"] != "copy" and self.info.has_audio)
        self.name.setPlaceholderText(p["name"])
        d = self.info.duration
        if p["vkbps"] and d:
            akbps = p["akbps"] if self.info.has_audio and p["acodec"] != "copy" else 0
            est = (p["vkbps"] + akbps) * 1000 / 8 * d
            self.estimate.setText(tr("About {} for this file", human_size(int(est))))
        else:
            self.estimate.setText(tr("The size depends on the picture"))

    def _list_saved(self):
        while self.saved.count():
            item = self.saved.takeAt(0)
            lay = item.layout()
            if lay is not None:
                while lay.count():
                    w = lay.takeAt(0).widget()
                    if w is not None:
                        w.deleteLater()
            elif item.widget() is not None:
                item.widget().deleteLater()
        presets = load_presets()
        if not presets:
            return
        self.saved.addWidget(section("Your presets"))
        for i, p in enumerate(presets):
            row = QHBoxLayout()
            row.addWidget(QLabel(str(p.get("name") or "")))
            info = QLabel(preset_text(p))
            info.setObjectName("dim")
            row.addWidget(info, 1)
            rm = QPushButton(tr("Delete"))
            rm.clicked.connect(lambda _=False, i=i: self._delete(i))
            row.addWidget(rm)
            self.saved.addLayout(row)

    def _delete(self, i: int):
        presets = load_presets()
        if 0 <= i < len(presets):
            del presets[i]
            save_presets(presets)
            self.emit({"ev": "settings"})
        self._list_saved()

    def save(self, run: bool):
        p = self.preset()
        presets = [q for q in load_presets() if {k: v for k, v in q.items() if k != "name"}
                   != {k: v for k, v in p.items() if k != "name"}]
        save_presets((presets + [p])[-12:])
        self.emit({"ev": "settings"})
        if run:
            self.send("encode", **p)
        self.close()


WINDOWS = {"Trim": TrimWindow, "Crop": CropVideoWindow, "Speed": SpeedWindow, "Preset": PresetWindow}

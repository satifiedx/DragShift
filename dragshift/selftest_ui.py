"""`DragShiftUI --selftest`: exercise every library and build every window, then exit.
CI runs it on the frozen build, so a missing module shows up before a release does."""
from __future__ import annotations

import subprocess
import tempfile
import traceback
from pathlib import Path


def check_text_renders():
    """Labels must really draw: paint text with the UI fonts and count the inked pixels."""
    from PySide6.QtCore import Qt
    from PySide6.QtGui import QColor, QImage, QPainter
    from .util import ui_family, ui_font
    for display in (True, False):
        img = QImage(200, 40, QImage.Format_ARGB32_Premultiplied)
        img.fill(0)
        p = QPainter(img)
        p.setFont(ui_font(16, True, display))
        p.setPen(QColor(255, 255, 255))
        p.drawText(img.rect(), Qt.AlignCenter, "WEBP 43.8 KB")
        p.end()
        inked = sum(1 for y in range(0, 40, 2) for x in range(0, 200, 2) if img.pixelColor(x, y).alpha() > 60)
        print(f"font {ui_family(display)!r}: {inked} inked samples")
        if inked < 30:
            raise RuntimeError(f"text does not render with font {ui_family(display)!r}")


def media_samples(d: Path) -> list[Path]:
    from .media_tools import NO_WINDOW, ffmpeg_exe
    clip, tone = d / "clip.mp4", d / "tone.mp3"
    base = [ffmpeg_exe(), "-hide_banner", "-loglevel", "error", "-y"]
    try:
        subprocess.run(base + ["-f", "lavfi", "-i", "testsrc2=s=320x240:r=25:d=3", "-f", "lavfi", "-i",
                               "sine=f=440:d=3", "-c:v", "mpeg4", "-c:a", "aac", "-shortest", str(clip)],
                       check=True, timeout=60, creationflags=NO_WINDOW)
        subprocess.run(base + ["-f", "lavfi", "-i", "sine=f=660:d=2", str(tone)], check=True, timeout=60,
                       creationflags=NO_WINDOW)
    except (OSError, subprocess.SubprocessError) as e:
        print(f"no ffmpeg, media windows skipped: {e}")
        return []
    return [clip, tone]


def run() -> int:
    from PIL import Image
    from . import convert, imaging, media_tools, tools
    from .settings import Settings, SettingsWindow, WelcomeWindow
    try:
        with tempfile.TemporaryDirectory() as tmp:
            d = Path(tmp)
            src = d / "t.png"
            Image.new("RGBA", (64, 48), (255, 100, 80, 255)).save(src)
            for t in convert.targets_for("image", ".png"):
                convert.convert(src, "image", t)
            pdf = convert.convert(src, "image", "PDF")[0]
            for t in convert.PDF_TARGETS:
                convert.convert(pdf, "pdf", t)
            (d / "n.txt").write_text("hello")
            docx = convert.convert(d / "n.txt", "text", "DOCX")[0]
            convert.convert(docx, "docx", "TXT")
            imaging.add_background(convert.open_image(src), imaging.Background())
            imaging.compress_pdf(pdf)
            imaging.protect_pdf(pdf, "pw")
            n = lambda *a, **k: None
            windows = [tools.CropWindow(str(src), n), tools.AddBgWindow(str(src), n), tools.EditWindow(str(src), n),
                       tools.AnnotateWindow(str(src), n), tools.RedactWindow(str(src), n),
                       tools.MetadataWindow(str(src), n), tools.CompressWindow([str(src)], n),
                       tools.ResizeWindow([str(src)], n), tools.ProtectWindow(str(pdf), n),
                       SettingsWindow(Settings()), WelcomeWindow(Settings())]
            got: list[dict] = []
            for m in media_samples(d):
                windows.append(media_tools.TrimWindow(m, got.append))
                windows.append(media_tools.SpeedWindow(m, got.append))
                if m.suffix == ".mp4":
                    windows.append(media_tools.CropVideoWindow(m, got.append))
                    if media_tools.frame_at(m, 1.0).isNull():
                        raise RuntimeError("no frame from ffmpeg")
                    if media_tools.waveform(m, 3.0) is None:
                        raise RuntimeError("no waveform from ffmpeg")
            for w in windows:
                w.close()
            check_text_renders()
        print("SELFTEST OK")
        return 0
    except Exception:
        traceback.print_exc()
        print("SELFTEST FAILED")
        return 1

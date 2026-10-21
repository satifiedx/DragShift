"""The matte look shared by every window: espresso + terracotta, dark or light like Windows
(or as pinned in Settings). Same tokens as native/src/theme.rs. The module keeps its old name
and painter names so the rest of the code didn't have to move."""
from __future__ import annotations

import os
import re
import sys

from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import QBrush, QColor, QImage, QPainter, QPainterPath, QPen

DARK = dict(bg="#211D1B", panel="#2D2826", sector="#383230", raised="#443D3A", line="#4A423E",
            text="#EDE6DF", dim="#A89D94", accent="#C98B6B", accent_hi="#D59C7E", on_accent="#211D1B",
            good="#9DB38A", warn="#D6B26E", bad="#C47A7A", shade="#000000")
LIGHT = dict(bg="#F4EFEA", panel="#EBE4DD", sector="#E2D9D0", raised="#D8CDC2", line="#D2C6BA",
             text="#2B2420", dim="#74675E", accent="#B8724F", accent_hi="#A86544", on_accent="#FFF8F2",
             good="#6F8A5B", warn="#A8853E", bad="#B05A5A", shade="#2B2420")

_PAL: dict | None = None


def _system_light() -> bool:
    if sys.platform == "win32":
        try:
            import winreg
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                                r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize") as k:
                return winreg.QueryValueEx(k, "AppsUseLightTheme")[0] == 1
        except OSError:
            return False
    try:
        from PySide6.QtGui import QGuiApplication
        return QGuiApplication.styleHints().colorScheme() == Qt.ColorScheme.Light
    except Exception:
        return False


def pal() -> dict:
    """The current palette (cached; `refresh()` after the theme setting changes)."""
    global _PAL
    if _PAL is None:
        from .settings import Settings
        theme = Settings().get("theme")
        light = theme == "light" or (theme not in ("dark", "light") and _system_light())
        _PAL = LIGHT if light else DARK
    return _PAL


def is_light() -> bool:
    return pal() is LIGHT


def color(name: str, alpha: int = 255) -> QColor:
    c = QColor(pal()[name])
    c.setAlpha(alpha)
    return c


def refresh_palette():
    """Forget the cached palette, so the next use reads the setting (and Windows) again."""
    global _PAL
    _PAL = None


def refresh():
    """Theme setting changed: restyle every open window."""
    refresh_palette()
    from PySide6.QtWidgets import QApplication, QWidget
    for w in QApplication.topLevelWidgets():
        if w.objectName() == "glassRoot":
            w.setStyleSheet(qss())
            for child in w.findChildren(QWidget):
                child.update()
            w.update()


def iridescent_pen(center: QPointF, width: float, alpha: int = 200, angle: float = 30) -> QPen:
    """Kept for old callers: a plain hairline in the rim colour now."""
    return QPen(color("line", max(alpha, 160)), width)


def iridescent_brush(rect: QRectF, alpha: int = 60) -> QBrush:
    return QBrush(color("accent", alpha))


def paint_glass(p: QPainter, path: QPainterPath, backdrop=None,
                backdrop_offset: QPointF = QPointF(0, 0), tint: QColor | None = None,
                rim: bool = True, rainbow_alpha: int = 170, highlight: float = 1.0) -> None:
    """Paint a matte surface filling `path`: one flat colour and a hairline rim. (The old
    glass arguments are accepted and ignored, apart from an opaque `tint`.)"""
    p.save()
    p.setRenderHint(QPainter.Antialiasing)
    fill = tint if tint is not None and tint.alpha() == 255 else color("panel")
    p.fillPath(path, fill)
    if rim:
        p.setPen(QPen(color("line"), 1.0))
        p.setBrush(Qt.NoBrush)
        p.drawPath(path)
    p.restore()


# ---------------------------------------------------------------- Qt stylesheet
_QSS = """
* { color: @text; font-family: "@FONT", sans-serif; font-size: 10pt; }
QWidget#glassRoot { background: transparent; }
QLabel#title { font-size: 11pt; font-weight: 600; }
QLabel#dim, QLabel.dim { color: @dim; font-size: 9pt; }
QLabel#section { color: @dim; font-size: 8.5pt; font-weight: 600; letter-spacing: 1px; }
QPushButton {
  background: @sector; border: none; border-radius: 8px; padding: 6px 15px; min-height: 18px;
}
QPushButton:hover { background: @raised; }
QPushButton:pressed { background: @line; }
QPushButton:disabled { color: @dim; }
QPushButton:checked { background: @accent; color: @on_accent; }
QPushButton#primary {
  background: @accent; color: @on_accent; font-weight: 600; padding: 7px 20px; border-radius: 8px;
}
QPushButton#primary:hover { background: @accent_hi; }
QPushButton#chrome { background: transparent; color: @dim; border: none; padding: 2px 8px; font-size: 11pt; border-radius: 6px; }
QPushButton#chrome:hover { background: @raised; color: @text; }
QPushButton#swatch { border-radius: 12px; padding: 0; min-width: 24px; max-width: 24px; min-height: 24px; max-height: 24px; }
QPushButton#swatch:checked { border: 2px solid @text; }
QSlider::groove:horizontal { height: 4px; background: @line; border-radius: 2px; }
QSlider::sub-page:horizontal { background: @accent; border-radius: 2px; }
QSlider::handle:horizontal { background: @text; width: 14px; height: 14px; margin: -5px 0; border-radius: 7px; }
QSlider::handle:horizontal:hover { background: @accent; }
QSpinBox, QDoubleSpinBox, QLineEdit, QComboBox {
  background: @bg; border: 1px solid @line; border-radius: 8px; padding: 4px 8px;
  selection-background-color: @accent; selection-color: @on_accent;
}
QLineEdit:focus, QSpinBox:focus, QDoubleSpinBox:focus, QComboBox:focus { border: 1px solid @accent; }
QComboBox::drop-down { border: none; width: 18px; }
QAbstractSpinBox::up-button, QAbstractSpinBox::down-button { width: 0; border: none; }
QFrame#card { background: @bg; border: 1px solid @line; border-radius: 12px; }
QLabel#big { font-size: 17pt; font-weight: 700; }
QLabel#good { color: @good; font-weight: 600; }
QLabel#warn { color: @warn; font-weight: 600; }
QComboBox QAbstractItemView { background: @panel; border: 1px solid @line; selection-background-color: @raised; selection-color: @text; outline: none; }
QCheckBox::indicator { width: 16px; height: 16px; border-radius: 4px; border: 1px solid @line; background: @bg; }
QCheckBox::indicator:hover { border: 1px solid @accent; }
QCheckBox::indicator:checked { background: @accent; border: 1px solid @accent; }
QTableWidget { background: @bg; border: 1px solid @line; border-radius: 8px; gridline-color: @line; }
QHeaderView { background: transparent; }
QTableCornerButton::section { background: transparent; border: none; }
QHeaderView::section { background: @sector; border: none; padding: 4px; font-weight: 600; }
QScrollBar:vertical { background: transparent; width: 8px; }
QScrollBar::handle:vertical { background: @raised; border-radius: 4px; min-height: 30px; }
QScrollBar::add-line, QScrollBar::sub-line { height: 0; }
QToolTip { background: @panel; color: @text; border: 1px solid @line; border-radius: 6px; padding: 4px; }
QMenu { background: @panel; border: 1px solid @line; border-radius: 8px; padding: 4px; }
QMenu::item { padding: 6px 18px; border-radius: 5px; }
QMenu::item:selected { background: @raised; }
"""


def qss() -> str:
    """The stylesheet in the current palette, with a font family that is really installed."""
    from .util import ui_family
    p = {**pal(), "FONT": ui_family(False)}
    return re.sub(r"@(\w+)", lambda m: p.get(m.group(1), m.group(0)), _QSS)


def __getattr__(name):        # keep `glass.QSS` working
    if name == "QSS":
        return qss()
    raise AttributeError(name)


# ---------------------------------------------------------------- helpers
def qimage_from_pil(im) -> QImage:
    im = im.convert("RGBA")
    data = im.tobytes("raw", "RGBA")
    qi = QImage(data, im.width, im.height, im.width * 4, QImage.Format_RGBA8888)
    return qi.copy()  # detach from the Python buffer


def pil_from_qimage(qi: QImage):
    from PIL import Image
    qi = qi.convertToFormat(QImage.Format_RGBA8888)
    ptr = qi.constBits()
    return Image.frombuffer("RGBA", (qi.width(), qi.height()), bytes(ptr), "raw", "RGBA",
                            qi.bytesPerLine(), 1).copy()


def rounded_rect_path(r: QRectF, radius: float) -> QPainterPath:
    path = QPainterPath()
    path.addRoundedRect(r, radius, radius)
    return path


# ---------------------------------------------------------------- no-compositor mode (XFCE etc.)
_NO_COMP = None


def no_compositor() -> bool:
    """Linux/X11 without a compositing manager: translucency would render black, so rounded
    surfaces are cut to their exact shape with a window mask instead (XShape)."""
    global _NO_COMP
    if _NO_COMP is None:
        _NO_COMP = False
        if sys.platform.startswith("linux") and not os.environ.get("QT_QPA_PLATFORM", "").startswith("offscreen"):
            try:
                from . import platform_linux
                _NO_COMP = not platform_linux.composited()
            except Exception:
                _NO_COMP = False
    return _NO_COMP


def apply_shape(widget, path: QPainterPath):
    """Clip a top-level window to `path` when there's no compositor (no-op otherwise)."""
    if not no_compositor():
        return
    from PySide6.QtGui import QRegion
    region = QRegion()
    for poly in path.simplified().toFillPolygons():   # one polygon per separate piece
        region = region.united(QRegion(poly.toPolygon()))
    widget.setMask(region)

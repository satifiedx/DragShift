"""The radial drop menu that pops up under the cursor while you Shift+drag a file.

Flat matte petals around a hub (same look as the native menu in native/src/menu.rs). The
shape and its soft shadow are drawn once per petal count and cached; every frame only draws
that, the hovered petal and the labels. Animations are time-based and tick at the monitor's
refresh rate, so they look the same at 60 Hz and 240 Hz."""
from __future__ import annotations

import math
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

from PySide6.QtCore import QElapsedTimer, QPoint, QPointF, QRectF, Qt, QTimer, Signal
from PySide6.QtGui import (QColor, QCursor, QFont, QGuiApplication, QImage, QPainter, QPainterPath,
                           QPainterPathStroker, QPen)
from PySide6.QtWidgets import QWidget

from . import glass
from .util import frame_ms, resolve_shortcuts, ui_family


@dataclass
class Item:
    label: str
    glyph: str = ""
    run: Callable[[list[str]], None] | None = None
    key: str = ""
    hint: str = ""


SIZE = 400          # window size (room for shadow + hover pop)
R_OUT = 158         # outer radius of petals
R_IN = 60           # inner radius of petals
R_HUB = 50          # hub radius
GAP_DEG = 3.2
CORNER = 10
POP_SECS = 0.19
SWAP_SECS = 0.22


def sector_path(c: QPointF, r0: float, r1: float, a0: float, a1: float) -> QPainterPath:
    """Annular sector between angles a0..a1 (degrees, Qt convention: 0 = 3 o'clock, CCW)."""
    path = QPainterPath()
    outer = QRectF(c.x() - r1, c.y() - r1, 2 * r1, 2 * r1)
    inner = QRectF(c.x() - r0, c.y() - r0, 2 * r0, 2 * r0)
    path.arcMoveTo(outer, a0)
    path.arcTo(outer, a0, a1 - a0)
    path.arcTo(inner, a1, -(a1 - a0))
    path.closeSubpath()
    return path


def angles(n: int, i: int) -> tuple[float, float]:
    span = 360.0 / n
    start = 90 + span / 2 - (i + 1) * span   # first petal centred at 12 o'clock, clockwise
    return start + GAP_DEG / 2, start + span - GAP_DEG / 2


def ease_out_back(x: float) -> float:
    c1 = 1.70158
    return 1 + (c1 + 1) * (x - 1) ** 3 + c1 * (x - 1) ** 2


def ease_out_cubic(x: float) -> float:
    return 1 - (1 - x) ** 3


def approach(v: float, target: float, rate: float, dt: float) -> float:
    """Exponential approach that looks the same at any refresh rate (`rate` per 1/60 s)."""
    nv = v + (target - v) * (1 - (1 - rate) ** (dt * 60))
    return target if abs(nv - target) < 0.002 else nv


def mix(a: QColor, b: QColor, t: float) -> QColor:
    t = min(max(t, 0.0), 1.0)
    return QColor.fromRgbF(a.redF() + (b.redF() - a.redF()) * t, a.greenF() + (b.greenF() - a.greenF()) * t,
                           a.blueF() + (b.blueF() - a.blueF()) * t, a.alphaF() + (b.alphaF() - a.alphaF()) * t)


_PETALS: dict[int, list[QPainterPath]] = {}


def rounded_sector(c: QPointF, r0: float, r1: float, a0: float, a1: float, k: float) -> QPainterPath:
    """Annular sector with its corners rounded by `k`: inset by k, then grown back with round joins."""
    ro, ri = r1 - k, r0 + k
    do, di = math.degrees(math.asin(k / ro)), math.degrees(math.asin(k / ri))
    outer = QRectF(c.x() - ro, c.y() - ro, 2 * ro, 2 * ro)
    inner = QRectF(c.x() - ri, c.y() - ri, 2 * ri, 2 * ri)
    core = QPainterPath()
    core.arcMoveTo(outer, a0 + do)
    core.arcTo(outer, a0 + do, (a1 - do) - (a0 + do))
    core.arcTo(inner, a1 - di, -((a1 - di) - (a0 + di)))
    core.closeSubpath()
    st = QPainterPathStroker()
    st.setWidth(2 * k)
    st.setJoinStyle(Qt.RoundJoin)
    st.setCapStyle(Qt.RoundCap)
    return st.createStroke(core).united(core).simplified()


def petals(n: int) -> list[QPainterPath]:
    if n not in _PETALS:
        c = QPointF(SIZE / 2, SIZE / 2)
        _PETALS[n] = [rounded_sector(c, R_IN, R_OUT, *angles(n, i), CORNER / 2) for i in range(n)]
    return _PETALS[n]


def menu_shape(n: int) -> QImage:
    """Hub + `n` petals with a soft shadow, in the current palette."""
    pal = glass.color
    c = QPointF(SIZE / 2, SIZE / 2)
    body = QPainterPath()
    body.addEllipse(c, R_HUB, R_HUB)
    for path in petals(n):
        body.addPath(path)
    # shadow: the silhouette, blurred by scaling down and up again, nudged down
    sh = QImage(SIZE // 4, SIZE // 4, QImage.Format_ARGB32_Premultiplied)
    sh.fill(0)
    p = QPainter(sh)
    p.setRenderHint(QPainter.Antialiasing)
    p.scale(0.25, 0.25)
    p.fillPath(body, pal("shade", int(255 * (0.20 if glass.is_light() else 0.40))))
    p.end()
    for _ in range(2):
        sh = sh.scaled(sh.width() // 2, sh.height() // 2, Qt.IgnoreAspectRatio, Qt.SmoothTransformation) \
               .scaled(sh.width(), sh.height(), Qt.IgnoreAspectRatio, Qt.SmoothTransformation)
    img = QImage(SIZE, SIZE, QImage.Format_ARGB32_Premultiplied)
    img.fill(0)
    p = QPainter(img)
    p.setRenderHint(QPainter.Antialiasing)
    p.setRenderHint(QPainter.SmoothPixmapTransform)
    p.drawImage(QRectF(-12, -6, SIZE + 24, SIZE + 24), sh)
    hub = QPainterPath()
    hub.addEllipse(c, R_HUB, R_HUB)
    for path in petals(n):
        p.fillPath(path, pal("sector"))
    p.fillPath(hub, pal("bg"))
    p.setPen(QPen(pal("line", 230), 1.0))
    p.setBrush(Qt.NoBrush)
    for path in petals(n):
        p.drawPath(path)
    p.drawEllipse(c, R_HUB - 0.5, R_HUB - 0.5)
    p.end()
    return img


class RadialMenu(QWidget):
    activated = Signal(object, list)     # Item, paths
    dismissed = Signal()

    def __init__(self, build_rings: Callable[[list[str]], dict[str, list[Item]]],
                 describe: Callable[[list[str]], str]):
        super().__init__(None)
        self.build_rings = build_rings
        self.describe = describe
        self.setWindowFlags(Qt.FramelessWindowHint | Qt.Tool | Qt.WindowStaysOnTopHint |
                            Qt.WindowDoesNotAcceptFocus | Qt.NoDropShadowWindowHint)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.setAttribute(Qt.WA_ShowWithoutActivating)
        self.setAcceptDrops(True)
        self.setMouseTracking(True)
        self.resize(SIZE, SIZE)

        self.paths: list[str] = []
        self.rings: dict[str, list[Item]] = {}
        self.ring_names: list[str] = []
        self.ring_idx = 0
        self.hover = -1          # petal index, -2 = hub, -1 = none
        self.cursor_pos = QPointF(SIZE / 2, SIZE / 2)
        self.hub_text = ""
        self.shapes: dict[tuple[int, bool], QImage] = {}   # (petal count, light) -> shape
        self.got_drag = False
        self.click_mode = False  # opened from tray/menu instead of a drag
        self._t = 1.0            # pop-in animation 0..1 (eased)
        self._swap = 1.0         # ring-swap animation 0..1 (eased)
        self.spring: list[float] = []   # per-petal hover amount 0..1
        self.hub_spring = 0.0

        self.clock = QElapsedTimer()
        self._pop_at = 0.0
        self._swap_at = -1.0
        self._last = 0.0
        self.frame = QTimer(self, timeout=self._tick)
        self.frame.setTimerType(Qt.PreciseTimer)

    # ------------------------------------------------------------ show / hide
    def popup(self, pos: QPoint | None = None, paths: list[str] | None = None):
        pos = pos or QCursor.pos()
        tl = pos - QPoint(SIZE // 2, SIZE // 2)
        scr = QGuiApplication.screenAt(pos) or QGuiApplication.primaryScreen()
        g = scr.geometry()
        tl.setX(min(max(tl.x(), g.left()), g.right() - SIZE))
        tl.setY(min(max(tl.y(), g.top()), g.bottom() - SIZE))
        glass.refresh_palette()
        self.move(tl)
        self.hover = -1
        self.got_drag = False
        self.click_mode = paths is not None
        early = []
        if paths is None and sys.platform == "darwin":
            from . import platform_mac
            early = platform_mac.drag_files()
            if early:
                print(f"menu: {len(early)} file(s) from the drag pasteboard", flush=True)
        if paths is not None:
            self._load(paths)
        elif early:
            self._load(early)           # slices show at once; drop still comes via dragEnter
        else:
            self.paths, self.rings, self.ring_names = [], {}, []
            self.hub_text = "Drop here"
        self._t = 0.0
        self._swap = 1.0
        self._swap_at = -1.0
        self._shape()
        self.show()
        self.raise_()
        self.clock.start()
        self._pop_at = self._last = 0.0
        self.frame.setInterval(frame_ms(scr))
        self.frame.start()
        if self.click_mode:
            # behave like a popup: a click anywhere outside dismisses it
            QTimer.singleShot(0, self.grabMouse)

    def close_menu(self):
        if self.isVisible():
            self.hide()
            self.frame.stop()
            self.dismissed.emit()

    def hideEvent(self, e):
        self.frame.stop()
        if QWidget.mouseGrabber() is self:
            self.releaseMouse()
        super().hideEvent(e)

    def _shape_for(self, n: int) -> QImage:
        key = (n, glass.is_light())
        img = self.shapes.get(key)
        if img is None:
            img = self.shapes[key] = menu_shape(n)
        return img

    def _load(self, paths: list[str]):
        paths = resolve_shortcuts(paths)
        self.paths = paths
        self.rings = {k: v for k, v in self.build_rings(paths).items() if v}
        self.ring_names = list(self.rings)
        self.ring_idx = 0
        self.hub_text = self.describe(paths)
        self.spring = [0.0] * len(self.items)
        self._shape()
        self.update()

    def _shape(self):
        """No compositor (XFCE etc.): cut the window to the menu's outline."""
        if not glass.no_compositor():
            return
        c = QPointF(SIZE / 2, SIZE / 2)
        path = QPainterPath()
        path.addEllipse(c, R_HUB + 2, R_HUB + 2)
        n = len(self.items) or 0
        if n:
            for i in range(n):
                a0, a1 = angles(n, i)
                path = path.united(sector_path(c, R_IN - 2, R_OUT + 14, a0 - 1, a1 + 1))
        else:
            outer = QPainterPath()
            outer.addEllipse(c, R_OUT + 14, R_OUT + 14)
            path = path.united(outer)       # united, not added: odd-even fill would punch a hole
        glass.apply_shape(self, path)

    def next_ring(self):
        """One extra Shift press = one switch (nothing flips on its own)."""
        if len(self.ring_names) > 1:
            self.ring_idx = (self.ring_idx + 1) % len(self.ring_names)
            self.hover = -1
            self.spring = [0.0] * len(self.items)
            self._swap = 0.0
            self._swap_at = self.clock.elapsed() / 1000 if self.clock.isValid() else 0.0
            self._shape()
            if self.isVisible() and not self.frame.isActive():
                self.frame.start()
            self.update()

    def _tick(self):
        now = self.clock.elapsed() / 1000
        dt = min(now - self._last, 0.1)
        self._last = now
        moved = False
        p = min((now - self._pop_at) / POP_SECS, 1.0)
        if self._t != 1.0:
            self._t = ease_out_back(p) if p < 1 else 1.0
            moved = True
        if self._swap_at >= 0 and self._swap != 1.0:
            q = min((now - self._swap_at) / SWAP_SECS, 1.0)
            self._swap = ease_out_cubic(q) if q < 1 else 1.0
            moved = True
        for i in range(len(self.spring)):
            target = 1.0 if i == self.hover else 0.0
            nv = approach(self.spring[i], target, 0.30, dt)
            if nv != self.spring[i]:
                self.spring[i] = nv
                moved = True
        ht = 1.0 if self.hover == -2 and self.click_mode and len(self.ring_names) > 1 else 0.0
        nh = approach(self.hub_spring, ht, 0.30, dt)
        if nh != self.hub_spring:
            self.hub_spring = nh
            moved = True
        if moved:
            self.update()

    # ------------------------------------------------------------ geometry
    @property
    def items(self) -> list[Item]:
        if not self.ring_names:
            return []
        return self.rings[self.ring_names[self.ring_idx]]

    def _angles(self, n: int, i: int) -> tuple[float, float]:
        return angles(n, i)

    def hit(self, pos: QPointF) -> int:
        c = QPointF(SIZE / 2, SIZE / 2)
        dx, dy = pos.x() - c.x(), c.y() - pos.y()
        d = math.hypot(dx, dy)
        if d <= R_HUB + 4:
            return -2
        if d > R_OUT + 30 or not self.items:
            return -1
        ang = math.degrees(math.atan2(dy, dx)) % 360
        n = len(self.items)
        rel = (90 + 180.0 / n - ang) % 360
        return int(rel // (360.0 / n)) % n

    def _set_hover(self, h: int, pos: QPointF | None = None):
        if pos is not None:
            self.cursor_pos = QPointF(pos)
        if h != self.hover:
            self.hover = h
        self.update()

    # ------------------------------------------------------------ drag & drop
    @staticmethod
    def _local_paths(md) -> list[str]:
        if not md.hasUrls():
            return []
        return [u.toLocalFile() for u in md.urls() if u.isLocalFile() and u.toLocalFile()]

    def dragEnterEvent(self, e):
        paths = self._local_paths(e.mimeData())
        if not paths:
            e.ignore()
            return
        self.got_drag = True
        if resolve_shortcuts(paths) != self.paths:
            self._load(paths)
        e.setDropAction(Qt.CopyAction)  # never let a Shift(=move) drop delete the source
        e.accept()

    def dragMoveEvent(self, e):
        self._set_hover(self.hit(e.position()), e.position())
        e.setDropAction(Qt.CopyAction)
        e.accept()

    def dragLeaveEvent(self, e):
        self._set_hover(-1)

    def dropEvent(self, e):
        idx = self.hit(e.position())
        items = self.items
        if 0 <= idx < len(items):
            e.setDropAction(Qt.CopyAction)
            e.accept()
            item, paths = items[idx], list(self.paths)
            self.hide()
            # return to Explorer's drag loop immediately; do the work after
            QTimer.singleShot(0, lambda: self.activated.emit(item, paths))
        else:
            e.ignore()
            self.close_menu()

    # ------------------------------------------------------------ click mode
    def mouseMoveEvent(self, e):
        if self.click_mode:
            self._set_hover(self.hit(e.position()), e.position())

    def mouseReleaseEvent(self, e):
        if not self.click_mode:
            return
        if e.button() == Qt.RightButton:
            self.close_menu()
            return
        idx = self.hit(e.position())
        if idx == -2:
            self.next_ring()
        elif 0 <= idx < len(self.items):
            item, paths = self.items[idx], list(self.paths)
            self.hide()
            QTimer.singleShot(0, lambda: self.activated.emit(item, paths))
        else:
            self.close_menu()

    def keyPressEvent(self, e):
        if e.key() == Qt.Key_Escape:
            self.close_menu()
        elif e.key() in (Qt.Key_Tab, Qt.Key_Shift):
            self.next_ring()

    def leaveEvent(self, e):
        if self.click_mode:
            self._set_hover(-1)

    # ------------------------------------------------------------ painting
    def paintEvent(self, e):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        p.setRenderHint(QPainter.TextAntialiasing)
        p.setRenderHint(QPainter.SmoothPixmapTransform)
        c = QPointF(SIZE / 2, SIZE / 2)
        t = self._t
        p.setOpacity(min(1.0, max(0.0, t * 1.5)))
        p.translate(c)
        s = 0.86 + 0.14 * t
        p.scale(s, s)
        p.translate(-c)

        items = self.items
        n = len(items)
        p.drawImage(0, 0, self._shape_for(n))

        rot = (1 - self._swap) * -18
        for i, it in enumerate(items):
            a0, a1 = angles(n, i)
            h = self.spring[i] if i < len(self.spring) else 0.0
            if h > 0.001:
                self._paint_hover(p, c, n, i, h)
            self._petal_label(p, c, it, (a0 + a1) / 2 + rot, (R_IN + R_OUT) / 2, h)
        if not n and self.paths:
            self._paint_unsupported(p, c)
        self._paint_hub(p, c)

    def _paint_unsupported(self, p: QPainter, c: QPointF):
        """Nothing to offer: say so in the top band, clear of the cursor and the drag image."""
        ext = Path(self.paths[0]).suffix.lower() or "this file"
        f = QFont(ui_family(True))
        f.setPixelSize(13)
        f.setBold(True)
        p.setFont(f)
        r_mid = (R_IN + R_OUT) / 2
        p.setPen(glass.color("text"))
        p.drawText(QRectF(c.x() - R_OUT, c.y() - r_mid - 18, 2 * R_OUT, 18), Qt.AlignCenter, f"Can't convert {ext}")
        f.setPixelSize(10)
        f.setBold(False)
        p.setFont(f)
        p.setPen(glass.color("dim"))
        p.drawText(QRectF(c.x() - R_OUT, c.y() - r_mid + 1, 2 * R_OUT, 16), Qt.AlignCenter,
                   "Images · PDF · Word · Text")

    def _paint_hover(self, p: QPainter, c: QPointF, n: int, i: int, h: float):
        a0, a1 = angles(n, i)
        mid = math.radians((a0 + a1) / 2)
        r_mid = (R_IN + R_OUT) / 2
        pc = QPointF(c.x() + r_mid * math.cos(mid), c.y() - r_mid * math.sin(mid))
        k = 1 + 0.05 * h                     # grows a little and moves out towards the cursor
        push = 6 * h
        off = QPointF(push * math.cos(mid), -push * math.sin(mid))
        p.save()
        p.translate(pc + off)
        p.scale(k, k)
        p.translate(-pc)
        p.fillPath(petals(n)[i], mix(glass.color("sector"), glass.color("accent"), h))
        p.restore()

    def _paint_hub(self, p: QPainter, c: QPointF):
        pal = glass.color
        if self.hub_spring > 0.001:
            p.setPen(QPen(pal("accent", int(255 * self.hub_spring)), 1.6))
            p.setBrush(Qt.NoBrush)
            p.drawEllipse(c, R_HUB - 3, R_HUB - 3)
        items = self.items
        hov = items[self.hover] if 0 <= self.hover < len(items) else None
        sub_col = pal("dim")
        if hov is not None:
            top, sub = hov.label if not hov.glyph else hov.label.title(), hov.hint or ""
        else:
            ring_name = self.ring_names[self.ring_idx] if self.ring_names else ""
            top = self.hub_text
            if len(self.ring_names) > 1:
                other = self.ring_names[(self.ring_idx + 1) % len(self.ring_names)]
                sub = f"⇧ {other.upper()}"
                sub_col = pal("accent")
            else:
                sub = ring_name.upper()
        f = QFont(ui_family(True))
        f.setPixelSize(14)
        f.setBold(True)
        p.setFont(f)
        p.setPen(pal("text"))
        p.drawText(QRectF(c.x() - R_HUB + 4, c.y() - 21, 2 * R_HUB - 8, 20), Qt.AlignCenter, top)
        f.setPixelSize(10)
        f.setBold(False)
        p.setFont(f)
        p.setPen(sub_col)
        p.drawText(QRectF(c.x() - R_HUB + 6, c.y() + 2, 2 * R_HUB - 12, 28),
                   Qt.AlignHCenter | Qt.AlignTop | Qt.TextWordWrap, sub)

    def _petal_label(self, p: QPainter, c: QPointF, it: Item, ang: float, r: float, h: float):
        rad = math.radians(ang)
        r += 6 * h
        pt = QPointF(c.x() + r * math.cos(rad), c.y() - r * math.sin(rad))
        p.save()
        p.setOpacity(p.opacity() * (0.2 + 0.8 * self._swap))
        col = mix(glass.color("text"), glass.color("on_accent"), h)
        dim = mix(glass.color("dim"), glass.color("on_accent"), h)
        f = QFont(ui_family(True))
        if it.glyph:
            f.setPixelSize(18)
            p.setFont(f)
            p.setPen(col)
            p.drawText(QRectF(pt.x() - 30, pt.y() - 24, 60, 24), Qt.AlignHCenter | Qt.AlignBottom, it.glyph)
            f.setPixelSize(10)
            f.setBold(True)
            f.setLetterSpacing(QFont.AbsoluteSpacing, 0.7)
            p.setFont(f)
            p.setPen(dim)
            p.drawText(QRectF(pt.x() - 46, pt.y() + 1, 92, 16), Qt.AlignCenter, it.label.upper())
        else:
            f.setPixelSize(14)
            f.setBold(True)
            f.setLetterSpacing(QFont.AbsoluteSpacing, 0.9)
            p.setFont(f)
            p.setPen(col)
            p.drawText(QRectF(pt.x() - 46, pt.y() - 12, 92, 24), Qt.AlignCenter, it.label)
        p.restore()

"""Persistent settings + the Settings and Welcome windows.

Settings live in %APPDATA%\\DragShift\\settings.json, shared with the native DragShift.exe
(which reads the trigger key, language and so on from the same file)."""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (QCheckBox, QComboBox, QGridLayout, QHBoxLayout, QLabel,
                               QPushButton, QSlider, QVBoxLayout)

from . import __version__
from .i18n import tr
from .util import CONFIG
from .window import GlassWindow

IS_WIN = sys.platform == "win32"
IS_MAC = sys.platform == "darwin"
IS_LINUX = sys.platform.startswith("linux")
APP = "DragShift"
RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"

DEFAULTS = {
    "output": "next",
    "jpg_quality": 90,
    "pdf_dpi": 200,
    "trigger": "shift",
    "welcomed": False,
    "lang": "auto",
    "gpu": True,
    "ui_idle_seconds": 60,
    "enabled": True,
}


def settings_path() -> Path:
    if IS_WIN:
        base = Path(os.environ.get("APPDATA") or Path.home())
    elif IS_MAC:
        base = Path.home() / "Library" / "Application Support"
    else:
        base = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
    d = base / APP
    d.mkdir(parents=True, exist_ok=True)
    return d / "settings.json"


class Settings:
    """Reads the file fresh on every get(), so changes made by the daemon are never lost."""

    def _load(self) -> dict:
        try:
            data = json.loads(settings_path().read_text(encoding="utf-8"))
            return data if isinstance(data, dict) else {}
        except (OSError, ValueError):
            return {}

    def get(self, key):
        d = DEFAULTS[key]
        v = self._load().get(key, d)
        if isinstance(d, bool):
            return v in (True, "true", "1", 1)
        if isinstance(d, int):
            try:
                return int(v)
            except (TypeError, ValueError):
                return d
        return v if isinstance(v, str) else d

    def set(self, key, value):
        data = {**DEFAULTS, **self._load(), key: value}
        p = settings_path()
        tmp = p.with_suffix(".json.tmp")
        tmp.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")
        os.replace(tmp, p)
        self.apply()

    def apply(self):
        for k in ("output", "jpg_quality", "pdf_dpi"):
            CONFIG[k] = self.get(k)


# ---------------------------------------------------------------- autostart
def launch_command() -> str:
    daemon = os.environ.get("DRAGSHIFT_DAEMON")
    if daemon:
        return f'"{daemon}" --quiet'
    if getattr(sys, "frozen", False):
        return f'"{sys.executable}" --quiet'
    return f'"{sys.executable}" -m dragshift --quiet'


def autostart_enabled() -> bool:
    if IS_LINUX:
        from . import platform_linux
        return platform_linux.autostart_enabled()
    if IS_MAC:
        from . import platform_mac
        return platform_mac.autostart_enabled()
    if not IS_WIN:
        return False
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as k:
            winreg.QueryValueEx(k, APP)
            return True
    except OSError:
        return False


def set_autostart(on: bool):
    if IS_LINUX:
        from . import platform_linux
        platform_linux.set_autostart(on)
        return
    if IS_MAC:
        from . import platform_mac
        platform_mac.set_autostart(on)
        return
    if not IS_WIN:
        return
    import winreg
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY, 0, winreg.KEY_SET_VALUE) as k:
        if on:
            winreg.SetValueEx(k, APP, 0, winreg.REG_SZ, launch_command())
        else:
            try:
                winreg.DeleteValue(k, APP)
            except OSError:
                pass


TRIGGERS = ([("shift", "Shift ⇧"), ("ctrl", "Control ⌃"), ("alt", "Option ⌥")] if IS_MAC
            else [("shift", "Shift"), ("ctrl", "Ctrl"), ("alt", "Alt")])
OUTPUTS = [("next", "Next to the original"), ("folder", "In a “DragShift” subfolder")]
DPIS = [(150, "150 dpi · small"), (200, "200 dpi · balanced"), (300, "300 dpi · print")]
LANGS = [("auto", "Same as Windows"), ("en", "English"), ("ru", "Русский")]


class SettingsWindow(GlassWindow):
    changed = Signal()
    quit_requested = Signal()

    def __init__(self, settings: Settings):
        super().__init__(tr("Settings"), size=(560, 600))
        self.s = settings
        grid = QGridLayout()
        grid.setHorizontalSpacing(14)
        grid.setVerticalSpacing(12)
        r = 0

        def combo(options, current):
            c = QComboBox()
            for v, label in options:
                c.addItem(tr(label), v)
            values = [v for v, _ in options]
            c.setCurrentIndex(values.index(current) if current in values else 0)
            return c

        def label(text):
            l = QLabel(tr(text))
            return l

        def dim(text):
            l = QLabel(tr(text))
            l.setObjectName("dim")
            l.setWordWrap(True)
            return l

        grid.addWidget(label("Language"), r, 0)
        self.lang = combo(LANGS, self.s.get("lang"))
        self.lang.currentIndexChanged.connect(lambda _: self._save("lang", self.lang.currentData()))
        grid.addWidget(self.lang, r, 1)
        r += 1

        grid.addWidget(label("Open the menu with"), r, 0)
        self.trigger = combo(TRIGGERS, self.s.get("trigger"))
        self.trigger.currentIndexChanged.connect(lambda _: self._save("trigger", self.trigger.currentData()))
        grid.addWidget(self.trigger, r, 1)
        r += 1
        grid.addWidget(dim("…pressed while you’re dragging a file"), r, 1)
        r += 1

        grid.addWidget(label("Save results"), r, 0)
        self.output = combo(OUTPUTS, self.s.get("output"))
        self.output.currentIndexChanged.connect(lambda _: self._save("output", self.output.currentData()))
        grid.addWidget(self.output, r, 1)
        r += 1

        grid.addWidget(label("JPG / WebP quality"), r, 0)
        row = QHBoxLayout()
        self.q = QSlider(Qt.Horizontal)
        self.q.setRange(60, 100)
        self.q.setValue(self.s.get("jpg_quality"))
        self.qv = QLabel(str(self.q.value()))
        self.qv.setObjectName("dim")
        self.qv.setFixedWidth(30)
        self.q.valueChanged.connect(lambda v: self.qv.setText(str(v)))
        self.q.sliderReleased.connect(lambda: self._save("jpg_quality", self.q.value()))
        self.q.valueChanged.connect(lambda v: None if self.q.isSliderDown() else self._save("jpg_quality", v))
        row.addWidget(self.q)
        row.addWidget(self.qv)
        grid.addLayout(row, r, 1)
        r += 1

        grid.addWidget(label("PDF → image resolution"), r, 0)
        self.dpi = combo(DPIS, self.s.get("pdf_dpi"))
        self.dpi.currentIndexChanged.connect(lambda _: self._save("pdf_dpi", self.dpi.currentData()))
        grid.addWidget(self.dpi, r, 1)
        r += 1

        grid.addWidget(label("Video"), r, 0)
        self.gpu = QCheckBox(tr("Use the graphics card when it helps"))
        self.gpu.setChecked(self.s.get("gpu"))
        self.gpu.toggled.connect(lambda on: self._save("gpu", on))
        grid.addWidget(self.gpu, r, 1)
        r += 1
        grid.addWidget(dim("NVIDIA NVENC, Intel Quick Sync or AMD AMF: several times faster video "
                           "compression. DragShift checks that it works and falls back to the CPU."), r, 1)
        r += 1

        if IS_WIN or IS_MAC or IS_LINUX:
            self.auto = QCheckBox(tr("Start DragShift with Windows") if IS_WIN else tr("Open DragShift at login"))
            self.auto.setChecked(autostart_enabled())
            self.auto.toggled.connect(set_autostart)
            grid.addWidget(self.auto, r, 1)
            r += 1
        self.body.addLayout(grid)
        self.body.addStretch(1)
        self.note = dim("")
        self.body.addWidget(self.note)
        about = QLabel(tr("DragShift {}  ·  originals are never modified", __version__))
        about.setObjectName("dim")
        foot = QHBoxLayout()
        foot.addWidget(about)
        foot.addStretch(1)
        if os.environ.get("DRAGSHIFT_DAEMON"):
            quit_btn = QPushButton(tr("Quit DragShift"))
            quit_btn.clicked.connect(self.quit_requested.emit)
            foot.addWidget(quit_btn)
        done = QPushButton(tr("Done"))
        done.setObjectName("primary")
        done.clicked.connect(self.close)
        foot.addWidget(done)
        self.body.addLayout(foot)

    def _save(self, key, value):
        if self.s.get(key) == value:
            return
        self.s.set(key, value)
        if key == "lang":
            from . import i18n
            i18n.reload()
            self.note.setText(tr("The new language applies to menus and windows opened from now on."))
        self.changed.emit()


class WelcomeWindow(GlassWindow):
    def __init__(self, settings: Settings, trigger_label: str = "Shift"):
        super().__init__(tr("Welcome to DragShift"), size=(560, 430))
        self.s = settings
        steps = [
            ("1", tr("Start dragging any file"), tr("in Finder or on the desktop") if IS_MAC else
             tr("in your file manager (Thunar, Nautilus…)") if IS_LINUX else tr("in File Explorer or on the desktop")),
            ("2", tr("Press {}", trigger_label), tr("the glass menu opens right under your cursor")),
            ("3", tr("Drop it on a slice"), tr("the result is saved next to the original")),
            ("⇄", tr("Tap {} again", trigger_label), tr("to switch between Convert and Tools")),
        ]
        for n, head, sub in steps:
            row = QHBoxLayout()
            badge = QLabel(n)
            badge.setAlignment(Qt.AlignCenter)
            badge.setFixedSize(34, 34)
            badge.setStyleSheet("background: qlineargradient(x1:0,y1:0,x2:1,y2:1, stop:0 #5ac8ff, stop:1 #2f7bff);"
                                "border-radius: 17px; font-weight: 700; font-size: 12pt;")
            row.addWidget(badge)
            col = QVBoxLayout()
            col.setSpacing(0)
            h = QLabel(head)
            h.setStyleSheet("font-size: 12pt; font-weight: 600;")
            d = QLabel(sub)
            d.setObjectName("dim")
            col.addWidget(h)
            col.addWidget(d)
            row.addSpacing(8)
            row.addLayout(col, 1)
            self.body.addLayout(row)
        self.body.addStretch(1)
        if IS_MAC:
            from . import platform_mac
            if not platform_mac.has_screen_recording():
                note = QLabel("Optional: allow Screen Recording so the glass can refract the windows "
                              "behind it (otherwise it shows your wallpaper). DragShift never saves or sends it.")
                note.setObjectName("dim")
                note.setWordWrap(True)
                self.body.addWidget(note)
                b = QPushButton("Allow Screen Recording…")
                b.clicked.connect(platform_mac.request_screen_recording)
                self.body.addWidget(b, 0, Qt.AlignLeft)
        if IS_LINUX:
            from . import platform_linux
            if platform_linux.is_wayland():
                note = QLabel("You're on a Wayland session: the Shift-while-dragging trigger needs X11 (e.g. XFCE). "
                              "Right-click a file → Open With → DragShift still works.")
                note.setObjectName("warn")
                note.setWordWrap(True)
                self.body.addWidget(note)
        foot = QHBoxLayout()
        if IS_WIN or IS_MAC or IS_LINUX:
            self.auto = QCheckBox(tr("Start with Windows") if IS_WIN else tr("Open at login"))
            self.auto.setChecked(True)
            foot.addWidget(self.auto)
        foot.addStretch(1)
        ok = QPushButton(tr("Got it"))
        ok.setObjectName("primary")
        ok.clicked.connect(self._done)
        foot.addWidget(ok)
        self.body.addLayout(foot)

    def _done(self):
        self.s.set("welcomed", True)
        if IS_WIN or IS_MAC or IS_LINUX:
            set_autostart(self.auto.isChecked())
        self.close()

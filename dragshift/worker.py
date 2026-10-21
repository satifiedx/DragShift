"""DragShiftUI: the editor half of DragShift, started on demand by the native DragShift.exe.

The daemon writes one JSON request per line to our stdin and reads JSON events from our stdout.
It shows every toast itself; we only open windows and run the image/PDF work. When the daemon
has nothing left for us it closes stdin and we exit, so no Python sits in memory while idle."""
from __future__ import annotations

import faulthandler
import itertools
import json
import os
import sys
import threading
import traceback

# stdout carries the protocol: anything else that prints must not end up there
_OUT = sys.stdout.buffer if sys.stdout is not None else None
sys.stdout = sys.stderr
_out_lock = threading.Lock()


def emit(ev: dict):
    if _OUT is None:
        return
    line = (json.dumps(ev, ensure_ascii=False) + "\n").encode("utf-8")
    with _out_lock:
        try:
            _OUT.write(line)
            _OUT.flush()
        except (OSError, ValueError):
            pass


def log(text: str):
    print(text, file=sys.stderr, flush=True)


from PySide6.QtCore import QObject, QRunnable, Qt, QThreadPool, QTimer, Signal  # noqa: E402
from PySide6.QtWidgets import QApplication, QWidget  # noqa: E402

from . import convert, imaging, report, tools  # noqa: E402
from .i18n import reload as reload_lang, tr  # noqa: E402
from .settings import Settings, SettingsWindow  # noqa: E402
from .util import UserError, category  # noqa: E402

IMAGE_WINDOWS = {"Crop": tools.CropWindow, "Add BG": tools.AddBgWindow, "Annotate": tools.AnnotateWindow,
                 "Redact": tools.RedactWindow, "Edit": tools.EditWindow, "Metadata": tools.MetadataWindow}
PDF_WORK = {"Compress": lambda p: [imaging.compress_pdf(p)], "Split": imaging.split_pdf,
            "Rotate": lambda p: [imaging.rotate_pdf(p)], "Extract": imaging.extract_pdf_images}


def friendly_error(e: Exception) -> str:
    if isinstance(e, UserError):
        return str(e)
    msg = str(e) or e.__class__.__name__
    low = msg.lower()
    if isinstance(e, PermissionError) or "permission denied" in low:
        return tr("No permission to write next to this file. Try a folder you own, or set Settings → Save results → subfolder.")
    if "cannot identify image" in low:
        return tr("This file doesn't look like an image DragShift can read.")
    if isinstance(e, FileNotFoundError):
        return tr("The file was moved or deleted.")
    if isinstance(e, MemoryError):
        return tr("Not enough memory for a file this big.")
    return msg


def result_event(r: report.Result) -> dict:
    return {"title": r.title, "detail": r.detail, "good": r.good, "outs": [str(o) for o in r.outs]}


class _Sig(QObject):
    line = Signal(str)
    eof = Signal()
    busy = Signal()


class PipeReader:
    """Reads stdin without ever leaving a read pending. On Windows a thread blocked in ReadFile on
    a synchronous pipe stalls every later DLL load in the process (the CRT probes the std handles),
    so the first numpy/PyMuPDF import would hang. Instead we peek on a timer and read only what's
    there; the worker only lives while there is work, so the polling costs nothing that matters."""

    def __init__(self, on_line, on_eof):
        import ctypes
        import msvcrt
        from ctypes import wintypes
        self.k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        self.k32.PeekNamedPipe.argtypes = [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p,
                                           ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p]
        self.k32.ReadFile.argtypes = [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
                                      ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p]
        self.ctypes, self.wintypes = ctypes, wintypes
        self.handle = msvcrt.get_osfhandle(sys.stdin.fileno())
        self.buf = bytearray()
        self.on_line, self.on_eof = on_line, on_eof
        self.timer = QTimer(interval=40, timeout=self.poll)
        self.timer.start()

    def poll(self):
        ct, wt = self.ctypes, self.wintypes
        avail = wt.DWORD(0)
        if not self.k32.PeekNamedPipe(self.handle, None, 0, None, ct.byref(avail), None):
            self.timer.stop()   # broken pipe: the daemon closed our stdin or went away
            self.on_eof()
            return
        if not avail.value:
            return
        chunk = ct.create_string_buffer(avail.value)
        got = wt.DWORD(0)
        if not self.k32.ReadFile(self.handle, chunk, avail.value, ct.byref(got), None):
            self.timer.stop()
            self.on_eof()
            return
        self.buf += chunk.raw[:got.value]
        while b"\n" in self.buf:
            raw, _, rest = self.buf.partition(b"\n")
            self.buf = bytearray(rest)
            line = raw.decode("utf-8", "replace").strip()
            if line:
                self.on_line(line)


class Task(QRunnable):
    """Runs fn() on the pool and reports to the daemon under `ref` ({"job": n} or {"tag": "wN"})."""

    def __init__(self, ref: dict, action: str, srcs: list, fn, extra: str, done):
        super().__init__()
        self.ref, self.action, self.srcs, self.fn, self.extra, self.done = ref, action, srcs, fn, extra, done
        self.setAutoDelete(True)

    def run(self):
        try:
            outs = self.fn() or []
            emit({"ev": "done", **self.ref, **result_event(report.summarize(self.action, self.srcs, outs, self.extra))})
        except Exception as e:  # noqa: BLE001
            if not isinstance(e, UserError):
                traceback.print_exc()
            emit({"ev": "error", **self.ref, "detail": friendly_error(e)})
        finally:
            self.done()


class Worker(QObject):
    def __init__(self, qapp: QApplication):
        super().__init__()
        self.qapp = qapp
        self.pool = QThreadPool.globalInstance()
        self.windows: list[QWidget] = []
        self.running = 0          # background saves started by windows
        self._lock = threading.Lock()
        self._tags = itertools.count(1)
        self.sig = _Sig()
        self.sig.line.connect(self.handle)
        self.sig.eof.connect(self.qapp.quit)
        self.sig.busy.connect(self.report_busy)
        if sys.platform == "win32" and sys.stdin is not None:
            self.reader = PipeReader(self.handle, self.qapp.quit)
        else:
            threading.Thread(target=self._read, daemon=True).start()

    # ------------------------------------------------------------ protocol
    def _read(self):
        stdin = sys.stdin.buffer if sys.stdin is not None else None
        if stdin is not None:
            for raw in stdin:
                line = raw.decode("utf-8", "replace").strip()
                if line:
                    self.sig.line.emit(line)
        self.sig.eof.emit()

    def handle(self, line: str):
        try:
            msg = json.loads(line)
        except ValueError:
            log(f"bad request: {line!r}")
            return
        op = msg.get("op")
        try:
            if op == "run":
                self.run(int(msg["job"]), msg.get("key", ""), msg.get("cat", ""), list(msg.get("paths", [])))
            elif op == "settings":
                self.open_settings()
            elif op == "cancel":
                pass   # our jobs are short and not cancellable; the daemon never offers it
            else:
                log(f"unknown request: {msg}")
        except Exception as e:  # noqa: BLE001
            traceback.print_exc()
            emit({"ev": "error", "job": msg.get("job"), "detail": friendly_error(e)})

    def report_busy(self):
        emit({"ev": "windows", "n": len(self.windows) + self.running})

    # ------------------------------------------------------------ requests
    def run(self, job: int, key: str, cat: str, paths: list[str]):
        kind, _, name = key.partition(":")
        paths = [p for p in paths if os.path.exists(p)]
        if not paths:
            emit({"ev": "error", "job": job, "detail": tr("The file was moved or deleted.")})
            return
        if cat in ("video", "audio"):
            from . import media_tools
            cls = media_tools.WINDOWS.get(name)
            if cls is None:
                raise UserError(f"unknown media tool {name}")
            self.open(job, name, lambda: cls(paths[0], emit))
            return
        same = [p for p in paths if category(p) == cat] or paths
        if kind == "convert":
            if cat == "image" and name == "PDF" and len(same) > 1:
                fn = lambda: convert.images_to_single_pdf(same)
            else:
                fn = lambda: [o for p in same for o in convert.convert(p, cat, name)]
            self.background({"job": job}, key, same, fn)
            return
        if cat == "pdf":
            if name == "Merge":
                self.background({"job": job}, key, same, lambda: [imaging.merge_pdfs(same)])
            elif name == "Metadata":
                self.open(job, name, lambda: tools.MetadataWindow(same[0], self.notify, is_pdf=True))
            elif name == "Protect":
                self.open(job, name, lambda: tools.ProtectWindow(same[0], self.notify))
            else:
                f = PDF_WORK[name]
                self.background({"job": job}, key, same, lambda: [o for p in same for o in f(p)])
            return
        if name == "Compress":
            self.open(job, name, lambda: tools.CompressWindow(same, self.notify))
        elif name == "Resize":
            self.open(job, name, lambda: tools.ResizeWindow(same, self.notify))
        elif name in IMAGE_WINDOWS:
            self.open(job, name, lambda: IMAGE_WINDOWS[name](same[0], self.notify))
        else:
            raise UserError(f"unknown action {key}")

    def open(self, job: int | None, name: str, make):
        try:
            w = make()
        except Exception as e:  # noqa: BLE001
            if not isinstance(e, UserError):
                traceback.print_exc()
            emit({"ev": "error", "job": job, "title": tr("Couldn't open {}", tr(name)), "detail": friendly_error(e)})
            return
        if hasattr(w, "submit"):
            w.submit = self.submit
        self.windows.append(w)
        w.destroyed.connect(lambda *_: self._closed(w))
        if hasattr(w, "present"):
            w.present()
        else:
            w.show()
        self.report_busy()
        emit({"ev": "window", "job": job})

    def _closed(self, w):
        if w in self.windows:
            self.windows.remove(w)
        self.report_busy()

    def open_settings(self):
        for w in self.windows:
            if isinstance(w, SettingsWindow):
                w.raise_()
                w.activateWindow()
                return
        w = SettingsWindow(Settings())
        w.changed.connect(self._settings_changed)
        w.quit_requested.connect(lambda: emit({"ev": "quit"}))
        self.open(None, "Settings", lambda: w)

    def _settings_changed(self):
        Settings().apply()
        reload_lang()
        emit({"ev": "settings"})

    # ------------------------------------------------------------ work
    def background(self, ref: dict, action: str, srcs: list, fn, extra: str = ""):
        with self._lock:
            self.running += 1

        def finished():
            with self._lock:
                self.running -= 1
            self.sig.busy.emit()

        self.pool.start(Task(ref, action, srcs, fn, extra, finished))

    def submit(self, action: str, srcs: list, fn, working: str = "", extra: str = ""):
        """A tool window saves: the daemon shows a progress card, the window can close at once."""
        tag = f"w{next(self._tags)}"
        from pathlib import Path
        names = Path(srcs[0]).name if len(srcs) == 1 else tr("{} files", len(srcs))
        emit({"ev": "start", "tag": tag, "title": working or tr("Working…"), "detail": names})
        self.background({"tag": tag}, action, srcs, fn, extra)
        self.report_busy()

    def notify(self, action: str, srcs: list, outs: list, extra: str = ""):
        """A tool window that saved by itself (no background job)."""
        emit({"ev": "done", **result_event(report.summarize(action, srcs, outs, extra))})


def started_by_hand() -> bool:
    """Someone double-clicked DragShiftUI.exe: start the real app instead of waiting on a console."""
    if "--selftest" in sys.argv or not getattr(sys, "frozen", False):
        return False
    try:
        if not sys.stdin.isatty():
            return False
    except (AttributeError, ValueError):
        return False
    from pathlib import Path
    import subprocess
    daemon = Path(sys.executable).resolve().parent.parent / "DragShift.exe"
    if daemon.exists():
        subprocess.Popen([str(daemon)], close_fds=True)
    return True


def main() -> int:
    if started_by_hand():
        return 0
    faulthandler.enable(sys.stderr)
    if sys.platform == "win32":
        try:
            import ctypes
            ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID("DragShift.App")
        except Exception:
            pass
    QApplication.setHighDpiScaleFactorRoundingPolicy(Qt.HighDpiScaleFactorRoundingPolicy.PassThrough)
    qapp = QApplication(sys.argv[:1])
    qapp.setApplicationName("DragShift")
    qapp.setQuitOnLastWindowClosed(False)
    from pathlib import Path
    from PySide6.QtGui import QIcon
    ico = Path(__file__).with_name("assets") / "dragshift.ico"
    if ico.exists():
        qapp.setWindowIcon(QIcon(str(ico)))
    Settings().apply()
    if "--selftest" in sys.argv:
        from .selftest_ui import run
        return run()
    Worker(qapp)
    emit({"ev": "log", "text": "ready"})
    return qapp.exec()


if __name__ == "__main__":
    sys.exit(main())

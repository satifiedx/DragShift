# PyInstaller spec for DragShiftUI (Windows): the editor windows and image/PDF work.
# Build from the repo root:  pyinstaller packaging/DragShiftUI.spec --noconfirm
# Output: dist/ui/DragShiftUI.exe + its libraries, installed next to DragShift.exe as ui\.
from PyInstaller.utils.hooks import collect_all

ROOT = SPECPATH + "/.."
heif_datas, heif_bins, heif_hidden = collect_all("pillow_heif")

a = Analysis(
    [ROOT + "/packaging/ui_main.py"],
    pathex=[ROOT],
    datas=[(ROOT + "/dragshift/assets/dragshift.ico", "dragshift/assets")] + heif_datas,
    binaries=heif_bins,
    hiddenimports=["dragshift.media_tools", "dragshift.selftest_ui"] + heif_hidden,
    excludes=["tkinter", "unittest", "pydoc", "ssl", "_ssl", "pywin32", "win32com", "pythoncom", "pywintypes",
              "dragshift.app", "dragshift.radial", "dragshift.trigger", "dragshift.toasts", "dragshift.handoff",
              "dragshift.platform_mac", "dragshift.platform_linux",
              "PySide6.QtNetwork", "PySide6.QtWebEngineCore", "PySide6.QtWebEngineWidgets", "PySide6.QtQml",
              "PySide6.QtQuick", "PySide6.Qt3DCore", "PySide6.QtMultimedia", "PySide6.QtCharts",
              "PySide6.QtDataVisualization", "PySide6.QtPdf", "PySide6.QtSql", "PySide6.QtTest",
              "PySide6.QtOpenGL", "PySide6.QtSvg", "PySide6.QtSvgWidgets", "PySide6.QtDesigner",
              "pandas", "yaml", "matplotlib", "scipy"],
)

DROP = ("Qt6Quick", "Qt6Qml", "Qt6Pdf", "Qt6WebEngine", "Qt63D", "Qt6Multimedia", "Qt6Charts",
        "Qt6VirtualKeyboard", "Qt6Designer", "Qt6ShaderTools", "Qt6Sql", "Qt6Test", "opengl32sw",
        "Qt6OpenGL", "Qt6DataVisualization", "Qt6Positioning", "Qt6Sensors", "Qt6SerialPort",
        "Qt6Bluetooth", "Qt6Nfc", "Qt6RemoteObjects", "Qt6Scxml", "Qt6SpatialAudio", "Qt6TextToSpeech",
        "Qt6WebChannel", "Qt6WebSockets", "Qt6Svg", "Qt6Network", "qtquick", "qml", "qsvg", "qtuiotouch",
        "qnetworklistmanager", "qtvirtualkeyboard", "qpdf", "qcertonly", "qschannel", "qopensslbackend", "libssl")
a.binaries = [b for b in a.binaries if not any(d.lower() in b[0].lower() for d in DROP)]
a.datas = [d for d in a.datas if "translations" not in d[0].replace("\\", "/")]

pyz = PYZ(a.pure)
# console subsystem: the daemon talks to us over stdin/stdout and starts us with CREATE_NO_WINDOW
exe = EXE(pyz, a.scripts, [], exclude_binaries=True, name="DragShiftUI", console=True, upx=False,
          icon=ROOT + "/dragshift/assets/dragshift.ico", version=ROOT + "/installer/version_info.txt")
coll = COLLECT(exe, a.binaries, a.datas, name="ui", upx=False)

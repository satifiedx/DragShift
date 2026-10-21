"""Excel and PowerPoint to PDF.

Microsoft Office does it best, so it is used when installed (driven through PowerShell, the build
has no pywin32), then LibreOffice. Without either, .xlsx becomes tables and .pptx is redrawn
slide by slide from its XML: text boxes, pictures, fills and tables in their places."""
from __future__ import annotations

import datetime as dt
import html
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path, PurePosixPath
from xml.etree import ElementTree as ET

from . import fonts
from .i18n import tr
from .util import UserError, sibling

NS = {
    "s": "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "p": "http://schemas.openxmlformats.org/presentationml/2006/main",
    "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
    "pr": "http://schemas.openxmlformats.org/package/2006/relationships",
}
R_ID = f"{{{NS['r']}}}id"
R_EMBED = f"{{{NS['r']}}}embed"
EMU_PER_PT = 12700
CREATE_NO_WINDOW = 0x0800_0000

SHEET_EXTS = {".xlsx", ".xlsm", ".xls"}
SLIDE_EXTS = {".pptx", ".ppt"}


def to_pdf(src: str | Path) -> list[Path]:
    src = Path(src)
    out = sibling(src, ext=".pdf")
    ext = src.suffix.lower()
    if office_export(src, out) or libreoffice_export(src, out):
        return [out]
    if ext == ".xlsx" or ext == ".xlsm":
        xlsx_to_pdf(src, out)
    elif ext == ".pptx":
        pptx_to_pdf(src, out)
    else:
        raise UserError(tr("Old {} files need Microsoft Office or LibreOffice to convert.", ext.lstrip(".").upper()))
    return [out]


# ---------------------------------------------------------------- Office and LibreOffice
_PS = {
    "Word": "$a = New-Object -ComObject Word.Application; $a.Visible = $false; $a.DisplayAlerts = 0\n"
            "try { $d = $a.Documents.Open($env:DS_SRC, $false, $true); $d.ExportAsFixedFormat($env:DS_OUT, 17); $d.Close($false) }\n"
            "finally { $a.Quit() }",
    "Excel": "$a = New-Object -ComObject Excel.Application; $a.Visible = $false; $a.DisplayAlerts = $false\n"
             "try { $d = $a.Workbooks.Open($env:DS_SRC, 0, $true); $d.ExportAsFixedFormat(0, $env:DS_OUT); $d.Close($false) }\n"
             "finally { $a.Quit() }",
    "PowerPoint": "$a = New-Object -ComObject PowerPoint.Application\n"
                  "try { $d = $a.Presentations.Open($env:DS_SRC, -1, 0, 0); $d.SaveAs($env:DS_OUT, 32); $d.Close() }\n"
                  "finally { $a.Quit() }",
}


def _office_app(ext: str) -> str:
    if ext in SHEET_EXTS:
        return "Excel"
    if ext in SLIDE_EXTS:
        return "PowerPoint"
    return "Word"


def _registered(prog_id: str) -> bool:
    try:
        import winreg
        winreg.CloseKey(winreg.OpenKey(winreg.HKEY_CLASSES_ROOT, prog_id + r"\CLSID"))
        return True
    except OSError:
        return False


def office_export(src: Path, out: Path) -> bool:
    """Export through Word, Excel or PowerPoint when installed. False when it isn't or it failed."""
    if sys.platform != "win32":
        return False
    app = _office_app(src.suffix.lower())
    if not _registered(f"{app}.Application"):
        return False
    script = "$ErrorActionPreference = 'Stop'\n" + _PS[app]
    env = dict(os.environ, DS_SRC=str(src.resolve()), DS_OUT=str(out.resolve()))
    try:
        subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script],
                       env=env, timeout=180, capture_output=True, creationflags=CREATE_NO_WINDOW)
    except (OSError, subprocess.SubprocessError):
        return False
    return out.is_file() and out.stat().st_size > 0


def _soffice() -> str | None:
    found = shutil.which("soffice")
    if found:
        return found
    for base in (os.environ.get("ProgramFiles"), os.environ.get("ProgramFiles(x86)")):
        if base:
            p = Path(base) / "LibreOffice" / "program" / "soffice.exe"
            if p.is_file():
                return str(p)
    for p in ("/Applications/LibreOffice.app/Contents/MacOS/soffice", "/usr/bin/libreoffice"):
        if Path(p).is_file():
            return p
    return None


def libreoffice_export(src: Path, out: Path) -> bool:
    exe = _soffice()
    if not exe:
        return False
    with tempfile.TemporaryDirectory() as tmp:
        try:
            subprocess.run([exe, "--headless", "--norestore", "--convert-to", "pdf", "--outdir", tmp, str(src)],
                           timeout=180, capture_output=True,
                           creationflags=CREATE_NO_WINDOW if sys.platform == "win32" else 0)
        except (OSError, subprocess.SubprocessError):
            return False
        made = Path(tmp) / (src.stem + ".pdf")
        if not made.is_file():
            return False
        shutil.move(str(made), out)
    return True


# ---------------------------------------------------------------- package helpers
class Package:
    def __init__(self, path: Path):
        try:
            self.zip = zipfile.ZipFile(path)
        except zipfile.BadZipFile:
            raise UserError(tr("This file is damaged or isn't really {}.", path.suffix.lstrip(".").upper())) from None
        self.names = set(self.zip.namelist())

    def close(self):
        self.zip.close()

    def xml(self, name: str):
        return ET.fromstring(self.zip.read(name)) if name in self.names else None

    def read(self, name: str) -> bytes | None:
        return self.zip.read(name) if name in self.names else None

    def rels(self, part: str) -> dict[str, str]:
        """Relationship id -> package path of the target, for `part`."""
        p = PurePosixPath(part)
        root = self.xml(str(p.parent / "_rels" / (p.name + ".rels")))
        out = {}
        for r in root if root is not None else []:
            target = r.get("Target", "")
            if r.get("TargetMode") == "External":
                continue
            full = target.lstrip("/") if target.startswith("/") else str(p.parent / target)
            out[r.get("Id")] = _normpath(full)
        return out

    def rel_of_type(self, part: str, kind: str) -> str | None:
        p = PurePosixPath(part)
        root = self.xml(str(p.parent / "_rels" / (p.name + ".rels")))
        for r in root if root is not None else []:
            if r.get("Type", "").endswith("/" + kind):
                return self.rels(part).get(r.get("Id"))
        return None


def _normpath(p: str) -> str:
    parts: list[str] = []
    for bit in p.split("/"):
        if bit == "..":
            if parts:
                parts.pop()
        elif bit not in ("", "."):
            parts.append(bit)
    return "/".join(parts)


# ---------------------------------------------------------------- xlsx
DATE_FMT_IDS = set(range(14, 23)) | set(range(45, 48))


def _col_index(ref: str) -> int:
    n = 0
    for ch in ref:
        if not ch.isalpha():
            break
        n = n * 26 + ord(ch.upper()) - 64
    return n - 1


def _number(v: str, is_date: bool, date1904: bool) -> str:
    try:
        f = float(v)
    except ValueError:
        return v
    if is_date:
        base = dt.datetime(1904, 1, 1) if date1904 else dt.datetime(1899, 12, 30)
        try:
            d = base + dt.timedelta(days=f)
        except OverflowError:
            return v
        return d.strftime("%Y-%m-%d") if f == int(f) else d.strftime("%Y-%m-%d %H:%M")
    if f == int(f) and abs(f) < 1e15:
        return str(int(f))
    return f"{f:.10g}"


def read_xlsx(src: Path) -> list[tuple[str, list[list[str]]]]:
    """[(sheet name, rows of cell text)] with empty edges trimmed."""
    pkg = Package(src)
    try:
        return _read_xlsx(pkg)
    finally:
        pkg.close()


def _read_xlsx(pkg: Package) -> list[tuple[str, list[list[str]]]]:
    s = NS["s"]
    shared = []
    sst = pkg.xml("xl/sharedStrings.xml")
    for si in sst if sst is not None else []:
        shared.append("".join(t.text or "" for t in si.iter(f"{{{s}}}t")))
    date_styles: list[bool] = []
    styles = pkg.xml("xl/styles.xml")
    if styles is not None:
        custom = {int(f.get("numFmtId")): f.get("formatCode", "") for f in styles.iter(f"{{{s}}}numFmt")}
        xfs = styles.find(f"s:cellXfs", NS)
        for xf in xfs if xfs is not None else []:
            fid = int(xf.get("numFmtId", 0))
            code = re.sub(r'"[^"]*"|\[[^]]*]', "", custom.get(fid, "")).lower()
            date_styles.append(fid in DATE_FMT_IDS or (fid in custom and any(c in code for c in "dmy")))
    wb = pkg.xml("xl/workbook.xml")
    if wb is None:
        raise UserError(tr("This file is damaged or isn't really {}.", "XLSX"))
    pr = wb.find("s:workbookPr", NS)
    date1904 = pr is not None and pr.get("date1904") in ("1", "true")
    rels = pkg.rels("xl/workbook.xml")
    sheets = []
    for sh in wb.iterfind("s:sheets/s:sheet", NS):
        if sh.get("state") in ("hidden", "veryHidden"):
            continue
        root = pkg.xml(rels.get(sh.get(R_ID), ""))
        if root is None:
            continue
        grid: dict[int, dict[int, str]] = {}
        for row in root.iterfind("s:sheetData/s:row", NS):
            r = int(row.get("r", len(grid) + 1)) - 1
            cells = grid.setdefault(r, {})
            for i, c in enumerate(row.iterfind("s:c", NS)):
                col = _col_index(c.get("r", "")) if c.get("r") else i
                kind, v = c.get("t", "n"), c.find("s:v", NS)
                if kind == "inlineStr":
                    text = "".join(t.text or "" for t in c.iter(f"{{{s}}}t"))
                elif v is None or v.text is None:
                    continue
                elif kind == "s":
                    text = shared[int(v.text)] if v.text.isdigit() and int(v.text) < len(shared) else ""
                elif kind == "b":
                    text = "TRUE" if v.text == "1" else "FALSE"
                elif kind in ("str", "e"):
                    text = v.text
                else:
                    style = int(c.get("s", 0))
                    text = _number(v.text, style < len(date_styles) and date_styles[style], date1904)
                if text:
                    cells[col] = text
        used = [(r, cs) for r, cs in grid.items() if cs]
        if not used:
            sheets.append((sh.get("name", ""), []))
            continue
        r0, r1 = min(r for r, _ in used), max(r for r, _ in used)
        c0 = min(min(cs) for _, cs in used)
        c1 = max(max(cs) for _, cs in used)
        rows = [[grid.get(r, {}).get(c, "") for c in range(c0, c1 + 1)] for r in range(r0, r1 + 1)]
        sheets.append((sh.get("name", ""), rows))
    return sheets


SHEET_CSS = """
* { font-family: sans-serif; }
h2 { font-size: 13pt; margin: 0 0 6pt 0; }
table { border-collapse: collapse; }
td { border: 0.5pt solid #999; padding: 1.5pt 3pt; font-size: 8pt; vertical-align: top; }
td.n { text-align: right; }
"""


def xlsx_to_pdf(src: Path, out: Path) -> None:
    from .convert import atomic_save, fitz
    sheets = read_xlsx(src)
    if not any(rows for _, rows in sheets):
        raise UserError(tr("This spreadsheet is empty."))
    num = re.compile(r"^-?[\d\s.,]+%?$")
    widest = max((len(rows[0]) for _, rows in sheets if rows), default=1)

    def write(dst):
        writer = fitz.DocumentWriter(str(dst))
        page = fitz.paper_rect("a4-l" if widest > 6 else "a4")
        for name, rows in sheets:
            if not rows:
                continue
            body = [f"<h2>{html.escape(name)}</h2><table>"]
            for row in rows:
                body.append("<tr>" + "".join(
                    f'<td class="n">{html.escape(v)}</td>' if num.match(v) else f"<td>{html.escape(v)}</td>"
                    for v in row) + "</tr>")
            body.append("</table>")
            story = fitz.Story(html="".join(body), user_css=SHEET_CSS + fonts.css(), archive=fonts.archive())
            more = True
            while more:
                dev = writer.begin_page(page)
                more, _ = story.place(page + (28, 28, -28, -28))
                story.draw(dev)
                writer.end_page()
        writer.close()
        fonts.fix_file(dst)

    atomic_save(write, out)


# ---------------------------------------------------------------- pptx
def _emu(v, default=0) -> float:
    try:
        return int(v) / EMU_PER_PT
    except (TypeError, ValueError):
        return default


class Slides:
    def __init__(self, src: Path):
        self.pkg = Package(src)
        pres = self.pkg.xml("ppt/presentation.xml")
        if pres is None:
            self.pkg.close()
            raise UserError(tr("This file is damaged or isn't really {}.", "PPTX"))
        sz = pres.find("p:sldSz", NS)
        self.w = _emu(sz.get("cx"), 720) if sz is not None else 720
        self.h = _emu(sz.get("cy"), 540) if sz is not None else 540
        rels = self.pkg.rels("ppt/presentation.xml")
        self.slides = [rels[s.get(R_ID)] for s in pres.iterfind("p:sldIdLst/p:sldId", NS) if s.get(R_ID) in rels]
        self.colors: dict[str, str] = {}
        self.fonts: dict[str, str] = {}

    # ------------------------------------------------------------ colours
    def load_theme(self, master: str | None):
        self.colors = {}
        theme = self.pkg.rel_of_type(master, "theme") if master else None
        root = self.pkg.xml(theme) if theme else None
        self.fonts = {}
        for kind in ("major", "minor"):
            latin = root.find(f".//a:fontScheme/a:{kind}Font/a:latin", NS) if root is not None else None
            self.fonts[kind] = latin.get("typeface", "") if latin is not None else ""
        scheme = root.find(".//a:clrScheme", NS) if root is not None else None
        for c in scheme if scheme is not None else []:
            name = c.tag.split("}")[1]
            srgb = c.find("a:srgbClr", NS)
            sysc = c.find("a:sysClr", NS)
            val = srgb.get("val") if srgb is not None else sysc.get("lastClr") if sysc is not None else None
            if _hex(val):
                self.colors[name] = val
        for alias, real in (("tx1", "dk1"), ("bg1", "lt1"), ("tx2", "dk2"), ("bg2", "lt2")):
            if real in self.colors:
                self.colors[alias] = self.colors[real]

    def color(self, parent) -> str | None:
        """'#rrggbb' from a solidFill-like element, or None."""
        if parent is None:
            return None
        srgb = parent.find("a:srgbClr", NS)
        if srgb is not None:
            return _hex(srgb.get("val"))
        sch = parent.find("a:schemeClr", NS)
        if sch is not None and sch.get("val") in self.colors:
            return "#" + self.colors[sch.get("val")]
        sysc = parent.find("a:sysClr", NS)
        if sysc is not None:
            return _hex(sysc.get("lastClr"))
        return None

    # ------------------------------------------------------------ placeholders
    @staticmethod
    def ph_key(sp):
        ph = sp.find("p:nvSpPr/p:nvPr/p:ph", NS)
        if ph is None:
            return None
        return ph.get("type", "body"), ph.get("idx")

    def ph_xfrm(self, parts: list, key):
        """Where a placeholder sits when the slide doesn't say: the layout's, then the master's."""
        kind, idx = key
        for part in parts:
            root = self.pkg.xml(part) if part else None
            if root is None:
                continue
            fallback = None
            for sp in root.iter(f"{{{NS['p']}}}sp"):
                k = self.ph_key(sp)
                xf = sp.find("p:spPr/a:xfrm", NS)
                if k is None or xf is None:
                    continue
                if idx is not None and k[1] == idx:
                    return xf
                if k[0] == kind or (kind in ("ctrTitle",) and k[0] == "title"):
                    fallback = fallback if fallback is not None else xf
            if fallback is not None:
                return fallback
        return None

    # ------------------------------------------------------------ drawing
    def render(self, out: Path):
        from .convert import atomic_save, fitz
        if not self.slides:
            raise UserError(tr("This presentation has no slides."))
        with fitz.open() as doc:
            for slide in self.slides:
                root = self.pkg.xml(slide)
                if root is None:
                    continue
                layout = self.pkg.rel_of_type(slide, "slideLayout")
                master = self.pkg.rel_of_type(layout, "slideMaster") if layout else None
                self.load_theme(master)
                page = doc.new_page(width=self.w, height=self.h)
                self.background(page, [slide, layout, master])
                rels = self.pkg.rels(slide)
                tree = root.find("p:cSld/p:spTree", NS)
                if tree is not None:
                    self.draw_tree(page, tree, rels, [layout, master], lambda x, y, w, h: (x, y, w, h))
            fonts.fix_text_map(doc)
            atomic_save(lambda dst: doc.save(dst, garbage=3, deflate=True), out)

    def background(self, page, parts):
        for part in parts:
            root = self.pkg.xml(part) if part else None
            fill = root.find("p:cSld/p:bg/p:bgPr/a:solidFill", NS) if root is not None else None
            col = self.color(fill)
            if col:
                page.draw_rect(page.rect, color=None, fill=_rgb(col))
                return

    def draw_tree(self, page, tree, rels, inherit, tf):
        for el in tree:
            tag = el.tag.split("}")[1]
            if tag == "grpSp":
                xf = el.find("p:grpSpPr/a:xfrm", NS)
                inner = tf
                if xf is not None:
                    off, ext = xf.find("a:off", NS), xf.find("a:ext", NS)
                    choff, chext = xf.find("a:chOff", NS), xf.find("a:chExt", NS)
                    if None not in (off, ext, choff, chext):
                        ox, oy, ew, eh = _emu(off.get("x")), _emu(off.get("y")), _emu(ext.get("cx")), _emu(ext.get("cy"))
                        cx, cy = _emu(choff.get("x")), _emu(choff.get("y"))
                        sx = ew / (_emu(chext.get("cx")) or 1) if ew else 1
                        sy = eh / (_emu(chext.get("cy")) or 1) if eh else 1

                        def inner(x, y, w, h, tf=tf, ox=ox, oy=oy, cx=cx, cy=cy, sx=sx, sy=sy):
                            return tf(ox + (x - cx) * sx, oy + (y - cy) * sy, w * sx, h * sy)
                self.draw_tree(page, el, rels, inherit, inner)
            elif tag == "sp":
                self.shape(page, el, inherit, tf)
            elif tag == "pic":
                self.picture(page, el, rels, tf)
            elif tag == "graphicFrame":
                self.table(page, el, tf)

    def rect(self, xf, tf):
        from .convert import fitz
        if xf is None:
            return None
        off, ext = xf.find("a:off", NS), xf.find("a:ext", NS)
        if off is None or ext is None:
            return None
        x, y, w, h = tf(_emu(off.get("x")), _emu(off.get("y")), _emu(ext.get("cx")), _emu(ext.get("cy")))
        return fitz.Rect(x, y, x + w, y + h)

    def shape(self, page, sp, inherit, tf):
        key = self.ph_key(sp)
        if key and key[0] in ("dt", "ftr", "sldNum"):
            return
        xf = sp.find("p:spPr/a:xfrm", NS)
        if xf is None and key:
            xf = self.ph_xfrm(inherit, key)
        r = self.rect(xf, tf)
        if r is None or r.is_empty:
            return
        sppr = sp.find("p:spPr", NS)
        fill = self.color(sppr.find("a:solidFill", NS)) if sppr is not None else None
        ln = sppr.find("a:ln", NS) if sppr is not None else None
        line = self.color(ln.find("a:solidFill", NS)) if ln is not None else None
        geom = sppr.find("a:prstGeom", NS) if sppr is not None else None
        if fill or line:
            width = _emu(ln.get("w"), 0.75) if ln is not None else 0.75
            kw = dict(color=_rgb(line) if line else None, fill=_rgb(fill) if fill else None, width=width or 0.75)
            if geom is not None and geom.get("prst") == "ellipse":
                page.draw_oval(r, **kw)
            else:
                page.draw_rect(r, radius=0.08 if geom is not None and geom.get("prst") == "roundRect" else None, **kw)
        body = sp.find("p:txBody", NS)
        if body is not None:
            title = bool(key and key[0] in ("title", "ctrTitle"))
            self.text(page, r, body, title)

    def text(self, page, r, body, title: bool):
        default = 36 if title else 18
        families = []
        bodypr = body.find("a:bodyPr", NS)
        anchor = bodypr.get("anchor", "t") if bodypr is not None else "t"
        paras = []
        for p in body.iterfind("a:p", NS):
            ppr = p.find("a:pPr", NS)
            align = {"ctr": "center", "r": "right", "just": "justify"}.get(ppr.get("algn") if ppr is not None else "", "left")
            if title and ppr is None:
                align = "left"
            runs = []
            for el in p:
                tag = el.tag.split("}")[1]
                if tag == "br":
                    runs.append("<br>")
                elif tag in ("r", "fld"):
                    t = el.find("a:t", NS)
                    if t is None or not t.text:
                        continue
                    rpr = el.find("a:rPr", NS)
                    style = []
                    size = default
                    latin = rpr.find("a:latin", NS) if rpr is not None else None
                    face = latin.get("typeface", "") if latin is not None else ""
                    face = {"+mj-lt": self.fonts.get("major", ""), "+mn-lt": self.fonts.get("minor", "")}.get(face, face)
                    face = face or self.fonts.get("major" if title else "minor", "")
                    if face:
                        families.append(face)
                        style.append(f"font-family:{fonts.stack(fonts.faces([face])[1])}")
                    if rpr is not None:
                        if rpr.get("sz"):
                            size = int(rpr.get("sz")) / 100
                        if rpr.get("b") == "1":
                            style.append("font-weight:bold")
                        if rpr.get("i") == "1":
                            style.append("font-style:italic")
                        if rpr.get("u", "none") != "none":
                            style.append("text-decoration:underline")
                        col = self.color(rpr.find("a:solidFill", NS))
                        if col:
                            style.append(f"color:{col}")
                    elif title:
                        style.append("font-weight:bold")
                    style.append(f"font-size:{size:g}pt")
                    runs.append(f'<span style="{";".join(style)}">{html.escape(t.text)}</span>')
            paras.append(f'<p style="text-align:{align}">{"".join(runs) or "&nbsp;"}</p>')
        if not any("<span" in p for p in paras):
            return
        rules, names = fonts.faces(families)
        css = (f"{rules}\n* {{ font-family: {fonts.stack(names)}; color: #{self.colors.get('tx1', '000000')}; }} "
               "p { margin: 0 0 4pt 0; }")
        inner = r + (7.2, 3.6, -7.2, -3.6)
        if anchor in ("ctr", "b"):
            # measure first, then shift the box so the text sits in the middle or at the bottom
            from .convert import fitz
            probe = fitz.open()
            pg = probe.new_page(width=page.rect.width, height=page.rect.height)
            spare, _ = pg.insert_htmlbox(inner, "".join(paras), css=css, scale_low=0, archive=fonts.archive())
            probe.close()
            if spare > 0:
                inner = inner + (0, spare / 2 if anchor == "ctr" else spare, 0, 0)
        page.insert_htmlbox(inner, "".join(paras), css=css, scale_low=0, archive=fonts.archive())

    def picture(self, page, pic, rels, tf):
        r = self.rect(pic.find("p:spPr/a:xfrm", NS), tf)
        blip = pic.find("p:blipFill/a:blip", NS)
        if r is None or r.is_empty or blip is None:
            return
        data = self.pkg.read(rels.get(blip.get(R_EMBED), ""))
        if not data:
            return
        try:
            page.insert_image(r, stream=data, keep_proportion=False)
        except Exception:
            try:  # formats MuPDF can't place directly (e.g. TIFF, odd PNGs) go through Pillow
                import io
                from PIL import Image
                buf = io.BytesIO()
                Image.open(io.BytesIO(data)).convert("RGBA").save(buf, "PNG")
                page.insert_image(r, stream=buf.getvalue(), keep_proportion=False)
            except Exception:
                pass  # EMF/WMF and friends: leave the spot empty

    def table(self, page, frame, tf):
        from .convert import fitz
        tbl = frame.find("a:graphic/a:graphicData/a:tbl", NS)
        xf = frame.find("p:xfrm", NS)
        if tbl is None or xf is None:
            return
        off, ext = xf.find("a:off", NS), xf.find("a:ext", NS)
        if off is None or ext is None:
            return
        x0, y0, _, _ = tf(_emu(off.get("x")), _emu(off.get("y")), 0, 0)
        widths = [_emu(g.get("w")) for g in tbl.iterfind("a:tblGrid/a:gridCol", NS)]
        y = y0
        for tr_ in tbl.iterfind("a:tr", NS):
            h = _emu(tr_.get("h"), 20)
            x = x0
            for i, tc in enumerate(tr_.iterfind("a:tc", NS)):
                w = widths[i] if i < len(widths) else 60
                cell = fitz.Rect(x, y, x + w, y + h)
                tcpr = tc.find("a:tcPr", NS)
                fill = self.color(tcpr.find("a:solidFill", NS)) if tcpr is not None else None
                page.draw_rect(cell, color=(0.6, 0.6, 0.6), fill=_rgb(fill) if fill else None, width=0.5)
                body = tc.find("a:txBody", NS)
                if body is not None:
                    self.text(page, cell, body, False)
                x += w
            y += h


def _hex(val) -> str | None:
    """'#rrggbb' when `val` really is six hex digits (it goes into css)."""
    return "#" + val if isinstance(val, str) and re.fullmatch(r"[0-9A-Fa-f]{6}", val) else None


def _rgb(col: str) -> tuple[float, float, float]:
    col = col.lstrip("#")
    return tuple(int(col[i:i + 2], 16) / 255 for i in (0, 2, 4))  # type: ignore[return-value]


def pptx_to_pdf(src: Path, out: Path) -> None:
    s = Slides(src)
    try:
        s.render(out)
    finally:
        s.pkg.close()

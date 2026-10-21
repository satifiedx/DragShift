"""Real installed fonts for PDFs drawn from HTML.

MuPDF's built-in sans (Nimbus) has no Cyrillic of its own: italic Cyrillic comes out with broken
letter spacing. So the fonts a document asks for are looked up among the ones Windows has, with
Arial behind them, and handed to MuPDF as @font-face rules."""
from __future__ import annotations

import os
import sys
from functools import lru_cache
from pathlib import Path

FALLBACK = "Arial"
VARIANTS = (("", "normal", "normal"), (" Bold", "bold", "normal"), (" Italic", "normal", "italic"),
            (" Bold Italic", "bold", "italic"))


@lru_cache(maxsize=1)
def installed() -> dict[str, Path]:
    """'calibri bold italic' -> font file, from the registry (machine and per-user fonts)."""
    if sys.platform != "win32":
        return {}
    import winreg
    windir = Path(os.environ.get("WINDIR", r"C:\Windows")) / "Fonts"
    out: dict[str, Path] = {}
    key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts"
    for hive in (winreg.HKEY_LOCAL_MACHINE, winreg.HKEY_CURRENT_USER):
        try:
            k = winreg.OpenKey(hive, key)
        except OSError:
            continue
        with k:
            i = 0
            while True:
                try:
                    name, value, _ = winreg.EnumValue(k, i)
                except OSError:
                    break
                i += 1
                if not isinstance(value, str) or not value.lower().endswith((".ttf", ".otf", ".ttc")):
                    continue
                path = Path(value) if os.path.isabs(value) else windir / value
                name = name.rsplit(" (", 1)[0]
                for part in name.split(" & "):          # "Cambria & Cambria Math"
                    out.setdefault(part.strip().lower(), path)
    return out


def faces(families) -> tuple[str, list[str]]:
    """(@font-face css, the font-family list to use) for the wanted families, Arial last.
    Families that aren't installed are dropped; with no fonts at all both come back empty."""
    have = installed()
    rules, names = [], []
    for fam in list(dict.fromkeys(f for f in families if f)) + [FALLBACK]:
        if fam.lower() not in have or fam in names:
            continue
        names.append(fam)
        for suffix, weight, style in VARIANTS:
            path = have.get((fam + suffix).lower())
            if path is None and suffix == " Italic":
                path = have.get(f"{fam} Oblique".lower())
            if path is None:
                continue                        # MuPDF fakes the missing bold/italic from the regular one
            url = path.as_posix().replace("'", "\\'")
            rules.append(f"@font-face {{ font-family: '{fam}'; font-weight: {weight}; font-style: {style}; "
                         f"src: url('{url}'); }}")
    return "\n".join(rules), names


def stack(names: list[str]) -> str:
    """CSS font-family value."""
    return ", ".join(f"'{n}'" for n in names) + (", sans-serif" if names else "sans-serif")


@lru_cache(maxsize=1)
def archive():
    """An archive rooted at the drive so absolute font urls resolve."""
    from .convert import fitz
    return fitz.Archive(Path(os.environ.get("WINDIR", r"C:\Windows")).anchor) if sys.platform == "win32" else None


def css() -> str:
    """Arial for everything, for documents that don't name their fonts. Goes after the page css
    (its `*` rule loses to any element rule there, like `pre { monospace }`)."""
    rules, names = faces([])
    return f"\n{rules}\n* {{ font-family: {stack(names)}; }}\n" if names else ""


def fix_text_map(doc) -> None:
    """Arial and friends draw space and hyphen with the same glyphs as no-break space and soft
    hyphen, and MuPDF names those glyphs by the latter in the PDF's ToUnicode maps: copied or
    searched text then has no plain spaces. Name them as space and hyphen again."""
    import re
    done = set()
    for x in range(1, doc.xref_length()):
        kind, val = doc.xref_get_key(x, "ToUnicode")
        if kind != "xref":
            continue
        sx = int(val.split()[0])
        if sx in done:
            continue
        done.add(sx)
        cmap = doc.xref_stream(sx).decode("latin-1")

        def block(m):
            b = re.sub(r"(<[0-9a-fA-F]+>\s*)<00[aA]0>", r"\1<0020>", m.group(0))
            return re.sub(r"(<[0-9a-fA-F]+>\s*)<00[aA][dD]>", r"\1<002D>", b)

        fixed = re.sub(r"beginbfchar.*?endbfchar", block, cmap, flags=re.S)
        if fixed != cmap:
            doc.update_stream(sx, fixed.encode("latin-1"))


def fix_file(path) -> None:
    """fix_text_map for a PDF already written to disk."""
    from .convert import fitz
    with fitz.open(path) as doc:
        before = doc.xref_length()
        fix_text_map(doc)
        if doc.is_dirty or doc.xref_length() != before:
            doc.saveIncr()

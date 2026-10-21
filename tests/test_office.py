"""Excel / PowerPoint to PDF without Office, and saving a clipboard picture."""
import io
import zipfile

import pytest
from PIL import Image

from dragshift import convert, office
from dragshift.convert import fitz

RELS = "http://schemas.openxmlformats.org/package/2006/relationships"
OD = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
S = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
P = "http://schemas.openxmlformats.org/presentationml/2006/main"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"


def rels(*items):
    body = "".join(f'<Relationship Id="{i}" Type="{OD}/{t}" Target="{tg}"/>' for i, t, tg in items)
    return f'<Relationships xmlns="{RELS}">{body}</Relationships>'


def make_xlsx(path):
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("xl/workbook.xml", f'<workbook xmlns="{S}" xmlns:r="{OD}"><sheets>'
                   '<sheet name="Продажи" sheetId="1" r:id="rId1"/>'
                   '<sheet name="Hidden" sheetId="2" r:id="rId2" state="hidden"/></sheets></workbook>')
        z.writestr("xl/_rels/workbook.xml.rels", rels(("rId1", "worksheet", "worksheets/sheet1.xml"),
                                                      ("rId2", "worksheet", "worksheets/sheet2.xml")))
        z.writestr("xl/sharedStrings.xml", f'<sst xmlns="{S}"><si><t>Товар</t></si><si><t>Дата</t></si>'
                   '<si><r><t>Чай </t></r><r><t>зелёный</t></r></si></sst>')
        z.writestr("xl/styles.xml", f'<styleSheet xmlns="{S}"><cellXfs count="2"><xf numFmtId="0"/>'
                   '<xf numFmtId="14"/></cellXfs></styleSheet>')
        z.writestr("xl/worksheets/sheet1.xml", f'<worksheet xmlns="{S}"><sheetData>'
                   '<row r="2"><c r="B2" t="s"><v>0</v></c><c r="C2" t="s"><v>1</v></c><c r="D2"><v>1.5</v></c></row>'
                   '<row r="3"><c r="B3" t="s"><v>2</v></c><c r="C3" s="1"><v>45658</v></c><c r="D3"><v>42</v></c></row>'
                   '</sheetData></worksheet>')
        z.writestr("xl/worksheets/sheet2.xml", f'<worksheet xmlns="{S}"><sheetData>'
                   '<row r="1"><c r="A1" t="inlineStr"><is><t>secret</t></is></c></row></sheetData></worksheet>')


def png_bytes(color=(200, 30, 30)):
    b = io.BytesIO()
    Image.new("RGB", (40, 30), color).save(b, "PNG")
    return b.getvalue()


def make_pptx(path):
    emu = 12700
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("ppt/presentation.xml", f'<p:presentation xmlns:p="{P}" xmlns:r="{OD}">'
                   '<p:sldIdLst><p:sldId id="256" r:id="rId2"/><p:sldId id="257" r:id="rId3"/></p:sldIdLst>'
                   f'<p:sldSz cx="{720 * emu}" cy="{405 * emu}"/></p:presentation>')
        z.writestr("ppt/_rels/presentation.xml.rels", rels(("rId2", "slide", "slides/slide1.xml"),
                                                           ("rId3", "slide", "slides/slide2.xml")))
        # the title sits where the layout puts it: the slide gives no position
        z.writestr("ppt/slideLayouts/slideLayout1.xml", f'<p:sldLayout xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree>'
                   '<p:sp><p:nvSpPr><p:cNvPr id="2" name="t"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>'
                   f'<p:spPr><a:xfrm><a:off x="{36 * emu}" y="{20 * emu}"/><a:ext cx="{648 * emu}" cy="{60 * emu}"/></a:xfrm></p:spPr></p:sp>'
                   '</p:spTree></p:cSld></p:sldLayout>')
        z.writestr("ppt/slideLayouts/_rels/slideLayout1.xml.rels", rels(("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")))
        z.writestr("ppt/slideMasters/slideMaster1.xml", f'<p:sldMaster xmlns:p="{P}" xmlns:a="{A}"><p:cSld>'
                   '<p:bg><p:bgPr><a:solidFill><a:schemeClr val="bg2"/></a:solidFill></p:bgPr></p:bg><p:spTree/></p:cSld></p:sldMaster>')
        z.writestr("ppt/slideMasters/_rels/slideMaster1.xml.rels", rels(("rId1", "theme", "../theme/theme1.xml")))
        z.writestr("ppt/theme/theme1.xml", f'<a:theme xmlns:a="{A}"><a:themeElements><a:clrScheme name="x">'
                   '<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1>'
                   '<a:dk2><a:srgbClr val="1F1F1F"/></a:dk2><a:lt2><a:srgbClr val="EEE8D5"/></a:lt2>'
                   '</a:clrScheme></a:themeElements></a:theme>')
        sp_title = ('<p:sp><p:nvSpPr><p:cNvPr id="2" name="t"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>'
                    '<p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang="ru"/><a:t>Квартальный отчёт</a:t></a:r></a:p></p:txBody></p:sp>')
        box = (f'<p:sp><p:nvSpPr><p:cNvPr id="3" name="b"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>'
               f'<a:xfrm><a:off x="{36 * emu}" y="{100 * emu}"/><a:ext cx="{300 * emu}" cy="{120 * emu}"/></a:xfrm>'
               '<a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="2E7D32"/></a:solidFill></p:spPr>'
               '<p:txBody><a:bodyPr anchor="ctr"/><a:p><a:pPr algn="ctr"/><a:r><a:rPr sz="2400" b="1">'
               '<a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:rPr><a:t>Выручка +12%</a:t></a:r></a:p></p:txBody></p:sp>')
        pic = ('<p:grpSp><p:nvGrpSpPr><p:cNvPr id="5" name="g"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm>'
               f'<a:off x="{400 * emu}" y="{100 * emu}"/><a:ext cx="{200 * emu}" cy="{150 * emu}"/>'
               f'<a:chOff x="0" y="0"/><a:chExt cx="{100 * emu}" cy="{75 * emu}"/></a:xfrm></p:grpSpPr>'
               '<p:pic><p:nvPicPr><p:cNvPr id="4" name="p"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>'
               '<p:blipFill><a:blip xmlns:r="' + OD + '" r:embed="rId9"/></p:blipFill>'
               f'<p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{100 * emu}" cy="{75 * emu}"/></a:xfrm></p:spPr></p:pic></p:grpSp>')
        z.writestr("ppt/slides/slide1.xml", f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree>'
                   f'{sp_title}{box}{pic}</p:spTree></p:cSld></p:sld>')
        z.writestr("ppt/slides/_rels/slide1.xml.rels",
                   rels(("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"), ("rId9", "image", "../media/image1.png")))
        table = (f'<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="6" name="tb"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>'
                 f'<p:xfrm><a:off x="{50 * emu}" y="{50 * emu}"/><a:ext cx="{400 * emu}" cy="{80 * emu}"/></p:xfrm>'
                 '<a:graphic><a:graphicData><a:tbl><a:tblGrid>'
                 f'<a:gridCol w="{200 * emu}"/><a:gridCol w="{200 * emu}"/></a:tblGrid>'
                 f'<a:tr h="{40 * emu}"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Город</a:t></a:r></a:p></a:txBody></a:tc>'
                 '<a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Киев</a:t></a:r></a:p></a:txBody></a:tc></a:tr>'
                 '</a:tbl></a:graphicData></a:graphic></p:graphicFrame>')
        z.writestr("ppt/slides/slide2.xml", f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree>{table}</p:spTree></p:cSld></p:sld>')
        z.writestr("ppt/slides/_rels/slide2.xml.rels", rels(("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml")))
        z.writestr("ppt/media/image1.png", png_bytes())


@pytest.fixture(autouse=True)
def no_office(monkeypatch):
    # the built-in renderers are what's under test, whatever is installed on this machine
    monkeypatch.setattr(office, "office_export", lambda *a: False)
    monkeypatch.setattr(office, "libreoffice_export", lambda *a: False)


def test_xlsx_reads_strings_numbers_and_dates(tmp_path):
    src = tmp_path / "book.xlsx"
    make_xlsx(src)
    (name, rows), = office.read_xlsx(src)
    assert name == "Продажи"
    assert rows == [["Товар", "Дата", "1.5"], ["Чай зелёный", "2025-01-01", "42"]]


def test_xlsx_to_pdf(tmp_path):
    src = tmp_path / "book.xlsx"
    make_xlsx(src)
    out, = convert.convert(src, "office", "PDF")
    with fitz.open(out) as d:
        text = d[0].get_text()
    assert "Чай зелёный" in text and "2025-01-01" in text and "secret" not in text


def test_pptx_to_pdf(tmp_path):
    src = tmp_path / "deck.pptx"
    make_pptx(src)
    out, = convert.convert(src, "office", "PDF")
    with fitz.open(out) as d:
        assert d.page_count == 2
        p1 = d[0]
        assert (p1.rect.width, p1.rect.height) == (720, 405)
        text = p1.get_text()
        assert "Квартальный отчёт" in text and "Выручка +12%" in text
        title = p1.search_for("Квартальный")[0]
        assert 15 <= title.y0 <= 80          # placed by the layout
        img = p1.get_image_rects(p1.get_images()[0][0])[0]
        assert abs(img.x0 - 400) < 1 and abs(img.width - 200) < 1   # group transform applied
        pix = p1.get_pixmap(clip=fitz.Rect(5, 5, 6, 6))
        assert pix.pixel(0, 0) == (0xEE, 0xE8, 0xD5)                # master background via the theme
        assert "Город" in d[1].get_text() and "Киев" in d[1].get_text()


def test_old_formats_need_office(tmp_path):
    src = tmp_path / "old.ppt"
    src.write_bytes(b"\xd0\xcf\x11\xe0")
    with pytest.raises(convert.UserError):
        convert.convert(src, "office", "PDF")


def test_save_clipboard(tmp_path, monkeypatch):
    from PIL import ImageGrab
    monkeypatch.setattr(ImageGrab, "grabclipboard", lambda: Image.new("RGBA", (64, 48), (10, 20, 30, 255)))
    out, = convert.save_clipboard("webp", tmp_path / "Screenshots")
    assert out.suffix == ".webp" and out.parent.name == "Screenshots"
    with Image.open(out) as im:
        assert im.size == (64, 48)
    monkeypatch.setattr(ImageGrab, "grabclipboard", lambda: None)
    with pytest.raises(convert.UserError):
        convert.save_clipboard("PNG", tmp_path)


def test_save_clipboard_file_from_snipping_tool(tmp_path, monkeypatch):
    # Win+Shift+S hands over its temp file; the copy must land in the chosen folder, named sensibly
    from PIL import ImageGrab
    temp = tmp_path / "ScreenClip"
    temp.mkdir()
    snip = temp / "{95755198-129F-4131-9D16-5CAD89F6E8AA}.png"
    Image.new("RGB", (30, 20), "red").save(snip)
    monkeypatch.setattr(ImageGrab, "grabclipboard", lambda: [str(snip)])
    dest = tmp_path / "Mine"
    out, = convert.save_clipboard("PNG", dest)
    assert out.parent == dest and "{" not in out.name and out.suffix == ".png"
    assert list(temp.iterdir()) == [snip]

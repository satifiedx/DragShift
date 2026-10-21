"""The Russian table must keep every {} placeholder, and tr() must fill them in order."""
from dragshift import i18n


def test_placeholders_match():
    bad = [k for k, v in i18n.RU.items() if k.count("{}") != v.count("{}")]
    assert not bad


def test_tr_fills_in_order(monkeypatch):
    monkeypatch.setattr(i18n, "_lang", "ru")
    assert i18n.tr("{} pages → {} {}s", 3, 3, "JPG") == "Страниц: 3 → 3 JPG"
    assert i18n.tr("not in the table {}", 5) == "not in the table 5"
    monkeypatch.setattr(i18n, "_lang", "en")
    assert i18n.tr("Extracted {} images", 2) == "Extracted 2 images"

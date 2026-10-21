"""The Russian and Ukrainian tables must keep every {} placeholder and cover the same strings,
and tr() must fill them in order."""
from dragshift import i18n


def test_placeholders_match():
    for table in (i18n.RU, i18n.UK):
        bad = [k for k, v in table.items() if k.count("{}") != v.count("{}")]
        assert not bad


def test_ukrainian_covers_russian():
    assert set(i18n.UK) == set(i18n.RU)


def test_tr_fills_in_order(monkeypatch):
    monkeypatch.setattr(i18n, "_lang", "ru")
    assert i18n.tr("{} pages → {} {}s", 3, 3, "JPG") == "Страниц: 3 → 3 JPG"
    assert i18n.tr("not in the table {}", 5) == "not in the table 5"
    monkeypatch.setattr(i18n, "_lang", "uk")
    assert i18n.tr("{} pages → {} {}s", 3, 3, "JPG") == "Сторінок: 3 → 3 JPG"
    monkeypatch.setattr(i18n, "_lang", "en")
    assert i18n.tr("Extracted {} images", 2) == "Extracted 2 images"

import pytest

from dragshift import i18n


@pytest.fixture(autouse=True)
def english(monkeypatch):
    # results are checked in English, whatever language this PC's DragShift is set to
    monkeypatch.setattr(i18n, "_lang", "en")

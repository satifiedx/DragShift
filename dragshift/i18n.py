"""English / Russian for the editor windows. Strings are English in the code; tr() looks them up.

tr("{} files", 3) fills "{}" placeholders in order after translating."""
from __future__ import annotations

import os
import sys

_lang: str | None = None


def _system_is_russian() -> bool:
    if sys.platform != "win32":
        import locale
        return (locale.getlocale()[0] or "").lower().startswith("ru")
    try:
        import ctypes
        return ctypes.windll.kernel32.GetUserDefaultUILanguage() & 0x3FF == 0x19
    except Exception:
        return False


def lang() -> str:
    global _lang
    if _lang is None:
        from .settings import Settings
        pick = Settings().get("lang")
        if pick in ("en", "ru"):
            _lang = pick
        else:
            env = os.environ.get("DRAGSHIFT_LANG")
            _lang = env if env in ("en", "ru") else ("ru" if _system_is_russian() else "en")
    return _lang


def reload():
    global _lang
    _lang = None


def tr(s: str, *args) -> str:
    out = RU.get(s, s) if lang() == "ru" else s
    for a in args:
        out = out.replace("{}", str(a), 1)
    return out


RU: dict[str, str] = {
    # ---- common
    "Cancel": "Отмена", "Apply": "Применить", "Save": "Сохранить", "Done": "Готово", "Copy": "Копировать",
    "Undo": "Отменить", "Redo": "Вернуть", "Reset": "Сбросить", "Show": "Показать", "Confirm": "Повторите",
    "Close (Esc)": "Закрыть (Esc)", "Working…": "Работаю…", "Couldn't open {}": "Не открылось: {}",
    "{} files": "Файлов: {}", "{} pages": "Страниц: {}", "1 page": "1 страница", "s": "с",
    "B": "Б", "KB": "КБ", "MB": "МБ", "GB": "ГБ", "px": "пикс", "W": "Ш", "H": "В",
    "Size": "Размер", "Quality": "Качество", "Background": "Фон", "Settings": "Настройки",
    "Original": "Оригинал", "Free": "Свободно", "Auto": "Авто", "Exact": "Точно",
    # ---- tool names (window titles, menu)
    "Crop Image": "Обрезка", "Crop": "Обрезать", "Add BG": "Фон", "Annotate": "Пометки", "Redact": "Скрыть",
    "Edit": "Правка", "Metadata": "Метаданные", "Compress": "Сжать", "Resize": "Размер",
    "Password-protect PDF": "Пароль на PDF", "Protect": "Поставить пароль", "Split": "Разделить",
    "Rotate": "Повернуть", "Merge": "Объединить", "Extract": "Картинки",
    "Trim": "Обрезать", "Crop Video": "Обрезка кадра", "Speed": "Скорость",
    # ---- working
    "Cropping…": "Обрезаю…", "Adding background…": "Добавляю фон…", "Saving annotations…": "Сохраняю пометки…",
    "Redacting…": "Закрашиваю…", "Applying edits…": "Применяю правки…", "Removing metadata…": "Убираю метаданные…",
    "Removing location…": "Убираю геолокацию…", "Compressing…": "Сжимаю…", "Resizing…": "Меняю размер…",
    "Encrypting…": "Шифрую…",
    # ---- crop
    "Aspect ratio": "Пропорции", "Double-click the image to reset": "Двойной клик по картинке сбрасывает рамку",
    "rotated {}°": "повёрнуто на {}°", "flipped": "отражено",
    # ---- add background
    "Padding": "Отступ", "Corners": "Углы", "Shadow": "Тень", "Angle": "Угол", "Custom colour": "Свой цвет",
    "Background colour": "Цвет фона", "Copy the result to the clipboard": "Скопировать результат в буфер",
    "Save with Background": "Сохранить с фоном", "Output  {} px  ·  PNG": "Итог  {} пикс  ·  PNG",
    "Copied ✓": "Скопировано ✓",
    "Sunset": "Закат", "Prism": "Призма", "Peach": "Персик", "Aurora": "Сияние", "Mint": "Мята",
    "Ocean": "Океан", "Dusk": "Сумерки", "Lemon": "Лимон", "Night": "Ночь", "Rose": "Роза",
    # ---- annotate
    "➜ Arrow": "➜ Стрелка", "▢ Box": "▢ Рамка", "◯ Circle": "◯ Круг", "✎ Pen": "✎ Карандаш",
    "▬ Marker": "▬ Маркер", "T Text": "T Текст", "① Step": "① Шаг", "Type, then Enter": "Пишите, потом Enter",
    "arrow": "стрелка", "box": "рамка", "circle": "круг", "stroke": "линия", "highlight": "маркер",
    "label": "надпись", "step": "шаг",
    # ---- redact
    "Drag over anything you want hidden. It's burned into the pixels — it can't be undone in the copy.":
        "Выделите всё, что надо скрыть. Это впечатывается в пиксели, в копии уже не отменить.",
    "Black box": "Чёрная плашка", "Pixelate": "Пиксели", "Blur": "Размытие", "{} areas": "Областей: {}",
    "{} hidden": "скрыто: {}", "black boxes": "чёрными плашками", "pixelated": "пикселями", "blurred": "размытием",
    # ---- edit
    "✦ Auto": "✦ Авто", "Automatic light and colour": "Автоматический свет и цвет", "B&&W": "Ч/Б",
    "Hold to compare": "Зажмите, чтобы сравнить", "Rotate 90°": "Повернуть на 90°",
    "Brightness": "Яркость", "Contrast": "Контраст", "Saturation": "Насыщенность", "Warmth": "Теплота",
    "Sharpness": "Резкость",
    # ---- metadata
    "Show all {} fields": "Показать все поля ({})", "Remove all metadata": "Удалить все метаданные",
    "Remove location only": "Убрать только геолокацию", "removed {} fields": "удалено полей: {}",
    " incl. GPS location": ", включая GPS", "GPS removed, camera info kept": "GPS убран, данные камеры остались",
    "No personal metadata found.": "Личных метаданных не нашлось.",
    "⚠  This photo reveals where it was taken.": "⚠  По этому фото видно, где оно снято.",
    "Field": "Поле", "Value": "Значение", "Open map": "Открыть карту",
    "Camera": "Камера", "Taken": "Снято", "Software": "Программа", "Exposure": "Выдержка",
    "Aperture": "Диафрагма", "ISO": "ISO", "Location": "Место",
    "title": "Название", "author": "Автор", "creator": "Создано в", "producer": "PDF-движок", "Pages": "Страницы",
    # ---- compress
    "Target size": "Нужный вес", "Make each file under": "Каждый файл меньше чем",
    "Convert to WebP (usually the smallest)": "Перевести в WebP (обычно легче всех)",
    "Original size": "Исходный размер", "Max 3840 px": "До 3840 пикс", "Max 2560 px": "До 2560 пикс",
    "Max 1920 px": "До 1920 пикс", "Max 1280 px": "До 1280 пикс", "Max 800 px": "До 800 пикс",
    "Compressed": "Сжато", "Compress {} files": "Сжать файлы ({})", "quality ≈ {}": "качество ≈ {}",
    "≈ {}% smaller": "≈ на {}% легче", "target {}": "цель {}", "quality {}": "качество {}",
    "Already well compressed — try a smaller size or WebP": "Уже неплохо сжато, попробуйте размер поменьше или WebP",
    # ---- resize
    "Presets — percent or longest side in px": "Пресеты: проценты или длинная сторона в пикселях",
    "Exact size": "Точный размер", "Keep proportions": "Сохранять пропорции",
    "Resize {} files": "Изменить размер файлов ({})",
    # ---- protect
    "AES-256 encryption. Anyone opening the PDF will need this password.":
        "Шифрование AES-256. Без этого пароля PDF не открыть.",
    "Password": "Пароль", "Use at least 4 characters.": "Минимум 4 символа.",
    "Passwords don't match yet.": "Пароли пока не совпадают.", "Looks good · {} password": "Норм · пароль {}",
    "strong": "надёжный", "okay": "средний", "weak": "слабый",
    # ---- video / audio editors
    "Fade in": "Нарастание", "Fade out": "Затухание", "From": "С", "to": "до",
    "[ Start here": "[ Начало тут", "End here ]": "Конец тут ]",
    "Set the start to the playhead (I)": "Начало там, где курсор (I)",
    "Set the end to the playhead (O)": "Конец там, где курсор (O)",
    "▶ Play": "▶ Слушать", "■ Stop": "■ Стоп", "Listen from the playhead (Space)": "Слушать от курсора (Пробел)",
    "Fast cut: no re-encoding, but it starts at the nearest keyframe":
        "Быстрая обрезка: без перекодирования, но начнётся с ближайшего ключевого кадра",
    "Save this frame": "Сохранить кадр", "Save the frame under the playhead as PNG": "Сохранить кадр под курсором в PNG",
    "Keep the pitch (voices don't sound like chipmunks)": "Сохранить тон (голоса не как у бурундуков)",
    "Change speed": "Изменить скорость", "Preview at": "Кадр на", "{}  →  {}": "{}  →  {}",
    "This file looks damaged, or it isn't a video or audio format DragShift can read.":
        "Файл битый или это не тот формат видео/аудио, который DragShift умеет читать.",
    "DragShift can't tell how long this file is.": "DragShift не может понять длительность этого файла.",
    "This file has no video, only sound.": "В этом файле нет видео, только звук.",
    # ---- settings / welcome
    "Language": "Язык", "Same as Windows": "Как в Windows", "Theme": "Тема", "Dark": "Тёмная", "Light": "Светлая", "Open the menu with": "Открывать меню клавишей",
    "…pressed while you’re dragging a file": "…нажатой, пока тащите файл",
    "Save results": "Куда сохранять", "Next to the original": "Рядом с оригиналом",
    "In a “DragShift” subfolder": "В подпапку «DragShift»", "JPG / WebP quality": "Качество JPG / WebP",
    "PDF → image resolution": "PDF → картинка, разрешение", "150 dpi · small": "150 dpi · лёгкие",
    "200 dpi · balanced": "200 dpi · баланс", "300 dpi · print": "300 dpi · для печати",
    "Video": "Видео", "Use the graphics card when it helps": "Использовать видеокарту, когда это быстрее",
    "NVIDIA NVENC, Intel Quick Sync or AMD AMF: several times faster video compression. DragShift checks that "
    "it works and falls back to the CPU.":
        "NVIDIA NVENC, Intel Quick Sync или AMD AMF: видео сжимается в разы быстрее. DragShift сам проверит, "
        "что всё работает, иначе возьмёт процессор.",
    "DragShift {}  ·  originals are never modified": "DragShift {}  ·  оригиналы никогда не меняются",
    "Quit DragShift": "Выйти из DragShift",
    "The new language applies to menus and windows opened from now on.":
        "Новый язык применится к меню и окнам, которые откроются дальше.",
    "Welcome to DragShift": "Добро пожаловать в DragShift", "Got it": "Понятно",
    "Start dragging any file": "Начните тащить любой файл", "Press {}": "Нажмите {}",
    "the menu opens right under your cursor": "меню откроется прямо под курсором",
    "Drop it on a slice": "Бросьте на нужный сектор",
    "the result is saved next to the original": "результат сохранится рядом с оригиналом",
    "Tap {} again": "Нажмите {} ещё раз",
    "to switch between Convert and Tools": "чтобы переключаться между Конвертером и Инструментами",
    "Start DragShift with Windows": "Запускать DragShift вместе с Windows", "Start with Windows": "Запуск с Windows",
    "Open DragShift at login": "Открывать DragShift при входе", "Open at login": "Открывать при входе",
    "in Finder or on the desktop": "в Finder или на рабочем столе",
    "in your file manager (Thunar, Nautilus…)": "в файловом менеджере (Thunar, Nautilus…)",
    "in File Explorer or on the desktop": "в Проводнике или на рабочем столе",
    # ---- results
    "same size": "тот же вес", "{}% smaller": "на {}% легче", "{}% larger": "на {}% тяжелее",
    "{} files → 1 {}": "{} файлов → 1 {}", "{} files → {}": "{} файлов → {}", "Compressed {} files": "Сжато файлов: {}",
    "already well compressed": "уже хорошо сжат", "Background added": "Фон добавлен", "Annotated": "Пометки добавлены",
    "Redacted": "Скрыто", "Metadata removed": "Метаданные удалены", "Split into {} PDFs": "Разделено на {} PDF",
    "Merged {} PDFs": "Объединено PDF: {}", "Rotated 90°": "Повёрнуто на 90°", "Password protected": "Пароль поставлен",
    "Extracted 1 image": "Достана 1 картинка", "Extracted {} images": "Достано картинок: {}",
    "Nothing was written": "Ничего не записалось",
    "{} pages → {} {}s": "Страниц: {} → {} {}", "{}: {} images": "{}: {} картинок",
    "burned into pixels": "впечатано в пиксели", "private copy": "приватная копия", "encrypted": "зашифровано",
    "Cropped": "Обрезано", "Resized": "Размер изменён", "Edited": "Отредактировано",
    # ---- file name suffixes
    "Combined": "Вместе", "Merged": "Объединено", "Protected": "С паролем", "Rotated": "Повёрнуто",
    "Clean": "Чистый", "No Location": "Без геолокации", "With Background": "С фоном", "page {}": "стр. {}",
    # ---- errors
    "This PDF is password-protected. Remove the password first.": "Этот PDF под паролем. Сначала снимите пароль.",
    "No images found in this PDF.": "В этом PDF нет картинок.",
    "No permission to write next to this file. Try a folder you own, or set Settings → Save results → subfolder.":
        "Нет прав писать рядом с этим файлом. Возьмите свою папку или включите Настройки → Куда сохранять → подпапка.",
    "This file doesn't look like an image DragShift can read.":
        "Это не похоже на картинку, которую DragShift умеет читать.",
    "The file was moved or deleted.": "Файл перенесли или удалили.",
    "Not enough memory for a file this big.": "Не хватает памяти для такого большого файла.",
}
# ---- end table

"""English / Russian / Ukrainian for the editor windows. Strings are English in the code; tr() looks them up.

tr("{} files", 3) fills "{}" placeholders in order after translating."""
from __future__ import annotations

import os
import sys

_lang: str | None = None


def _system_lang() -> str:
    """ru / uk when Windows (or the locale elsewhere) is in that language, else en."""
    if sys.platform != "win32":
        import locale
        loc = (locale.getlocale()[0] or "").lower()
        return "ru" if loc.startswith("ru") else "uk" if loc.startswith(("uk", "ukrainian")) else "en"
    try:
        import ctypes
        primary = ctypes.windll.kernel32.GetUserDefaultUILanguage() & 0x3FF
    except Exception:
        return "en"
    return {0x19: "ru", 0x22: "uk"}.get(primary, "en")


def lang() -> str:
    global _lang
    if _lang is None:
        from .settings import Settings
        pick = Settings().get("lang")
        if pick in LANGS:
            _lang = pick
        else:
            env = os.environ.get("DRAGSHIFT_LANG")
            _lang = env if env in LANGS else _system_lang()
    return _lang


def reload():
    global _lang
    _lang = None


def tr(s: str, *args) -> str:
    table = TABLES.get(lang())
    out = table.get(s, s) if table else s
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
    "New preset": "Свой пресет",
    "Name": "Название",
    "Video codec": "Видеокодек",
    "Video bitrate": "Битрейт видео",
    "Resolution": "Разрешение",
    "Audio codec": "Аудиокодек",
    "Audio bitrate": "Битрейт звука",
    "Only save": "Только сохранить",
    "Save and compress": "Сохранить и сжать",
    "Your presets": "Ваши пресеты",
    "Delete": "Удалить",
    "{} kbps": "{} кбит/с",
    "by quality": "по качеству",
    "About {} for this file": "Для этого файла примерно {}",
    "The size depends on the picture": "Размер зависит от картинки",
    "Compress video to": "Сжимать видео в",
    "H.265 · half the size": "H.265 · в 2 раза меньше",
    "H.264 · plays everywhere": "H.264 · играет везде",
    "About half the size of H.264 at the same quality. Plays on phones, TVs and Windows 10/11.": "Файл примерно в 2 раза меньше, чем в H.264, при том же качестве. Играет на телефонах, телевизорах и Windows 10/11.",
    "Plays everywhere, even on old devices and in every messenger. Files come out bigger.": "Играет везде, даже на старых устройствах и в любом мессенджере. Файлы больше.",
    "The smallest files, but slow to encode without a new graphics card. Old devices won't play it.": "Самые маленькие файлы, но без новой видеокарты кодирует долго. Старые устройства не потянут.",
    "Open format for the web (WebM): browsers, YouTube. The sound becomes Opus.": "Открытый формат для веба (WebM): браузеры, YouTube. Звук будет в Opus.",
    "The usual sound for MP4. Plays everywhere.": "Обычный звук для MP4. Играет везде.",
    "Best quality per kilobyte: voice and music stay clean at a low bitrate.": "Лучшее качество на килобайт: голос и музыка звучат чисто даже при низком битрейте.",
    "Old but universal.": "Старый, но понятен всем.",
    "The sound stays as it is, no quality loss and no time spent.": "Звук остаётся как есть: без потерь и без лишнего времени.",
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
    "Old {} files need Microsoft Office or LibreOffice to convert.":
        "Старые файлы {} конвертируются только через Microsoft Office или LibreOffice.",
    "This file is damaged or isn't really {}.": "Файл повреждён или это не {}.",
    "This spreadsheet is empty.": "В таблице пусто.",
    "This presentation has no slides.": "В презентации нет слайдов.",
    "There's no picture on the clipboard. Take a screenshot with Win+Shift+S first.":
        "В буфере нет картинки. Сначала сделайте скриншот через Win+Shift+S.",
    "Screenshot": "Скриншот",
    "Screenshot saved": "Скриншот сохранён",
}


UK: dict[str, str] = {
    # ---- common
    "Cancel": "Скасувати", "Apply": "Застосувати", "Save": "Зберегти", "Done": "Готово", "Copy": "Копіювати",
    "Undo": "Скасувати", "Redo": "Повернути", "Reset": "Скинути", "Show": "Показати", "Confirm": "Повторіть",
    "Close (Esc)": "Закрити (Esc)", "Working…": "Працюю…", "Couldn't open {}": "Не відкрилося: {}",
    "{} files": "Файлів: {}", "{} pages": "Сторінок: {}", "1 page": "1 сторінка", "s": "с",
    "B": "Б", "KB": "КБ", "MB": "МБ", "GB": "ГБ", "px": "пікс", "W": "Ш", "H": "В",
    "Size": "Розмір", "Quality": "Якість", "Background": "Тло", "Settings": "Налаштування",
    "Original": "Оригінал", "Free": "Вільно", "Auto": "Авто", "Exact": "Точно",
    # ---- tool names (window titles, menu)
    "Crop Image": "Обрізання", "Crop": "Обрізати", "Add BG": "Тло", "Annotate": "Позначки", "Redact": "Приховати",
    "Edit": "Правка", "Metadata": "Метадані", "Compress": "Стиснути", "Resize": "Розмір",
    "Password-protect PDF": "Пароль на PDF", "Protect": "Поставити пароль", "Split": "Розділити",
    "Rotate": "Повернути", "Merge": "Об'єднати", "Extract": "Картинки",
    "Trim": "Обрізати", "Crop Video": "Обрізання кадру", "Speed": "Швидкість",
    # ---- working
    "Cropping…": "Обрізаю…", "Adding background…": "Додаю тло…", "Saving annotations…": "Зберігаю позначки…",
    "Redacting…": "Зафарбовую…", "Applying edits…": "Застосовую правки…", "Removing metadata…": "Прибираю метадані…",
    "Removing location…": "Прибираю геолокацію…", "Compressing…": "Стискаю…", "Resizing…": "Змінюю розмір…",
    "Encrypting…": "Шифрую…",
    # ---- crop
    "Aspect ratio": "Пропорції", "Double-click the image to reset": "Подвійний клік по картинці скидає рамку",
    "rotated {}°": "повернуто на {}°", "flipped": "віддзеркалено",
    # ---- add background
    "Padding": "Відступ", "Corners": "Кути", "Shadow": "Тінь", "Angle": "Кут", "Custom colour": "Свій колір",
    "Background colour": "Колір тла", "Copy the result to the clipboard": "Скопіювати результат у буфер",
    "Save with Background": "Зберегти з тлом", "Output  {} px  ·  PNG": "Результат  {} пікс  ·  PNG",
    "Copied ✓": "Скопійовано ✓",
    "Sunset": "Захід", "Prism": "Призма", "Peach": "Персик", "Aurora": "Сяйво", "Mint": "М'ята",
    "Ocean": "Океан", "Dusk": "Сутінки", "Lemon": "Лимон", "Night": "Ніч", "Rose": "Троянда",
    # ---- annotate
    "➜ Arrow": "➜ Стрілка", "▢ Box": "▢ Рамка", "◯ Circle": "◯ Коло", "✎ Pen": "✎ Олівець",
    "▬ Marker": "▬ Маркер", "T Text": "T Текст", "① Step": "① Крок", "Type, then Enter": "Пишіть, потім Enter",
    "arrow": "стрілка", "box": "рамка", "circle": "коло", "stroke": "лінія", "highlight": "маркер",
    "label": "напис", "step": "крок",
    # ---- redact
    "Drag over anything you want hidden. It's burned into the pixels — it can't be undone in the copy.":
        "Виділіть усе, що треба приховати. Це впікається в пікселі, у копії вже не скасувати.",
    "Black box": "Чорна плашка", "Pixelate": "Пікселі", "Blur": "Розмиття", "{} areas": "Областей: {}",
    "{} hidden": "приховано: {}", "black boxes": "чорними плашками", "pixelated": "пікселями", "blurred": "розмиттям",
    # ---- edit
    "✦ Auto": "✦ Авто", "Automatic light and colour": "Автоматичне світло й колір", "B&&W": "Ч/Б",
    "Hold to compare": "Затисніть, щоб порівняти", "Rotate 90°": "Повернути на 90°",
    "Brightness": "Яскравість", "Contrast": "Контраст", "Saturation": "Насиченість", "Warmth": "Теплота",
    "Sharpness": "Різкість",
    # ---- metadata
    "Show all {} fields": "Показати всі поля ({})", "Remove all metadata": "Видалити всі метадані",
    "Remove location only": "Прибрати лише геолокацію", "removed {} fields": "видалено полів: {}",
    " incl. GPS location": ", включно з GPS", "GPS removed, camera info kept": "GPS прибрано, дані камери лишилися",
    "No personal metadata found.": "Особистих метаданих не знайшлося.",
    "⚠  This photo reveals where it was taken.": "⚠  З цього фото видно, де його знято.",
    "Field": "Поле", "Value": "Значення", "Open map": "Відкрити карту",
    "Camera": "Камера", "Taken": "Знято", "Software": "Програма", "Exposure": "Витримка",
    "Aperture": "Діафрагма", "ISO": "ISO", "Location": "Місце",
    "title": "Назва", "author": "Автор", "creator": "Створено в", "producer": "PDF-рушій", "Pages": "Сторінки",
    # ---- compress
    "Target size": "Потрібна вага", "Make each file under": "Кожен файл менший за",
    "Convert to WebP (usually the smallest)": "Перевести у WebP (зазвичай найлегший)",
    "Original size": "Початковий розмір", "Max 3840 px": "До 3840 пікс", "Max 2560 px": "До 2560 пікс",
    "Max 1920 px": "До 1920 пікс", "Max 1280 px": "До 1280 пікс", "Max 800 px": "До 800 пікс",
    "Compressed": "Стиснуто", "Compress {} files": "Стиснути файли ({})", "quality ≈ {}": "якість ≈ {}",
    "≈ {}% smaller": "≈ на {}% легше", "target {}": "ціль {}", "quality {}": "якість {}",
    "Already well compressed — try a smaller size or WebP": "Уже непогано стиснуто, спробуйте менший розмір або WebP",
    # ---- resize
    "Presets — percent or longest side in px": "Пресети: відсотки або довга сторона в пікселях",
    "Exact size": "Точний розмір", "Keep proportions": "Зберігати пропорції",
    "Resize {} files": "Змінити розмір файлів ({})",
    # ---- protect
    "AES-256 encryption. Anyone opening the PDF will need this password.":
        "Шифрування AES-256. Без цього пароля PDF не відкрити.",
    "Password": "Пароль", "Use at least 4 characters.": "Мінімум 4 символи.",
    "Passwords don't match yet.": "Паролі поки не збігаються.", "Looks good · {} password": "Норм · пароль {}",
    "strong": "надійний", "okay": "середній", "weak": "слабкий",
    # ---- video / audio editors
    "Fade in": "Наростання", "Fade out": "Згасання", "From": "З", "to": "до",
    "[ Start here": "[ Початок тут", "End here ]": "Кінець тут ]",
    "Set the start to the playhead (I)": "Початок там, де курсор (I)",
    "Set the end to the playhead (O)": "Кінець там, де курсор (O)",
    "▶ Play": "▶ Слухати", "■ Stop": "■ Стоп", "Listen from the playhead (Space)": "Слухати від курсора (Пробіл)",
    "Fast cut: no re-encoding, but it starts at the nearest keyframe":
        "Швидке обрізання: без перекодування, але почнеться з найближчого ключового кадру",
    "Save this frame": "Зберегти кадр", "Save the frame under the playhead as PNG": "Зберегти кадр під курсором у PNG",
    "Keep the pitch (voices don't sound like chipmunks)": "Зберегти тон (голоси не як у бурундуків)",
    "Change speed": "Змінити швидкість", "Preview at": "Кадр на", "{}  →  {}": "{}  →  {}",
    "New preset": "Свій пресет",
    "Name": "Назва",
    "Video codec": "Відеокодек",
    "Video bitrate": "Бітрейт відео",
    "Resolution": "Роздільність",
    "Audio codec": "Аудіокодек",
    "Audio bitrate": "Бітрейт звуку",
    "Only save": "Лише зберегти",
    "Save and compress": "Зберегти й стиснути",
    "Your presets": "Ваші пресети",
    "Delete": "Видалити",
    "{} kbps": "{} кбіт/с",
    "by quality": "за якістю",
    "About {} for this file": "Для цього файлу приблизно {}",
    "The size depends on the picture": "Розмір залежить від картинки",
    "Compress video to": "Стискати відео в",
    "H.265 · half the size": "H.265 · удвічі менше",
    "H.264 · plays everywhere": "H.264 · грає всюди",
    "About half the size of H.264 at the same quality. Plays on phones, TVs and Windows 10/11.": "Файл приблизно вдвічі менший, ніж у H.264, за тієї ж якості. Грає на телефонах, телевізорах і Windows 10/11.",
    "Plays everywhere, even on old devices and in every messenger. Files come out bigger.": "Грає всюди, навіть на старих пристроях і в будь-якому месенджері. Файли більші.",
    "The smallest files, but slow to encode without a new graphics card. Old devices won't play it.": "Найменші файли, але без нової відеокарти кодує довго. Старі пристрої не потягнуть.",
    "Open format for the web (WebM): browsers, YouTube. The sound becomes Opus.": "Відкритий формат для вебу (WebM): браузери, YouTube. Звук буде в Opus.",
    "The usual sound for MP4. Plays everywhere.": "Звичайний звук для MP4. Грає всюди.",
    "Best quality per kilobyte: voice and music stay clean at a low bitrate.": "Найкраща якість на кілобайт: голос і музика звучать чисто навіть за низького бітрейту.",
    "Old but universal.": "Старий, але зрозумілий усім.",
    "The sound stays as it is, no quality loss and no time spent.": "Звук лишається як є: без втрат і без зайвого часу.",
    "This file looks damaged, or it isn't a video or audio format DragShift can read.":
        "Файл пошкоджений або це не той формат відео/аудіо, який DragShift уміє читати.",
    "DragShift can't tell how long this file is.": "DragShift не може зрозуміти тривалість цього файлу.",
    "This file has no video, only sound.": "У цьому файлі немає відео, лише звук.",
    # ---- settings / welcome
    "Language": "Мова", "Same as Windows": "Як у Windows", "Theme": "Тема", "Dark": "Темна", "Light": "Світла",
    "Open the menu with": "Відкривати меню клавішею",
    "…pressed while you’re dragging a file": "…натиснутою, поки тягнете файл",
    "Save results": "Куди зберігати", "Next to the original": "Поруч з оригіналом",
    "In a “DragShift” subfolder": "У підпапку «DragShift»", "JPG / WebP quality": "Якість JPG / WebP",
    "PDF → image resolution": "PDF → картинка, роздільність", "150 dpi · small": "150 dpi · легкі",
    "200 dpi · balanced": "200 dpi · баланс", "300 dpi · print": "300 dpi · для друку",
    "Video": "Відео", "Use the graphics card when it helps": "Використовувати відеокарту, коли це швидше",
    "NVIDIA NVENC, Intel Quick Sync or AMD AMF: several times faster video compression. DragShift checks that "
    "it works and falls back to the CPU.":
        "NVIDIA NVENC, Intel Quick Sync або AMD AMF: відео стискається в рази швидше. DragShift сам перевірить, "
        "що все працює, інакше візьме процесор.",
    "DragShift {}  ·  originals are never modified": "DragShift {}  ·  оригінали ніколи не змінюються",
    "Quit DragShift": "Вийти з DragShift",
    "The new language applies to menus and windows opened from now on.":
        "Нова мова застосується до меню й вікон, які відкриються далі.",
    "Welcome to DragShift": "Ласкаво просимо до DragShift", "Got it": "Зрозуміло",
    "Start dragging any file": "Почніть тягнути будь-який файл", "Press {}": "Натисніть {}",
    "the menu opens right under your cursor": "меню відкриється просто під курсором",
    "Drop it on a slice": "Киньте на потрібний сектор",
    "the result is saved next to the original": "результат збережеться поруч з оригіналом",
    "Tap {} again": "Натисніть {} ще раз",
    "to switch between Convert and Tools": "щоб перемикатися між Конвертером та Інструментами",
    "Start DragShift with Windows": "Запускати DragShift разом з Windows", "Start with Windows": "Запуск з Windows",
    "Open DragShift at login": "Відкривати DragShift під час входу", "Open at login": "Відкривати під час входу",
    "in Finder or on the desktop": "у Finder або на робочому столі",
    "in your file manager (Thunar, Nautilus…)": "у файловому менеджері (Thunar, Nautilus…)",
    "in File Explorer or on the desktop": "у Провіднику або на робочому столі",
    # ---- results
    "same size": "та сама вага", "{}% smaller": "на {}% легше", "{}% larger": "на {}% важче",
    "{} files → 1 {}": "{} файлів → 1 {}", "{} files → {}": "{} файлів → {}", "Compressed {} files": "Стиснуто файлів: {}",
    "already well compressed": "уже добре стиснутий", "Background added": "Тло додано", "Annotated": "Позначки додано",
    "Redacted": "Приховано", "Metadata removed": "Метадані видалено", "Split into {} PDFs": "Розділено на {} PDF",
    "Merged {} PDFs": "Об'єднано PDF: {}", "Rotated 90°": "Повернуто на 90°", "Password protected": "Пароль поставлено",
    "Extracted 1 image": "Дістано 1 картинку", "Extracted {} images": "Дістано картинок: {}",
    "Nothing was written": "Нічого не записалося",
    "{} pages → {} {}s": "Сторінок: {} → {} {}", "{}: {} images": "{}: {} картинок",
    "burned into pixels": "впечено в пікселі", "private copy": "приватна копія", "encrypted": "зашифровано",
    "Cropped": "Обрізано", "Resized": "Розмір змінено", "Edited": "Відредаговано",
    # ---- file name suffixes
    "Combined": "Разом", "Merged": "Об'єднано", "Protected": "З паролем", "Rotated": "Повернуто",
    "Clean": "Чистий", "No Location": "Без геолокації", "With Background": "З тлом", "page {}": "стор. {}",
    # ---- errors
    "This PDF is password-protected. Remove the password first.": "Цей PDF під паролем. Спершу зніміть пароль.",
    "No images found in this PDF.": "У цьому PDF немає картинок.",
    "No permission to write next to this file. Try a folder you own, or set Settings → Save results → subfolder.":
        "Немає прав писати поруч із цим файлом. Візьміть свою папку або ввімкніть Налаштування → Куди зберігати → підпапка.",
    "This file doesn't look like an image DragShift can read.":
        "Це не схоже на картинку, яку DragShift уміє читати.",
    "The file was moved or deleted.": "Файл перенесли або видалили.",
    "Not enough memory for a file this big.": "Не вистачає пам'яті для такого великого файлу.",
    "Old {} files need Microsoft Office or LibreOffice to convert.":
        "Старі файли {} конвертуються лише через Microsoft Office або LibreOffice.",
    "This file is damaged or isn't really {}.": "Файл пошкоджений або це не {}.",
    "This spreadsheet is empty.": "У таблиці порожньо.",
    "This presentation has no slides.": "У презентації немає слайдів.",
    "There's no picture on the clipboard. Take a screenshot with Win+Shift+S first.":
        "У буфері немає картинки. Спочатку зробіть скриншот через Win+Shift+S.",
    "Screenshot": "Скриншот",
    "Screenshot saved": "Скриншот збережено",
}

TABLES = {"ru": RU, "uk": UK}
LANGS = ("en", "ru", "uk")
# ---- end table

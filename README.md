# DragShift

**Начните тащить любой файл, нажмите Shift и бросьте его на сектор стеклянного меню.** Конвертация и правка картинок, PDF, документов, **аудио и видео** прямо из Проводника, без открытия программ.

[English below](#english)

![DragShift](docs/preview.png)

## Скачать

В [последнем релизе](../../releases/latest):

- **DragShift-2.0.0-Setup.exe** (рекомендую). Ставится только для вашего пользователя, админ не нужен. Добавляет пункт в Пуск, автозапуск (по желанию), пункт **DragShift…** в контекстное меню и **Отправить → DragShift**. Удаляется начисто через Параметры → Приложения.
- **DragShift-2.0.0-Portable.zip**. Распаковал и запустил `DragShift.exe`, ничего не ставится.

Windows 10 и 11, 64 бита. ffmpeg уже внутри, ничего доустанавливать не надо.

> SmartScreen может ругнуться, потому что exe не подписан платным сертификатом. Жмите **Подробнее → Выполнить в любом случае**. Хеши файлов лежат в `SHA256SUMS.txt` рядом с релизом.

## Протестить за 2 минуты

1. Поставьте DragShift-Setup.exe и оставьте галку «Запустить». Справа внизу появится карточка «DragShift готов», а в трее иконка.
2. Возьмите любой **mp3/wav/flac**, начните тащить мышкой и, не отпуская, нажмите **Shift**. Под курсором откроется меню.
3. Бросьте файл на **MP3**, **OPUS** или **FLAC**. Готовый файл появится рядом с оригиналом, оригинал не трогается.
4. Ещё раз Shift во время перетаскивания переключает кольца: **Конвертация → Инструменты → Сжатие**.
5. Видео (**mp4/mkv/mov…**): кольцо **Сжатие** ужмёт ролик до 10, 25 или 50 МБ для Discord и мессенджеров. На видеокартах NVIDIA, Intel и AMD это в разы быстрее.
6. **Инструменты → Обрезать** открывает окно с волной и раскадровкой: тащите края, слушайте пробелом, ставьте затухания.
7. Без перетаскивания: правый клик по файлу → **DragShift…**, **Отправить → DragShift**, или иконка в трее → **Открыть файл…**.

## Что умеет

| Файлы | Конвертация | Инструменты |
|---|---|---|
| **Аудио** mp3, m4a, aac, wav, flac, ogg, opus, aiff, wma, alac, amr, ape… | MP3, M4A, WAV, FLAC, OGG, OPUS, AIFF, WMA, ALAC | Обрезка с волной, нарастание и затухание, прослушка. Битрейт 320…64k. Скорость с сохранением тона |
| **Видео** mp4, mkv, mov, webm, avi, wmv, flv, ts, 3gp… | MP4, MKV, WEBM, MOV, AVI, GIF, MP3, M4A, WAV | Сжать до 10/25/50 МБ или в 2 и 4 раза (GPU: NVENC, Quick Sync, AMF), обрезка, кадрирование, поворот без перекодирования, скорость, убрать звук, вытащить звук, стоп-кадр в PNG |
| **Картинки** png, jpg, webp, heic, avif, tiff, bmp, gif, ico | PNG, JPG, WEBP, HEIC, TIFF, AVIF, BMP, PDF, ICO, GIF | Сжатие, размер, кадрирование, правка света и цвета, пометки, скрытие личного, фон, метаданные и GPS |
| **PDF** | JPG, PNG, DOCX, TXT | Сжатие, разделение, поворот, объединение, пароль, достать картинки, метаданные |
| **DOCX, TXT, MD, CSV…** | PDF, TXT, DOCX | |

Когда это возможно, DragShift вообще не перекодирует: смена контейнера, поворот MP4/MOV и быстрая обрезка копируют потоки как есть и делаются за доли секунды. Обложки альбомов и теги сохраняются. Битрейт выше исходного DragShift делать откажется, потому что файл только раздуется. А если ролик и так меньше 10 МБ, он просто скажет об этом и ничего не создаст.

Язык интерфейса берётся из Windows (русский или английский), поменять можно в Настройках.

## Почему он не тормозит

DragShift 2 переписан: то, что работает всегда (хуки клавиатуры и мыши, меню, уведомления, ffmpeg), это маленькая нативная программа на Rust.

- **В простое около 1 МБ рабочей памяти** (2…4 МБ private) и **0% процессора**: никаких таймеров и опросов, только системные хуки.
- Окна редактора (PySide6) запускаются, только когда вы их открываете, и **сами выгружаются через минуту** простоя.
- ffmpeg работает с пониженным приоритетом, так что даже сжатие 4K не мешает играм и работе.
- Пример: ролик 1080p60 на 40 секунд, 172 МБ → 9.2 МБ за 3.8 секунды на GTX 1060.

## Безопасность

- Оригиналы никогда не меняются, результат всегда пишется новым файлом, во время работы во временный `.part`.
- В комплекте ffmpeg 8.0.1 «essentials» от [gyan.dev](https://www.gyan.dev/ffmpeg/builds/). Сборка скачивает строго этот архив и проверяет SHA-256 архива и `ffmpeg.exe` до использования ([VirusTotal: 0/69](https://www.virustotal.com/gui/file/5af82a0d4fe2b9eae211b967332ea97edfc51c6b328ca35b827e73eac560dc0d)).
- Никакой сети, телеметрии и рекламы. Весь код открыт.

## Сборка из исходников

Нужны Rust (stable), Python 3.10+ и Inno Setup 6.

```bash
python -m venv .venv
.venv\Scripts\pip install -r requirements.txt pyinstaller
powershell -ExecutionPolicy Bypass -File packaging\build.ps1 -Python .venv\Scripts\python.exe
```

Результат в `dist\`. Для разработки хватит `cargo build --release` в `native\`: собранный `DragShift.exe` сам найдёт `.venv` и запустит окна из исходников. Проверки: `DragShift.exe --selftest` (33 сценария аудио и видео) и `pytest`.

Устройство: `native/` демон на Rust (меню, хуки, трей, уведомления, ffmpeg, задачи), `dragshift/` окна редактора и работа с картинками и PDF. Они общаются JSON-строками через stdin/stdout.

## Благодарности

Оригинальный DragShift сделал [llamaboi55](https://github.com/llamaboi55/DragShift): стеклянное меню, редакторы картинок и PDF. Этот форк добавил аудио и видео, нативный демон, русский язык и новую сборку. Версии для Mac и Linux живут в оригинальном репозитории, код Python-приложения 1.x здесь сохранён (`dragshift/app.py`), но в этом форке они не собираются и не проверяются.

Лицензия MIT. ffmpeg распространяется под GPL v3 отдельной программой, его лицензия лежит в `ffmpeg\`.

---

<a name="english"></a>
## English

**Start dragging any file, press Shift, drop it on a slice of the glass menu.** Convert and edit images, PDFs, documents, **audio and video** right from File Explorer.

**Download** from the [latest release](../../releases/latest): `DragShift-2.0.0-Setup.exe` (per-user, no admin, adds a right-click **DragShift…** entry and Send To) or `DragShift-2.0.0-Portable.zip`. Windows 10/11 x64, ffmpeg included.

**Try it:** install, drag an mp3 or a video, press Shift while dragging, drop it on MP3, OPUS or **25 MB**. Press Shift again to switch rings (Convert, Tools, Compress). **Tools → Trim** opens a waveform editor with fades and preview playback.

**What's new in 2.0**
- Audio: MP3, M4A, WAV, FLAC, OGG, OPUS, AIFF, WMA, ALAC; bitrate presets; trim with waveform, fades and listening; speed with pitch kept.
- Video: MP4, MKV, WEBM, MOV, AVI, GIF; compress to 10/25/50 MB or ½ / ¼ with GPU encoders (NVENC, Quick Sync, AMF) and a CPU fallback; trim (exact or keyframe-fast), crop, lossless rotate, speed, mute, extract soundtrack, frame snapshot.
- A native Rust daemon: about 1 MB working set and 0% CPU while idle. The PySide6 editor windows start only when needed and exit after a minute of idle.
- Russian and English UI, following Windows or set in Settings.
- Streams are copied instead of re-encoded whenever possible; cover art and tags are kept; originals are never modified. DragShift won't raise a bitrate or "compress" a file that is already under the target size, it just tells you so.

**Build:** Rust stable, Python 3.10+, Inno Setup 6, then `packaging\build.ps1` (see above). `DragShift.exe --selftest` runs 33 audio/video scenarios.

Based on [llamaboi55/DragShift](https://github.com/llamaboi55/DragShift). MIT license; the bundled ffmpeg (GPL v3) is a separate program with its license in `ffmpeg\`.

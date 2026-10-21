## DragShift 2.0.0

Тащите файл, жмите **Shift**, бросайте на сектор меню. Теперь ещё и **аудио и видео**.

**Скачать:** `DragShift-2.0.0-Setup.exe` (рекомендую, админ не нужен) или `DragShift-2.0.0-Portable.zip`. Windows 10/11 x64, ffmpeg уже внутри.

### Новое
- **Аудио:** MP3, M4A, WAV, FLAC, OGG, OPUS, AIFF, WMA, ALAC. Битрейт 320…64k, обрезка с волной и затуханиями, скорость с сохранением тона.
- **Видео:** MP4, MKV, WEBM, MOV, AVI, GIF. Сжатие до 10/25/50 МБ на видеокарте (NVENC, Quick Sync, AMF), обрезка, кадрирование, поворот без перекодирования, скорость, убрать или вытащить звук, стоп-кадр.
- **Нативное ядро на Rust:** в простое около 1 МБ памяти и 0% процессора. Окна редактора открываются по требованию и сами закрываются через минуту.
- Русский и английский интерфейс, правый клик → **DragShift…**, **Отправить → DragShift**, установщик и портативная версия.

### Проверка
Хеши всех файлов лежат в `SHA256SUMS.txt`. ffmpeg 8.0.1 essentials от gyan.dev, хеш сверяется при сборке, VirusTotal 0/69.

---

## English
Drag a file, press **Shift**, drop it on a slice. Now with **audio and video**: 9 audio formats, bitrate, trim with waveform and fades, speed; video convert, GPU compress to 10/25/50 MB, trim, crop, lossless rotate, speed, mute, extract audio, frame grab. New Rust core idles at about 1 MB RAM and 0% CPU. Get the Setup.exe (no admin) or the portable zip; check files against `SHA256SUMS.txt`.

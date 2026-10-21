## DragShift 2.2.0

Тащите файл, жмите **Shift**, бросайте на сектор меню.

**Скачать:** `DragShift-2.2.0-Setup.exe` (рекомендую, админ не нужен) или `DragShift-2.2.0-Portable.zip`. Windows 10/11 x64, ffmpeg уже внутри.

### Новое
- **H.265 при сжатии видео.** При том же размере картинка заметно лучше, чем в H.264. Для старых устройств в Настройках → «Сжимать видео в» можно вернуть H.264.
- **Свой пресет сжатия.** В кольце «Сжатие» кнопка **Свой пресет**: видеокодек (H.265, H.264, AV1, VP9, к каждому короткое пояснение), битрейт видео, разрешение, аудиокодек (AAC, Opus, MP3 или оригинал) и битрейт звука. Окно сразу считает примерный размер, сохранённые пресеты появляются прямо в меню.
- **Субтитры в mkv.** Сжатый mkv остаётся mkv со всеми дорожками субтитров и шрифтами. В MP4 и WebM переезжают текстовые субтитры.
- **Драг + Shift больше не отваливается.** Windows иногда молча снимает хук мыши (например, если при загрузке система подвисла), и работал только правый клик. Теперь DragShift это замечает и ставит хук заново.

### Проверка
Хеши всех файлов лежат в `SHA256SUMS.txt`, результаты VirusTotal в README. ffmpeg 8.0.1 essentials от gyan.dev, хеш сверяется при сборке.

---

## English
New in 2.2: video compression in H.265 by default (H.264 in Settings), a **New preset** window in the Compress ring (codec, bitrate, resolution, audio), MKV subtitles and fonts kept, and the drag trigger recovers when Windows drops the mouse hook. Check files against `SHA256SUMS.txt`.

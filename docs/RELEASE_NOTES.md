## DragShift 2.1.0

Тащите файл, жмите **Shift**, бросайте на сектор меню.

**Скачать:** `DragShift-2.1.0-Setup.exe` (рекомендую, админ не нужен) или `DragShift-2.1.0-Portable.zip`. Windows 10/11 x64, ffmpeg уже внутри.

### Новое
- **Excel и PowerPoint в PDF:** xlsx, xlsm, xls, pptx, ppt. Если стоит Microsoft Office или LibreOffice, конвертирует через них. Без них DragShift сам рисует таблицы и слайды, со шрифтами презентации.
- **Скриншот из буфера в файл:** Win+Shift+S, потом иконка в трее → **Сохранить скриншот в «Изображения»** или **в папку…** (папка запоминается). Любой формат: PNG, JPG, WEBP, HEIC, TIFF, AVIF, BMP, PDF, ICO, GIF.
- PDF из документов теперь с настоящими шрифтами: курсив на кириллице ровный, скопированный текст с нормальными пробелами. DOCX конвертируется через Word, если он стоит.
- Украинский перевод: «папка» вместо «тека».

### Проверка
Хеши всех файлов лежат в `SHA256SUMS.txt`, результаты VirusTotal в README. ffmpeg 8.0.1 essentials от gyan.dev, хеш сверяется при сборке.

---

## English
New in 2.1: Excel and PowerPoint to PDF (through Office or LibreOffice when installed, otherwise drawn by DragShift itself), and save the clipboard screenshot from the tray in any image format, to Pictures\Screenshots or a folder you pick. Document PDFs now use real installed fonts. Check files against `SHA256SUMS.txt`.

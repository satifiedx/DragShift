# DragShift macOS end-to-end — 12/17 passed

| # | Scenario | Result | Detail | Time |
|---|---|---|---|---|
| 1 | JPG → PNG | ✅ | D-Photo.png · 2400×1600 | 11.3s |
| 2 | JPG → WEBP | ✅ | D-Photo.webp · 2400×1600 | 9.8s |
| 3 | JPG → HEIC | ✅ | D-Photo.heic · 2400×1600 | 10.3s |
| 4 | JPG → PDF | ❌ | AssertionError: no output file appeared | 23.5s |
| 5 | PNG → JPG | ✅ | D-Graphic.jpg · 800×533 | 8.7s |
| 6 | PDF → JPG (3 pages) | ❌ | AssertionError: no output file appeared | 23.4s |
| 7 | PDF → TXT | ✅ | D-Report.txt | 8.9s |
| 8 | DOCX → PDF | ❌ | AssertionError: no output file appeared | 23.2s |
| 9 | PDF Split | ✅ | 3 PDFs | 10.6s |
| 10 | PDF Rotate | ✅ | all pages at 90° | 10.0s |
| 11 | PDF Compress | ✅ | 227 KB → 108 KB | 10.1s |
| 12 | Merge 2 PDFs | ✅ | 5 pages | 13.0s |
| 13 | Compress (window, Enter) | ❌ | AssertionError: no output file appeared | 26.5s |
| 14 | Resize 50% (window, Enter) | ✅ | 2400×1600 → 1200×800 | 12.2s |
| 15 | Add BG (window, Enter) | ❌ | AssertionError: no output file appeared | 27.1s |
| 16 | Metadata: remove all (Enter) | ✅ | 0 metadata fields left | 12.4s |
| 17 | Slices filled before the mouse moves | ✅ | files read from the drag pasteboard | 0s |
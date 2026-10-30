# DragShift macOS end-to-end — 12/17 passed

| # | Scenario | Result | Detail | Time |
|---|---|---|---|---|
| 1 | JPG → PNG | ✅ | D-Photo.png · 2400×1600 | 12.0s |
| 2 | JPG → WEBP | ✅ | D-Photo.webp · 2400×1600 | 9.9s |
| 3 | JPG → HEIC | ✅ | D-Photo.heic · 2400×1600 | 10.5s |
| 4 | JPG → PDF | ❌ | AssertionError: no output file appeared | 23.9s |
| 5 | PNG → JPG | ✅ | D-Graphic.jpg · 800×533 | 9.5s |
| 6 | PDF → JPG (3 pages) | ❌ | AssertionError: no output file appeared | 23.5s |
| 7 | PDF → TXT | ✅ | D-Report.txt | 9.3s |
| 8 | DOCX → PDF | ❌ | AssertionError: no output file appeared | 23.5s |
| 9 | PDF Split | ✅ | 3 PDFs | 10.0s |
| 10 | PDF Rotate | ✅ | all pages at 90° | 10.6s |
| 11 | PDF Compress | ✅ | 227 KB → 108 KB | 11.1s |
| 12 | Merge 2 PDFs | ✅ | 5 pages | 13.1s |
| 13 | Compress (window, Enter) | ❌ | AssertionError: no output file appeared | 26.3s |
| 14 | Resize 50% (window, Enter) | ✅ | 2400×1600 → 1200×800 | 12.5s |
| 15 | Add BG (window, Enter) | ❌ | AssertionError: no output file appeared | 27.1s |
| 16 | Metadata: remove all (Enter) | ✅ | 0 metadata fields left | 12.8s |
| 17 | Slices filled before the mouse moves | ✅ | files read from the drag pasteboard | 0s |
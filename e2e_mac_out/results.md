# DragShift macOS end-to-end — 12/17 passed

| # | Scenario | Result | Detail | Time |
|---|---|---|---|---|
| 1 | JPG → PNG | ✅ | D-Photo.png · 2400×1600 | 11.2s |
| 2 | JPG → WEBP | ✅ | D-Photo.webp · 2400×1600 | 8.6s |
| 3 | JPG → HEIC | ✅ | D-Photo.heic · 2400×1600 | 10.6s |
| 4 | JPG → PDF | ❌ | AssertionError: no output file appeared | 23.3s |
| 5 | PNG → JPG | ✅ | D-Graphic.jpg · 800×533 | 8.9s |
| 6 | PDF → JPG (3 pages) | ❌ | AssertionError: no output file appeared | 23.3s |
| 7 | PDF → TXT | ✅ | D-Report.txt | 9.2s |
| 8 | DOCX → PDF | ❌ | AssertionError: no output file appeared | 23.7s |
| 9 | PDF Split | ✅ | 3 PDFs | 9.8s |
| 10 | PDF Rotate | ✅ | all pages at 90° | 9.7s |
| 11 | PDF Compress | ✅ | 227 KB → 108 KB | 10.4s |
| 12 | Merge 2 PDFs | ✅ | 5 pages | 13.4s |
| 13 | Compress (window, Enter) | ❌ | AssertionError: no output file appeared | 26.4s |
| 14 | Resize 50% (window, Enter) | ✅ | 2400×1600 → 1200×800 | 12.4s |
| 15 | Add BG (window, Enter) | ❌ | AssertionError: no output file appeared | 27.2s |
| 16 | Metadata: remove all (Enter) | ✅ | 0 metadata fields left | 12.6s |
| 17 | Slices filled before the mouse moves | ✅ | files read from the drag pasteboard | 0s |
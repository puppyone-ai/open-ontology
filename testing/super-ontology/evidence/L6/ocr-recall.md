# OCR recall on the synthetic scanned subset (L6 acceptance 1, B-S9-01, B-S9-04)

Run: `tools/ocr-parser/scanned_subset.py render` then `measure`, inside the `oo-docs-ocr-parser:dev` container
(PP-StructureV3, models from `tools/ocr-parser/models.lock.json`, `--network none`, one thread). Raw numbers:
`ocr-recall.json`.

| Measure | Value | Target |
|---|---|---|
| Subset | 44 files, 56 page images: every pdf page of the golden Proposals (pdfium bitmaps), and the md, txt, docx and pptx text of the golden set drawn on A4 pages with a CJK font, then degraded like a scan (paper grey, noise, skew up to 0.4 degree, blur) | |
| Core-type recall (name or alias found in the OCR text of an anchor file; 90 entities with an anchor file in the subset) | **0.9667** (87 of 90) | >= 0.65 |
| By D15 type | Decision 10/10, Document 41/44, Event 5/5, Organization 8/8, Person 9/9, Project 4/4, Term 5/5, Topic 5/5 | |
| Missed | three Document entities whose name is a file label (`速记0412`, `速记0601`, `速记0828`) and not text in the page | |
| Engine path, the 14 digital-pdf entities through the adapter's pdf path with the text-layer fill (adapter version 2, image rebuilt 2026-11-18) | **1.0** (14 of 14) | |
| Embedded chart and screenshot text, character recall (ocr-parser, standalone image) | chart 0.9412, screenshot 1.0, mean **0.9706** | >= 0.9 |
| Embedded chart and screenshot text, character recall (doc-parser RapidOCR path, PP-OCRv5 mobile ONNX, host run) | chart 1.0, screenshot 1.0 | >= 0.9 |

What the number is: the OCR-only share of the extraction recall. The LLM extraction step is not in this loop (no model
key in CI); a name that OCR returns is a name the extractor can see. Character recall is the multiset overlap of
ground-truth characters with the OCR output (reading order of chart labels does not matter).

Resources of the ocr-parser image on linux/arm64 (Docker Desktop VM): one A4 page at 200 dpi takes about 20 s with one
thread; peak memory of one worker is about 6.8 GB with text detection bounded at a 1280 px long side (4.2 GB at 960,
more than 12 GB at the full 2339 px side). The image therefore asks for `mem_limit` 10g per worker. The aarch64 Paddle
wheel crashes (segfault in `predictor.run`) with more than one OpenBLAS thread, so the sidecar runs one thread per
worker and scales by workers.
Not run in this version: SLANeXt_wired table structure (tables are returned as OCR text with
`DOCS_PARSE_TABLE_STRUCTURE_OFF`).

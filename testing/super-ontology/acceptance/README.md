# Acceptance folder (lane AB)

A fictional smart-glasses launch (Northport Optics, Aurora NP-7), in Chinese and English, 45 files: text and scanned PDFs, xlsx (several sheets), csv,
docx, pptx, wav and m4a meetings (macOS `say`, two voices each), a whiteboard and a screenshot, md/txt notes, and 7 unrelated files.
All synthetic. `python3 -B tools/super-ontology/acceptance/gen.py` rewrites it (`--check` compares the text side; audio and scan bytes come from the OS).

* `project/`: the folder to drop into My files.
* `gold/relations.jsonl`: expected cross-file relations (surface names and the files that must carry the same thing).
* `gold/unlinked.jsonl`: unrelated files and the reason the app should give (`unrelated_content`, `empty_file`, `corrupt_file`, `unsupported_format`).
* `gold/questions.jsonl`: 34 questions with answers and source files (single, cross, numeric, audio, cross-language, 4 unanswerable).
* `results/`: scores and per-file timings from runs. The gold is frozen at the commit that adds it and is never edited after a run.

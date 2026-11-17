# Golden set generator

All content is synthetic: a fictional company with nine invented employees, customers and projects; no customer data.
`python3 -B tools/super-ontology/golden/gen.py` rewrites this tree; `--check` regenerates into a temp dir and compares bytes.

* Content lives in `tools/super-ontology/golden/world.py`; writers in `writers.py`; stdlib only.
* Determinism: no clocks; canaries come from `random.Random(20261117)`; zip containers are `ZIP_STORED` with timestamp
  2026-01-01 so bytes do not depend on the zlib version; PDF has no Info/ID and uses a hand-written object layout.
* Fonts/licences: no font is embedded anywhere. The PDFs reference the standard non-embedded CJK font `STSong-Light`
  (Adobe-GB1, `UniGB-UCS2-H` encoding) plus an identity `ToUnicode` map; OOXML files only name `SimSun`/`Calibri`.
* Binary files (docx/xlsx/pptx/pdf) are committed; `MANIFEST.sha256` records the sha256 of every file.
* Anchors in labels are computed by the writers while rendering, so they always match the files.
* `redteam/` holds files that must never reach a scope that does not include them (canary strings, red-team-only names,
  one same-name-different-person decoy). `canaries.txt` lists the canaries; `scopes.json` the scope combinations.
* `token-calibration.json` records measured sizes per file; `tokens` stays null until a tokenizer calibration is run.

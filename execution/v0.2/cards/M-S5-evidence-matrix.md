## Card M-S5 — lane M: evidence index and the generated support matrix

TODO: S5-M-01 (exit: generated levels consistent with evidence; AT-ENG-03). Works ahead of the S4 exit. Keep your S6
commits (72e6509f2, a41aab6f9, b05c806b5) on the branch; rebase on v0.2/product-loops first. Granted for this card:
`execution/v0.2/evidence-index.json`, `docs/database-support/support-matrix.generated.json` (generated only) and a new
`tools/evidence-index/**`.
1. `tools/evidence-index/collect.py`: build `execution/v0.2/evidence-index.json` rows
   {target, version, suite, variant, result, run_id, sha256, emulated} (schema in specs/scripts/gen_support_matrix.py)
   from the committed evidence summaries across the tree (git ls-files '*evidence*.json', '*SUMMARY*.json',
   '*RESULTS*.json', '*VALIDATION*.json', tools/oo-test PINS/S5 validation docs, testing/engines/**). One small adapter
   per summary format; a summary that does not state a suite/version/result explicitly yields no row (never infer a
   pass). sha256 = the summary's own recorded hash of the raw result, else sha256 of the committed summary file. Mark
   amd64-on-arm64 runs `emulated`. Unit tests per adapter.
2. Run `python3 specs/scripts/gen_support_matrix.py` then `--check` and `--self-test`; commit the generated matrix.
3. Report a target × version × capability table of the generated levels and list every P0 version/suite (T0–T8)
   that has no evidence row — that list is the S5 gap list.

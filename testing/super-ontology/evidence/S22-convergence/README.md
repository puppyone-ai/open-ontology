# S22 convergence (lane AE6)

Tree: so/AE6-final on feat/super-ontology 36df993e8, commits SO-F-S23-01..08 (two test updates, install test, image and media-parser pins, a settle-step fix, the acceptance
runner, two lock commits). Model cb/gpt-6.2-sol by file path. Built app: DMG sha256 `34790241dfe2a020444ee5ac479558afe5966607ff0e729ea24fa4f3a4f42aff`, engine image
`oo-docs-engine:dev-ae6b` (sha256:f1bda026...), media-parser image `oo-docs-media-parser:dev-asr` (sha256:26732703..., Whisper base + sherpa-onnx), OCR and document parser images unchanged.
Everything ran headless: the app launched hidden (`open -g -j`), isolated HOME, no GUI driving.

## Gates

| gate | result | number |
|---|---|---|
| cargo fmt, clippy -D warnings (workspace, all targets) | PASS | clean |
| cargo test --workspace (non-ignored) | PASS after two test fixes | 313 target results ok; two failures were stale tests (FM5 replay not updated for D97, docs-report key list without type_precision), fixed with SO-F-S23-01/02 |
| oo-docs live suites (all, against Postgres) | PASS | 64 targets; install 13/13 incl. 0154 to 0160 upgrades (a stale table count and 4-table drop fixed, SO-F-S23-03); jobs_live, share_live pass on a Postgres with 500 connections and track_commit_timestamp (the first pass failed on connection slots and that setting, not on code); speaker_live 6, media_acceptance_live 2, pipeline, review, state, freshness, extract_llm_stage, llm_live pass |
| profiles_live / structure_live / media_class_model_live | PASS through their runners | see below (they need the runner's environment, so 0 s failures in the bare loop are not failures) |
| xtask architecture, contracts, docs; super_ontology_schema | PASS | pass, pass, pass; 4/4 |
| app: typecheck, lint, check:i18n, check:brand, gen:contract:check | PASS | 0 errors |
| app: visual | PASS | 59 passed |
| app: pnpm test 3x green | PARTIAL | 3 runs of 1080 + 3 runs of the 23 real-file-system watcher tests: all green. As ONE run the watcher's idle-window test (10,000 files) fails 3 of 4 times on this host at load 6-8 (late FSEvents backlog delivered inside its idle window); alone it passes every time. Not changed |
| check_trailers | PASS | 0 violations |
| check_ledger | 6 | 1 known (SO-B-L6-02) plus the five new Plan ids SO-F-S23-0N waiting for their ledger rows (the coordinator adds them) |
| L4 golden, published view | PASS | entity P .967 R .890; type P .989; fact P .903 R .786 |
| L4 holdout, published view | PASS | entity P .965 R .863; type P .988; fact P .901 R .759 |
| single-word Decisions | PASS | 0 (engineering corpus, 8 Decisions, 0 false, 7 of 8 true found) |
| review capacity (D97 method of lane AJ) | PASS | 34,104-file synthetic folder: 2 pending = the 2 planted ambiguities, 0 unplanted; rule of three 95% bound 3/34,104 = 8.8 per 100k files (single digits); index 1627 s, publish 636 s (debug build) |
| admission, profiles corpus (D94) | PASS | class accuracy 41/41; entities from instruction, code and other files 0; reference gives only Reference and Person; record entities 6/6; hard gates pass |
| structure, held-out corpus (D93) | PASS | classes 5/5, completeness 1.0, property recall 1.0, type accuracy .917 (gate .90), relation recall and precision 1.0, coverage in class .909 |
| 45-file acceptance, packaged app (audio and OCR included) | PASS | all 42 supported files published with no accept-source and no review item (first at 16 s, median 128 s, last 276 s; the other 3 are the corrupt pdf and the .key and .sketch files); relations strict 21/25 (lenient 21), precision .992, 0 leaks; people 9/9, vendors 4/4, product 1/1, decisions 3/3, figures 2/4, dates 2/4 |
| acceptance: states | | classes record 33, media 6, other 5, instruction 1; recipe and itinerary are `other` (not admitted: "archive file, not in the ontology"); 4 recordings are conversations by rules (speakers to name 2, 2, 3, 2; 9 placeholders, none blocking); unlinked scorer 4/7 with its rules unchanged because the scorer's vocabulary predates not-admitted and speaker states |
| Ask through the app's ask IPC (off-screen) | PASS | 29/30 correct, 27 cited, honest on unanswerable 4/4, audio 3/3, cross 10/10, numeric 5/6 |
| T1 on the built app | PASS | see t1/summary.txt |
| oo serve 10 start/stop cycles | PASS | 10/10 |
| upgrade from the delivered build's state | PASS | 0157, 0159, 0160 applied, drives and files intact |

## Found and fixed during the run
* The settle step (lane AE2) counted pending speaker_identity items, which since D97 are not reviews: a recording whose only content was speaker items stayed indexed. Fixed with a test that is red without the change (SO-F-S23-06); the acceptance run before the fix left that one file indexed.
* run_packaged.mjs knew the old state folder and the old upload-start answer (SO-F-S23-07); scoring untouched.

## Not done or open
* The media-parser image is one layer on the existing image; the full Dockerfile build needs the campplus export and recompiles FFmpeg and OpenCV. The L6 SBOM was not regenerated for sherpa-onnx.
* `oo serve --stop` followed at once by `oo serve` can answer "another instance is starting" for a few seconds (seen twice in the upgrade check).
* L4 numbers are one run per set (the model's recall spread is about .03); fact recall on the holdout is .009 above the gate.
* The 45-file corpus relations: figures 2/4 and dates 2/4 (numbers written in words are not normalised).

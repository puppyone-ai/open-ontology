# L6 evidence (30 §L6 acceptance, B-L6-2 close)

Compose run `be2-l6e2e-10` of `tools/oo-test/super_ontology_l6.py --suite e2e` (`oo serve --runtime compose` with the `ocr` and
`media` profiles; engine image `oo-docs-engine:dev-be2`, media image `oo-docs-media-parser:dev-be2`, both in
`deploy/docs/images.lock.json`), plus the L1 cell evidence that already sat in this directory. `l6-e2e-result.json` and
`l6-e2e-log.txt` are the run; 27 of 29 checks pass. L2 stays green (`../L2`, run `be2-l6close`, 40/40).

| # | Acceptance | Result | Numbers |
|---|---|---|---|
| 1 | scanned subset core recall >= 0.65 end to end; embedded chart text >= 0.9 | PASS at the sidecar, PARTIAL end to end | sidecar (44 files, 56 pages): core recall 0.9651 (83/86), chart and screenshot text 0.9706 (`ocr-recall.md`). Through the compose pipeline: a scanned pdf and a png publish and carry the 3 planted words (3/3). The 44 file subset was not pushed through the engine: the LLM extraction stage is not in the loop, so a pipeline recall number would only restate the OCR one |
| 2 | ASR and diarization bake-off | PASS (earlier card) | `asr-bakeoff.md` |
| 3 | 2 h meeting; speaker identity; kill and resume; ISA replay; no speaker features left | PARTIAL | speaker identity in the compose run: 4 `speaker_identity` items (one per label), nothing under a named Person before confirmation, after confirming S1 as 小王 the fact's subject is that Person: PASS. 2 h run (sidecar pipeline, `long-audio.json`): killed after 40 chunks, resume reused 40 and computed 219 (404 s), fresh run 444.9 s, replay byte identical, speaker feature directory gone: PASS. **Not measured**: core recall >= 0.6 on the 2 h meeting through the real pipeline (needs the extractor over the 2 h transcript), peak memory of the 2 h run (the cell recorded time only) |
| 4 | video: one key frame per slide, frame text recall >= 0.9, `#t=` provenance | PASS | 2 slides, 2 `frame_text` blocks, character recall 1.00, blocks carry `t_start_ms`. Needed two sidecar fixes (FFmpeg 9 `-vsync`, scenedetect against an FFmpeg-free OpenCV), commit `Fix two media-parser defects` |
| 5 | Linux/compose: H.264/AAC is `DOCS_PARSE_CODEC_UNAVAILABLE`; `-buildconf` proof | PASS | mp4 (H.264/AAC) and m4a (AAC) are `unsupported DOCS_PARSE_CODEC_UNAVAILABLE`; `-buildconf` and the no egress proof in `l6-docs-media.txt` (`FFMPEG GATE: PASS`). macOS host decode is Desktop phase two |
| 7 | refresh diff in BRIEF head | PASS | after a changed fact the refreshed BRIEF has `## 自上次以来的变化（v5 → v6）` (`a7-BRIEF-after-refresh.md`) |
| 8 | parser version bump | PARTIAL | the container version is written and found a real engine bug: the engine read the sidecar version from a nested `parser` object but `/v1/info` has `parser_id` and `parser_version` at the top, so no upgrade was ever noticed (fixed, commit `Read the sidecar parser version...`). Now: 4 files `outdated` at engine start (`files.outdated_by_version`), old facts stay published during the rerun (min 15 of 15), no churn (same fact keys, 0 retracted), DOC-G10 held 0 items. **FAIL**: one of the 4 files (the meeting wav, its facts are held speaker statements) stays `indexed` after the rerun, it never returns to `published`; the other 3 do (`a8-failure-diagnostics.txt`). Open for the coordinator, see the report |
| 10 | media jobs running while document latency meets D49 | PASS (D49), note | D49 as calibrated (md detected to published <= 7 s): 2.42 s with a media job queued or running at the edit. The edit to a new BRIEF took 40.9 s in that run, over the L2 bound of 30 s (the check fails, it is not an L6 bound): publication windows plus the ASR load of the long recording |
| 6, 9 | SBOM gate; `puppyone share` equality | 6 PASS (`sbom-*.json`); 9 phase two | |

Phase two (not here): F-S10-05 macOS host decode helper, F-S10-07 `#t=` jump in Desktop, F-S10-06 speaker confirm UI, the
Puppyone CLI `puppyone share` equality (acceptance 9).

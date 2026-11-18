# L3 sign-off verification (verify-L3), NOT SIGNED OFF on this HEAD

Branch `so/verify-L3` on `feat/super-ontology` e82490f57, 2026-11-17. Details: `verify-l3-tests.md` (same folder).
Result: **the OO-side sign-off is blocked, not passed.** This host could not reach any registry, so the
locked `docs-engine` image (and therefore the full stack runner, compose check and both S3 cells) could not
be run. The table below separates what was run on this HEAD from what is carried forward from the earlier run
(branch `so/B-S3-05-pipeline-routing`, commit 7834b28c2, section "History" below). Carried-forward numbers
are not verification at this HEAD.

| # | Acceptance (30 section L3) | verify-L3 status | Measured on this HEAD | Evidence |
|---|---|---|---|---|
| 1 | Golden set parse success >= 98 %, readable error codes | NOT VERIFIED here (blocked: engine image) | none this run; earlier run 51/51 (100 %) | `verify-l3-tests.md`; earlier: `l3-run.txt`, `l3-golden-states.json` |
| 2 | Fault injection: sidecar kill, PG disconnect, 5xx, half-written file | **FAIL (open)** | `sidecar_supervision` 6/6 pass; `faults` 8/9 pass; PG-restart case hung (no result after 20+ min, killed) | `verify-l3-tests.md`, `faults-run.txt` (earlier) |
| 3 | Poison file: <= 3 attempts, then quarantine, throughput unharmed | fault-level PASS; full-stack NOT VERIFIED | `faults`: poison pill quarantined after 2 crashes, rest flows (pass) | `verify-l3-tests.md`; earlier: `l3-run.txt` (quarantined at attempt 2, 33.4 s) |
| 4 | Idempotence: 10 copies parsed once | NOT VERIFIED here | none this run; earlier: 1 published + 9 duplicate | earlier: `l3-run.txt` |
| 5 | fs and s3 rounds: add, modify, rename, delete | NOT VERIFIED here; s3 cells blocked | none this run (`download` ignored tests need an S3 endpoint, not run) | earlier: `s3-life-rustfs.md`, `s3-life-minio.md` |
| 6 | 1000 files cold parse <= 30 min, memory limits | NOT VERIFIED here | none this run; earlier 23.8 min, peak 66 MiB / 1697 MiB / 411 MiB | earlier: `bench-cold1000.md`, `bench-cold1000.json` |
| 7 | Desktop frontend status matrix | phase two (D80) | not in this phase | section below |
| 8 | Auto detection, D49 latency (md/txt p95 <= 7 s, pdf/docx <= 30 s, xlsx <= 20 s) | NOT VERIFIED here | none this run; earlier md/txt p95 6.13 s (poll) and 6.11 s (hint), pdf 8.26 s, docx 6.21 s, xlsx 6.24 s | earlier: `l3-latency-poll.json`, `l3-latency-hint.json` |
| 9 | Targeted sync does not sweep; seven freshness states visible | NOT VERIFIED here at full stack; API/conformance level PASS | `freshness_live` 3/3, `conformance_freshness` 1/1 | earlier: `l3-run.txt` |
| 10 | Format-only DOCX change does not re-extract | NOT VERIFIED here | none this run | earlier: `l3-run.txt` |

Note on item 8: D49 was recalibrated on 2026-11-17 to md/txt p95 <= 7 s (spec 00). The earlier 5 s
FAIL in this README's history no longer applies to the spec, but the 6.1 s measurement is from the earlier run.

### Other test results on this HEAD (verify-L3)

- `cargo test -p oo-docs --locked`: exit 0 (unit 338 passed, integration 0 failed).
- `cargo test -p oo-connectors --features objectstore-s3 --locked --lib objectstore`: exit 0 (24 passed).
- `--ignored` suites: all pass except the following:
  - `reprocess`: 3/4 pass. The `a_parser_upgrade_marks_files_outdated_and_reprocesses_them_in_small_batches` assertion
    expects 2 queued reprocess jobs (`reprocess.rs:300`), the code gives 1 because `REPROCESS_INFLIGHT = 1`
    (`crates/oo-docs/src/jobs/stages.rs:60`). Both come from commit f2b604e99 and the spec gives no number
    (10 section 11.2 says only "low priority, small batches"). This is a mismatch between test and code;
    not changed here. Needs a development agent or a coordinator decision.
  - `faults`: `postgres_restarted_under_a_running_engine_loses_nothing` hung after `docker restart` of the
    labelled PG container (no result after 20+ min). Killed. Not a pass.
  - `download` (3 ignored tests) did not run: needs `--features objectstore-s3` and an S3 endpoint.
- `eval_live` and `faults` failed on the first pass only because required env vars were missing from the
  invocation (`OO_EVAL_OUT`, `OO_DOCS_TEST_PG_CONTAINER`). Reruns with the variables passed (`eval_live` passes).

### Gates in force at L3 (30 section 4)

| Gate | Result this run | Evidence |
|---|---|---|
| State-machine contract (front/back enum consistency) | backend side pass: `conformance_state` 1/1, `contract` 3/3, `conformance_jobs` 1/1. Desktop-side matrix comparison is phase two. | `verify-l3-tests.md` |
| Fault injection suite | 8/9 pass, PG-restart hung | `verify-l3-tests.md` |
| SBOM licence gate (D64: fail on forbidden licence or missing audit record) | NOT re-run (needs the doc-parser image). Earlier gate result: pass, five weak-copyleft components reviewed. | `sbom-doc-parser.gate.txt`, `sbom-doc-parser.json` (earlier) |

### Phase two (Desktop, not verified here, D80)

- Item 7 (status matrix screenshots), the upload queue, bulk retry and the parse-result tab.
- Drive registration form for S3 (endpoint, bucket, prefix, `secret_ref`), virtual scrolling for large Drives.
- Desktop reconnect without losing status (item 7).
- Desktop end-to-end run of items 1 to 10 and the Desktop part of the gates.

### Changes made by verify-L3

- `tools/oo-test/super_ontology_compose.py`: `LANE` set to `super-ontology-verify` (was `super-ontology-be2`), so this lane
  labels and cleans up only its own containers. Test-script change only.
- No product code changed. `deploy/docs/images.lock.json` unchanged.

## History (earlier run, branch so/B-S3-05-pipeline-routing at 7834b28c2, 2026-11-17)

# L3 evidence, "read the files" (30 section L3 acceptance 1-10, OO side)

Branch `so/B-S3-05-pipeline-routing` (on `feat/super-ontology` at 7834b28c2), 2026-11-17, local arm64
Docker Desktop. The stack is the real one: `oo serve --runtime compose` (docs-db, docs-engine and the
Docling doc-parser in containers, the daemon on the host). No model, no network for the parser (D17).
Per spec 00 D80 nothing here needs Desktop; UI items are verified through the API.

Result: 8 of the 9 testable items PASS; item 8 passes for pdf, docx and xlsx and **misses its md/txt target**.
Item 7 is a Desktop item (not run in this phase).

| # | Acceptance | Result | Numbers | Evidence |
|---|---|---|---|---|
| 1 | Golden set: per format parse success >= 98 %, every failure has a readable code | **PASS** | md 18/18, txt 3/3, csv 3/3, xlsx 4/4, docx 10/10, pptx 6/6, pdf 7/7 (51/51 = 100 %); the 13 hostile fixtures end in the state and code of 10 section 4.4 (encrypted docx and pdf, legacy .doc, two zip bombs, plain zip, traversal docx, gb18030 txt, ruled-table pdf, merged and hidden xlsx, two csv dialects); every non published file carries a code that is in `errors.v1.json` | `l3-run.txt`, `l3-golden-states.json`, `sidecar-tests-opencv.txt` |
| 2 | Fault injection: sidecar killed mid parse, PG disconnect, object store 5xx, half written file; the rest keeps flowing, nothing is stuck | **PASS** | see `faults.md` (9 engine tests, 6 sidecar supervision tests, s3 5xx with retries) | `faults.md`, `faults-run.txt` |
| 3 | Poison: at most 3 attempts, then quarantine, throughput unharmed | **PASS** | quarantined after 2 crashes (attempt 2) in 33.4 s, 2 worker kills; the 16 other files published 16/16, none needed a second parse job; the sidecar parses afterwards | `l3-run.txt` |
| 4 | Idempotence: 10 copies parsed once | **PASS** | md, pdf, docx, xlsx: 1 published + 9 `duplicate`, 1 parse job and 1 parse result per content, every one of the 10 paths has its own (equal) evidence | `l3-run.txt` |
| 5 | fs and s3 rounds: add, modify, rename, delete | **PASS** (fs) / **PASS** (s3 on RustFS and on MinIO) | fs: chains `stored>...>published`, modify goes `superseded>stored`, old evidence withdrawn (0 live) and new live (15), rename = `duplicate` with evidence on the new path and `file.renamed`, no fact retracted, delete withdraws evidence and retracts facts left without it (1 -> 5). s3: 20/20 checks on each store, including a same size modify and rename by copy+delete | `l3-run.txt`, `s3-life-rustfs.md`, `s3-life-minio.md` |
| 6 | 1000 files cold parse <= 30 min, memory inside the 10 section 12.9 limits | **PASS** | 1000/1000 published in **23.8 min** (1425 s, 42.1 files/min); peak memory engine 66 MiB (limit 1.5 GiB), doc-parser 1697 MiB (3 GiB), docs-db 411 MiB (1 GiB) | `bench-cold1000.md`, `bench-cold1000.json` |
| 7 | Frontend status matrix | not in this phase | Desktop item (D80) | - |
| 8 | Auto detection and latency (D49) | **md/txt FAIL**, pdf, docx, xlsx PASS | see the latency table below | `l3-latency-poll.json`, `l3-latency-hint.json`, `l3-latency-hops-*.txt` |
| 9 | A targeted sync does not sweep; the seven freshness states are visible | **PASS** | targeted sync published a new file, generation 670 -> 670, the file deleted on disk was still listed, the next full sync swept it (670 -> 671); `up_to_date`, `syncing`, `processing`, `publishing`, `waiting_review`, `degraded` (`DOCS_SOURCE_ROOT_MISSING`), `held` (`DOCS_SOURCE_MASS_DELETE_HELD`, 120 would be deleted, confirmed with the generation) all observed in the events | `l3-run.txt` |
| 10 | A format-only DOCX change does not run the extraction again | **PASS** | second version `indexed` with `reused_from` (by block ids), evidence 3 -> 3 live, none left on the old version, no fact retracted; a changed word is extracted again (control) | `l3-run.txt` |

Finder drop (item 8, "no API call"): files written straight into the Drive directory were detected by the poll (1 s in the
first measurement, default 30 s in the second together with the targeted sync hint the Desktop watcher sends, D46) and
reached `published` without any call to the API except the hint.

## Item 8, latency (D49: md/txt p95 <= 5 s, <= 20 page pdf/docx <= 30 s, xlsx <= 20 s)

Drop to `published`, measured from the client, 100 ms polling of the file list.

| group | mode | n | p50 | p95 | target | |
|---|---|---|---|---|---|---|
| md/txt | 1 s poll | 20 | 5.45 s | 6.13 s | 5.0 s | **FAIL** |
| md/txt | hint (30 s poll) | 10 | 4.05 s | 6.11 s | 5.0 s | **FAIL** |
| pdf | 1 s poll | 10 | 8.04 s | 8.26 s | 30 s | pass |
| pdf | hint | 5 | 8.10 s | 8.15 s | 30 s | pass |
| docx | 1 s poll | 10 | 6.03 s | 6.21 s | 30 s | pass |
| docx | hint | 5 | 4.01 s | 4.13 s | 30 s | pass |
| xlsx | 1 s poll | 10 | 6.06 s | 6.24 s | 20 s | pass |
| xlsx | hint | 5 | 4.08 s | 6.21 s | 20 s | pass |

Where the md/txt time goes (median seconds after the file was first seen, `l3-latency-hops-*.txt`):

| step | 1 s poll | hint |
|---|---|---|
| `queued` (hashed, after `settle_seconds` = 2) | 2.29 | 2.44 |
| `parsing` / `parsed` (pump tick 250 ms) | 2.64 | 2.96 |
| `extracting` / `indexed` | 3.14 / 3.15 | 3.46 / 3.47 |
| `published` (publish window of 2 s, 10 section 4.7) | 4.84 | 3.98 |

The floor of an md/txt file is therefore `settle_seconds` (2 s, spec default) + about 1 s of stage hand-offs + the
2 s publish window (0.5 s to 1.7 s observed, a file that is indexed just after the window's job has run waits for the
next window) + up to 1 s of polling when there is no hint. With the spec defaults the 5 s target cannot hold at p95 even
with the hint (p50 is 4.05 s, p95 6.11 s). D49 says "estimate, calibrated after L3". Options for the coordinator:
calibrate md/txt to p95 <= 7 s with the defaults, or change the defaults (publish window 1 s, pump tick 100 ms would take
about 1.5 s off). The engine constants were not changed here because both numbers are in the spec.

## Not covered or weaker than the table suggests

- The golden set has 3 to 18 files per format; 98 % is therefore "all of them" (one miss would be 2 to 33 %). The bench
  (1000 synthetic variants of the same files, all published) is the larger sample.
- The bench files are small (3.7 MiB for 1000 files, one page pdfs); the memory figures are for that mix.
- The 20-page pdf/docx latency was measured with the one-page golden files, so the 30 s target has headroom here that a
  real 20-page file will use.
- CRC64: RustFS and MinIO return no CRC64 for objects uploaded without a checksum (probe says `crc64: None`); the
  verification path is tested against an in-process S3 look-alike (`crates/oo-connectors/tests/s3_mock.rs`).
- The official MinIO image left Docker Hub; the compatibility cell runs the drop-in fork `pgsty/minio` (pinned by digest).

## Reproduce

```
tools/oo-test/oo-test l3 run --suite super-ontology --run-id <id>                # items 1, 3-5, 8-10
tools/oo-test/oo-test l3 run --suite super-ontology --only faults --run-id <id>  # item 2
tools/oo-test/oo-test l3 run --suite super-ontology --bench cold1000 --run-id <id>  # item 6
tools/oo-test/super_ontology_s3_life.py --store rustfs|minio                       # item 5, s3 half
```

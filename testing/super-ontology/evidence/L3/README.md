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

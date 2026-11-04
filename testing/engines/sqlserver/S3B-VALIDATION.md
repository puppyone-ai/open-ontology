# S3-E-02 full-fixture and durable-cycle sign-off

Author acceptance, 2026-11-04. Machine-readable artifacts and SHA-256 values:
`S3B-RESULTS.json`. No Gateway, customer-system or production acceptance claim.

## Results

| SQL Server | Observed patch | Full T0/T1/T2/T8 | T7 | Platform |
|---|---|---|---|---|
| 2019 CU32 | 15.0.4430.1 | pass, run A | pass, run B (reduced) | linux/amd64 emulation |
| 2022 CU20 | 16.0.4205.1 | pass, run C | pass, run C (full) | linux/amd64 emulation |
| 2025 CU1 | 17.0.4006.2 | pass, run C | pass, run D (reduced) | linux/amd64 emulation |

- A: `h-s5-20261104T110027508904Z-113387a9-3026-4c2d-a8b4-81bc3355cfb7`
- B: `h-s5-20261104T111630531947Z-2c00f4cb-f200-4085-9e43-dadb2ecfe5a4`
- C: `h-s5-20261104T112000431525Z-e9dcef45-c031-4f3a-a0b3-da8e9dace4e4`
- D: `h-s5-20261104T115934108048Z-3f2cd793-d2e2-46df-8011-7098fe2c3313`

Full runs use:

```sh
tools/oo-test/oo-test matrix --engine sqlserver \
  --versions 2019,2022,2025 --suites T0,T1,T2,T7,T8
```

Run C retries `--versions 2022,2025`. Runs B/D use `--scale 1000 --tables 3`;
D selects only `--versions 2025`. Reduced audit runs are not scale evidence.

Each full T1 compares 231 tables and 2,000,848 rows, including 2M events,
200 small tables and a 300-column table. Concurrent admin writers committed
131/125/123 transactions respectively; no writer errors in accepted runs.
All table/range comparisons matched. Source doctor passed; aggregate doctor
is intentionally blocked by the absent model secret (no model call).

All three versions completed three C5, three CT and three CDC cycles through
separate `oo-load` processes. Every cycle matched Agent DB. Idle cycles had
zero logical target writes/new generations and zero C5 payload. CT/CDC
positions were monotonic; wrong epochs were rejected without durable-state
changes. The incremental workload is the keyed growth table, not all 231
tables simultaneously.

T7 classified 29 SELECT batches and eight snapshot-session batches per
accepted audit; no loss or truncation. All business rows and non-system
object identities were unchanged. Six CDC objects were inventoried. Run D
also observed one native idle-scan `cdc.lsn_time_mapping` append, attributed
by unchanged catalog identity, increasing LSN, zero transaction/begin-LSN,
bounded equal timestamps, an enabled capture job, 1-to-65 empty-scan growth
and zero scan errors. Other CDC object changes remain failures.

T8 sampled one source connection, met 25% pacing, and recorded logical
target writes of 112050165 / 112050279 / 111066045 bytes. Model calls: zero.
Sampling is not a continuous server-wide maximum; byte counters exclude
WAL, indexes and metadata.

## Retained failures and verification

Earlier attempts remain intact: sqlcmd input-line wrapping corrupted a
comparison query; empty CDC metadata lacked explicit JSON; transient local
Docker socket refusals interrupted runs; the original 2025 audit correctly
rejected an unattributed idle-scan append. The first attribution retry used
session-0 telemetry, which omitted the live scan counter, and failed.
The correction uses retained individual scan sessions and negative tests;
no gold value or failed historical report was changed.

Evidence `e-s3b-dev`:

- `cargo check --workspace --locked`: passed before each commit.
- `cargo test --manifest-path tools/oo-load/Cargo.toml --locked`: 27 passed,
  six ignored live-fixture tests (not represented as passes).
- `cargo test -p oo-connectors --locked --lib --features sqlserver`: 125 passed.
- Full connector suite: 235 passed, 21 ignored, one pre-existing failure:
  `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`.
  Archived branch-base run and current retry both report `group_signal`
  `rejected` instead of `sent`; see `parse-cli-base.log` and
  `parse-cli-retry.log`. `file_worker_parent` itself passed all eight here.
- `cargo test --manifest-path testing/engines/sqlserver/Cargo.toml --locked`:
  13 passed.
- `python3 -m unittest discover -s tools/oo-test -v`: 57 passed.
- `aggregate.py` rechecked raw artifact hashes, all full comparisons/cycles,
  idle counters, current audit classification, epoch negatives and cleanup.
- Lane-owned source/target containers removed; no shared-file patch or
  Cargo.lock change required. Coordinator review/admission remains separate.

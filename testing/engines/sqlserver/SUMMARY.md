# SQL Server S3-E-02: partial / experimental

Rebased on `v0.2/product-loops` at `fa986ceb1`. No verified/full-card acceptance.

## Delivered

Opt-in tiberius 0.13.0/rustls; verified TLS; scoped, snapshot-bound keyset reads
using source collation; row/byte/deadline/pacing budgets; canonical Arrow receipts;
C5 source digests; CT/CDC candidates bound to database GUID, recovery fork and
schema. Each change poll refreshes its snapshot and invalidates old chunk plans.
No source feature is enabled by product code. Precision codecs derive from preview.
`oo-load` snapshots and its scope/TLS unit test are wired through an in-scope path
module pending the coordinator-owned public export.

## Sequential amd64 live evidence

| Run | Observed version | Evidence run/variant | Concurrent target writer commits | Target logical bytes |
|---|---|---|---:|---:|
| 2019 CT | 15.0.4430.1 | e-s3-20261104-final/2019-ct | 8 | 430 |
| 2022 CT | 16.0.4205.1 | e-s3-20261104-final2/2022-ct | 6 | 430 |
| 2025 CT | 17.0.4006.2 | e-s3-20261104-final/2025-ct | 5 | 424 |
| 2019 CDC | 15.0.4430.1 | e-s3-20261104-final2/2019-cdc | 7 | 424 |

Reproduce: `python3 testing/engines/sqlserver/run.py --version <major>
--mode <ct|cdc> --run-id <new-id>`. Lane-local `images.pins` uses H's format.

All four runs pass a **nine-row synthetic integration fixture**, not scale QA:
- Concurrent admin writer; old snapshot values preserved. Real `oo-load` Agent DB
  values match one of two independently queried committed source states while
  insert/update/delete/key moves continue. Replay preserves target write bytes.
- CT: 3 changes including same-session polling; CDC: 4 records; idle poll empty.
  C5 source digests change after mutation and match on idle. No durable target ACK.
- Isolated T7 sample: nonempty, zero-dropped-event Extended Events; CT has
  36 SELECT/6 SNAPSHOT-or-ROLLBACK batches, CDC 40/6. All attributed to reader;
  fixture rows unchanged. CDC internal-object attribution is not proven.
- T8 sample: independently observed reader connection peak 1; pacing >=3x query
  wall time; target bytes recorded above; no model calls.

## Gates (evidence/e-s3-20261104)

- `cargo check --workspace --locked`: pass.
- Harness `cargo test --manifest-path testing/engines/sqlserver/Cargo.toml --locked`:
  13 pass. `python3 -m unittest discover -s testing/engines/sqlserver -p test_runner.py`:
  2 pass. `oo-load` cargo tests: 20 pass, 6 existing opt-in tests ignored.
- Connector tests pass excluding `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`.
  Exact test fails identically on archived base `fa986ceb1`: macOS group_signal
  rejected versus sent. Base `file_worker_parent`: 8 pass.
- `cargo deny --config ci/deny.toml check advisories licenses` and its sqlserver
  feature retry pass; `cargo audit`: zero vulnerabilities. Three spec/plan checks pass.
- Failed TLS fixture, advisory fetch, SQLcmd XML wrapping and `engine_chunk_digest`
  runs are retained. Correct canonical IPC receipts and exact hex audit transport
  fixed their causes; no audit assertion or gold state was weakened.

## Remaining exits / coordinator boundary

**S3-E-02 is not done.** `proposed-integration.patch` is the exact unapplied public
export/CLI feature/doctor diff (apply-check only, not compiled). Public doctor still
rejects SQL Server. `oo-load --c5/--sync` remains explicitly PostgreSQL-only.
Missing: >=3 durable SQL Server incremental target cycles and idle zero rewrite;
full shapes/scale (2M rows, wide/many tables, live collation/key cases); negative
permission/retention/epoch acceptance; 2022/2025 CDC evidence. Unique-index-only
and keyless tables use a single bounded read. Unsupported types/key shapes and
oversized change windows fail closed; large LOB allocations remain unverified.
Snapshots are not cross-process restart tokens. Candidate positions are not ACKs.

All created containers removed; unrelated services untouched. Pinned images cached.

## Proposed docs/product/05 text

SQL Server 2019 CU32, 2022 CU20 and 2025 CU1 have lane-authored amd64 synthetic
snapshot/CT/C5 evidence, plus 2019 CDC evidence. Support remains experimental,
not verified. Native positions bind database GUID and recovery fork; checkpoints
require target commit. Public doctor wiring, full fixture coverage and durable
incremental target-cycle acceptance remain pending.

## Result integrity

- `e-s3-20261104-final/2019-ct/result.json`: `a7a9cd589a8a668966e6a7888c69b49ead8af2d5f71d9ea50bd77a1f47ca7548`
- `e-s3-20261104-final2/2022-ct/result.json`: `40fa2bbaec4b3403706f7edb9438bc72139e651527cb297128974facd09466f5`
- `e-s3-20261104-final/2025-ct/result.json`: `ce014c151431aebee08439a0edd462e1d601bbe5cfa3870e55c8177922226591`
- `e-s3-20261104-final2/2019-cdc/result.json`: `43686ce06f4ec481d2c3e83dfeb2be1f0f7e2332c22b2da45ae076c63ad83988`

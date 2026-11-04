# C-S3c: family loader acceptance

Date: 2026-11-04. TODOs: S3-C-02, S4-C-03.
Status: author-complete for the card's local acceptance scope.

Rebased ten prior lane-C commits onto integration `c1d944b36`. The
`6444b4c83` conflict is now `952e1dbe3`: M's constructor/sync/C5 layout is
retained, with the explicitly selected family passed to `mysql_engine`.
The target duration-to-interval mapping from `a342d058c` is retained.

Product changes: domain/server-aware MariaDB epochs (not advancing GTID
sequences), explicit C1 refusals for TiDB/OceanBase, family C5 identity
probes, and awaited MySQL-wire teardown on snapshot/C5 exit. MariaDB C1
remains primary-only and behind `mysql-binlog`. TiDB membership or process
restart conservatively requires rebaseline; OceanBase uses tenant ID and
creation time, requiring SELECT on `oceanbase.DBA_OB_TENANTS`.

## Reproduce

```sh
cargo build --manifest-path tools/oo-load/Cargo.toml --locked \
  --features mysql-binlog --target-dir target
uv venv evidence/c-s3c-20261104/venv
uv pip install --python evidence/c-s3c-20261104/venv/bin/python PyMySQL==1.1.2
# Run serially, each with a fresh run ID:
evidence/c-s3c-20261104/venv/bin/python testing/engines/mariadb/run.py \
  --family mariadb --run-id c-s3c-mariadb-closed --acceptance
evidence/c-s3c-20261104/venv/bin/python testing/engines/mariadb/run.py \
  --family tidb --run-id c-s3c-tidb-closed --acceptance
evidence/c-s3c-20261104/venv/bin/python testing/engines/mariadb/run.py \
  --family oceanbase --run-id c-s3c-oceanbase-closed --acceptance
```

All three runner commands exited 0. They extend `tools/oo-test/t2_p0.py`,
reusing M's Agent DB, receipt and SourceMeter conventions, not a fork.
The source is read-only in product code; fixture administration, writer,
verification and sampling use separate admin connections.

## Final matrix

| Version | T0 | T1 | T2 C1 | T2 C5 | T8 | Writer commits | Maximum sampled duty |
|---|---|---|---|---|---|---|---|
| mariadb-10.11.19 | pass | pass | 3 cycles | 3 cycles | pass | 9 | 3.82% |
| mariadb-10.6.28 | pass | pass | 3 cycles | 3 cycles | pass | 16 | 2.65% |
| mariadb-11.4.13 | pass | pass | 3 cycles | 3 cycles | pass | 4 | 10.76% |
| mariadb-11.8.9 | pass | pass | 3 cycles | 3 cycles | pass | 10 | 6.20% |
| pingcap-tidb-v7.5.0 | pass | pass | named refusal | 3 cycles | pass | 27 | 5.62% |
| pingcap-tidb-v8.5.0 | pass | pass | named refusal | 3 cycles | pass | 5 | 14.39% |
| oceanbase-oceanbase-ce-4.3.5.4-104000042025090916 | pass | pass | named refusal | 3 cycles | pass | 8 | 17.15% |

T1 compares the real Agent DB against all six fixture shapes, including
signed TIME(6), decimal(30,8), unsigned bigint, JSON, binary, string/composite
keys, duplicates and empty tables. The first load overlaps committed writes;
B-consistency correction rereads after quiescence before exact comparison.
T2 compares both keyed tables after each cycle, checks non-regressing C1
positions, and requires zero payload/generation/physical target rewrites on
the third (unchanged) cycle. Every MariaDB version rejects both a wrong epoch
and an actual changed GTID domain without modifying durable target state.

T8 uses 10ms persistent admin sampling, preserving the 25% duty threshold
and connection limits of one for snapshot/C5 and three for C1. Comparison
connections are not attributed to the product reader. Samples and source
metadata are retained; sampled budgets are not an unsampled hard-peak proof.

## Evidence integrity

All final source manifests match the delivered product and runner files.
All seven runs used loader SHA-256:
`6bc074388b4e432904be0dfd7a098f64e9779330343f8c6dc7ae3fbedf2351e9`.
Raw outputs are under ignored `evidence/<run-id>/`. The following SHA-256
values identify each version's `acceptance/acceptance.json`:

| Run ID | Version directory | SHA-256 |
|---|---|---|
| c-s3c-mariadb-closed | mariadb-10.11.19 | `256feb736c79a91251222521125bf6f1b0e4cfd489c887f4c53b0cd9578d3583` |
| c-s3c-mariadb-closed | mariadb-10.6.28 | `619a5821130e02a6b9088631ed8daec97241031c1eab0d4c20db58815848f3c9` |
| c-s3c-mariadb-closed | mariadb-11.4.13 | `63244f4eec37e30149492592c294e451b0615b317f45a9547485927f67499df8` |
| c-s3c-mariadb-closed | mariadb-11.8.9 | `4319a52505ef52edda6c65491cd8bee8c8a91bb0218eb7feeaf26ae3f3ad1a37` |
| c-s3c-tidb-closed | pingcap-tidb-v7.5.0 | `03c4aefba67caf59a3bf5f60a0c57e0cea8967abc915039555625cbfd05fbfe6` |
| c-s3c-tidb-closed | pingcap-tidb-v8.5.0 | `75ac90896a7522055dca7f391af2e3010dfed4eb7ccd78d7147b7ca7fe90212b` |
| c-s3c-oceanbase-closed | oceanbase-oceanbase-ce-4.3.5.4-104000042025090916 | `33db9dc0a59470a6028e7d6bb34782c81422cab3aa350ff8de9c7121d6d5de9f` |

`evidence/c-s3c-20261104/final-audit.json` confirms seven passing reports,
matching source manifests and binary hashes, six compared tables per version,
all expected cycles, no generated credentials in logs, and no lane-C containers.
Container passwords and TLS keys remain only in ignored private fixture files;
bootstrap-log credentials are redacted before persistence.

## Non-live validation

Raw logs: `c-s3c-20261104`.

- `cargo check --workspace --locked`: passed before each commit.
- `cargo fmt -p oo-connectors` and loader-manifest formatting/check: passed.
- `cargo test -p oo-connectors --locked --features mysql-binlog --lib`:
  205 passed, four ignored.
- Full connector tests: 326 passed, 31 ignored, one pre-existing host failure.
  `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`
  fails with `group_signal=rejected`, expected `sent`. Reproduced on base
  `c1d944b36` in this same worktree (`base-failure.log`), then returned to the
  lane branch. A separate full run excluding only that proven failure passed.
- Loader tests via `cargo test --manifest-path tools/oo-load/Cargo.toml --locked`:
  11 passed by default, 12 with `--features mysql-binlog`; six ignored each.
- `python3 testing/engines/mariadb/test_runner.py`: seven passed.
- `python3 -m unittest discover -s tools/oo-test -p 'test_*.py'`: 48 passed.
- `specs/scripts/check_spec.py`, `check_plan.py`, `check_v02_plan.py`: passed
  (document integrity only, not product acceptance).
- Ignored tests are not acceptance passes; live acceptance is the matrix above.

## Preserved failures and limits

Earlier runs remain intact: `c-s3c-tidb-a` (fixture root authentication),
`c-s3c-tidb-b` / `c-s3c-tidb85-c` (comparison incorrectly used TiDB's
fraction-truncating TIME_TO_SEC), `c-s3c-tidb-d` / `c-s3c-oceanbase-a`
(coarse 200ms T8 sampling), `c-s3c-oceanbase-final` (missing metadata SELECT),
and `c-s3c-oceanbase-grant` (extra reader session at a cycle boundary).
Fixes preserve the typed gold and T8 thresholds: parse actual TIME(6), sample
at 10ms, use a separate verification identity, and await product disconnect.

Scope is local synthetic fixtures: MariaDB and OceanBase standalone, TiDB
unistore single-node. This does not establish distributed TiKV, replicas,
production scale, Gateway publication or source-real-time freshness.
No support-matrix, docs/specs/execution state, root dependencies or lockfiles
were changed. No shared-file patch is requested. Coordinator acceptance and
matrix promotion remain separate. No created containers remain running.

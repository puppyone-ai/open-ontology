# Lane-C compatible-family fixtures

Current loader acceptance: [C-S3c summary](C-S3c-SUMMARY.md).
Earlier C-S3b results and blockers are retained in the [historical summary](C-S3b-SUMMARY.md).

Run from the worktree root:

```sh
python3 testing/engines/mariadb/run.py --family mariadb --run-id <unique-id>
python3 testing/engines/mariadb/test_runner.py
```

This runner owns only freshly named, lane-labeled disposable containers.
It never connects to an existing server. Source connections use TLS and a
SELECT-only account; a separate admin connection seeds and mutates fixtures.
Generated passwords remain in private, ignored evidence files, not argv.

Images use exact patch versions and runnable platform digests from
`testing/infra/images.pins`, recorded with source SHA-256 manifests.
MariaDB uses arm64; TiDB/OceanBase use amd64. At most two
lane-C containers may run; OceanBase requires an otherwise idle lane. Free
disk gates are 20 GiB for every family, checked before pull and creation. A cleanup failure
halts the matrix and names only the owned container requiring attention.

The Rust fixture is ignored by normal cargo tests and requires an explicit
OO_FAMILY_FIXTURE config produced by this runner. Normal unit-test success is
not evidence that the live fixture ran. See SUMMARY.md for tested revisions
and the distinction between connector checks and full T0-T2 acceptance.

To exercise the actual loader against a fresh PostgreSQL Agent DB:

```sh
CARGO_TARGET_DIR=target cargo build --manifest-path tools/oo-load/Cargo.toml --locked
python3 testing/engines/mariadb/run.py --family mariadb --loader-snapshot --run-id <unique-id>
```

The optional mode starts at most one source and one lane-C target. It compares
static rows (including duplicates and NULLs) against independent fixture gold.
It is not concurrent-writer T1, C1/C5 through the loader, or T8 acceptance.
The typed table is mandatory: unsupported duration loading fails the suite,
not a skip. `loader/result.json` records partial comparisons and cleanup.
`shared-loader.patch` is a coordinator proposal, not an applied product fix.
## C-S3c loader acceptance

`run.py --acceptance` extends `tools/oo-test/t2_p0.py`; it does not use the
legacy static-only `load.py` path. Build the feature-enabled loader first:

```sh
cargo build --manifest-path tools/oo-load/Cargo.toml --locked \
  --features mysql-binlog --target-dir target
uv venv evidence/family-env
uv pip install --python evidence/family-env/bin/python PyMySQL==1.1.2
evidence/family-env/bin/python testing/engines/mariadb/run.py \
  --family mariadb --run-id family-mariadb --acceptance
```

Use `--family tidb` or `--family oceanbase` with a new run ID for each.
Runs are serial and create at most one source and one Agent DB. Do not run
multiple family runners concurrently. Source and target cleanup is automatic.
Runtime credentials and TLS material remain under ignored evidence directories.

The shared runner compares all six fixture shapes (including decimal,
unsigned bigint, JSON, bytes and signed microsecond duration), commits writes
during the first snapshot, and checks the quiescent correction against the
Agent DB. It runs three C5 cycles for every family and three C1 cycles for
MariaDB. The third cycle must write no payload or new target generation.
MariaDB also rejects a changed live GTID domain without changing target state.
TiDB/OceanBase C1 must return their named `*_c1_unsupported_use_c5` error.

T8 retains the 25% sampled duty limit and one-reader limit (three for C1).
The family sampler uses a persistent, separate admin connection at 10ms
intervals; verification reads use that separate admin identity, not the
metered product user. No sampler failure or missing observation is a pass.

OceanBase C5 requires read-only `SELECT` on `oceanbase.DBA_OB_TENANTS` for
the tenant incarnation. The fixture administrator grants it explicitly.
TiDB's DDL server identities are conservative: membership or process restart
requires a new baseline. Its local fixtures use unistore, not distributed TiKV.
MariaDB C1 remains primary-only and behind the non-default `mysql-binlog`
feature. See `C-S3c-SUMMARY.md` for evidence and earlier failed runs.


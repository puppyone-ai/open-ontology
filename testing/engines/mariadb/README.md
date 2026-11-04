# Lane-C compatible-family fixtures

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

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

Images are resolved and started by immutable digest, recorded with source
SHA-256 manifests. MariaDB uses arm64; TiDB/OceanBase use amd64. At most two
lane-C containers may run; OceanBase requires an otherwise idle lane. Free
disk gates are 3 GiB for MariaDB/TiDB and 16 GiB for OceanBase. A cleanup failure
halts the matrix and names only the owned container requiring attention.

The Rust fixture is ignored by normal cargo tests and requires an explicit
OO_FAMILY_FIXTURE config produced by this runner. Normal unit-test success is
not evidence that the live fixture ran. See SUMMARY.md for tested revisions
and the distinction between connector checks and full T0-T2 acceptance.

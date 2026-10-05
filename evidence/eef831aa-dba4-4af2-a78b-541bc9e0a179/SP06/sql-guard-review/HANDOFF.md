# Handoff — a179

## Result

Verdict: **APPROVE LIMITED** `03512f91324ac33e772e9702fe806e8e04037ec4`.

`guard_select` admits one supported `SELECT` through `pg_query` 6.2.1, rewrites stars and logical names on that AST, and deparses. The Linux 1.94 pre-overlay smoke exited 0 (library 5, author `sql_guard` 24, clippy, arch-check). The fresh oracle exited 0 (5 tests, clippy). PostgreSQL 17.10 over TLS executed the guarded join and star as `oo_a179_shadow`, which is not the table owner, and the rows matched. Hidden column and insert returned `42501`. The logical relation returned `42P01`.

Read `FINDINGS.md` before treating the snapshot OIDs as something the guard resolved, or `count(*)` `ok:1` as a column-GRANT proof. The 500 ms parser worker is not implemented. `oo-gateway` still has no public entry. Do not put `guard_select` on an untrusted gateway path. Node, depth, and byte budgets are not that worker, and the tokio multi-thread runtime is not that worker.

## Do next

Leave the limited acceptance in place. Do not edit `sql_guard.rs`, the lock, or `ci/deny.toml` to widen this slice. Do not close SP06-W02. Do not add an executor, an HTTP route, or an audit writer in order to publish this function. Parser process isolation remains the blocker for a public read API.

The author's precise `cargo update -p pg_query --precise 6.2.1` stands. Do not rewrite `Cargo.lock` to hide it. A later supply-chain review can run `cargo deny` against the PostgreSQL license headers inside the crate. That is not an admission defect.

## Review

Grok 4.7 in this session reviewed the fixed commit, wrote the harness, and ran the builder and the one database. The author session was a different evidence commit (`957a8076`). No release is claimed. No parent checkbox was changed.

## State left behind

Own containers, volumes, network, and `/tmp/oo-a179-*` are removed (`cleanup.txt`). Failed harness attempts remain under `linux/probe/attempt-1/`, `attempt-2/`, and `attempt-3/`. Other tasks' containers were not removed. `docker prune` was not used.

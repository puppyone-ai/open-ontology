# Handoff — a175

## Result

Source commit `03512f91324ac33e772e9702fe806e8e04037ec4` adds `oo_gateway::sql_guard`, contract `contracts/gateway/sql-guard-v1.md`, and the `pg_query` 6.2.1 lock edges. The Linux 1.94 focused run exited 0: library tests (5), `tests/sql_guard.rs` (24, live path skipped), clippy `-D warnings` on `--all-targets`, and `oo-arch-check`. A separate run with `OO_A175_LIVE=1` executed the guarded select on PostgreSQL 17.10 under `oo_r_dispatcher` and compared 42 / west. The oracle is `live/oracle.txt`. The lock delta is `linux/lock/added.txt` (35 added, `linux/lock/removed.txt` empty).

Read `FINDINGS.md` before treating the license split or the live SQLSTATE values as a privilege proof. `cargo deny` was not run. Prost rejects a 48-`NOT` nest as a parse error before the depth budget of 128 can fire. Unguarded `SELECT * FROM logistics.shipment` returned `42P01`. Guarded `count(*)` returned one row.

## Do next

Independent review of `03512f91324ac33e772e9702fe806e8e04037ec4`. The reviewer should check the deparse idempotence path, the physical recheck, the star expansion against hidden columns, and the 35 new lock pairs.

Leave `policy.rs`, frozen migrations, core, connectors, pipeline, builder, the architecture gate, spec, and the ledger unchanged. Do not edit `pg-policy-v1.md` to claim this parser. Do not add an executor, an HTTP route, or an audit writer in order to close SP06-W02. The parent item stays open.

## Review

Grok 4.7 in this session wrote the library, the tests, the contract, and these notes, and ran the builder and the one database. That is one person. An independent review is still open. No release is claimed. No parent checkbox was changed.

## State left behind

Own containers, volumes, network, and `/tmp/oo-a175-*` are removed (`cleanup.txt`, each command exit 0). Failed attempts remain in this directory. Other tasks' containers were not removed. `docker prune` was not used.

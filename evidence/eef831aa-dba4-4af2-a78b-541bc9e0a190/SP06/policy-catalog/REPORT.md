# a190 live PostgreSQL PolicyCatalog

Verdict: APPROVE LIMITED. The loader reads one real catalog snapshot and the live test ran that snapshot through `guard_select` and `DedicatedSession`.

Source commit `78a60fdeabcada0d024fc6f9a81e0e6b184260e8`. Fixed archive `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6`. Contract `oo.gateway.policy-catalog/v1`. SP06-W01 and SP06-W02 stay open. Authentication, the audit WAL, row-level security, the public worker, the pool, and the rest of the R4 whitelist stay open. No release is claimed.

## What was built

`policy::catalog::load_policy_catalog` takes a trusted TLS `tokio_postgres` client, compiled column ACL policies, an explicit `PolicyIdentity`, and validated limits. `crates/oo-gateway/src/policy.rs` gained only:

```rust
#[path = "policy_catalog.rs"]
pub mod catalog;
```

`lib.rs`, `sql_guard.rs`, `query_executor.rs`, manifests, and the lockfile were not edited. The build used a `git archive` of `3a251dea` plus the three owned Rust files. Moving workspace files were not copied into that archive.

The loader checks identity coherence before it uses the client. It then opens one `REPEATABLE READ READ ONLY` transaction, checks `max(run_generation)` for the source and config digest, checks the seven `NOLOGIN` role flags, and reads base-type OIDs, the comparison operators, and `count(*)` from `pg_catalog`. Physical names come from `_oo.physical_mappings` and `_oo.lookup_physical_mapping`. A column is visible only when the compiled allowlist names it and `has_column_privilege` returns true. Published columns whose type is not one supported base type refuse the load. Hidden columns, including `_oo_pos`, contribute to `pending_unclassified` only. `reltuples` of `-1` follows the caller's `UnknownRows` policy. The transaction is committed, then `pg_current_xact_id_if_assigned()` must be null.

The admitted set is `pg_catalog.count(*)` plus `<`, `<=`, `<>`, `=`, `>`, `>=` for bool/bool, int2/int2, int2/int4, int4/int2, int4/int4, int4/int8, int8/int4, int8/int8, and text/text. That is 54 operators. Each must be an immutable internal non-extension member. Integer literals use the `int4` OID. Missing any required object fails the load.

## Pre-edit smoke

`linux/launch.exit` is 0. Image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, Rust 1.94.0. The container mounted `/tmp/oo-a190-fixed` read-only. That archive has no `policy_catalog.rs`. `linux/lib-test.log` reports `test result: ok. 10 passed; 0 failed`. Clippy on that archive exited 0. This smoke does not exercise the new loader.

## Focused proof

One builder, `oo-a190-rust194`, on the same image. `RUSTUP_TOOLCHAIN=1.94.0`. Private bridge `oo-a190-net`, cap-drop ALL, no-new-privileges, memory 8g, 4 cpus. Cargo and target volumes were private. At start, `host_cargo=0 rust_builders=0 global=0 cap=6`.

PostgreSQL was already up: `oo-a190-pg`, image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64. No second database was started. `pg/up.exit` is 0. The fixture database was `oo_a190`, distinct from earlier gateway fixtures. Frozen migrations `0000`, `0001`, `0003`, `0004`, and `0005` were applied by the test. `0002` was not applied.

Final phase exits, all 0:

- libclang, rustfmt, rustfmt-component, clippy-component
- `cargo test -p oo-gateway --lib`: 14 passed, 0 failed
- `cargo test -p oo-gateway --test policy_catalog_pg_live` with `OO_A190_LIVE` unset: 1 passed
- `cargo clippy -p oo-gateway --lib --test policy_catalog_pg_live -- -D warnings`
- `OO_A190_LIVE=1` the same test: `ok`, finished in 0.83s

`focused/launch.exit` is 0. Container inspect: exit 0, oom false, privileged false, network `oo-a190-net`, cap-drop ALL.

`live/oracle.txt` records the fresh fixture:

- `reltuples_before=-1`, then `estimated_rows=3` after `ANALYZE`
- explicit estimates `0`, `max`, and `unknown`
- clerk pending 2, auditor pending 6, revoked `paid` pending 3, added `extra` pending 3
- type OIDs read back from `pg_catalog`: int2 21, int4 23, int8 20, text 25, bool 16
- `function_oid=2803`, `operator_count=54`
- `pg_sleep`, `count(any)`, and int4 `+` were not admitted
- star returned 3 rows, `count(*)` where `paid = true` returned 1, auditor amount rows were 3
- guard refusals for function, write, and hidden column; direct `SET ROLE` refusals were SQLSTATE `42501`
- `fixture.touch` was not admitted; altering published `memo` to a domain returned unsupported
- every loader window recorded `scan_delta=0`; mapping count stayed 16
- `done=ok`

The oracle has no passwords and no `pt_` or `pc_` names. A scan of the evidence tree against the fixture password files found no copies.

Committed blob SHA-256 matches `focused/source-sha256.txt` and `source/sha256.txt`.

## Attempts kept

- `focused/attempt-1`: library compile failed. The commit still borrowed the client, and `unwrap_err` required `Debug` on `BoundInput`.
- `focused/attempt-2`: the integration test failed to compile. The oracle writer needed `&mut self`, a text array was passed as `[&str; 5]`, `Stats` lacked `Debug`, and insert needed `&mut Client`. Library tests had already passed, 14 of 14.
- `focused/attempt-3`: Clippy `-D warnings` rejected digit grouping, two one-element clones, and the insert tuple. The compile-only test had passed.
- `focused/attempt-4`: the live test panicked because `count(int4)` is not a `pg_proc` row. PostgreSQL 17.10 has `count(*)` (`pronargs` 0, OID 2803) and `count(any)` (`pronargs` 1, OID 2147). The catalog load had already completed. The test now requires that `count(any)` OID to be absent.

## Cleanup

Own containers, volumes, network, archive, overlay, and `/tmp` secrets were removed. Every `cleanup/rm-*.exit` is 0. `oo-a190-rust194`, `oo-a190-pg`, volumes `oo-a190-cargo`, `oo-a190-target`, and `oo-a190-pgdata`, and network `oo-a190-net` are gone. Exited containers labeled `a064` were left in place. No prune ran.

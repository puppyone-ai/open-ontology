# Findings — a179 SQL admission review

Verdict: **APPROVE LIMITED** `oo_gateway::sql_guard::guard_select` on `03512f91324ac33e772e9702fe806e8e04037ec4`, tree `58592c770e4b4cb17d1b653f52964b44da870c80` (`linux/smoke/archive-tree.txt`). Author evidence commit `957a807610b8f330c64f5b92e9d94f927fdae532`.

The measure is contract `contracts/gateway/sql-guard-v1.md` for one supported `SELECT`. Docs `docs/07-gateway.md` section 4.2 rules R1–R9 stay the parent rules. This review does not close SP06-W02, does not publish a read API, and does not claim a release.

No product, dependency, or gate line needs to change for this limited acceptance. The deciding fresh run is `linux/probe/oracle-test.log` (5 passed) and `live/oracle.txt`. `linux/probe/attempt-1/` and `linux/probe/attempt-2/` are preserved harness misses. `linux/probe/attempt-3/` is the same oracle before the clippy component was present in a new container. `linux/smoke/` is the pre-overlay archive.

## F1. What was admitted

`guard_select` (`crates/oo-gateway/src/sql_guard.rs` lines 424–429) parses with `pg_query` 6.2.1, rewrites the AST from a caller-supplied snapshot, and deparses. Lines 457–485 parse that deparse again, require the second deparse to match, then admit the executed SQL on a physical-name catalog (`Surface::Physical`, line 482). A failure in that chain is `STATEMENT_NOT_ALLOWED` with a static serialize reason. The original text is not returned as executed SQL (lines 153–161).

The fresh harness called `pg_query::parse` and `pg_query::deparse` on the executed SQL itself. The second deparse matched. A second `guard_select` on a catalog whose logical names were the physical names (`ship_store.pt_ship`, `ship_store.pt_counter`) returned the same SQL and an empty star list. The executed text contained the physical names and did not contain `logistics`, `cargo_bay`, `front_desk`, `badge_secret_z9`, or `pc_badge`.

`schema_banned` (lines 1987–1992) refuses `_oo` on the user surface and allows it on the physical recheck surface. The live catalog used `_oo` as the approved physical schema. The public call still returned a plan, which is the internal recheck succeeding. A user-surface snapshot whose relation schema is `_oo` is `CatalogInvalid` reason `relation schema`. That is the contract's split, not a second public name for the shadow table.

## F2. Fresh oracle

`linux/probe/oracle-test.log`: 5 passed, 0 failed, Rust `1.94.0`, `--locked --offline`. Clippy `-D warnings` on `--all-targets` then exited 0 (`linux/probe/clippy.exit`).

Static cases, all on synthetic snapshot OIDs 2301, 2501, 1601, 7001, 7002, 8001, and 8002 so a hardcoded PostgreSQL OID would fail:

- `alias_join_and_star_omit_the_hidden_column`: `s.*` plus `d.desk_name` on an inner join expands to `order_id`, `region` in catalog order. The hidden column is not in the expansion or the executed SQL. Three `=` operators bound, two to OID 8001 and one to 8002.
- `snapshot_oids_bind_one_operator_and_function_signature`: `lower(region)` binds OID 7001. `count(*)` binds OID 7002, stays `count(*)`, and is not a star expansion. `lower(order_id)`, `region = 42`, and `count(order_id)` are `FUNCTION_NOT_ALLOWED` with the text `function or operator is not allowed`.
- `missing_and_hidden_diagnostics_share_one_error`: hidden column `badge_secret_z9` and missing column `ghost_col_z9` produce one display. It lists `logistics.cargo_bay.order_id` and `logistics.cargo_bay.region`, ends with `1 columns are pending classification` (`column_message`, lines 2054–2066), and does not contain the hidden name or `pc_badge`. Hidden relation `hidden_bay_z9` and missing relation `missing_bay_z9` both display `relation is not visible` with no hint. A star on a relation whose estimate is 42424242, including `WHERE 'yes'::bool`, is `COST_TOO_HIGH`. The message names `logistics.cargo_bay` and does not contain `42424242`. `WHERE 'no'::bool` is admitted and the star still omits the hidden column.
- `unknown_ast_fails_closed_without_echoing_the_input`: a `DELETE` inside `WITH`, and `EXPLAIN INSERT`, are `Write` (`write statement is not allowed`). A subquery, `$1`, `IN`, and `order_id::int` are unsupported with static labels (`subquery`, `parameter`, `IN predicate`, `cast`). `pg_catalog.pg_class` is `system schema is not allowed`. `SELEKT badge_secret_z9` is `SQL parse failed`. None of those displays contain `badge_secret_z9`, `cargo_bay`, `pg_class`, or `pg_catalog`.

`live/oracle.txt`, PostgreSQL 17.10, TLS, `search_path` empty:

- `admin_ssl=on`, `reader_ssl=on`, certificate SAN `DNS:oo-a179-pg`.
- `tableowner=oo_a179_owner`, `session_user=oo_a179_login`, `current_user=oo_a179_shadow`. The executing role is not the owner.
- Guarded join of `region` and `desk_name` returned `west` and `ada`, equal to the administrator's read of the physical columns.
- Guarded `s.*` returned one row, two columns, `42` and `west`.
- `SELECT pc_badge FROM _oo.pt_ship` returned `42501`.
- `INSERT INTO _oo.pt_ship` returned `42501`.
- `SELECT order_id FROM logistics.cargo_bay` returned `42P01`. Schema `logistics` exists and has no such table.
- `count(*)` returned `ok:1`. That is not a column-GRANT proof. Contract line 89 still allows a later `42501`.

The harness read `pg_type`, `pg_operator`, and `pg_proc` and put those OIDs in the snapshot (`oid_source=pg_catalog_query_by_harness`). `guard_select` did not resolve them. This is not an authentication proof and not an OID-resolution proof.

## F3. Pin, native build, and the 35 lock pairs

`pg_query` 6.2.1 is `registry+https://github.com/rust-lang/crates.io-index`, checksum `da43168013d137b3ca49b0044c1108ec09ff596a0626d4db89209c0a1d450cd2`. That matches the contract. The fetched `pg_config.h` line 636 is `#define PG_VERSION_NUM 170007`. The crate manifest license is `MIT`. The crate root has no `LICENSE` file. Vendored PostgreSQL sources carry `Portions Copyright (c) 1996-2024, PostgreSQL Global Development Group` (`linux/probe/pg-copyright-sample.txt`). `pg_query` `src/query.rs` contains the FFI `unsafe` sites. Product `lib.rs` line 8 is `#![forbid(unsafe_code)]`. The review grep of `sql_guard.rs`, `lib.rs`, and both test files found no `unsafe` outside that attribute and the Cargo.toml comment.

The pre-overlay smoke compiled `pg_query` v6.2.1 (`linux/smoke/lib-test.log`). Eight Debian trixie arm64 debs matched `linux/smoke/libclang-debs.txt` byte for byte, including `libclang-19-dev` `7502c447f454496501e1e68806190cb55facbe9a23559c04d2e6e17e5ba6bacc` and `libllvm19` `8a48e1a49f7fcc3be74e697767d72902f4b03041401d1529bcfad1849df39148`. They were extracted on the private cargo volume because `/tmp` is `noexec`. `ld-linux` resolved `libLLVM.so.19.1` and `libz3.so.4` (`linux/smoke/libclang-ldd.txt`). `stdbool.h` was present at the recorded LLVM 19 include path. `BINDGEN_EXTRA_CLANG_ARGS` kept the recorded gcc and `/usr/include` paths. The builder had `CapDrop=["ALL"]`, `CapAdd=null`, no privilege, no host network, no published port, and no host home. No capability was added and apt was not used.

`linux/lock/provenance.txt`: 35 added pairs, 0 removed, 0 checksum or source changes against parent `93e6b6a24fab71c10dd9966ee28d5258516bed63`. Every added pair is a crates.io registry pin. Workspace `rust-version` is 1.94.0. The highest `rust-version` among the 35 is `home` 0.5.12 at 1.88. Smoke and probe both ran `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

Native or FFI edges in those 35: `pg_query` (C `libpg_query` plus bindgen), `bindgen` 0.66.1, `clang-sys` 1.9.1 (`links = "clang"`), `libloading` 0.8.9, `rustix` 0.38.44 and 1.1.5, and `linux-raw-sys` 0.4.15 and 0.12.1. `prettyplease` `links = "prettyplease02"` is a cargo links key, not a C library. Manifest licenses are MIT, Apache-2.0, MIT OR Apache-2.0, BSD-3-Clause, or ISC, plus the rustix and linux-raw-sys `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` expression. `ci/deny.toml` allows MIT, Apache-2.0, BSD-3-Clause, and ISC. It has no PostgreSQL SPDX id. `cargo deny` was not run. This pin is not a supply-chain release approval. The gate file was not edited.

## F4. Cargo update deviation

The a175 prompt says `cargoupdatenone` (`execution/prompts/v01-gateway-sql-guard-active.md`). The author still ran `cargo update -p pg_query --precise 6.2.1` (`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a175/SP06/sql-guard/linux/compile.sh`). `linux/lock/update.log` in that evidence says `Updating crates.io index` and notes 27 unchanged dependencies behind latest. The resulting lock is the one reviewed: 35 additions, no removals, no checksum edits of pairs that already existed. This review did not run `cargo update` and did not rewrite `Cargo.lock`. `linux/smoke/lock-before.sha256` and `lock-after.sha256` are both `f9d55dde46b355ba631e03652455cb5f3efe449c8f82e813979ebd2cfb314845`. `cargo-update-invoked.txt` is `no`.

## F5. What stays open

SP06-W02 stays open. The accepted behavior is the contract's `SELECT` subset, not docs R1–R9 as a whole and not the rest of SP06.

Docs R1 treats `WITH ... SELECT` as a `SelectStmt`. This slice refuses `WITH` (contract line 53). Docs R2's write walk is implemented for `InsertStmt`, `UpdateStmt`, `DeleteStmt`, and `MergeStmt`, and the fresh cases confirmed `WITH` plus `DELETE` and `EXPLAIN INSERT`. Docs R4's `pg_proc` whitelist and volatile-function policy are not implemented. Admission is membership in the caller snapshot (contract line 86). Docs R5's ban on `pg_catalog` relations is implemented and the name is not echoed. `pg_catalog` functions and operators remain allowed when the snapshot has them. Docs R7a's star rewrite is implemented. The same pass also rewrites logical names to physical names, which docs section 5.2 does not list. The contract does. Docs R8's 500 ms parse kill and restricted worker are not implemented (contract lines 61 and 72). `sql_guard.rs` contains no `500`, no thread pool, and no timeout. `NODE_BUDGET` (line 27), `DEPTH_BUDGET` (line 30), and `SQL_BYTE_BUDGET` (line 24) are not that worker. Prost's recursion limit, which the contract says turns a 48-`NOT` nest into `SQL_PARSE_ERROR` before depth 128, was not re-measured here.

`crates/oo-gateway/src/bin/oo-gateway.rs` lines 1–4 print that the process entry is not implemented and return exit code 2. Nothing in the gateway crate calls `guard_select` except the tests. Publishing this function on a public or untrusted gateway path is blocked until the 500 ms parser worker exists. The current tokio multi-thread runtime is not that worker.

Also open, and refused when the SQL contains them: authentication, token exchange, HTTP, portals, pools, SQL execution inside the library, row-level security, the audit WAL, sqlparser dialect hints, parameters, general casts, CTE scope, subqueries, `LATERAL`, set operations, `GROUP BY`, `DISTINCT`, windows, `NATURAL` / `USING`, `LIKE` / `IN` / `BETWEEN`, and `ORDER BY` an output alias. `pg-policy-v1.md` was not edited.

## F6. Preserved harness misses

`linux/probe/attempt-1/oracle-test.log` failed in the harness: the `IN` label is `IN predicate`, a public recheck cannot use schema `_oo` (that surface is internal), and `ALTER ROLE ... PASSWORD $1` is a syntax error. `linux/probe/attempt-2/oracle-test.log` counted the join's `ON` operator, so the expected operator count was 2 and the plan had 3. The live section of attempt 2 already recorded the same `live/oracle.txt` facts. `linux/probe/attempt-3/clippy.log` is `cargo-clippy` missing in a fresh container. The final probe installed the component and clippy exited 0. These files are not product failures.

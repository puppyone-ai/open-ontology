# Findings

Author notes for `4f3f329aa0dabb3f848e6040fdc5379df921c2f9`. These are not an independent review.

## Catalog

`load_policy_catalog` and `compile_column_acl` are the author-approved gateway code on baseline `78a60fdeabcada0d024fc6f9a81e0e6b184260e8`. Independent review of that catalog is queued. This command does not change that status and does not widen the compiler.

`CompiledPolicy` fields stay private. The command checks the public request: one project, one digest, one generation, one login, one slug, and `identity.role` equal to `oo_r_` plus that slug. The loader checks the compiled value on the server.

## Privilege

`_oo` is revoked from `PUBLIC`. `lookup_physical_mapping` is granted to `oo_pipeline_runtime`, not to the fixture login. The test, not the command, grants the login `USAGE` on `_oo`, `SELECT` on `_oo.physical_mappings`, and `EXECUTE` on that function before it runs `query-pg`. The command never issues `GRANT`.

The live desk catalog reported `pending_unclassified=2`. The test records that number and checks that `seal` is not visible. It does not require the pending count to be 1.

## Session

`TlsConnection` is `Unpin` on this toolchain. One `tokio::select!` polls the catalog load and the connection future. If the load finishes, the same connection is moved into `DedicatedSession::bind`. If the connection future finishes first, the command returns TLS failure and does not query. The driver is spawned only inside `bind`.

The executor's application name stays `oo-gateway-policy`. Timeouts, cancel, and drop behavior are the executor's. Unknown authentication, claims row filters, and the public parser worker stay blocked.

## Audit

Admission validates hashes, fingerprint, backend pid, and backend start, then opens the file, then `flush` and `sync_all`. A guard refusal does not call the hook, so those runs leave the path absent. An existing path or a non-directory parent is refused before TCP. `pg_stat_database.sessions` was unchanged for those refusals and for a CA that rustls rejected before connect.

`sync_all` covers the file contents the process wrote. It is not a directory fsync, not a WAL, and not a crash-injection proof. A kill after the admission sync and before the completion sync can leave an admit line without a complete line. File mode is not a security claim.

## Lock

`cargo check --offline` was allowed to edit the lock. `focused/lock-edge.rs` compared every package block with the baseline. The only change is the `oo-gateway` dependency list gaining `serde` and `serde_json`, inserted after `rustls` and before `sha2 0.10.9`. Registry names, versions, and checksums of the other packages were unchanged. The workspace lock matched `78a60fde` immediately before that edge was copied back.

## Tests that failed before the green run

Preserved under `focused/attempts/`.

1. The help string closed at an interior quote. The test used `[u8]::contains` for a byte string. `lock-edge` was executed on the noexec `/tmp` mount. `launch.sh` had `set -e`, so a container failure skipped `launch.exit`.
2. `Read::by_ref` was ambiguous on `File`. The lock edge still passed.
3. `CARGO_BIN_EXE_query_pg` is not set by `cargo check --tests`. The fallback name was not on `PATH`.
4. The stdin spawn inherited stdout, so the captured line count was 0. The child output itself had the two turns.
5. `pg_stat_user_tables.n_tup_ins` moved from 0 to 3 while the row digest stayed equal. The fixture inserts became visible during the run. The test now clears the stats snapshot and waits until the three inserts are counted, then requires the same triple after the command.
6. Clippy is not stored in the image layer. `rustup component add clippy --toolchain 1.94.0` was required again, as in the pre-edit smoke.
7. Clippy denied a one-shot `loop` around `select!`, a one-element `clone` into a slice, and the test helper arity. The loop is now one `select!`. The slices use `from_ref`. The helpers allow `too_many_arguments`.

## Out of scope

Caller authentication, HTTP entry, file-permission deployment, production audit retention, and a source-database read are not implemented. No parent SP06 checkbox is closed.

# CDC apply-pg review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a320`. Unit CDC-D04 writer `tools/cdc-apply-pg`, contract `cdc-apply-pg-v1`. Fixed commit `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`, tree `539aa555a721a0b87b40546b9d4dad5569e3c863`. Repo head at both launches: `895864a5bf64e50cef84075d20fd7b0c9e41c264`. The fixed commit is an ancestor of that head. `linux/preoverlay/worktree-vs-fixed.txt` is empty for the product, `crates/oo-core`, the pipeline migrations, and the root lock.

Verdict: **APPROVE_LIMITED** for frozen development child CDC-D04 / E36, `tools/cdc-apply-pg`, at `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`. The accepted scope is the fenced apply proved below: one recovered sealed spool, the existing target installer and physical mapping, canonical `bool` / `int2` / `int4` / `int8` / `text` rows, and `apply_receipts` in one PostgreSQL transaction on one dedicated connection. That includes exact replay, `OO010`, `OO013`, fence `55P03`, both SIGKILL windows, hidden-commit reconciliation, fail-closed `commit_unknown`, row-failure rollback, generation left durable before the write transaction, and `needs_segments` before credentials above 200000 events.

Verdict: **WITHHELD** for frozen development child CDC-D05 / E40, `tools/cdc-offset-store`. This review did not build or run that crate. A durable control mirror and a contiguous CAS watermark were not tested.

Parent SP04 stays open. ACK, XACK, TRIM, source or WAL cross-table certification, segmented apply above 200000 events, a Debezium normalizer, the router, segments, customer delivery, and release stay unaccepted. This file does not edit the parent ledger and does not record coordinator acceptance. The earlier sentence "CDC-D04 stays open" mixed this child with the parent ledger. The proof files were not re-run for the clarification below. This wording adds no review count.

`ack_allowed` stays false. `source_cross_table_consistent`, `upstream_commit_lsn_verified`, `debezium_normalizer`, `p2_verified`, `p11_verified`, and `control_lease_consulted` stay false. `commit_position_provenance` stays `caller_declared`. `db_cross_table_consistent` is true only for this process's one local transaction. Segmented 500000-event apply, source or WAL certification, a Debezium normalizer, ACK, XACK, TRIM, a control-database mirror, the router, segments, customer delivery, and release remain parent-required.

No product file, root lock, migration, or `crates/oo-core` file was edited. a313 remains the root dependency writer. This review did not open a new spool review.

## What was reviewed

Author evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a301/SP04/cdc-apply-pg`. REPORT, FINDINGS, and HANDOFF were read and left in place. This review's files are only under this directory.

The product build is `git archive` of `52ce234` for `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, `tools/cdc-spool`, `tools/cdc-apply-pg`, and `tools/session-store/Cargo.lock`, then `cp -R` inside the container. The live workspace was not the build tree. The author's proof archived dependency commit `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65` and overlaid a live product tree. This archive already contains `tools/cdc-apply-pg`. `linux/preoverlay/overlay-absent.exit` is 0.

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, Rust 1.94.0. Postgres image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 17.10. Both were local (`--pull never`). Each new builder had 6 GiB, 1 CPU, `no-new-privileges`, CapDrop ALL, private bridge `oo-a320-eef831aa-net`, HOME `/tmp`. One fetch `--locked` was recorded, then tests and Clippy were `--locked --offline -j 1`. Recipe origins are in `recipe-origin.txt`.

The approved spool record is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a299/SP04/cdc-spool-review` at fixed `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`. Its verdict is limited to that spool archive. The author's REPORT, FINDINGS item 4, HANDOFF, and `CONTRACT.md` lines 229–230 still say a299 has not accepted the spool. That sentence is older than the a299 record. It is not a new spool defect, and this review did not re-test the spool.

## Private PostgreSQL

`pg/up.sh` exit 0. Container `oo-a320-pg`, id `0a1225f7e3619e87191d9991756d51195c3dbca7de3972a1ea09d0b217fc9c28`. Memory 1073741824, NanoCpus 1000000000. SAN `DNS:oo-a320-pg` is in `pg/server-san.txt`. No host port. Private keys and password files stayed under `/tmp/oo-a320-eef831aa-dba4-4af2-a78b-541bc9e0a320/secrets`. Foreign Postgres containers were left running.

The author live test reads `OO_A301_*` and creates databases `oo_a301`, `oo_a301_bare`, and `oo_a301_drift`. Preoverlay pointed those variables at this server. Overlay later dropped those databases and the installer roles on this private server before `install()`. That is cleanup of databases this server's author suite created. a301's container was not touched.

## Author suite before the overlay

`linux/preoverlay`, container `oo-a320-preoverlay`, id `fc0f9ddc1bbef753f08dcef6dd4117440d4de6394aadd96dabe802bb14e40081`.

Capacity before start: `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=1`.

Host config: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a320-eef831aa-net`, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000.

Raw exits: docker 0, version 0, source 0, lock-subset 0, fetch 0, test 0, clippy 0, lock-stable 0, product-unchanged 0, core-migrations-unchanged 0, overlay-absent 0.

`linux/preoverlay/lock-subset.stdout` is `lock_subset_ok tool_checksums=284`. Tool lock SHA-256 `67d47b32d081d28504a3d0fe0fcb431c49c618dd1f5599fa188d6f8878e1e78f`, same bytes after the suite. `linux/preoverlay/author-results.txt`:

- tls 2 passed, 0.00s (`commit_message_is_the_simple_query`, `source_has_no_plaintext_connector`)
- live 1 passed, 1.94s (`fenced_writer_on_private_postgres`)
- local 5 passed, 0.00s
- scale 1 passed, 14.85s (`over_limit_returns_needs_segments_before_connect`)

Nine tests, 0 failed. Clippy `--all-targets -- -D warnings` exit 0. The vendor crate `sqlx-postgres` printed two unfulfilled `dead_code` expectations. The `cdc-apply-pg` Clippy exit is 0.

## Review overlay

The overlay is Safe Rust in `overlay/a320_apply.rs`. Expected row text, the lineage intersection, and position `18446744078004518944` for LSN `1/20` are written in that file. It does not call the author test helpers. `#![forbid(unsafe_code)]` is the package lint. Before this run the review file bound the eighth `REGISTER_SQL` argument (the identity digest), removed an unused kill helper, and named the lines seed table explicitly. There is no failed overlay directory.

`linux/overlay`, container `oo-a320-overlay`, id `3044d3a9309778f4de35e318884c8b781699b8daa1883f033db5c6e13266c9c0`.

Capacity before start: `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=1`.

Host config: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a320-eef831aa-net`, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000.

Raw exits: docker 0, version 0, source 0, overlay-absent 0, lock-subset 0, fetch 0, test 0, clippy 0, lock-stable 0, product-unchanged 0, core-migrations-unchanged 0, lock-subset-after 0. `product-before.sha256` matches `product-after.sha256` and the preoverlay product hash. The after hash excludes `tests/a320_apply.rs`. Core and migration hashes match the preoverlay file. Lock hash unchanged. `lock-subset.stdout` is again `lock_subset_ok tool_checksums=284`.

`linux/overlay/test.stdout`: 2 passed, 0 failed, 20.15s, `--test-threads=1`.

- `a320_upfront_refuses_before_credentials` calls library `apply` with a password path that stays absent.
- `a320_fenced_writer_on_private_postgres` uses the compiled CLI `cdc-apply-pg apply <request.json>`, plus one library `apply` for the zero-row delete.

Phase lines from that log:

- `installed versions=physical-mapping/v1,target-bootstrap/1,target-fence-receipts/1,target-fence-receipts/3`
- `lineage_regex function=1 constraint=1`
- `generation_durable_before_transaction count=1 generation=1 receipt=absent qty=11`
- `applied generation=2 receipt_fence=2 rows=2`
- `replay generation=3 receipt_fence_unchanged=2`
- `oo010 generation=4 receipt_fence=2 qty=9223372036854775807`
- `delete_missing api=inserted lines=1`
- `fence_held sqlstate=55P03 stale=OO013 display_omits_code=1 generation=7`
- `sigkill_before_commit signal=9 generation_kept=8 resumed=inserted`
- `sigkill_after_commit signal=9 replay_fence=10`
- `commit_unknown then_one_row fence=14`
- `needs_segments count=200001 elapsed_ms=2579`

Catalog types from `pg_type.typname` were `int8,int8,text,bool,int2,int4` on orders and `int8,int8,int2,int4` on lines. Both relations reported `relpersistence=p`. Bool `::text` values that matched were `true` and `false`.

The first live refusal was injected `row_failure`. Generation count stayed 1 and the value stayed 1, the receipt was absent, and qty stayed 11. Later connections each advanced the stored generation. OO010 left the receipt fence at 2 and qty at int8 max while the generation value became 4. The holder connection adopted generation 7. The blocked writer, with `control_lease_valid` true, returned `fence_held` / `55P03` and left generation at 7. `control_lease_consulted` stayed false. A direct `commit_apply_receipt` on the holder at the previous generation returned `DatabaseError` code `OO013`. sqlx `Display` of that error did not contain `OO013`.

SIGKILL used `Child::kill` and required `ExitStatusExt::signal() == Some(9)`. Before commit, one `oo_a320_runtime` backend on `oo_a320` held the advisory lock, the receipt and row 400 were absent, and the adopted generation stayed after the signal. The resume inserted row 400 once. After commit and before the response, the same single backend still held the lock, and the receipt and row 401 were already visible. The later CLI was `replay` with `row_writes` 0 and the same receipt timestamp and fence.

`CDC_APPLY_INJECT=hide_commit` returned applied with one row 402. The same inject plus `CDC_APPLY_RECONCILE=down` returned exit 3, kind `commit_unknown`, `target_applied` false, and `db_cross_table_consistent` false. A later normal apply left exactly one row 403. Its receipt `cross_table_consistent` text was `true`, fence 14.

Upfront, with the password file absent the whole time: lineages outside the intersection (`0` plus 63 `a`, `g` plus 63 `a`, and `billing`) returned `identity`; a 64-hex lineage starting with `f` and a missing spool returned `spool_io`; a segment object returned `needs_segments`; declared `numeric`, `float8`, `jsonb`, and bare `int` returned `unsupported_type`; an int cell on the declared text column returned `type_mismatch`; 200001 sealed events with keys and columns cleared returned `needs_segments` in 2579 ms. `pg_get_functiondef` of `_oo.commit_apply_receipt` and `pg_get_constraintdef` of `apply_receipts_lineage_check` both contain `^[a-z][a-z0-9_.:-]{0,127}$`, which is also in `src/model.rs`. The scanned sources `src/apply.rs`, `src/main.rs`, `src/model.rs`, and `src/tls.rs` do not contain `XACK`, `pg_advisory_lock`, `GRANT `, or `TRIM `.

## Author attempts left in a301

Those directories were not copied here. Exits re-read from `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a301/SP04/cdc-apply-pg`:

| directory | docker | test | clippy |
|---|---|---|---|
| linux/precode | 0 | absent | absent |
| linux/proof | 0 | 0 | 0 |
| linux/proof-1 | 1 | 101 | absent |
| linux/proof-2 | 101 | 101 | absent |
| linux/proof-3 | 101 | 101 | absent |
| linux/proof-4 | 101 | 101 | absent |
| linux/proof-5 | 101 | 0 | 101 |
| linux/proof-6 | 101 | 0 | 101 |
| linux/proof-7 | 101 | 0 | 101 |

The author's REPORT describes proof-1 as a bool `?` and 14 lock checksums outside the fixed locks, proof-2 as a live-test compile failure, proof-3 as an `UPDATE` of `_oo.schema_migrations` rejected by the immutability trigger, proof-4 as sqlx `Display` omitting `OO013`, and proof-5 through proof-7 as Clippy on `collapsible_if`, `manual_unwrap_or`, and `too_many_arguments`. `linux/precode` is the author's smoke on the dependency archive before the product overlay. This review's archive includes the product, and the author suite above ran on that tree.

## Inventory

`product.sha256` and the first `MANIFEST.sha256` were produced by the byte-copied a199 `tools/sha256_manifest.rs` (`d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`), over the fixed-commit archive at `/tmp/oo-a320-eef831aa-dba4-4af2-a78b-541bc9e0a320/preoverlay/archive/tools/cdc-apply-pg`. That archive and the private Postgres are already removed. `product.sha256` stays the 12-file product inventory from that run. `linux/manifest/MANIFEST-first.sha256` is a byte copy of the first evidence inventory, retained before this clarification.

The clarification rewrites `MANIFEST.sha256` once, with the same owned tool, after this text. It repeats no test and starts no PostgreSQL. The first run's `linux/manifest/console.log`, `docker-run.exit`, `hostconfig.txt`, and `container-removed.txt` stay on disk and are rows of the rewrite. The rewrite output `MANIFEST.sha256` is not a row of itself. The host writes these files after that rewrite, so they are not rows: `linux/manifest/refresh-console.log`, `linux/manifest/refresh-hostconfig.txt`, `linux/manifest/refresh.exit`, and `linux/manifest/refresh-removed.txt`.

## Clarification

The proof in `linux/preoverlay` and `linux/overlay` is unchanged. Author failures under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a301/SP04/cdc-apply-pg` stay there. CDC-D04's frozen card leaves ACK, XACK, TRIM, and a control mirror to a later child, and it leaves a 500000-event segmented apply outside this writer. CDC-D05 is that later control-mirror child and is **WITHHELD** here. Their absence is not an unmet requirement of the accepted CDC-D04 scope.

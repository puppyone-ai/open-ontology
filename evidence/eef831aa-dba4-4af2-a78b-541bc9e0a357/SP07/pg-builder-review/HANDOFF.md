# Handoff

Verdict: **WITHHOLD**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a357`. Consumer `a13b1cc80ed66c07199a41d47b31fd42530f2be9`. Producer `c23db28dc27f5cf20741dd36e8a8e88129ad1715`, a348 `APPROVE_LIMITED` for the producer seam only. Decisive Linux run `linux/proof-4`, container id `1f4ae724378772f0243cebf63ec50d9beffa59e4ab2ad51806a10a7d577ec358`, `docker-run.exit` 0. One live grok-4.7 call in `live-one/`, generate exit 0, dataset `shop.account`.

`parent_acceptance` is false. This file does not accept E47. It does not accept SP07. It does not accept S0–S10 as a complete parent. It does not accept public authentication, semantic publication, or a release. It does not add a countable unit. It does not edit the ledger.

The exact gap: per-table role packs drop the other table's approved overlay. The shelf review records `SHELF_ID_BODY_a357`, and the later bin pack omits it. The sealed party sentence is in the party role file and absent from the account role file. No shipped command writes one combined final pack that keeps both. See `FINDINGS.md`.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `overlay/a357_consumer.rs`
- `linux/proof-4/console.log`, `linux/proof-4/test-a357.log`, `linux/proof-4/phases/`
- `linux/failed-attempts/launch-128/ATTEMPT.txt`
- `linux/proof-1/test-a357.log`, `linux/proof-2/test-a357.log`, `linux/proof-3/clippy-oracle.log`
- `live-one/summary.json`, `live-one/inputs.sha256`, `live-one/call-posture.txt`, `live-one/bearer-posture.txt`
- `origin/recipes.txt`, `linux/proof-4/archive-blobs.txt`, `CLEANUP.txt`, `MANIFEST.sha256`

## Decisive exits

`linux/proof-4/docker-run.exit` 0. Phase exits 0 except `lock-subset-builder.exit` 1 and `lock-subset-fixture.exit` 1. Those two lines are `lock_subset: pg-builder 0.1.0 source is not registry: none` and the same for `pg-fixture`. Locks were not replaced. `root-lock.exit` 0. `product-bytes.exit` 0. `product-diff.txt` is empty. `cargo-update-invoked.txt` is `no`.

Author counts before the oracle existed: pg-builder 12 passed, pg-fixture 2 passed, product Clippy 0, fixture Clippy 0. Oracle: 6 passed in 8.73s. Oracle Clippy 0. Names: `cache_hit_opens_no_socket_and_stale_scopes_miss`, `forged_echo_and_failure_leave_curated_and_cache_unchanged`, `rebind_copies_descriptive_bytes_and_replaces_prompt_echoes`, `separate_packs_drop_the_other_tables_approved_text`, `stored_three_table_receipts_keep_hashes_and_split_the_pack`, `unreviewed_mark_unknown_column_and_deferred_prose`.

Memory `6442450944`, NanoCpus `1000000000`, PidsLimit 256, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a357-eef831aa-net`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Rust 1.94.0 aarch64-unknown-linux-gnu inside that container. PostgreSQL was not started. `capacity.txt` recorded `own=0 global=0 host_cargo=0` before start.

Preserved non-zero runs: launch-128 exit 128 before a container; proof-1 and proof-2 `docker-run.exit` 101 with oracle test failures; proof-3 `docker-run.exit` 101 with oracle tests passed and Clippy failed. Consoles were not overwritten.

## Live call

`live-one/summary.json`: outcome `validated`, cache `miss` then `stored`, HTTP 200, socket `opened`, attempt 1, model `grok-4.7`, origin `http://127.0.0.1:8317`. Usage 2473 / 3054 / 5527. Declared cost `unknown`, amount null. `input_sha256` `681fe64efbb169b1473c5354db37181584000d6ce55afae0b890d3a2d29b2973`. `candidate_sha256` `1daee66e41275eb71d0787f678f03b4b523412a15cac622bc5c175050c93d141`. Request hash `53d2cb58d8289d6b3bbfc0d87ec5ba1bba65fc5fe239c16a31a2cb339ec3f3ac` matches the sealed ready pass-1 account batch. The new candidate was not reviewed and was not packed. Host toolchain for this binary is Homebrew rustc 1.98.1. The Linux 1.94 smoke is proof-4. Bearer length 6, not printed. `model_calls=1`.

## Owned path

The commit for this review contains only `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a357/SP07/pg-builder-review/`. Product bytes at `a13b1cc80ed66c07199a41d47b31fd42530f2be9` were compared after the oracle file was removed. `CLEANUP.txt` records removal of the private network, the two volumes, and the private tmp tree. `MANIFEST.sha256` is the std-only manifest of this directory and is excluded from its own listing.

A later repair of the pack gap needs its own UUID, its own evidence, and a new independent review. This directory is closed as WITHHOLD.

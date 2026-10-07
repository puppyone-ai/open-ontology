# CDC-D05 / E40 / SP04-W05 control mirror

## Scope and verdict

`tools/cdc-offset-store` reads an existing target apply receipt and records it in a second database. The control transaction stores the verified receipt, then compare-and-swaps a contiguous watermark only when the declared transaction is complete, the position is bound, and the caller-supplied previous and next transactions match. A partial segment, a hole, or an epoch barrier still stores the receipt and leaves the watermark where it is.

The crate does not acknowledge a source, trim a stream, or certify `source_applied` or `fence_proof`. Those output flags stay false. `checkpoint_provenance` is `caller_declared_pending_real_normalizer`.

This report does not claim parent acceptance, SP04-W05 closure, AT-PL-05, or a release. Source acknowledgement, Valkey `XACK` and `XTRIM`, the reject ledger, large-segment integration, and the moving CDC writer remain parent work.

Fixed commit `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc`, tree `7e7dd9a20deb39db87d84595a6b4cd6bc77e7f70`. The product proof archives that commit and overlays only `tools/cdc-offset-store`. The root lock in the archive stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. No root manifest or root lock was edited by this task. `cargo update` was not invoked. The subset check reported `registry=128 path=1 missing=0 mismatch=0`. The child lock is `e8c67af05bcdcf5641b03209e72c47abec5628fa88fd3eafc03e3c5ecb8b9465` and is `tools/cdc-offset-store/Cargo.lock`.

Contract: `tools/cdc-offset-store/CONTRACT.md`, name `cdc-offset-store-v1`.

## Recipes

Origins, paths, and the hashes measured in this session are in `recipe-origin.txt`. Launch limits are 6 GiB and 1 CPU, `cap-drop ALL`, no host network, no host home. PostgreSQL is one private container, no host port. `linux/tools/sha256_manifest.rs` matches the a199 blob `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`.

## Pre-edit smoke

`linux/precode` ran on the pinned Linux arm64 Rust 1.94 image before this overlay. `smoke.exit` is 0. The console records `proof=ok tests=43`. `tools/cdc-transaction/Cargo.lock` stayed `4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a`. `cargo-update-invoked.txt` is `no`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Container `oo-a310-precode` was removed by that launch.

## Decisive verification

`linux/impl-5` is the passing product run. Earlier attempts are preserved and are not overwritten.

- `linux/impl-1`: library compile failed. `transaction()` needs `&mut Client`, two `?` operators could not convert a PostgreSQL error into `()`, and `PROVENANCE` was unused. `smoke.exit` 101. The database was not written.
- `linux/impl-2`: contract tests passed (11). `live_chain` panicked at `adopt_target_generation` with `WrongType { postgres: Uuid, rust: "&str" }`. Clippy reported `collapsible_if`, `result_unit_err`, and `manual_is_multiple_of`. `smoke.exit` 101. Roles and both databases had been created.
- `linux/impl-3`: `live_chain` passed in 2.41s. Clippy failed on `tests/live.rs` (`too_many_arguments`, `collapsible_if`). `smoke.exit` 101.
- `linux/impl-4`: `live_chain` passed in 2.36s. Clippy failed on `tests/contract.rs` (`too_many_arguments`, `cloned_ref_to_slice_refs`). `smoke.exit` 101.

After impl-2, impl-3, and impl-4, only this task's PostgreSQL container and volume `oo-a310-eef831aa-pgdata` were removed and created again. The notes are `pg/recreate-after-impl-2.txt`, `pg/recreate-after-impl-3.txt`, and `pg/recreate-after-impl-4.txt`. Foreign containers were not touched. First-start inspect files are under `pg/attempt-1`, `pg/attempt-2`, and `pg/attempt-3`.

`linux/impl-5` then ran on a fresh cluster. `phases/*.exit` are all 0. `smoke.exit` 0. `docker-run.exit` 0. OOMKilled false. Privileged false. Memory 6442450944. NanoCpus 1000000000. CapDrop ALL. Network `oo-a310-eef831aa-net`. The passing container was `oo-a310-impl-5`, removed by the launch script. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `cargo-update-invoked.txt` is `no`.

From the impl-5 logs:

- `tests/contract.rs`: 11 passed, 0 failed
- `statements_keep_dollar_blocks`: passed
- `live_chain`: passed in 2.52s
- `cargo clippy --locked --offline --all-targets -- -D warnings`: exit 0

`live_chain` asserts, in one private PostgreSQL 17.10 container, two databases over TLS: migration drift before receipt version 3, least-privilege `42501` across schemas, a missing receipt, a fenced `commit_apply_receipt` for two tables, replay, `OO010` on a changed payload, a partial segment that does not move the watermark, a later transaction that does not max across the hole, an epoch barrier, a backend kill before commit with one retry, a hidden commit reply that resolves by lookup, a lookup outage that returns `commit_unknown` and does not insert twice, an immutable receipt hash, and install drift. The fixture calls `_oo.adopt_target_generation` and `_oo.commit_apply_receipt`. The product only selects the stored receipt.

`linux/restart` restarted the postmaster and the test passed, then the script exited 1 because `restart_counts_ok` is an `eprintln` and Cargo omitted it from `test.log`. `linux/restart-2` is the passing rerun. Postmaster start changed from `2026-10-07 10:41:38.911979+00` to `2026-10-07 10:42:35.229878+00`. `restart_counts` passed in 0.17s and printed `restart_counts_ok`. Row counts matched the impl-5 file. Replaying txn-1 did not move the watermark. `smoke.exit` 0. `docker-run.exit` 0. Same 6 GiB and 1 CPU limits. Container `oo-a310-restart-2`, removed by the launch script.

## Images

Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, not pulled. PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64, not pulled.

Copies of the product bytes are under `source/blobs/tools/cdc-offset-store/`. `MANIFEST.sha256` is the Rust inventory of this directory and does not include itself. `linux/manifest/console.log`, `hostconfig.txt`, `docker-run.exit`, and `container-removed.txt` are written after that inventory.

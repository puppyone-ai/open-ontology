# CDC-D05 independent review

## Scope and verdict

This review reads `tools/cdc-offset-store` at fixed commit `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`, tree `539aa555a721a0b87b40546b9d4dad5569e3c863`. The author evidence under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a310/SP04/cdc-offset-store/` was read and left unchanged. Product sources were not edited. `git diff` of `tools/cdc-offset-store` and of target migrations `0000` through `0003` against that commit is empty. `source/product-vs-fixed.exit` and `source/core-vs-fixed.exit` are both 0.

The crate mirrors an existing target apply receipt into a second database and compare-and-swaps a contiguous watermark. The independent probe created receipts with `_oo.adopt_target_generation` and `_oo.commit_apply_receipt`, then called the product CLI. The mirror request carries no receipt body. Preoverlay author checks passed before that probe was added to the archive.

Parent SP04, full source acknowledgement, fenced segments, and the reject ledger stay open. This review does not claim parent acceptance, SP04-W05 closure, AT-PL-05, or a release.

## Recipes

Origins and measured hashes are in `recipe-origin.txt`. Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, Rust 1.94.0, not pulled. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 17.10, not pulled. Builders are 6 GiB and 1 CPU. `cargo update` was not invoked. Root lock stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Child lock `e8c67af05bcdcf5641b03209e72c47abec5628fa88fd3eafc03e3c5ecb8b9465`. Subset line `registry=128 path=1 missing=0 mismatch=0 names=true`.

## Preoverlay

`linux/preoverlay` ran the author's 13 checks and Clippy on the fixed archive before `review_a321.rs` existed in that archive. `smoke.exit` 0. `docker-run.exit` 0. Console ends `proof=ok tests=13`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `cargo-update-invoked.txt` is `no`.

- `tests/contract.rs`: 11 passed, 0.01s
- `statements_keep_dollar_blocks`: passed
- `live_chain`: passed in 2.33s
- `cargo clippy --locked --offline --all-targets -- -D warnings`: exit 0

Host config: OOMKilled false, Privileged false, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000, network `oo-a321-eef831aa-net`. Container `060b79383f62f364722333403e18231b5382c8e88de67c8c1fda3832a1f54dd4`, removed by the launch. After that run, only `oo-a321-pg` and volume `oo-a321-eef831aa-pgdata` were removed and created again so the independent probe did not inherit the author fixture roles. The note is `pg/recreate-after-preoverlay.txt`. First-cluster inspect files are in `pg/attempt-1`.

## Independent probe

`linux/overlay-attempt-1` is the preserved compile failure. `tests/review_a321.rs` called `CertificateDer::from_pem_slice` without `PemObject` in scope (`E0599`). `smoke.exit` 101. `test.exit` 101. `clippy.exit` 101. The database was not written. Registry subset had already passed. The import was added in the harness only.

`linux/overlay` is the passing probe. `independent_chain` passed in 2.67s. Clippy exit 0. `smoke.exit` 0. `docker-run.exit` 0. Same 6 GiB and 1 CPU limits. Container `2872cddf390ed01abe49ca126ee94b6253aa93ffc6201e186b2c1f437c2f4b15`, removed by the launch. Root lock unchanged. `registry=128 path=1 missing=0 mismatch=0 names=true`.

`linux/overlay/independent-counts.json` records the committed end state: 10 mirrored receipts, 2 watermark rows, epoch 1 at `txn-after` position 40, epoch 2 at `txn-e2` position 100, `race_fault` `receipt_hash_conflict`, `linearizable_multi_actor` false, and `source_applied`, `ack_allowed`, `external_xack`, and `external_xtrim` false. The txn-1 receipt hash is `95537a00f6685831ce38aaf0b72a3bc6a3ba3f08d1c12328dbc31403cd9fa5c2`.

The passing assertions cover install and install replay, migration drift before `0003`, runtime `42501` on a cross-schema read and on creating a table in `oo_cdc_offset`, a missing target receipt, fenced `commit_apply_receipt` after `adopt_target_generation`, rejection of an unfenced insert, exact replay, `OO010` on a changed payload, a partial segment that leaves the watermark, a next-transaction hole that does not take the max position, an epoch barrier that does not move the older epoch, caller `source_gap` and `epoch_barrier` flags that hold the watermark and stay false as source certificates, a concurrent equal-hash insert that returns `receipt_hash_conflict` on unique violation `23505`, a hidden COMMIT resolved by lookup, `CDC_OFFSET_RECONCILE=down` returning `commit_unknown` without a second insert, and install drift. `0002` is present and not applied. `0004` is absent. The product sources contain no Valkey command names.

## Restart

`linux/restart-attempt-1` restarted the postmaster from `2026-10-07 11:19:35.381771+00` to `2026-10-07 11:26:22.742032+00`. Receipt and watermark counts matched the overlay file. Replaying txn-1 with an empty `expected_previous` returned kind `replay`, the same hash, `checkpoint_advanced` false, and `watermark_hold` `expected_previous_mismatch`. The harness expected a null hold. `smoke.exit` 101. That is a harness assertion. The product was not edited.

`linux/restart-attempt-2` restarted the postmaster from `2026-10-07 11:26:22.742032+00` to `2026-10-07 11:29:08.144051+00`. `independent_restart` passed in 0.16s. Clippy failed because the fresh container had not installed the clippy component. `smoke.exit` 1.

`linux/restart` is the passing persistence run. Postmaster start changed from `2026-10-07 11:29:08.144051+00` to `2026-10-07 11:30:11.128223+00`. `independent_restart` passed in 0.17s. The stored head `orders/txn-after` replayed with the same receipt hash, `checkpoint_advanced` false, and a null hold (`already_checkpoint`). Receipt count stayed 10 and both epoch watermarks stayed at the recorded values. Clippy exit 0 after `rustup component add clippy`. `smoke.exit` 0. `docker-run.exit` 0. Memory 6442450944. NanoCpus 1000000000. OOMKilled false. Privileged false. CapDrop ALL. Container `08c63aae3044900aed3844edee166c0ab64de63f2d2be377ce4c04b5d453f9a7`, removed by the launch.

## Inventory

`linux/tools/sha256_manifest.rs` is the a199 std-only tool. It checks the empty digest and `abc`, then writes `MANIFEST.sha256` for every regular file except that manifest. `CLEANUP.txt` is written before that inventory. `linux/manifest/console.log`, `hostconfig.txt`, `docker-run.exit`, and `container-removed.txt` are written after it.

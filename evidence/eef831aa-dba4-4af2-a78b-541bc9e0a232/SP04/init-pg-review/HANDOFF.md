# Handoff

Verdict: APPROVE_LIMITED for commit `6e1e6e576d1afd2bde10dedad649f09dee95bd34`.

The commit adds:

- `crates/oo-pipeline/src/bin/init-pg.rs`
- `crates/oo-pipeline/tests/init_pg_cli.rs`
- `contracts/pipeline/init-pg-cli-v1.md`

Tree `7f387aeea34cfc374940bf454767f3a49b167be9`. Parent `d71a17d68769306bb55a9b377c2aefb948da55c9`. `init-pg` is Cargo's autobin. It is not a subcommand of `oo-pipeline`. Stdin protocol is `oo.pipeline.init-pg/v1`.

Rebuild from that commit. `rust-toolchain.toml` names `1.98.1`. The Linux runs set `RUSTUP_TOOLCHAIN=1.94.0`. Do not treat the dirty workspace HEAD as the archive.

Smoke, before the review harness: `linux/smoke/docker-run.exit` 0, offline `init_pg_cli` 0 (`4 passed; 0 failed; 1 ignored`), lock unchanged. Clippy was deferred. Proof: `linux/proof/docker-run.exit` 0, review test 0, clippy 0, `live/outcome.txt` `init_pg_review_ok`. The first proof is preserved under `linux/proof/attempt-1/` and is a harness failure, not a product failure.

The review oracle is `overlay/init_pg_a232_review.rs`. It is not product source. It checks migration checksums and replay, runtime non-membership, wrong CA, nonadmin, a second empty database on the same cluster, partial schema preservation, and a bad checksum. One private PostgreSQL 17.10 cluster hosted those databases.

`init-pg` does not publish and does not grant `oo_target_owner` to `oo_pipeline_runtime`. The harness made that grant before `import-pg`. Import state `awaiting_publish` is not publication. The partial schema that `oo_target_owner` cannot use remains exit 1 and was not expanded here.

This evidence does not accept M1-D05A, SP04-W05, semantic publication, authentication, WAL, or a release.

# analyze-pg operator command

Session `eef831aa-dba4-4af2-a78b-541bc9e0a236`. Fixed archive `a48db85fc13148874349d318affd995f7212188d`, tree `fd464408028e135a08fe1eaaafdeefb7ea9cccd1`. Product commit `6ac2216505723b51ae116b1938aca47846ade35f` on the three owned paths. Workspace HEAD at the start of the evidence notes was `56c506b8b061783873634486cbc39a3888b0e6bc`. This is the M1-D06B operator command evidence. It does not accept M1-D06B, does not close a parent checkbox, and does not release.

a207 remains the only writer of the connector manifest and the root lock. This batch did not edit `Cargo.toml`, `Cargo.lock`, `lib.rs`, the installer, the import library, or an existing CLI. `cargo update` was not run. `analyze-pg` is Cargo's autobin. `oo-pipeline.rs` was not edited.

## What ran

Smoke, before the overlay, on the fixed archive: `linux/smoke/docker-run.exit` is 0, `lib.exit` 0, `cli.exit` 0, `fetch.exit` 0, `lock-unchanged.exit` 0, `overlay-absent.exit` 0. Offline fetch exited 101 and the locked online fetch exited 0. `rustc` 1.94.0 linux/arm64. `clippy.exit` is the text `deferred_until_overlay`. The three analyze-pg paths were absent. Base file hashes are `linux/smoke/archive-sha256.txt`.

Library smoke: 2 passed. CLI smoke: `import_pg_cli_live` 3 passed and 1 ignored, `init_pg_cli` 4 passed and 1 ignored, `target_statistics_pg_live` 1 ignored. No live database test ran in smoke.

Proof mounted that archive plus `crates/oo-pipeline/src/bin/analyze-pg.rs`, `crates/oo-pipeline/tests/analyze_pg_cli.rs`, `contracts/pipeline/analyze-pg-cli-v1.md`, and the evidence catalog harness copied only to `crates/oo-gateway/tests/analyze_pg_catalog_proof.rs` inside the proof tree. `linux/proof/base-match.exit` is 0. `lock-unchanged.exit` is 0. `cargo-update-invoked.txt` is `no`. `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

The deciding proof container was `oo-a236-proof`, id `2ae195e2ee4b7ddebdd62377c2be6efbc87217357cdff7e5b9e0cf1dd9fa382b`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `--pull never`, linux/arm64, 6442450944 bytes, 1 CPU, `CapDrop=["ALL"]`, network `oo-a236-eef831aa-net`, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=1g`. Source was mounted read-only. Builder secrets were mounted read-only at `/run/analyze-secrets` and held `admin.pass`, `ca.crt`, `wrong-ca.crt`, and `login.pass`. Private keys stayed under `/tmp` and were not mounted. `OOMKilled=false`. `RUSTUP_TOOLCHAIN=1.94.0`. `CARGO_BUILD_JOBS=1`.

Private PostgreSQL was `oo-a236-pg`, id `da49f8ecde0e943aae0fa29022c29e3c58a4df2f554f9635bac1ba0c4ac4a693`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 17.10, linux/arm64, 1g, 1 CPU, no host port. Server certificate SAN is `DNS:oo-a236-pg`. `up.exit` is 0.

## Command results

Deciding proof phases, all 0: `test`, `clippy`, `live`, `libclang`, `catalog`, `secret-scan`, `lock-unchanged`, `docker-run`.

`cargo test -p oo-pipeline --test analyze_pg_cli` without `--ignored`: 4 passed, 1 ignored.

`cargo clippy -p oo-pipeline --bin analyze-pg --test analyze_pg_cli -- -D warnings`: exit 0. The vendor `sqlx-postgres` crate printed two unfulfilled `dead_code` expectations. Those warnings are outside this crate.

Ignored live test `analyze_pg_binary_on_private_postgres`: 1 passed in 2.26s. Chain: compiled `init-pg`, compiled `import-pg`, unknown `reltuples` `-1`, compiled `analyze-pg`, known estimate. `facts.txt`:

- import `awaiting_publish`, 2 rows, audit before import 2
- `reltuples_before=-1`, `sentinel_before=-1`
- refusals `wrong_ca,runtime,role,stale_generation,mapping_absent`, then `reltuples_after_refusal=-1`
- `published=false`, `estimate_is_exact=false`
- `reltuples_after_bits=1073741824`, `estimated_rows=2`, `count_rows=2`, `connection=reusable`
- `sentinel_after=-1`, `maintenance=explicit`
- `source_audit_delta=0`, `source_reader_writes=0`, `source_reltuples=-1`

`1073741824` is the bit pattern of `2.0f32`. `count(*)` was 2. The receipt still has `estimate_is_exact` false. The live test required a second `analyze-pg` call to stay unpublished with the same bits.

Catalog harness, evidence only: `policy_catalog_query_after_analyze_pg` passed. `UnknownRows::Refuse`, `policy_catalog=loaded`, `query_status=succeeded`, `query_count=2`, `statistics_published=false`. Libclang extract exit 0, `stdbool.h=present`.

`secret-scan.exit` is 0. Password file text is absent from `facts.txt` and `catalog-facts.txt`.

## Failed attempts

`linux/proof/attempt-1/` exited 101. `OOMKilled=false`. Unit tests passed. Clippy rejected `Harness::expect_analyze` for eight arguments. The live count query was decoded as `TEXT`.

`linux/proof/attempt-2/` exited 101. Clippy exited 0. Import exited 1 with `oo-pipeline: source privilege registration_refused` because the source audit function was `SECURITY DEFINER`.

The deciding run is the third proof, stored at `linux/proof/`. The line-tables-only retry did not run.

## Owned paths

Product commit `6ac2216505723b51ae116b1938aca47846ade35f`:

- `crates/oo-pipeline/src/bin/analyze-pg.rs` `ebdb96007ab8534dedee09f39ac5744f77dd58e8b4f6c78fe714c1729e9156f4`
- `crates/oo-pipeline/tests/analyze_pg_cli.rs` `047f0225019da3852b0bbc8e6ad3ff1820f4bd34ea4c7f28caec4bffb81f41c4`
- `contracts/pipeline/analyze-pg-cli-v1.md` `2364dd4f6ee66fa6f0565ad5cc624bae79460f7b46024bb7900d387c063c35c5`

Those hashes are `linux/proof/overlay-sha256.txt` and match the workspace files committed. Blobs are `source/blobs/`. The catalog harness hash in that file is `2aad525b524ca3115219689ef0a266f31e76ad657b6ffd1e90f7379603c527a1`. It is not in the product commit.

## Inventory

`SHA256SUMS` lists this evidence directory after cleanup. It was produced by `tools/sha256_manifest.rs`, which self-checks the empty digest and the `abc` digest. The manifest file is the tool output and is not one of its rows. `SHA256SUMS.sha256` is `tools/sha256.rs file` of that manifest. Both tools are the reviewed Safe Rust oracles. The inventory compile used the host `rustc` after the proof container and the private database were gone. It is not a product build and it did not use the workspace `Cargo.lock`.

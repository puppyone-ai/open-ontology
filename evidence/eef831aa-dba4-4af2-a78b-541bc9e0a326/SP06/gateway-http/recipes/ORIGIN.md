# Recipe origin

Development `eef831aa-dba4-4af2-a78b-541bc9e0a326`. The smoke launcher follows the reviewed a319 preoverlay launcher, which itself records the a204 launcher. The inside script follows the a199 phase style (identity, then locked cargo, raw exits). The a200 smoke launcher is the other reviewed smoke shape (archive blob check, overlay-absent, capacity gate).

| Source | Commit | Path | SHA-256 |
|---|---|---|---|
| a204 | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a199 | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/preoverlay/inside.sh` | `4523876ae9f6faa0fc719583cac356ca1b8bf466fe7121b9a39297640604067c` |
| a200 | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |
| a204 | same review commit as the launcher | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |
| a319 launcher used as the immediate shape | `e8d297f95b32052ea587fe8ad67b6e8493a00300` tree contains the a319 evidence | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a319/SP00/gateway-dependencies-review/linux/preoverlay/launch.sh` | `5dede18986dbf7341e9d7ae132665a274c2de52d8f0a74ebf5562f7791e37b4f` |

Changes from the a319 launcher: owner `a326`, session `eef831aa-dba4-4af2-a78b-541bc9e0a326`, fixed commit `e8d297f95b32052ea587fe8ad67b6e8493a00300`, network `oo-a326-eef831aa-net`, volumes `oo-a326-eef831aa-cargo` and `oo-a326-eef831aa-target`, container `oo-a326-smoke`, task `gateway-http`, memory `6g`, `--cpus 1`. The archive is only `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, and `crates`. No probe mount, no blob mount, no Postgres. The inside script checks rustc 1.94.0, refuses `http_gateway.rs` / `http_error.rs` / `build.rs`, then `cargo check --locked -p oo-arch-check` and `cargo check --locked -p oo-gateway`. `/tmp` stays `noexec`. `TMPDIR=/target/tmp`. `MAKEFLAGS=-j1`.

Attempt 1 stopped in `pg_query` bindgen: the pinned image has no libclang, and `cap-drop ALL` cannot `apt-get`. Attempt 2 reuses the a200 extractor and the two Rust tools below, copied byte-for-byte. The only extractor edits are the a326 cargo paths and `/evidence` instead of `/evidence/linux/smoke`. The gcc include directory is the one directory under `/usr/lib/gcc/aarch64-linux-gnu/*/include` rather than a hardcoded `14`.

| Source | Commit | Path | SHA-256 |
|---|---|---|---|
| a200 | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| a200 | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a200 | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/extract-libclang.sh` | `dc0d980692091bec55d4fbc3eb12a0eda9c5aaa4b9f7d5c6eab995b2a726690d` |

Smoke attempt 2 exited 0 (`linux/smoke-2/`, container `2d40c51c1afa`, `inside_ok=2026-10-07T12:09:32Z`). Later builders keep that launcher shape: 6 GiB, 1 CPU, `cap-drop ALL`, private network, `TMPDIR=/target/tmp`, and `CARGO_BUILD_JOBS=1`. The lock oracle is `tools/lock-consumer.rs`, adapted from a313 `tools/lockdiff.rs` (`58041b895fd19549bf0da35439f9e201217c6b429c9c6a4cf4ecac629c050f08`). The immutable proof archives commit `51d08e59ddb3c92fc0b9f2958966b56e19032b7f` with no product overlay.

# Recipe origin

Independent review `eef831aa-dba4-4af2-a78b-541bc9e0a319`. The launch and hash tools come from the reviewed a204, a199, and a200 files. `lockdiff.rs` is the byte-identical author oracle from the fixed archive, checked again by `tools/crate_verify.rs`.

| Source | Commit | Path | SHA-256 |
|---|---|---|---|
| a204 | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a199 | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/preoverlay/inside.sh` | `4523876ae9f6faa0fc719583cac356ca1b8bf466fe7121b9a39297640604067c` |
| a200 | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |
| a204 | same review commit as the launcher | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |
| fixed archive | `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a313/SP00/gateway-dependencies/tools/lockdiff.rs` | `58041b895fd19549bf0da35439f9e201217c6b429c9c6a4cf4ecac629c050f08` |

`tools/sha256_manifest.rs` and `tools/lockdiff.rs` are unmodified copies. `tools/crate_verify.rs` is new and is the review oracle for live metadata, license files, the five added triples, and the httpdate feature edge.

Changes from the a204 launcher: owner `a319`, UUID-private network and volumes, memory `6g`, `--cpus 1`, `CARGO_BUILD_JOBS=1`, global rust-builder cap 10, no Postgres, `RUSTUP_TOOLCHAIN=1.94.0`, pinned image digest unchanged. The archive is fixed commit `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96` and includes the frozen probe. The pre-overlay body runs scoped `oo-arch-check` before the archived probe's four loopback tests and Clippy. No product overlay is applied. `/tmp` stays `noexec`; `TMPDIR` is `/target/tmp` so rustup can install Clippy. `crate_verify` reads both multiline and one-line feature arrays because hyper-util 0.1.20 writes `http1` and `server` on one line. The edge gate ignores the probe package itself and still requires exactly the five registry triples.

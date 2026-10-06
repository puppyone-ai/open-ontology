# Recipe origin

Session `eef831aa-dba4-4af2-a78b-541bc9e0a242`. Fixed archive `4b15249b70ee38d5a4d21c546b1329e6532ccfed`. Review only. No product, library, root manifest, or lock edits.

| Origin | Commit | Path | SHA-256 |
| --- | --- | --- | --- |
| a204 launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a204 inside | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| a199 manifest hasher | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |
| a200 pg wrap | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/pg-wrap.sh` | `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f` |
| a200 pg up | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh` | `96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45` |
| a200 libclang extract | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/extract-libclang.sh` | `dc0d980692091bec55d4fbc3eb12a0eda9c5aaa4b9f7d5c6eab995b2a726690d` |
| a200 sha256 tool | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a200 select-debs | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |

`tools/sha256_manifest.rs`, `tools/sha256.rs`, `tools/select-debs.rs`, and `pg/pg-wrap.sh` are byte copies of those origins. The same four hashes were recomputed on the copies.

`linux/proof/launch.sh` keeps the a204 shape: `git archive` of the fixed commit, private bridge, private cargo and target volumes, capacity gate, `--pull never`, `--cap-drop ALL`, `HOME=/tmp`, no prune. Changes: this UUID, 6 GiB, one CPU, global builder cap 10, smoke-exit gate, private PostgreSQL secret mount, writable harness mount, and a refusal to overwrite `console.log`.

`pg/up.sh` keeps the a200 private TLS PostgreSQL shape. Changes: this UUID, SAN and alias `oo-a198-pg` because the frozen query-pg live test dials that name, alias `oo-a242-pg` for the review request, `other.crt` for that test's invalid-CA case, and a refusal to overwrite `up.exit`. The image is not pulled.

`linux/proof/extract-libclang.sh` keeps the a200 extract. Changes: cargo paths `a242-clang-root` and `a242-debs`, and evidence directory `libclang`. `linux/proof/inside.sh` compiles `sha256.rs` before the extract, because `select-debs` does not implement `sha256 file`.

The repository `rust-toolchain.toml` at the fixed commit asks for 1.98.1. The archive copy is renamed before cargo runs so rustup stays on image toolchain 1.94.0. Product bytes are not edited.

Attempt 1 showed the image has no rustfmt component. That check is not part of the a200 recipe and is not a product verdict. `linux/proof/inside.sh` now installs Clippy with `rustup component add clippy --toolchain 1.94.0` when it is absent, matching a200 `linux/smoke/inside.sh`. The attempt 1 outputs stay under `linux/proof/attempts/1/`.

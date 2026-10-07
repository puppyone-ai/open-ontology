# Recipe origin

Review `eef831aa-dba4-4af2-a78b-541bc9e0a337`. Fixed product commit `51d08e59ddb3c92fc0b9f2958966b56e19032b7f`. The launch shape is the a326 proof launcher, which records the a319 launcher, which records the a204 launcher. The inside phase order follows a199. Libclang extraction follows the a200 extractor as adapted by a326. Postgres follows the a326 `pg/up.sh` shape.

| Source | Commit | Path | SHA-256 |
|---|---|---|---|
| a204 | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a199 | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/preoverlay/inside.sh` | `4523876ae9f6faa0fc719583cac356ca1b8bf466fe7121b9a39297640604067c` |
| a200 | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |
| a200 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/extract-libclang.sh` | `dc0d980692091bec55d4fbc3eb12a0eda9c5aaa4b9f7d5c6eab995b2a726690d` |
| a200 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a200 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| a204 | same review commit as the launcher | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |
| a319 | `e8d297f95b32052ea587fe8ad67b6e8493a00300` tree contains that evidence | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a319/SP00/gateway-dependencies-review/linux/preoverlay/launch.sh` | `5dede18986dbf7341e9d7ae132665a274c2de52d8f0a74ebf5562f7791e37b4f` |
| a326 | `51d08e59ddb3c92fc0b9f2958966b56e19032b7f` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/proof-3/launch.sh` | `636e8f1b7eff70adaee4fb5e11f897ff87804ddcb81c210581b7c93f578bff9f` |
| a326 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg/up.sh` | `3a403d479fb79e11c53a0b6d5c7a9cd0a5b453564bac8063207933e70812a92c` |
| a326 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/lock-consumer.rs` | `65724acb0df0f12cd07a1a7d2767cb3309069a753b28793a2f6332074f1cadc2` |

`tools/sha256.rs`, `tools/select-debs.rs`, `tools/sha256_manifest.rs`, and `tools/lock-consumer.rs` are unmodified copies. `scope_review.rs` and `review_http.rs` are new. `extract-libclang.sh` changes the a326 cargo directory names from `a326` to `a337`. `pg/up.sh` changes the owner, database, SAN, and network alias to `a337`. `launch.sh` changes the owner, memory is already 6 GiB, CPU is already 1, and the archive commit stays `51d08e59`. The inside script keeps the a199 phase lines, runs the 42 library tests, the 8 loopback tests, and Clippy on the read-only archive, and only then copies the tree to add `review_http.rs`.

`linux/preoverlay/inside.sh` called `cargo fetch --locked -p oo-gateway -p oo-arch-check`. Cargo 1.94 rejected `-p` (`fetch.exit=1`). `linux/preoverlay-2/inside.sh` and `linux/preoverlay-3/inside.sh` use `cargo fetch --manifest-path /src/Cargo.toml --locked`, the same form as the a204 and a200 fetch lines. `review_http.rs` then collapsed one nested `if` after Clippy `collapsible_if` in `linux/preoverlay-2/`. The failed test source is `linux/preoverlay-2/review_http.failed.rs`.

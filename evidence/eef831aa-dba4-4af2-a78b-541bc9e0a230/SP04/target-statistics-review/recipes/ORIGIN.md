# Recipe origins

Review of product commit `613aa981dc2c57c0005cc6589e1a8ee7757f2a69`, parent `5af5db7e9bc27a32bacb4d617bd2393c8edc2cb6`, tree `b3ca121831767fc2b717836889ef25d1e1a719a2`. Author evidence commit `02b84f4bf731f6a24dfc6aece454d7e875d86938`.

| Origin | Commit | Path | SHA-256 |
|---|---|---|---|
| a204 preoverlay launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a204 preoverlay inside | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| a204 overlay launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/launch.sh` | `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee` |
| a200 postgres up | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh` | `96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45` |
| a200 pg-wrap | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/pg-wrap.sh` | `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f` |
| a200 libclang extract | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/extract-libclang.sh` | `dc0d980692091bec55d4fbc3eb12a0eda9c5aaa4b9f7d5c6eab995b2a726690d` |
| a200 cleanup | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/cleanup/run.sh` | `1943ba8e5e1d128b96b3c25cf281a2386f83167ec6901fecc0110340dfaf2ff6` |
| a200 `sha256.rs` | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a200 `select-debs.rs` | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| a199 `sha256_manifest.rs` | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

`pg-wrap.sh`, `sha256.rs`, `select-debs.rs`, and `sha256_manifest.rs` are byte-identical to those origins. `extract-libclang.sh` keeps the a200 body. Its cargo and evidence paths say `a230` and `/evidence/libclang`.

Changes from the launch scripts: owner `a230`, session `eef831aa-dba4-4af2-a78b-541bc9e0a230`, fixed commit `613aa981dc2c57c0005cc6589e1a8ee7757f2a69` (the product is already in the archive), builder memory 6 GiB, `--cpus 1`, global builder cap 10, one private Postgres, task-private cargo, target, network, CA, home, and tmp. The builder mount has the CA and password files and does not have `server.key` or `ca.key`. The pre-overlay smoke expects `target_statistics.rs` and refuses the review harness. The proof copies only the two review tests into a throwaway archive. No host network, no privileged mode, no shared cache, no prune.

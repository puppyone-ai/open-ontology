# Recipe origins

Fixed archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`, tree recorded in `source/fixed-tree.txt`. Saved before the pre-edit smoke.

| Origin | Path | SHA-256 |
|---|---|---|
| a204 `b2b3adab` launch shape | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a204 preoverlay inside | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| a204 overlay-3 launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/launch.sh` | `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee` |
| a200 PG up | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh` | `96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45` |
| a200 pg-wrap | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/pg-wrap.sh` | `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f` |
| a200 libclang extract | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/extract-libclang.sh` | `dc0d980692091bec55d4fbc3eb12a0eda9c5aaa4b9f7d5c6eab995b2a726690d` |
| a200 smoke inside | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/inside.sh` | `b56d83ddbf95ecb0a33ab5028f2f99c562d9f6e51d47075c2ee7584ac315a0eb` |

| a200 `sha256.rs` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a200 `select-debs.rs` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| a199 `sha256_manifest.rs` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

Copied `sha256.rs`, `select-debs.rs`, and `sha256_manifest.rs` unchanged. `extract-libclang.sh` keeps the a200 body. Its cargo and evidence paths say `a219` and `/evidence/libclang`. That copy's digest is `ac1d802986d25de11dab80627db4b0e5cc8af9cfa04d2bfe96dcef0edb0cd0ad`.

Changes from those scripts: owner `a219`, session `eef831aa-dba4-4af2-a78b-541bc9e0a219`, fixed commit `c2db9c430b822a824a1062b6c3c7345924df7dd8`, builder memory 6 GiB, `--cpus 1`, global builder cap 10, task-private cargo, target, network, CA, home, and tmp. The builder mount has the CA and password files and does not have `server.key` or `ca.key`. No host network, no privileged mode, no shared cache, no prune.

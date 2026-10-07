# Recipe origin

Actor a350. Content hashes are SHA-256 of the files as reused. `pg/pg-wrap.sh` in this directory matches the a326 file byte for byte.

| Source | Commit | Path | SHA-256 |
|---|---|---|---|
| a326 | `2908c705994c2d1fc590d17ab712808e37824764` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg/pg-wrap.sh` | `238d2d9828dab0e2f8402cc935adf324890b4802aeba842679bd3473e49ebbf6` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg/up.sh` | `3a403d479fb79e11c53a0b6d5c7a9cd0a5b453564bac8063207933e70812a92c` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg/init-roles.sh` | `a84c4fda4ae913e894f460f6fbaabf767c7ba174e738b0701be05c5379491f94` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/smoke-2/extract-libclang.sh` | `1c2f68d652f667eee5d861ec11bb0ee5654ade6908826e3f6dcb4c7e1c1f3024` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/smoke-2/launch.sh` | `8ae3e0b124f83575ccb1989094b2982a5ee9be7059f4d80042101d8056783ff7` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/proof-3/inside.sh` | `f14f35f6a78b50cd92a88517c678736e7b1d147ada38862a96057176d78a3d10` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/lock-consumer.rs` | `65724acb0df0f12cd07a1a7d2767cb3309069a753b28793a2f6332074f1cadc2` |
| a341 | working tree of the reviewed repair | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-live-repair/linux/smoke/inside.sh` | `44b05354fdbc7f9853ea9a6f0be5878d2d149f8c7f9140154e9c1482906dd846` |
| a341 | working tree of the reviewed repair | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-live-repair/linux/smoke/launch.sh` | `6eec60671ed7c0999d7d2d10d9fe72665a464f9655c75972b40b6e95500af238` |

`up.sh` changes the owner, database, network alias, and SAN. The container limits, the five capabilities, the TLS server files, and `log_statement=none` stay. `init-roles.sh` changes `oo_a326` to `oo_a350`. `extract-libclang.sh` is mounted read-only and is not edited. The proof launcher follows the a326 smoke launcher: private network, 6 GiB, 1 CPU, `cap-drop ALL`, `TMPDIR=/target/tmp`, no host network, no host home, no shared cargo cache.

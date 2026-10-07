# Recipe origin

Review `eef831aa-dba4-4af2-a78b-541bc9e0a360`. Fixed product commit `97b6a9eb9caa2b6059d82bdb98b703bf5ee202d8`. The archive is `git archive` of that commit. The work tree is not copied over it.

`pg/pg-wrap.sh` is a byte copy of the a326 wrapper. `pg/up.sh` and `pg/init-roles.sh` keep that recipe and change the database to `oo_a360`, the alias to `oo-a360-pg`, and the SAN to `DNS:oo-a360-pg`. The capability set stays `cap-drop ALL`, then `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, `SETGID`.

| Source | Commit | Path | SHA-256 |
|---|---|---|---|
| a326 | `2908c705994c2d1fc590d17ab712808e37824764` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg/pg-wrap.sh` | `238d2d9828dab0e2f8402cc935adf324890b4802aeba842679bd3473e49ebbf6` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg/up.sh` | `3a403d479fb79e11c53a0b6d5c7a9cd0a5b453564bac8063207933e70812a92c` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg/init-roles.sh` | `a84c4fda4ae913e894f460f6fbaabf767c7ba174e738b0701be05c5379491f94` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/smoke-2/extract-libclang.sh` | `1c2f68d652f667eee5d861ec11bb0ee5654ade6908826e3f6dcb4c7e1c1f3024` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/smoke-2/launch.sh` | `8ae3e0b124f83575ccb1989094b2982a5ee9be7059f4d80042101d8056783ff7` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| a326 | same tree | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/lock-consumer.rs` | `65724acb0df0f12cd07a1a7d2767cb3309069a753b28793a2f6332074f1cadc2` |

`extract-libclang.sh`, `sha256.rs`, and `select-debs.rs` are mounted read-only and are not edited. `tools/review_tls.rs` is the independent oracle. It is copied into the container work tree only after `cargo check --locked` of the fresh archive.

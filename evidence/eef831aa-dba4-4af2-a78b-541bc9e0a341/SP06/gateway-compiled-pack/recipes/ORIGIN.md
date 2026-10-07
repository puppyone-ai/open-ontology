# Proof recipe origin

Development `eef831aa-dba4-4af2-a78b-541bc9e0a341`. Fixed archive `cd8f754c1b4db22fc962370c4b29bb846b0fccc3`. The launcher follows a326 `linux/proof-3/launch.sh`. The inside phase order follows a326 `linux/proof/inside.sh` and the a199 style recorded there: identity, then locked cargo, raw exits. Libclang extraction follows a326 `linux/smoke-2/extract-libclang.sh`, which records the a200 extractor. Postgres follows a326 `pg/up.sh`. a337 `recipes/ORIGIN.md` is the review that limited the previous catalog to operator metadata.

| Source | Commit that stored it | Path | SHA-256 |
|---|---|---|---|
| a326 | `51d08e59ddb3c92fc0b9f2958966b56e19032b7f` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/proof-3/launch.sh` | `636e8f1b7eff70adaee4fb5e11f897ff87804ddcb81c210581b7c93f578bff9f` |
| a326 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/proof/inside.sh` | `f14f35f6a78b50cd92a88517c678736e7b1d147ada38862a96057176d78a3d10` |
| a326 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg/up.sh` | `3a403d479fb79e11c53a0b6d5c7a9cd0a5b453564bac8063207933e70812a92c` |
| a326 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg/pg-wrap.sh` | `238d2d9828dab0e2f8402cc935adf324890b4802aeba842679bd3473e49ebbf6` |
| a326 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/pg/init-roles.sh` | `a84c4fda4ae913e894f460f6fbaabf767c7ba174e738b0701be05c5379491f94` |
| a326 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/smoke-2/extract-libclang.sh` | `1c2f68d652f667eee5d861ec11bb0ee5654ade6908826e3f6dcb4c7e1c1f3024` |
| a326 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a326 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| a337 | `cd8f754c1b4db22fc962370c4b29bb846b0fccc3` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a337/SP06/gateway-http-review/recipes/ORIGIN.md` | `47f42cc4439271a40b825705006ca55e82d4f8b98cd9037c3c64f599d6f01724` |

`tools/sha256.rs` and `tools/select-debs.rs` are not copied. The builder bind-mounts the a326 directory read-only. `pg-wrap.sh` keeps the same postgres arguments. `extract-libclang.sh` changes only the cargo directory names from `a326` to `a341`. `init-roles.sh` changes the database name to `oo_a341`. `up.sh` changes the owner, database, SAN, and network alias to `a341`. `launch.sh` changes the owner, archive commit, task name, and overlay mounts. Memory stays 6 GiB and CPU stays 1. The inside script adds a fetch because the a341 cargo volume starts empty, runs the focused `pack_catalog` and loopback smoke on the read-only archive before the overlay, then builds `role-pack-bundle` and the new live test from the work tree. It does not run the a326 lock-consumer because this task does not change `Cargo.lock`.

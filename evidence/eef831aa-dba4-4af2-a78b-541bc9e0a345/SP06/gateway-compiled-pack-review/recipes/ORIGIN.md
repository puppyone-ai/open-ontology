# Recipe origin

Review `eef831aa-dba4-4af2-a78b-541bc9e0a345`. Fixed product commit `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c`. The launcher follows a341 `linux/smoke-3/launch.sh`. The inside phase order follows that same smoke: identity, locked fetch, archive tests, then a private file. Libclang extraction follows a341 `linux/smoke-3/extract-libclang.sh`. Postgres follows a341 `pg/up.sh`. a341 `recipes/ORIGIN.md` records the a326 and a337 recipe chain.

| Source | Commit that stored it | Path | SHA-256 |
|---|---|---|---|
| a341 | `b3ebb97e13112cb19183c01b9f544ea1ad3994e1` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-compiled-pack/linux/smoke-3/launch.sh` | `28362260279f7e036866bc31f3fd5848371375f7e194bfc3ffa9866bb1a4bef5` |
| a341 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-compiled-pack/linux/smoke-3/inside.sh` | `d31453f4ef521f206ac3a931bd8d20ea5e215162ffd76e79066a5bfaf9ead0e3` |
| a341 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-compiled-pack/linux/smoke-3/extract-libclang.sh` | `90a05dd88284e19ec7f34ef2f3280e37a8972e39027ab8cfab248faba2b51666` |
| a341 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-compiled-pack/pg/up.sh` | `413472f33cbb1c150da826023408b7da1e541d644aa077a54b96fd292e411654` |
| a341 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-compiled-pack/pg/pg-wrap.sh` | `eda1fb4698a9699bfcc2fe581bcb12bf727715708d1e89ef7f13a22b870e08f8` |
| a341 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-compiled-pack/pg/init-roles.sh` | `de6973fa11522f486f544716a77aa1158ce390f1a879274dd62b4ad47238621a` |
| a341 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-compiled-pack/recipes/ORIGIN.md` | `82ba9649398b9dcb7c548ac6f347b25fce51f1cfc966f82a331d994402354322` |
| a326 | `3f4a358ab1502215e956cdfe1f02ba899b06a8ca` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a326 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| a337 | `cd8f754c1b4db22fc962370c4b29bb846b0fccc3` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a337/SP06/gateway-http-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

`tools/sha256.rs` and `tools/select-debs.rs` are not copied. The builder bind-mounts the a326 directory read-only. `tools/sha256_manifest.rs` is an unmodified copy used after the builder exits. `pg-wrap.sh` is an unmodified copy of the a341 file. `extract-libclang.sh` changes cargo directory names from `a341` to `a345`. `init-roles.sh` changes the database name to `oo_a345`. `up.sh` changes the owner, database, SAN, network alias, and evidence directory to this review. `launch.sh` changes the owner, the archive commit to `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c`, and drops the product overlay mounts. Memory stays 6 GiB and CPU stays 1. The inside script runs the fixed-archive pack, loopback, and Clippy checks before it copies `review_pack.rs` into a private work tree.

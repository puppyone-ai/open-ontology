# Recipe origin

Review `eef831aa-dba4-4af2-a78b-541bc9e0a345`. Fixed archive `99e93d0f88eb95d8b3834c534dc8ae90d62753ec`. No product overlay.

| Source | Commit that stored it | Path | SHA-256 |
|---|---|---|---|
| a341 repair | `829aef90251726264a1bdd6c666dacf2270a8806` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-live-repair/linux/smoke/launch.sh` | `6eec60671ed7c0999d7d2d10d9fe72665a464f9655c75972b40b6e95500af238` |
| a341 repair | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-live-repair/linux/smoke/inside.sh` | `44b05354fdbc7f9853ea9a6f0be5878d2d149f8c7f9140154e9c1482906dd846` |
| a341 repair | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-live-repair/linux/smoke/extract-libclang.sh` | `90a05dd88284e19ec7f34ef2f3280e37a8972e39027ab8cfab248faba2b51666` |
| a341 repair | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-live-repair/pg/up.sh` | `a552a74d99452ae0546d2e98f080852e60f9bde1b6d76ee0d71411327b083a3a` |
| a341 repair | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-live-repair/pg/pg-wrap.sh` | `eda1fb4698a9699bfcc2fe581bcb12bf727715708d1e89ef7f13a22b870e08f8` |
| a341 repair | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-live-repair/pg/init-roles.sh` | `de6973fa11522f486f544716a77aa1158ce390f1a879274dd62b4ad47238621a` |
| a326 | `3f4a358ab1502215e956cdfe1f02ba899b06a8ca` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a326 | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |

`extract-libclang.sh` and `pg-wrap.sh` are byte-identical copies. `init-roles.sh` changes the database name to `oo_a345`. `up.sh` changes the owner, database, SAN, network alias, and evidence directory. `launch.sh` archives `99e93d0f` including `testing/real-db/gateway-compiled-pack` and does not mount a workspace overlay. `inside.sh` checks the fixed-archive hashes, leaves the default test ignored, runs focused Clippy, then runs only `OO_A326_LIVE=1 --ignored`. It does not run `http_gateway_pg_live`. Memory stays 6 GiB and 1 CPU for the builder, and 1 GiB and 1 CPU for Postgres. a326 tools are bind-mounted, not copied.

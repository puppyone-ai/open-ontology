# Recipe origin

Session `eef831aa-dba4-4af2-a78b-541bc9e0a244`. Fixed archive `8f533140a2823568c5f375190f8708934cd413fa`, tree `94005c335ef758877b545fee4f4f258123e4e41e`.

The hashes below were computed on the host with `shasum -a 256` so each copied recipe can be named. They are not this review's inventory. The inventory is `tools/sha256.rs` compiled inside the pinned image by `linux/inventory/launch.sh`, then rewritten by `linux/inventory/rerun.sh`. `tools/sha256.rs` is a byte-for-byte copy of the a222 file. It is not the a199 `sha256_manifest.rs` blob. `cmp` also confirmed byte equality for `tools/select-debs.rs` and `pg/pg-wrap.sh`.

| Origin | Commit | Path | SHA-256 |
| --- | --- | --- | --- |
| a204 launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a200 smoke launch | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |
| a199 manifest hasher | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |
| a222 proof launch | `aa2e33e45555fa78f2411164e50d6ab1815036dc` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/linux/proof/launch.sh` | `e1d6556a8c227ee942f502beb7c1efdfcacd11ed271ee1e81a15608064146463` |
| a222 extract-libclang | `aa2e33e45555fa78f2411164e50d6ab1815036dc` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/linux/proof/extract-libclang.sh` | `6a39526820f55f7f9508848dc8a08f571c91b2813561601db9e3cede0ef778e0` |
| a222 pg up | `aa2e33e45555fa78f2411164e50d6ab1815036dc` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/pg/up.sh` | `b874e0a9616d5262e8745e9d0aa3dee93ecab26b321cfed53ad1df60146cd0ac` |
| a222 pg wrap | `aa2e33e45555fa78f2411164e50d6ab1815036dc` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/pg/pg-wrap.sh` | `d707d4e64c060e78245a941719495cf8865783754d953f41d9c503e549cc11d9` |
| a222 sha256 tool | `aa2e33e45555fa78f2411164e50d6ab1815036dc` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a222 select-debs | `aa2e33e45555fa78f2411164e50d6ab1815036dc` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| a225 inventory launch | `934c8098703b6462c4f774878d1a4241bd84905d` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a225/SP12/trial-statistics/linux/inventory/launch.sh` | `f7807489580a7ffd55e1bbf9d8c042bdeb1d69264ff8bca4eda72097cc515e80` |
| a231 inventory launch | `a62037195fb18304f6cc4a8586e03f193bfa4d09` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/linux/inventory/launch.sh` | `c4944b72c811ca3f11f093346f07f57f13e1e08b78dd21d47e221901db9b0c23` |
| a231 inventory rerun | `a62037195fb18304f6cc4a8586e03f193bfa4d09` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/linux/inventory/rerun.sh` | `ba5637536019ed0de8b70d985088bc98177367b9556656d2a39a8bc196d49a88` |
| a231 proof launch | `a62037195fb18304f6cc4a8586e03f193bfa4d09` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/linux/proof/launch.sh` | `c76a0f7e46288d01aba0a88696ef995a609ff61b66ff69f910fdb0f3ed57ecb7` |
| a231 proof inside | `a62037195fb18304f6cc4a8586e03f193bfa4d09` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/linux/proof/inside.sh` | `41e7d720f5fe5287c294db48eb92bcd144c5e6b6ce043cd12a95e45bb5c5ac85` |
| a231 extract-libclang | `a62037195fb18304f6cc4a8586e03f193bfa4d09` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/linux/proof/extract-libclang.sh` | `df7d5c7a5a3dc107dd3d4d411794474a8045d8e1946afaafce3d7005e9665508` |
| a231 smoke launch | `a62037195fb18304f6cc4a8586e03f193bfa4d09` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/linux/smoke/launch.sh` | `4912706f31c76358487f59a06bf29e26c77d5d3f3c78e894f1729e08b2f598fa` |
| a231 pg up | `a62037195fb18304f6cc4a8586e03f193bfa4d09` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/pg/up.sh` | `8f3909b6792bf0dc537e42a1b246856486fe4db945ac1725ff46ea764283ee75` |

a204 is the fixed-archive launch shape: `git archive`, a private bridge, private cargo and target volumes, a capacity gate, `--pull never`, `--cap-drop ALL`, `HOME=/tmp`, phase exits, and inspect after exit. a231 used that shape at 6 GiB and 1 CPU. This review keeps 6 GiB and 1 CPU. a200 is the private TLS Postgres shape. This review's database is `oo-a244-pg` on `oo-a244-eef831aa-net`, with the alias `oo-a198-pg` so the frozen fixture host resolves.

The proof launcher started from the a231 proof launch, which records the a222 proof launch. The smoke launcher started from the a231 smoke launch. Inventory started from the a231 inventory launch, which records the a225 inventory launch. Deltas: owner `a244`, fixed commit `8f533140` with `benchmarks/sql-runner` already in the archive, no worktree overlay, a peek suite, `backends-after-runner` captured before admin release, and an evidence-only hash. The live workspace is later than `8f533140`, so the inventory does not mount it.

| This review | SHA-256 | Delta |
| --- | --- | --- |
| `linux/smoke/launch.sh` | `0a648cb9424771214ea98d3d9fa334053b76ff855a79adde00d08c0d21ba2286` | a231 smoke, owner a244, runner already in the archive |
| `linux/smoke/inside.sh` | `f474e466872ce0ffbd1c5908b125a1d7562497d8ce8f0d88e9b2a668c3f44f5a` | sealed sql-runner tests on 1.94.0 |
| `linux/proof/launch.sh` | `0af3808e31aacc1de2340f28674d7052aee4f8c2694973058df4682707532a7e` | a231 proof launch, owner a244, 6 GiB, 1 CPU |
| `linux/proof/inside.sh` | `a5b679004dbf5bc8478156239fadff5574cb8b5e80a5bec2594ca8b0b091265d` | five suites, occupy, child scan, no overlay |
| `linux/proof/extract-libclang.sh` | `052027a36e5c8bdd1fa21450830549284483238ad09e0250dee1318062f8bf88` | a231 extract, a244 paths |
| `linux/inventory/launch.sh` | `acf653a2749a4fc1635463961b97b45c8550f85b7ba5c75a1e03c3415275576d` | a231 inventory, evidence root only |
| `linux/inventory/rerun.sh` | `9e45dff39ae53cd13541f5a9c97e5735bd8dc9fa5d7469e76c0108e9b76796fa` | second pass outside the tree |
| `pg/up.sh` | `0ea496774b924d104a8419400eadb05403e58ca9372f7825ceb352e58b257e03` | a231 up, a244 names, alias `oo-a198-pg` |
| `pg/pg-wrap.sh` | `d707d4e64c060e78245a941719495cf8865783754d953f41d9c503e549cc11d9` | byte copy of a222 |
| `tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` | byte copy of a222 |
| `tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` | byte copy of a222 |

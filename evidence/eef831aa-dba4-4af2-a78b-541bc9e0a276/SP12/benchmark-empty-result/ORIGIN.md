# Recipe origin

Session `eef831aa-dba4-4af2-a78b-541bc9e0a276`. Fixed archive `8a6395aef5b1d3b16d1b3d72833d2df2ebaa536e`. `linux/live/fixed-ancestor.exit` is 0.

`tools/sha256_manifest.rs` is a byte copy of the a199 file. `cmp` exited 0. Host `shasum -a 256` of both copies is `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. The evidence inventory itself is that tool, compiled in the pinned image by `linux/manifest/launch.sh`. Host `shasum` in the table below is the recipe record in `linux/live/origin-recipes.txt`, not the inventory.

| Recipe | Path | SHA-256 |
| --- | --- | --- |
| a199 manifest tool | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |
| a200 Postgres up.sh | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh` | `96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45` |
| a200 pg-wrap.sh | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/pg-wrap.sh` | `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f` |
| a222 sha256 tool | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a222 select-debs | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| a244 extract-libclang | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a244/SP12/sql-runner-review/linux/proof/extract-libclang.sh` | `052027a36e5c8bdd1fa21450830549284483238ad09e0250dee1318062f8bf88` |
| this session post launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/linux/post/launch.sh` | `eb4ae3432bca63304fc8a702457689e8cc1affa5e2f356a522cecea4945f33e5` |

`pg/pg-wrap.sh` matches the a200 file byte for byte. `tools/sha256.rs` and `tools/select-debs.rs` match the a222 files byte for byte. `pg/up.sh` changes the owner, container name, network, volume, and certificate name to a276. `linux/live/extract-libclang.sh` changes the cargo-volume paths to a276.

Image pins: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` and `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Builder memory is 6442450944 and NanoCpus is 1000000000. Postgres memory in `pg/up.sh` stays 1g, 1 CPU, with the reviewed capability subset `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, and `SETGID` after `--cap-drop ALL`.

The smoke and post launches follow the a276 post script, which follows the fixed-archive shape used in this session: `git archive` of `8a6395ae`, private network `oo-a276-eef831aa-net`, private cargo and target volumes, `--pull never`, `--cap-drop ALL`, and `HOME=/tmp`. The live launch adds the secret mount, the libclang tools, and no product overlay, so the adapter binary remains the post build.

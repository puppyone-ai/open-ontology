# Recipe origins

Fixed archive `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`.
Copied bytes were checked with `shasum -a 256` before any edit. Unchanged files keep the origin digest.

| File | Origin | SHA-256 | Change |
| --- | --- | --- | --- |
| `tools/sha256.rs` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` | none |
| `tools/sha256_manifest.rs` | same review, `tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` | none |
| `tools/select-debs.rs` | same review, `tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` | none |
| `linux/extract-libclang.sh` | `.../a274/.../linux/chain/extract-libclang.sh` `982f18e430e6b8136a4cb30cb7e0711015318d0b40c0939475bfae8142e55ed4` | rewritten paths | a305 cargo and evidence paths only |
| `pg/pg-wrap.sh` | a274 `pg/pg-wrap.sh`, which records a254 `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f` | rewritten comment | server flags unchanged |
| `pg/up.sh` | a274 `pg/up.sh`, which records a261 `03d9488ad5aa9a3cff7bb00e248ae80a85183d562165db29197df1463595bc92` | rewritten | UUID `a305`, alias `oo-a305-pg`, reuse of an existing owned server |
| `linux/launch.sh` | a204 overlay-3 launch `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee`; a199 overlay-3 launch `c837e9d18e1ae9fa9ae90d03f920d645f9b30d235a2384e630aa0a9c9782b69a`; a200 proof launch `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`; a274 chain launch | rewritten | 6 GiB, 1 CPU, cap 10, this UUID |
| `fixture/source.sql` | a274 `fixture/source.sql` | role names | `oo_a261_reader` / `oo_a261_writer` become `oo_a305_reader` / `oo_a305_writer`. Rows, comments, and grants otherwise match |

The chain called by the driver is `pg-onboarding inspect` and `pg-onboarding apply-reviewed` from that archive, with the child binaries named by `tools/pg-onboarding/tests/live_chain.rs`.

## Identity delta

Pre-edit bytes were hashed by `deploy/trusted-demo/tools/sha256.rs` inside the pinned 1.94 container. The record is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a312/SP11/trusted-demo-identity/linux/pre-edit-smoke-attempt-1/work/origin-sha256.txt`. Unchanged tool hashes remain the rows above. `pg/pg-wrap.sh` stays `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`.

| File | Pre-edit SHA-256 | Change |
| --- | --- | --- |
| `CONTRACT.md` | `0b12693fc1903988b4fbcf618dab3214de9f8d4da3d3b5d35808823d2b323364` | manifest rules before the script change |
| `README.md` | `8e28046e1524a96a8bcfc68d819dd04d3b046fec303c516f194028d711496389` | operator commands take `--manifest` |
| `start.sh` | `19621892999a94a5a6ade482bb0895a04b624199dfdf3de9937047b40f1579b4` | require the manifest on every mode |
| `linux/launch.sh` | `699fbadf6f8a4a0780d46c1779d48fd817720b52db8c9420134218bd0e7dfd49` | names and labels from the manifest; create refuses collisions |
| `linux/inside.sh` | `616bacaebe61938dfec96d219c95312d04673c420271c7c8e226dfbe57ed85a0` | neutral cargo paths; read `/deployment.json` |
| `linux/stop.sh` | `3e1c2a1ceb50eeb4f2e2d446fd5cd330f076139bb156b3a3a88ec59aca469210` | four-label filter |
| `linux/clean.sh` | `17e1a8429fed5b20af3baa68b203146ca96fa9e71347b19ea4f7ff6fdc10c875` | same filter; no prune |
| `linux/extract-libclang.sh` | `2210dc5e235d56e37a5ee544b4b2cd98b64a86de45816479b97eeb35b8efab5f` | `demo-clang-root` instead of an owner-specific cache path |
| `pg/up.sh` | `8e6b48fe1695f72bbd7cf8deee2a0ecf2f5643eafa73af6185565a09facf7333` | `create` refuses existing objects; `reuse` requires matching labels |
| `fixture/source.sql` | `59b34e6c40ad268aff81dd8e00624a70d16a9f596df5d5feb1aaccdee3e3dd92` | grants use psql identifier variables; rows unchanged |
| `fixture/load.sh` | `89694a24f5d83efd1301b98f73aebaa2af8a75e11f21bc1985eed81fe34adacf` | repo path from the script; owner passed as an env token |
| `fixture/inside-cluster.sh` | `0d229701c820f38fe05d48b7a038580129b59ca4d91ebf3143391ee6df23489f` | role names from the validated owner |
| `fixture/db-present.sh` | `33ebcae9520ffb41967b3ffd66b71dd5640378bc9c5b38719736b2be8f15b23d` | same |
| `fixture/grant-runtime.sh` | `cd48068a34bc16a770037fa32613f2cf013fe2fd9fe2166f0bb911f8fefba911` | same |
| `fixture/membership-watch.sh` | `1887a8847d98de2e19f6b7eb7ccb51a356a38d97187e1ac63a24b12467d992b9` | refuse foreign container labels |
| `src/drive.rs` | `8f285740cdde091fc23917bbb58981bd6bf7e875a71c30018c0c683e6bdc999b` | resource fields come from the manifest |

`linux/manifest.sh` is new. It is the shared canonical-manifest check. The driver checks the same grammar.

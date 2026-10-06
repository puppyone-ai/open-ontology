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

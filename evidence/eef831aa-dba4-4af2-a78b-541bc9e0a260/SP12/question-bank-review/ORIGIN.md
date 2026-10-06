# Recipe origin

Session `eef831aa-dba4-4af2-a78b-541bc9e0a260`. Fixed archive `c275aa6326ae945ef09534bd6fb5b7f2a5eb938b`, tree `e021e83a56354c380582765349152f2024b9f5ad`.

The hashes below were computed on the host with `shasum -a 256` so each copied recipe can be named. They are not this review's inventory. The inventory is `tools/sha256_manifest.rs` compiled inside the pinned image by `linux/inventory/launch.sh`, then rewritten by `linux/inventory/rerun.sh`.

| Origin | Commit | Path | SHA-256 |
| --- | --- | --- | --- |
| a204 launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a204 inside | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| a200 smoke launch | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |
| a199 manifest hasher | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

a204 is the fixed-archive launch shape: `git archive`, a private bridge, private cargo and target volumes, a capacity gate, `--pull never`, `--cap-drop ALL`, `HOME=/tmp`, phase exits, and inspect after exit. This review uses 6 GiB and 1 CPU. a200 is the private TLS Postgres recipe. It was not copied. This review starts no Postgres container and claims no private TLS session. a199 supplies the hasher. `tools/sha256_manifest.rs` is a byte copy of that file. Both hashes are `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. No line was edited.

Deltas from those recipes: owner `a260`, fixed commit `c275aa63`, the question bank already in the archive, no worktree overlay, one FIPS smoke test, and an independent `A260` driver mounted only at `/review`. `linux/proof-2/` is the same proof recipe with a new evidence directory after the first driver stopped on a missing key. The live workspace is later than `c275aa63`, so the inventory does not mount it.

| This review | SHA-256 | Delta |
| --- | --- | --- |
| `linux/smoke/launch.sh` | `85ff682f261a3c260cf3891ce07774bea624cd10bfa3f914dbd05dc714613113` | a204 shape, owner a260, 6 GiB, 1 CPU, question bank already in the archive |
| `linux/smoke/inside.sh` | `0ededb0d0a9022d942e5ca3969ceb896785aa677787c03b2c1279778525de86b` | FIPS test only, toolchain renamed |
| `linux/proof/launch.sh` | `6da6f52490a30d81167dc58f148ddc7888ea82a777e2319b589d0be1b885ccdf` | same archive, harness mount, blob copy, tree diff |
| `linux/proof/inside.sh` | `587c374de917b8af2a0d6ecfbe2b9b0f6ba691f4100acfefbf7c9cca6d35a456` | locked offline build, sealed tests, fmt, clippy, independent drive |
| `linux/proof-2/launch.sh` | `d6e31f129973b1efccd2707ff76e60240cda6b0f2c9d061815ebe7de0e4c2218` | new directory and container name after the first driver stop |
| `linux/proof-2/inside.sh` | `587c374de917b8af2a0d6ecfbe2b9b0f6ba691f4100acfefbf7c9cca6d35a456` | byte-identical to `linux/proof/inside.sh` |
| `linux/inventory/launch.sh` | `1d6b3bf98595781aadce45af6946f04ce860b96b7fe05946a48054fe7acf8244` | a199 hasher, evidence root only, binary at `/sha256_manifest` |
| `linux/inventory/rerun.sh` | `dc4339d01a11549dfd97f4c4e0ebeab08b84fb2ab9ad72e24b2fa9594a6f7345` | second pass, stdout outside the tree |
| `tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` | byte copy of a199 |
| `tools/drive.rs` | `23c31b222d71874f7087886582994ce8d67c4aa876b393e13a15de6d54b6af51` | independent Safe Rust oracle; final-key insert added after proof 1 |

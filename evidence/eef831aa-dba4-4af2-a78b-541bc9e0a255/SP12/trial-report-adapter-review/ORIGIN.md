# Recipe origin

Session `eef831aa-dba4-4af2-a78b-541bc9e0a255`. Fixed archive `8e7925d7f4576cb6a9f4027eca66ab1671e3fd1a`, tree `f45bc72e39a4adcbd2fe6392db80fb9db8a67ad1`.

The hashes below were computed on the host with `shasum -a 256` so each copied recipe can be named. They are not this review's inventory. The inventory is `tools/sha256_manifest.rs` compiled inside the pinned image by `linux/inventory/launch.sh`, then rewritten by `linux/inventory/rerun.sh`.

| Origin | Commit | Path | SHA-256 |
| --- | --- | --- | --- |
| a204 launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a200 smoke launch | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |
| a199 manifest hasher | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

a204 is the fixed-archive launch shape: `git archive`, a private bridge, private cargo and target volumes, a capacity gate, `--pull never`, `--cap-drop ALL`, `HOME=/tmp`, phase exits, and inspect after exit. This review uses 6 GiB and 1 CPU. a200 is the private TLS Postgres recipe. It was not copied. This review starts no Postgres container and claims no private TLS session. a199 supplies the hasher. `tools/sha256_manifest.rs` is a byte copy of that file. Both hashes are `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. No line was edited.

Deltas from those recipes: owner `a255`, fixed commit `8e7925d7`, the adapter already in the archive, no worktree overlay, fetch limited to the three benchmark manifests, and an independent `A255` driver mounted only at `/review`. The live workspace is later than `8e7925d7`, so the inventory does not mount it.

| This review | SHA-256 | Delta |
| --- | --- | --- |
| `linux/smoke/launch.sh` | `1d9d43601d454d2dcfc8105b7d799f654cd79b518da0097c01e6bd8c1864de62` | a204 shape, owner a255, 6 GiB, 1 CPU, adapter already in the archive |
| `linux/smoke/inside.sh` | `fa2b026f52e65a7f56f83c31ffe3b3f8b722fbe27fe3700b722fbc880c1d0d5e` | bounds only, three-manifest fetch, toolchain renamed |
| `linux/proof/launch.sh` | `53992ff618a41c823abda99d5fd268be4a3539c8dfeb8f41c6ade25f3b98ee30` | same archive, harness mount, blob copy, tree diff |
| `linux/proof/inside.sh` | `ecacbd46e74735e39495dc9b28a02d6d06521e1e2170ea47a6bd76dc578576f2` | locked offline builds, sealed tests, fmt, clippy, independent drive |
| `linux/inventory/launch.sh` | `c3c2ee9ecdfdbb5620a57ccb3171a1c26638bcbdc8d3915dd22f7d58878a83b8` | a199 hasher, evidence root only, binary at `/sha256_manifest` |
| `linux/inventory/rerun.sh` | `c00ab7c67592cdb2e23c6c90b8be62e2d2a8768a29d3ccc1f069e4c0cee137fd` | second pass, stdout outside the tree |
| `tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` | byte copy of a199 |
| `tools/drive.rs` | `b2e11795f97632dafb07cc34a2ae2e81da7eaa9cdee94c3742fdb9a3cba30624` | independent Safe Rust oracle |
| `tools/stub.rs` | `b95265a263539c845eff848a6fdab7ec5b497af9cc6c14c8ca6c8e8c6e726049` | purpose-built subprocess fixture |

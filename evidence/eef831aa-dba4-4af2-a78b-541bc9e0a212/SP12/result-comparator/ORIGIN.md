# Recipe origin

Session `eef831aa-dba4-4af2-a78b-541bc9e0a212`. Fixed product commit `a661dcdf72ff91561ae6b7cb79916e582884d08a`.

The smoke launcher copies the a204 pre-overlay shape: `git archive` of the fixed commit, a private bridge network, private cargo and target volumes, a capacity gate, `--pull never`, `--cap-drop ALL`, `HOME=/tmp`, offline fetch then one online fetch, phase exit files, and inspect after the container exits. It does not prune and it does not remove another owner's containers.

| Origin | Commit | Path | SHA-256 |
| --- | --- | --- | --- |
| a204 launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a204 inside | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| a199 manifest hasher | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |
| a200 smoke launch | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |

Changes from the a204 launcher: owner `a212`, memory 6 GiB, one CPU, global builder cap 10, no review overlay, and the smoke target is `oo-core` decimal tests. The repository `rust-toolchain.toml` at the fixed commit asks for 1.98.1. The archive copy of that file is renamed before cargo runs so rustup stays on the image toolchain 1.94.0. The product bytes are not edited.

a200 is the private-network and TLS PostgreSQL recipe. This task uses no PostgreSQL, so the database, CA, and clang steps are not copied.

a199 supplies the Safe Rust manifest hasher used after the proof. The smoke does not hash with Python or with `shasum`. The file `tools/sha256_manifest.rs` is a byte copy of that origin. Both hashes are `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. No line was edited. It is compiled with `rustc --edition 2021` inside the pinned 1.94 image, self-checks the empty string and `abc`, and writes the inventories. It does not link `sha2` and it does not call a host hasher.

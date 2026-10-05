# Recipe origin

Session `eef831aa-dba4-4af2-a78b-541bc9e0a225`. Fixed product commit `c2db9c430b822a824a1062b6c3c7345924df7dd8`.

The smoke launcher copies the a212 smoke shape, which copies the a204 pre-overlay shape: `git archive` of the fixed commit, a private bridge network, private cargo and target volumes, a capacity gate, `--pull never`, `--cap-drop ALL`, `HOME=/tmp`, offline fetch then one online fetch, phase exit files, and inspect after the container exits. It does not prune and it does not remove another owner's containers.

| Origin | Commit | Path | SHA-256 |
| --- | --- | --- | --- |
| a212 smoke launch | `0ae42f942e075be4171646d373c49dcfe1029817` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a212/SP12/result-comparator/linux/smoke/launch.sh` | `9f7436ddd6b62baa6f5131bc49aae554b4f2df680fc7d966e3e0a34e283b9d00` |
| a212 smoke inside | `0ae42f942e075be4171646d373c49dcfe1029817` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a212/SP12/result-comparator/linux/smoke/inside.sh` | `1f3d708b6fcab08576e0f06ec62c18daf808e301dd8883130c199c978103efb8` |
| a212 impl-5 launch | `0ae42f942e075be4171646d373c49dcfe1029817` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a212/SP12/result-comparator/linux/impl-5/launch.sh` | `615851f79e8dd6bb0fa6c7c855bc994032a601892bc98753f787b0ea84bddcd5` |
| a212 impl-5 inside | `0ae42f942e075be4171646d373c49dcfe1029817` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a212/SP12/result-comparator/linux/impl-5/inside.sh` | `525ca23b801420a8c9cf4764f6fc8856d631cf2d00ccfee3ec10eb5886260fb6` |
| a212 inventory launch | `0ae42f942e075be4171646d373c49dcfe1029817` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a212/SP12/result-comparator/linux/inventory/launch.sh` | `118cdd532364b5a3b84f6a217fc2a12af5d29b7fa6323a74ecb9b0889cb638b0` |
| a204 launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a204 inside | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| a200 smoke launch | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |
| a199 manifest hasher | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

Changes from the a212 launcher: owner `a225`, fixed commit `c2db9c43`, crate `benchmarks/trial-statistics`, memory 6 GiB, one CPU, global builder cap 10, and the smoke target is the `oo-core` JSON number scanner. The repository `rust-toolchain.toml` at the fixed commit asks for 1.98.1. The archive copy of that file is renamed before cargo runs so rustup stays on the image toolchain 1.94.0. The product bytes are not edited.

a200 is the private-network and TLS PostgreSQL recipe. This task uses no PostgreSQL, so the database, CA, and clang steps are not copied.

a199 supplies the Safe Rust manifest hasher used after the proof. The smoke does not hash with Python or with `shasum`. The file `tools/sha256_manifest.rs` is a byte copy of that origin. Both hashes are `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. No line was edited. It is compiled with `rustc --edition 2021` inside the pinned 1.94 image, self-checks the empty string and `abc`, and writes the inventories. It does not link `sha2` and it does not call a host hasher.

`linux/inventory/launch.sh` is the a212 inventory launcher with the owner, crate path, network, and container name changed. It writes its console outside the evidence tree and copies that console in after the hasher exits. `linux/inventory/rerun.sh` is the second pass: after that console, hostconfig, and exit files are closed, it rewrites `crate-sha256.txt` and `SHA256SUMS.txt`. The second stdout stays at `/tmp/oo-a225-eef831aa-dba4-4af2-a78b-541bc9e0a225/hash-again.log` and is not copied into this directory.

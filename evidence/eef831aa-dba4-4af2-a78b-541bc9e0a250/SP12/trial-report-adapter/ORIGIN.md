# Recipe origin

Session `eef831aa-dba4-4af2-a78b-541bc9e0a250`. Fixed product commit `8f192847ea7c4906c13f8419b5de2669875f7ef9`.

The smoke launcher copies the a225 smoke shape, which copies the a204 pre-overlay shape: `git archive` of the fixed commit, a private bridge network, private cargo and target volumes, a capacity gate, `--pull never`, `--cap-drop ALL`, `HOME=/tmp`, offline fetch then one online fetch, phase exit files, and inspect after the container exits. It does not prune and it does not remove another owner's containers.

| Origin | Commit | Path | SHA-256 |
| --- | --- | --- | --- |
| a225 impl-4 launch | `934c8098703b6462c4f774878d1a4241bd84905d` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a225/SP12/trial-statistics/linux/impl-4/launch.sh` | `6c1e508ab64eb04408189f12f3617e30cea6e5c69453bc936550b10e6d28817a` |
| a225 impl-4 inside | `934c8098703b6462c4f774878d1a4241bd84905d` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a225/SP12/trial-statistics/linux/impl-4/inside.sh` | `de9e253803531594e5acb144a22ed37664e3fe9a55f39fe86ab1af2b7dc67f46` |
| a225 smoke launch | `c5f4019574541296e15628325738864d97e4a47f` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a225/SP12/trial-statistics/linux/smoke/launch.sh` | `722e6fee565c80b446eb0d5ade8a70947cdcb4a222010bc9eaaac9d1fdde7f9f` |
| a225 smoke inside | `c5f4019574541296e15628325738864d97e4a47f` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a225/SP12/trial-statistics/linux/smoke/inside.sh` | `28dc75be92b02bcec42f45a45966e06c7a059cd29deca901f55de7ddc0e9ce75` |
| a225 inventory launch | `934c8098703b6462c4f774878d1a4241bd84905d` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a225/SP12/trial-statistics/linux/inventory/launch.sh` | `f7807489580a7ffd55e1bbf9d8c042bdeb1d69264ff8bca4eda72097cc515e80` |
| a225 inventory rerun | `934c8098703b6462c4f774878d1a4241bd84905d` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a225/SP12/trial-statistics/linux/inventory/rerun.sh` | `dbe77f72407441627e5c6990c0a940134f475999e3e768488f9042e7889994f4` |
| a204 launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a199 manifest hasher | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |

Changes from the a225 launcher: owner `a250`, fixed commit `8f192847`, crate `benchmarks/trial-report-adapter`, memory 6 GiB, one CPU, global builder cap 10, and the smoke target is the `oo-core` JSON number scanner. The implementation archive also keeps `benchmarks/sql-runner` and `benchmarks/trial-statistics` so the live proof can compile those CLIs. The repository `rust-toolchain.toml` at the fixed commit asks for 1.98.1. The archive copy of that file is renamed before cargo runs so rustup stays on the image toolchain 1.94.0. The product bytes are not edited.

a200 is the private-network and TLS PostgreSQL recipe. This task uses no PostgreSQL, so the database, CA, and clang steps are not copied. This unit does not claim a private TLS session.

a199 supplies the Safe Rust manifest hasher used after the proof. The smoke does not hash with Python or with `shasum`. The file `tools/sha256_manifest.rs` is a byte copy of that origin. Both hashes are `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. No line was edited. It is compiled with `rustc --edition 2021` inside the pinned 1.94 image, self-checks the empty string and `abc`, and writes the inventories. It does not link `sha2` and it does not call a host hasher.

`linux/inventory/launch.sh` is the a225 inventory launcher with the owner, crate path, network, and container name changed. It writes its console outside the evidence tree and copies that console in after the hasher exits. `linux/inventory/rerun.sh` is the second pass: after that console, hostconfig, and exit files are closed, it rewrites `crate-sha256.txt` and `SHA256SUMS.txt`. The second stdout stays at `/tmp/oo-a250-eef831aa-dba4-4af2-a78b-541bc9e0a250/hash-again.log` and is not copied into this directory.

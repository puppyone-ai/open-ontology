# Recipe origin

Session `eef831aa-dba4-4af2-a78b-541bc9e0a267`. Fixed archive `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`. That commit is an ancestor of the repository HEAD used for this work.

`linux/manifest/recipe-origin.sha256` is the digest file from `tools/sha256_manifest.rs`, compiled in the pinned image with `rustc --edition 2021 -D warnings`. The tool printed `empty_sha256=e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` and `abc_sha256=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`. `cmp` of this session's tool against the a199 file exited 0 (`linux/manifest/origins-console.log`, `cmp_a199_exit=0`). The same digest is on both copies.

| Name in the digest file | Commit cited by the recipe | Path | SHA-256 |
| --- | --- | --- | --- |
| `a204-preoverlay-launch.sh` | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| `a204-preoverlay-inside.sh` | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| `a200-smoke-launch.sh` | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |
| `a199-sha256_manifest.rs` | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |
| `a256-impl5-launch.sh` | `755cd270be8d7498c60ddb7829600ac3b7801c8e` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a256/SP12/question-bank/linux/impl-5/launch.sh` | `7deda2f1d47c4d621849fa090373ab3688c48cb206e14c12c45420191b008d90` |
| `a256-impl5-inside.sh` | `755cd270be8d7498c60ddb7829600ac3b7801c8e` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a256/SP12/question-bank/linux/impl-5/inside.sh` | `18e7652a111acc73d9899693cb47bfff8689bdece209b2883e38ffc83ebeb90a` |

a204 is the fixed-archive launch shape: `git archive`, a private bridge, private cargo and target volumes, a capacity gate, `--pull never`, `--cap-drop ALL`, `HOME=/tmp`, and inspect after exit. a256 impl-5 is the immediate launch and inside shape this session copied. a200 is the private Postgres smoke shape. This session runs zero Postgres containers and does not copy that database recipe. a199 is the manifest tool, byte-identical here.

Deltas from those recipes: owner `a267`, fixed commit `567d621e`, memory 6 GiB, 1 CPU, global builder cap 10, own builder cap 1. The archive keeps `benchmarks/question-bank`, `benchmarks/sql-runner`, `benchmarks/query-result-adapter`, and `benchmarks/result-comparator`. Only `benchmarks/question-bank-suite` is overlaid. a256 removed `benchmarks` on its archive; this launch leaves those four benches in place. Smoke hashes the sealed question-bank and sql-runner locks and runs one decimal bank test plus the sql-runner FIPS vectors. The impl inside script formats, generates the crate lock offline, prebuilds the three runner binaries, rustc-compiles the query-pg test double, then runs `cargo test` and `cargo clippy`. The manifest launch uses `--network none`, `rustc --edition 2021`, and `--pids-limit 2048`.

Image: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. The Postgres image was not started.

The first origins container, `oo-a267-manifest-origins` id `eec8c2a535a0e4014dc2e247f2da039591476c227ba69b58e7968c632488d601`, wrote the self-check to `linux/manifest/origins-console.log` and left the per-file digest on the container tmpfs. `oo-a267-manifest-origins-print` id `5e6cea54b2ee5e8ac33bf4225213588179a12ba8e75505074a48a402d6727f70` wrote `linux/manifest/recipe-origin.sha256`. Both exited 0. Host config for each: privileged false, network none, CapDrop ALL, memory 6442450944, NanoCpus 1000000000.

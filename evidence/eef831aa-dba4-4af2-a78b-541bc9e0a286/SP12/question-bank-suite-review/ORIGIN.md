# Origin recipes for the a286 question-bank-suite review

Fixed archive: `c551afea55d5a9164760878a5d6a76835b869875`.
Pre-seam adapter parent: `f57e430f51b9f44175ffab5ae876cd64dd558c78`.
Seam: `eb0bbbe7b82789cbdc362771e1b89226fc1af832`.
Suite introduction: `c9b3c6fffef507afc2e4c54d1fc5933f008ae04a`.

This review does not edit product crates, root `Cargo.toml`, root `Cargo.lock`, or bench locks.

| Recipe | Path | Role here |
| --- | --- | --- |
| a204 launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | Builder launch shape |
| a199 manifest | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs`, via the a281 manifest launch | Inventory |
| a200 postgres | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh` and `pg-wrap.sh` | Private PostgreSQL |
| a200 libclang | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/extract-libclang.sh` plus a222 `sha256.rs` and `select-debs.rs` | Bindgen libraries |
| a276 live | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/linux/live/` | Query-pg TLS probe shape |
| a267 proof | `benchmarks/question-bank-suite/tests/proof.rs` on the fixed archive | Frozen regression, separate from the fresh bank |

`linux/smoke/origin-recipes.txt` records the SHA-256 of those files at launch. `pg/pg-wrap.sh` and `tools/sha256_manifest.rs`, `tools/sha256.rs`, and `tools/select-debs.rs` are byte copies.

Deltas from a204 and a276: owner a286, session `eef831aa-dba4-4af2-a78b-541bc9e0a286`, fixed commit `c551afea`, pre-seam archive `f57e430f`, memory 6 GiB, one CPU, `CARGO_BUILD_JOBS=1`, global builder cap 10, one builder for this owner. The smoke archive is the integrated seam. The opt-in must be present there and absent from the parent adapter. PostgreSQL names are `oo-a286-pg`, SAN DNS `oo-a286-pg`, no host port.

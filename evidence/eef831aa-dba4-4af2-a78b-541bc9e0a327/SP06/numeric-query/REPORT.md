# Numeric query, a327

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a327`. Scope E46/M1-D14-N. Fixed ancestor `e6ebf93b5ac261c097a24d145611e0300e4f88b5`.

This task admits `numeric` for semantic quantity and decodes PostgreSQL numeric binary into an exact decimal cell. A private TLS PostgreSQL 17.10 run compiled the reviewed policy and imported the exact fixture. `install-column-acl` then refused the numeric columns. `query-pg` did not run, so canonical query cells were not produced.

No M1-I01, M1-I02, parent acceptance, or release claim.

## Commits

| Commit | What |
| --- | --- |
| `e6ebf93b5ac261c097a24d145611e0300e4f88b5` | Fixed archive. Immutable. |
| `3517cd131719cfdd572ba2b85bb7013e6c90588e` | Frozen numeric wire, type, semantic, and cell contract. Committed before product code. |
| `c671a3caa1265dbf97a313b065cc13c188c1533a` | Product admission and exact decoder. Ancestor of current HEAD. |

The live harness in `crates/oo-gateway/src/bin/query-pg.rs` is an addition on top of that product commit. Evidence for this UUID lives under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a327/SP06/numeric-query/`.

## What the private database showed

Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Container `oo-a327-pg`, 6 GiB, 1 CPU, private network `oo-a327-eef831aa-net`, TLS, no published port. `pg/up.exit` is 0.

Builder image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Linux arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One a327 builder at a time, 6 GiB, 1 CPU, `CARGO_BUILD_JOBS=1`, locked offline. The proof mount was the e6ebf93 archive plus the four owned Rust files. Cargo.lock stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

On the last live run the compiled tools did this:

| Step | Exit |
| --- | --- |
| catalog-pg | 0 |
| captured-pg-draft | 0 |
| review-pg-draft | 0 |
| reviewed-effective-oos | 0 |
| role-pack | 0 |
| role-pack-bundle | 0 |
| import-pg before init | 1, stderr `oo-pipeline: target schema is not ready` |
| init-pg | 0 |
| import-pg | 0 |
| reviewed-policy | 0 |
| install-column-acl | 7, stderr `install-column-acl: policy type` |

`reviewed-policy` wrote one policy for `shop.entry`. The column semantics are `identifier`, `quantity`, `quantity` for `entry_id`, `amount`, and `wide`. `amount` is `numeric(18,4)`. `wide` is `numeric(38,4)`. The import copied the six source rows, including the 38-digit value, the negatives, the scale-4 zeros, and the nulls. No query stdout, stderr, or audit file exists, because the query was not launched.

## Linux checks before that boundary

Pre-edit smoke exit 0. Post-edit product phases on Rust 1.94 exited 0 for `oo-gateway` lib tests, `a327_numeric`, `query-pg` unit tests, and clippy `--lib --bins -D warnings`. The post-edit harness exit is 2 because it required an unowned quantity test to fail. That test still passes: an unbounded numeric column has no `data_type`, so admission returns semantic before the numeric rule. The contract records that. The product phases in that run were green.

The last live builder, before the proof test, also exited 0 for toolchain, overlay, libclang, clippy, `query-pg` decimal unit test, and the pipeline, builder, and gateway binaries. `test-live` exited 101 at the install boundary.

## Preserved failed attempts

| Directory | Result |
| --- | --- |
| `linux/postedit-failed-1` | Docker could not mount a new file onto the read-only archive. |
| `linux/postedit-failed-2` | `PgNumericCell` was not `Debug`. |
| `linux/postedit-failed-3` | Clippy was absent until `rustup component add`. |
| `linux/live-failed-1` | The test filter matched nothing, so cargo exited 0 with 0 tests. |
| `linux/live-failed-2` | Fixture insert `sqlstate=22003`. `numeric(18,4)` cannot store Infinity. |
| `linux/live-failed-3` | `import-pg` exit 1: `full import refused numeric_nan`. |
| `linux/live` | Install type refusal above. |

## Cell contract that did run on Linux

The decoder and the JSON cell live in the product commit. Linux 1.94 lib tests cover the wire vectors, NaN, both infinities, scale and precision refusal, and non-numeric OIDs. The hand literals for `123456789012345678` scale 4 and the 38-digit coefficient are in `crates/oo-gateway/tests/a327_numeric.rs`. They were not compared with a `query-pg` response.

## Ownership

Edited only the reserved paths. Manifests, the lock, `lib.rs`, gateway HTTP, a317 data, and `policy_catalog.rs` were not modified by this task. A dirty `policy_catalog.rs` and `query_executor_pg_live.rs` are present in the working tree from another actor and were not mounted into the proof.

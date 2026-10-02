# a106 handoff

Local commit for an independent fixed-commit review. No push, no pull request, no release, no self-approval. The `oo query` child is not accepted. a103's request-changes stands until that review and coordinator receipt. This is not SP09.

## Identity

Parent of this commit: `dde0dff30e64c9f4c30a7b1371508de38efea553`, tree `902e30f21751d9fdf5d0c7aa63bb4bc69f8a1260`, parent `aba39fae9fe7e174cd58fa560091fecffcb39c0a`.

The activation card named `647387de108a8f45c82df2d5387302f8d9f9a899`, tree `3f374c1e50609ee4b6f5d1c98b65155f2776cc6d`, parent `fcb59dd211c7b27d1e605c0ebc858d545b828a15`. That commit is an ancestor. The tip had already moved by two receipt commits before the first build. The lock blob is the same on all three: `15ae1071c7236a956547e3f1d37d627b2cbebfce`, raw sha256 `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f`. This change does not edit `Cargo.lock` or any manifest.

Review source remains `3347f5a280da52c70525879b4608170b6b92b523`. Before this edit, `load.rs` blob `57024a15faea0648b95e44a0eecb5875b65c9f8a` matched that commit.

This file cannot record the hash of the commit that contains it. Read `git rev-parse HEAD` and `git rev-parse 'HEAD^{tree}'` after the commit.

## Owned paths

- `crates/oo-cli/src/load.rs`
- `crates/oo-cli/tests/common/mod.rs`
- `crates/oo-cli/tests/query.rs`
- `contracts/cli/query/README.md`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a106/SP09/cli-fifo/`

No root manifest, CLI manifest, lock, SDK, core, connector, pipeline, jobs, helper, old evidence, spec, plan, or ledger path is in this commit.

## What the runs showed

Rust 1.94.0, linux/arm64, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Fetch was online. Test and Clippy were `--locked --offline`. Not an air-gap claim.

Baseline run `c1c68a79-25ad-4575-a198-dcbd21ea5952` on the immutable `dde0dff` archive: fetch, the previous nine tests, and Clippy exited 0. The red probe then reproduced the three peerless FIFO stalls. Each was signal 9, no exit code, empty stdout and stderr, 0 accepts, elapsed 1218ms, 1216ms, and 1222ms, with `--timeout-ms 120000`. Probe exit 0 means that failure was reproduced. `product_pass=no`.

Fixed run `8ed66641-38ee-47d2-89b9-26ca8b6a6606` on the archive plus the owned overlay: fetch 0, 11 integration tests passed, 0 failed, 19.35s, Clippy `-D warnings` 0, docker 0. The new tests reject peerless and writer-held token, SQL, and CA FIFOs with the documented `not-file` exits before a socket. A killed child is not scored as exit 1. Dangling symlink, hard link, token 4096, 4096 plus a newline, token 4098, and SQL 65536 stayed on the contract. The previous nine tests, including SQL 65537 and the CA TLS cases, passed again.

## Cleanup

Both a106 UUIDs are gone: container, bridge network, cargo volume, and target volume, each removal exit 0. No `oo.owner=a106` resource remained. Prune was not run. a108's retained volumes were not reused or removed. a110's postgres container was not touched.

## Unsigned

Foreign uid, ACL, same-owner rewrite, zeroization, a live path-to-descriptor race, every device `open`, the 16 MiB render cap, backend cancellation, stalled stdin, the rest of the 117-case matrix, conversation, Gateway, PostgreSQL, sandbox `from_env` and proxy, the full SDK, supply, dual-arch, and release.

# Handoff

Repair commit: `c27d6349ffa7cf8629a5a79c62015ca5c255e18f`. Parent: `90dfac17534983e3f3f93d5f437806193c24095b`. Base reviewed by a195: `c22cf08aa5973ae2e2d95e0d2b72f98487f8555a`. This file does not contain the evidence commit's own SHA.

Review next. This is not a release, not a public gateway, and not an admission change.

## Read this first

`REPORT.md` is the run record. `FINDINGS.md` is the repair judgment. `source/COMMIT` and `source/blobs/` are the four owned files from that commit. `SHA256SUMS` lists every other file in this directory. `SHA256SUMS.sha256` is the digest of that list, both written by `tools/sha256.rs` (self-test empty = `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`, `abc` = `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`).

The 500 ms deadline now includes socket waits, the process-group signal, the direct wait, and the joined IO thread. Same-group descendants are killed. A descendant that has left the group is not. `RESTRICTION_CAPABILITY` stays `Unsupported`. `UNTRUSTED_ADMISSION` stays `Blocked`. SP06-R8, authentication, the public gateway, and release stay open.

## What is where

- `source/blobs/parser_worker.rs` — blob `59c1518d5d2941f04e2c51ec1585de1e9e57c283`. Before the repair the blob was `aabf78ad16f621ea5fd82bc4fa94548c28895767`.
- `source/blobs/oo-sql-parser-worker.rs` — blob `99e04e5b9810fb8885534c685a59d49c3ad5b0d8`.
- `source/blobs/parser_worker_tests.rs` — blob `8b47ac63ed490a77a734218bd9ec14942b62a7f1`.
- `source/blobs/parser-worker-v1.md` — blob `14fdbcd0244c0f83c8dd640a34b53325dc2c4dbc`.
- `linux/smoke/` — pre-edit Rust 1.94 smoke. Tests and clippy passed. The red compile failed.
- `linux/smoke/red-attempt-2/` — preserved hang of the unbounded std probe. Container exit 143.
- `linux/smoke/red-attempt-3/` — preserved harness compile failure.
- `linux/smoke/red-attempt-4/` — pre-edit grandchild hang, 3.002s, and the std-api probe.
- `linux/proof/` — tests passed before clippy was installed in that container.
- `linux/proof-clippy/` — `needless_return` on the pre-fix `spawn_worker`.
- `linux/proof2/` — final tests and clippy, both exit 0.
- `tools/sha256.rs`, `tools/select-debs.rs`, `tools/std-api-probe.rs` — std-only tools. The probe is evidence, not product code.

The product diff is only `crates/oo-gateway/src/parser_worker.rs`, `crates/oo-gateway/src/bin/oo-sql-parser-worker.rs`, `crates/oo-gateway/tests/parser_worker.rs`, and `contracts/gateway/parser-worker-v1.md`.

## Cleanup after this commit

Remove only this task's leftovers:

- container `oo-a201-red2` if it is still exited, plus `oo-a201-smoke`, `oo-a201-red3`, `oo-a201-red4`, `oo-a201-proof`, `oo-a201-proof-clippy`, `oo-a201-proof2`, `oo-a201-hash`, and `oo-a201-hash-run` if any remain
- network `oo-a201-eef831aa-net`
- volumes `oo-a201-eef831aa-cargo` and `oo-a201-eef831aa-target`
- directory `/tmp/oo-a201-eef831aa-dba4-4af2-a78b-541bc9e0a201`

Leave the pinned Rust image. Do not prune. Do not remove other owners' containers or volumes.

The hash container writes its metadata, then a second run of the already-built `sha256` binary refreshes `SHA256SUMS`. That second run does not write another evidence file after the manifest.

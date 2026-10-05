# Handoff

Verdict: **APPROVE LIMITED**

Parent of this evidence commit: `73d772c5cc7bac52c9e95754444f4c839a3ec646`. This file does not contain the evidence commit's own SHA. The source under review is `c22cf08aa5973ae2e2d95e0d2b72f98487f8555a`. Author evidence is `a63f2de3fd411532e11f5863c0f595724ad6f149`.

## Read this first

`REPORT.md` is the run record. `FINDINGS.md` is the line-cited judgment. `linux/proof/oracle.txt` is the append-only process oracle from the passing proof. `SHA256SUMS` lists every other file in this directory. `SHA256SUMS.sha256` is the digest of that list, both written by `tools/sha256.rs` (self-test empty = `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`, `abc` = `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`).

The approval covers the one-shot direct child on that source: legal plans match `guard_select`, the codec bounds hold, and timeout, crash, oversize, malformed, trailer, cancel, blocked pipes, and a late frame kill and reap that pid. `RESTRICTION_CAPABILITY` stays `Unsupported`. `UNTRUSTED_ADMISSION` stays `Blocked`. A grandchild that holds stdout is not reaped and can block `finish`'s join. SP06-R8, authentication, the public gateway, and release stay open.

## What is where

- `source/blobs/` — owned files copied from the archive after `git hash-object` matched `c22cf08`.
- `linux/smoke/` — pre-overlay Rust 1.94 smoke. `attempt-1/` is the empty `libclang.so` loader failure.
- `linux/proof/attempt-1/` — harness compile failure, no supervisor run.
- `linux/proof/attempt-2/` — 11 tests passed; clippy was absent in that fresh container.
- `linux/proof/` — recorded proof, docker exit 0, oracle, host config, mounts.
- `overlay/parser_worker_a195_review.rs` — the only test added, and only on the private proof tree. The immutable archive does not contain it.
- `tools/sha256.rs`, `tools/select-debs.rs` — host-free Rust tools used inside the container.

Failed attempts were kept. Product files were not edited. The worktree's dirty `crates/oo-gateway/src/policy.rs` was not compiled. The containers mounted the archive or the private proof copy.

## Cleanup after this commit

Remove only this task's leftovers:

- containers `oo-a195-smoke`, `oo-a195-proof`, and `oo-a195-hash` if any remain
- network `oo-a195-eef831aa-net`
- volumes `oo-a195-eef831aa-cargo` and `oo-a195-eef831aa-target`
- directory `/tmp/oo-a195-eef831aa-dba4-4af2-a78b-541bc9e0a195`

Leave the pinned Rust image. Do not prune. Do not remove other owners' containers, including `oo-a196-*` and `oo-a190-pg`.

`linux/hash/launch.sh` writes the hash container's metadata, then runs the manifest a second time so those metadata files are inside `SHA256SUMS`. It does not write another evidence file after that second run.

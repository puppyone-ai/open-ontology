# HANDOFF — a138 reader R3

Source commits, parent first:

1. `8bda0cd48976b23d268d125b34bc8130d94b8253` — typed half-open key chunks on the existing repeatable-read owner.
2. `58afa97ded85a7dd89c72532bfc8973c45255d7c` — cancel an in-flight portal open or Execute on that owner. Tree `543fd103532a8366ce92a0b4eaea4e089eb76d2d`.

Fixed input remains `dd5bf44160f949c0ac21a0ce3ddf5ee5e90b4200`. `error.rs` and `config.rs` match that blob.

## Read first

1. `FINDINGS.md` — 300 second deadline versus the 120 second server cap, uuid not a declared key, guard `pg_cancel_backend` succeeded, dropped `next_batch` left the backend active, terminated run did not finish.
2. `REPORT.md` — images, toolchain, exits, oracles, and the four overwritten cancel failures.
3. `contracts/source/pgsql/reader-keys-v1.md`, `reader-lifecycle-v1.md`, and `bounded-stream-v1.md`.
4. `linux/keys/keys-outcome.txt` and `linux/cancel/cancel-outcome.txt`.
5. `linux/cancel/console.log` — the passing lib, keys, cancel, and Clippy run. Earlier cancel consoles were overwritten.

## Still open

Independent fixed review of `8bda0cd4` and `58afa97d` is next. R4 integration is unsigned. Resume cursors, CDC, and the registry API are unsigned. `bounded_reader` and `type_support.converted` stay false. This run did not exercise a drain that exceeds 5000 ms, and it did not observe `25006` from the read-only guard. Those remain the documented `cleanup_unconfirmed` path.

No ledger checkboxes were marked. No push and no release.

## Cleanup and manifest

`cleanup.txt` records removal of `oo-a138-pg`, `oo-a138-msrv-cancel`, `oo-a138-msrv-keys`, `oo-a138-msrv-smoke`, network `oo-a138-eef831aa-net`, the four `oo-a138-eef831aa-*` volumes, and this handle's `/tmp/oo-a138-*` paths. No prune. `hashes.sha256` lists every other file in this directory. The evidence commit is path-limited to this directory.

# Handoff

Role Pack SQL-string and name closure repair on `eff1959bf028a65b20b7fca855d79c23e4b188fd`, tree `2f08d4ecef7ad2f3b617e8a04afcb45f0cf0186f`, parent `9e3f757fb34746c64c6fca3d60cf0c5e342a1885`. Subject: `builder: close role-pack SQL string and name closure`.

The requirement record is `REPORT.md`. The line-level notes are `FINDINGS.md`. The hash check is `ORACLE.md`. Review `95289c50` on `ef34ad7bc6d89376d0b1952dfe49e5efb2a8744d` is the defect record. This directory does not mark that review accepted. Independent review of `eff1959b` is next and was not started here.

The source commit contains only:

- `crates/oo-builder/src/role_pack.rs`
- `crates/oo-builder/tests/role_pack.rs`
- `contracts/builder/role-pack-v1.md`

No dependency, lock, core, other builder, spec, or ledger file was edited by this task. The parent already carried the a173 gateway lock (`Cargo.lock` blob `8364c36f2c70919cd3a1df18a2e57698a44051a8`, content `45a784dc73ee1ba7531430bb4f32f368f22220271c28fccb84540593953bcffd`). Nothing was pushed.

The source commit was written after `linux/focused-2` exited 0 and before this evidence package. It was not amended.

Deciding run: `linux/commit/`. Container `aee84d9d3ce40ef13f9ea2f9cb9e01540f7aa10f07a981a40cb21d5294a57920`, docker exit 0, archive of `eff1959b`, `role_pack` 11 passed, clippy `-D warnings` exit 0. Offline fetch exit 101, then online fetch exit 0. `linux/commit/expect-sha256.txt` matches the committed content hashes, including the committed lock.

Preserved earlier runs:

- `linux/preedit/` on archive `ef34ad7b`, lock `34b7364c3c68b48a7d1c1cab40b659408ce9087f14a15626c38ed2016a63af8e`, 10 passed, clippy 0. Offline fetch 101 on an empty cargo volume, then online fetch 0.
- `linux/focused/` post-edit failure on that same lock. Ten existing tests passed. The new test panicked on the needle `source-declared` inside the relationships intro sentence. Clippy exit 0. `snapshot/` keeps those three files.
- `linux/focused-2/` on the same three product bytes and the `ef34ad7b` lock. 11 passed, clippy 0, offline fetch 0.

`source/blobs/` is a copy of the committed compiler inputs listed in `source/blobs.sha256`. `MANIFEST.sha256` is the SHA-256 of every file in this directory except itself.

The a168 Python oracle remains at `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a168/SP07/role-pack-review/`. It was not rewritten. The new check is Rust `sha2` 0.10.9 inside `tests/role_pack.rs`.

`CLEANUP.txt` records removal of this task's network, cargo volume, target volume, and `/tmp` tree. The builder containers were removed by their launch scripts. The pinned Rust image was left in place. `docker prune` was not run. PostgreSQL was not started. Slot B resources were not removed.

Domains, tokenizer, read API, search, mount, authentication, a policy adapter, a database re-proof of row security, and publication gates G1–G13 stay open. This is not SP07-W06 approval. A visibility snapshot is not authentication and not a grant. A successful parse is not SQL execution.

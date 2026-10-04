# Handoff

Role Pack dependency closure repair on `ef34ad7bc6d89376d0b1952dfe49e5efb2a8744d`, tree `9f2f6d353a957cc400c7be01d575599260117fe4`, parent `06a99f1181b648863975eb214c14f949d835a67e`. Subject: `builder: close role-pack visibility for quoted names and labels`.

The requirement record is `REPORT.md`. The line-level notes are `FINDINGS.md`. The hash check is `ORACLE.md`. Review `d61f9a4c` on `3dadf678ff0fbfdc1ddd2ac4144ee68cb2ba772c` is the defect record. This directory does not mark that review accepted. Independent review of `ef34ad7b` is next.

The source commit contains only:

- `crates/oo-builder/src/role_pack.rs`
- `crates/oo-builder/tests/role_pack.rs`
- `contracts/builder/role-pack-v1.md`

No dependency, lock, core, other builder, spec, or ledger file was edited. Nothing was pushed.

Deciding run: `linux/focused-2/`. Container `1ded18353e936686b6bdaeb9d5ddafc832f03dd07d0e5b18a54c30696ab51f45`, docker exit 0, `role_pack` 10 passed, clippy `-D warnings` exit 0, offline fetch exit 0. The snapshot hashes in that directory match the committed content hashes.

Preserved earlier runs:

- `linux/preedit/` on archive `3dadf678`, 9 passed, clippy 0. Offline fetch 101 on an empty cargo volume, then online fetch 0.
- `linux/focused/` post-edit failure. Nine existing tests passed. The new test panicked on the needle `ex-quoted` inside kept id `ex-quoted-visible`. Clippy `needless_character_iteration` exit 101.

`source/blobs/` is a copy of the committed compiler inputs listed in `source/blobs.sha256`. `MANIFEST.sha256` is the SHA-256 of every file in this directory except itself.

The a168 Python oracle remains at `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a168/SP07/role-pack-review/`. It was not rewritten. The new check is Rust `sha2` inside `tests/role_pack.rs`.

`CLEANUP.txt` records removal of this task's network, cargo volume, target volume, and `/tmp` tree. The three builder containers were removed by their launch scripts. The pinned Rust image was left in place. `docker prune` was not run. PostgreSQL was not started. Slot B resources were not removed.

Domains, tokenizer, read API, search, mount, a database re-proof of row security, and publication gates G1–G13 stay open. This is not SP07-W06 approval. A visibility snapshot is not authentication and not a grant.

# Handoff

Owner of this delivery: session `eef831aa-dba4-4af2-a78b-541bc9e0a256`, paths `benchmarks/question-bank/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a256/SP12/question-bank/**` only.

The next reader can rebuild from the fixed archive without editing the root lock:

1. `git archive c828e2781fc37eb62249fab54fa0b7fff86f0249` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, and `crates`.
2. Rename that archive's `rust-toolchain.toml` so rustup does not select 1.98.1.
3. Copy `benchmarks/question-bank` onto the archive. Delete the copied `Cargo.lock` if the next proof must regenerate it inside the archive. The committed lock is the attempt 5 lock.
4. `cargo test --locked --offline --manifest-path benchmarks/question-bank/Cargo.toml` with `RUSTUP_TOOLCHAIN=1.94.0`.

Do not run `cargo update` for this crate. a207 remains the only writer of the root manifest and the root lock.

Proof to read first: `linux/impl-5/`. Smoke: `linux/smoke/`. Failed attempts and the earlier green attempt 3 stay in place. Contract: `benchmarks/question-bank/CONTRACT.md`. Reviews: `review/01-public-input.md`, `review/02-approval-and-roles.md`, `review/03-views-and-hashes.md`, and `review/04-payload-and-typed-gold.md`.

Private resources for this session are the network `oo-a256-eef831aa-net` and the volumes `oo-a256-eef831aa-cargo` and `oo-a256-eef831aa-target`, plus `/tmp/oo-a256-eef831aa-dba4-4af2-a78b-541bc9e0a256`. They are removed at the end of this delivery. Do not prune and do not remove another owner's containers.

The second inventory stdout stays at `/tmp/oo-a256-eef831aa-dba4-4af2-a78b-541bc9e0a256/hash-again.log` and is not part of the hashed tree. Cleanup removes that file with the private tmp directory.

This delivery stops at the public synthetic question-bank validator. The 300-question bank, two-human annotation, the 20% dual-annotation gate, B-D06 acceptance, and a release remain open.

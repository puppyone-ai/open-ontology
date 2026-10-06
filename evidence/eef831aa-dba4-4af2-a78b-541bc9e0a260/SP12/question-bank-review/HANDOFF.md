# Handoff

Owner of this review: session `eef831aa-dba4-4af2-a78b-541bc9e0a260`, path `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a260/SP12/question-bank-review/**` only. No product path was edited.

Verdict in `REPORT.md`: APPROVE_LIMITED for the public synthetic question-bank validator at `c275aa6326ae945ef09534bd6fb5b7f2a5eb938b`. Read `FINDINGS.md` for the hand-bank hashes, the canary split, and the refusal pointers. Read `ORIGIN.md` for recipe hashes. Read `CLEANUP.txt` for the resources removed after the inventory.

Raw delivered proof: `linux/proof-2/`. Earlier driver stop, kept: `linux/proof/`. Smoke, before both: `linux/smoke/`. Sealed sources: `source/blobs/benchmarks/question-bank/`. Contract in that blob: `CONTRACT.md`. Independent driver: `tools/drive.rs`. Inventory: `SHA256SUMS` and `crate-sha256.txt`, written by `tools/sha256_manifest.rs` inside the pinned image. The second-pass log is `/tmp/oo-a260-eef831aa-dba4-4af2-a78b-541bc9e0a260/hash-again.log` and is outside the manifest. Cleanup removes that directory with the rest of this session's tmp.

The next reader can rebuild from the fixed archive without a worktree overlay:

1. `git archive c275aa6326ae945ef09534bd6fb5b7f2a5eb938b` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `benchmarks/question-bank`.
2. Rename that archive's `rust-toolchain.toml` so rustup stays on 1.94.0.
3. Fetch only the question-bank manifest. Do not `cargo update`. The crate lock stays a subset of the root lock.
4. `cargo test --locked --offline --manifest-path benchmarks/question-bank/Cargo.toml` with `RUSTUP_TOOLCHAIN=1.94.0`.

a207 remains the only writer of the root manifest and the root lock.

This review stops at the question-bank crate at that commit. B-D06, the public 300-question bank, two-human annotation, the 20% dual-annotation gate, and a release remain open. `linux/proof-2/independent/scope.txt` records `not_approved=300 two-human 20-percent-dual private-holdout customer B-D06 release`. That line is the driver's bound. It is not a parent acceptance.

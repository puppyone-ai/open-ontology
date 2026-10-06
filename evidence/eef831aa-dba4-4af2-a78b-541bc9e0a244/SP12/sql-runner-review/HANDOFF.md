# Handoff

Reviewer session `eef831aa-dba4-4af2-a78b-541bc9e0a244`. Owned path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a244/SP12/sql-runner-review/**` only. No product path was edited.

Verdict in `REPORT.md`: APPROVE_LIMITED for the trusted SQL runner at `8f533140a2823568c5f375190f8708934cd413fa`. Read `FINDINGS.md` for the harness failure, the truncation failure, and the backend snapshot. Read `ORIGIN.md` for recipe hashes. Read `CLEANUP.txt` for the resources removed after the inventory.

Raw proof: `linux/proof/`. Preserved first launch: `linux/proof-1/`. Smoke, before that proof: `linux/smoke/`. Sealed sources: `source/blobs/benchmarks/sql-runner/`. Contract in that blob: `CONTRACT.md`. Hand suites: `hand/` and `expect/`. Inventory: `SHA256SUMS`, written by `tools/sha256.rs` inside the pinned image. The second-pass log is `/tmp/oo-a244-eef831aa-dba4-4af2-a78b-541bc9e0a244/hash-again.log` and is outside the manifest. Cleanup removes that directory with the rest of this session's tmp.

A parent reader can treat the runner at that commit as reviewed for the suites this directory actually ran: pass with k=2, mismatch, hidden-column refusal, unmapped-relation refusal, matched truncated prefix, peek match plus peek refusal, timeout, and missing completion, plus the hash and gold-path bounds. `backend_cleanup` `unknown` stays a non-observation. The after-runner snapshot still had a client backend waiting on Lock.

Leave the ledger, the acceptance counts, and the product tree alone. This directory does not accept query-pg, the adapter, the comparator, trial statistics, a question bank, a real frozen database, an agent session, a customer run, or a release. It does not re-review a242. a207 remains the sole writer of the connector manifest and the root lock.

To rebuild the same runner bytes, archive `8f533140` including `benchmarks/sql-runner`, rename `rust-toolchain.toml` on that copy, and build with `RUSTUP_TOOLCHAIN=1.94.0` on `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Do not `cargo update`. Do not overlay a later worktree. The query-pg digest in this proof is `8dda46d6d887b02e2568bba4d9f6d9f6a51548b3494e889999f9b9e275e8f92f`. A later commit will not reproduce it.

Private names used here: network `oo-a244-eef831aa-net`; volumes `oo-a244-eef831aa-cargo`, `oo-a244-eef831aa-target`, `oo-a244-eef831aa-pgdata`; containers `oo-a244-pg`, `oo-a244-smoke`, `oo-a244-proof`, `oo-a244-hash`, `oo-a244-hash-2`. The first proof container id is `d95bb048b05930f0191f6503cf18287379ba59c7913d769132a383b5b93b68d4`. Do not prune. Do not remove another owner's containers. At proof start the global rust-builder count was 1 and that builder was foreign. Other sessions' builders were running when these documents were written. They are not this cleanup.

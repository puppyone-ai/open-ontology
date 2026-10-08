# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a385`. Grok was the only executor. No subagents and no worktrees.

## Owned paths

- `tools/feedback-receiver/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a385/**`

`tools/audit-ingest`, `tools/selfbuild-suggestions`, gateway, core, control, jobs, root manifests, frontend, the ledger, and the reviewed deploy recipes were read and not modified. Codex updates the ledger.

## Status

The feedback receiver queue slice has a passing private proof. See `REPORT.md`, `raw/live-result.txt`, and `pg/receipt-counts.txt`. This is not full SP02-W06, not full SP13-W04, and not parent or goal acceptance.

The SP13-W04 slice here is the queue: bounded gap, reject, failure, and comment items become stored review candidates with sources. `GET /v1/reviews` and the `oo_feedback_receiver` tables are the reader surface. A suggestion job, a review page, and the post-accept G-gate are outside this card.

Source list: `SOURCE-INVENTORY.txt`. Hashes of the tool files and of this evidence tree, except `SHA256SUMS` itself, are in `SHA256SUMS`.

## What the next reader should rerun

`linux/proof/launch.sh` on a machine that already has the two pinned images. It refuses to pull. It mints a new secret directory on every launch. The passing run deleted that directory, the volumes, the network, and both containers (`CLEANUP.md`). A later launch creates a new PostgreSQL volume. Do not point it at another task's volumes or secret directory. Do not prune.

The builder must set `RUSTUP_TOOLCHAIN=1.94.0`. Host `cargo` and `rustc` are not the proof. One builder, 6GiB, 1 CPU, including the launch. One PostgreSQL 17.10, 1GiB, 1 CPU. Global builder cap is 10. The script checks that cap before it starts the database and again before the builder.

`inside.sh` compares hashes of the immutable inputs before and after. The seeded `tools/feedback-receiver/Cargo.lock` was accepted. `cargo generate-lockfile` runs only if `cargo metadata --locked` rejects it, and the subset check still refuses checksums that are not in the root lock. `CARGO_NET_OFFLINE=true` is set after fetch. Do not `cargo update` the root lock.

If a run fails after the live test has inserted receipts, remove only `oo-a385-pgdata` before the next launch. Keeping that volume makes the next first accept look like a duplicate. Keep or drop `oo-a385-cargo` and `oo-a385-target` only as this card's volumes. Copy `linux/proof/phases` aside first, because the next `inside.sh` overwrites those logs. `raw/console.log` appends.

## Integration surface

- Binary: `feedback-receiver --config <path>`.
- Library: `submit_review` calls `selfbuild_suggestions::produce` and returns the ack. The HTTP server calls the same `review` function with the receipt id.
- Migration: `migrations/oo_feedback_receiver_v1.sql`, applied by the administrator in the proof, not by the receiver role.
- Contract and wire schemas were frozen and were not rewritten after implementation: `CONTRACT.md`, `schema/request.v1.json`, `schema/ack.v1.json`, `schema/candidate.v1.json`, `schema/source.v1.json`, `schema/review.v1.json`, `schema/registry.md`.

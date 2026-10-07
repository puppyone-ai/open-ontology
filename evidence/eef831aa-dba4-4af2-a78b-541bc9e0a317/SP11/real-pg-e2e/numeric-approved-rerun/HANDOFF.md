# Handoff

Owned paths for this rerun:

- `testing/real-db/pg-e2e/**` (unchanged from `f4381843c7f376f196bc30c78c18d9701dc29a6d`, tree `79e2ddc120b2117f90d3e79ff61571490243f23b`)
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a317/SP11/real-pg-e2e/numeric-approved-rerun/**`

No product path was edited. Product bytes under test are `git archive` of `21cc96241a841b47dbbcb116549030e38a643ba0`. Dirty worktree files were not mounted as the compiler source. a313 remains the only root manifest and lock writer.

The parent `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, and `cleanup.log` record the first chain on `10ecd7654fd7939b6655a0e50a84214a7fca316e`. They were not rewritten.

## What the next reader should open

- Frozen contract: `testing/real-db/pg-e2e/CONTRACT.md`. `wide-qty` stays expected exit 0 with `WHERE entry_id IN (0, 1, 2)`.
- This rerun: `REPORT.md`, `FINDINGS.md`, `linux/chain/DIAGNOSIS.txt`, `live/run/apply-outcome.json`.
- Request the product refused: `live/run/queries/wide-qty.request.json`.
- Archive function that refuses it: `source/blobs/sql_guard.rs`. Blob id `1db8f3ed11a2d816d485bdd2865f37aff3e5cbed`. Other blob ids are `source/BLOBS.txt`.
- Recipe origin note: `origin/recipes.txt`.
- Pre-chain archive smoke: `linux/smoke/`. Docker exit 0. `pg-onboarding` totals 24 passed and 1 ignored (`live_chain`).
- The one live attempt: `linux/chain/` and `live/run/`.
- Privilege and cancel outputs: `live/side/`. One cancel already ran.
- Measurements: `live/measurements/import.txt` and `live/measurements/pg-stats.txt`.
- Inventory: `SHA256SUMS`, from `tools/sha256_manifest.rs` via `linux/inventory/launch.sh`. The walker skips `SHA256SUMS`. Files written after the walk are listed in `linux/inventory/NOTE.txt`.

## Blocking seam for the coordinator

Phase `apply`. Parent exit 1. Stderr `pg-onboarding: query-wide-qty exit`. Child `query-wide-qty` exit 6. Stderr `query-pg: guard`. Stdout empty. No `live/run/audit/wide-qty.jsonl`.

Cause in the fixed archive: `sql_guard::admit_op` rejects `AexprIn` as `GuardError::Unsupported { construct: "IN predicate" }`. `query-pg` collapses every guard error to exit 6 and the static line `query-pg: guard`.

`reviewed-policy` on this commit exited 0 for both roles. `numeric` quantity is admitted there. The numeric cell probe and the target COPY compare did not run.

Leave the frozen statement, the decisions, the 100000-row formulas, and the expected exit 0 in place. A product change that admits a bounded literal `IN` list needs its own later review. Rewriting this statement into equality disjunctions would hide the seam.

M1-I01 and M1-I02 stay `not_accepted`. `query-wide-count` and `query-wide-null` are two admitted reads. They are not a trusted-query signoff. The product-wide full 87 parent goal is unchanged.

## Commands

Archive smoke, before the chain: `linux/smoke/launch.sh`.

Live chain: `linux/chain/launch.sh`. One 6 GiB builder and one private PostgreSQL. The builder was removed when apply exited 1.

Inventory, after this handoff text: `linux/inventory/launch.sh`. One 6 GiB builder, no second PostgreSQL. The walker binary is the one the chain installed at `/cargo-home/bin/sha256_manifest`.

Owned cleanup, after the inventory: `cleanup.sh` in this directory. It writes `cleanup.log` here. It does not rewrite the parent `cleanup.log`.

## Left in place until cleanup

`oo-a317-pg`, volume `oo-a317-eef831aa-pgdata`, network `oo-a317-eef831aa-net`, cargo volume `oo-a317-eef831aa-cargo`, target volume `oo-a317-eef831aa-target`, and `/tmp/oo-a317-eef831aa-dba4-4af2-a78b-541bc9e0a317`. Cleanup removes only `oo.owner=a317` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a317`. It does not prune and it does not name other owners.

## Outside this line

New pipeline integration waits until those units are approved on their own. This directory does not close them.

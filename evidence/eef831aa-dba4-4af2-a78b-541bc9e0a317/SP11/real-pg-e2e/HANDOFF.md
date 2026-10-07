# Handoff

Owned paths:

- `testing/real-db/pg-e2e/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a317/SP11/real-pg-e2e/**`

No product path was edited. Product bytes under test are `git archive` of `10ecd7654fd7939b6655a0e50a84214a7fca316e`. Dirty worktree files were not mounted as the compiler source. a313 remains the only root manifest and lock writer.

## What the next reader should open

- Frozen contract: `testing/real-db/pg-e2e/CONTRACT.md`. It stays frozen. The amount column stays `numeric(18,4)`. `national_id` stays unclassified.
- Contract review, before the driver: `reviews/contract.md`.
- Driver review, before the live chain: `reviews/driver.md`. It says the chain had not run yet. That sentence is historical.
- Chain review, after the failure: `reviews/chain.md`.
- Recipe origins: `origin/recipes.txt`.
- Pre-edit archive smoke: `linux/smoke/`. `smoke.exit` is 0. `passed=24 failed=0 ignored=1`.
- Driver failures kept: `linux/focused-attempt-1` through `linux/focused-attempt-5`.
- Passing driver tests and Clippy: `linux/focused/`. `focused.exit` is 0. `passed=9 failed=0 ignored=0`.
- The one live attempt: `linux/chain/` and `live/run/`. Diagnosis: `linux/chain/DIAGNOSIS.txt`. Apply outcome: `live/run/apply-outcome.json`.
- Policy request that the product refused: `live/run/steps/reviewed-policy-wide.stdin`.
- Archive functions that refuse it: `source/blobs/reviewed_policy.rs` and `source/blobs/reviewed-policy.rs`. Blob ids match `10ecd765`.
- Privilege and cancel outputs: `live/side/`. One cancel already ran.
- Measurements: `live/measurements/import.txt` and `live/measurements/pg-stats.txt`.
- Inventory: `SHA256SUMS`, from `tools/sha256_manifest.rs` via `linux/inventory/launch.sh`. The walker skips `SHA256SUMS`. Files written after the walk are listed in `linux/inventory/NOTE.txt` and are absent from the sums. `cleanup.log` is one of those files.

## Blocking seam for the coordinator

Phase `apply`. Parent exit 1. Stderr `pg-onboarding: reviewed-policy-wide exit`. Child `reviewed-policy-wide` exit 11. Stderr `reviewed-policy: semantic`. Stdout empty.

Cause in the fixed archive: `physical_type` / `base_type` rejects catalog type `numeric` on allow-list column `shop.entry.amount`. Exit 11 is `ReviewedPolicyError::Semantic`. `int8` quantity is compatible. Hidden `national_id` is not on the allow list.

Do not retune the frozen decisions, the row count, or the expected query exits to obtain a zero apply exit. Do not hand-write target rows, pack bytes, or query output. A product change that admits `numeric` needs its own later review. The same archive's `query-pg` result decoder still refuses numeric oid 1700; this chain did not reach that call.

M1-I01 and M1-I02 stay `not_accepted`. A passing trusted-query fixture was not produced. The product-wide full 87 parent goal is unchanged.

## Commands

Archive smoke, before the driver existed: `linux/smoke/launch.sh`.

Focused driver tests, after the contract review: `linux/focused/launch.sh`.

Live chain, after `focused.exit` was 0: `linux/chain/launch.sh`. One 6 GiB builder and one private PostgreSQL. The builder was removed when apply exited 1. The database was left up until `cleanup.sh`.

## Left in place until cleanup

`oo-a317-pg`, volume `oo-a317-eef831aa-pgdata`, network `oo-a317-eef831aa-net`, cargo volume `oo-a317-eef831aa-cargo`, target volume `oo-a317-eef831aa-target`, and `/tmp/oo-a317-eef831aa-dba4-4af2-a78b-541bc9e0a317`. `cleanup.sh` removes only `oo.owner=a317` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a317`. It does not prune and it does not name other owners.

## Outside this line

New pipeline integration waits until those units are approved on their own. This directory does not close them.

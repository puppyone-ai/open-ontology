# Handoff

Owned paths for this rerun:

- `testing/real-db/pg-e2e/**` (unchanged from `f4381843c7f376f196bc30c78c18d9701dc29a6d`, tree `79e2ddc120b2117f90d3e79ff61571490243f23b`)
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a317/SP11/real-pg-e2e/in-approved-rerun/**`

No product path was edited. Product bytes under test are `git archive` of `6f87942f1c66ba1a9787b7d7194e58a507af63ef`. Dirty worktree files were not mounted as the compiler source. a313 remains the only root manifest and lock writer.

The parent `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, and `cleanup.log` record the first chain on `10ecd7654fd7939b6655a0e50a84214a7fca316e`. `numeric-approved-rerun/` records the second chain on `21cc96241a841b47dbbcb116549030e38a643ba0`. They were not rewritten.

## What the next reader should open

- Frozen contract: `testing/real-db/pg-e2e/CONTRACT.md`. `wide-qty` stays expected exit 0 with `WHERE entry_id IN (0, 1, 2)`.
- This rerun: `REPORT.md`, `FINDINGS.md`, `live/run/apply-outcome.json`, `live/work/probe.stdout`, `live/run/steps/query-wide-qty.stdout`.
- Archive function that admits the statement: `source/blobs/sql_guard.rs`. Blob id `6d7aef7bb95c9550a7e6f1bc7f3f22f7c098de4f`. `admit_op` at the `AEXPR_IN` branch calls `admit_in`. Other blob ids are `source/BLOBS.txt`.
- Recipe origin note: `origin/recipes.txt`.
- Pre-chain archive smoke: `linux/smoke/`. Docker exit 0. `pg-onboarding` totals 24 passed and 1 ignored (`live_chain`).
- Focused owned driver: `linux/focused/`. Docker exit 0. 9 passed. Clippy exit 0.
- The one live attempt: `linux/chain/` and `live/run/`. `chain.exit` is 0.
- Privilege, cancel, and restart outputs: `live/side/` and `linux/chain/watch.log`. One cancel already ran. One restart already ran.
- Target rows: `live/actual/*.copy`. Independent gold check phases `check-copy-party`, `check-copy-account`, and `check-copy-entry` are 0.
- Measurements: `live/measurements/import.txt`, `live/measurements/probe.txt`, `live/measurements/pg-stats.txt`, and the pack digest files.
- Inventory: `SHA256SUMS`, from `tools/sha256_manifest.rs` via `linux/inventory/launch.sh`. The walker skips `SHA256SUMS`. Files written after the walk are listed in `linux/inventory/NOTE.txt`.

## Coordinator seam

The previous blocking child on archive `21cc962` was `query-wide-qty` exit 6, stderr `query-pg: guard`. On this archive that child exit is 0 and the three gold int8 cells came back. Target COPY, fresh query, numeric probe, hidden and write guards, two-role pack digests, ACL installs, cancellation, and restart all have exit files, and the chain phases that gate them are 0.

No new failing phase was produced. The outcome file still records the open parent gates:

- `m1_i01` = `not_accepted`
- `m1_i02` = `not_accepted`
- `semantic_publication` = `missing`
- `public_auth` = false
- `authentication` = `not_claimed`
- `authorization` = `not_claimed`
- `harness_mount` = `not_performed`
- `wal` = `missing`
- `natural_language` = `missing`
- `analyze_published` = false
- `estimate_is_exact` = false
- `database_release` = false

Leave the frozen statement, the decisions, the 100000-row formulas, and the expected exits in place. A passing trusted-query status on `trusted_local_admin` is not M1-I01 or M1-I02 signoff. The product-wide full 87 parent goal is unchanged.

`crates/oo-gateway/src/pack_catalog.rs` differs between `21cc962` and `6f87942`. This chain consumed the compiled bundle from `6f87942` and checked the two digests. It did not open a pack-catalog review.

## Commands

Archive smoke, before the chain: `linux/smoke/launch.sh`.

Focused owned driver, after smoke and before the chain: `linux/focused/launch.sh`.

Live chain: `linux/chain/launch.sh`. One 6 GiB builder and one private PostgreSQL. The builder was removed when the chain exited 0. PostgreSQL stayed up for the COPY, the fresh query, and the restart.

Inventory, after this handoff text: `linux/inventory/launch.sh`. One 6 GiB builder, no second PostgreSQL. The walker binary is the one the chain installed at `/cargo-home/bin/sha256_manifest`.

Owned cleanup, after the inventory: `cleanup.sh` in this directory. It writes `cleanup.log` here. It does not rewrite the parent `cleanup.log` or the numeric-approved `cleanup.log`.

## Left in place until cleanup

`oo-a317-pg`, volume `oo-a317-eef831aa-pgdata`, network `oo-a317-eef831aa-net`, cargo volume `oo-a317-eef831aa-cargo`, target volume `oo-a317-eef831aa-target`, and `/tmp/oo-a317-eef831aa-dba4-4af2-a78b-541bc9e0a317`. Cleanup removes only `oo.owner=a317` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a317`. It does not prune and it does not name other owners.

## Outside this line

New pipeline integration waits until those units are approved on their own. This directory does not close them. Integration parents stay unaccepted while the outcome fields above stay open.

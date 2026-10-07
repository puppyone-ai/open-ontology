# Handoff

Owned path for this review:

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a349/SP11/real-pg-100k-review/**`

No product path was edited. Product bytes under test are `git archive` of `6f87942f1c66ba1a9787b7d7194e58a507af63ef`. The fixture mounted read-only is `testing/real-db/pg-e2e` at tree `79e2ddc120b2117f90d3e79ff61571490243f23b`. a313 remains the root manifest and lock writer.

## What the next reader should open

- Frozen contract: `testing/real-db/pg-e2e/CONTRACT.md`.
- This review: `REPORT.md`, `FINDINGS.md`, `oracle/result.txt`, `live/run/apply-outcome.json`, `live/work/probe.stdout`, `live/run/steps/query-wide-qty.stdout`.
- Independent formula: `oracle/oracle.rs`. It does not link `pg-e2e`.
- Archive blobs: `source/BLOBS.txt` and `source/blobs/`.
- Recipe origins: `origin/recipes.txt`. `pg-wrap.sh`, `extract-libclang.sh`, `tools/sha256.rs`, `tools/sha256_manifest.rs`, and `tools/select-debs.rs` keep the reviewed digests named there. `extract-libclang.sh` is the a274 byte copy and writes `/cargo-home/a274-clang-root` on this session's cargo volume.
- Pre-chain archive smoke: `linux/smoke/`. Docker exit 0. `pg-onboarding` totals 24 passed and 1 ignored (`live_chain`). Toolchain `rustc 1.94.0`.
- Focused owned driver: `linux/focused/`. Docker exit 0. 9 passed. Clippy exit 0.
- The one live chain: `linux/chain/` and `live/run/`. `chain.exit` is 0. Oracle phase exit is 0.
- Privilege, cancel, and restart: `live/side/` and `linux/chain/watch.log`.
- Target rows: `live/actual/*.copy`.
- Measurements: `live/measurements/import.txt`, `live/measurements/probe.txt`, `live/measurements/pg-stats.txt`, and the pack digest files.
- Inventory: `SHA256SUMS`, from `tools/sha256_manifest.rs` via `linux/inventory/launch.sh`. The walker skips `SHA256SUMS`. Files written after the walk are listed in `linux/inventory/NOTE.txt`.

## Coordinator seam

The author rerun on this archive already recorded a passing chain. This directory is a separate private database, a separate builder volume, and a separate oracle. The outcome file still records the open parent gates:

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

A passing trusted-query status on `trusted_local_admin` is not M1-I01 or M1-I02 signoff. The product-wide full 87 parent goal is unchanged. No new counted unit was opened.

## Commands already run

Archive smoke, before the chain: `linux/smoke/launch.sh`.

Focused owned driver, after smoke and before the chain: `linux/focused/launch.sh`.

Live chain: `linux/chain/launch.sh`. One 6 GiB builder and one private PostgreSQL. The builder was removed when the chain exited 0. PostgreSQL stayed up through COPY, the fresh query, and the restart.

Inventory follows this handoff: `linux/inventory/launch.sh`. One 6 GiB builder. The walker binary is the one the chain installed at `/cargo-home/bin/sha256_manifest`.

Owned cleanup follows the inventory: `cleanup.sh` in this directory. It removes only `oo.owner=a349` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a349`. It does not prune and it does not name other owners.

## Left in place until cleanup

`oo-a349-pg`, volume `oo-a349-eef831aa-pgdata`, network `oo-a349-eef831aa-net`, cargo volume `oo-a349-eef831aa-cargo`, target volume `oo-a349-eef831aa-target`, and `/tmp/oo-a349-eef831aa-dba4-4af2-a78b-541bc9e0a349`.

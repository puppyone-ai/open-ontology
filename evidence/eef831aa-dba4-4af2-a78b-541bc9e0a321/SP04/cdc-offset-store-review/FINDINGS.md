# Findings

Reviewer: this session. No subagent. No product edit. Scope stayed the frozen mirror at `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`.

## What the proof shows

Preoverlay repeated the author's contract tests, dollar-block splitter check, `live_chain`, and Clippy on the fixed archive before the review file was copied in. Those exits are 0. The author result was not reused as this review's proof.

The independent chain uses a fresh private TLS cluster, two databases, and the fixed target scripts `0000`, `0001`, and `0003`. `0002` is hashed and left unapplied. `0004` is not in the archive. Receipts enter `_oo.apply_receipts` only through `commit_apply_receipt`. The product login selects that table and `_oo.schema_migrations`. It does not write the target.

Install as `oo_cdc_offset_admin` returned `installed`, then `replay`. A reader insert, a reader select of `target_generations`, a runtime connect to the target, a runtime select of a foreign schema, and a runtime `CREATE TABLE` in `oo_cdc_offset` returned `42501`. An admin update of `receipt_hash` returned `23514`.

Exact replay of the same hash kept one row. A changed payload on the target returned `OO010` and left the stored mirror. A missing target receipt returned `target_receipt_absent` and wrote no control row. A partial segment was stored with `watermark_hold` `partial_segment` and the watermark position stayed put. The next transaction, with the watermark still expecting the missing successor, was stored with `source_gap` and did not advance to the later position. A later epoch with the previous epoch's watermark in `expected_previous` held `epoch_barrier` and left the older epoch. A following call with `expected_previous` null inserted the new epoch row.

Caller `source_gap` true on an otherwise complete next transaction held the watermark and left `source_applied` false. The same call with the flag false advanced. Caller `epoch_barrier` true held, and the same call with the flag false advanced. Those flags are caller inputs. They are not a source certificate. The author live helpers send them false; this probe is the live call that was missing from the author findings.

A second session inserted the same receipt hash and left the transaction open. The product insert waited on the lock, then received unique violation `23505` after that session committed. The CLI exited 2 with `fault` `receipt_hash_conflict`, `durable_control_mirror` false, one stored row, and the watermark unchanged. A later single-writer call replayed that row and held `source_gap` because the raced transaction is not the declared next transaction. `linearizable_multi_actor` in the counts file is false. This is the single-writer conflict label, not a multi-actor linearizability claim.

`CDC_OFFSET_INJECT=hide_commit` dropped the COMMIT reply after the server had the row. Lookup returned kind `replay` and the watermark moved to that transaction. `CDC_OFFSET_RECONCILE=down` on a later hidden commit returned exit 3 `commit_unknown` and did not insert a second row. The follow-up call was a replay of the partial row.

`external_xack`, `external_xtrim`, `ack_allowed`, `source_applied`, and `fence_proof` stayed false on the asserted outputs. A scan of `src/` found no `XACK`, `XTRIM`, `VALKEY`, `valkey`, `XADD`, or `XGROUP`. `Cargo.toml` has no `cdc-apply-pg` dependency and no path dependency. `checkpoint_provenance` is `caller_declared_pending_real_normalizer`.

After three postmaster restarts, the final one changed `pg_postmaster_start_time` from `2026-10-07 11:29:08.144051+00` to `2026-10-07 11:30:11.128223+00`. The 10 receipt rows and both watermark rows were still the recorded values. Replaying the epoch-1 head returned the stored hash and did not move either epoch.

## What remains parent work

Source acknowledgement, a real normalizer, Valkey `XACK` and `XTRIM`, the reject ledger, and large-segment integration stay parent work. The ledger and plan checkboxes for CDC-D05 were not changed.

An uncommitted root `Cargo.toml` hunk for axum, tower, and tower-http was already in the worktree. This review did not write it and does not commit it. a313 remains the root writer.

## Attempts kept

`linux/overlay-attempt-1`, `linux/restart-attempt-1`, and `linux/restart-attempt-2` keep their logs and exit files. The passing runs are `linux/preoverlay`, `linux/overlay`, and `linux/restart`.

# Findings

Verdict: **APPROVE LIMITED** the unpublished full import at `cc764cbc5f9d5d2cf6056a2c41ff2612161b5ff4`.

No product change is requested. The fresh oracle covered the composition claims. Reused primitive diffs are empty except the additive installer.

## Fence failure leaves the run open

`ImportSession::fail_fence` (`full_import.rs` around the `fail_target_generation` call) marks the shard failed and unlocks. It does not call `retire_run`.

On shard `a159take` the live run was `open` after `fail_fence`. A later `pull_and_apply` on the old connection failed with `OO011`, `OO018`, or `OO013` (the harness accepts only those three, and the test passed). `finish` then returned `reader is not exhausted`. The harness ignores a failed retire inside that branch (`let _ = self.retire_current`). The stored state stayed `open`. It did not become `awaiting_publish`.

`contracts/pipeline/full-import-v1.md` retires a run for cancel, a source deadline, a lost snapshot, or a typed refusal. Fence failure is not in that list. Cancel, deadline, source loss, the oversized read, and finish-before-exhaustion all stored `retired` in this run. The open row cannot be sealed by the old connection. This is recorded as a lifecycle fact. It is not a change request.

## In-memory `RunSummary`

`RunState` and `RunSummary` have public fields (`full_import.rs`). A Rust caller can construct `AwaitingPublish` in memory. The comment above `RunSummary` says callers cannot. The database row is the seal. This oracle's coverage gap stored `retired`. An unbound `commit_apply_receipt` did not increase receipts or `awaiting_publish` rows. Install created zero runs. `RunComplete` is the reader token from `finish_complete`; this review did not rebuild that type.

## What the approval leaves out

Publication count on the target was 0. The Agent could not read the shadow or the quality table. The stats grant does not replace the pid, owner, generation, or shard-lock checks. jsonb oid 3802 was refused and did not create a shadow. The approved oid list is `full_import.rs` `APPROVED_OIDS` (16, 17, 20, 21, 23, 25, 114, 700, 701, 1042, 1043, 1082, 1083, 1114, 1184, 1700, 2950). That list is not a full PostgreSQL type certificate.

a152's second-database limit still stands: a cluster that already has `oo_target_owner` is not a fresh bootstrap. This review dropped those roles between the installer fixture and the import fixture. It did not rerun the a152 matrix.

`SnapshotFinished` is banned as a contiguous string by `hook_digest_is_sixty_four_hex_and_snapshot_finished_is_not_used`, which the smoke library tests passed. The entry calls `finish_complete`.

## Attempt 1

`linux/probe/attempt-1/test.stderr` is an overlay compile failure: `create_login` borrowed `self` twice, `ImportSession` is not `Debug`, and `query_scalar` required a `'static` query. Exit 101. The product archive was not modified. The green probe is attempt 2 in `linux/probe/`.

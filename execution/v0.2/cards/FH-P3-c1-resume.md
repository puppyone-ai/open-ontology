## Card FH-P3 — C1 per-table isolation and schema classification (S6-M-06, S6-M-07; D22)

Branch `v0.2/fh-p` reset to `993d1162a8cbbb13aae5d4d7ee156477c346d708` (your passes 1–2 merged; installer already applies 0129 and 0135 — do not edit
`crates/oo-pipeline/src/bin/install-retained-target.rs`). Same ownership and commit rules as pass 2 (owner identity,
no agent trailers, `Plan: S6-M-06 S6-M-07`; migrations 0130–0134 only). Read `tools/oo-load/VALIDATION-FH-P2.md`
(your C1 semantics section) first.

Implement for the C1 (log CDC) path on PostgreSQL (pgoutput), MySQL (binlog, feature mysql-binlog) and Oracle (LogMiner):
1. Per-table isolation with the same per-table status/stale fields and exit codes 0/3/4 as C5.
2. Quarantine semantics you defined: a stale C1 table's position is frozen; events for it are not applied; the shared
   source position advances only for healthy tables without losing the quarantined table's events (it rejoins through
   an aligned per-table rebaseline/C5 correction, with no gap and no duplicate — prove it in tests).
3. Use the shared classifier for C1 DDL/shape changes with exactly the C5 classes (additive, unpublished_only,
   same-type widening pass; target-type widening per-table rebuild; drift stale; unproven stale).
4. Tests: the five C1 T4 cases (add/drop/rename column, integer and string widening) and T9 isolation cases on the C1
   path must pass on PostgreSQL 17, MySQL 8.4 and Oracle 23 with one frozen binary; full t3t4 and t9_isolation on those
   three (sequential, ≤2 DB containers, oo.lane=fhp); T1 concurrent-writer on one engine per path to show no gap/no dup
   after a quarantine and rejoin; pipeline lib, oo-load default + mysql-binlog, connectors all-features, clippy, fmt.
List secret-review-bound files you change. Report: commits, semantics, per-case pass/fail per engine, gaps.


RESUME NOTE: your previous run of this card was cut off by a model-provider usage limit (429). The 19 uncommitted changes in this worktree are your own in-progress work: review them with `git status` / `git diff`, keep what is sound, and continue to the card's exit criteria. Do not reset the worktree.

## Card M-S4f — lane M: the scheduled lite cycle and seven healthy

TODO: S4-M-01 (last exit). Rebase on v0.2/product-loops first. Lane P removed your blocker: the Builder accepts retained
generations in its data-only cycle (tools/pg-builder/scripts/retained_build.py) and `release-coordinator
publish-retained` opens reviewed fences and activates (tools/release-coordinator/src/retained.rs). Grants as M-S4b.
Lane A3 is wiring `oo build` (crates/oo-cli/src/admin/**) — do not edit it.
1. deploy/lite scheduler/worker: each scheduled cycle runs `oo-load sync` → `oo-load publish` candidate → Builder
   retained data-only (zero model calls when only data changed; a semantic change stops the cycle with a named
   `review_required` status instead of publishing) → `publish-retained` reusing the owner-reviewed registry → write the
   gateway document/Pack → Gateway reload → `oo-load prune`.
2. Prune also closes/removes fence rows of pruned generations (lane P noted they stay open).
3. Live in deploy/lite on PostgreSQL 17.11: first `oo build` (or lane P's harness) to a published library, then one
   scheduled no-op cycle and one scheduled data-change cycle; `oo status` reports all seven services/jobs healthy
   (Builder as a completed one-shot) with measured timestamps; Gateway serves the new data. Evidence.

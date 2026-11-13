## Card T10-mitigate-2 — chunk-consistent reads (coordinator decisions) and finish T10 (S6-H-02; D24)

Branch `v0.2/t10` (committed; never delete this card). Your handoff `tools/oo-test/T10-MITIGATE.md` was right to stop
at the proof. The coordinator decides:

D-a. Keyed tables on PostgreSQL (initial load and C5): drop the exported snapshot owner. Each chunk is ONE short
     `REPEATABLE READ READ ONLY` transaction that reads the range payload and computes that range's digest proof over
     the same rows inside the same transaction (or derives the digest from exactly the transferred rows with the same
     canonical digest function). The C5 "before digest / payload / after digest" sequence across separate snapshots
     is replaced by this single-snapshot-per-chunk proof, which removes the A-to-B-to-A gap. Bound each chunk
     transaction by rows, bytes and elapsed seconds (record the bound). No-key physical (ctid) plans keep the
     exported snapshot; document that as a residual (C1 already requires keys).
D-b. Consistency class "chunk_consistent": every range is point-in-time at its own chunk transaction; skew across
     ranges is bounded by the cycle; the next C5 cycle corrects. Record it in receipts/publication metadata. It is
     the lite default for keyed PostgreSQL and MySQL tables; `snapshot: single` stays an explicit option.
     MySQL already reads per chunk: apply the same same-transaction digest/payload proof there.
D-c. MySQL T10 C1 preparation must not use the handoff repair path (its 100,000-key / 32 MiB cap stays). Start C1
     from the binlog file/position captured at the end of the initial load via `log_sync` `initial_position` +
     `baseline_aligned` (the existing aligned-baseline path), as T2 does.
D-d. T10 source-only settings: PG `max_slot_wal_keep_size=-1` on the T10 container; Oracle preparation/initial
     timeouts = 3x (rows / measured rows-per-second), min 900 s. Record both. Never mask a product error.

Tests: unit tests for chunk bounds and same-transaction proof; adversarial tests with updates, deletes and key moves
between chunks (published generation stays digest-consistent per range; next cycle converges to the source).
Live: T1 and T2 at full scale on PG 17 and MySQL 8.4 (tools/oo-test t1/t2_p0/t2, P0 fixtures).
Then re-measure all T10 phases (baseline, initial, initial_duty100, c1, c5 idle/changed at both policies) on Oracle 23,
MySQL 8.4, PostgreSQL 17 at the 50% foreground rate; give before/after against live-4 for the changed-C5 phase.
Update both "Production impact" docs with measured numbers and recommended settings.

Unrelated containers on the host are idle and must never be stopped, inspected beyond `docker stats`, or modified;
never write their names anywhere. Commit as the owner, no agent trailers, trailer `Plan: S6-H-02`. Do not stop at a
design question already decided above; if a NEW blocker appears, implement the safest bounded option, test it and
record it, then continue.
Report: per engine × phase table, before/after, correctness re-runs, commits, tests, residuals.

# S4-F-01 OLAP acceptance summary

Source commit `746398339`, `oo-load` debug binary sha256 prefix `1858326ab3a83dde`,
run on 2026-11-04 on the macOS arm64 host (native arm64 images, no emulation).
Raw evidence stays in the ignored `evidence/<run-id>/<family>/`; the hashes
below are of each run's `result.json`.

| Family | Version (pin) | Run id | T0 | T1 | T2 | T8 | Transport | result.json sha256 |
|---|---|---|---|---|---|---|---|---|
| ClickHouse | 26.8.21.10 (`clickhouse-26.8.21.10`) | `f-s4-final-clickhouse` | passed | passed | passed | passed | TLS | `49d54c6b1ccade45b3fab8eaaffa2a345ffc27d7a99f0222213e71fff9430cbc` |
| Doris | 3.1.4 (`doris-fe-3.1.4`, `doris-be-3.1.4`) | `f-s4-final-doris` | passed | passed | passed | passed | plaintext loopback | `6ee0e4494982a5254ad8b0b7ff3a1b3b11cf517cb963665f3466d61cd394ec52` |
| StarRocks | 3.5.21 (`starrocks-allin1-3.5.21`) | `f-s4-final-starrocks` | passed | passed | passed | passed | plaintext loopback | `680fcb1e44fbd5444eec3206431687dd0e93f1e0580a84eee7e6e4e37ae612ac` |

What each run showed:

- T0: exact version identity; the missing last grant was graded `needs_setup`
  with a DBA statement (ClickHouse `GRANT SELECT ON system.mutations`, Doris
  `GRANT SELECT_PRIV`, StarRocks `GRANT SELECT ON ALL TABLES IN DATABASE`);
  then every table planned with adjacent coverage and three partition-aligned
  ranges on `events`, every chunk read within the 4,000-row budget, digests
  deterministic and equal to receipt rows, and a split covering its parent.
- T1: four (ClickHouse) or five (Doris, StarRocks) tables, 30,000-row
  partitioned `events` in 12 chunks, equal to the source as normalized row
  multisets after a load under a concurrent insert/update/re-key/delete writer
  (5, 8 and 4 writer cycles), a corrected load and a no-rewrite replay. Type
  edges include UInt64 max, Int128/LARGEINT, Float32 widening, -0.0, NaN/Inf
  (ClickHouse), DECIMAL(38,10), non-UTF-8 bytes, Date32 1900, nested types as
  declared engine text, and an excluded aggregate-state column.
- T2: baseline plus three `--c5` partition-refresh cycles. Cycle 2 reread only
  the two changed `events` ranges and the one changed `growth` range (for
  ClickHouse 383,120 of 2,274,528 snapshot bytes); cycle 3 read, replaced and
  wrote nothing. A schema change was refused as
  `c5_rebaseline_required_lineage` with the target unchanged; `--sync` was
  refused as `<family>_c1_unsupported_use_c5`.
- T8: peak one reader connection in every loader run; sampled duty 0-14.4%;
  engine busy/rest accounting 24.1-24.8% against the 25% budget; model tokens 0.

Limits of this evidence: one patch per family (ClickHouse 24.x LTS, Doris 4.x
and StarRocks 4.x are declared in the version rules but not exercised); small
fixtures (30,000-row largest table, not the 2,000,000-row scale fixture);
Doris/StarRocks fixtures without TLS; consistency `B`; T3-T7 not run.

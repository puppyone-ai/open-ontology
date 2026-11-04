## Card E-S5c — lane E: typed Oracle columns through the SourceEngine path

TODO: S5-E-04 (prerequisite) and S5-G-01. Rebase on v0.2/product-loops first. Lane G's gold dialogue found
(`tools/oo-test/T6-COMPOSITE-RERUN-VALIDATION.md` on branch v0.2/g-selfbuild-budget, diagnosis 1): the Oracle
SourceEngine (`crates/oo-connectors/src/oracle/engine.rs` `chunk_schema` and its page flush) emits UTF-8 for every
column, so NUMBER/DATE/TIMESTAMP land in the Agent DB as text; the Gateway then refuses SUM and orders lexically
(MIN/MAX `100.125,99.125` vs gold `1.125,120.125`). The typed `oracle/arrow_part.rs` mapping exists but this path does
not use it. Fix the producer, not the Gateway cast fence.
1. SourceEngine emits typed Arrow with the same mapping as arrow_part: NUMBER(p,s) → decimal128(p,s) (p ≤ 38),
   unconstrained NUMBER → the lossless decimal-text form the PG path uses (DecimalUtf8, lands as text by design — say
   so in the chunk schema), INTEGER-like → int64 where exact, BINARY_FLOAT/DOUBLE → float64, DATE/TIMESTAMP →
   timestamp_micros (naive), TIMESTAMP WITH (LOCAL) TIME ZONE → timestamp_micros_utc, RAW → binary, CLOB/NCLOB → utf8,
   BLOB → binary. Digest and C5/C1 paths use the same typed values (digests change → state the rebaseline). Unit tests.
2. Rerun Oracle 23.5 / 21.3 / 11.2 T1 (Agent DB comparison now typed) and T2 (C5; C1 opt-in on 23) through oo-load,
   plus a typed check: SUM/MIN/MAX/ORDER BY over a NUMBER column through the published Gateway equal the source.

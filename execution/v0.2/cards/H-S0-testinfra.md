## Card H-S0 — lane H: shared fixtures, gold questions, oo-test, image pins, amd64 runner

TODO: S0-H-01, S0-H-02, S0-H-03, S0-H-04, S0-H-05. Lane label: `h`. Spec: REQ-ENG-04 (test side).

Read: docs/product/05-version-matrix-and-testing.md (§2 images, §3 fixture shapes, §4 suites, §5 comparison,
§6 entry points), docs/13-evaluation-and-benchmarks.md §4.5, testing/infra/ (compose, images.pins,
images.lock.json, builders/run-matrix.sh), `crates/oo-connectors/src/engine/mod.rs` JSON shapes
(TableRef, ChunkReceipt, Watermark, RangeDigest).

Deliver:
1. S0-H-01 deterministic neutral fixture generator in testing/engines/seed-common/ (Python 3 stdlib or a
   small standalone tool): one logical model (orders, shipments, customers, org hierarchy region→area→site,
   code tables, calendar, events) with every shape in 05 §3 — int PK; varchar PK with case-insensitive
   collation and case/accent/trailing-space/emoji edges; composite PK; unique-only; no-key; ≥2,000,000-row
   table (`--scale`); 300-column table; `--tables N` small tables for large-schema runs; monthly shard family;
   audit/backup tables; declared FKs and undeclared implicit relations; exact decimals; unsigned ints;
   timestamps with/without tz; LOB/JSON; NULL vs empty string; sensitive-looking synthetic columns. Dialect
   DDL + bulk load for MySQL 5.7/8.x, PostgreSQL 13–18, Oracle 11g/21c/23ai in testing/engines/<engine>/seed/.
   Data is generated at run time; nothing >1 MiB committed.
2. S0-H-02 20 gold questions in testing/engines/gold/ (count, multi-join, group/aggregate, time range, NULL
   handling, code translation, multi-hop entity relations), gold SQL per dialect, expected answers computed
   on the source; answers must agree across the three engines for the same seed.
3. S0-H-03 `tools/oo-test` (standalone tool with its own lock, or Python): `oo-test matrix --engine
   mysql|postgres|oracle --versions ... --suites T0[,T1..]` — start pinned container (labels oo.run_id,
   oo.lane=h), wait healthy, seed, run suite, write evidence/<run-id>/ (raw exits, comparison report, resource
   receipt, SHA256SUMS), clean up only its own resources. A reusable comparison oracle (05 §5: canonical value
   normalization, per-chunk ordered hash, first differing key) other lanes call; document its API. T0 = start
   + seed + version identification + gold answers; T1+ are hooks engine lanes fill.
4. S0-H-04 digests in testing/infra/images.pins + images.lock.json (same format) for mysql 5.7.44 (amd64),
   8.0.x, 8.4.x, 9.x; postgres 12–18 bookworm; gvenzl/oracle-free 23-slim; gvenzl/oracle-xe 21-slim and 11
   (amd64). Use `docker buildx imagetools inspect`.
5. S0-H-05 `--platform linux/amd64` automatically for amd64-only images, longer health timeouts, evidence
   marked "emulated"; `--runner remote` stub for a future SSH host.
Exit: `oo-test matrix` runs T0 for real on mysql 5.7.44/8.0/8.4/9.x, postgres 13–18, oracle 23ai/21c/11g
(one at a time), with evidence.

Write scope: testing/engines/seed-common/**, testing/engines/gold/**, testing/engines/*/seed/**,
tools/oo-test/**, testing/infra/images.pins, testing/infra/images.lock.json. Prefix `testing` or `oo-test`.

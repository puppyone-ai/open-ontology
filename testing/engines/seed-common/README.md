# Neutral logistics fixture

```sh
python3 testing/engines/seed-common/generate.py --engine mysql --scale 2000000 --tables 200
python3 testing/engines/seed-common/generate.py --engine oracle --version 11g
python3 testing/engines/seed-common/generate.py --engine postgres --growth-after 100 --growth-count 20000
```

SQL is generated at runtime and sent only through the **fixture admin**
connection, never a product source reader. The event bulk load is one
server-side INSERT SELECT over bounded digit products, not two million
client INSERTs. Data/DDL are deterministic; scale changes only events and
the initial growth table count. Supply the version for Oracle capability
selection. The 120-order/24-customer semantic model is independent of scale.

Shapes: identity/sequence-backed integer PK (regions), collated string PK
(MySQL/PostgreSQL), composite PK, unique-only, duplicate no-key rows, 300
columns, configurable small tables, 12 monthly shards, audit/backup, views,
partitioned tables where available, actual declared FKs and intentionally
undeclared shipment-site/event-order links. Exact decimals include >64-bit
values; unsigned maximum is native MySQL and a checked decimal elsewhere.
LOB has 16,000 characters; binary, JSON, enum/set (native or compatibility
representation), timestamps, Unicode/emoji and NULL/empty values are present.
Sensitive-looking column names contain unmistakable `SYN-` values, not
real personal data.

The growth command appends deterministic IDs after the given high-water id;
invoke repeatedly from an independent admin workload between/during lane
tests. It does not install a permanent background writer. The default
starting growth table contains IDs 1..100 (or scale if smaller).

## Explicit dialect differences

- MySQL uses utf8mb4_unicode_ci. PostgreSQL uses ICU level-1 nondeterministic
  collation. They need not have identical trailing-space ordering.
- Oracle 11g has no data-bound column collation; all Oracle fixtures currently
  retain a binary VARCHAR PK plus an NLSSORT(BINARY_AI) unique index. This
  enforces linguistic uniqueness but **does not prove collated-PK chunking**.
  The engine lane must test linguistic range ordering independently.
- Oracle 11g XE has no partitioning option: its named partition fixture is
  explicitly unpartitioned. 21c/23ai get real range partitions.
- Oracle empty string is NULL. Shared gold questions use NULLIF(note,'')
  explicitly, while type_edges preserves each engine's native semantics.
- Oracle JSON is CLOB (with IS JSON validation on 21c/23ai), not a claim of
  native JSON type support. MySQL uses JSON; PostgreSQL uses JSONB.
- MySQL stores UTC TIMESTAMP plus the original-zone column; PostgreSQL and
  Oracle also exercise timezone-aware timestamp types.

These compatibility cases are not native feature acceptance. Raw seed SQL
is not committed; generated SQL remains well below 1 MiB even at default
scale because the server expands the bulk rows.

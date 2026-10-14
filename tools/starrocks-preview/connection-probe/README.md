# Configured StarRocks native reader

This standalone reader reuses the existing StarRocks typed Arrow library while
obtaining host, port, database, user and password_file from one private JSON.
It does not inherit the old fixture's fixed database, user or table selection.

Run `starrocks-connection-probe --config CONFIG --out NEW_DIRECTORY`.
`source` contains those five fields and `expected_build`; `tables` is an
operator-approved array of `{name, columns}`. Budgets require max_rows,
max_bytes and deadline_ms. The output retains actual engine identity, schema,
reader grants, read audit, typed Arrow and a complete-read receipt. A row/byte
cap, unsafe type, schema drift, write grant or native error refuses the capture.

Optional `method: C4` requires a native Duplicate Key table, `partition`, and
its complete approved `observed_partitions` set. Source partition metadata must
be unchanged before/after the partition read. `derived_from`, explicit typed
columns and read-only `sql` describe a derived projection; these are never
claimed as a native source table or native constraint.

`producer.py` consumes the official native scheduler occurrence, calculates
source_generation from acknowledged_source_generation, performs a fresh
configured C5 read, and calls official source-admission seal-native. It does
not write target ACKs or task state. C4 producer support is intentionally
refused until its independently verified partition-scope recipe is complete.

Status: the first configured reader revision passed Linux unit/release checks;
its socket-preference fix has a fixed ELF. Current C4/derived extensions are
not yet built or runtime verified. This package alone does not constitute
S0–S10, Pack, Gateway, ongoing maintenance or dialogue completion.

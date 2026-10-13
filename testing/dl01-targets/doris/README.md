# DL01-W1-12 Apache Doris target preparation

Synthetic Apache Doris 3.1.4 FE+BE fixture for duplicate, aggregate, and unique keys, decimals, nulls, range partitions, and the absence of a stored binary key. The independent reader is the pinned MySQL protocol client. It is not a MySQL or StarRocks server, and a protocol `VERSION()` string is not the product identity.

Frozen `oo-connectors` at `577160c22de63adb9b38e0147f3bc2da893c4d9d` is PostgreSQL-only (`oo.source.pgsql/v1`). This directory does not implement a Doris connector and does not promote the support matrix. Downstream catalog, copy, Builder, and consumption wait for a committed shared interface.

No Rust builder is assigned. Linux/MSRV 1.94 applicability stays pending until a Rust consumer. Do not start `mysqld`.

```bash
testing/dl01-targets/doris/scripts/probe-image.sh
testing/dl01-targets/doris/scripts/admit-and-start.sh
testing/dl01-targets/doris/scripts/setup-dba.sh
testing/dl01-targets/doris/scripts/journey.sh
testing/dl01-targets/doris/scripts/cleanup.sh
```

Passwords are created under `/tmp/oo-dl01w1-12-secrets` and are not arguments, logs, or Git files. DBA (`oo_dba`) loads the fixture. Reader (`oo_reader`) has select only. Resource names use the prefix `oo-dl01w1-12`.

`reader/compare.py --self-check` checks the comparator only. It is not capability acceptance. The oracle is `fixtures/expected.json`, written from the documented key models before the engine run.

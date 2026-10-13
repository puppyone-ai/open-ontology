# StarRocks preparation fixture

DL01-W1-13 prepares one real StarRocks target. It does not promote the support matrix, does not accept DBX-W12, and does not implement the shared source consumer.

The product is StarRocks. The FE query port speaks the MySQL wire protocol. A MySQL, MariaDB, or Apache Doris server is a different product and fails the identity gate in `scripts/journey.py`.

## What is pinned

See `PIN.md`. The runnable image is `starrocks/allin1-ubuntu` digest `sha256:9b98696ee0cb46831eabe197ae5e71f483ee3c2e16ab57fd27cb51a5c9ee6618` (tag 3.5.21, `linux/arm64`). One container runs one FE and one BE with `replication_num=1`, capped at 1 CPU and 4GiB. No host port is published.

## Fixture

`sql/dba-load.sql` is executed by `oo_dba`. `sql/reader-positive.sql` is executed by `oo_reader`, which is granted `SELECT` only. Passwords are generated at runtime under `/tmp/oo-dl01w1-13-secrets` and are not stored in Git.

`oracle/expected.json` states the visible rows for duplicate, primary, aggregate, and unique key models, including decimal scale, NULL, empty string, and zero. Partition sums for `dup_events` are part of that file. The server result is compared to the file. The file is not produced from the server.

## Run

```sh
python3 testing/dl01-targets/starrocks/scripts/test_oracle_unit.py
python3 testing/dl01-targets/starrocks/scripts/journey.py
```

The unit test does not start a database. The journey records a runtime preflight and does not compile Rust. Linux/MSRV applicability stays pending until a Rust consumer exists.

`scripts/cleanup.sh` removes only `oo-dl01w1-13-starrocks` and the `oo-dl01w1-13` network when their prefix label matches. It does not remove the image and it does not prune other containers.

## Not claimed

Catalog, Arrow, local PostgreSQL, Builder, and CLI/HTTP consumption wait for the fixed shared interface. There is no change-feed. Consistency grade for this fixture is S0. Cloud budget (AT-SRC-12) does not apply to this self-managed container. `verified` and `certified` are not claimed.

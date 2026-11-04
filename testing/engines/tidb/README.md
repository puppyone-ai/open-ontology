# TiDB owned fixtures

```sh
python3 testing/engines/mariadb/run.py --family tidb --run-id <unique-id>
```

Pins: `pingcap/tidb:v7.5.0`, `pingcap/tidb:v8.5.0`, linux/amd64.
The runner records the resolved digest and starts that immutable digest.
These fixtures use single-node unistore, not distributed TiKV/PD production.

Product: `oo_connectors::mysql::tidb::TidbEngine` with feature `mysql`.
Snapshots/C5 only; TiCDC is out of scope. No MySQL replication protocol is
assumed. READ ONLY is not enabled via TiDB no-op settings: SQL guards and a
SELECT-only account enforce source read behavior. PROCESS is additionally
required for local PROCESSLIST load measurement. Missing PROCESS is a probe
requirement, not an invented zero load value. A zero max_connections means
unlimited, not zero capacity. No cluster-wide load or historical-read claim.

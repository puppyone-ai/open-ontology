## Card E-S3b — lane E: SQL Server full-fixture acceptance and durable cycles

TODO: S3-E-02 (plan exit T0–T1). Rebase on v0.2/product-loops first: your adapter and the coordinator's doctor/export
wiring are integrated (942df0fc2). Deliver (keep going; commit as you go):
1. Add SQL Server to lane H's `tools/oo-test` runner/matrix (neutral generator shapes, 2M events, 200 tables, 300-column
   table; dialect DDL under testing/engines/sqlserver/seed) and pins (one patch per major, lane H's format).
2. T0 and T1 (Agent DB comparison with concurrent writer, through oo-load) and T8 on 2019 / 2022 / 2025, sequentially.
3. `oo-load --c5` and `--sync` (CT; CDC when enabled) arms for SQL Server with durable positions and source_epoch, using
   lane M's structure; T2 three C5 + three CT cycles on 2022; CDC on 2022/2025 once; T7 with CDC internal-object
   attribution per D12.
Lane C is concurrently editing `tools/oo-load/src/main.rs` for MySQL-family arms: keep your arm in its own module and a
one-line dispatch entry to keep the rebase small.

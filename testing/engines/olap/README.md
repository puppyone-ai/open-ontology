# Lane-F OLAP acceptance (ClickHouse, Doris, StarRocks)

`run.py` runs T0, T1, T2 and T8 for one family against fresh, `oo.lane=f`
containers: one source and one PostgreSQL Agent DB, never more. Run from the
worktree root, one family at a time:

```sh
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4
CARGO_TARGET_DIR=target cargo build --manifest-path tools/oo-load/Cargo.toml --locked
uv venv evidence/olap-env
uv pip install --python evidence/olap-env/bin/python PyMySQL==1.1.2
evidence/olap-env/bin/python testing/engines/olap/run.py --family clickhouse --run-id <id>
evidence/olap-env/bin/python testing/engines/olap/run.py --family doris --run-id <id>
evidence/olap-env/bin/python testing/engines/olap/run.py --family starrocks --run-id <id>
```

Images come from `testing/infra/images.pins` (native arm64). Doris runs FE and
BE in one container: the BE image with the FE image mounted read-only
(`--mount type=image`), so a Doris run is still one source container.
The runner refuses to start below 20 GiB free disk or while any lane-F
container exists, and removes only the containers it created.

Credentials are generated per run under the ignored
`evidence/<run-id>/<family>/.private/` and deleted at the end. They reach
servers through mounted files (ClickHouse users XML with hashes, PostgreSQL
password file), HTTP headers or stdin, never argv; command logs are redacted.
Fixture DDL and writes use a separate admin identity. The product reader is
`fixture_reader`: ClickHouse `readonly=1` profile with `max_execution_time`,
Doris `SELECT_PRIV`, StarRocks `SELECT ON ALL TABLES IN DATABASE`.

ClickHouse runs over TLS (generated CA, server certificate for 127.0.0.1;
`--plaintext` disables it). The Doris and StarRocks fixtures are plaintext on
loopback, which `oo-load` accepts only for a literal loopback host and reports
as `plaintext_loopback`.

| Suite | What runs | Pass condition |
|---|---|---|
| T0 | `crates/oo-connectors/tests/olap_live.rs` twice: before the last grant (ClickHouse `system.mutations`, Doris/StarRocks `SELECT`) and after | Exact version identity; missing grant graded `needs_setup` with a DBA statement; then every table planned with adjacent coverage, partition-aligned ranges, all chunks read under the budget, digests deterministic and equal to receipt rows, a split covering its parent |
| T1 | `oo-load` snapshot while a writer inserts, updates, re-keys and deletes; quiesce; corrected load; replay | Every table equal to the source as a multiset of normalized rows; replay rewrites nothing |
| T2 | Partition-aligned baseline, then three `--c5` cycles: change partition A, change partition B and the growth table, no change | Equal after each cycle; cycle 2 rereads only changed ranges; cycle 3 reads, replaces and writes nothing (tuple counters, relations, views); schema change refused as `c5_rebaseline_required_lineage` with the target unchanged; `--sync` refused as `<family>_c1_unsupported_use_c5` |
| T8 | Admin-side sampling of reader connections and active statements during every loader run, plus the engine's own busy/rest accounting | Peak one reader connection; sampled and engine duty cycle at most 25% |

`result.json` and `SHA256SUMS.json` in the run directory are the evidence
summary; `source-manifest.json` binds the run to the source files. Consistency
is `B` (no snapshot across chunks); model tokens are zero.

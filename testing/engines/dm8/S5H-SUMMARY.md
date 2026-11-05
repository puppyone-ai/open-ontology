# S5-E-03: DM8 product T0, T1, T8 and C5 on native arm64

Run: `d8-s5h-r2`. Date: 2026-11-05. Tested tree:
`95b05d6f57680312899d1b23b3d8cc409eea955f` (clean; verdict rows carry it as
`product_sha`); the branch was then rebased onto `v0.2/product-loops`
`50b625c71`, whose only Rust change since is in the Oracle engine, so the
DM8, oo-load and doctor code under test is unchanged. Server: owner-accepted (E340) image
`sha256:8ddae8147e7a3a3f9bb20c0988a35857dfcaf1bbf2944a1babc5a7f513274f75`,
`uname -m` = `aarch64` (native, not emulated). Measured banner
`DM Database Server x64 V8 03134284604-20260707-335949-20228` (DM prints
`x64` on this arm64 build), version major `8`.

```sh
python3 testing/engines/dm8/fixture.py create --run-id d8-s5h-r2 --full
python3 testing/engines/dm8/product_t0.py --run-id d8-s5h-r2 --binary target/debug/oo
python3 testing/engines/dm8/target_acceptance.py --run-id d8-s5h-r2 --c5
python3 testing/engines/dm8/fixture.py remove --run-id d8-s5h-r2   # cleanup 0/0
```

Fixture: 231 tables, 2,000,000 `EVENTS` rows, a 300-column table, the
27-column `TYPES` table, composite, decimal and character keys, an empty
and a keyless table. The reader `OOREADER` has only per-table `SELECT`.

## Verdict rows (`tools/oo-test/verdicts.py`)

| Suite | Variant | Result | Evidence sha256 |
|---|---|---|---|
| T0 | snapshot | pass | `947b38c5e88395bbdf403d3fd38ca0091914a43fef3035d9e892a2f2a4d0bfa7` (`product-t0/doctor-report.json`) |
| T1 | snapshot | pass | `465b79303f67119da94833350d3372fdf48294f79bbf8ac3b5f68fe56630d311` (`target/target-acceptance.json`) |
| T8 | snapshot | pass | same |
| T2 | c5 | pass | same |
| T8 | c5 | pass | same |

- T0: the real `oo doctor` (lane H `t0.validate`) is ready in `auto` and
  `C5`; `C1` is blocked with every log/native change capability reported
  `unsupported` (no DM LogMiner integration). An ungranted table yields
  `GRANT SELECT ON "FIXTURE"."T0_DENIED" TO "OOREADER"` and exit 1.
  `COMPATIBLE_MODE`/`BLANK_PAD_MODE` are reported unmeasured with advice
  (`GRANT SELECT ON V$DM_INI`), never a blocker. The model is a local
  synthetic replay, not live model acceptance.
- T1: the snapshot load ran under a concurrent SYSDBA writer (1,377
  commits: updates, inserts, deletes, primary-key moves, composite-key
  moves); one more commit landed after the load returned. Exactly one C5
  correction cycle followed (no quiesced reload). It published 2
  generations, 8,709,887 target bytes (`EVENTS` 2 ranges, keyless
  `NO_KEY` refreshed as a whole-table unit). Afterwards all 231 tables,
  2,000,466 rows, equal the source rendered independently by `disql`
  (the 300-column table in key-joined column groups). A resumed cycle 1
  wrote nothing.
- C5: cycle 2 idle — 0 target bytes, 0 generations, 0 payload, 0 source
  payload bytes; cycle 3 changed one row — 1 generation, one 90,000-row
  range replaced (7,111,701 bytes); cycle 4 idle — 0/0/0/0. Every cycle was
  compared table by table with the source and was equal.
- T8: one reader session throughout (peak 1 over 16,561 `V$SESSIONS`
  samples); every load and cycle ran at exactly 25.0% read duty
  (e.g. load 24.0 s read / 71.9 s sleep; correction 69.4 s / 208.2 s).
  Worker 256 MiB, target cgroup `oom 0`. Model calls/tokens 0.

## Agent DB types (measured with `format_type`)

`BIGINT` bigint; `INT` integer; `SMALLINT`/`TINYINT`/`BYTE` smallint;
`BIT` boolean; `DECIMAL(31,8)` numeric(31,8); `NUMBER(10,2)`
numeric(10,2); `FLOAT`/`DOUBLE`/`REAL` double precision; `DATE` date;
`TIMESTAMP(6)` timestamp; `TIMESTAMP(6) WITH TIME ZONE` and `WITH LOCAL
TIME ZONE` timestamptz as UTC instants (D17, offset not kept); text types
text; `BLOB`/`BINARY`/`VARBINARY` bytea. Exact text, stated: `NUMBER`
without precision, `TIMESTAMP(9)`, `TIME`, `INTERVAL`.

## Supply chain

The `dm8` feature adds no Rust dependency; `Cargo.lock`,
`tools/oo-load/Cargo.lock` and `ci/deny.toml` are unchanged from the base.
`cargo audit` passes for both lock files. `cargo deny --config
ci/deny.toml check advisories licenses`: licenses ok; advisories fail only
on `RUSTSEC-2026-0442` (`arrow-data 60.0.0`, unsound), which is in the
base lock file and identical on `v0.2/product-loops`. The vendor DPI
client lives only in the local, unpublished worker image; no Dameng
library enters a default artifact.

## Gaps

- DM SSL is not wired into the worker: remote sources and `tls_ca` are
  refused; only loopback in the worker's network namespace is accepted.
  SSH and log CDC (C1) are not integrated.
- Consistency B (per-statement read committed); the watermark is an
  observation time, not a log position.
- An earlier full run on the previous base (`d8-s5h-20261105`) also
  passed, but its correction cycle happened to find nothing to fix; the
  harness now forces a post-load commit and requires the correction to
  publish. It is superseded by this run.

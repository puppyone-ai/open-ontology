# E-S5g: Db2 C5 and product T0

Plan: S5-E-03. Run: `e-s5g-20261105`. Base: `065045092`.
Rebased onto the local coordinator branch before editing.

## C5

`oo-load --db2-worker WRAPPER --c5 FILE` now dispatches to the shared
durable C5 loader. The source connector fences table creation identity,
column types/lengths/scales/nullability/code pages and primary keys.
It adopts durable key ranges only with matching table/key/strategy,
contiguous bounds and valid key values. No new crate or lockfile change.

```sh
python3 testing/engines/db2/target_acceptance.py \
  --run-id e-s5g-20261105 --attempt c5-v3 --c5
```

Db2 11.5.9.0, linux/amd64 emulation. Four synthetic tables, 2,000 events,
composite key, empty table and duplicate/null no-key rows. This is focused
incremental acceptance, not another 231-table/2M-row acceptance.
The separate admin writer ran during the initial snapshot; it was stopped
before a full B-consistency correction and the three C5 cycles.

| Cycle | Agent DB comparison | Logical target bytes | New generations |
|---|---|---:|---:|
| 1: establish digests | all four tables equal | 0 | 0 |
| 2: update one range | all four tables equal | 9119 | 1 |
| 3: idle | all four tables equal | 0 | 0 |

Cycle 3 also reads zero payload bytes (digest queries still read source
rows). Explicit T2/c5 pass row: `c5-v3/index-rows.json`. The raw report,
receipts and per-cycle independent row comparisons are retained.
No Gateway publication, source-real-time watermark or log CDC is claimed.

## Product T0: blocked by coordinator code

```sh
python3 testing/engines/db2/product_t0.py --run-id e-s5g-20261105 \
  --attempt product-t0-v2 --binary target/debug/oo
```

Exit 1, explicit T0/snapshot fail, version not measured by doctor.
The adapter invokes the actual CLI and imports H's `t0.validate`, replay
and verdict writer; it does not modify the lane-H launcher. Doctor rejects
Db2 before invoking its registered adapter: `engine not available in this
build`. The pre-dispatch allowlist omits Db2 although registry and dispatch
already contain it. Exact requested coordinator patch (not applied):

```diff
--- a/crates/oo-cli/src/admin/doctor.rs
+++ b/crates/oo-cli/src/admin/doctor.rs
@@ -308,7 +308,7 @@
         let setup = (|| {
             let family = family(source)?;
             match registry.get(&family) {
-                Some(Adapter::Postgres | Adapter::Oracle | Adapter::Mysql | Adapter::Sqlserver) => {
+                Some(Adapter::Postgres | Adapter::Oracle | Adapter::Mysql | Adapter::Sqlserver | Adapter::Db2) => {
                     ()
                 }
                 _ => return Err("engine not available in this build".into()),
```

After applying, rerun T0; its remaining mode/missing-grant checks have not
executed. H's launcher still only constructs P0 fixtures; this lane adapter
uses its validator without relabelling lane-E fixtures as lane-H.

## Validation and preserved failures

- `cargo check --workspace --locked`: pass.
- `cargo test -p oo-connectors --features db2 --locked`: one baseline failure,
  `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`.
  The identical `group_signal=rejected` versus `sent` failure was reproduced
  from an archived `065045092` under this worktree (`base-deadline.log`).
- Same crate command with that one test skipped: 236 pass.
- `cargo test --manifest-path tools/oo-load/Cargo.toml --locked`: 39 pass.
- `python3 -m unittest discover -s testing/engines/db2 -v`: 16 pass.
- Touched-crate format checks and all three spec/plan checkers: pass.
- First T0 attempt also exposed a failed-verdict fallback bug in this new
  adapter; the corrected adapter emits an unmeasured failure row.
- `c5-v1` failed at the shared CLI engine allowlist; fixed by admitting
  explicit Db2 C5 only, with a regression test. `c5-v2` passed but reused
  comparison filenames; `c5-v3` reran with separate per-cycle raw files.

All failed attempts remain in evidence. Source and Agent DB containers
created for these Db2 runs were removed; no unrelated resources touched.

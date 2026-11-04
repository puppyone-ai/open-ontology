# M-S1d sign-off (2026-11-04)

Status: partial. Rebased on `0ac7ec034`; product runtime changes are the minimal
MySQL/PostgreSQL split hooks and engine-neutral loader integration. No Oracle,
shared engine API, root dependencies, migrations or lockfiles were changed.

## TODO exits

- S1-M-01: not done overall. The card's two real SIGKILL recovery checks pass
  against a fresh Agent DB with a fake SourceEngine. Immutable-parent composition
  and shared/Gateway publication are not delivered by this local admission path.
- S1-C-01: not done. Real trait splits pass on all four MySQL versions, but full
  T1 fails on mandatory fixture types. No fixture or gold value was removed.
- S1-C-02: not done. Source sampling, budget enforcement and target logical write
  bytes are implemented; aborted MySQL loads are not full T8 acceptance.
- S1-D-01: done for this card's T1/T8 exits only. PostgreSQL 13-18 full-scale
  T1/T8 and deterministic real growing-range split tests pass.

## Full-scale matrix

Commands (all use H's unchanged generator, 2,000,000 events and 200 small tables):

```sh
python3 tools/oo-test/t1.py --engine postgres --versions 13,14,15,16,17,18 --run-id m-s1d-resume-pg-full
python3 tools/oo-test/t1.py --engine postgres --versions 15,16,17,18 --run-id m-s1d-pg-persistent
python3 tools/oo-test/t1.py --engine postgres --versions 17 --run-id m-s1d-pg-transport
python3 tools/oo-test/t1.py --engine mysql --versions 5.7.44,8.0.44,8.4.7,9.5.0 --run-id m-s1d-resume-mysql-full
```

Every accepted PG run compares all 233 tables after a concurrent writer and
corrective reread, then verifies receipt replay with no added target writes.
Peak reader connections are 2 in every invocation; the separate topology cap
is 2, and ReadBudget aggregate duty is capped at 25%. Target bytes are committed
logical tuples, including abandoned stages, not physical disk or network bytes.
ReadBudget has no target-byte or connection-count fields; no such cap is invented.

| Engine | T1 / T8 | Maximum sampled duty | Target logical bytes | Run ID |
| --- | --- | --- | --- | --- |
| PostgreSQL 13 | passed / passed | 2.939% | 223,467,868 | `m-s1d-resume-pg-full` |
| PostgreSQL 14 | passed / passed | 4.503% | 223,873,453 | `m-s1d-resume-pg-full` |
| PostgreSQL 15 | passed / passed | 3.443% | 223,444,711 | `m-s1d-pg-persistent` |
| PostgreSQL 16 | passed / passed | 4.297% | 223,778,464 | `m-s1d-pg-persistent` |
| PostgreSQL 17 | passed / passed | 3.755% | 223,734,157 | `m-s1d-pg-transport` |
| PostgreSQL 18 | passed / passed | 4.375% | 223,752,370 | `m-s1d-pg-persistent` |
| MySQL 5.7.44 (amd64 emulated) | failed / partial | not accepted | 0 | `m-s1d-resume-mysql-full` |
| MySQL 8.0.44 | failed / partial | not accepted | 0 | `m-s1d-resume-mysql-full` |
| MySQL 8.4.7 | failed / partial | not accepted | 0 | `m-s1d-resume-mysql-full` |
| MySQL 9.5.0 | failed / partial | not accepted | 0 | `m-s1d-resume-mysql-full` |

All these loads used final product binary SHA-256
`6d38ca1ebbf44b0f6c7f1c94aee861edf09ee6725a97116323158a7edfb02f1c`.
No mysql-binlog feature was used. The harness evolved between runs: PG13/14
used per-sample execs; PG15/16/18 used a persistent admin sampler; PG17 also
used bounded retries of confirmed pre-dispatch read-only Docker refusals.
SQL errors, partial output and writes are not retried. Budgets and gold values
were unchanged. Original failures and all retry-attempt logs remain intact.

MySQL type_edges runs first to fail early without excluding any table. MySQL
5.7.44 fails `charset`; a separate owned catalog probe
(`m-s1d-mysql57-catalog`, reduced diagnostic fixture) confirms latin1 on
original_zone, text_lob, enum_value, set_value and nullable_text. The three
newer versions fail `unsupported_type`; the existing mapper explicitly rejects
SET. Aborted loads have insufficient samples and never count as T8 passes.
Oracle T1 remains pending lane E's LOB work; no Oracle code or live run is claimed.

## Real split and recovery evidence

```sh
python3 tools/oo-test/split_probe.py --engine postgres --versions 13,14,15,16,17,18 --run-id m-s1d-resume-pg-split --test-binary target/debug/deps/source_split-273569d06f02b19e
python3 tools/oo-test/split_probe.py --engine mysql --versions 5.7.44,8.0.44,8.4.7,9.5.0 --run-id m-s1d-resume-mysql-split --test-binary target/debug/deps/source_split-273569d06f02b19e
python3 tools/oo-load/test-target.py --run-id m-s1d-resume-recovery
```

The growth probe inserts via a separate fixture-admin connection only after
planning. All ten versions recover exactly 28 keys without duplicates; PG uses
three trait splits per version, MySQL one. Children preserve parent bounds,
advance revisions, use unique IDs and converge without a table replan.
Probe binary SHA-256:
`873c44f2d9b7fec9f45fb92769e9a3d1f90ad1af320f3b2209381a29d5b69b65`.

Recovery SIGKILLs occur after stage commit and after publication (exit -9 each).
The final process exits 0 with one receipt, three rows and 174 write bytes at
all checkpoints. This proves target idempotency with a fake source, not recovery
of an entire real-source matrix or immutable-parent/Gateway publication.

## Checks

Raw checks: `evidence/m-s1d-resume-check/`. Builds/tests use debug=0 and
incremental=0 to limit local disk growth; no compiler assertions are disabled.

- `cargo check --workspace --locked`: passed before each new commit.
- `cargo test -p oo-pipeline --locked`: 346 passed, 53 live tests ignored.
- `cargo test -p oo-connectors --features mysql --locked -- --skip parent_deadline_kills_the_worker_and_the_next_call_completes`: 295 passed, 24 ignored.
- The excluded `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes` fails identically on archived base `e42600761` (group_signal rejected versus sent), retained in `m-s1d-check/base-parse-file.log`. The rebase from that base adds only coordination/docs before this lane's commits.
- `CARGO_TARGET_DIR=target cargo test --manifest-path tools/oo-load/Cargo.toml --locked`: 6 passed, 3 live tests ignored by default; all three live test paths are exercised separately above.
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/oo-test -v`: 29 passed.
- Touched Rust formatting, `check_spec.py`, `check_plan.py`, `check_v02_plan.py`, `git diff --check`, staged secret/path scans: passed.

## Retained failed history

- `m-s1d-pg-smoke`: reduced PG17 pass on an intermediate binary, not acceptance.
- `m-s1d-mysql-smoke` / `m-s1d-mysql-smoke2`: initial TLS-file mode failure, then required SET rejection; neither is acceptance.
- `m-s1d-pg-full` / `m-s1d-mysql-full` / `m-s1d-recovery`: earlier Docker metadata read-only/start failures, no full load or kill acceptance.
- `m-s1d-resume-pg-full`: PG15/17/18 T8 failed on Docker-daemon access; PG16 comparison failed similarly. PG13/14 passed.
- `m-s1d-pg-persistent`: PG15/16/18 passed; PG17 comparison still hit a pre-dispatch Docker refusal. The separate transport run passed.

## Requested out-of-scope patches

Both proposals are **unapplied and untested**; `git apply --check` accepts them
together. Lane C must review, add/execute mapping and real-version coverage,
and rerun the full MySQL matrix. Legacy preview rejection remains unchanged.
No Cargo.lock changes are requested.

### SET capture (includes a proposed unit test)

```diff
--- a/crates/oo-connectors/src/mysql/map.rs
+++ b/crates/oo-connectors/src/mysql/map.rs
@@ -211,11 +211,18 @@
     reserved: &BTreeSet<String>,
 ) -> Result<MappedColumn, PreviewError> {
     let mut normalized = column.clone();
+    let set_as_text = column.data_type.eq_ignore_ascii_case("set");
+    if set_as_text {
+        normalized.data_type = "text".into();
+    }
     let transcode = matches!(column.charset.as_deref(), Some("gbk" | "ucs2"));
     if transcode {
         normalized.charset = Some("utf8mb4".into());
     }
     let mut mapped = map_column(&normalized, boolean_tinyint1, session_utc, reserved)?;
+    if set_as_text {
+        mapped.caveat = Some("set_as_text");
+    }
     if transcode {
         mapped.plan.mapping.metadata.push((
             "mysql_source_charset".into(),
@@ -733,6 +740,17 @@
     }
 
     #[test]
+    fn capture_set_preserves_declared_type_and_text_semantics() {
+        let mut col = column("set", "set('a','b')", "");
+        col.charset = Some("utf8mb4".into());
+        let mapped = map_capture_column(&col, false, true, &reserved()).unwrap();
+        assert_eq!(mapped.plan.mapping.source_type, "set('a','b')");
+        assert_eq!(mapped.plan.arrow, Some(ArrowPhysical::Utf8));
+        assert_eq!(mapped.caveat, Some("set_as_text"));
+        assert_eq!(capture_expression(&mapped), "`amount`");
+    }
+
+    #[test]
     fn geometry_and_set_are_unsupported() {
         let geo = column("geometry", "geometry", "");
         assert_eq!(
```

### latin1 capture through the existing round-trip guard

```diff
--- a/crates/oo-connectors/src/mysql/map.rs
+++ b/crates/oo-connectors/src/mysql/map.rs
@@ -211,7 +211,7 @@
     reserved: &BTreeSet<String>,
 ) -> Result<MappedColumn, PreviewError> {
     let mut normalized = column.clone();
-    let transcode = matches!(column.charset.as_deref(), Some("gbk" | "ucs2"));
+    let transcode = matches!(column.charset.as_deref(), Some("latin1" | "gbk" | "ucs2"));
     if transcode {
         normalized.charset = Some("utf8mb4".into());
     }
@@ -234,7 +234,7 @@
         .iter()
         .find(|(k, _)| k == "mysql_source_charset")
     {
-        if matches!(charset.as_str(), "gbk" | "ucs2") {
+        if matches!(charset.as_str(), "latin1" | "gbk" | "ucs2") {
             return format!(
                 "CASE WHEN {name} IS NULL THEN NULL WHEN CAST(CONVERT(CONVERT({name} USING utf8mb4) USING {charset}) AS BINARY) = CAST({name} AS BINARY) THEN CONVERT({name} USING utf8mb4) ELSE REPEAT(CAST(0xFF AS BINARY),OCTET_LENGTH({name})) END AS {name}"
             );
```

## Evidence hashes

Each entry is SHA-256 of `evidence/<run-id>/<file>`; raw files remain git-ignored.

```text
471f4a98fb7034df4cbc86b75c60535e860a0a082ea711609af07b4fa9948506  m-s1d-resume-pg-full/results.json
8f008f932874d4cf63a7eddb38e3830dfb3a95c502968d9c52bbaff6fc959d78  m-s1d-pg-persistent/results.json
b279af803f5a0afef99eb89655973e70b9f0a80cdeaf101874179abcd8616c88  m-s1d-pg-transport/results.json
1e23bb8bcabde48e36fdb1b87520988677a5bbb96eb2fa0380c71871b0bcc371  m-s1d-resume-mysql-full/results.json
e9cb0f837a4d3d42aeafbc8b3709d5ff53324259b279beb9cf1229bfa583d3b6  m-s1d-resume-pg-split/results.json
c65c512871ed54dc189bcaf9b4de4b26b9fe486d8a005656b71062ff610a00ac  m-s1d-resume-mysql-split/results.json
7d36e4ee689bb91178ffd8acb4ce550ba1747465ea81c14fd760f631dfbae35a  m-s1d-resume-recovery/sigkill-results.json
4d93fedba8dee2f98d6c7c6a9eaad2894a138228cbddb407ea0da6d7224c444d  m-s1d-mysql57-catalog/catalog.json
```

All owned run cleanup receipts have empty failure lists. Final live lane-M
container/volume inventories are saved in `m-s1d-resume-check`; no containers
are left running. No unrelated resource was stopped, restarted or removed.
No push, PR, service changes or customer operations were performed.

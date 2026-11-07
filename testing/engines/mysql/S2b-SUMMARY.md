# Lane C S2b: partial delivery

S2-C-01 and S2-C-02 remain **not done**. This report supplements, rather
than replaces, `S2-SUMMARY.md`. The obsolete `S2-prerequisites.patch` was
removed. No default feature or workspace manifest/lock was changed.

## Delivered and tested

- Standalone preview now opts into `mysql-binlog`, uses mysql_async 0.37.1,
  and shares Tokio 1.53.3 with source-admission and the connector. Its lock
  uses the workspace's patched zstd 0.13.4 / zstd-safe 7.4.0 /
  zstd-sys 2.1.2 stack. Both standalone lockfiles were refreshed.
- `mysql::c5::Cycle` compares `SourceEngine::digest_range` results against
  an explicitly confirmed baseline. Positive time/recent hints only
  reorder work; every range is covered. A periodic full read catches a
  simulated digest collision. Failed, abandoned, foreign and stale
  proposals cannot advance the baseline. A changed digest during reading
  refuses the proposal rather than acknowledging stale content.
- A no-change proposal has zero replacement ranges and zero outgoing
  payload. This is a connector-side result, **not** an Agent DB receipt.
  A rare full-read cycle still reads source payload to check its baseline.
- `mysql_preview::c1_retention::inspect(...).require_sufficient()` returns
  a `RequirementKind::Retention` with an explicit new-generation
  rebaseline requirement when actual available history is too short.
  Measurement starts at `MysqlEngine::earliest_binlog()` and reads the
  oldest log's first dated event using the existing driver's binlog
  stream. It uses server time, rechecks the log inventory, and never
  derives history from expiration settings. It does not allocate a
  generation or alter a source setting.

## Final live matrix

Commands:

```sh
python3 testing/engines/mysql/run.py --suite s2-c5 --run-id c-s2b-c5-20261104-03
python3 testing/engines/mysql/run.py --suite s2-retention --run-id c-s2b-retention-20261104-03
```

| Version | C5 baseline/change/no-change/rare-full | Retention sufficient/short |
| --- | --- | --- |
| 5.7.44, amd64 | PASS | PASS |
| 8.0.44 | PASS | PASS |
| 8.4.7 | PASS | PASS |
| 9.5.0 | PASS | PASS |

The fixture uses lane H's shared synthetic seed. Every C5 cycle compares
the in-memory replacement ranges with actual source rows; GTID sets are
monotonic. The short-history case emits a Requirement, not a fabricated
generation. All lane-C fixture containers and volumes were removed.

## Local validation and preserved failures

Evidence run: `c-s2b-20261104`.

- `cargo check --workspace --locked`: PASS before each commit.
- `cargo fmt -p oo-connectors -- --check`: PASS. New preview Rust files
  pass direct rustfmt checks; legacy preview files were not reformatted.
- Connector feature-enabled unit tests: 179 passed, 2 live ignores;
  `mysql_c5_cycle`: 4 passed.
- Preview unit tests: 242 passed, 15 ignores. Source-admission unit tests:
  63 passed, 1 ignore.
- Full connector suite: only
  `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`
  fails. The identical `group_signal` assertion fails on unmodified base
  `c90c457c0`; see `base-parse-deadline.log`.
- Source-admission `oracle_gate`: seven historical-evidence failures,
  reproduced on that base; `base-admission-oracle.log` lists every test.
- The full preview suite reaches `c5_layered_live`, whose fixed local
  fixture refuses the connection. This is not a pass. The unmodified base
  preview cannot resolve its conflicting Tokio pins. No test was weakened.
- Preview lock audit improves from 11 to 6 inherited advisories. The
  remaining ctutils, rustls and rustls-webpki findings are recorded in
  `preview-audit.json`; the standalone tool is **not audit-clean**.
- Earlier failed runs are retained: the temporary missing-dependency probe
  overlapped the first 9.5 C5 compile; retention's first matrix encountered
  a type-inference compile error. Both final matrices above pass. A base
  build sharing the target directory also briefly displaced the current
  connector artifact; rebuilding the current source restored the passing
  feature-enabled tests.

Raw evidence index: `evidence/c-s2b-20261104/final-index.json`.
SHA-256: `5c578d11912842cb6f7f565197ad708816193a98c8957c735592b5e739da0861`.

## Remaining scope and dependency request

No MySQL `ChangeSource` implementation or fake stream was added. Continuous
L0 staging, chunk-window log correction, tombstones, C1 three-cycle tests,
GTID-lineage/purge recovery and multi-source/multi-binding handoff remain
unimplemented. Time-column/recent-window observation is not connected to
the new C5 hint input. The retention helper is not yet wired into the old
preview handoff, which still uses its older settings-based eligibility.
The Agent DB zero-rewrite/zero-generation and rebaseline-publication exits
remain pending lane M.

The connector cannot directly reuse the preview's stream-polling code
without a direct Stream dependency. `stream-trait-probe.log` proves E0432
for `futures_util::StreamExt` under `--features mysql-binlog`. The probe was
removed. The connector manifest is outside this card's write scope.
Requested coordinator patch (existing locked package, no new version):

```diff
--- a/crates/oo-connectors/Cargo.toml
+++ b/crates/oo-connectors/Cargo.toml
@@
-mysql-binlog = ["mysql", "mysql_async/binlog"]
+mysql-binlog = ["mysql", "mysql_async/binlog", "dep:futures-util"]
@@
 [dependencies]
+futures-util = { version = "=0.3.34", optional = true, default-features = false, features = ["std"] }
--- a/Cargo.lock
+++ b/Cargo.lock
@@
 name = "oo-connectors"
 version = "0.1.0"
 dependencies = [
  "arrow-array",
  "arrow-buffer",
  "arrow-ipc",
  "arrow-schema",
+ "futures-util",
  "mysql_async",
```

This request enables the next implementation step; it does not itself
complete S2-C-01 or the remaining S2-C-02 exits.

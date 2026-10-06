# Findings

Reviewer: this session, sole coder and reviewer. This is not a separate reviewer and it is not parent acceptance.

## Verdict

The frozen child matches `linux/impl-1` for the PostgreSQL contract, `linux/impl-3` for clippy, and `linux/restart` for the post-restart read. All three used Linux arm64 Rust 1.94 and fixed commit `c8588faab992af1193ff58dbc489a0b117796aa0`. `fresh_pg_contract` exited 0. Clippy `--all-targets -D warnings` exited 0 on impl-3. After a host restart of the same PostgreSQL data volume, `restart_snapshot_persists` exited 0. The child lock's registry triples are a subset of the root lock (`registry=208 path=3 missing=0 mismatch=0`). The root lock hash did not change. The child lock hash is the same in impl-1 and impl-3.

I read the impl-1, impl-2, and impl-3 phase exits, test logs, clippy logs, `lock-subset.txt`, `lock-hash.txt`, and the two postmaster timestamps after each container exited.

## What the proof shows

`fresh_pg_contract` installs into an empty `oo_a293`, replays that install, and refuses a second project and a second allowlist. Revoking `CREATE` returns SQLSTATE `42501` and leaves `oo_enum_control` absent. A pre-existing `oo_dict` schema and `oo_dict.enums` table is `Collision` and is not dropped by the adapter. Damaging the stored SQL checksum or adding a snapshot column is `InstallDrift` and is not repaired. A wrong expected report digest inserts nothing. An `as_of` of `not-a-date` is `invalid_as_of` and inserts nothing. `inferred-missing.yaml` is `inferred_unapproved` and inserts nothing.

The handwritten historical JSON for `deprecated-historical.yaml` at `2020-01-01`, observation code `4` count `3`, and the default threshold hashes to `63fb59e7431cd131c512c3bfa1c61987c717d13bfe902552925d2b782649d9f6`. The library test compared that literal with `serde_json::to_string` of `enum_history::compile` and passed. The same digest was stored. Status stays `deprecated`. Class stays `open`. Description is null. Comparison is `above`. Ratio is `3/3`. `owner_authenticated` is false.

Putting the same identity with threshold `Some("0.95")` and the digest of that non-defaulted report is `SnapshotDrift`. The stored `threshold_defaulted` stays true. A second version of the same YAML can be inserted. Changing its published status from `deprecated` to `approved` and putting the original payload again is `SnapshotDrift`, and the damaged status remains `approved`. The historical row stays `deprecated`.

A compare-and-swap with a stale current id leaves the pointer empty. A wrong config digest is `Activation` and leaves the pointer empty. Revoking `oo_enum_view` from `oo_enum_admin` makes activation fail with SQLSTATE `42501`, rolls the pointer back, and leaves the served table empty.

Two activations of `retro-2023` and `cov-2026` with an empty expected current produced one success and one `CasConflict`. The view was then moved to the coverage snapshot when the winner was the other snapshot. The reader saw code `1` status `approved` class `open`, and code `9` status `unexplained` with a null class and a null description. The final compare-and-swap selected the historical snapshot. The view's `as_of` was `2020-01-01`, which differed from `current_date::text` in the test. Product SQL does not call `current_date`.

Runtime `UPDATE`, `DELETE`, and `INSERT` on `oo_dict.enums` returned SQLSTATE `42501`. Runtime `SELECT` of `enum_yaml`, `report_json`, notice, published, served, and `biz.receipts` returned `42501`. `SET ROLE oo_enum_view` then selecting `enum_yaml` and `biz.receipts` returned `42501`. `oo_r_other` cannot select the view. `read_view` as `oo_enum_admin` is `LoginRole`.

Coverage code `9` is notice reason `unknown_code` count `5`. Ratio `95/100` compares `equal`, `threshold_met` is true, and `review_needed` is true. Ratio `94/100` compares `below` and `threshold_met` is false. Retroactive 2023-12-31 and 2024-01-01 published definition statuses are `superseded` then `approved`. The 2024-01-01 classes are `open` then `closed`.

The view columns are `enum_id`, `code`, `description`, `class`, `valid_from`, `valid_to`, `version`, `as_of`, `project_id`, `status`. The owner is `oo_enum_view`. The view definition has no `now(`, `current_date`, `current_timestamp`, `clock_timestamp`, `date_trunc`, `current_setting`, `enum_yaml`, `report_json`, or `row_kind`. Deployment flags are all false. The nested report JSON contains `"database_used":false`, `"owner_authenticated":false`, and `"metric_certified":false`.

After the postmaster restart, the historical view row, snapshot count 6, damaged drift status, coverage notice, ratio `95/100`, pinned digest, snapshot replay, and install replay still hold.

The migration scan rejects `IF NOT EXISTS`, `CREATE OR REPLACE`, clock calls, and a `format!` token in `sql.rs`.

## Retained failures

`linux/precode/console.log` records the read-only `mv` of `rust-toolchain.toml`. `toolchain-aside.txt` in that directory says `moved-aside`. The smoke exit is still 0. The file was not corrected after the run.

`linux/impl-1` preserves clippy exit 101 for `&HISTORICAL_SHA` where the constant is already `&str`. The live contract in that same log passed.

`linux/impl-2` preserves clippy exit 101 for `concat!` of one raw string. The database was not reinstalled.

## Limits that stay in force

The deployment binds one project. A caller cannot pass another project id into the view. A shared database for many projects is not authorized here.

The store does not certify finance, semantics, or an owner. Declared owner text and a review `by` label stay labels. `historical_business_class` does not promote a row to `approved`.

Unknown codes are visible when that snapshot is served. They do not receive an inferred class. Warnings are stored as notices. Runtime roles cannot read those private rows.

There is no sweeper and no current-clock update. `docs/05-agent-db.md` section 6.3 (`oo_dict.enum_values`, `now()`, `date_trunc`) is not the view this child installed.

The view role can select `oo_enum_control.served`. That grant is what makes the security-barrier view readable. The same role cannot select snapshot YAML, report JSON, notices, or published rows. The live test checks those refusals.

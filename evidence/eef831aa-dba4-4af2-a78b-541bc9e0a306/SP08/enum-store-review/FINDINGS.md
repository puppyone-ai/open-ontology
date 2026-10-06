# Findings

Verdict: **APPROVE_LIMITED** for `tools/enum-store` at `72dc9e2db841de4de368216bbbe13d738f850ea9` only.

The approval is the behavior exercised by `linux/smoke`, `linux/overlay`, and `linux/restart`. It is a limited development verdict for that frozen store. It is not parent acceptance of SEM-D04, not SP08 closure, and not a release.

## Store behavior the exits support

Install binds one project, `a306-receipts` in the independent test and `project-one` in the author test. A second project id is `ProjectBinding`. An allowlist change is `Allowlist`. Replay requires the same migration hash, snapshot shape, project, and allowlist. Revoking `CREATE` returns SQLSTATE `42501` and leaves `oo_enum_control` absent. A pre-existing `oo_dict` schema is `Collision` and is not dropped. Damaging `sql_sha256` or adding a snapshot column is `InstallDrift`, and the damaged catalog is left in place until the harness restores it. The deployment row keeps `finance_certified`, `semantic_certified`, `owner_authenticated`, `metric_certified`, and `clock_used` false. A successful store receipt sets `database_used` true and those five flags false.

`put_snapshot` re-executes `enum_history::compile` on the caller YAML, the civil `as_of`, the observations, and the threshold spelling. The overlay checks that report with `report_sha256` and with `sha256_hex` of `serde_json::to_string`. The handwritten historical digest printed by the test is `7b715c151e8b63392c4df73a7c507d09ea5862b834eca53b5174831cd2b3094d`. The coverage digests are `559091929746e4012afc17176f71c27d5e5f03c8c915beac15ba4b2b07dd6d25` for `95/100` and `51400204dd207f3cd866480a3f3a11d8cfc286ae62fc13d694e2539af6043bf1` for `94/100`. A wrong expected digest inserts nothing (`ReportMismatch`). An `as_of` of `not-a-date` inserts nothing (`invalid_as_of`). Inferred source without an approved review inserts nothing (`inferred_unapproved`). The same identity with a different stored body is `SnapshotDrift` and does not overwrite. Spelling the default threshold as `0.95` drifts, and `threshold_defaulted` stays true. A damaged published status stays `approved` while the historical row stays `deprecated`.

Unknown observation code `8` is served as status `unexplained` with a null class and a null description, and it is stored as a notice with reason `unknown_code`. `95/100` compares `equal`, meets the default `0.95` threshold, and sets `review_needed`. `94/100` compares `below` and does not meet the threshold. Retroactive rows stay `superseded` then `approved`. `historical_business_class` does not rewrite `deprecated` or `superseded` to `approved`.

Activation is compare-and-swap on the expected pointer. A stale expected id left a NULL pointer in place. Two activations that both expected NULL produced one receipt with `grants == 1` and one `CasConflict`. Revoking membership of `oo_enum_admin` in `oo_enum_view` returned SQLSTATE `42501` and rolled the pointer and the served rows back. A later revocation, after the historical row was served, left that pointer and the deprecated served status in place.

The view is a static projection of `oo_enum_control.served`. It has the ten contract columns. It has no argument and no clock call. The owner is `oo_enum_view`, `NOLOGIN`, with `security_barrier=true` and `security_invoker=false`. That owner can select `served`. The same role receives `42501` on snapshot YAML, report JSON, notices, published rows, and `biz.receipts`. The runtime role receives `42501` on those control tables, including `served`, and on `UPDATE`, `DELETE`, and `INSERT` of the view. `oo_r_other` receives `42501` on the view. `read_view` as `oo_enum_admin` is `LoginRole`.

`docs/05-agent-db.md` section 6.3 draws `oo_dict.enum_values` with `now()`, `date_trunc`, and class `unexplained`. The contract and this migration install `oo_dict.enums` without those calls. One project at install is the intended binding. The passing tests do not establish a shared multi-project view, an authenticator, a finance certification, or a semantic certification.

A host restart of this task's data volume retained the active historical row, `as_of` `2019-07-04`, six snapshots, the damaged drift status, the coverage notice, both coverage ratios, the stored report digest, snapshot replay, and install replay. The recorded `pg_postmaster_start_time()` values change at each restart, and the retention run's before-time equals the overlay run's after-time.

## Dependencies

All 208 child-lock registry triples are in the root lock, with no missing or mismatched checksums. Direct pins from the enum-store manifest and the helper pins `chrono` 0.4.45, `chrono-tz` 0.10.4, and `sha2` 0.10.9 are present. Path packages are `enum-history`, `enum-store`, and `oo-core`, each 0.1.0. Neither lock file changed inside the containers. No manifest in the product tree was edited. `a207` remains the connector manifest and root-lock writer. This review did not run `cargo update` or `cargo generate-lockfile`.

The helper's own approval stays with `a278` at its own archive. This store calls `compile` again and checks the digest. It does not reopen helper policy. Declared owner text and `review.by` stay labels. The nested report keeps `database_used`, `source_modified`, `oo_dict_written`, `metric_certified`, `owner_authenticated`, and `sweeper_ran` false.

## Preserved author records

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a293/SP08/enum-store/` was not modified and was not rerun.

`linux/precode/console.log` records the read-only `mv` of `rust-toolchain.toml`. `toolchain-aside.txt` in that directory says `moved-aside`. The smoke exit there is still 0.

`linux/impl-1` preserves clippy exit 101 for `&HISTORICAL_SHA`. The live contract in that log passed.

`linux/impl-2` preserves clippy exit 101 for `concat!` of one raw string.

This review's smoke and overlay clippy exits are 0 on archive `72dc9e2`. Those new logs do not replace the author exits.

# Findings

Verdict: **APPROVE_LIMITED** for `tools/calendar-store` at `cc98bab7eba2c58c127435db4d2ae777797259de` only.

The approval is the behavior exercised by `linux/smoke-2`, `linux/overlay-1`, and `linux/restart`. It is a limited development verdict for that frozen store. It is not parent acceptance of SEM-D03, not SP08 closure, and not a release.

## Store behavior the exits support

Install binds one project. A second project id is `ProjectBinding`. An allowlist change is `Allowlist`. Replay requires the same migration hash, published shape, project, and allowlist. Damaging `sql_sha256` or adding a published column is `InstallDrift`, and the damaged catalog is left in place until the harness restores it. No runtime grant is issued at install. The deployment row keeps `finance_approval`, `authenticated`, `ontology_published`, and `clock_used` false.

`put_snapshot` resolves the original YAML through `business-calendar` at an explicit `as_of`. The overlay compares that report, and the view, with handwritten literals in `overlay/a297_calendar.rs` for April in `Asia/Shanghai`, 29 February 2016 in `UTC`, a Pacific Daylight offset in `America/Los_Angeles`, the 2017-10-15 São Paulo gap, and the 1976-09-26 Paris overlap. A wrong resolved hash inserts nothing (`ResolvedMismatch`). An invalid `as_of` inserts nothing (`snapshot_refused`). The same identity with a different stored body is `SnapshotDrift` and does not overwrite. The drift row's `resolved_sha256` remained 64 zeros after the second postmaster start.

Activation is compare-and-swap on `snapshot_id IS NOT DISTINCT FROM` the expected pointer. A stale expected id left the pointer unchanged, both while it was NULL and after one of two concurrent activations had committed. The concurrent pair returned one `activated` receipt with one grant and one `CasConflict`. Revoking membership of `oo_calendar_admin` in `oo_calendar_view` returned SQLSTATE `42501` and left the pointer as it was. The view owner is `NOLOGIN` and is not a superuser. Runtime roles are not members of that owner. The runtime role, and a session that had run `SET ROLE oo_calendar_view`, received `42501` on snapshot YAML, closure label, closure record hash, report JSON, the published table, and `biz.receipts`.

The view is a static projection of the active snapshot. It has 34 typed anchor columns. It does not read `now()`, `date_trunc`, or `current_setting`. `docs/05-agent-db.md` §6.3 draws `oo_dict.calendar` with those calls and `current_setting('oo.as_of')`. The contract and this migration refuse that shape. One project at install is the intended binding. The passing tests do not establish a shared multi-project view, an authenticator, a finance approval, or publication.

A host restart of this task's data volume retained the active São Paulo snapshot, the explicit `as_of` `2017-10-15T04:00:00Z`, the six snapshot rows, the view text, and install replay. The two recorded `pg_postmaster_start_time()` values differ from each other and from the time recorded before the overlay restart.

## Dependencies

All 208 child-lock registry triples are in the root lock, with no missing or mismatched checksums. Direct pins named by the calendar-store contract and the helper pins `chrono` 0.4.45, `chrono-tz` 0.10.4, and `sha2` 0.10.9 are present. Path packages are the three named above, each 0.1.0. Neither lock file changed inside the containers. No manifest in the product tree was edited. `a207` remains the connector manifest and root-lock writer. This review did not run `cargo update` or `cargo generate-lockfile`.

The helper's own approval stays with `a283`. This store calls `resolve` again and checks the digest. It does not reopen helper policy. `week_start` stays calendar metadata. A caller closure hash is stored as supplied provenance. `finance_approval`, `authenticated`, `database_used`, and `clock_used` on the helper report stay false; the store's `database_used` on a successful read is true because the read used PostgreSQL.

## Preserved failures

These are harness records. They are not product edits, and they were not deleted.

`linux/proof` `docker-run.exit` 125. Docker rejected a relative bind path. No container was created.

`linux/proof-1` container `485cb00e954fe74b8cc3806912b1d032135e60d45247141b3c74cf92f81d65c4`, `docker-run.exit` 101. Identity, blob compare, and the 208-triple subset passed. `cargo fetch --locked` failed because `crates/oo-core` inherits `edition` and the archive had no root `Cargo.toml`. Tests did not start.

`linux/proof-2` `docker-run.exit` 1. `docker restart oo-a297-pg` ran. The postmaster query used the OS user `postgres` as the database role. This server's login role is `oo_fixture_admin`, so both timestamp files are empty and the launch stopped before the builder. The later `linux/overlay-1` timestamps are the first pair that differ.

The Debian Rust image contains a Python binary (`linux/smoke-2/python-present.flag` is 1). This review did not use a Python oracle. Hash and lock checks are `tools/sha256_manifest.rs` and `tools/lock_subset.rs`. Date checks are the Rust overlay.

## Author directory

`a289` `linux/impl-1` still records the smoke that removed the admin database ACL before `CREATE SCHEMA`, so the expected `42501` did not run inside that statement, and Clippy still records the unused closure parameter. `a289` `REPORT.md` still records that the first PostgreSQL start created `oo_a289` owned by `oo_calendar_admin`, that `pg/up.exit` 0 belongs to that start, and that `pg/up.sh` was overwritten in place with `OWNER oo_fixture_admin`, so the first script body is not a separate file. This review did not rewrite those files.

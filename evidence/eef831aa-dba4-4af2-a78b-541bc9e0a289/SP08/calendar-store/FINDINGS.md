# Findings

Reviewer: this session, sole coder and reviewer. This is not a separate reviewer and it is not parent acceptance.

## Verdict

The frozen child matches `linux/impl-2` and the post-restart `linux/restart` read. Both are Linux arm64 Rust 1.94 on fixed commit `216e6df82a61d2bc22548ce1a5075d5f4a045648`. Focused tests exited 0: 3 library, 2 contract, and the live contract. Clippy `--all-targets -D warnings` exited 0. After a host restart of the same PostgreSQL data volume, `restart_snapshot_persists` exited 0. The child lock's registry triples are a subset of the root lock (`registry=208 path=3 missing=0 mismatch=0`). The root lock hash did not change. The child lock hash is the same in impl-1 and impl-2.

I read the impl-2 and restart phase exits, test logs, clippy log, `lock-subset.txt`, `lock-hash.txt`, and the two postmaster timestamps after each container exited.

## What the proof shows

`fresh_pg_contract` installs into an empty `oo_a289`, replays that install, and refuses a second project and a second allowlist. Damaging the stored SQL checksum or adding a published column is `InstallDrift` and is not repaired by the product. A wrong expected resolved hash inserts nothing. An `as_of` of `not-a-date` inserts nothing. Replaying an identical snapshot keeps one row. Damaging `resolved_sha256` and putting the same identity again is `SnapshotDrift`, and the damaged hash remains.

A compare-and-swap with the wrong current snapshot id leaves the pointer empty. Revoking `oo_calendar_view` from `oo_calendar_admin` makes activation fail with SQLSTATE `42501` and rolls the pointer back. Restoring the membership and activating the April snapshot grants the allowlisted reader. The reader sees the handwritten April dates. A later stale current id, and an activation that names April's config digest while São Paulo is current, leave the view on São Paulo.

The view rows match the handwritten literals, not a second call that prints the helper's dates into the assertion:

- April fiscal, `Asia/Shanghai`, `as_of` `2026-04-01T03:30:00Z`: today, month start, quarter start, and fiscal year start are `2026-04-01` at `2026-03-31T16:00:00Z`. The previous month end, quarter end, fiscal year end, and last closed period end are `2026-03-31` at `2026-03-30T16:00:00Z`.
- Leap February, UTC, `as_of` `2016-03-01T00:30:00Z`: previous month end is `2016-02-29`. Last closed is absent.
- `America/Los_Angeles`, `as_of` `2018-06-21T06:30:00Z`: the civil day is `2018-06-20`. Previous quarter start is `2018-01-01` at `2018-01-01T08:00:00Z`.
- `America/Sao_Paulo`, `as_of` `2017-10-15T04:00:00Z`: today is `2017-10-15` with midnight status nonexistent and a NULL instant. Month start is `2017-10-01` at `2017-10-01T03:00:00Z`.

The stored `as_of` differs from `statement_timestamp()` in UTC. Five snapshots remain. The private closure label `close-2026-03` is still on the snapshot table. Runtime `SELECT` of snapshot YAML, closure label, record hash, report JSON, the published table, and `biz.receipts` is SQLSTATE `42501`. The view owner selecting `biz.receipts` and the snapshot table is `42501`. A role that is not on the install allowlist cannot select the view. `read_view` as `oo_calendar_admin` is refused before connect.

Deployment flags `finance_approval`, `authenticated`, ontology publication, and `clock_used` are false. The view definition contains no `now`, `date_trunc`, or `current_setting`, and it does not project closure labels, calendar YAML, or report JSON. The view owner is `oo_calendar_view`.

After the postmaster restart, the same São Paulo literals, the snapshot count 5, the private label, and install replay still hold.

The unit test in `publish.rs` keeps a nonexistent midnight and an ambiguous midnight as status plus a null instant. The migration scan rejects `IF NOT EXISTS`, `CREATE OR REPLACE`, clock functions, and a `format!` token in `sql.rs`.

## Limits that stay in force

The deployment binds one project. A caller cannot pass another project id into the view. A shared database for many projects is not authorized here.

The closure record hash is the caller's declared hex. It is not a finance approval. Activation does not publish an ontology.

The view role has no credential and is not a member path for runtime roles. `GRANT` of view `SELECT` happens only in the activation transaction, for roles recorded at install.

Public and runtime sessions cannot read private config, closure owner labels, JSON, or the declaration hash through the view.

There is no process clock and no default today. A blocked midnight is not shifted.

## Retained failures

`linux/impl-1` connected as the calendar admin only after the test had removed that role's database ACL, so the expected `42501` inside `CREATE SCHEMA` never ran. Clippy stopped on the unused closure parameter. Those logs stay. The database was recreated with `oo_fixture_admin` as owner and an explicit `CONNECT, CREATE` grant for `oo_calendar_admin`. The test then revokes only `CREATE`.

## Not accepted here

Parent acceptance, full SP08, a public gateway, authentication, a calendar owner UI, a metric certificate, multi-project shared-database authorization, and a production release are outside this child.

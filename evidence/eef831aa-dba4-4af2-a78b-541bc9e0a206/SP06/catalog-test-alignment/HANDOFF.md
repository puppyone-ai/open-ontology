# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a206`.
Source commit `a661dcdf72ff91561ae6b7cb79916e582884d08a`.
Parent `84c03d01ce35728924100e42ca142c0ef650ac71`.
Proof archive `5e3ba29f8d460a1aa6919733100c34a68a2fdb75`.
Path `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a206/SP06/catalog-test-alignment/`.

The catalog live test now follows the executor that ships with it. A validated guard refusal leaves `oo_r_clerk` bound and does not call the audit hook. A different role on that session is `RoleReuse`. A new session can bind `oo_r_auditor` and read its column. Do not move the bind back behind `guard_select` to restore `bound_role() == None`.

## Files

- `crates/oo-gateway/tests/policy_catalog_pg_live.rs`
- `crates/oo-gateway/src/policy.rs` generation comment only
- `contracts/gateway/policy-catalog-v1.md` generation scope and the xid sentence

`query_executor.rs`, `policy_catalog.rs`, `Cargo.lock`, manifests, and gates were not edited. Their blobs match the proof archive. Copies are in `source/blobs/`. `source/COMMIT` lists the git ids.

## How the proof was run

Linux arm64, Rust 1.94.0, one private 8 GiB builder at a time, one PostgreSQL 17.10 named `oo-a206-pg` with DNS alias `oo-a190-pg`. `OO_A190_LIVE=1` is required for the fixture. Unset, the test returns before it connects.

Read `linux/red/reproduced.txt` for the pre-edit failure and `live/green-oracle.txt` for the post-edit pass. Attempt 1 of the post-edit run is `linux/green/attempt-1/` and failed `rustfmt --check` on pre-existing `policy.rs` lines. Those lines were not reformatted.

## Left open

`StaleGeneration`'s comment still says the maximum is stored for the project. The SQL filters on source and config digest. `ensure_committed` is still the xid-null check. The contract now says that check is not an idle proof. Authentication, durable audit, row-level security, pool reuse, and a non-superuser grantor were not established. SP06-W01 and SP06-W02 stay open.

## Cleanup

Own containers, volumes, network, and `/tmp/oo-a206-eef831aa-dba4-4af2-a78b-541bc9e0a206` are removed in `cleanup/run.log`. No prune. No other task's containers or volumes. No push.

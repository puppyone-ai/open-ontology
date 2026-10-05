# Handoff

Verdict: LIMITED.

Reviewed commit: `78a60fdeabcada0d024fc6f9a81e0e6b184260e8`.
Author evidence: `9d658725fabfbf38694227bf462ba23d9c4d39cb`.
Session: `eef831aa-dba4-4af2-a78b-541bc9e0a200`.
Path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/`.

The catalog load on this commit follows the contract for this fixture: digest-scoped generation, privilege intersection, `attnum` order, server OIDs, the 54-operator set plus `count(*)`, explicit unknown-row policies, and an idle admin client after commit and rollback. The fixed executor binds `oo_r_<slug>` before `guard_select`. A guard refusal leaves that binding in place. See FINDINGS F1 and `live/oracle.txt` keys `bound_role_after_guard` and `role_reuse_after_guard`.

Do not treat a190 FINDINGS.md line 39 as a description of `78a60fde`. Do not move the role bind to after the guard to satisfy `policy_catalog_pg_live.rs` line 575. A correction of that assertion is a test change. It is not a loader change.

## Evidence

- `REPORT.md`, `FINDINGS.md`, this file.
- `source/COMMIT` and `source/blobs/` are the reviewed bytes. Smoke `linux/smoke/archive-blobs.txt` matches `git hash-object` to `git rev-parse` for those paths.
- `linux/smoke/` is the pre-overlay Rust 1.94 lib test and lib clippy. Exit 0. Overlay absent. Lock unchanged.
- `linux/proof/` is the live test. Exit 0. Attempts 1-3 are preserved beside it.
- `live/oracle.txt` is the measured fixture. It has no passwords and no physical names.
- `pg/` records the private server image, SAN, and capability set. Password files were only under `/tmp/oo-a200-eef831aa-dba4-4af2-a78b-541bc9e0a200/secrets` and are removed by cleanup.
- `SHA256SUMS` is the Rust tool `tools/sha256.rs`, mode `dir`. `SHA256SUMS.sha256` is the hash of that manifest.

Two commits, owned path only, no push, no amend:

1. `evidence: archive a200 policy catalog review sources`
2. `evidence: record the a200 policy catalog review`

## Left open

SP06-W01 and SP06-W02 stay open. Authentication, durable audit, row-level security, pool behavior, and a non-superuser grantor were not established. `AmbiguousMapping` was not forced through the mapping primary key. The post-commit check does not read `xact_start`; the second-session observation on this run did. A 42501 from the executor closes that dedicated session after a confirmed rollback.

## Cleanup

`cleanup/run.log` records removal of containers `oo-a200-smoke`, `oo-a200-proof`, and `oo-a200-pg`, volumes `oo-a200-eef831aa-cargo`, `oo-a200-eef831aa-target`, and `oo-a200-eef831aa-pgdata`, network `oo-a200-eef831aa-net`, and the `/tmp/oo-a200-eef831aa-dba4-4af2-a78b-541bc9e0a200` directory. No prune. No other task's containers or volumes.

# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a241`. Review only. Fixed commit `13566602dc36ce6ebd03fef25a9bd17aae7a281c`.

## Verdict

APPROVE_LIMITED for the frozen `install-column-acl` CLI scope plus the required separate `query-pg` check. Details are in `REPORT.md` and `FINDINGS.md`.

M1-D14D is still open. This handoff does not accept a parent goal and does not authorize a release.

## Proven here

Private arm64 Rust 1.94.0 and PostgreSQL 17.10. One builder at a time, 6GiB, 1 CPU, and one private Postgres. Smoke of the committed CLI tests with the live environment unset: 12 passed, no live oracle. Overlay: install, exact replay, bad second table, stale generation, hidden-write 42501, explicit mapping-read grants, `reltuples` unknown, `query-pg` policy refusal, `ANALYZE`, `query-pg` success for rows 41 and 42, and a producer policy paste refused as `policy stale` without changing clerk's 6 column grants. Clippy `-D warnings` on `install-column-acl` and the review test exited 0. Lock hash unchanged.

## Not proven

Non-superuser grantor. Authentication. Audit WAL. Publication. Row-level security. The installer does not grant mapping read and does not analyze. Those two steps were fixture SQL.

## Attempts

`attempt-1/` compile failure in the harness `json!`. `attempt-2/` false owner leak on the captured-pg-draft envelope, after the query had already succeeded. `attempt-3/` Clippy `collapsible_if` in the harness. Green logs are `linux/smoke/` and `linux/overlay/`.

## Cleanup

`CLEANUP.txt` records removal of `oo-a241-pg`, the three a241 volumes, network `oo-a241-eef831aa-net`, and the private `/tmp` tree. No prune. Foreign containers were not removed.

## Next session

Leave the product tree, the plan checkbox, and the lock alone. A new expansion needs its own frozen unit before coding. Do not treat APPROVE_LIMITED as closure of M1-D14D.

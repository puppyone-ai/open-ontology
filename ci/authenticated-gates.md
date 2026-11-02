# Authenticated engineering gates

This replaces the workflow wiring described in the bootstrap CI README. The
legacy `xtask gates --check` interface still rejects arbitrary imported captures;
it is not silently changed into an authority for caller-supplied JSON.

## Required jobs

The nine foundation jobs remain required. This product change activates four
additional jobs without moving their phase deadlines or changing acceptance IDs:

| Job / suite | Actual scope |
| --- | --- |
| gate-integration-a / integration-a | Fresh owned PG17 transaction rollback, receipt replay, multipart hydration and published-watermark continuity. |
| gate-nightly-a-m / nightly-a-m | The integration scenarios plus empty-baseline proof and fresh/upgrade migration lifecycle. Runs on PRs as well as nightly. |
| gate-security-b / security-b | Fresh owned PG17 column/role denial, read-only write refusal, real Gateway/CLI authentication and publication mismatch. |
| gate-ontology-d / ontology-d | Real Rust builder and synthetic library Self-build, YAML, role Pack, visibility, review and dependency-cache regressions. |

Commands are `cargo xtask test --suite NAME --format json`. These commands
supervise real child processes through the existing pinned Linux subreaper.
Each must produce a matching receipt, execute nonzero cases, exit zero and clean
up its own processes. Missing PostgreSQL, missing fixtures, skipped/zero-selected
cases, malformed receipts and unfinished cleanup fail. No customer environment
or external model is used. Python entry points can also run owned fixtures on
macOS; the supervised xtask gate itself requires native Linux.

Each receipt lists its covered and uncovered scope. A green capability job is
not full AT-PL-01/AT-DB-01/AT-GW-01/AT-OOS-01 closure, a customer sign-off or an
assertion that source CDC, public SLA or every supported database is complete.
`closes_acceptance`, `release_authority` and `protected_github_policy` stay false.

## Execution evidence

`ci/gate_receipts.py record` binds the command, source commit, run/attempt, exit,
start/end and log digest. Those files alone are not proof. Aggregate authenticates
to the fixed GitHub API using the current workflow's ephemeral read-scoped token:

1. Match repository, exact event head/base, run ID, attempt and workflow path.
2. Fetch that attempt's complete job list; each required job and capture step
   must be completed successfully on the expected hosted runner.
3. Match the exact capture-step script contract, not a command substring. A
   comment, unreachable branch, fabricated exit or continue-on-error is rejected.
4. Fetch uniquely named run-and-attempt artifacts. Verify GitHub's SHA-256 for
   the downloaded archive, run/source identity, bounded members, receipt fields,
   log digest and byte count, time window and structured success. API credentials
   are never forwarded to the signed artifact-download location.
5. Compare the candidate policy against BOTH the base commit's immutable floor
   and its active manifest. Removed/weakened requirements, moved deadlines,
   retargeted capabilities and inactive changed capabilities fail.

PRs test the event's exact head. The base need not be an ancestor; the merge base
selects PR-specific changed paths while the event's base tip supplies policy.
A failed job, missing log, tampered archive, stale attempt, duplicate job/artifact,
unknown command or absent GitHub authentication is nonzero. Rerun the **whole
workflow**, not only failed jobs, because evidence from older attempts is not
accepted. Aggregate does not consume unauthenticated local jobs.json as proof.

Like any repository CI, verifier and workflow source changes require normal PR
review. GitHub's run/artifact identity is the execution trust root, not a claim
that candidate verifier code is an independently approved release policy. Branch
protection, named CODEOWNERS, release signing and product acceptance remain
separate; this change does not manufacture them.

## Regression entry points

- `python3 -m unittest discover -s ci -p 'test_*.py'`
- `cargo test --locked -p xtask`
- `cargo xtask gates --check-manifest --format json`

The suite runners create fresh private directories and local clusters and stop
only those instances. Uploaded artifacts contain gate logs and result receipts,
not fixture credentials or database files. The root README is unchanged.

# B-S3c executor handoff

Status: partial. Six finding-specific commits are complete locally, but
findings 4 and 6 still need the coordinator-owned TLS transport to propagate
TCP peer addresses. L2 independent acceptance is not claimed.

## Scope and commits

Rebased onto `v0.2/product-loops` before editing; post-rebase base:
`5ef8ca5c49f65b2f0e8c0220db7f4959f4b2b1b9`.

| Finding | Commit | Result |
| --- | --- | --- |
| 1 health | e8633679b | Live absolute/idle/overlap filters; <=14 day warning retained |
| 2 renewal | e394cc259 | Fresh runtime lookup before admin connection; rate bound |
| 3 native admin | 9fe2c2aab | Seven native commands, npm delegation, unique hash prefixes |
| 4 limiter | 446cec80a | Separate route/identity buckets and global cap; IP integration pending |
| 5 persistence | fe8104bdd | Save 0600 credential before verification; retain on failure |
| 6 audit | ea74f011a | Code digest linkage, verified workload digest, device observations; IP pending |

## TODO exit evidence

Evidence run: `b-s3c-20261104`; raw logs remain git-ignored in `evidence/`.

- S1-B-01: not done for the full card exit. Renewal/admin/prefix/audit local
  tests pass, but per-IP limits and peer audit attribution await integration;
  real-PG assertions did not run.
- S1-B-02: done for the local fix. Health SQL structural regression failed
  before and passes after; certificate/14-day boundary tests pass. Updated
  real-PG health assertion is unexecuted, not runtime acceptance.
- S3-B-01: not done for the full card exit. Native admin and recovery paths
  pass real HTTPS fixture tests; peer/device audit integration and PG remain.
- S3-B-02: done for this card's CLI fixes. Native/npm admin approval and
  existing prompt tests pass. No other agent was launched, so this does not
  assert the full plan's separate-agent T6 acceptance.
- S3-M-01: coordinator independent review still required.

Regression red evidence: `01-red.log` (SQL structure), `02-red.log` (junk
renew), `03-red-node.log` (native admin), `04-red.log` (route/peer buckets),
`05-red.log` (identity/read failure recovery), `06-red.log` (audit linkage
and disclosure). Initial `02-green.log` failed due test-only ambiguous
Rustls CryptoProvider selection; explicit provider fixed it; failure retained.

Final validation, all exit 0:

- `cargo check --workspace --locked` -> `final-check.log`.
- `cargo test -p oo-gateway -p oo-cli --locked` -> `final-tests.log`.
- `node --test packages/cli/test/*.test.mjs packages/cli/scripts/*.test.mjs`
  -> `final-node.log`: 43 passed, 0 failed, 0 skipped.
- `cargo fmt -p oo-gateway -p oo-cli`; formatting check passes.
- Python runner syntax compilation passes.
- `git apply --check tools/gateway-token-store/S3C-peer-address.patch` passes.
  This checks patch applicability only, not compilation or runtime behavior.

## Engine evidence and operational boundaries

Owned PostgreSQL 17 runners (store/HTTP and native entrypoint) were NOT RUN.
`final-disk.txt` records `df -g /`: 15 GB available, below the required 20 GB
Docker gate. PG integration tests remain ignored in the normal Cargo suite.
The existing owned runner uses host PG17; any future execution for this card
must also honor the requested Docker-only fixture rule. No PG version or
real-database acceptance is claimed. No unrelated failures required a base
exception. No containers were created; none are left running. No customer
systems, token files, services, LaunchAgents, other worktrees or remote refs
were modified. Fixture-generated credentials stay outside git.

## Exact integration patch and remaining limits

`S3C-peer-address.patch` contains the exact proposed unified diff. It changes
`crates/oo-gateway/src/http_gateway.rs` to retain the accepted TCP address,
then uses that trusted address in auth bucket keys and audit actors, with a
regression for workload changes on the same IP. The transport file is outside
the lane's auth-file write scope, so the patch was NOT applied. Evidence of
the missing input: `TlsListener::accept` currently binds `(tcp, _remote)` and
`TlsPeer` carries only `workload`; handlers cannot reconstruct the address.
Forwarded headers are deliberately not used as an unsafe substitute.

Current workaround: verified workloads have separate identity buckets;
anonymous peers share one conservative bucket, and audit actors explicitly
say `peer=unmeasured`. Apply the patch, format, rerun tests and real owned-PG
checks once disk permits. Audit device observations use existing enroll/
redeem categories with distinct outcome strings, so no old migration changes
or new migration are needed. Malformed/rate-limited requests fail before DB
access. Device polls are observations of requests, not claims of success.

Cargo.lock: unchanged. No shared manifest, migration, docs/specs, execution
state, engine interface or support-matrix files were modified.

## Evidence SHA-256

- `final-check.log`: `d8c28a5e60831a89ca951655eae2064d2bc51f3256ad065fd074b7baa4ed4aea`
- `final-tests.log`: `7ffed116399470e04a4c281591efc3a268cf7083a36e2082358cc0dca9a4a261`
- `final-node.log`: `582ab981c616251b597926a7034d843db10cb5caead29d9f0dd09c1ef58b0ce0`
- `final-disk.txt`: `66e36683aaff2d0725b7faa7f40a2e9a84659fef5cb2f88f1c0abdd77b953d77`
- `fmt-check.log`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`

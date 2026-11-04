## Card B-S3c — lane B: fix the L2 independent review findings

TODO: S3-B-01, S3-B-02, S1-B-01, S1-B-02 (fixes); enables S3-M-01 exit. Independent review at d876260db found the
following. Fix each with a test that fails before and passes after; commit one fix per commit. Rebase on
v0.2/product-loops first. Work rule: smallest local workaround and keep going.
1. MAJOR auth_lifecycle.rs:277 — /v1/health token expiry uses min(absolute_deadline_ms) over all non-revoked tokens
   without `absolute_deadline_ms > now` and includes rotated-out tokens: health reports "expired" forever after any
   token expires. Filter to live tokens (and exclude rotated-out tokens past overlap_until), keep the ≤14-day warning.
2. MAJOR auth_lifecycle.rs:139-171 → lifecycle.rs:359-403 — POST /v1/auth/renew touches the control DB (admin
   connection, advisory lock, audit insert) for any junk bearer. Authenticate the presented token through the
   existing runtime lookup before any DB write; add a per-route limiter; junk requests produce no audit rows and
   no admin connections (test it).
3. MAJOR crates/oo-cli/src/admin/mod.rs:123,139 — native `oo token issue|list|renew|rotate|revoke|enroll|approve`
   return ADMIN_NOT_IMPLEMENTED / are missing; approve exists only in the npm wrapper. Implement them natively over
   the admin API (mTLS/admin auth as the existing harness), and let the server resolve a unique hash prefix from
   `list` output (ambiguous prefix → clear error). The npm wrapper delegates to the native binary.
4. minor auth_lifecycle.rs:25-37,111 / auth_device.rs:99 — one global 10/min limiter shared by enroll and device-code:
   separate buckets per route plus a per-peer bucket under the global cap.
5. minor crates/oo-cli/src/lifecycle.rs:680-686 (and auth.mjs:283-293) — write the token file (0600) before the
   post-login verification; on verification failure keep the file and report clearly (or self-revoke).
6. minor auth_lifecycle.rs:134, auth_device.rs:223, lifecycle.rs:695 — audit the redemption source: put the
   enrollment code_sha256 in related_sha256 plus the peer/workload; audit device-flow requests and polls.
Then run: cargo test -p oo-gateway -p oo-cli --locked, node tests, and the owned-PG runners (Docker; only if
`df -g /` shows ≥ 20 GB free — otherwise record that it did not run).

## Card B-S0S1 — lane B: Gateway token lifecycle, /v1/meta, expiry monitoring

TODO: S0-B-01, S1-B-01, S1-B-02. Lane label: `b`. Spec: REQ-TOK-01/02/04, AT-TOK-01, AT-TOK-03 (part). Decision D9.

Read: docs/product/04-onboarding-and-gateway-binding.md §5, docs/07-gateway.md §3 (esp. §3.4) and §9,
tools/gateway-token-store/ (CONTRACT.md, migrations/oo_token_control_v1.sql, src), crates/oo-gateway/src
(trusted_control.rs, http_gateway.rs, public_surface.rs, cache.rs, bin/issue-token.rs).
Never touch any running Gateway, LaunchAgent or token file on this machine.

Deliver:
1. S0-B-01: forward-only migration (new file; never change existing migration bytes) for max lifetime
   policy (default 90 d, configurable ≤365 d), first_issued_at, rotated_from / overlap_until, enrollment
   codes (hash only, single use, default 24 h expiry, redeemed_at/by), self-renew policy flag + window,
   audit events per operation. Applies on an empty DB and on a v1 DB.
2. S1-B-01: issue/list/renew/rotate/revoke/enroll in the store + admin API (reuse trusted_control
   patterns) + `issue-token` subcommands. renew keeps the value, deadline ≤ first_issued_at + max
   lifetime, not for expired/revoked; rotate gives a new value and keeps the old one until overlap_until
   (default 7 d); enroll returns `OO-XXXX-XXXX`, exchanged once at public `POST /v1/auth/enroll` for an
   `ooct_` token; `POST /v1/auth/renew` self-renew inside the policy window only; list never returns
   plaintext. Plaintext only in 0600 files or once in a response body; never logged.
3. S1-B-02: `GET /v1/meta` (gateway version, protocol version, minimum CLI version, current publication
   id, freshness) and nearest token / internal mTLS certificate expiry in `/v1/health` (warn ≤14 days).
4. Tests: every operation success + failure (expired, revoked, double redemption, beyond max lifetime, wrong
   audience, self-renew outside window/disabled, rotate overlap boundary, one audit row per operation, cache
   invalidation ≤5 s after revoke) on real PostgreSQL (postgres:17 container labelled oo.lane=b, or the
   repo's existing real-PG test harness).

Write scope: tools/gateway-token-store/**, crates/oo-gateway/src/{auth*.rs (new), trusted_control.rs,
http_gateway.rs, public_surface.rs, cache.rs, bin/issue-token.rs} and new files for this feature,
crates/oo-gateway/tests/token_*, crates/oo-gateway/Cargo.toml only if needed (report lock change). API
details go in tools/gateway-token-store/CONTRACT.md. Prefix `gateway` / `token-store`.

# Replay-safe first-user issuance

The existing authenticated `POST /internal/v1/tokens` `issue` operation accepts
an optional `retry` object with `key` and `token` fields. Ordinary issuance is
unchanged. The retry key is a bounded identifier; the token is a credential,
not a label. The trusted administrator must generate it from 32 random bytes,
encode it as the usual `ooct_` base64url token, and durably save it in a private
file **before** dispatch. Never put request material in argv or logs.

The project transaction lock serializes concurrent attempts. The new
`oo_token_lifecycle.issue_retry` table stores only hashes of the key, normalized
grant plus authenticated actor, and token. The map, token and audit commit in
the same transaction. A matching replay returns the same credential and its
current expiry; changed grants, actors or material fail closed. Revoked,
expired or rotated credentials are never recreated by replay.

Run the existing `issue-token install` operation to install the additive
`oo_token_issue_retry_v1.sql` extension. Its own manifest is checksum-verified;
the accepted v1 and v2 migrations are unchanged. The Gateway runtime role cannot
read this ledger. Existing non-retry operations remain compatible with a
v2-only installation; retry issuance requires the extension.

Validation:

```sh
cargo check --workspace --locked
cargo test --locked -p oo-gateway
python3 tools/gateway-token-store/test_issue_retry.py --run-id UNIQUE-RUN-ID
```

The last command starts and stops its own native PostgreSQL 17.11 under
`evidence/`, tests real transactions over TLS, and never uses an existing
database or a container. The lifecycle module is compiled by `oo-gateway`;
the standalone legacy store crate does not compile it.

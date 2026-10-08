# Findings

## CSRF

`getIdentityCurrent` stores a new CSRF hash on every success. Comparing `X-OO-CSRF` with that new token rejects a token the caller just received.

Mutations now call pinned `POST /control/v1/auth/bootstrap/consume` with an unknown 43-character token before `getIdentityCurrent`. The identity server checks the stored CSRF hash first. A mismatch returns `csrf.rejected` and leaves the session in place. A match then returns `bootstrap.rejected` or `bootstrap.unavailable` and rolls back the consume. The producer then writes one `bootstrap_rejected` audit row. The session is not revoked and the CSRF hash is not rotated by that call. The following `getIdentityCurrent` rotates it, so the next mutation needs a fresh current token.

Subject, projects, capabilities, and revision still come only from the typed current envelope. Caller JSON still fails `try_from_caller_json`.

## Identity

Private archive only: `a6f92da8a0bf872f8a30efb116c2c7b14463505b`, evidence `dc434a4a6aae3509f6a9fcb3218a2167649ef9e1`. The repo tree `tools/control-identity` was not modified. The hold append exists only under `/tmp/oo-a373/archive/identity`. Configured audience stays distinct from `client_id`. The test issuer's `Mode::Ok` puts `client_id` in `aud`. This is a test issuer. It is not a customer IdP certificate. a375's audience finding is the reason `a6f92da8` exists. That product still awaits independent acceptance.

Debug binary SHA-256: `84a71a2e2c1964b460f01ac1bf7d7ad5ad40de5eefc26e115041bc3c5093628c` for `control-identity`. `96f66f97bf26fd7bc4a449efb7afe5b94450bc9ec41f85df2c394a347be58ecf` for `review-ledger` from `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b`. The ledger receipt remains a command hash, not semantic membership proof. The joint decision left `ledger_authenticated` false.

## Model and 100k

No producer binary for `oo-pipeline`, `pg-builder`, `selfbuild-rules`, or `selfbuild-suggestions` is in the target volume. The 100k COPY files were not loaded. `oo_identity`, `oo_selfbuild`, and `oo_review` were not dropped.

`cli-proxy` listens on host `127.0.0.1:8317`. Inside the bridge network, `127.0.0.1:8317` is connection refused and `host.docker.internal:8317` accepts a TCP connection. The service config and the generated pg-builder endpoint require origin `http://127.0.0.1:8317`. Host networking was not used.

## Other limits

The profile producer sets `statement_timeout` to 10s and does not set `lock_timeout` to 1s. The 300s snapshot bound is the child deadline. One ledger database stays bound to one project. Column-level label decisions are omitted from the operator review document unless a complete column set is sent. Pack settings stay structural. A job whose S9 and S10 are already succeeded is still marked succeeded at stage `S8` by the worker. That path was not run here.

Operation IDs, OpenAPI, JSON schemas, and the migration were not changed.

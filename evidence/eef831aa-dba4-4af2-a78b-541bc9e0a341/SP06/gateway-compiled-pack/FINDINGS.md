# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a341`. The passing proof is `linux/smoke-3/`. Product commit `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c`. Earlier exits stay in `linux/smoke/` and `linux/smoke-2/`.

## F1. a337 declared digest no longer loads

a337 `FINDINGS.md` recorded `pack_digest_check=declared`. The operator fixture had no `compiler` field. Substituting `pack_digest` with `ab` repeated 32 times still loaded and was returned. A file SHA-256 of 64 zeros exited 1 before `listen.addr`.

`linux/smoke-3` `live-test.stdout` records `compiled_role_pack_metadata_hides_unclassified_pii ... ok`. That test loads packs emitted by `role-pack-bundle` from `cd8f754`, compares HTTP `pack_digest` with the bundle's `pack_digest` string, and compares each returned file SHA-256 with SHA-256 of the artifact text. The same test rewrites the declared digest to `ab` repeated 32 times and removes `compiler`. Both processes exit 1 with no `listen.addr` and stderr containing `oo-gateway: packs`. `load_dir` returns `pack_digest`, `legacy_unsupported`, `source_digest`, `path_duplicate`, and `budget` for those edits. The unit test `operator_fixture_is_legacy_unsupported` covers the old fixture shape.

## F2. Role-visible columns and the hidden column

The public fixtures are compiler input, not a handwritten sealed pack. Picker visibility allowlists `order_id` and `picker_note`. Auditor visibility allowlists `order_id` and `auditor_phone`. `unclassified_pii` has no `pii` object. The compiler refused that visibility with `classification.unknown_pii`. Neither sealed artifact contains `unclassified_pii`. Picker HTTP columns are `shop.orders` / `order_id` and `picker_note`. Auditor HTTP columns are `shop.orders` / `auditor_phone` and `order_id`. The README sentence "does not authenticate" is absent from the HTTP body.

## F3. Five-second cache, revoke, and the closed port

The live test uses the a337 timing. The revoke is committed before the warm request. That request, 1 second after the first 200, stays 200. The request at 5.2 seconds from the same start returns 401. A new process returns 401. The warm hit does not move the deadline.

With `endpoint.json` pointing at `127.0.0.1:1`, the request at 1 second stays 200 and the request at 5.2 seconds returns 401. `/ready` stays `database=not_probed`. A restart during the outage returns 401. Restoring the endpoint returns 200 with the auditor digest and columns. The test duration is 11.44 seconds, which is the two 5.2 second deadlines plus the requests around them.

## F4. Auth and SQL bounds that this process still has

Forged `X-OO-Role`, `X-OO-Project`, and `X-OO-Scope` return 403 `FORBIDDEN`. A session token returns 401 `UNAUTHENTICATED`. A missing bearer returns 401. `/health?marker=query-must-not-echo` returns 400 `QUERY_NOT_ALLOWED` and does not echo the marker. `POST /v1/sql` returns 501 `SQL_UNSUPPORTED` with a bearer and without one. Loopback still passes its 8 tests, including the body limit, the single permit, and the 408 timeout. Those loopback tests were not edited.

## F5. What the loader does not do

The gateway does not run the compiler and does not link `oo-builder`. Unicode NFC folding stays in the compiler. The loader checks path shape and exact duplicate paths. `load_operator_dir` still maps every `PackError` to the static string `packs`, so the process stderr does not carry `pack_digest` or `source_digest`. Those reasons are asserted through `PackCatalog::load_dir`.

`fixture_artifact` ignores its arguments and returns `legacy_unsupported`. `crates/oo-gateway/tests/http_gateway_live.rs` was not edited and was not run. Enabling `OO_A326_LIVE=1` would fail that ignored test.

Root `Cargo.toml` and `Cargo.lock` were not modified. The smoke-3 lock compare exited 0.

## F6. Parent gaps stay open

Health and the session body still set `authentication` to `loopback-http` and `external_mtls`, `oauth`, and `idp` to false. This process does not terminate public client TLS, perform an OAuth exchange, or call an IdP. Readiness does not probe PostgreSQL. Durable audit is not in this child. SQL is not executed. This packet does not close E45. An independent review of commit `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c` against `testing/real-db/gateway-compiled-pack/CONTRACT.md` is still required.

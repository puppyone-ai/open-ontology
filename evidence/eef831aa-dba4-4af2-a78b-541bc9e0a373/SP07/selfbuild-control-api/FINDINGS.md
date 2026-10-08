# Findings

## CSRF

`getIdentityCurrent` stores a new CSRF hash on every success. Comparing `X-OO-CSRF` with that new token rejects a token the caller just received.

Mutations call pinned `POST /control/v1/auth/bootstrap/consume` with an unknown 43-character token before `getIdentityCurrent`. The identity server checks the stored CSRF hash first. A mismatch returns `csrf.rejected` and leaves the session in place. A match then returns `bootstrap.rejected` or `bootstrap.unavailable` and rolls back the consume. The producer then writes one `bootstrap_rejected` audit row. The session is not revoked and the CSRF hash is not rotated by that call. The following `getIdentityCurrent` rotates it, so the next mutation needs a fresh current token.

Subject, projects, capabilities, and revision still come only from the typed current envelope. Caller JSON still fails `try_from_caller_json`.

## Identity

The shop run used fixed identity service `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. That implementation is independently accepted. Evidence packet `dc434a4a6aae3509f6a9fcb3218a2167649ef9e1` was not modified. The repo tree `tools/control-identity` was not modified. Contract `089b00d7f70da1bc42f7e201dfa070748c651d5b` was not modified. This run does not claim a customer IdP certificate.

Debug binary SHA-256: `84a71a2e2c1964b460f01ac1bf7d7ad5ad40de5eefc26e115041bc3c5093628c` for `control-identity`. `96f66f97bf26fd7bc4a449efb7afe5b94450bc9ec41f85df2c394a347be58ecf` for `review-ledger` from `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b`. The ledger receipt remains a command hash, not semantic membership proof. Every converge-ledger `authenticated` value stayed false.

## Shop 100k

The green builder reused the existing producer binaries. `oo_identity`, `oo_selfbuild`, `oo_review`, `oo_fixture`, `oo_selfbuild_shop`, `oo_review_shop`, and `oo_review_converge` were not dropped. The 100k fixture was already loaded and was not copied again.

`cli-proxy` listens on host `127.0.0.1:8317`. Inside the bridge network that loopback address is refused and `host.docker.internal:8317` accepts TCP. Product config and the generated builder endpoint keep origin `http://127.0.0.1:8317`. The success phase forwards that address inside the builder. The failure phase runs first and records `generate.failed`.

The compiler requires a `column_decisions` entry for every outcome column, including hidden `phone` and `national_id`. Those hidden columns were explicit rejects. `shop.party` and `shop.party.display_name` were explicit approves with overwrite true. `shop.entry.memo` was an explicit defer. The server fills ledger `current_sha256` from `curated.json` and strips it from `operator.json`, which is the field shape the producer applies. `field_candidates_applied` is true and `reseal_count` is 1 on both convergence packs. The pack file hash is `c4f5c00c3ac9fcd640c476a3135126b486212d2317761214d1d269fdf445653c`.

Catalog captures of this fixture differ only in `captured_at`. That timestamp changes the seal request hash and therefore the model cache key. After a live catalog read, the worker adopts a stored catalog of the same schema hash when the rest of the document matches. The second build then recorded 0 misses and 0 opened sockets. The first convergence build remains the fresh miss: 3 misses, 3 opened sockets, cost `unknown`.

A killed child used to be stored as a stage failure, and `set_job` could overwrite `cancel_requested`. The worker now leaves a killed child unmarked as a producer failure and waits for the cancel commit. The green cancel is `b_369d1e63bae4faa084ffc0c3474dd4a1`: status `canceled`, S9 still `awaiting`, pack GET 403. S0–S8 of that job had already succeeded before the cancel landed, so this observation is a checkpoint cancel of a running job, not a mid-generate kill. The earlier S0 cancel `b_ad9189fb55129fb4447c4743584be58f` remains `canceled`.

Job `b_f36a05605491fe53429435921539158c` was not reclaimed. It is still `running` at stage column S5 with S9 `awaiting` and `reseal_count` 0. It is retained failed-attempt history, not the convergence result.

Operation IDs, OpenAPI, JSON schemas, and the migration were not changed. The profile producer still sets `statement_timeout` to 10s. The child snapshot deadline is the stage bound. `compiled_pack` stays `not_published`. PII classification does not issue a grant.

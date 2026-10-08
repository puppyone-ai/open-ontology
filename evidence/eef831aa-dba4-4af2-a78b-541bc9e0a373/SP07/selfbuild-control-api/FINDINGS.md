# Findings

## CSRF

`getIdentityCurrent` stores a new CSRF hash on every success. Comparing `X-OO-CSRF` with that new token rejects a token the caller just received.

Mutations now call pinned `POST /control/v1/auth/bootstrap/consume` with an unknown 43-character token before `getIdentityCurrent`. The identity server checks the stored CSRF hash first. A mismatch returns `csrf.rejected` and leaves the session in place. A match then returns `bootstrap.rejected` or `bootstrap.unavailable` and rolls back the consume. The producer then writes one `bootstrap_rejected` audit row. The session is not revoked and the CSRF hash is not rotated by that call. The following `getIdentityCurrent` rotates it, so the next mutation needs a fresh current token.

Subject, projects, capabilities, and revision still come only from the typed current envelope. Caller JSON still fails `try_from_caller_json`.

## Identity

Private archive only: `a6f92da8a0bf872f8a30efb116c2c7b14463505b`, evidence `dc434a4a6aae3509f6a9fcb3218a2167649ef9e1`. The repo tree `tools/control-identity` was not modified. The hold append exists only under `/tmp/oo-a373/archive/identity`. Configured audience stays distinct from `client_id`. The test issuer's `Mode::Ok` puts `client_id` in `aud`. This is a test issuer. It is not a customer IdP certificate. a375's audience finding is the reason `a6f92da8` exists. That product still awaits independent acceptance.

Debug binary SHA-256: `84a71a2e2c1964b460f01ac1bf7d7ad5ad40de5eefc26e115041bc3c5093628c` for `control-identity`. `96f66f97bf26fd7bc4a449efb7afe5b94450bc9ec41f85df2c394a347be58ecf` for `review-ledger` from `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b`. The ledger receipt remains a command hash, not semantic membership proof. The joint decision left `ledger_authenticated` false.

## Shop 100k

The final builder reused the existing producer binaries. `oo_identity`, `oo_selfbuild`, `oo_review`, `oo_fixture`, `oo_selfbuild_shop`, and `oo_review_shop` were not dropped. The 100k fixture was already loaded and was not copied again.

`cli-proxy` listens on host `127.0.0.1:8317`. Inside the bridge network that loopback address is refused and `host.docker.internal:8317` accepts TCP. Product config and the generated builder endpoint keep origin `http://127.0.0.1:8317`. The success phase forwards that address inside the builder. The failure phase runs first and records `generate.failed`.

Stored confirm tokens stay in the ledger. The compiler file maps `flag` to `boolean_flag`, `label` and `text` to `free_text`, and `name` to `person_name`. `amount` maps to `quantity` because semantic type `money` requires a column unit and the sealed `shop.entry.amount` column has none. A unit is not written. Unknown tokens return `malformed`. `generation.json` and `pack.json` are parsed without the 65536-byte HTTP body cap.

Column label decisions are omitted from `operator.json`. The producer rejects a partial `column_decisions` array, and defer or approve is not invented for the other columns. `field_candidates_applied` is false. `reseal_count` is 0. Outcome is `reviewed_unchanged`. Effective columns carry the bound semantic types and confirmed classification. Descriptions and caveats are empty, so the role renderer has no prose to mark `UNREVIEWED`. Declared foreign keys render as source-declared and not a grant. Hidden `phone` and `national_id` are absent from both role texts. `compiled_pack` stays `not_published`.

S9 remains `awaiting` on `b_f36a05605491fe53429435921539158c`. The job row stays `running`. Cancel of a later build, `b_ad9189fb55129fb4447c4743584be58f`, reached `canceled` at S0 after status `running` was observed. Its pack GET is 403.

Operation IDs, OpenAPI, JSON schemas, and the migration were not changed. One ledger database stays bound to one project. The profile producer still sets `statement_timeout` to 10s. The child snapshot deadline is the stage bound.

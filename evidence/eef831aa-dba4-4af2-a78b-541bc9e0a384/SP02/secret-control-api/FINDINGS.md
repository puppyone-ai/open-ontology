# Findings

Author notes for session `eef831aa-dba4-4af2-a78b-541bc9e0a384`. These are delivery notes. They are not an independent acceptance.

## F1. Identity current rotates CSRF

`tools/control-identity` `current()` mints a new CSRF token and the session slide stores that hash. Comparing a token just returned by `GET /control/v1/identity/current` with the next mutation fails `csrf.rejected`. This card does not edit `tools/control-identity`.

`tools/secret-control-api` follows the reviewed selfbuild consumer. Before `fetch_current`, it `POST`s `/control/v1/auth/bootstrap/consume` with the session cookie, the exact `https_origin`, the presented `X-OO-CSRF`, and a 43-character unknown bootstrap token. A missing bootstrap row returns `bootstrap.rejected` and rolls the transaction back, so the stored CSRF hash is unchanged. A code that starts with `bootstrap.` means origin and CSRF matched. `fetch_current` then reads capabilities, and that read rotates CSRF. A process-wide mutex serializes confirm plus fetch. The live test refreshes CSRF before each successful mutation. Wrong CSRF stays `csrf.rejected`. Bad origin is rejected locally and by identity.

Attempt 08 is the 403 that showed this. Attempts 09 through 12 are later 503s after the probe itself succeeded. The green `author-proof.json` case `csrf` is 403 `csrf.rejected`, and `parallel_rotate` is one 200 plus one `csrf.rejected`.

## F2. Idempotency JSONB parameter

`INSERT_IDEMPOTENCY` passed a Rust `String` to `$4::jsonb`. The driver rejected it before PostgreSQL assigned a SQLSTATE: parameter 3 (0-based) could not convert `String` to `jsonb`. The statement in `src/sql.rs` is now `($4::text)::jsonb`. The migration file was not edited. Attempt 12 holds that driver line. The green write test then committed version rows, including the idempotent replay and the idempotency conflict.

## F3. Half-written rotation does not become current

`finish()` commits only on `Ok`. A unique nonce violation or a stale `expected_version` returns an error and drops the transaction. The scripted nonce pair `[1;12]`, `[1;12]` returns `version.conflict` with head version 1 and one row. The following nonce `[2;12]` commits version 2 and two rows. `stale_rotate` on the binary path is `version.conflict`.

## F4. Restart ciphertext

`resume.sh` restarts the same `oo-a384-pg` volume after `live_secret=write_done`. It does not reinstall and it does not regenerate the master key. The restart test opens `secret://lab/alpha/builder/db` at version 2 and refuses the retired version. `run.json` has no value. The host script overwrites the witness and records `witness_absent=true`.

## F5. Gates left open

Customer KMS is an external gate. `key_source=kms` refuses startup and does not unwrap. mTLS CA issuance is not claimed. The listener verifies an operator-supplied client CA. The test IdP is not a customer IdP. Foreign pool refresh, Gateway, and Valkey are not claimed. The ledger was not edited.

## F6. Fixture schema reset

`resume.sh` drops `oo_identity_control` and `oo_secret_control` on the owned databases before the write builder, so install can record `committed` and then `replayed`. That reset is fixture setup. Product code does not query `oo_identity_control`. The green launch log contains the `DROP SCHEMA` notices. The restart phase does not drop the schema.

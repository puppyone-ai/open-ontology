# Findings

## F1. WITHHOLD original E54 — ID Token audience is not bound to `client_id`

Clause: OIDC Core 1.0 §3.1.3.7. The client must validate that `aud` contains its registered `client_id`.

Evidence: `linux/raw/independent-proof.json` case `audience_client_id_binding`, and `audience_probe.binding_holds=false`.

Configuration under test: `issuer` was the private HTTPS test issuer, `audience` was `configured-aud`, `client_id` was `rp-client`. Tokens were RS256, signed by a key published at the issuer JWKS.

- `aud=other-aud`, `azp=rp-client`: HTTP 401, `audience.rejected`.
- `aud=rp-client`, `azp=rp-client`: HTTP 401, `audience.rejected`.
- `aud=configured-aud`, `azp=rp-client`: HTTP 302, and the following current call was HTTP 200 for `fixture-admin`.

`claims` in `src/crypto.rs` accepts the token when any `aud` entry equals `binding.audience`. `azp`, when present, is compared with `binding.client_id`. A single audience omits that comparison when `azp` is absent. The frozen contract states the same rule in the ID Token table: "`aud` contains the configured audience".

When the operator sets `audience` equal to `client_id`, a token for that shared value is accepted and `other-aud` is rejected (`bound_arbitrary_aud`). That is a configured-audience check. It is not a check that `aud` contains `client_id` for every configuration the file format allows.

`APPROVE_LIMITED` was not reached. `binding_holds` is true only when `other-aud` is rejected, `aud` equal to `client_id` returns 302, and the distinct configured audience is rejected. The third leg failed.

No product patch was applied.

## F2. Retained note — table owner can TRUNCATE `audit_event`

`linux/raw/truncate-note.txt`:

```
audit_event owner=oo_identity_admin
trigger=CREATE TRIGGER audit_event_append_only BEFORE DELETE OR UPDATE ON oo_identity_control.audit_event FOR EACH ROW EXECUTE FUNCTION oo_identity_control.reject_mutation()
```

The trigger is `BEFORE UPDATE OR DELETE`. It does not cover `TRUNCATE`. The table owner can `TRUNCATE`. This review retains the note. It did not run `TRUNCATE` and it did not open an unrelated database campaign. The service does not issue `TRUNCATE`. This note is not a second withhold by itself.

## Passed measurements (not findings against the build)

Recorded in `independent-proof.json` with `ok=true` (68 cases, 0 failed on the final run):

- Management roles come from operator bindings. Extra ID Token claims `role`, `roles`, `groups`, `data_grants=oo_r_all`, and `authorized=true` left the auditor on project `delta` with the auditor capability set. `project.admin` stayed `forbidden`. An unbound subject received no projects. Envelope fields stayed `data_grants=none`, `owner_certification=not_issued`, `agent_session=not_issued`, `harness_session=not_issued`, `gateway_token=not_issued`.
- Caller JSON does not authenticate. `POST` current returned 405 `method.not_allowed`. `POST` logout with a JSON body returned 400 `malformed` and no session cookie. `VerifiedSession::try_from_caller_json` returned `unauthenticated`.
- Cookie attributes, CSRF, Origin, PKCE, state, nonce, issuer, signature, `alg=none`, expiry, future `iat`, redirect allowlist, bootstrap single-use, JWKS duplicate `kid` (one fetch), unknown `kid` (two fetches), bounded rotation (two fetches), restart survival, and restart revocation behaved as the case list records.
- Child registry checksum triples are a subset of the root lock: count 225, missing 0.

Actual response bodies, with `csrf_token` replaced by `REDACTED`, are in `responses/actual/` next to the copied schemas in `responses/schema/`.

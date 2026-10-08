# FINDINGS

Verdict: **WITHHOLD**. Bound to this session's `linux/proof/private-pg.json`, `linux/csrf/probe.json`, `linux/csrf/observe.json`, and the archives in `linux/archive/trees.txt`. A passing live test does not accept the card, SP08, or the parent goal.

The author's packet is not the contract. Frozen `CONTRACT.md` blob `dc64a53f5f54b8f4b080ecaa8561a16605d65d34` at `fbd0467617a3c1514e7ffbef52f1138e20416ac6` still says every request forwards only the `__Host-oo-session` cookie to exactly `GET /control/v1/identity/current`, and that a mutation requires `X-OO-CSRF` equal to that envelope's `csrf_token` plus `identity_revision` equal to `revision`.

## Fix 1. Stop using bootstrap consume as a CSRF oracle

`src/identity.rs` blob `57d4eeb050886877977dfe677d8022e23993ca41` defines `UNKNOWN_BOOTSTRAP` as 43 `A` characters and `CONSUME_PATH` as `/control/v1/auth/bootstrap/consume`. `confirm_presented_csrf` posts that token with the caller cookie and the caller `X-OO-CSRF`, and sets `Origin` to the configured identity origin. Status 403 or 409 whose `error.code` starts with `bootstrap.` is treated as CSRF success. Status 200 falls through after the consume. The following `GET /control/v1/identity/current` is then checked for revision only.

On identity `a6f92da8a0bf872f8a30efb116c2c7b14463505b`, `Store::current` mints a new opaque token, stores `sha256` of it in `csrf_hash` (`SLIDE_SESSION`), and returns the new plaintext. The header the client just presented cannot equal `csrf_token` in that same response. `consume_post` checks origin before the CSRF hash. An unknown token with a valid CSRF and the right origin returns `bootstrap.rejected` and `Store::consume_bootstrap` commits `INSERT` into `oo_identity_control.audit_event` with action `bootstrap_rejected`. The session hash is left in place.

`linux/csrf/probe.json` measured that over HTTP:

- `current_rotated` true, and `stored_hash_is_second_token` true.
- Stale header, real origin: 403 `csrf.rejected`, `stale_audit_delta` 0, hash unchanged.
- Fresh header, wrong origin: 403 `origin.rejected`, `origin_audit_delta` 0.
- Fresh header, real origin, unknown token: 403 `bootstrap.rejected`, `consume_audit_delta` 1, hash unchanged and equal to the presented token.
- `bootstrap_states` is `["consumed"]`.

`linux/csrf/observe.json` on the product database `oo_a404` after `private_postgres_publish_and_rollback`: `bootstrap_rejected` / `bootstrap.rejected` count 16. Those rows are the probe's write, once per CSRF-valid mutation the live test performed.

Required change, not done here:

- Delete the consume call. Do not edit `CONTRACT.md` to bless the probe, and do not replace the equality sentence with a 43-character shape check.
- The identity owner (session 402, the only writer of `tools/control-identity`) adds a no-write confirm: compare the presented token with the stored hash, return the current capability envelope, and write nothing. No CSRF rotation, no idle slide, no audit insert, no consume, no revoke.
- Until that route exists and this service calls it, release mutations fail closed.

Commit `e2bd526e96175efb52fa1181843fbe991ae7992b` appeared after the archive. Its contract text drops the equality, states that a 43-character token which does not match the hash is not rejected, and says this service does not call consume. That commit was not compiled. It is not the fix above.

## Fix 2. Bind pack membership in the read path

`docs/06-ontology-layer.md` §7.1 compiles one Knowledge Pack per `oo_r_<role>` and mounts that pack for the user's role. `specs/open-ontology-v0.1-plan.md` SP08-W04 publishes an immutable candidate, closes old and new access for the affected roles, then CAS-activates.

The executed producer pin does not enforce that on the read the API serves:

- View `oo_release_control.publication_status` is `FROM oo_release_control.active_pointer WHERE singleton`. It does not select `project_id`. The five certificate columns are SQL `FALSE`. `observe.json` `pg_get_viewdef` contains `singleton` and does not contain `project_id`. Column list: `version`, `candidate_id`, `source_hash`, `pack_digest`, `manifest_sha256`, and the five false flags. The API `pointer()` reads that view with no project parameter and the HTTP layer stamps `runtime.project_id()` from process config.
- `UPDATE_POINTER` does bind the write: `WHERE singleton AND project_id = $6 AND version IS NOT DISTINCT FROM $7`. The read used for `getActiveRelease` does not.
- `READ_FILES` is `SELECT path, sha256, byte_len FROM oo_release_control.candidate_file WHERE candidate_id = $1 ORDER BY path`. No role predicate. `observe.json`: `candidate_file_policies` 0, `candidate_file_rls` false, `candidate_file_count` 81. `getActiveRolePack` returns those files when `operation.role_name` equals the query role and the receipt `pack_digest` matches. It does not ask whether the caller is a member of that `oo_r_` pack.

Identity `a6f92` keeps management bindings in the operator file. `projects_for` filters `config.bindings` by subject in process memory. `capabilities_for("auditor")` is `audit.read` and `identity.current`. `capabilities_for("admin")` includes `project.admin`. None of those roles is an `oo_r_` role. `VerifiedSession::try_from_caller_json` returns `unauthenticated` for every caller body. The release parser requires the fixed identity envelope, so caller JSON is not authority. The product then maps `project.admin` or `audit.read` to `Scope::Pack`.

The live proof shows the consequence: step `auditor-pack` is 200 `getActiveRolePack` on the active wide version, step `reviewer-pack` is 403 `capability`, step `role-mismatch` is 403 `role`. That separates management capabilities. It does not show that the auditor is a member of the sealed clerk pack.

The dirty identity worktree, listed in `linux/build-1/identity-worktree.txt` and not compiled, has `READ_BINDINGS` equal to `SELECT project_id, subject, role FROM oo_identity_control.management_binding` with no `WHERE`. The untracked table check allows only `admin`, `reviewer`, `steward`, `auditor`, and `business_owner`. That SELECT does not bind pack membership either. This review did not compile it and does not ask 402 to treat it as the fix.

Required change, not done here:

- `publication_status` and the pack file read must be predicated on the caller's membership in the sealed pack, including `project_id`.
- Do not treat `project.admin` or `audit.read` as that membership.
- Do not stamp `project_id` from process config when the reader SQL does not contain it.

## Fix 3. Do not sign production

The view forces `production_ready_certificate` and `full_sp08_accepted` to false. The proof fields are `production_ready` false, `full_sp08_accepted` false, `customer_approval` `not_issued`, `test_scope` `not_production`. Omitted G7 and a fabricated fence stay `not_ready` with a null pointer. Production scope is `production_blocked` on an empty pointer and again while `ver-n` is active. `test_issuer`, a missing authenticated owner, missing runsc, and a fence or replica that is not production-admissible stay failed. SP08-W04 says a G10 fixture is not a customer result. No customer gate was presented to this review.

Required change, not done here: leave those flags false until a real customer gate exists. This review does not sign production.

## Behaviour the live test did show

Two test-scope versions, `ver-n` then `ver-w`, class `test_scope_not_production`. Missing gate and fabricated fence leave the pointer null. Idempotency replay is 202. A different body is 409 `idempotency.conflict`. Cross-project current and a missing operation are both 403 `forbidden` detail `scope`. Restart of a running row with no candidate is `resume_uncertain` and leaves `ver-n2`. A queued publish finishes after restart at `ver-restart`. Rollback to the narrower pack returns `rolled_back` / `not_issued`. A wider restore is 409 `compat.refused` before the producer would widen access.

Those results stay inside the test fixture. They do not clear the three fixes.

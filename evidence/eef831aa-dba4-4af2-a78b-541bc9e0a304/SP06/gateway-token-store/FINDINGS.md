# Findings

Reviews of the frozen GW-D01 / SP06-W01 gateway token store. These reviews do not accept the parent, SP06-W01, or a release.

## 1. Contract against docs/07 section 3

`tools/gateway-token-store/CONTRACT.md` was on disk before the implementation files. It matches docs/07 section 3 on the points this unit owns.

- Session token `oost_`, client token `ooct_`, opaque server lookup, hash stored, plaintext not a JWT.
- Session idle 30 minutes and absolute 8 hours. Client default 1 hour. Service-account maximum 90 days.
- One role. Claims frozen at issue. Audience compared as the gateway canonical URL.
- Positive cache at most 5 seconds. After expiry, an unavailable store refuses. A warm entry is not extended when the store is down. Revocation of a fresh lookup is visible once the database commit is visible. Pub/sub is named in the doc and is explicitly unimplemented here. The contract says the TTL is the bound.
- Principal, role, and claims are stored data. The adapter does not authenticate an IdP and does not terminate mTLS. `DeclaredTransport` records a transport decision made outside this crate. That matches the frozen card. It is not an implementation of the doc's mTLS handshake or OAuth authorization server.

Fail-closed extras beyond the one-line role rule in the contract: `one_role` also refuses `+` and the names `public`, `admin`, `oo_token_admin`, `oo_token_runtime`, and `oo_fixture_admin`, all as `role_merge`. The contract's forbidden claim keys match `canon.rs`.

Out of scope in both the contract and the code: public HTTP auth, OAuth/PKCE/refresh, RFC 8693, Redis, audit WAL, agent SQL, session-lease certificates, query cancel.

## 2. Source: hash only, roles, no forged auth

Reviewed `src/token.rs`, `src/store.rs`, `src/trusted_control.rs`, `src/canon.rs`, `src/model.rs`, `src/sql.rs`, and `migrations/oo_token_control_v1.sql`.

- `PlainToken::sha256_hex` hashes the prefixed UTF-8 token. `token_hash` does the same on lookup. Insert parameters are the hex hash and the frozen columns. The plaintext is not a SQL parameter.
- `Debug` for `PlainToken` is `PlainToken(redacted)`. `Drop` zeroizes the buffer with `zeroize` 1.9.0 `alloc`. `getrandom` 0.2.17 is default-features off. `rand` is not a direct dependency. `draw_32` returns 32 bytes and does not print them.
- `TrustedControlAdapter::new` returns `login_role` unless the login is `oo_token_admin`, and it does not connect. `lookup` / `lookup_for_test` require `oo_token_runtime` before `token_hash`. There is no `authenticated`, `mtls`, or `caller_authenticated` field on `IssueGrant` or `LookupRequest`.
- Receipts use `ControlFlags` with `auth_provider_used`, `idp_used`, `mtls_authenticated`, and `pubsub_used` set false in `control_flags`.
- `classify` order is project/principal/policy/knowledge `stale`, audience `audience`, role `role`, claims `claims`, then sandbox/session/transport `binding`.
- Forbidden claim keys return `forged_auth_claim` before a row is inserted. The live test `forged_claims_and_role_merge_never_issue` saw a zero row count after those refusals.
- Runtime grants in the migration are `USAGE`, `SELECT` on the three tables, and `UPDATE (last_used_ms, idle_deadline_ms)`. The shape check's `EXPECTED_PRIVILEGES` matched PostgreSQL 17.10: install succeeded, and granting `INSERT` to runtime was `install_drift` on `privilege_token_insert`.
- Schema check constraints reject a `token_sha256` that matches `^(oost_|ooct_)`. The live scan uses `position`, so `_` is not a LIKE wildcard.

## 3. Tests and raw exits against the frozen cases

Final raw exit is `linux/proof/docker-run.exit` = 0 and `linux/proof/console.log` `test result: ok. 18 passed` in 10.51s, then Clippy `Finished`, then `PHASE proof_ok`. Host config: ExitCode 0, OOMKilled false, 6 GiB, 1 CPU.

| Frozen case | Test | Result in that log |
|---|---|---|
| Issue stores the hash; schema has no raw token | `issue_stores_the_hash_only_and_samples_randomness` | passed |
| Randomness sample, no quality claim | same test, plus `token::os_csprng_sample_has_no_quality_claim` | `randomness.txt` `quality_claim=false` |
| Injected expiry; production clock separate | `injected_expiry_does_not_certify_the_production_clock`, `production_issue_reads_the_system_clock` | passed; production receipt `wall` is system and `lookup` is the production function |
| Stale tuple, audience, role, claims, binding | `stale_audience_role_claims_and_binding_are_refused` | passed |
| Forged claims and role merge never issue | `forged_claims_and_role_merge_never_issue` | passed |
| Revoke by token, session, principal, project; replay; no resurrection | `revoke_scopes_are_idempotent_and_do_not_resurrect` | passed |
| Concurrent issue/revoke | `concurrent_issue_and_revoke_cannot_resurrect` | passed |
| 5s cache, fresh lookup rejects, DB stop fails closed, restart still revoked | `cache_bounds_revocation_and_fails_closed_when_the_database_is_down` | passed; `remaining_ms=4598` at stop |
| Runtime grant refusal | `runtime_grants_and_wrong_callers_are_refused` | passed |
| Install drift fail-closed | `install_replay_and_drift_fail_closed` | passed |

Earlier exits kept: smoke 101, proof-1 101, proof-2 101, proof-3 1. proof-3's console shows the same 18 tests passing before Clippy was missing. The final run is the one that includes Clippy.

## 4. Lock subset, evidence scope, cleanup

`linux/proof/console.log` line `subset_ok registry=125 path=gateway-token-store` ran before `cargo fetch --locked`. The checker compares registry name, version, and checksum to the fixed root `Cargo.lock`. The only path package is `gateway-token-store`.

The hand lock does not add a version. proof-1 showed `--locked` rejecting a true encoding mismatch (package order and three unreachable packages left over from the session-store copy). That attempt is preserved. The edited lock dropped `itoa` 1.0.18, `serde_json` 1.0.151, and `zmij` 1.0.23, and dropped `serde_derive` from `serde`'s edge. Those packages remain available in the root lock. `serde` stays because the pinned `phf` edge in this resolve still names it. No root manifest write.

`git status` at report time showed only `tools/gateway-token-store/` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a304/SP06/gateway-token-store/`. Evidence does not contain a PEM private key or a password file. `pg/roles.txt` is `CREATE ROLE` / `REVOKE` / `GRANT` / `roles_ready`. `init-roles.sh` interpolates password files at runtime and does not store them.

Cleanup after the proof: `docker rm -f oo-a304-pg`, volume rm of the three `oo-a304-eef831aa-*` volumes, network rm of `oo-a304-eef831aa-net`, and removal of `/tmp/oo-a304-eef831aa-dba4-4af2-a78b-541bc9e0a304`. `cleanup.txt` shows each name absent afterward. No prune and no foreign container removal.

The full evidence SHA-256 manifest is produced after this file, by the std-only `tools/sha256_manifest.rs` (same bytes as a199, `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`). The manifest file itself is excluded from its own output. An inventory container's host inspect written after that process exits is outside the manifest.

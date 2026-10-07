# Findings

Independent review of the gateway token store frozen at `99ac19af42059a308ba9c418815619e5a081f407`. These findings do not accept the GW-D01 parent, SP06-W01, or a release.

## 1. Pre-overlay author suite

`linux/preoverlay` ran the unmodified crate from a git archive of `99ac19af` on a fresh private TLS PostgreSQL 17.10. `PHASE overlay_absent` ran before fetch. The author's live module saw `OO_A304_LIVE=1`, host `oo-a304-pg`, and database `oo_a304` through the second SAN and network alias on `oo-a314-pg`.

Raw exit `linux/preoverlay/docker-run.exit` is 0. The console lists these 18 tests passed in 10.40s, then Clippy `Finished`, then `PHASE proof_ok`:

- `cache::tests::a_hit_does_not_move_expiry_and_only_positive_rows_are_stored`
- `canon::tests::audience_and_claims_follow_the_contract`
- `live::cache_bounds_revocation_and_fails_closed_when_the_database_is_down`
- `live::concurrent_issue_and_revoke_cannot_resurrect`
- `live::forged_claims_and_role_merge_never_issue`
- `live::injected_expiry_does_not_certify_the_production_clock`
- `live::install_replay_and_drift_fail_closed`
- `live::issue_stores_the_hash_only_and_samples_randomness`
- `live::production_issue_reads_the_system_clock`
- `live::revoke_scopes_are_idempotent_and_do_not_resurrect`
- `live::runtime_grants_and_wrong_callers_are_refused`
- `live::stale_audience_role_claims_and_binding_are_refused`
- `sha256::tests::known_answers`
- `sql::tests::migration_does_not_autofix_or_store_a_raw_token`
- `store::tests::migration_digest_is_64_hex`
- `token::tests::os_csprng_sample_has_no_quality_claim`
- `token::tests::plaintext_debug_is_redacted_and_hash_matches_sha256`
- `trusted_control::tests::runtime_login_is_refused_before_connect_and_debug_redacts_the_password`

`linux/proof/phase/stop-window.txt` recorded `remaining_ms=4523` while Postgres was stopped. That is inside the five-second cache window. The author final proof's `remaining_ms=4598` is a separate run and was not repeated here.

Author `linux/proof-1/docker-run.exit` is 101. Author `linux/proof-3/docker-run.exit` is 1. Author `linux/proof/docker-run.exit` is 0. Those three files were cited from the author tree. This review did not recreate the hand-lock refusal or the Clippy-absent stop.

## 2. Hash, clock, tuple, and declared transport

`a314_hash_only_random_debug_and_system_clock` passed on `linux/overlay-2`.

- Two `draw_32` samples were 32 bytes, not all zeroes, and not equal. The record says `distinct_draws=true` and `quality_claim=false`. That sample is not a randomness quality claim.
- The session receipt starts with `oost_`, length 48. The stored `token_sha256` is 64 lowercase hex characters and equals `token_hash` of the revealed plaintext. A scan found no `oost_` or `ooct_` prefix and no plaintext in `token_sha256`.
- `Debug` of the receipt contained neither `oost_` nor the revealed plaintext. `LookupCache::expiry_nanos` on the plaintext returned none.
- Injected issue reported `WallSource::Injected`. Production `issue` and `lookup` reported `WallSource::System`, and `issued_at_ms` fell between two `SystemWall.now_ms` reads. `FixedWall` stayed on the injected calls.

`a314_tuple_audience_role_claims_and_declared_transport` passed.

- A matching tuple, audience, role, claims, and egress binding was accepted. A changed policy version was `stale`. A different gateway URL was `audience`. A different role was `role`. A changed claim was `claims`. `DeclaredTransport::ExternalClient` on a sandbox session was `binding`.
- Issue flags had `auth_provider_used`, `idp_used`, `mtls_authenticated`, and `pubsub_used` clear. The declared transport value is stored and compared. This crate did not perform an mTLS handshake.
- Role `analyst,admin` was refused as `role_merge` before an extra row. Claim key `authenticated` was refused as `forged_auth_claim`. The project still had one token row.
- A client token starts with `ooct_`. Presenting it with an egress binding was `binding`. A lookup whose CA was the wrong fixture certificate failed with `unavailable`, `tls`, or `connect`.

## 3. Deadlines, cache, grants, revocation, race

`a314_session_and_client_deadlines` passed on the corrected run.

- Constants observed: session idle `1_800_000` ms, session absolute `28_800_000` ms, client default `3_600_000` ms, client maximum `7_776_000_000` ms.
- An unused session token was `expired` at `issued_at + SESSION_IDLE_MS`. A fresh session token was accepted at idle minus one millisecond. A session token that was looked up before the absolute cap was accepted at absolute minus one millisecond and `expired` at the absolute deadline. The harness does not extend a deadline by a warm cache hit.
- A client token stored no idle deadline. Its absolute deadline was the default one hour. Lookup at that deadline minus one millisecond was accepted, and lookup at the deadline was `expired`. A supplied TTL of `CLIENT_MAX_MS` was stored. `CLIENT_MAX_MS + 1` was `input`. A session grant that carried a caller TTL was `input`.

The preserved `linux/overlay` failure called the idle-minus-one lookup first on the same token. That successful lookup slides `idle_deadline_ms`, so the original idle was still valid. `linux/overlay/harness.rs` line 440 is that order. `overlay/a314_review.rs` expires the unused token first.

`a314_cache_revoke_stop_and_restart` passed.

- The first lookup set `database_used`. The second did not, and the cache expiry instant was unchanged.
- After a committed token revoke, a new cache returned `revoked`. The already warm cache still served the row without a database read, and its expiry instant was unchanged.
- Postgres was stopped while `remaining_ms=4768`. The warm lookup during the outage did not use the database and did not move the expiry. After that remaining time plus 250 ms, lookup returned `unavailable`. The refusal did not move the expiry.
- After Postgres started again, lookup returned `revoked`. An `unavailable` result while the server was coming up was retried. An acceptance would have failed the test.

`a314_runtime_grants_and_install_drift` passed. Runtime SQLSTATE `42501` refused `INSERT` into `token_record`, `UPDATE` of `role_name`, `DELETE` from `revocation`, and `nextval` on `revocation_seq`. `UPDATE` of `last_used_ms` and `UPDATE` of `idle_deadline_ms` were accepted. Install replay returned `InstallDisposition::Replay`. An added column was `install_drift` on `columns_token_record`. `GRANT INSERT` to `oo_token_runtime` was `install_drift` on `privilege_token_insert`. After both were undone, install replay succeeded.

`a314_revoke_scopes_do_not_resurrect` passed. Token revoke was `Applied`, and the same revoke again was `Replay` with the same sequence. The revoked token stayed `revoked`. Session revoke made a later issue of that session id `scope_closed`. After a principal revoke, a new session id for that principal was accepted and the older token stayed `revoked`. Project revoke was idempotent. A later client issue in that project was accepted, and the revoked client token stayed `revoked`.

`a314_race_does_not_resurrect` passed with two worker threads. Issue and revoke of one session overlapped on `WaitGate`. The lookup after that overlap was `revoked`. Revoke-then-issue of a second session id was `scope_closed`, and `proj-a314-race` still had one token row.

## 4. Lock, product tree, evidence

Every builder printed `subset_ok registry=125 path=gateway-token-store` against the root lock content hash `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. That is the lock blob `2990f901ea98a7b465f462cce8742bc5c51c13aa` shared by `25dfc902`, `99ac19af`, and report HEAD `b5ee3f4c`. The subset checker did not read a newer root baseline.

`linux/overlay-2/product-lib-diff.txt` is empty. The archive diff adds only the cfg(test) module line. Crate digests that differ from `linux/proof/crate-manifest.sha256` are:

- added `256286c12ac18f4e81f7b40bac12375c473af91040197aeb7f22822394be72cc  ./src/a314_review.rs`
- `./src/lib.rs` changed from `6cac5d52aed4f82c6895744ee73fb2ebd6dd07732e753e9506086c8e1eaf14e5` to `faecfa811bc6a3edfe410234c8f398294acc313266d1858420c10079f1e2ab91`

`source/fixed-crate` matches the 16-file pre-overlay manifest. Evidence contains no `*.key`, `*.pass`, or `*.pem`. `cleanup.txt` shows the a314 container, three volumes, network, and tmp directory absent. No prune and no foreign name was removed.

The full evidence SHA-256 manifest is produced after this file by `tools/sha256_manifest.rs`. `linux/inventory/scope.txt` names the host files written after that walk.

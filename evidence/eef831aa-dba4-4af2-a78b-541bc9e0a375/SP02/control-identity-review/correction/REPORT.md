# Control identity audience recheck

Original finding F1 is closed on product commit `a6f92da8a0bf872f8a30efb116c2c7b14463505b`.

Clause: OIDC Core 1.0 §3.1.3.7. With `audience` `configured-aud` and `client_id` `rp-client`, a real RS256 ID Token is accepted when `aud` contains `rp-client`, and it is rejected when `aud` is only the configured audience.

Original E54 stays unaccepted. A customer IdP is not claimed. The SP02 parent stays unaccepted. The full goal stays unaccepted. Verdict file text: `FINDING_CLOSED`.

## Commit under test

The product commit is the one named by evidence commit `dc434a4a6aae3509f6a9fcb3218a2167649ef9e1`, in `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a379/SP02/control-identity-audience-repair/REPORT.md` and `HANDOFF.md`. `git archive` of `tools/control-identity` at that commit is `archive/`. All 28 paths match `linux/raw/product-files.sha256` from that repair packet, including `CONTRACT.md` `e8589e5c6cdec584657443a4b485ba2ecdf375b097eb6f3076388f6b8f79ceb5` and `src/crypto.rs` `c959508be662cc241f7b87f2c15f36eaa651e2bf41afee22adbecd95027024d6`. `archive-before.sha256` and `archive-after.sha256` match. `archive-immutable.exit` is 0.

The reviewed producer remains `35bd6b8157cecd89f2ae085b012b4b0137562ddb`. Schema and OpenAPI bytes in this archive match the hashes recorded for the `089b00d7f70da1bc42f7e201dfa070748c651d5b` publication. Copies are under `responses/schema/`.

## Smoke

Linux arm64, before `tests/audience.rs` was copied into the work tree. `smoke.exit` is 0. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

`rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Builder: 6 GiB, 1 CPU, `cap-drop ALL`, `no-new-privileges`, private network `oo-a375c-eef831aa-net`, no host port. `builder-hostconfig.txt` records memory 6442450944 and NanoCpus 1000000000. Final inspect: exit 0, OOMKilled false.

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 1 GiB, 1 CPU, no host port (`pg/ports.json` is `{}`). `init-roles.txt` is `ok`. OOMKilled false. `pg-wrap.sh` matches origin `a587031c719a484830d78e493605f48e2056cae4` at `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f`.

## What ran

Locked offline, `--test-threads=1`. Live and cli integration tests were not executed. The database work was one `install` and the login rows for the six legs below, then `DROP SCHEMA`. No `TRUNCATE`.

| Step | Exit |
|---|---|
| smoke | 0 |
| rustup-clippy | 0 |
| cargo-fetch | 0 |
| cargo-test-lib | 0 |
| cargo-clippy | 0 |
| cargo-test-audience | 0 |
| cargo-clippy-audience | 0 |
| archive-immutable | 0 |
| run-proof | 0 |

Library 11 passed, 0 failed, including `ring_signed_token_verifies_and_rejects_a_tampered_claim`. Clippy `--all-targets -D warnings` on the archive, then Clippy `--test audience -D warnings`, reported no warnings. Root manifests and both lockfiles were not modified. The child `Cargo.lock` hash is `1c078aa1ef4f877d43d693cc1aaecbf48317952f1359ce8f126ac6f9e27ddb40`, the same bytes as the original review archive. This run did not reparse the root lock. That original count remains 225 registry packages, missing 0, for those bytes.

## Audience measurement

Private HTTPS issuer. Code, token, and JWKS were served over TLS. The ID Token was RS256 (`id_token_alg` `RS256`). `verify_id_token` checks the signature before `claims`, so `audience.rejected` is a signed token that failed the audience rule. The service fetched JWKS once (`jwks_fetches` 1) and reused that key for the later legs inside the 60 second cache. `split_config` recorded `audience` `configured-aud` distinct from `client_id` `rp-client`.

| Leg | `aud` | `azp` | HTTP | Result |
|---|---|---|---|---|
| `aud_other` | `other-aud` | `rp-client` | 401 | `audience.rejected` |
| `aud_client_id` | `rp-client` | `rp-client` | 302 | current 200, subject `fixture-admin` |
| `aud_configured_only` | `configured-aud` | `rp-client` | 401 | `audience.rejected` |
| `aud_client_id_no_azp` | `rp-client` | absent | 302 | current 200, subject `fixture-admin` |
| `aud_multi_matching_azp` | `rp-client` and `extra` | `rp-client` | 302 | current 200, subject `fixture-admin` |
| `aud_multi_without_azp` | `rp-client` and `extra` | absent | 401 | `audience.rejected` |

`original_finding` is `closed`. `azp_rules` is `held`. Bodies are under `responses/actual/`. `csrf_token` values are the literal `REDACTED`. Accepted current bodies have `authority` `management_session`, `data_grants` `none`, `session_class` `management_cookie`, and `not_issued` for owner certification, agent session, harness session, and gateway token. `install_state` is `uninitialized` and `projects` is empty because this recheck did not consume bootstrap.

## Reused evidence

Not rerun. `linux/raw/reused-68-before.sha256` and `linux/raw/reused-68-after.sha256` both record:

- `independent-proof.json` `f917a652b0fcec042699852ea2a3d43d109ae3c5e40a3952f184eebb8ae82438`
- `independent-proof.verdict` `d880d5aac37b0bdf5ec323addf7c640da1b6abaf8982e245df918f512ba00c3b`

That packet is 68 cases, 0 failed, verdict `WITHHOLD`, on `35bd6b8157cecd89f2ae085b012b4b0137562ddb`. Its role, PKCE, state, nonce, issuer, expiry, JWKS rotation, restart revocation, caller JSON, and project revision measurements stay that packet's evidence. The table-owner `TRUNCATE` note there stays a retained note. This recheck did not repeat it.

## Cleanup

Owned `oo-a375c-proof`, `oo-a375c-pg`, volumes `oo-a375c-cargo`, `oo-a375c-work`, and `oo-a375c-eef831aa-pgdata`, network `oo-a375c-eef831aa-net`, and `/tmp/oo-a375c-eef831aa-dba4-4af2-a78b-541bc9e0a375` are removed. `oo-a373-pg` was still running. See `linux/raw/owned-cleanup.txt`. The host OpenSSL retry is in `linux/raw/FAILED-ATTEMPTS.md`.

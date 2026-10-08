# Control identity audience repair

Author repair of original E54. This packet does not accept E54, the SP02 parent, or the full goal. It does not claim a customer IdP.

Product commit `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. The reviewed producer was `35bd6b8157cecd89f2ae085b012b4b0137562ddb`. The schema and OpenAPI bytes remain the `089b00d7f70da1bc42f7e201dfa070748c651d5b` publication. `CONTRACT.md` is the file that changed.

## Corrected sentence

Documented in `CONTRACT-SENTENCE.md` before the product edit, while `CONTRACT.md` was still `35f9c4a1fccf08b695848c6e6291f191a392cf8ecd079490f80855a773035732`.

`iss` exact, `aud` contains the registered `client_id` for every permitted configuration, a distinct configured audience does not replace `client_id`, multiple audiences require `azp` equal to `client_id`, a present `azp` equals `client_id`, and a single audience equal to `client_id` is accepted when `azp` is absent

`schema/config.schema.json` still requires `audience` (1..=128) and the `config_commit` preimage still includes it. OpenAPI and the response schemas are byte-identical to the 089b publication. The rejection code remains `audience.rejected`.

## Smoke before edits

Linux arm64, before any product edit. `smoke.exit` is 0. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

`rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Builder limits for that container: 6 GiB, 1 CPU, `cap-drop ALL`, `no-new-privileges`, private network, no host port. Log: `linux/raw/smoke.log`.

## Focused proof

One later builder, same image and limits, and one PostgreSQL 17.10. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 1 GiB, 1 CPU, no host port (`pg/ports.json` is `{}`). `init-roles.txt` is `ok`. The source mount was read-only. The cargo home was a private volume. `pg-wrap.sh` matches origin `a587031c719a484830d78e493605f48e2056cae4` at `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f`.

Locked offline tests, `--test-threads=1`:

| Step | Exit |
|---|---|
| `rustup-clippy` | 0 |
| `cargo-fetch` | 0 |
| `cargo-test-focused` | 0 |
| `cargo-clippy` | 0 |
| `run-proof` | 0 |

Library 11 passed, `usage_is_silent` passed, `live_identity_boundaries` passed. Clippy `--all-targets -D warnings` reported no warnings. Root `Cargo.toml`, root `Cargo.lock`, and `tools/control-identity/Cargo.lock` were not modified.

The live test loaded a config whose `audience` was `oo-control` and whose `client_id` was `oo-control-client`, and asserted those strings differ. The private HTTPS issuer signed RS256 ID tokens. Accepted callbacks were followed by `GET /control/v1/identity/current` returning 200 for subject `fixture-admin`.

| Probe | `aud` | `azp` | HTTP | Result |
|---|---|---|---|---|
| `aud_client_id` | `oo-control-client` | `oo-control-client` | 302 | accepted, current 200 |
| `aud_client_id_no_azp` | `oo-control-client` | absent | 302 | accepted, current 200 |
| `aud_multi_matching_azp` | `oo-control-client` and `extra` | `oo-control-client` | 302 | accepted, current 200 |
| `aud_arbitrary` | `other-aud` | `oo-control-client` | 401 | `audience.rejected` |
| `aud_configured_only` | `oo-control` | `oo-control-client` | 401 | `audience.rejected` |
| `aud_multi_without_azp` | `oo-control-client` and `extra` | absent | 401 | `audience.rejected` |

`linux/raw/author-proof.json` records 52 cases and `customer_integration=not_claimed`. The same live test still ran the existing login, role, PKCE, state, nonce, issuer, signature, expiry, JWKS, restart, and bootstrap paths, because those paths mint `aud` and were affected by the membership change.

## Reused 68-case evidence

Not rerun. Reviewed packet `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a375/SP02/control-identity-review/`.

- `linux/raw/independent-proof.json` `f917a652b0fcec042699852ea2a3d43d109ae3c5e40a3952f184eebb8ae82438`
- `linux/raw/independent-proof.verdict` `d880d5aac37b0bdf5ec323addf7c640da1b6abaf8982e245df918f512ba00c3b`

That run recorded 68 cases and 0 failed on `35bd6b8157cecd89f2ae085b012b4b0137562ddb`. Its withhold was the split configuration this commit changes. When `audience` equals `client_id`, membership in `aud` is the same predicate as before. No new database campaign was run. The table-owner `TRUNCATE` note in that packet stays a retained note.

## Cleanup

Owned `oo-a379-proof`, `oo-a379-pg`, volumes `oo-a379-cargo`, `oo-a379-work`, and `oo-a379-eef831aa-pgdata`, network `oo-a379-eef831aa-net`, and `/tmp/oo-a379-eef831aa-dba4-4af2-a78b-541bc9e0a379` are removed. `oo-a373-pg` was still running afterward. See `linux/raw/owned-cleanup.txt`.

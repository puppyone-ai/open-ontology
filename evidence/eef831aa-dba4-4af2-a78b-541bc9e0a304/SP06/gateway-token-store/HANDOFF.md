# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a304`. Unit: opaque Gateway token store for frozen GW-D01 / SP06-W01. Fixed commit `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`.

## Owned paths

- `tools/gateway-token-store/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a304/SP06/gateway-token-store/**`

Do not edit root `Cargo.toml`, root `Cargo.lock`, or another card's tree. a207 is the connector manifest and root lock writer.

## What a later reader can rerun

The proof launcher is `evidence/.../gateway-token-store/linux/proof/launch.sh`. It archives the fixed `Cargo.lock` and `rust-toolchain.toml`, overlays this crate, checks the lock subset, then `cargo test --locked --offline` and `cargo clippy --locked --offline --all-targets -- -D warnings`. `inside.sh` installs the Clippy component for toolchain 1.94.0 when the pinned image does not have it. That matches the a200 smoke recipe.

Private resources must be new for a new run. The names used here (`oo-a304-pg`, `oo-a304-proof`, `oo-a304-eef831aa-net`, the three volumes, `/tmp/oo-a304-eef831aa-dba4-4af2-a78b-541bc9e0a304`) were removed after this proof. `cleanup.txt` records that. A rerun has to create its own network, because smoke-2 created the network this proof reused and cleanup deleted it. `pg/up.sh` exits 2 when the network is missing.

Builder limits that held: 6 GiB, 1 CPU, `CARGO_BUILD_JOBS=1`, one rust-builder, one Postgres, global cap 10, no host cargo. Images, pulled never:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

The evidence listing launcher is `linux/inventory/launch.sh`. Its origin is a199 `linux/manifest/launch.sh`, the run whose exit file is 0. a199 `linux/manifest-4` exited 126 because the binary was placed on a noexec tmpfs. This copy keeps the binary on the image root filesystem (`/usr/local/bin`). Changes from that origin: 6 GiB, 1 CPU, global cap 10, this UUID's paths, and a crate-manifest compare before the evidence walk.

## Behavior to keep

Contract `gateway-token-store-v1` in `tools/gateway-token-store/CONTRACT.md`. Plaintext once to the issuer. SHA-256 stored. Session `oost_`, client `ooct_`, 32 CSPRNG bytes. Admin issue/revoke versus runtime lookup. Positive cache 5 seconds, monotonic, not extended on failure. Revocation scopes are exact and idempotent. A later admin issue after a principal or project revoke is a new grant whose floor includes that sequence. A revoked session id stays closed.

`issue_for_test` and `lookup_for_test` take an injected clock and do not certify production. Production `issue`, `revoke`, and `lookup` use `SystemTime`.

## Left undone on purpose

Public HTTP authentication, IdP, OAuth, PKCE, refresh, RFC 8693, mTLS handshake, Redis or other pub/sub, audit WAL, agent SQL, session-lease certification, query cancel. SP06-W01 is not closed. The GW-D01 parent is not closed. No release.

If a future change needs a crate that is not already in the fixed root lock, that change belongs to a207, not to a lock edit in this tree.

# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a314`. Independent review of the opaque Gateway token store frozen at `99ac19af42059a308ba9c418815619e5a081f407`.

## Owned path

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a314/SP06/gateway-token-store-review/**`

The product tree `tools/gateway-token-store` was read and archived. It was not edited. Root `Cargo.toml` and root `Cargo.lock` were not edited. a313 is the root lock writer.

## What a later reader can rerun

`linux/preoverlay/launch.sh` archives `99ac19af` (`Cargo.lock`, `rust-toolchain.toml`, `tools/gateway-token-store`), checks the 125-registry subset against that lock, then runs `cargo test --locked --offline` and `cargo clippy --locked --offline --all-targets -- -D warnings`. The author live tests require `OO_A304_LIVE=1`, host `oo-a304-pg`, and database `oo_a304`. `pg/up.sh` supplies that alias and SAN beside `oo-a314-pg`, and `pg/init-roles.sh` creates both databases.

`linux/overlay-2/launch.sh` repeats the archive, copies `overlay/a314_review.rs` into the archive only, inserts the cfg(test) module line, and runs `cargo test --locked --offline --lib a314_review`. It sets `OO_A314_LIVE=1`, host `oo-a314-pg`, and database `oo_a314`. It does not set `OO_A304_LIVE`.

`linux/overlay/` is the failed harness attempt. Leave its console and `harness.rs` in place.

Private resources from this run are gone. `cleanup.txt` records that. A rerun needs new names, a new network, new volumes, and a new tmp directory. `pg/up.sh` exits when its network is missing. Builder limits that held: 6 GiB, 1 CPU, `CARGO_BUILD_JOBS=1`, one rust-builder at a time, one Postgres, global cap 10, no host cargo. Images, pulled never:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

`linux/inventory/launch.sh` compiles `tools/sha256_manifest.rs` to `/usr/local/bin/sha256_manifest` with `--network none`. It compares the live crate and `source/fixed-crate` with `linux/proof/crate-manifest.sha256`, then writes `MANIFEST.sha256`. The tool skips that output path. Files written after the container exits stay outside the listing. a199 `linux/manifest-4` exited 126 because its binary sat on a noexec tmpfs. This launcher keeps the binary on the image root filesystem.

## Behavior this review observed

Contract `gateway-token-store-v1`. Plaintext returned once. SHA-256 stored. Session `oost_`, client `ooct_`, 32 OS CSPRNG bytes, no quality claim. Admin issue and revoke. Runtime lookup with `SELECT` plus `UPDATE` of `last_used_ms` and `idle_deadline_ms`. Positive cache 5 seconds of `Instant`. A hit and a refusal leave the expiry instant where it is. A committed revoke is visible to a new cache. After the cache window, a stopped database is `unavailable`. After restart, the revoked token stays revoked. Session idle 30 minutes and absolute 8 hours. Client default 1 hour and maximum 90 days. Production issue and lookup use `SystemTime`. `FixedWall` is the test seam.

`DeclaredTransport` is compared as a frozen binding. Issue flags for an auth provider, an IdP, mTLS, and pub/sub stayed false in the focused tests.

## Left outside this review

Public HTTP authentication, an IdP, OAuth, PKCE, refresh, RFC 8693, an mTLS handshake, a session lease, and a release. SP06-W01 stays open. The GW-D01 parent stays open.

A crate that is absent from the fixed root lock belongs to a313. This tree does not edit that lock.

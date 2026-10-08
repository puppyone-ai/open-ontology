# Findings

## Failed pre-edit smoke

The first builder invocation used `bash -lc`. That login shell cleared `PATH`, so `rustc` and `cargo` were not found even though the container exited 0. OpenSSL 3.5.5 was present. This is not a pass. The retry used `bash -c` with `PATH=/usr/local/cargo/bin` and `RUSTUP_TOOLCHAIN=1.94.0`, without mounting the repo root, and reported `rustc 1.94.0` on `aarch64-unknown-linux-gnu`. Raw logs: `linux/raw/smoke-attempt-1-login-shell.log` and `linux/raw/smoke-attempt-3.log`.

## Lock subset

`cargo generate-lockfile` on the direct pins selected 26 patch versions that are not in the root lock, including `hyper 1.12.0`, `hyper-util 0.1.21`, and `cc 1.6.0`. Those crates were then pinned to the root versions with default features off. A direct pin of `quinn-proto` with default features also pulled `fastbloom 0.17.0`, which the root lock does not contain. Turning default features off removed it, because the workspace's reqwest edge does not enable that feature. The regenerated lock matches the root lock on every registry name, version, and checksum. The root lock was not edited.

## PKCS#8

OpenSSL 3.5 `genpkey -outform DER` writes a PKCS#1 RSAPrivateKey. `ring` and the public-key parser require PKCS#8. The tests now run `openssl pkcs8 -topk8 -nocrypt`. The first unit run failed in `pkcs8_rsa_public` before this wrap.

## Nonce status

`nonce.rejected` is HTTP 400, with `state.rejected`, `pkce.rejected`, and `redirect.rejected`. Issuer, audience, expired token, and algorithm failures are 401. The first complete live run reached the negative matrix and stopped because the test expected 401 for a bad nonce. The test now expects 400. The handler was already returning the frozen code.

## Clippy component

The pinned Rust image has rustup proxies for Clippy and does not contain the 1.94 component. `rustup component add clippy --toolchain 1.94.0-aarch64-unknown-linux-gnu` installed that component inside the builder. No crates.io version changed.

## Capability drop and copied secrets

`docker cp` into the builder left files owned by host uid 501. With `cap-drop ALL`, root could not `chmod` those files. The password and CA were rewritten by a process inside the container, mode 0600. The password was not passed on argv.

## What the live run checked

Positive login through the query bootstrap token, current, CSRF and Origin refusal without revoke, logout, restart survival of both the revoked and the later live session, the five management roles, project and revision guards, caller JSON rejected, foreign redirect, PKCE, state, algorithm, issuer, audience, azp, multi-audience, nonce, expiry, future `iat`, IdP error, ambiguous kid with one JWKS fetch, unknown kid with two fetches, key rotation with two fetches, idle expiry, OAuth expiry, subject mismatch without burn, superseded token, concurrent consume, replay, bootstrap unavailable after init, and revision conflict. Raw case list: `linux/raw/proof.json`.

## Out of scope

No customer credential, host auth store, frontend file, root manifest, or enterprise database was changed. No agent, harness, or Gateway token was issued.

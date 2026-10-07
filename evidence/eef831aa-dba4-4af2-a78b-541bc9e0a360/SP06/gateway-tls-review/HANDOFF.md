# Handoff

Verdict: APPROVE_LIMITED for the public TLS identity transport at `97b6a9eb9caa2b6059d82bdb98b703bf5ee202d8`.

E45 stays open. This packet does not accept the parent gateway goal, OAuth, an IdP, token issuance, the session proxy, public SQL, or full SP06 auth.

No product edit is required for this card. `crates/oo-gateway`, `Cargo.toml`, and `Cargo.lock` were read from the archive and compared again after the review tests. The hashes match.

## Proof to keep

`linux/smoke` is the preserved pass of `cargo check --locked`, author Clippy, loopback (8), TLS default (5 passed, 1 ignored), the pack binaries, and the author two-role live test. Its `docker-run.exit` is 101 because the first review oracle failed Clippy. Do not rerun those green suites to interpret this packet.

`linux/smoke-2` is the continuation. It reused the cargo and target volumes, installed Clippy because the fresh container did not have it, and ran the corrected review oracle. Default review tests: 3 passed, 1 ignored. Ignored live test: 1 passed in 11.14s. `inside-ok.txt` is `2026-10-07T22:16:53Z`.

Raw exits, host config, and `review-summary.txt` are under those directories. Source hashes are in `source/INVENTORY.txt`. Recipe origins are in `recipes/ORIGIN.md`. Owned cleanup is in `CLEANUP.txt`.

## Scope that stays refused

`POST /internal/v1/session-tokens` is 403. An `oost_` session token is 401. `POST /v1/sql` is 501, and the HTTP module still does not name the executor. `/ready` reports `database=not_probed`. The workload URI is read from the verified leaf. It does not select the token role.

`http_gateway_live` under `OO_A326_LIVE=1 --ignored` was not part of this campaign. A default `cargo test` leaves it ignored. That is not a new result from this review.

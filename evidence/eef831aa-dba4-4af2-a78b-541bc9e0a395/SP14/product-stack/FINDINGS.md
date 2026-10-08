# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a395`. These findings are the author record. They do not accept the parent or the goal.

## F1. The live journey is incomplete

`run/journey.json` ready is false. Reason `journey_incomplete`. Missing `oidc_current`, `catalog`, `pack`, `release`, `session_close`, `audit_ack`. `run/health.json` has the same missing list and an empty `tcp_only` list. Exits are 2 and 2.

The oracle accepts a step only when HTTP is 200 and the body matches `deploy/product-stack/src/decide.rs`. Catalog acceptance requires `catalog.rows` 100000. That body was not captured. SQL row counts in `run/rows-*.txt` are persistence snapshots. They are not a source catalog.

## F2. Standard and minimal fail closed on this host

runsc is absent. `run/preflight-standard.json` and `run/preflight-minimal.json` set allow false, deployment refused, reason `runtime_unavailable`, `runc_substituted` false. Gate exits are 2. `run/phase.log` records `standard_and_minimal_refused`. The compose sandbox-gate service requests runtime runsc and was not started.

Dev preflight allowed localprocess with `production_signed` false. That profile is the one that reached `up`.

Observability was refused with `images_unpinned`. No Grafana image was pulled.

## F3. Fixed services that did answer

Identity pin `a6f92da8a0bf872f8a30efb116c2c7b14463505b` installed and listened. `GET` without a cookie returned HTTP 401 and the contract unauthenticated body in `run/probes/oidc_current.json`. The oracle requires HTTP 200 and operation `getIdentityCurrent`. The 401 is a real response and is not ready.

Audit pin `1078d69e8c3cf8dacb145198f1e4123c767a196b` installed `oo_audit_ingest`. One event and one receipt survived repeat and restart. The probe recorded HTTP 200 and body `{"error":"non_json"}` because the captured transcript was not a JSON ack. The rows were not rewritten into `oo.audit.ingest/v1`.

Session pin `53017ed29c98c49fb44a7eb18d5df6d5de273046` (evidence short pin `53017ed29`) installed and issued a grant. Evidence keeps `grant_sha256` `886651451acd34f4b5c4781a6ed9b2e3d15af1af3f2f3d051e39f695b77609b1`. The server log shows listening twice after restart, deployment localprocess, `production_signed` false. `oo_session_api.conversation` stayed 0. `run/session-http.log` is one blank line. `run/probes/session_close.json` is HTTP 0 and `non_json`. A listening line is rejected by `session_ok`. No S1–S11 close receipt is in evidence.

## F4. Producers that were not connected

`source-control-api` and `release-control-api` are recipe files only. Their probes are HTTP 0 and `recipe_only`. They were not compiled.

`selfbuild-control-api` was compiled from `git archive HEAD` of the committed tree, because the worktree was dirty. The image exists. The compose profile stayed off. The pack probe is HTTP 0 and `producer_binaries_absent`. This card did not use `oo-a373-pg` and did not run a selfbuild 100k campaign.

The SBOM field for selfbuild is the literal `HEAD`. The builder log does not store the archive commit. Local reflog shows HEAD at `b05a892deceb93268404eb54348845f65ada9ec0` from `2026-10-08T07:52:46Z` until `2026-10-08T08:00:34Z`, which covers `linux/build-ok` inside `2026-10-08T07:54:33Z`–`2026-10-08T07:59:56Z`. That reflog line is not an image digest and is not a pin written by the builder.

## F5. Repeat and restart did not move the owned data

Secret SHA-256 `b8f74515d9f8b63ccd46d748d3991f614037372ebd21fb4e22a945f38066119e` is identical before, after repeat, and after restart. The three row files are byte-identical. The journey still fails because the six exchanges are missing. Equal snapshots do not clear `journey_incomplete`.

## F6. Owned resources were removed; local images remain

`run/cleanup.txt` reports containers 0, volumes 0, networks 0, state absent, foreign unchanged. The foreign name list is unchanged across the phase. `leftover-observation.txt` records a later check: still no a395 container, volume, network, or `/tmp` state.

Four local tags from phase 1 remain. `run/images.txt` says absent because the second `images.sh` pass did not see the service binaries in `STATE/bin`. The tag IDs in `linux/failed-attempts/phase-1/images.txt` match a later `docker image inspect`. The images were not pushed. Cleanup does not prune images.

## F7. Earlier failures that the second phase fixed in the harness

Phase 1 identity and session install exited 1: `control-db` was undefined unless compose also selected profile `dev`. Audit install exited 2: the official image creates role `oo_fixture_admin` and does not create role `postgres`. The second phase install exits are 0. Those fixes are in `deploy/product-stack/ops/up.sh` and `ops/lib.sh`. They do not supply the missing HTTP bodies.

Build 1 failed clippy because the component was absent. Build 2 failed clippy on `needless_range_loop`. `linux/build-ok/clippy.exit` and `linux/build/clippy.exit` are 0 after `words.iter_mut().enumerate().take(16)` in `src/sha256.rs`. Tests stayed green on every builder, including the failed ones (`test.exit` 0).

## F8. Out of scope and untouched

G1 files under `apps/web/deploy/**` were not edited. The identity commit G1 already uses remains `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. Root `Cargo.toml` and `Cargo.lock` were not changed. No cargo update and no generate-lockfile. E51 ordinary 100k was not reloaded. E54 and E55 were not re-run. Frontend trees were not edited.

`deploy/product-stack/src/lib.rs` and `src/main.rs` both carry `#![forbid(unsafe_code)]`.

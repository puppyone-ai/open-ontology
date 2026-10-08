# SP11-W06 / SP14-W02 product stack report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a395`. Owner label `oo.owner=a395`.

The card is not complete. The live journey oracle returned ready false. Parent SP11-W06 / SP14-W02 is not accepted. The goal is not accepted. This report records the loop that actually ran.

## Decision

`run/journey.json`:

```json
{"ready":false,"production_signed":false,"parent_accepted":false,"goal_accepted":false,"deployment":"localprocess","reasons":["journey_incomplete"],"missing":["oidc_current","catalog","pack","release","session_close","audit_ack"]}
```

`run/health.json`:

```json
{"ready":false,"production_signed":false,"parent_accepted":false,"goal_accepted":false,"tcp_only":[],"missing":["oidc_current","catalog","pack","release","session_close","audit_ack"]}
```

`run/journey.exit` is 2. `run/health.exit` is 2. `run/phase.exit` is 0 because the phase wrapper finished recording. A zero phase exit is not a ready stack.

No 100k catalog body was captured. No pack publish body was captured. No release activation body was captured. No session close receipt was captured. The audit probe stored a non-JSON body, so the ack is not accepted. OIDC current is a real HTTP 401.

## What the second phase ran

Host orchestration only. Product crates, the root workspace, and `apps/web/deploy/**` were not edited. Profile `dev` is localprocess. `production_signed` stayed false.

Order recorded in `run/phase.log`: standard and minimal refused, up, session exchange, health capture, snapshot before, repeat, snapshot after repeat, stop, restart, snapshot after restart, cleanup, phase finished.

| Step | Exit file | Exit |
|---|---|---|
| init | `run/init.exit` | 0 |
| preflight dev | `run/preflight-dev.exit` | 0 |
| preflight standard | `run/preflight-standard.exit` | 2 |
| preflight minimal | `run/preflight-minimal.exit` | 2 |
| preflight observability | `run/preflight-observability.exit` | 2 |
| standard gate | `run/standard-gate.exit` | 2 |
| minimal gate | `run/minimal-gate.exit` | 2 |
| observability gate | `run/observability-gate.exit` | 2 |
| image record | `run/images.exit` | 0 |
| PostgreSQL ready | `run/pg-ready.exit` | 0 |
| identity install | `run/identity-install.exit` | 0 |
| audit install | `run/audit-install.exit` | 0 |
| session install | `run/session-install.exit` | 0 |
| up | `run/up.exit` | 0 |
| oidc probe | `run/oidc-probe.exit` | 0 |
| audit probe | `run/audit-probe.exit` | 0 |
| session grant | `run/session-grant.exit` | 0 |
| session exchange | `run/session-exchange.exit` | 0 |
| repeat | `run/repeat.exit` | 0 |
| stop | `run/stop.exit` | 0 |
| restart | `run/restart.exit` | 0 |
| sbom | `run/sbom.exit` | 0 |
| cleanup | `run/cleanup.exit` | 0 |
| health oracle | `run/health.exit` | 2 |
| journey oracle | `run/journey.exit` | 2 |
| phase wrapper | `run/phase.exit` | 0 |

Enabled profiles in `run/profiles.txt`: `dev`, `identity`, `audit`, `session`. Selfbuild, source-fixed, and release-fixed stayed off.

## Preflight

`run/preflight-dev.json`: allow true, deployment `localprocess`, runsc `absent`, reasons `[]`, `runc_substituted` false, `production_signed` false, `parent_accepted` false, `goal_accepted` false.

`run/preflight-standard.json`: allow false, deployment `refused`, reasons `["runtime_unavailable"]`, `runc_substituted` false.

`run/preflight-minimal.json`: allow false, deployment `refused`, reasons `["runtime_unavailable"]`, replica `not_claimed`.

`run/preflight-observability.json`: allow false, reasons `["images_unpinned"]`. No observability image was pulled.

`run/phase.log` begins `standard_and_minimal_refused`. The sandbox-gate service was not started. runc was not substituted. Dual-arch AT-OPS-01 was not run and is not claimed. `other_arch` in the preflight documents is `x86_64_integration_node`.

## Persistence and cleanup

Secret tree SHA-256, identical in `run/secret-before.sha`, `run/secret-after_repeat.sha`, and `run/secret-after_restart.sha`:

`b8f74515d9f8b63ccd46d748d3991f614037372ebd21fb4e22a945f38066119e`

Row snapshots `run/rows-before.txt`, `run/rows-after_repeat.txt`, and `run/rows-after_restart.txt` share SHA-256 `2624b3d804e9871a202b805d9e8ee12e057b72d4ccdb76a0ee9f23f2f34825fc`. Counts:

- `oo_audit_ingest.event=1`, `oo_audit_ingest.receipt=1`
- `oo_identity_control.install_manifest=1`; audit_event, bootstrap_token, oauth_transaction, session are 0
- `oo_session_api.entry_grant=1`, `http_session=1`, `install_manifest=1`; `conversation=0` and the other api tables are 0
- `oo_session_control.install_manifest=1`, `session_head=1`
- `oo_token_control.install_manifest=1`; revocation and token_record are 0

`run/secret-mode.bad` is 0. `run/secret-files.json` lists 33 paths, each mode `0600`. The files themselves were under `/tmp/oo-a395-eef831aa-dba4-4af2-a78b-541bc9e0a395` and were removed by cleanup. Evidence stores paths and modes, not secret bytes, private keys, cookies, or grant material.

`run/cleanup.txt`:

```text
containers=0
volumes=0
networks=0
state=absent
foreign_unchanged
```

`run/foreign-before.txt` and `run/foreign-after.txt` are the same nine names. `run/owned-after.txt` is empty. A later observation in `leftover-observation.txt` still shows zero a395 containers, volumes, and networks, and no state directory. Local image tags were not removed and were not pushed.

PostgreSQL 17.10 on aarch64 accepted connections after restart (`run/oo-a395-control-db.log`). The database role used for local admin SQL is `oo_fixture_admin`. There is no `postgres` role.

## Exchanges

| Name | HTTP | Recorded body | Oracle |
|---|---|---|---|
| oidc_current | 401 | unauthenticated `oo.control-identity/v1` body in `run/probes/oidc_current.json` | missing; 401 is not getIdentityCurrent |
| catalog | 0 | `not_started`, `source-control-api`, `recipe_only` | missing; 100k rows were not read |
| pack | 0 | `not_started`, `selfbuild-control-api`, `producer_binaries_absent` | missing |
| release | 0 | `not_started`, `release-control-api`, `recipe_only` | missing |
| session_close | 0 | `{"error":"non_json"}` | missing; `run/session-http.log` is empty |
| audit_ack | 200 | `{"error":"non_json"}` | missing; the row counts are not an ack document |

Identity install committed `sql_sha256` `432ecb6d0ca917a2f77d37d042fb7f10ab36f5d66bbb2e8c1d617e9f594172ef`. Init recorded `config_commit` `810d2c8dda370c1ea6bd7ff91d30b1ea582411302c6277c37b4541f04f9e2419` and `init_ok`. The identity log shows `outcome=listening` on `0.0.0.0:8443`. The issuer in the composed config is not a logged-in IdP. No current-identity success body was stored.

Audit install applied the schema (`CREATE SCHEMA`, two tables, grants) and exited 0. The database held one event row and one receipt row across all three snapshots. The probe status line was parsed as HTTP 200. The body saved for the oracle is `{"error":"non_json"}`. That body was not rewritten into an ack.

Session install exited 0 with `api_store`, `session_store`, and `token_store` installed, `production_signed` false, deployment `localprocess`. `run/session-grant.log` records `grant_sha256` `886651451acd34f4b5c4781a6ed9b2e3d15af1af3f2f3d051e39f695b77609b1` and does not contain the grant. The server log shows two listening lines, ports 36103 and 40775, both `production_signed` false. Conversation count stayed 0. No close receipt was stored.

## Builds

Smoke, before the stack edits, `linux/smoke/`: `docker-run.exit` 0, `rustc.exit` 0, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, image `arch=arm64`. Network mode none. Memory 6442450944. NanoCpus 1000000000. CapDrop ALL. Privileged false. Container removed.

Service build `linux/build-ok/`, inside `2026-10-08T07:54:33Z` to `2026-10-08T07:59:56Z`: `docker-run.exit` 0, `test.exit` 0, `clippy.exit` 0, `oracle-build.exit` 0, and fetch/build exits 0 for control-identity, audit-ingest, session-control-api, and selfbuild-control-api. Test lines: 2 passed, 0 passed, 12 passed. Hostconfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a395-eef831aa-net`, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000. `capacity.txt` at start: own 0, global 1, host_cargo 0. Container removed.

Oracle-only rebuild `linux/build/`, inside `2026-10-08T08:17:28Z` to `2026-10-08T08:17:34Z`: same test and clippy exits 0. `build-selection.txt` sets all four service builds to 0. It replaced the oracle binary after phase 1 had deleted `STATE/bin`. It did not refetch the four services. Container removed.

`linux/build-ok/build-selection.txt`: identity, audit, session, and selfbuild build flags 1; source and release `recipe_only`.

## Images and SBOM

Phase 1 `linux/failed-attempts/phase-1/images.txt` records these local tags as built:

- `oo-a395-identity:local` `sha256:adef401ef8fc5d7d03c55c7a02e6c2b53311f1fa86cf645092c285c156a2680d`
- `oo-a395-audit:local` `sha256:29a1b858d4d83dc4ee1aebd72bda081e3a35eb54402eee04771f138b9826b2c7`
- `oo-a395-session:local` `sha256:0da76da3332ac39090140eeab9c5460de6f8cb4dceced451b2bf6122138f6eeb`
- `oo-a395-selfbuild:local` `sha256:1776de64ef45d3c0b48d3ac86dbbd38b5162ad838a29d18a04db9214128efbb2`

The second phase `run/images.txt` says `absent` for those four binaries. `ops/images.sh` writes absent when `STATE/bin/<bin>` is not executable, and the oracle-only rebuild had left only `oo-product-stack` there. The script does not delete an existing tag. Compose still started identity, audit, and session from the tags phase 1 built. Selfbuild stayed off the compose profile. Nothing was pushed.

`run/sbom.json` is SPDX-2.3 input text named `oo-product-stack-inputs`. `parent_accepted` is false. The note says checksums are copied from supplied Cargo.lock text and recipe_only units are not images. Seven units: control-identity, audit-ingest, session-control-api, selfbuild-control-api, and oo-product-stack locked; source-control-api and release-control-api unlocked. This is not a runtime image scan.

## Failed attempts kept

`linux/failed-attempts/build-1/`: `docker-run.exit` 1, `test.exit` 0, `oracle-build.exit` 0, `clippy.exit` 1. Clippy stderr: cargo-clippy is not installed for toolchain 1.94.0. Container removed.

`linux/failed-attempts/build-2/`: `clippy-component.exit` 0, `test.exit` 0, `oracle-build.exit` 0, `clippy.exit` 101, `docker-run.exit` 1. One deny: `needless_range_loop` in `src/sha256.rs`. Container removed. The later builds have `clippy.exit` 0.

`linux/failed-attempts/phase-1/`: identity-install 1, session-install 1, audit-install 2, session-grant 3, oidc-probe 2, pg-ready 0, up 0, journey 1, health 1, phase 0. `journey.json` and `health.json` are empty. Identity and session install logs: compose profile did not include `dev`, so `control-db` was an undefined service. Audit and row snapshots failed with `role "postgres" does not exist`. Session grant failed because `/state/grant.hex` was absent. The second phase exits above are the rerun after those script fixes. The 100k journey was not rerun because it had never passed.

## Bounds

One rust-builder at a time, 6 GiB, 1 CPU, label `oo.kind=rust-builder` only on that compile container. Oracle and probe containers use other labels. One private PostgreSQL, 1 GiB, 1 CPU, name `oo-a395-control-db`, image digest pinned in `deploy/product-stack/compose/compose.yml`. No host network, no published ports, no privilege, no Docker socket, no foreign prune. Cleanup removed only `oo.owner=a395` resources.

Linux arm64 Rust 1.94 smoke was not repeated in this write-up pass. The service builder and the oracle-only rebuild were not started again. No second PostgreSQL was started. Foreign containers listed in `foreign-before.txt` were not removed.

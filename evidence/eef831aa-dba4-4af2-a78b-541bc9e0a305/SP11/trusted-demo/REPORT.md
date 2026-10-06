# OPS-D01 trusted-operator PostgreSQL demo

Session `eef831aa-dba4-4af2-a78b-541bc9e0a305`. Fixed product archive `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`. Owned paths are `deploy/trusted-demo/**` and this evidence directory. This report records the development demonstration. It does not accept the parent SP11-W06 unit, M1, I01, I02, a release, authentication, authorization, semantic publication, natural language, a model provider, or a harness. The same session wrote the demo and this report. That is not an independent approval.

## Result

`./deploy/trusted-demo/start.sh --public-fixture` exited 0. Host record: `start-public.exit` is `0`, `start-public.state` is `DONE`. Stderr was empty. Stdout was:

```
postgres_ready name=oo-a305-pg alias=oo-a305-pg
fixture_preserved
restart_postgres
postgres_ready name=oo-a305-pg alias=oo-a305-pg
fixture_preserved
```

`linux/public-attempt-3/phase.exit` is 0. `linux/public-attempt-3/docker-run.exit` is 0. `linux/public-attempt-3/drive.log` is `ready_for_trusted_query`. `linux/smoke/phase.exit` is 0. `linux/smoke/docker-run.exit` is 0. `linux/smoke/drive-smoke.log` is `smoke_repeated`. `live/smoke/smoke-ok.txt` is `role_pack_unchanged` and `query_rows_repeated`.

Receipt `live/receipt.json`, protocol `oo.deploy.trusted-demo-receipt/v1`, status `ready_for_trusted_query`. Fixed commit matches the archive. Non-claims in that file:

- `human_approval = not_claimed`
- `authentication = not_claimed`
- `authorization = not_claimed`
- `semantic_publication = missing`
- `natural_language = unsupported`
- `model_provider = unsupported`
- `harness = not_performed`
- `m1_i01 = not_accepted`
- `m1_i02 = not_accepted`
- `parent_acceptance = false`

Inspect `live/run/outcome.json` stays `phase=inspect`, `status=awaiting_review`, `source_cut=48`, `target_fence=1`, `mapping_run_generation=48`, `database_state=child_compilers_applied_no_release`. Apply `live/run/apply-outcome.json` is `status=ready_for_trusted_query`, `analyze_published=false`, `estimate_is_exact=false`, `database_rollback=false`, `database_release=false`. Hidden column `shop.customer.private_note` has reason `classification.absent`. Provision effects recorded by the tool are north `installed` and narrow `replay`. Config identity `3be8e75ed25b3884a441bed73a0db0d773c77dd07b60bd9de47230b6beb14035`. Schema hash `da877f8bc78b7a9a14b8648004bfd904d8bc89b774bbca1253c1758927a74a2f`.

Role-pack SHA-256, matching the receipt and a host `shasum -a 256` of the step stdout:

- north `74e5069b398b8a50118ead3630fefba8b0bc66adcc9c76e33bc8a3237bcbfa38`
- narrow `4530c2c34cd31e3a4f44710416a23274af426771e56e8c5b71af4bf3bcc0c8ac`

The smoke builder rehashed those files and repeated the north customer query. The smoke stdout file is byte-identical to `live/run/steps/query-north-customer.stdout`.

## Chain

Source id and project are `public_shop`. The first public probe on attempt 2 sent `public-shop`. The fixed `SourceId::new` accepts `^[a-z][a-z0-9_]{0,62}$` and printed `oo-pipeline: source config source_id`. Attempt 3 used `public_shop`. Its not-ready probe stderr is `oo-pipeline: target schema is not ready` with exit 1, then init and import ran.

North customer rows from `query-pg`, exit 0:

- customer 41, display `丙号`, active true
- customer 42, display null, active false
- customer 43, display `丁铺`, active true

`柜员甲` occurs in `query-north-orders.stdout` and does not occur in `query-narrow-orders.stdout`. `private_note` does not occur in the north customer, narrow customer, or narrow order stdout. Comment marker `甲注评` occurs 0 times in `catalog.json` and `captured-draft.json`.

Query exits:

| Step | Exit | Stderr |
| --- | --- | --- |
| query-north-customer | 0 | |
| query-north-orders | 0 | |
| query-north-count | 0 | |
| query-narrow-customer | 0 | |
| query-narrow-orders | 0 | |
| query-north-before | 5 | `query-pg: policy` |
| query-north-hidden | 6 | `query-pg: guard` |
| query-north-insert | 6 | `query-pg: guard` |
| query-north-pos | 6 | `query-pg: guard` |

Bad review used a copy of the inspect tree and an extra decision `shop.customer.not_a_column`. `live/bad-run/apply-outcome.json` is `status=failed`, `step=reviewed-effective-oos`, `detail=exit`. Child stderr is `reviewed-effective-oos: oo.builder.reviewed-effective-oos/v1/decision.unknown the decision names a column that is not on the effective draft`. Step stems in that directory stop at `reviewed-effective-oos`. There is no `install-column-acl`, `provision-mapping-reader`, `analyze-pg`, `role-pack`, or `query-` file there. The good run directory stayed the one that reached `ready_for_trusted_query`.

Operator text is `live/work/operator-stage.txt`. It names `public-fixture` as a fixture label. The executed query argv in the receipt is `/cargo-home/a305-bins/query-pg --request /evidence/live/run/queries/north-customer.request.json`. Repeating that request as-is is not the restart command, because the audit file already exists. Restart is `./deploy/trusted-demo/start.sh --smoke`.

Secret scans, written by the Safe Rust driver and re-read by that driver: `secret-scan.txt` is `secret_absent files=415`. `smoke-secret-scan.txt` is `secret_absent files=446`. Neither line contains a password. No `.pass`, `.key`, or `.pem` file is in this evidence tree.

## Builders and smoke

Linux arm64 Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Toolchain text `rustc 1.94.0 (4a4ef493e 2026-03-02)`. The new builder was 6 GiB (`Memory=6442450944`), 1 CPU (`NanoCpus=1000000000`), `Privileged=false`, `CapDrop=["ALL"]`, `OOMKilled=false`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, container `oo-a305-pg`, id `da567a83607612931de313d0dc68ce6f0b3382e041f2f08f098e531a6bed003a`, 1 GiB, 1 CPU, not privileged. `NetworkSettings.Ports` is `{"5432/tcp":null}` (no host publish). `cargo-update-invoked.txt` is `no`. Onboarding test count on attempt 3 is `passed=24 failed=0 ignored=1`, including `test live_chain ... ignored`, `cli_failed_child_keeps_marker_and_static_stderr ... ok`, and `duplicate_decisions_refuse_before_review ... ok`. Clippy `--all-targets -- -D warnings` exit 0. Driver tests and driver Clippy exit 0 before the workspace binaries. All 33 phase exit files under `linux/public-attempt-3/phases/` are 0. All 7 under `linux/smoke/phases/` are 0.

| Run | docker-run.exit | Record |
| --- | --- | --- |
| `linux/public` | 1 | Drive test build failed: unused imports and `Bound` missing `Debug`. Container `c48137609d9b75e1aa7a0cfe550f038ed51a5ebca415b06dd93d4f364fd118e8`. Onboarding tests had already passed. |
| `linux/public-attempt-1` | 1 | Drive tests passed. Clippy failed on a range loop and a `vec![0_u8; 128]`. Container `449f5afcfd5f91ec0203b975829502fa19863d081bbe12a6e828d4e4bc9aa6e3`. |
| `linux/public-attempt-2` | 1 | Binaries built. Inspect stopped at `import-not-ready` detail `shape`. Stderr `oo-pipeline: source config source_id`. Preserved at `linux/public-attempt-2/failed-run/`. Container `91ee505608b5912f03d2b45b2472e83dbb3f068220ff55cbd91af493346fe4ab`. |
| `linux/public-attempt-3` | 0 | `public_shop`. Chain reached `ready_for_trusted_query`. Container `25e84c3dff690e74feb339171c246450aec398266dc51b6f38828ce65792b06a`, removed after the run. |
| `linux/smoke` | 0 | Same volume, role-pack hashes unchanged, north query repeated. Container `d4638a21f96f348df0b65c444da35c67136d75978e73307303e1bc4cab86094a`, removed after the run. |

No second PostgreSQL. No `docker pull`, prune, or foreign cleanup. Host `cargo`/`rustc` were not executed. Capacity before attempt 3 was `own=0 global=0 host_cargo=0`.

## Inventory

Compiled binaries and their SHA-256 are `linux/public-attempt-3/bin-sha256.txt` (init-pg, oo-pipeline, analyze-pg, oo-builder, reviewed-effective-oos, role-pack-bundle, reviewed-policy, install-column-acl, provision-mapping-reader, query-pg, pg-onboarding, drive). In-container walks are `linux/public-attempt-3/manifest.sha256` (422 lines) and `linux/smoke/manifest.sha256` (453 lines). Files the host wrote after those walks are listed in `HANDOFF.md`. `SHA256SUMS` in this directory is a later host inventory of the evidence tree and is not itself a member of that list. `source/tools/pg-onboarding/` is `git archive` of the fixed commit. `source/ARCHIVE-COMMIT` is that commit.

Recipe origins and the hashes copied from a274 / a204 / a200 / a199 are `deploy/trusted-demo/ORIGIN.md`.

# Provisioning a new MySQL source (v1)

Contract: `specs/chunked-baseline/ONBOARDING-API.md`. Evidence: `specs/chunked-baseline/PROVISIONING.md`.

Files:
- `provision.py` (parent dir): CLI, journal, phases, review/decide gates.
- `provision/site-v1.json`: pinned binaries (path + SHA-256) and shared infrastructure names. No secrets.
- `provision/site-v2.json`: format profile for `--format chunked-typed/v2`. It replaces only the five pins it names
  (`source_onboarding`, `producer`, `oo_pipeline`, `job_worker`, `oo_source_admission`); every other pin is shared with
  v1, and the v1 pins are unchanged. Currently the T13b build (not yet sealing, see Status). T13c's binaries replace them.
- `provision/fixture_mysql57.py`: owned MySQL 5.7.44 fixture for tests (`oo-ob-fixture-mysql57`). Not a customer database.

## Commands

```sh
P=tools/source-onboarding/provision.py
python3 $P start  --input SIX.json --password-file PW --out RUN [--egress direct|eip] [--format v1|chunked-typed/v2] [--read-replica REPLICA.json] [--label TEXT]
python3 $P status --out RUN          # status JSON; exit 0
python3 $P review --out RUN          # review JSON (S9 items, binds_fact per item)
python3 $P decide --out RUN --decisions DECISIONS.json
python3 $P resume --out RUN
python3 $P cancel --out RUN
```

`--ca-file CA.pem` (required, verifies the source TLS certificate) and optional `--discover-endpoint HOST:PORT`
(host-side schema discovery address when the host cannot resolve the six-field host; refused with `--egress eip`).

`--format chunked-typed/v2` sets `onboarding.c1_baseline_format` in the capture operator (the v1 operator is unchanged)
and pins the binaries of `provision/site-v2.json`. It also sets `onboarding.child_deadlines_ms: {"native-step": 21600000}`
(6 h), because a 5 GB chunked capture runs 30–60 min; the default per-command deadline stays 600000 ms. An unknown format is `format_invalid`; a format without a profile is
`format_unavailable`.

`--egress eip`: the egress phase opens `ssh -L 127.0.0.1:<auto>:<six host>:<six port> oo@192.0.2.10`. The producer
(in the trusted container) dials `host.docker.internal:<auto>` and verifies TLS against the six-field host, which goes into
the operator as `tls_server_name` (`site-v1.json` `egress.producer_tls_server_name: true`). Host-side discovery and the
artifacts profile read use `127.0.0.1:<auto>`. The forward's local port is kept in `private/eip-local-port.json`. After a
failure the runner tears the forward down, and a later phase re-opens it on that same port, because the producer's
driver config pins the port for a re-run.

`SIX.json` is exactly `{"type":"mysql","host":…,"port":…,"database":…,"user":…,"secret_ref":…}`, 0600.
`PW` is a 0600 file holding only the password. Passing `--password` is refused.

`--profile-input PROFILE.json --profile-password-file PPW` (required for publish): `{"user","secret_ref"}` of a
SELECT-only profile reader. The tool writes it into the static-ready operator as `onboarding.product.profile_access`
(exactly those two keys). The secret is registered in the run's secret registry and resolved by the pinned binary
under `host_secret_root` (0600). The six-field binding stays the CDC account: capture and follow compare it
unchanged, and `profile_access` is outside that drift check. Network source SQL (profile and typed oracle) uses the
profile account because the producer's read-only gate refuses replication grants.

`DECISIONS.json`: `{"reviews":[{"name":"db.table.col","binds_fact":"<64 hex>","decision":"confirmed"}],"confirm_columns":["db.table.col", …]}`.

Rejections are refused: `decide` returns `rejection_requires_rebuild` for any `"rejected"` decision, and nothing is
recorded. Contract note: in this API version a rejected visible column leaves the build without a classification for
that column, and S10 (`confirm`) fails with `classification.incomplete`. Hiding a column that already has a decision
fails later with `membership.refused`. So feedback on a column is a rebuild (the UI offers that), not a rejection.
Unreviewable (PII) columns are withheld by policy and are never submitted.
It must cover every pending item and `confirm_columns` must equal the confirmed names.

## Two-pass PII build

After `s0_s8`, the `pii_rebuild` phase checks the build for visible PII. A column with `pii != "none"` that is not yet hidden is written to `private/hidden-declaration.json` (automated `control_plane_test` declaration derived from model classification, `business_owner_signed=false`; not a business signature). A new sealed bundle (`bundle-static-v1-pii<N>`) is then made without the hidden columns from `roles.json`, the visibility and the native model. `catalog.json` and `profile.json` are copied byte-for-byte, and the API project is pointed at the new bundle with `hidden_columns` set (API restart only when changed). The previous product directory is archived as `product-pass-<N>.<k>`, and a new build starts (new S5 call). Sealed bundles are write-once: a rewrite of an existing sealed input is refused (`sealed_inputs_rewrite`). At most 3 builds; a column that is still visible after the 3rd fails with `pii_unstable`.

## Typed-oracle round and publish

The follow computes the typed oracle (`typed-oracle.json`, `quality.full_typed_cell_differences`) only after S0, so the
first publish can be refused with `follow_failed:actual_api_refused` (the seed carries no oracle value). That publish
failure runs `oracle_rebuild`: the value is read from the follow's own output (it must agree in both files), must be 0
(`typed_oracle_mismatch` otherwise), and a new sealed seed carries it with the same hidden set. A new S0-S8 build
(real S5, or the API's cache for identical inputs) starts; its review decisions are archived per round
(`decisions-before-oracle-<n>.json`) and review, pack and publish run again. Bounded: a seed already at the current
content version with the same value is refused (`publish_refused_after_oracle`); at most two oracle builds.

The token admin file is the token store's shared credential (one role, `oo_token_admin`, the same value in every gateway
generation); the run copies it from `site.product.token_admin_source` and never generates one.

## Run directory

```
RUN/
  run.json            run id, label, format, egress, names (container/db/role prefixes); no secrets
  six.json            validated six fields (0600)
  journal.jsonl       append-only phase events (no secrets)
  status.json         written on demand by `status` (not persisted)
  runner.pid runner.lock runner.log
  evidence/           NNNN-<label>.{started.json,stdout,stderr,exit.json} for every external command
  phases/<phase>.json per succeeded phase: started/finished/wall_seconds/data
  private/            0700; secret-registry.json (secret_ref -> per-run password path), ssh.sock while egress is up
  review/             decisions.json (canonical, hash-stable), phase evidence
  product/            product_flow.py output (prepared-overlay.json, build.json, product-receipt.json, …)
<secret_root>/provision/<run_id>/password.pass   0600 copy; <secret_root> = ~/.cache/oo-mysql-connection-whole
```

`<secret_root>` is the trusted container's only secret mount, so per-run passwords must live there.
This is a deliberate deviation from "under RUN/private" (documented in the report).

## Phases

`validate, egress, identity, target, capture, artifacts, product_runtime, s0_s8, pii_rebuild, awaiting_review, review, pack, publish, replica_check, gateway, token, smoke, ready`.

Live, `--egress eip` (owned fixture, reverse tunnel ECS 127.0.0.1 → fixture, removed afterwards):
- v1: `validate`, `egress`, `identity`, `target`, `capture`, `artifacts`, `product_runtime` succeeded (run `v1-eip-2`). The
  capture receipt has `applied_pos=65`, `load_state=awaiting_publish`. Artifacts discovered 2000/6000/3000/500 rows.
  The run was cancelled in `s0_s8` (no LLM step kept).
- `chunked-typed/v2` (T13b binaries): `capture` fails. The producer's `prepare`, `native-put`, `native-fire` and the
  composed digest pass for all four tables. The final load then refuses with `OO030 full import binding does not match
  the run` (B1, T13c). Nothing is sealed. A `resume` re-opens the forward on the same port and reaches the worker again.

Status of the live path (owned fixture `oo-ob-fixture-mysql57`, direct egress; run `live-4`, clean):
- `validate`, `egress` (direct; eip verified up to the producer gate), `identity`, `target`, `capture`, `artifacts`:
  implemented and run live.
- `product_runtime`: API container and serve, plus release control (new `<prefix>_release_control_v1` DB with grants,
  `oo-ob-<short>-release` container, identity forward, official `release-control-api install`). Live: `installed`.
- `s0_s8` and `pii_rebuild`: real S5 per build (one LLM call each). Visible PII of a build is declared hidden and a new
  build runs on a sealed bundle without those columns (see "Two-pass PII build"). Sealed inputs are never rewritten after a build.
- `awaiting_review`, `review`: `decide` refuses `policy_not_applied` / `withheld_drift` if the stored withheld set is not
  what the current overlay derives. Live: 21 reviews accepted (fixture decisions, not a business owner's).
- `pack`: implemented (verifies the product receipt). Live: succeeded.
- `publish`: static-ready operator plus pinned `source-onboarding-linux follow` in the runtime container. Failed attempts
  are archived as `publish-attempt-N` and can be retried. Live: `follow_failed:owned_fixture_only` (BLOCKER, below).
- `replica_check`: standby `pg_is_in_recovery`, read-only session, reader role SET and TLS. Live (diagnostic run
  directly against live-4): `t|on|<reader>|t|t`.
- `gateway`, `token`, `smoke`, `ready`: implemented and unit-tested. Not exercised live: they depend on `follow`
  completing (gateway, token issuance and the gateway-ready record are produced by follow).

Pinned binaries: `source-onboarding` = `362c2f0c…` (P12, `profile_access`; capture and follow), producer = `mysql-preview`
`ff1e7f80…` (P11, `source-sql`, `c1-onboarding-prepare`). Previous: `db854a1f…` (P11, commit f37d5c192 / 5ae26e2f5).
Every operator carries `onboarding.product.source_access = "network"`, so profile SQL goes through `source-sql`.

Seed semantics: the sealed `source_semantics.json` does not claim a typed-oracle state. The oracle runs inside follow,
which records `quality.full_typed_cell_differences`. A seed that said `typed_oracle: not_run` contradicted that result
and made the maintenance step refuse the pack (`pack.refused`).

Earlier blocker (resolved by P11 network source-SQL, not by widening the allowlist; the legacy path is kept unchanged): the pinned `source-onboarding-linux` (sha `c7a216a6…`) contains
`source_sql`'s guard `container starts_with "oo-dl01w1-06-" or == "oo-mysql57-reference-20261016"` (error
`owned_fixture_only`). `follow` runs profile SQL through that guard before any Docker call, so the owned fixture
`oo-ob-fixture-mysql57` is refused. Options: (a) owner decision to extend the guard to owned `oo-ob-` fixtures in source,
rebuild and re-pin the binary; (b) the owner names an existing approved owned-fixture container. Renaming the fixture to
match the guard was not done: it would impersonate shared infrastructure.

Known limitations for customer sources (product decisions, not yet taken):
- The producer only accepts sources marked `fixture=owned-mysql-57` and user `oo_cdc`, and refuses hosts containing
  `rds` or `amazonaws` (`refuse_customer`). Customer sources need a producer change and an explicit decision.
- The producer needs binlog retention >= 7 days (`expire_logs_days`). For customer sources this is a DBA prerequisite.
- Money classification: the sealed money projection requires the source owner to declare each money column. The tool
  declares every MySQL `DECIMAL` column as money with unit `unspecified` (no currency claim), and records that
  declaration in `rules.json`. For customer sources this must be a real owner decision.
- The typed oracle (`mysql57-audit-native-target.py`) is hard-wired to the old scope and is not ported; the bundle
  records `typed_oracle: not_run`.

Runtime notes that the code depends on:
- Shared infrastructure is resolved by network alias with `docker network inspect` plus `docker inspect` (never by container name).
- Anything passed into the trusted container must sit under `/tmp/oo-mysql-connection-whole` (identical path) or the secret root
  (mounted at `/secrets`). `start` refuses other `--out` locations (`out_not_shared`).
- The worker's runtime password is `loader.hex`; the cluster's trust rules for 127.0.0.1 make psql checks meaningless.
- The producer only accepts sources marked `fixture=owned-mysql-57` and user `oo_cdc` (customer guardrail, `refuse_customer`).
- The producer requires binlog retention >= 7 days (`expire_logs_days`). This is a DBA prerequisite.

## Error codes (stable)

`six_fields_invalid, inline_password_refused, private_file, password_file_invalid, format_unavailable, format_invalid,
egress_invalid, label_invalid, replica_invalid, run_exists, run_not_found, binary_missing:<name>, binary_pin:<name>,
ssh_forward_failed, forward_source_unreachable, producer_tls_server_name_unsupported, control_path_too_long,
identity_pg_unavailable, phase_not_implemented:<phase>, not_awaiting_review, decisions_shape, review_shape,
review_duplicate, review_unknown_item, binds_fact_invalid, binds_fact_mismatch, decision_invalid, decisions_incomplete,
confirm_columns_mismatch, no_confirmed_columns, decisions_drift, review_not_ready, product_config_missing,
policy_not_applied, withheld_drift, withheld_item_submitted, pii_unknown, pack_incomplete,
follow_failed:<code>, follow_timeout, gateway_ready_missing, gateway_not_ready, standby_network, replica_check_failed,
gateway_missing, gateway_not_running, gateway_mounts, gateway_network, gateway_not_listening, token_hash_mismatch,
token_ttl, smoke_freshness, smoke_tables, smoke_count, smoke_failed:<label>, withheld_column_queryable,
runner_active, runner_exited, run_canceled, command_failed:<label>`.
`follow_failed:<code>` carries the code from follow's stderr (for example `owned_fixture_only`), never the raw text.

## Tests

```sh
cd tools/source-onboarding && python3 -m unittest test_provision -v
```

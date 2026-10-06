# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a249`. Fixed commit `e0704257a1509b968a80a9ccd2f200319d5483ad`. Product commit `9df074813572569950d85bbe951069b4db83facc`.

`parent_acceptance` is false. This is not a completed parent goal and not a release. M1-D14F stays open.

## Left in the tree

`provision-mapping-reader` reads one operator document from stdin and calls `oo_gateway::mapping_reader_access::provision_mapping_reader`. The policy objects use the reviewed-policy field names. The explicit `login` must equal the compiled policy login. Success writes `protocol`, `effect`, `login`, and `role` with no trailing newline. `effect` is `installed` or `replay`.

The library at the fixed commit was already public. This change does not edit `lib.rs`, `mapping_reader_access.rs`, `policy.rs`, SQL, manifests, or the lock.

The contract is `contracts/gateway/provision-mapping-reader-cli-v1.md`, protocol `oo.gateway.provision-mapping-reader-cli/v1`. It states that the grant is administrator-selected provisioning, that authentication and tenancy are not decided here, and that mapping rows are visible database-wide.

## Proof

Smoke of the fixed archive, before overlay, exit 0 on Rust 1.94.0. Development chain exit 0 on Rust 1.94.0 and PostgreSQL 17.10. Oracle: `dev-live/oracle.txt`. Outcome: `dev-live/outcome.json`. The independent review run wrote the same oracle again; that copy is `review/live-record/`. Verdict: `review/REPORT.md`.

## Still open

Authentication of the administrator and of the login. Audit WAL. Row-level security. Publication. A non-superuser grantor; the fixture used `oo_fixture_admin`. A login with this grant can read every mapping row in the database. That exposure is named, not closed.

The live test covered the essential path: two roles, catalog SQLSTATE 42501 before the command, installed, replay, partial privilege, stale generation, and an unknown login. It does not repeat the wider library refusal matrix from a240. `query-pg` was not edited and was not invoked. The test called `load_policy_catalog` and `DedicatedSession::execute_trusted_select` with `UnknownRows::Zero`.

## Cleanup

`cleanup/run.sh` removes only `oo-a249-smoke`, `oo-a249-chain`, `oo-a249-review`, `oo-a249-pg`, the a249 cargo, target, and pgdata volumes, network `oo-a249-eef831aa-net`, and `/tmp/oo-a249-eef831aa-dba4-4af2-a78b-541bc9e0a249`. Label checks use `index .Config.Labels "oo.owner"` and `index .Labels "oo.owner"`. It does not prune and does not touch another owner. `cleanup/CLEANUP.txt` records that run. Pinned images stayed.

# Session store durable adapter

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a275`. This directory records the PostgreSQL adapter in `tools/session-store` for the fixed session-transition reducer. The decisive Linux proof is `linux/impl-7`. Earlier attempts are preserved. This file does not accept a parent ledger row, does not close SP11, and does not certify a physical release.

## What was built

`tools/session-store` is its own Cargo workspace. It path-depends on the fixed `tools/session-transition` tree and does not edit that tree, the root manifests, or the root lock. Schema name is `oo_session_control`. The migration is `migrations/oo_session_control_v1.sql`. Install stores the SHA-256 of that file and replays only when the hash and the catalog shape match. A different hash or a partial schema is drift. The installer does not repair it.

Rows are keyed by project and session. The head stores state JSON, owner generation, revision, the current bound tuple, and `journal_len`. The journal cap remains 1024. A full journal still returns `dedup_exhausted` from the reducer. This adapter does not raise that cap.

Each transition starts a transaction, locks the row, reads the stored snapshot, calls `session_transition::transition`, and updates the row only when that call is an accepted new revision. Refusals and idempotent hits commit the read without an update. The caller supplies `now_ms`. There is no global clock and no authenticator. The login must be `oo_session_admin` for install and bootstrap, or `oo_session_runtime` for transition, get, and reconcile. A client principal that equals a role name is refused. Public authentication is not implemented.

A COMMIT whose transport result has no SQLSTATE is `commit_unknown`. The adapter does not report that as `rolled_back` and does not cancel the backend. Reconcile reads the row on a fresh connection and reports the event durable only when that event id is in the stored journal.

## Decisive run

`linux/impl-7` container name `oo-a275-impl-7`. `docker-run.exit`, `test.exit`, `clippy.exit`, `lock.exit`, `fetch.exit`, `archive.exit`, and `version.exit` are 0.

HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a275-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`.

Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

Fixed archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`, tree `7e83a381c8a90c5f61f3ce73947eb7ac441381e9`. Reducer directory tree `a25733870f1ffac55c057dd399270905ccca9151`, commit `b0f7da4771e3c3625a45329c721e174f4380571c`. Repo HEAD recorded at the impl-7 launch is `585fef5cf2542b3a29a11b407a4f2f346acb95ce`.

Library tests: 11 passed. Live tests on the private database: 12 passed in 2.34s. Clippy `--locked --offline --all-targets -- -D warnings` finished with no warnings. `Cargo.lock` in the crate matches `linux/impl-7/Cargo.lock` (`ad43022b2fd98a158781df1f7dbb928b857683e54871a4210f3360bfff525f9a`). Every registry checksum in that lock is from the fixed archive lock. Direct pins are the root pins.

Live tests that passed:

- `install_replay_drift_partial_and_privileges`
- `wrong_ca_does_not_connect`
- `two_connections_race_one_winner`
- `rollback_before_commit_leaves_the_row`
- `new_connection_replays_and_conflict_refuses`
- `commit_unknown_reconciles_without_a_second_transition`
- `owner_lease_takeover_keeps_the_workspace`
- `changed_principal_new_session_and_project_separation`
- `project_key_and_principal_are_not_authentication`
- `column_json_mismatch_is_not_repaired`
- `cli_acks_refusals_and_hides_the_password`
- `product_does_not_read_the_fixture_directory`

## PostgreSQL

One private server, container `oo-a275-pg`, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/up.exit` is 0. No host port (`pg/ports.json` is `{}`). Network `oo-a275-eef831aa-net`. Privileged false. Caps added are CHOWN, DAC_OVERRIDE, FOWNER, SETGID, and SETUID. Server certificate SAN is `DNS:oo-a275-pg`. Secrets stayed under `/tmp/oo-a275-eef831aa-dba4-4af2-a78b-541bc9e0a275` and were not copied into this evidence directory. The builder mounted only the fixture directory.

## Precode

`linux/precode` ran before the adapter sources were compiled. `docker-run.exit` is 0. The same Rust image, 6 GiB, 1 CPU, CapDrop ALL, private bridge. The fixed reducer tests passed: 1 SHA-256, 2 CLI, 12 reducer. The archive did not contain `session-store`. That directory was not overwritten.

## Reducer review already on disk

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a272` verdict is APPROVE_LIMITED for the pure reducer and CLI at `b0f7da4771e3c3625a45329c721e174f4380571c`. Its findings do not report a reducer defect. This adapter calls that reducer. This session did not edit it. a272 does not review this adapter.

## TLS

`rustls_connector` follows the same rules as `oo_gateway::policy::rustls_connector`: aws-lc-rs, an empty root store plus the caller PEM, `PemObject::from_pem_slice`, hostname verification, and no client certificate. `SslMode::Require` is the only mode. This crate does not path-depend on `oo-gateway`, `oo-pipeline`, or `oo-connectors`. a207 remains the only writer of the connector manifest and the root lock.

The lost-COMMIT probe uses that same `ClientConfig` and hides the server reply after the COMMIT bytes are flushed. It is not selectable from the CLI.

## Limits

The journal holds 1024 events. Passing that cap is a documented prerequisite, not a migration this crate performs.

The identity tuple is caller-supplied. `auth_provider_used` stays false inside the reducer. The adapter sets its own receipt `database_used` when it used PostgreSQL. It does not treat the principal as a login.

Session-end S1–S11 timings, a control IdP, harness launch, and a physical release are outside this crate. This run is not a full SP11 claim and not an independent review of the adapter.

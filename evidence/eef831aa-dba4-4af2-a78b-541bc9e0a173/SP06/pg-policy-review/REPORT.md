# a173 review — column ACL

Verdict: **APPROVE LIMITED**.

Task `a173`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a173`. Repository `/Users/redux/puppy-one/open-ontology`. Reviewed source `d3429726618a4e69ada7fb35131f9043a21e23e9`. Author evidence `22d99427b444ca34c6a0cff910eebd2665ac77e7`. Review source commit `997eb638f546cd7c73fa77a56e07591678f17780`.

The approval covers the public primitive in `oo_gateway::policy`: `compile_column_acl`, `install_column_acl`, `connect_require_tls`, and `rustls_connector`. Contract `contracts/gateway/pg-policy-v1.md` (`oo.gateway.pg-policy/v1`). The fresh proof is `linux/probe-4/live/oracle.txt` on a clinic fixture that is not the author's shipment fixture. Product files and dependency files were not edited.

## What was read

Author `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md` under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a171/SP06/pg-policy-compiler/`, plus `supply/dependency-delta.md`. The fixed `policy.rs`, the author live test, `contracts/gateway/pg-policy-v1.md`, `docs/07-gateway.md`, `docs/02-technology-stack.md`, `ci/deny.toml`, and `is_gateway_postgres_stack` in `crates/oo-arch-check/src/policy.rs`.

## Pre-overlay smoke

One Linux arm64 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `cargo 1.94.0`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. The container ran before the review overlay existed. `linux/smoke/docker-run.exit` is 0. `linux/smoke/worst.exit` is 0. Id `bdcf9331959cd75c07b01ca4c6a1402c94b1c48027aa2fce63a8fcda6e07d794`. Not OOM, not privileged, cap drop `ALL`, network `oo-a173-net`, no published port, `HOME=/tmp`.

| Phase | Exit |
| --- | --- |
| archive hash compare | 0 |
| overlay absent | 0 |
| `cargo fetch --locked --offline` on an empty private cargo volume | 101, preserved |
| `cargo fetch --locked` | 0 |
| crate byte checksums, 24 of 24 | 0 |
| clippy component | 0 |
| `cargo clippy --locked --offline -p oo-gateway --lib -- -D warnings` | 0 |
| `cargo test --locked --offline -p oo-gateway --lib` | 0, 5 passed |
| architecture tests named below | 0, 3 passed, 40 filtered out |

Architecture tests: `gateway_postgres_stack_is_allowed`, `gateway_sync_postgres_and_sqlx_are_forbidden`, `dev_only_gateway_pipeline_is_forbidden`. The gate source was not edited.

## Database

One PostgreSQL container, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, name `oo-a173-pg`, arm64. `pg/server-version.txt` is `17.10 (Debian 17.10-1.pgdg12+1)`. `pg/up.exit` is 2. `docker exec` was started without `-i`, so the first `pg_hba.conf` write was empty and the server kept the previous file (`pg/log-tail.txt`, `pg/attempt1-hba/`). `pg/repair-hba.sh` rewrote the file and reloaded. `pg/repair.exit` is 0. `pg/ssl-on.txt` is `on`. `pg/pg-hba-after.txt` is `local all all trust` and `hostssl all all all scram-sha-256`. A non-TLS TCP connection as `oo_a173_grantor` exited 2: `no pg_hba.conf entry ... no encryption` (`pg/nossl-rejected.txt`). Server certificate SAN is `DNS:oo-a173-pg` (`pg/server-san.txt`). The network alias `oo-a173-other` is not on that certificate.

The review did not start a second database. Passwords and private keys stayed under `/tmp/oo-a173-eef831aa-dba4-4af2-a78b-541bc9e0a173/secrets` and were removed in cleanup. The client mount used by the probe had the CA certificate and the password files, not the keys.

## Probe

The harness is `overlay/policy_pg_a173_review.rs`, copied onto a work tree of the immutable archive. It calls the public compile and install API. It does not run `policy_pg_live`. Production code and the test harness are `forbid(unsafe_code)`. Container `oo-a173-probe4`, same Rust image and private cargo and target volumes, cap drop `ALL`, read-only `/src`. `linux/probe-4/docker-run.exit` is 0. `linux/probe-4/test.exit` is 0. `linux/probe-4/clippy.exit` is 0. `clinic_visit_column_acl_review` passed in 0.34s. Id `99da6a8db34a337e22c96c857e455122fd40541915d6149cbafc07537f802014`.

| Attempt | Result |
| --- | --- |
| `linux/probe` | Exit 101. Other CA failed the TLS handshake. The harness required the public text to contain `cert` or `UnknownIssuer`. Clippy was absent because the smoke container's component install did not persist. |
| `linux/probe-2` | Clippy 0. `RowFilterRefused` matched. Commit delta was 0 against an expected 1. PostgreSQL 17 does not publish `pg_stat_database` on the next statement. |
| `linux/probe-3` | Clippy 0. Commit delta was 4 against an expected 2. `tokio-postgres` prepares each `&str` statement, so one call is two transactions. |
| `linux/probe-4` | Exit 0. Deltas are compared with a same-session calibration. |

## Measured install

Fixture: database `oo_a173`, superuser grantor `oo_a173_grantor`, login `oo_a173_session` created `LOGIN INHERIT`, owner `a1730000-0000-4000-8000-000000000173`, shard `shard_a173`, source `clinic`, namespace `ward`, table `visit`, columns `visit_id`, `wing`, `mrn`. Generations 4 and 11 for `clinic` and the review digest. Decoy source `pharmacy` at generation 50 with the review digest. Decoy digest at generation 99 for `clinic`. Oracle:

- `generation_scoped=11`, `generation_source_only=99`, `generation_digest_only=50`. Install of generation 11 succeeded, so the lookup used both source and digest. `current_generation` is `policy.rs` lines 590–603.
- Pure no-SQL calibration: `commit=4 rollback=0`. A read plus a rolled-back transaction: `commit=6 rollback=1`. Row filter and unknown PII matched the pure pair. Stale generation 4, missing login `oo_a173_missing`, and superuser login `oo_a173_grantor` matched the rollback pair and returned `StaleGeneration { policy: 4, current: Some(11) }` or `LoginAbsent`.
- `refusal_roles=absent`. After the green run, `pg_roles` names `oo_r_picker` and `oo_r_scribe` only (`linux/probe-4/catalog-map.txt`).
- Picker installed then replayed. Scribe installed. `grantor=oo_a173_grantor`. Membership grantor is `oo_a173_grantor`, inherit false, set true, admin false. Login `rolinherit` is true. Picker and scribe flags are all false: no login, not superuser, not createdb, not createrole, not inherit, not bypassrls, not replication.
- Shrink of picker to `visit_id` and expand onto `mrn` returned `GrantMismatch`. The `attacl` snapshot was unchanged.
- `visit_id` `pc_c2dfff4fcdb51e42cb605ec5ce73ab4cbffad2f79b4f9d0efdd14c33a0c0` is `{oo_r_picker=r/oo_target_owner,oo_r_scribe=r/oo_target_owner}`. `wing` `pc_4fa8dab5fa59b5037a072aa85a784044ba6846ead130cdcf35bbf4059319` is `{oo_r_picker=r/oo_target_owner}`. `mrn` `pc_1aebc0fbc6985eb0e711aaabcc312426ba5304d45295617dd6e39f46e79d` and `_oo_pos` are null.
- `relacl={oo_target_owner=arwdDxtm/oo_target_owner,oo_pipeline_runtime=a/oo_target_owner}`.
- Reader TLS was true on backend pid 237. Picker read `8801` and `north`. Scribe read `8801` and received `42501` on `wing`. After `ROLLBACK`, `RESET ROLE`, and `RESET ALL`, the same pid remained, `current_user` was the login, and `visit_id` was `42501`.
- `SET LOCAL oo.claims` returned SQLSTATE `00000` from the harness. `mrn` stayed `42501`. `transaction_read_only=off`.
- `42501` for picker on `mrn`, `_oo_pos`, column `note_after` added after install, `INSERT`, `UPDATE`, `DELETE`, `_oo.physical_mappings`, `_oo.full_import_run`, and `_oo.lookup_physical_mapping`.
- Schema migrations are the five frozen versions, each `applied_by=oo_target_owner`. `0002` is not among them. `SET LOCAL ROLE oo_target_owner` is in the fixture schema install. The library scan found no `SET ROLE`, no `set_config`, no `oo.claims`, no `NoTls`, and found `SslMode::Require` (line 324) and `with_no_client_auth` (line 302).

`linux/probe-4/catalog-map.txt` is a later local-socket `psql`. That socket is trust, so its `pg_stat_ssl.ssl` is false. The reviewed sessions are the oracle rows `admin_tls=true` and `reader_tls=true`.

## TLS

Wrong CA and the alias outside the SAN both failed before SQL. Public `Display` was `postgres sqlstate= error performing TLS handshake`. `db_error` (`policy.rs` lines 546–557) keeps `Error::to_string()` and drops the rustls source, so the text does not say `UnknownIssuer` or `NotValidForName`. The differential still holds: the other CA fails, the other name fails, and the pinned CA plus `oo-a173-pg` reaches `pg_stat_ssl.ssl = true`. `connect_require_tls` passes the password only to the driver. `with_no_client_auth` does not authenticate a principal. This review does not call that authentication.

## Supply and the architecture list

`supply/primary-review-summary.txt`: added 24, removed 0, problems 0. Lock checksum, the author's tsv, and a fresh crates.io checksum match for every added pair. None are yanked. Newest pair `libredox` 0.1.25 was created `2026-09-21`, before `2026-09-28`. Checksums of `tokio` 1.53.1, `rustls` 0.23.45, `tokio-rustls` 0.26.5, `digest` 0.11.3, `whoami` 2.1.3, `sha2` 0.10.9, `sha2` 0.11.0, and `phf` 0.12.1 are unchanged.

`fallible-iterator` 0.2.0 publishes `MIT/Apache-2.0`. `wasi` 0.14.7+wasi-0.2.4 publishes `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. `wasite` 1.0.2 publishes `Apache-2.0 OR BSL-1.0 OR MIT`. `fallible-iterator`, `libredox`, and `tokio-postgres-rustls` publish no `rust_version`. `cargo deny`, `cargo audit`, and an SBOM were not run. Those release gates stay open.

`is_gateway_postgres_stack` (`crates/oo-arch-check/src/policy.rs` lines 135–140) names `tokio-postgres`, `postgres-types`, `postgres-protocol`, and `deadpool-postgres`. `tokio-postgres-rustls` is a direct gateway dependency and is not in that list. The gate was not weakened.

## Cleanup

`cleanup.txt`. Removed `oo-a173-pg`, volumes `oo-a173-cargo`, `oo-a173-target`, and `oo-a173-pgdata`, network `oo-a173-net`, and `/tmp/oo-a173-eef831aa-dba4-4af2-a78b-541bc9e0a173`. Each removal exited 0. Probe containers were already gone. No other container was removed. `docker prune` was not used. A scan of this directory found no private-key block. The string `ward-token` appears only as the harness constant in `overlay/policy_pg_a173_review.rs`. It is absent from the oracle.

## Open

Authentication of the administrator and of the login, the audit WAL, the gateway parser, portal execution, `CancelToken`, the pool, row-level security, claims binding, publication, and a non-superuser grantor remain open. `deadpool-postgres` is not a dependency of this slice. The tested grantor is superuser `oo_a173_grantor`. Column `attacl` shows grantor `oo_target_owner`, which is the table owner PostgreSQL records when a superuser grants. The library does not `SET ROLE` to produce that string. A passing functional probe is not a release approval.

# a097 review — fixed source role GUC

Decision: **approve** for the role-GUC correction and the main source admission interface that a087 rejected.

A login role with `ALTER ROLE ... SET role` and a caller private CA now connects. `inspect_access` returns `role_mismatch` / `session_user` and `startup_option` / `startup_options`, `registration_allowed` is false, and `catalog` returns `Config` / `session_user_mismatch` before a repeatable-read snapshot. `pg_stat_ssl` has no row for that backend. The report records `negotiated_version=unknown` and `server_observation=unknown`. Mode stays `verify-full`, trust stays `caller-ca-only`, and hostname verification stays on. `version.admission=tested` on that session is the server-version admission for 17.10. It is not a TLS version, not a compatibility test of the missing stats row, and not certificate proof.

This accepts that merged admission slice on the a087 guards and provenance plus this case. It does not sign SP03, PostgreSQL 13 or 14, types, the bounded reader, target import, CLI, supply chain, amd64, customer data, runsc, protected CI, or release. A catalog pass does not accept the vertical.

## Pin

Acceptance input is `git archive` of `34fae5ddd73ff250fde6f51aa681fb0f7fbd0362`. Tree `c35b95664d53dcef1497bbd224ecbeec65586cea`. Parent `35320e9f691cc3c879eb371446101b01839dcb8e`. Archive file count 3720 equals `git ls-tree -r`. `2980e85af22c3ade05272c66c7d2835b52730322` is an ancestor. Worktree HEAD at launch was `82d4c85492fc1b76fa1048bbd970de2909594295`. The moving worktree was not the compiler input, and the fixed commit was not treated as an exit code.

The commit changes 11 files, +191/−18. It does not change `Cargo.toml`, `Cargo.lock`, vendor, SDK, xtask, or CI. Root lock SHA-256 `2b530fd334db74c992cd9238b4f71668ade554d4523c58782ca23c48ef0cd3e7` equals the parent. Connector manifest SHA-256 `b9d8c7bfda4c563994024190ab0ad73c9fffefe013625a760aa47bc1f483216d`. a093's lock `9b8f7e50d3b20042bb2d606f47276b76dd68f3568136343040fbea1ecd622057` is an older overlay baseline, not this tree.

`a095` owns the root. `a092` owns the pipeline. a093 handle 62030 was not treated as the root owner, and the stale a093 note that a088 owns the root was left as written.

## What was run

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, cargo `1.94.0`, `RUSTUP_TOOLCHAIN=1.94.0`, Debian 13, kernel `6.12.76-linuxkit`. PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, server `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num` 170010. One owned builder at a time, one owned fixture, user-defined bridge, no published ports, no host network, no privileged mode, no socket mount, no host `HOME`. Rust containers used `--cap-drop ALL`, `no-new-privileges`, and `--init`. Process listings were pid, ppid, etime, and comm. `cargo fetch --manifest-path Cargo.toml --locked` had no `-p`. Later tests used `--locked --offline`.

| Phase | Exit | Meaning |
| --- | --- | --- |
| clippy component | 0 | |
| fetch | 0 | |
| `--lib` | 0 | 19 passed, including `missing_ssl_row_is_unknown` and `identity_reads_hidden_role_guc_without_changing_it` |
| `--doc` | 0 | |
| package test, no fixture | 101 | `OO_PG_FIXTURE` is `NotPresent`. Required missing-fixture result |
| clippy `-D warnings` | 0 | |
| smoke container | 101 | `oom=false`. `final.exit` follows the package test |
| author `live_pg` | 0 | 1 passed in 4.55s |
| first oracle compile | 101 | harness: `ConnectOptions` was out of scope |
| second oracle runtime | 101 | harness: admitted inspect was counted as a leak, and `super.pass` is the bootstrap password. The role password is `login-super.pass` |
| third oracle | 0 | 2 passed in 14.15s. Lock still `2b530fd3…` |
| `cargo tree -e features -i sqlx-core -p oo-connectors` | 0 | `_tls-rustls-ring-no-roots` present. `webpki` count 0 |

Vendor `sqlx-core` `src/lib.rs` SHA-256 `03f813d1b4ae006049bc3fc213c6302164bd864d505db54e040ff0c4e586dba9` matches the parent and `2980e85`. The compile-error guard still admits only `_tls-rustls-ring-no-roots`. `oo-connectors` has `#![forbid(unsafe_code)]`. No `std::env::set_var` in `src` or `tests`. Connector SQL contains no `SET ROLE`, `RESET ROLE`, `SET SESSION AUTHORIZATION`, or `pg_read_all_stats` grant.

## Role GUC and the cases around it

Production `PgConnector` only. Hostile `PGOPTIONS`, `PGSSLMODE=disable`, forged cert paths, `PGUSER`, `PGPASSWORD=forged-secret`, `PGDATABASE`, `PGAPPNAME`, `PGHOST`, and `PGPORT` were applied with `Command::env` on one child. The parent environment was unchanged. The child still connected the reader as `oo_src_reader`, observed `TLSv1.3`, and was admitted.

`IDENTITY` is one `SELECT`. The allow-list argument is `DateStyle`, `client_encoding`, `TimeZone`, `extra_float_digits`, `application_name`. The source filter names `client`, `session`, `startup`, `user`, `database`, and `database user`. PostgreSQL 17 has no GUC source named `startup`; that token matches nothing and is still present. `role` is read with `current_setting('role')` and is a finding when the value is distinct from `none`.

Observed over verify-full with the caller CA:

| Login | Identity | Extra | TLS report | Registration | Catalog |
| --- | --- | --- | --- | --- | --- |
| `oo_src_reader` | aligned, role `none` | empty | observed `TLSv1.3` | allowed, `read_only` | empty selection hash `cb2b03b8…e5234ecc`, algorithm `pg-catalog-hash/v1` |
| `oo_src_guc_login` | `oo_src_guc_login` / `oo_src_side` | `role` | unknown / unknown, admission `tested` | both blocks | `session_user_mismatch`, statement delta 0 |
| `oo_a097_plain`, `oo_a097_setnone` | aligned, role `none` | empty | observed `TLSv1.3` | allowed, `read_only` | |
| `oo_a097_odd` (`SET role = 'NONE'`) | aligned, setting collapsed to `none` | empty | observed `TLSv1.3` | allowed, `read_only` | no privilege change |
| `oo_a097_allow` | `TimeZone=UTC`, `DateStyle=ISO, MDY` | empty | observed `TLSv1.3` | allowed, `read_only` | allow-list held |
| `oo_a097_self` | users equal, setting `oo_a097_self` | `role` | observed `TLSv1.3` | `startup_option` only | `startup_options`, delta 0 |
| `oo_a097_userguc` | aligned | `work_mem` | observed `TLSv1.3` | `startup_option` | `startup_options`, delta 0 |
| `oo_a097_dbuser` | aligned | `temp_buffers` (`database user`) | observed `TLSv1.3` | `startup_option` | `startup_options`, delta 0 |
| reader and plain on `oo_a097_db` | aligned | `work_mem` (database) | observed `TLSv1.3` | `startup_option` | `startup_options`, delta 0 |
| `oo_a097_case_login` | `oo_a097_case_login` / `Oo_A097_Case` | `role` | unknown / unknown | both blocks | `session_user_mismatch`, delta 0 |

A fresh connection as the GUC login was blocked again with unknown TLS. A fresh reader connection was admitted with `TLSv1.3`. The server reapplies the role GUC. The connector does not clear it.

`CREATE ROLE "none"` fails because the name is reserved. Exact `role=none` is the unset value. A missing `pg_stat_ssl` row with aligned identity was not forced: when `session_user` equals `current_user`, this server returns `ssl=true` and `TLSv1.3` (`oo_a097_self`). The lib test `missing_ssl_row_is_unknown` covers `None, None` and yields `ServerTls::Unknown` without marking the handshake observed. `Some(false)` is `tls_not_negotiated` before the version string is read. `Some(true)` with a missing version, `Some(true)` with `ssl`, and `None` with `Some("TLSv1.3")` are `tls_version_unknown`. Those arms ran in the 19 lib tests.

Wrong CA and wrong host (`oo-source-wrong`, SAN is `DNS:oo-source-pg`) both return `Tls` / `tls_handshake` with connector statement delta 0. A separate non-production `PgSslMode::Disable` connection was accepted and `pg_stat_ssl.ssl` was false. The fixture `pg_hba.conf` is `host` plus scram for remote clients, not `hostssl`. The server can accept plaintext. The production `connect` path hardcodes `PgSslMode::VerifyFull` and has no switch for disable.

Regression inspects, same API: side direct write, inherit path, SET-only path, noset `read_only` and admitted, definer `public_definer`, superuser / bypassrls / createrole / createdb `privileged_role`, operator `boom_op`, cast `boom_cast`. Author `live_pg` passed the same families and the full reader catalog hash `b0dbcffe78f8818af5c72f37c2db048c7058ccfaf8eb1eef4b17b83ac37d9692`.

## Contract and docs

`TlsReport.server_observation` is a new field. Protocol remains `oo.source.pgsql/v1`. Empty schema hash remains SHA-256 of `{"tables":[]}`, `cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc`. No JSON schema in the pin defines `TlsReport`. Builder catalog input is a different `deny_unknown_fields` type and does not carry this report.

New JSON round-trips. A default serde reader of the old four fields accepts the new payload and ignores `server_observation`. A `deny_unknown_fields` reader of those four fields rejects it. The new struct rejects old JSON that lacks the field, because there is no `#[serde(default)]`. That is a stored-document compatibility impact for a strict consumer. No stored AccessReport corpus in this pin uses the old shape. The docs paragraph states the observation rule and does not say that old payloads still deserialize. I am not treating the missing default as an admission defect.

`docs/17-rust-implementation.md` gains one paragraph. Parent and frozen `specs/sources-baseline.json` SHA-256 `ae6eff09f20c1f399c5b0332668f5b0b7ab4f038b725f7377888052f0c649d14`, 18246 bytes, 122 lines, baseline head `e271542aa645aa181da7887d416de0698752a805`. This commit's file is `f16f770e29dc02537a71c734b9355e21812ef65a2fc60880e22543a95bf29534`, 18962 bytes, 124 lines. The paragraph says the connector trusts only the caller CA, keeps `sslmode=verify-full`, records a missing `pg_stat_ssl` row as `unknown`, still rejects `ssl=false` as `tls_not_negotiated`, emits both block reasons, and does not send `SET ROLE`, `RESET ROLE`, or `SET SESSION AUTHORIZATION`. That matches the code. It does not weaken secret handling or TLS admission.

`check_plan.py` against the archive, `PYTHONDONTWRITEBYTECODE=1`, exit 1. The only failures are `Source hash drift` and `Source size/line drift` for `docs/17-rust-implementation.md`. Recommendation: the coordinator versions `specs/sources-baseline.json` to `f16f770e…` when this paragraph is accepted. This review did not edit the baseline, and the check failure is not a product blocker.

## SQL audit

This run's log is `sql-audit/postgresql.log`, 1139003 bytes, SHA-256 `3ce3e13600d662795ee081f9255be7994752caa44a9d78cd6607d5eb5ec20388`. Password-file hits 0. Comment canary hits 0. Two lines contain the word `PASSWORD`; both are the `[redacted]` marker from fixture setup before statement logging was useful, and neither line has a 12-hex token. The raw log is preserved because the secret scan was clean.

`app=oo-pg-source` statement and execute lines: 1680. Verbs: `SELECT` 1416, `WITH` 118, `START` 73, `ROLLBACK` 73. No other verb. Other app names on the log are `oo-a097-observe`, empty, `psql`, `[unknown]`, and `oo-a097-fixture`.

a093's author log was hashed and then deleted. `2652658b3fc92d6920942269ab338f05157128e9c1a81001ee55b89ec718e708` cannot be recomputed from the hash. It is not evidence for this run. a093's RED `tls_not_negotiated` exit 101, the missing-fixture package-test 101, and the missing-`Debug` first-green 101 stay in that evidence. The later author green (fetch reused, lib 19, live 1, clippy 0) is consistent with this run's live exit 0 and is not a substitute for this audit.

## Cleanup

Own names only. No prune. `docker rm` exit 0 for the smoke, follow-up, both oracle containers, and the fixture. Network and the three volumes (`cargo`, `target`, `pglog`) exit 0. `/tmp/oo-eef831aa-dba4-4af2-a78b-541bc9e0a097` removed. A later `docker inspect` of each removed object exited 1. No container name containing `a097` remained.

## Left unsigned

PostgreSQL 13 and 14, bounded read, type conversion, CDC, loader, publishing, gateway, CLI, runtime, customer data, CI, runsc, release, and the supply-chain gate were not run. a087's missing log mount 101, oracle `set_var` compile 101, oracle runtime 101, and cargo-deny history remain in that evidence. This review did not rerun deny and did not widen `ci/deny.toml`.

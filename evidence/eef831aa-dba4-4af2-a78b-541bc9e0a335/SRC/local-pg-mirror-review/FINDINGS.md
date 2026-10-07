# Findings

Public-fixture review only. Enterprise access did not run. Verdict is WITHHELD.

1. The task budget is one source connection. `crates/oo-connectors/src/snapshot.rs` `SnapshotOwner::open` calls `session::try_reserve(..., 2)`, then `connect_reserved` for the data session and again for the guard. The data session runs one `BEGIN` of `REPEATABLE READ READ ONLY`. The guard runs `begin_read_committed`. `tools/local-pg-mirror/src/copy.rs` then writes `source_socket_peak: Some(2)` and `source_socket_fact: "verified_reader_guard"` on every path that has reached `ImportSession::open`. Those fields are constants, not a read of `pg_stat_activity`.

2. The independent sampler, connected as admin to database `postgres`, counted `pg_stat_activity` rows for `datname = 'revsrc'` and `usename = 'oo_rev_src'` every 5ms during a 20000-row copy. The maximum was 2. The only application name recorded was `oo-pg-source`. The manifest for that copy also said peak 2. A third source socket was not observed.

3. Discover and the copy recheck use `PgConnector::prepare_snapshot` on one session and drop it before `SnapshotOwner::open`. The gold manifest recorded `recheck_connections` 1. Catalog drift returned before the owner open and recorded peak 0. That path does use one connection. The row copy does not.

4. The author's `CONTRACT.md` states the two-socket fact and says the child cannot hide the guard. That sentence amends the task budget inside the child freeze. The original card and the plan's hard budget were not amended by a separate frozen decision. This review leaves both texts as they are.

5. Hand gold, types, and mapping held on the fresh fixture. int8 stayed a full decimal integer, numeric(12,2) stayed `10.50` and `0.00`, NULL stayed NULL, and the empty string stayed empty. Logical names remained `public` plus the source table. Physical table names are `pt_` plus 60 hex characters. Oids matched int8 20, numeric 1700, text 25, timestamp 1114, json 114, bytea 17, and bool 16. `jsonb` 3802 was quarantined as `unsupported_column` without cell bytes.

6. After `REVOKE CONNECT` and termination of the source login's backends, the source dial failed and `read_local` still returned the five gold rows from `revmirror`. The target file has host, port, database, reader user, server name, CA SHA-256, and `tls` `verify-full`. It has no password and no source field.

7. Replay of the complete manifest with a missing password path returned `replay_complete` and left source backends at 0. Resume of the incomplete manifest returned `resume_refused` without creating that path. `read_local` of the incomplete manifest returned `not_complete` before reading a password. Cancel, one-batch stop, and a 1ms deadline stayed `incomplete`.

8. `mysql` returned `unsupported_engine engine` with exit 3 before the missing secret path was created. Statement timeout 30001 returned `budget statement_timeout` the same way. Parse rejected batch 1001, lock timeout 1001, and snapshot deadline 300001. The ceilings in the running copy were batch 1000, statement 30000, lock 1000, and snapshot 300000.

9. `cargo generate-lockfile` was not repeated. The committed tool lock matches the author's installed lock, SHA-256 `78117125b0ebcdfbf1363d949428655fb1d7dd7be126567fdc2003a605ad874b`, and the subset check reported 251 checksums inside the fixed root lock and the archived tool locks. Root `Cargo.lock` was unchanged before and after both builds.

10. Installer roles are cluster-wide. The author live test and the independent oracle therefore used separate data volumes. Attempt 1 failed before `CREATE DATABASE`. Attempts 2 and 3 passed the oracle and failed Clippy in the review harness, not in the product sources. Their logs stay under `linux/independent-attempt-*`.

11. The one-connection requirement cannot be met by deleting the guard. A repeatable-read session does not see catalog commits that land after its snapshot. The guard is the read-committed observation. Removing it would weaken that check. The upstream choice is recorded in `REPORT.md`.

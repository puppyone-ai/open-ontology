# Findings

## Preserved attempts

`linux/precode` `docker-run.exit` is 0. Reducer tests on the fixed archive passed. `session-store` was absent.

`linux/impl-1` `docker-run.exit` is 4 and `lock.exit` is 4. `cargo generate-lockfile` selected newer transitive crates than the fixed archive. The first reconcile pinned `js-sys` to the archive version while `web-sys` 0.3.106 still required `js-sys` 0.3.106. The script stopped. No product compile ran.

`linux/impl-2` `lock.exit` is 0 and `test.exit` is 101. The lock reconcile now defers a pin that the current graph cannot take and retries. Compile then failed: the reducer function `transition` collided with the adapter function, the two TLS streams could not share one match arm, and `CertificateDer::from_pem_slice` needs `PemObject` in scope. The same import is used by `oo-gateway`.

`linux/impl-3` `test.exit` is 101. Two source guards matched text that the guards themselves, or a migration comment, contained: `IF NOT EXISTS` and `SystemTime`.

`linux/impl-4` `test.exit` is 101. Library tests passed. Bootstrap returned `Query { sqlstate: None }`. A `String` parameter does not accept PostgreSQL `numeric`, so the driver rejected `$n::numeric` before the server assigned a SQLSTATE. The install test also required `Installed` even when an earlier test in the same process had already created the schema.

`linux/impl-5` `test.exit` is 0 and `clippy.exit` is 101. The 12 live tests passed. Clippy rejected `8_812_750_31` and three nested `if let` blocks. The lock value is unchanged as `881_275_031`.

`linux/impl-6` `test.exit` is 0 and `clippy.exit` is 101. Live tests passed again. Clippy rejected the byte-zeroing loop in the CLI scrub. It is `bytes.fill(0)`.

`linux/impl-7` is the clean run. `docker-run.exit`, `test.exit`, and `clippy.exit` are 0.

The first `pg/up.sh` attempt exited before `docker run` because `chmod 600` on `$SEC/*` removed search permission from the init directory. No container was created and `up.exit` was not written. The script now changes mode only on files and keeps the init directory searchable. The recorded `pg/up.exit` is the successful start.

## Numeric parameters

Revision and owner generation are `numeric(20,0)`. The adapter sends canonical decimal text. The SQL casts that text with `$n::text::numeric`, so the driver parameter type stays text and the server performs the numeric conversion. A mismatch between the numeric columns and the state JSON is `consistency`. The adapter does not rewrite the row to make them match.

## Commit uncertainty

`commit_unknown_reconciles_without_a_second_transition` passed on impl-7. The probe flushes COMMIT and then hides the server reply. The call returns `commit_unknown`. A fresh connection reports the event durable. A different event id on that row is absent. The test does not cancel the backend.

## a272

The independent reducer review at `eef831aa-dba4-4af2-a78b-541bc9e0a272` is APPROVE_LIMITED. Its preserved failure is a lock-parser harness bug, not a reducer defect. The overlay tests and Clippy in that review exited 0. No finding there required this session to stop or to edit `tools/session-transition`.

## Out of scope, still true

Public authentication is not implemented. The runtime role is SELECT and UPDATE on `session_head` only. A claimed principal is not a login. The journal cap is 1024. This record does not certify S1–S11 and does not accept SP11 for a parent ledger.

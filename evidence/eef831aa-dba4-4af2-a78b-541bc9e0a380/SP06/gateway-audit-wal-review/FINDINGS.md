# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a380`. Verdict: **APPROVE_LIMITED of original E55**. The measurement is `raw/boundary-result.txt` on product `1078d69e8c3cf8dacb145198f1e4123c767a196b`. The author live test is not the verdict.

## Proved on this run

- DedicatedSession admission is durable before the first result row. `admit` returns before `client.take()`. The frame is indexed only after `sync_active()`. Inspect between those points shows `admitted` and no completion frame. The SQL text is not in the segment.
- Completion remains after the collector is down (`wal-send` exit 75, segment kept, GC `removed=0`) and is readable by a new `wal-inspect` process.
- Workload identity is the client URI SAN `spiffe://oo/workload/gateway-audit-wal`, mapped to project `shop`. `handle()` reads `certificate_uri` and does not read `x-project`. The sender still sets that header to `not-the-identity`. A stranger URI is HTTP 403. A forced project is exit 75 and adds no event.
- Event and receipt for a spool are written in one transaction: counts 2, 2, and 0 orphans. The same bytes return the same receipt rows. A different payload hash is HTTP 409 and adds no row.
- GC deletes a sealed segment only after a checkpoint whose tuple and payload hash match. A caller `ack.json` does not. A checkpoint with a flipped payload hash deletes nothing.
- Injected fsync failure, a 100-byte quota, and a full 262144-byte private tmpfs each return `AuditUnavailable` with `xact_start` null and backend state `idle`.
- `audit_ingest` has `INSERT` and `SELECT` only. There is no `feedback` table. Guard refusal writes no SQL.

## Bounds of this seam

- The PostgreSQL server process was not restarted. `reader_restart=ok` is a new `wal-inspect`. `restart_collector=ok` is a new collector process and the same receipt rows.
- No crash was injected between the event insert and the receipt insert. Atomicity evidence is the single `commit()` in `insert_event`, the equal counts, the zero orphans, and the 409 that leaves those counts unchanged.
- `UNTRUSTED_ADMISSION` is the constant `Blocked`. Public SQL was not executed.
- The feedback receiver is absent (`feedback` table count 0). This is not SP02-W06 and not full SP06.
- `dedicated_session_audit_is_durable` passed in 0.00s because `OO_A371_LIVE` was unset and the test returned. Do not read that line as a second live proof.
- `rust-toolchain.toml` remains `1.98.1`. A shell that follows the toolchain file is not the 1.94 proof.
- `deploy/trusted-demo/pg/pg-wrap.sh` comments an older hash than the file on disk. The file-on-disk digest is `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`. The recipe was not edited. `up.sh` was hash-pinned and not executed.
- `pg/postgres.log` contains the statement text `SELECT rolpassword FROM pg_authid` from the privilege probe. `secret-scan.exit` is 0. The scan compared evidence with the password files and the non-PEM lines of the three private keys. Those secrets were deleted with the private directory and are not in this packet.
- ENOSPC used `/oo-wal-quota` (262144 bytes), not the host disk.
- Parent acceptance, the acceptance counters, and the Plan/ledger freeze text were left as they stood.

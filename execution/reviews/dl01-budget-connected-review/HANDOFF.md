# Handoff

Consumers of the shared native PostgreSQL admission boundary can develop against source `6590cff4acc1cb265d0ed05d460b0582251cf360` and seal `6a732aaae6a27c6cf074e16df3e1cbb8c83cda0d`. The independent verdict for that boundary is approved. Parent DBX-W02, the rest of the source, the DL, integration, release, and the support matrix remain open.

Use the fresh evidence in this directory, not the author packet's green exits. The author packet stays `author_complete_review_deferred`.

What the boundary does:

- `PgSession::inspect_budget` and `admit-pg-budget` agree on this PostgreSQL 17.10 TLS fixture. Units are Rows, Bytes, and WallClock, in that order, with unknown estimate confidence inside the admin caps.
- A legacy `oo.source.pgsql/v1` report still fails `adapt-pg` with `BUDGET_UNIT_UNKNOWN`.
- A blocked admit returns before `read_typed`.
- Rows and bytes client caps fail the read. The exhaust CLI asserts the fact name `max_rows` only, so a bytes hit is raw exit 2 and `fact=max_bytes`.
- `statement_timeout` fails the read when the DBA holds `ACCESS EXCLUSIVE` and the configured millisecond setting matches.
- R2, R3, and unapproved R1w are denied. Decision `zero_write` stays `not_audited`. Prove reader writes with `audit-log` on csvlog. On the journey log that proof is basis `pg_csvlog`.

Cancel evidence:

- Generation `cancelled=38` from the producer cancel phase means the caller token fired before `next_batch`. It does not prove `pg_cancel_backend`.
- The in-flight proof is `pg_reader_cancel_live` plus csvlog fields. User is field 1. Statement text is field 13. This run's test binary exited 101 because its scanner requires the substring `app=`. The phase notes and the field counts are the evidence: reader cancel 3, reader terminate 0, admin terminate separate.
- The cancel log also contains reader `SAVEPOINT` and `RELEASE SAVEPOINT`. Do not claim that log as a zero-write proof under the current allow-list.

Build notes for the next replay:

- The official `rust:1.94` image used here does not ship clippy. Install the clippy and rustfmt components on toolchain 1.94.0 inside the one builder. Do not start a second compiler.
- Attempt 1 offline exit 101 stays a failed attempt. A later offline exit 0 only means the private cargo cache was warm.
- Author `pg-ready.exit` 1 and the first lock exit 0 are historical flags. Read `pg_isready` and the lock count from this review instead.
- Raw exits, the csv logs, and `linux/binary-identity.txt` are the provenance. The ELF binaries were not committed.

No ledger update belongs in this handoff.

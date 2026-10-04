# Handoff — a148 COPY encoder review

Independent review of fixed source `d80472d8a99afd66d8d0e71420f7be2bb62e65c8`, tree `a3b10827acfac28c4abf986831cbcb26823477dc`, parent `8e0d1da658d65878be96f6f2a7a4478dd3222b19`. Author evidence is `9f95d072c09eed4e332d36156d0b31b6b277401c`. Verdict: **APPROVE LIMITED** for the bounded encoder. Do not mark the ledger. Do not check SP04-W02 from this evidence.

## Read these

1. `REPORT.md` — verdict, identities, commands, exits, and the PostgreSQL oracle.
2. `FINDINGS.md` — line citations for order, NULL versus unavailable, encoding, domain checks, budgets, and refused types.
3. `linux/smoke/` — package test and clippy of the pure archive. All phase exits 0. Ignored live tests stayed ignored.
4. `linux/oracle-attempt-6/` — green public-encoder run. `test.exit` 0, `clippy.exit` 0, `docker-run.exit` 0, `checks.txt`, `fresh.copy`, `domain-violation.copy`.
5. `linux/oracle-attempt-7/` — PostgreSQL 17.10 decode of those bytes. `pg-version.txt` is `17.10 (Debian 17.10-1.pgdg12+1)|170010`. Count `2`. Schema and row diffs exit 0. Rollback `null`. Domain stderr is SQLSTATE `23514`. Later session `pg-after.txt` is `null|null|null`.
6. Failed attempts, left as recorded: `linux/oracle-attempt-1` (compile 101), `linux/oracle-attempt-2` and `linux/oracle-attempt-3` (mount 125), `linux/oracle-attempt-4` (reject count 101), `linux/oracle-attempt-5` (clippy 101). `linux/oracle/console.log` is the same compile failure as attempt 1.

## What the proof showed

The public `CopyEncoder` emitted a header, two row pieces, a trailer, and `Done` for a fixture that is not the author's sample. PostgreSQL decoded system position 0, bytea, text, bool, int2/4/8 extremes, numeric scale 4, date, time, timestamp, timestamptz, float bits including signed zero and a NaN payload, json, uuid, varchar, and unpadded `bpchar` that the server stored at octet length 4. Unselected and masked text stayed out of the COPY bytes. Unavailable was refused. Domain CHECK `(VALUE > 0)` failed in the server after the encoder had written int4 `-1`.

## Left in place

The Loader is still unimplemented. Source types outside the contract table stay refused: jsonb, interval, arrays, enums, money, and negative numeric scale. Domain checks stay on the server. The `psql` session was test administration on a private trust postmaster and wrote no fence, shadow, publication, or receipt. Author live md5 `bfb64118b3d931be583473c4e5d942e9` was not used as this oracle.

## Cleanup

Attempt 7 removed container `oo-a148-pg`, volumes `oo-a148-eef831aa-cargo`, `oo-a148-eef831aa-target`, and `oo-a148-eef831aa-pg`, and network `oo-a148-eef831aa-net`. `linux/oracle-attempt-7/cleanup.txt` lists those names and an empty remaining-a148 section. Snapshots under `/tmp/oo-a148-eef831aa-*` were removed by the attempt traps. Do not prune. Do not remove another owner's containers. Slot A is free.

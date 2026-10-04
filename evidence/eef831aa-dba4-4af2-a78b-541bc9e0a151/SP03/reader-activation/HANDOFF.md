# Handoff — a151 reader activation

Source commit `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`, parent `b72d2f8b115eb9882141eac7f3cfaa05c906f3ed`.

Fixed base for the Linux archive was `4ed22b8c1189e72b06b20a45335860ac9a3ea44e`, tree `fafb0fe6752b8240eb2b29025fc38bccdfb7144d`. That base contains reader `16ca2969cd449653a64d98d9831d3c8b8976942a`. R4 approval is `0791e971f49cd8afc646d80179241ddbcfaa1a1c`. Do not mark the ledger. Do not treat this commit as SP03-W03 closure, full-type certification, CDC, or release.

`scan`, `bounded_read`, and `bounded_reader` are now supported. The reason string is `PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission`. `cdc`, `type_conversion`, `registry_api`, `source_ddl`, and `source_write` stay unsupported. `TypeSupport.converted` stays false.

## Read these

1. `REPORT.md` — requirement map, blob table, containment, and both probe exits.
2. `FINDINGS.md` — the flag table, the four early `not_exhausted` cases, the bound `RunComplete`, and the files left with old wording.
3. `commands.md` — launch lines, exits, and the cleanup that follows the evidence commit.
4. `linux/smoke/console.log` — archive of `4ed22b8c` before the edit. Lib `63 passed`, clippy exit 0, elapsed 40s.
5. `linux/probe/complete-outcome.txt` — fresh oracle. It ends `complete_live_ok`. Generation 1 digest `82e3f995a7ff539ec41d6ad1f14e6210e17268ac1c887c13cf38b43389bf1fe0`. Schema hash `b76a05b20baf5847f2acd640290d33b6765c8d4ce2099e1d0cec0f328b3616db`.
6. `linux/probe/console.log` — lib exit 0, clippy exit 0, `pg_reader_complete_live` 1 passed in 2.71s, `docker-run.exit` 0.
7. `linux/probe-1/console.log` — clippy exit 101, E0716 in `lossless_live.rs`. Leave it. The green probe is the correction.
8. `linux/probe/postgresql.log` — 251888 bytes. Secret-file scan reported zero hits. Do not treat the log as a second oracle.

a149 evidence to reuse, not rerun: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a149/SP03/reader-completion-review/`. Snapshot, stream, session, SQL, the connector manifest, and `Cargo.lock` are the same blobs that review executed.

## API to hold

`SnapshotOwner::finish` returns `SnapshotFinished`. That is lifecycle close only. Generation 3 in the outcome returned it with `admission=0`.

`SnapshotOwner::finish_complete` returns `RunComplete` only when this owner delivered `StreamItem::Exhausted` for every selected table and the existing release checks passed. Generations 4–7 returned `not_exhausted`. Cancel returned `cancelled`. A portal past `max_value` returned `Budget` `max_value`. None of those minted `RunComplete`.

The public capability list is the advertisement. It does not add a second reader implementation.

## Cleanup

After the evidence commit, remove only `oo-a151-probe`, `oo-a151-pg`, the four `oo-a151-eef831aa-*` volumes, network `oo-a151-eef831aa-net`, and `/tmp/oo-a151-eef831aa-dba4-4af2-a78b-541bc9e0a151`. `oo-a151-smoke` and the failed probe container were already removed so the next owned builder could take the name. Their logs remain under `linux/smoke/` and `linux/probe-1/`. Commands are in `commands.md`. Do not prune. Do not remove `oo-a152-*` or any other task's containers.

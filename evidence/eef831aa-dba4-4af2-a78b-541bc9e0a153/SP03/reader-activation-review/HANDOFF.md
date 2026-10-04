# Handoff — a153 reader activation review

Verdict: **APPROVE_LIMITED**. Fixed source `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`, parent `b72d2f8b115eb9882141eac7f3cfaa05c906f3ed`.

This review signs the advertisement of `scan`, `bounded_read`, and `bounded_reader` for the PostgreSQL full-mode snapshot, the bounded pull, and `RunComplete` admission. The reader under that advertisement is the a149 production API. `SnapshotOwner::finish` returns `SnapshotFinished` as lifecycle close only. `SnapshotOwner::finish_complete` returns `RunComplete` only after this owner delivered `StreamItem::Exhausted` for every selected table and the existing release checks passed.

Do not check SP03-W03. Do not edit the ledger. Do not treat the three flags as full-type certification, CDC, incremental mode, registry support, source DDL, or source write. Do not mark a production loader as admitting `RunComplete`. Author evidence `057e30db` is not this approval.

## Read these

1. `REPORT.md` — requirement map, blob table, containment, and both exits.
2. `FINDINGS.md` — the flag table, the assertion-only path deviation, and the stale-doc lines.
3. `commands.md` — launch lines, exits, and the cleanup that follows the evidence commit.
4. `diff/parent.diff` — the ten-file diff against parent `b72d2f8b`.
5. `diff/assertion-boundary.diff` — `lib.rs` lines 111–132 and `types/tests.rs` lines 121–148 only.
6. `linux/smoke/console.log` — archive of `bedd6a2c`. Lib `63 passed`, clippy exit 0, elapsed 41s.
7. `linux/probe/complete-outcome.txt` — fresh composition. It ends `complete_live_ok`. Generation 1 schema hash `b76a05b20baf5847f2acd640290d33b6765c8d4ce2099e1d0cec0f328b3616db`. Digest `b2b301ead04b1cc970f7bdc4b45ffc8f4fd6285ca1440472f83e8908afb2006c` includes this fixture's host, database, and username.
8. `linux/probe/console.log` — `pg_reader_complete_live` 1 passed in 2.76s, `docker-run.exit` 0.
9. `linux/probe/postgresql.log` — 251888 bytes. Secret-file scan reported zero hits. Do not treat the log as a second oracle.

a149 evidence to reuse, not rerun: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a149/SP03/reader-completion-review/`. Snapshot, stream, session, SQL, and the connector manifest are the same blobs that review executed. `Cargo.lock` at this commit is `ef30377cade0ad07ba4192125f434b01452a5622`, equal to the parent and different from a149's `15ae1071c7236a956547e3f1d37d627b2cbebfce`. This commit did not change the lock.

## API to hold

`connector_capabilities` returns `scan` and `bounded_read` supported, with the reason `PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission`. `cdc`, `type_conversion`, `registry_api`, `source_ddl`, and `source_write` stay unsupported. `type_capabilities` `bounded_reader` uses the same reason. `TypeSupport.converted` stays false.

`SnapshotOwner::finish` returns `SnapshotFinished`. Generation 3 in the outcome returned it with `admission=0`.

`SnapshotOwner::finish_complete` returns `RunComplete` only when this owner delivered `StreamItem::Exhausted` for every selected table and the existing release checks passed. The public capability list is the advertisement. It does not add a second reader implementation.

## Path boundary the coordinator already accepted

`lib.rs` and `types/tests.rs` were not on the a151 write card. Their diffs are assertion updates so `--lib` matches the new flags. Leave that fact in the record. It is not a reason to reopen the advertisement.

## Documentation follow-up

Align the nine stale lines listed in `FINDINGS.md`. The important ones are `crates/oo-connectors/README.md` lines 11 and 17, `contracts/source/pgsql/README.md` lines 3 and 47, `reader-keys-v1.md` lines 3 and 37, `type-conversion-v1.md` lines 3 and 53, and `snapshot-owner-v1.md` line 3. Do not treat those sentences as a failed live run.

## Cleanup

After the evidence commit, remove only `oo-a153-probe`, `oo-a153-pg`, the four `oo-a153-eef831aa-*` volumes, network `oo-a153-eef831aa-net`, and `/tmp/oo-a153-eef831aa-dba4-4af2-a78b-541bc9e0a153`. `oo-a153-smoke` was removed before the probe so the next owned builder could take a single slot. Its logs remain under `linux/smoke/`. Commands are in `commands.md`. Do not prune. Do not remove `oo-a152-*` or any other task's containers.

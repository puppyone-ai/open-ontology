# FINDINGS — current catalog guard

Session `eef831aa-dba4-4af2-a78b-541bc9e0a125`. Commit `2254eab39c14a1902e65e72eb3674debc1c54f03`.

## F1. `snapshot_live` fresh open hits `max_rows_per_sec`

Container `eef831aa-dba4-4af2-a78b-541bc9e0a125-rust-live-1` exited 101. `live_start=2026-10-03T10:22:34Z`, `live_end=2026-10-03T10:22:51Z`. The panic is `fresh open: SourceError { kind: Budget, fact: Some("max_rows_per_sec"), sqlstate: None }` at `crates/oo-connectors/tests/snapshot_live.rs:654`.

`same_cut` had already completed on source `snap_src` with `max_rows=1000`. The next full capture in that same 1 second window was refused before its SQL. The server log for that second held 125 `oo-pg-source` statements and the next second held 113. `schema_and_privilege` did not run. Secret hits were 0. The log copy has no leading NUL.

The 50000-row constant was not raised. The test was not given a sleep. `catalog.rs` was not edited. This is the gap in `catalog-rate-proposal.md`: an unarmed `fetch_all` charges `min(max_rows, 50000)` before the round trip, and several captures of 1000-row charges share one window. That charge is not a server-side `LIMIT`. A later card that can edit `catalog.rs` is the path that binds each statement's limit and charges that same integer before the query.

## F2. A single open with `max_rows=50000` refuses before the keyword query

Oracle attempt 3, source `lotrate`, generation 1. The log after the marker shows both `START TRANSACTION` statements (repeatable read and read committed), the node read `pg_is_in_recovery`, and `ROLLBACK` on both sessions. It does not show `pg_get_keywords` or `transaction_timestamp`. `wait_count` returned to the pre-open backend count before the assertion, so both sockets closed. The error was `budget` / `max_rows_per_sec` with no SQLSTATE. Generation 1 stayed consumed (`generation_reused`). After 1100 ms, generation 2 with `max_rows=1000` opened and the keyword query was logged.

`collect_catalog` starts with `KEYWORDS`, which is an unarmed `fetch_all`. `CAPTURED_AT` is later. Startup, both begins, and the node read had already charged the window, so a 50000-row charge did not fit. The oracle's first assertion expected `transaction_timestamp` and was corrected to this measured boundary. Attempt 2 retains that failure.

## F3. Two simultaneous owners on one source fit this fixture

`prove_slots` opens two owners, four `oo-pg-source` backends, then finishes both. The oracle recorded `slots_recovered` for `lotclean`, `lotkill`, `lotca`, `lotprin`, `lotdb`, `lotnode`, and `lotexp`. None of those opens returned `max_rows_per_sec`. That does not change F1. `snapshot_live` stacks more than two full captures on one source id inside one window.

## F4. Terminating the guard is `57P01` on the probe and `io` on finish

Probe: kind `protocol`, fact `database`, sqlstate `57P01`. Finish: fact `io`. The finish result was not `SnapshotFinished` and the probe fact was not `schema_drift`. `57P01` was not relabeled.

## F5. Committed DDL during an open owner is `schema_drift`

Measured on PostgreSQL 17.10. `ADD COLUMN` on the other selected table, on the probed table before the probe, after a successful probe, after the last probe, and on a zero-probe finish all returned `schema_drift`. The returned probe batch stayed on the old cut. Column drop, rename, type, typmod, `SET NOT NULL`, primary-key replacement (`1:id` to `10:a125_pk`), and relation recreate (oid 16464 to 16493) did the same. An index rename from `batch_pkey` to `batch_pk_a125` with the same key finished successfully. Attnums were read from the server: 4, 5, 6, 7, 8, 9, and 10. They were not taken from the a121 run. DDL elapsed times were 0 or 1 ms, under the 5000 ms bound. No `55P03` was observed on these commits.

## F6. Cap refusal does not consume the generation

With three public connections held, open of generation 41 returned `max_connections` and the backend pid set was unchanged. After one holder dropped, generation 41 opened. Generation 42 was refused while four permits were held, then opened after the owner finished. Both backends were gone within the 5000 ms observation bound on the finish, abort, and drop paths.

## F7. a121 stays red

a121 attempt 6 still records `SnapshotFinished` after a committed `ADD COLUMN`. This session did not rewrite that evidence. The current oracle is the counter-measurement for this implementation.

## F8. No additional product path was required

The guard, the ledger, and the savepoint pair are in the six committed files. The catalog `LIMIT` extension remains a proposal because `catalog.rs` is outside this card's writable set.

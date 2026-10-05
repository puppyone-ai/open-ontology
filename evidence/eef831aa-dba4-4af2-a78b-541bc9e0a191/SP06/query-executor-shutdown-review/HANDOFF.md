# Handoff — a191

## Result

Independent review of the fixed dedicated-session shutdown. Verdict **APPROVE LIMITED**.

Source `15d23fe9f2cbc74e3f3278d70d0f56390568126e`. Parent `c77013d5c67830ff4a504eba096c84f645d41727`. Tree `feec821ab54f0aaf0001218f90150befce2803b2`. Baseline `8a289f0c699abb8c707ca45ad45cb036abc3fc05`. Author evidence `c3ac02007d279a4c149bfe70e1486de103cb4c08`. Prior review evidence `df6067e9c6098f874b857d53fec139b78d6107c8`.

Reachable PostgreSQL 17.10: lock-blocked drop 10 ms and `pg_sleep` drop 9 ms after 5200 ms still active, both `partial`, SQLSTATE absent, rollback confirmed, end audit recorded, backend gone by pid and `backend_start`. Server `57014`, user cancel `57014`, client deadline with SQLSTATE absent, and end-hook failure stay four different results. Role bind, guard refusal, admit refusal, same-role reuse, and flag-role `SessionClosed` match the contract. `UNTRUSTED_ADMISSION` remains `Blocked`.

Limit: `cancel_query` is awaited before the 750 ms poll. A stalled cancel TCP left `Drop` blocked and the backend active at 1502 ms. The producer still held the client (`SessionBusy` for a second call). A stalled `CancelHandle` did not abort the next generation inside the 1500 ms snapshot. Details are in `FINDINGS.md` section 4.

## Do next

Leave SP06-W04, SP06-W05, and SP06-W06 open. Leave the public admission `Blocked`. Do not add an HTTP route, a pool, or an untrusted statement entry from this review.

Read `live/oracle.txt` before changing `send_bound_cancel` or `CancelHandle::cancel`. Putting a deadline on `cancel_query` and then dropping the socket is a contract edit, not a silent fix. The reachable-server drops already meet the 1 s bar.

The a184 typed-AST campaign stays sufficient for those rows. The fixture functions are volatile and are not an R4 whitelist. The autocommit audit table is not a production WAL.

## Review

Grok 4.7 in this session was the only coder, tester, and reviewer. No subagent and no worktree. One Rust 1.94 builder at a time, one PostgreSQL 17.10. Pre-overlay smoke `linux/smoke/smoke.exit` is 0. Deciding live phase `linux/proof/phases/live.exit` is 0. Raw `linux/proof/proof.exit` is 1 from the scanner matching its own source (`leak_hits=0`). That miss is `linux/proof/leakscan-self-hit.txt`. Failed smoke and proof attempts remain under `linux/smoke/attempt-*` and `linux/proof/attempt-*`.

This review does not approve a release and does not mark a parent item done.

## State left behind

`cleanup.txt` records removal of `oo-a191-smoke`, `oo-a191-proof`, `oo-a191-pg`, volumes `oo-a191-cargo`, `oo-a191-target`, and `oo-a191-pgdata`, network `oo-a191-net`, and `/tmp/oo-a191-eef831aa-dba4-4af2-a78b-541bc9e0a191`. Pinned images stay. `docker prune` was not used. Other tasks' containers stay.

`SHA256SUMS` lists the other files in this directory. `SHA256SUMS.sha256` is the digest of that list. Both were written by the Rust `sha256` tool. The final marker scan is `linux/proof/leak-scan-final.txt`, from the already-built `leakscan` binary, with the secret files passed as needles and with no secret bytes printed.

# Findings

Author reading of `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1` against baseline `15d23fe9f2cbc74e3f3278d70d0f56390568126e`. This is not an independent review.

## 1. Baseline defect, reproduced before the edit

a191 (`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a191/SP06/query-executor-shutdown-review/FINDINGS.md`, section 4) already named the gap: `cancel_query` was awaited with no deadline, and the 750 ms poll started only after that await returned. A stalled cancel TCP therefore held the producer past 1500 ms. The same section also showed a finished generation lending the session while a `CancelHandle` packet was still in flight, so the next statement could be active on the same pid when the packet later completed.

`linux/red` reran that proxy on the untouched baseline, before these source edits. The harness is `linux/red/stall_append.rs`. It is not part of the source commit.

- `red_drop=blocked:true ms:1501 active:true accepts:1`. At 1500 ms the user backend was still active and the proxy had accepted the cancel TCP.
- `red_race=held:true next_started:true next_active:true cancel:sent`. After the short query finished, the next `page_hold` execute was accepted and became active while the cancel TCP was held. Releasing the proxy delivered `Sent`.

`linux/red/attempt-1` is the earlier harness deadlock (exit 137). Its partial oracle already had the drop fact at 1503 ms. The corrected harness drains the finished stream and waits for idle before drop, then releases the proxy before dropping the next stream. That rerun is `linux/red` exit 0.

## 2. One deadline

`begin_cleanup_deadline` (line 2086) stores the deadline on the session the first time any cleanup action needs it. `budget_left` (line 2072) is the pure remainder. `cleanup_remaining` calls it with `Instant::now()`. The unit test `cleanup_budget_does_not_grow_after_it_starts` injects the clock: 200 ms later leaves 550 ms, 500 ms later leaves 250 ms, and both 750 ms and 900 ms leave zero.

`await_hot` (line 1279) uses that shared deadline for `send_bound_cancel` (line 1399). If the send expires, a shutdown path returns `Abandoned` without starting a fresh 750 ms poll of the pinned future. A client deadline still returns `Hot::Deadline` when the transport expires, so the summary stays `failed` / `Timeout` / SQLSTATE absent and rollback is not claimed. A ready future is still preferred over the sleep, so a reachable server answer is consumed and `ROLLBACK` uses only the time left.

`CancelHandle::cancel` applies the same deadline to observe and then to `cancel_query`. The timeout wrapper is constructed without an await between the gate and the call. Dropping that future is what ends a stalled TCP. There is no second 750 ms budget after it expires.

The first proof (`linux/proof/attempt-1`, exit 101) compiled the library and passed the live campaign, then clippy `-D warnings` stopped on `budget_left` because only the `cfg(test)` unit test called it. `cleanup_remaining` now calls it, so the non-test library uses the same function. That attempt's oracle is preserved and is not the committed byte string. The committed bytes are the green `linux/proof` overlay.

## 3. Generation fence

`try_reserve_cancel` (line 2109) requires `open`, `accepting_cancel`, a matching generation, and an empty reservation. It sets `cancel_reserved` and, if no deadline exists yet, the one 750 ms deadline, before any await. `mark_cancel_packet` (line 2134) records that the request was written, clears the reservation, and wakes waiters. `release_reservation` clears a reservation that did not send.

`settle_lend` (line 2188) waits until the reservation is clear or the shared remainder is zero. `may_lend` (line 2082) is false when the session is closed, a reservation remains, or a packet was written. The unit test covers `(open, reserved, packet) = (true, true, false)`, `(true, false, true)`, `(false, false, false)`, and the one lendable triple `(true, false, false)`.

On the green run, `stalled_handle=ms:753 busy:yes status:succeeded closed:true rollback:yes`. While the proxy held the cancel TCP, the next `page_hold` execute returned `SessionBusy`. The handle then returned `Timeout` with SQLSTATE absent, inside 700 ms to 1000 ms. After the producer finished, the summary was `succeeded` with rollback confirmed, the socket was dropped, `is_open` was false, and the next execute was `SessionClosed`. The connection was not lent to another generation.

## 4. Fail closed, and what "gone" means

`stalled_drop=ms:759 status:partial rollback:no backend:present slo:miss accepts:1`. Drop of an in-flight `page_hold` returned inside the same window. Status `partial`, stop `Protocol`, SQLSTATE absent, rollback not confirmed, socket dropped, session not open, one proxy accept. The following `pg_stat_activity` read still saw the same backend start, so the fact says `backend:present`. The test does not require the row to be gone. Expiry is recorded as an SLO miss.

`whole_page_budget=sqlstate:none elapsed_ms:3050 rollback:no` is the client `Instant` firing across portal pages. `fixture.pace_row` sleeps 1.2 s on the first `i4` and 30 s on the next. The wall is 3000 ms. The second page returned `Timeout` with SQLSTATE absent, rollback was not claimed, and `backend_gone` was true on a real read. That path is separate from `client_budget=57014 elapsed_ms:1786 late:not_sent`, which is still the server timer.

`fail_close_session` does not write a "backend gone" field. Absence stays a later observation.

## 5. Paths that stayed

The green campaign is the existing live test plus the three new proofs. These facts were produced by the same process:

- Lock drop 7 ms and sleep drop 6 ms, both `partial`, rollback confirmed, socket dropped, end audit ok, both well under 1 s. A reachable server still gets its rollback inside the shared remainder.
- Server deadline `57014` at 1838 ms.
- In-flight user cancel `sent 57014`.
- Late cancel `not_sent`, then a live handle `sent`.
- End-hook failure `drop_stream=partial end_audit_ok:false`.
- Finished-query drop `finished_drop_next=still_active drop_ms:7`.
- `untrusted_admission=blocked`, `live_result=compared`, and the role, type, guard, and audit facts in the same oracle.

Role bind is the same decision table. `role_bind_is_permanent_for_a_valid_role_only` passed with the two new unit tests. The smoke on the baseline passed 15 lib tests; the green proof passed 17.

## 6. Left open

The contract's open list is unchanged in substance. Untrusted admission stays blocked because the parser worker's kernel sandbox is unsupported and the authentication gate is absent. Authentication, HTTP, the CLI, a production audit WAL, row filters, a pool, and parent SP06-W04, SP06-W05, and SP06-W06 stay open. This reading does not accept the parent items or a release.

a191 said that dropping `cancel_query` at 750 ms would be a contract change and did not request it. This commit is that change. The contract section "Cancel, failure, reuse" now states the one deadline, the reservation, the fail-closed session, and the difference between a reachable rollback and an unreachable transport. An independent review still has to read the commit against that text.

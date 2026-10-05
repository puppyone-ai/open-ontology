# Handoff — a184

## Result

Verdict: **APPROVE LIMITED** `8a289f0c699abb8c707ca45ad45cb036abc3fc05`.

`DedicatedSession::execute_trusted_select` runs `guard_select` on the caller's original SQL and then the mapped physical `SELECT` on one TLS session. The Linux 1.94 pre-overlay smoke exited 0 (library `query_executor` 5, clippy). The fresh oracle exited 0 (1 test, scan clean, clippy). PostgreSQL 17.10 over TLS, fixture `depot.pallet`, compared int2/int4/int8 extrema, text, bool, and null with the database. OID 1700 and OID 1043 were refused with call count 0. The client deadline delivered one row at 4543 ms and no SQLSTATE. The server timeout kept SQLSTATE 57014 at 1590 ms. Late cancel was `not_sent`. In-flight cancel was `sent`.

Read `FINDINGS.md` before treating a guard refusal as `RoleReuse`, or a catalog entry as docs rule R4. The 500 ms parser worker is not implemented. `UNTRUSTED_ADMISSION` stays `Blocked`.

## Do next

Leave the limited acceptance in place. Do not close SP06-W04, SP06-W05, or SP06-W06. Do not add an HTTP route, a pool, or an untrusted statement entry in order to publish this function.

`read_pages` lines 1182–1196 are the drop arm. A drop while `Execute` waited on `ACCESS EXCLUSIVE` did not take that arm. The deciding oracle records status `failed`, SQLSTATE `57014`, elapsed 2713 ms, on a 3000 ms wall. The socket was closed and the status was not `succeeded`. A later change can make that arm preempt the blocked portal. This review did not edit `query_executor.rs`.

Contract line 19 says the second role returns `RoleReuse` before the guard. Lines 614–617 do that only after a successful bind has stored `bound_role` (lines 952–953). After a guard refusal the oracle recorded `after_guard_refusal_other_role=allowed`. After a successful bind it recorded `role_reuse_audit_delta=0`.

## Review

Grok 4.7 in this session reviewed the fixed commit, wrote the harness, and ran the one builder and the one database. The author session was a different evidence commit (`ce040593`). No release is claimed. No parent checkbox was changed.

## State left behind

Own containers, volumes, network, and `/tmp/oo-a184-eef831aa-dba4-4af2-a78b-541bc9e0a184` are removed (`cleanup.txt`). Failed harness attempts remain under `linux/smoke/attempt-1/` and `linux/probe/attempt-1/` through `attempt-7/`. Other tasks' containers were not removed. `docker prune` was not used.

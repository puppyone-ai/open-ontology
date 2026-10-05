# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a206`. Source `a661dcdf72ff91561ae6b7cb79916e582884d08a`. Proof tree `5e3ba29f8d460a1aa6919733100c34a68a2fdb75`.

## The guard refusal leaves the role bound

`DedicatedSession::execute_trusted_select` stores a syntactically valid `oo_r_` role that matches `identity.role` before `guard_select` and before audit admission. An invalid role does not bind. A later different valid role on that session is `RoleReuse`. Guard refusal does not call the audit hook.

The pre-edit test still had this line after every refusal in `deny_hidden_write_and_injection`:

```rust
assert_eq!(session.bound_role(), None);
```

`OO_A190_LIVE=1` on the fixed source panicked there. Cargo exit 101. The assertion text was `left: Some("oo_r_clerk")` and `right: None`, at `policy_catalog_pg_live.rs:575`. The fixture had already loaded both catalogs and returned the auditor's three amount rows. `live/red-oracle.txt` is that partial record.

The corrected test, on a fresh session, requires an empty role before any statement. After `pg_sleep(0)`, `count(amount)`, `amount + 1`, `INSERT`, and `SELECT secret`, it requires `bound_role()` to be `oo_r_clerk`, the session to stay open, and the admit and complete counters to stay at their pre-loop values. An auditor `SELECT amount` on that same session is `ExecError::RoleReuse`, and the binding stays `oo_r_clerk`. A second session starts unbound, refuses auditor `SELECT qty` without putting `qty` in the display text, binds `oo_r_auditor`, then returns amounts -4, 10, and 20. That read adds one admit and one complete. The first session is still bound to the clerk. Oracle keys: `guard_bound_role=clerk`, `guard_audit_delta=0`, `role_reuse=refused`, `fresh_other_role=auditor`, `fresh_read_audit=1`.

The production bind in `query_executor.rs` was not edited. The blob tested is the blob in the source commit: `90eaf126aefd877111f773d4b2a9012ddab9103b`.

## Generation is per source and digest

The installer and the loader both use:

```sql
SELECT max(run_generation) FROM _oo.physical_mappings
WHERE source = $1 AND config_digest = $2
```

`policy.rs` around the `current_generation` query, and `policy_catalog.rs` in `ensure_generation`. The field comment used to say the maximum stored for the project, which reads as one latest generation for the whole project. It now says the maximum for this source and config digest, and that another digest's later generation does not raise it. The contract's generation check says the same.

`StaleGeneration`'s own field comment still says "for the project". That comment was left in place. The SQL and the field comment above are the ones the live loader uses.

## The null xid check is not an idle proof

`ensure_committed` still only reads `pg_current_xact_id_if_assigned() IS NULL`. A read-only repeatable-read transaction that has not assigned a transaction id also returns null while `xact_start` is set. The contract now says that. The function was not rewritten, and this run did not add an `xact_start` probe to the loader.

The green fixture still reached `done=ok`, including the loader windows whose scan deltas are 0. That is the existing test's idle observation of the admin client after it returns. It does not make the xid predicate stronger.

## The default run is still a gate

Without `OO_A190_LIVE=1` the test prints that the comparison was not run and returns. Smoke and the post-edit run both recorded that line. Cargo reports `ok` for that return. `linux/smoke/gated-note.txt` and `linux/green/gated-note.txt` say that result is not a live pass. The red oracle remains the failed pre-edit run. The green oracle is the run with the variable set.

## Limits

Clippy covered `oo-gateway` lib and `policy_catalog_pg_live` only. The other gateway integration tests were not run. `policy.rs` was not passed through `rustfmt` because the current rustfmt also rewrites older lines in that file. Attempt 1 kept that diff.

Authentication, the audit WAL, row-level security, the pool, and a non-superuser grantor stay open. The audit counter is the in-memory hook. Direct `42501` checks are the login's own statements, not a new authorization campaign. SP06-W01 and SP06-W02 stay open. No release is claimed.

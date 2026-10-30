# C5 overlay visibility v1 integration contract

- Install `0068_native_c5_overlay_visibility.sql` once, as `oo_target_owner`,
  after 0066/0067. Installer registration belongs to the main agent.
- Existing finish and validity signatures remain unchanged. Finish calls the
  original implementation and wraps its exact UNION query, retaining the OID,
  columns, ownership, receipt lineage and immutable private heaps.
- Only the creating transaction (open run, bound generation, prepared_xid)
  sets `visibility_gate` and updates the registered view definition/hash.
  Legacy ungated views remain unpublishable; there is no retrofit bypass.
- The security-barrier predicate is
  `_oo.c5_overlay_visible(physical_name::text, CURRENT_USER)`.
  The SECURITY DEFINER helper pins its search path and dynamically reads the
  coordinator fence. Missing fence / non-open / other role means false.
  Only the exact owner/control roles have trusted internal reads.
- Each registered view has an owner-owned `pv_<physical suffix>()` boolean-only
  ACL latch (`SELECT false`). Open grants EXECUTE; close/stage revoke it.
  The gate additionally checks its current catalog ACL, so a repeatable-read
  snapshot established during close cannot reuse an old open fence row after
  columns are staged again. Grants and fence state still commit atomically.
- Public EXECUTE is limited to boolean visibility/validation; no reader gets
  fence, registry, delta, exclusion or root privileges. Existing row-count
  callback retains its metadata-only behavior.
- Coordinator installation adds `oo_internal_read`, SELECT-only and
  `TO oo_target_owner`, on native root/component heaps. FORCE RLS remains
  enabled; neither owner nor readers acquire BYPASSRLS.
- View admission requires the gated registry proof, matching definition/hash,
  OID/owner, barrier options, helper dependency and complete original receipts.
  Native resolution additionally binds source/config/generation/schema/table.
- Closed and staged states keep the view predicate false; open reads the same
  transaction's fence state. Column ACLs and permanent removal history remain
  authoritative. Observations query the actual predicate, never fake pg_policy.
- Close terminates reader logins including SET ROLE members before locking.
  Prepared readers refuse close rather than receive a false success receipt.
  View transitions take ACCESS EXCLUSIVE locks, matching heap policy DDL's
  reader exclusion; a prepared open blocks reads until COMMIT PREPARED.
  Repeatable-read visibility proof still uses the reader's snapshot and SELECT.
- The native library failure callback is replaced additively in 0068: it closes
  validated views via the ACL latch rather than unsupported heap RLS DDL, and
  retains opened columns/removal history. Its synthetic receipt fixture tests
  the callback, not full library publication acceptance.

Verification:

```
python3 product/tools/release-coordinator/tests/c5_overlay_visibility_fixture.py
python3 product/tools/release-coordinator/tests/c5_overlay_visibility_fixture.py --docker
```

Default creates/stops a new PG17 Unix-socket cluster with prepared transactions
enabled and executes the Rust coordinator bodies. Docker mode creates only
`visibility_fixture` in `oo-c5-incremental-20261025-fixture-pg`, via docker exec
Unix sockets; it refuses to reuse an existing database. Logs/data are retained.
Neither path accepts a DSN, customer endpoint or EIP.

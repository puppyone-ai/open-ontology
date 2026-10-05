# Findings

Fixed tree `a661dcdf72ff91561ae6b7cb79916e582884d08a`. Executor blob `90eaf126aefd877111f773d4b2a9012ddab9103b`, file SHA-256 `2ad166e91a407f5bff39d814d7beb4a1bc69710e30851260528184f3e8558925`. CLI blob `b91238462eb5097de0059ec75e73776ccb2488d0`, file SHA-256 `431424a068a7184e71e9aa18ebf07240a03370e85e3626af2476946c3e196d11`.

## 1. Executor reservation and pre-guard bind stay

`query_executor.rs` lines 639–644: when `cancel_reserved` is set, `execute` returns `ExecError::SessionBusy` before the statement runs. Lines 659–666: `RoleBind::Proceed` stores `request.role` in `guard.bound_role` when it is empty, and that store happens before `guard_select` at line 666. Smoke `linux/smoke/prebind.txt` is `661:                        guard.bound_role = Some(request.role.to_string());`.

a205 already recorded that aborting a reserved `CancelHandle::cancel` leaves `cancel_reserved` and the next execute stays `SessionBusy`. This review did not rerun that matrix and does not change that verdict. The CLI has no cancel handle, so a green CLI run is not an executor approval.

## 2. Compiled CLI on a fresh private target

`OO_A208_LIVE=1` ran `review_query_pg_on_a_fresh_fixture` in 11.48s, exit 0. The binary is the archive `query-pg`, not a library substitute. Database `oo_a208`, host `oo-a208-pg`, TLS CA from the task secret directory, login `oo_a208_login`.

Keeper rows match the admin typed read: three rows, int16/int32/int64 extremes as JSON strings, combining-character caption, bools, and no token. `SELECT *` keeps those cells and changes `executed_sha256` away from `original_sha256`. Reader projections and a two-turn stdin session share one backend pid. Truncation returns one row and 55 bytes. Business digest and insert/update/delete counts are unchanged after the run (`27fe2ad79d54cb2d9c8f37b73f7dfb183cedc340cbb201ad9483a90a27d8cc3e`, `3,0,0`).

Stdout, stderr, and audit files were scanned for the admin password, the login password, `env-not-used-a208`, the three token values, `pt_`, `pc_`, and `postgres://`. The scan facts are `clean`.

## 3. Exits and trusted limits

Asserted on the compiled binary, with stderr compared to the exact bytes where noted:

| Case | Exit | Stderr check |
| --- | --- | --- |
| Success paths, including truncation and the locked reader query | 0 | empty |
| Bare argv | 2 | `query-pg: usage\n` |
| Password text inside the request, `max_operators` 53, ninth stdin turn | 3 | request token on the operators case; session count unchanged for the password case |
| Garbage CA, wrong CA | 4 | `query-pg: tls\n` on the wrong CA |
| Row filter, role mismatch | 5 | `query-pg: policy\n` on the role mismatch; session count unchanged for the row filter |
| Logical column `token`, write, `pg_sleep` | 6 | no audit file |
| Existing audit file, audit parent is not a directory | 7 | `query-pg: audit\n` on the parent; session count unchanged for the existing file |
| Keeper score after `REVOKE SELECT` on that physical column | 8 | quiet-scanned; bytes not stored |

`Fail::exit` (`query-pg.rs` lines 777–790) maps `Fail::Query` to code 8 and `query-pg: query`. That is the only arm that returns 8.

Password bytes in the request are refused before connect. `PGPASSWORD` is removed unless the star case sets it, and that value does not appear in stdout.

## 4. Audit file and the lock sample

`audit.admit` is line 685. The producer task that runs the user SQL is spawned at line 702. Both sit after the pre-guard role store.

The reader statement `SELECT caption FROM studio.plate` finished exit 0 while this review held `ACCESS EXCLUSIVE` on the shadow table. Samples over 8 seconds saw the admit line, never a completion line together with `wait_event_type = Lock`, and a final `pg_stat_activity` row `none/none/active/1` for `application_name = oo-gateway-policy`. After unlock, the audit file's first record is `admit` and the second is `complete`, summary `succeeded`, `end_audit_ok` true. Oracle keys: `audit_before_query=file_order_only`, `audit_completion=after_unlock`, `audit_gap=0`, `audit_lock_seen=0`, `audit_admit_seen=1`.

The later keeper statement did sample the gap. `revoke_wait=1` means admit was present, completion was absent, and a Lock wait was visible. Revoking `SELECT` on the physical score column in that transaction, then committing, produced exit 8, summary `failed`, summary sqlstate `42501`, and the same sqlstate on the last audit line.

A direct `SET ROLE oo_r_reader` and `SELECT` of the physical token column returns `42501` as well. The logical name `token` never reaches that path: it is exit 6 and the audit file is absent.

The missed reader Lock sample is an observation limit of this oracle. It is not charged as a product defect. File order plus the source order above is the audit-before-query evidence for the reader statement. The keeper statement is the evidence that a waiting user query can still fail `42501` after admit.

## 5. a206 alignment, including digest scope

`OO_A190_LIVE=1` passed in 0.77s on the decisive proof. `live/oracle.txt` records `guard_bound_role=clerk`, `guard_audit_delta=0`, `role_reuse=refused`, `fresh_other_role=auditor`, `fresh_read_audit=1`, `done=ok`. Unset `OO_A190_LIVE` remains a gate; the passing live run is the alignment evidence. This is not a new acceptance of the catalog component.

On the fresh fixture, generation 6 of digest `a208…` loads. Generation 21 of that digest is `CatalogError::StaleGeneration`. Generation 6 of digest `b208…` is `StaleGeneration` even though 21 exists for `b208…`. Generation 21 of `b208…` has no shadow table and returns `MappingAbsent`, which is not `StaleGeneration`. That matches `ensure_generation` (`policy_catalog.rs` lines 447–461) and `current_generation` (`policy.rs` lines 598–603): `WHERE source = $1 AND config_digest = $2`.

`ColumnAclPolicy.generation` (`policy.rs` lines 91–94) says a later generation on another digest does not raise this maximum. `PolicyError::StaleGeneration.current` (`policy.rs` lines 159–164) still says "Maximum `run_generation` stored for the project, if any." The behavior under test follows the SQL. The field comment is the stale sentence.

## 6. Fixture precondition

`grant_mapping_read` in the review oracle grants the login schema usage, `SELECT` on `_oo.physical_mappings`, and `EXECUTE` on `lookup_physical_mapping`. Catalog load uses those rights. They are not a CLI behavior and they are not a product patch. A login without them is outside this run.

## 7. Harness failures that were kept

Attempts 001–005 exited nonzero and remain under `linux/proof/attempts/`. Causes were review-oracle compile errors (`E0716`, missing `Debug`, `cloned_ref_to_slice_refs`, `E0597`, `E0308`, `E0507`), a review assertion that required admit and a Lock wait in the same sample, and `2BP01` when a previous database still owned `oo_r_*` roles. The a190 source was not edited to drop `oo_a208`. This task dropped its own leftover databases before the decisive run. Product hashes matched the smoke on every proof that reached `product-hash`.
